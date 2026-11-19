"""Remote Linux helper, sent over SSH stdin; no site-specific configuration."""

from __future__ import annotations

import hashlib
import json
import os
import re
import shutil
import stat
import sys
import tarfile
import tempfile
from contextlib import contextmanager
from pathlib import Path
from typing import Iterator

FILES = {
    "install.sh": 0o755,
    "install.ps1": 0o644,
    "maibot-manager-linux-arm64": 0o755,
    "maibot-manager-linux-x86_64": 0o755,
    "maibot-manager-macos-arm64": 0o755,
    "maibot-manager-windows-x86_64.exe": 0o644,
}
MAX_FILE_BYTES = 512 * 1024 * 1024


def target_directory(value: str) -> Path:
    """Reject aliases and symlinks before touching the explicitly named directory."""
    directory = Path(value)
    if not directory.is_absolute() or directory == Path(directory.anchor):
        raise ValueError("远端目录必须是非根目录的绝对路径")
    if directory.resolve(strict=True) != directory or not directory.is_dir():
        raise ValueError("远端目录必须已存在且不能包含符号链接或 ..")
    return directory


@contextmanager
def destination_lock(directory: Path) -> Iterator[None]:
    import fcntl

    with (directory.parent / f".{directory.name}.release-sync.lock").open("a") as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError as error:
            raise RuntimeError("另一个同步任务正在更新该目录，请稍后重试") from error
        yield


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def unpack_verified(archive: Path, stage: Path) -> dict:
    """Extract only the seven fixed regular files; never use extractall."""
    with tarfile.open(archive, "r:gz") as package:
        members = package.getmembers()
        if len(members) != len(FILES) + 1 or {m.name for m in members} != set(FILES) | {
            "manifest.json"
        }:
            raise ValueError("同步包文件列表不符合约定")
        for member in members:
            if not member.isfile() or not 0 < member.size <= MAX_FILE_BYTES:
                raise ValueError("同步包包含非法文件类型或大小")
            source = package.extractfile(member)
            if source is None:
                raise ValueError("同步包中的文件不可读取")
            with source, (stage / member.name).open("xb") as destination:
                shutil.copyfileobj(source, destination)
    manifest = json.loads((stage / "manifest.json").read_text(encoding="utf-8"))
    entries = manifest.get("files", {})
    if set(entries) != set(FILES) or not isinstance(manifest.get("tag"), str):
        raise ValueError("同步包校验清单无效")
    for name, mode in FILES.items():
        path = stage / name
        expected = entries[name]
        if expected.get("size") != path.stat().st_size or expected.get("sha256") != sha256(path):
            raise ValueError(f"远端 SHA-256 / 大小校验失败：{name}")
        path.chmod(mode)
    return manifest


def apply_archive(directory: Path, archive: Path) -> dict:
    """Replace the six files individually and restore every replacement on failure."""
    pattern = rf"\.{re.escape(directory.name)}\.upload-[a-f0-9]{{32}}\.tar\.gz"
    if (
        archive.parent != directory.parent
        or not re.fullmatch(pattern, archive.name)
        or archive.is_symlink()
    ):
        raise ValueError("上传包不位于指定目录旁的受管临时路径")
    stage = Path(tempfile.mkdtemp(prefix=f".{directory.name}.stage-", dir=directory.parent))
    cleanup = True
    try:
        with destination_lock(directory):
            manifest = unpack_verified(archive, stage)
            backups = stage / "previous"
            backups.mkdir()
            for name in FILES:
                target = directory / name
                if (
                    target.is_symlink()
                    or target.exists()
                    and not stat.S_ISREG(target.stat().st_mode)
                ):
                    raise ValueError(f"目标文件不是普通文件：{name}")
                if target.exists():
                    shutil.copy2(target, backups / name)
            replaced = []
            try:
                for name in FILES:
                    os.replace(stage / name, directory / name)
                    replaced.append(name)
                for name, expected in manifest["files"].items():
                    if sha256(directory / name) != expected["sha256"]:
                        raise ValueError(f"替换后校验失败：{name}")
            except (Exception, KeyboardInterrupt):
                failed = []
                for name in reversed(replaced):
                    try:
                        if (backups / name).exists():
                            os.replace(backups / name, directory / name)
                        else:
                            (directory / name).unlink()
                    except OSError:
                        failed.append(name)
                if failed:
                    cleanup = False
                    raise RuntimeError(
                        f"文件恢复未完成，备份保留于 {backups}；请手动恢复：{', '.join(failed)}"
                    )
                raise
            return {"tag": manifest["tag"], "files": manifest["files"]}
    finally:
        if cleanup:
            shutil.rmtree(stage)
        archive.unlink(missing_ok=True)


def main() -> int:
    try:
        action, raw_directory, *rest = sys.argv[1:]
        directory = target_directory(raw_directory)
        if action == "check" and not rest:
            print(json.dumps({"ready": True}))
        elif action == "apply" and len(rest) == 1:
            print(json.dumps(apply_archive(directory, Path(rest[0]))))
        else:
            raise ValueError("无效的远端同步操作")
        return 0
    except (
        OSError,
        ValueError,
        RuntimeError,
        tarfile.TarError,
        KeyError,
        TypeError,
    ) as error:
        print(f"远端同步失败：{error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
