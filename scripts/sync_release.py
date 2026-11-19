"""Sync the latest stable GitHub release over SSH, then refresh its EdgeOne directory."""

from __future__ import annotations

import argparse
import base64
import hashlib
import io
import json
import os
import re
import shlex
import shutil
import subprocess
import sys
import tarfile
import tempfile
import time
import uuid
from dataclasses import dataclass, field
from pathlib import Path, PurePosixPath
from typing import Any
from urllib.error import HTTPError, URLError
from urllib.parse import quote, urlsplit
from urllib.request import (
    HTTPRedirectHandler,
    OpenerDirector,
    ProxyHandler,
    Request,
    build_opener,
)

from edgeone_cache import EdgeOneClient, SyncError, load_credentials, validate_cache_url

PROJECT_ROOT = Path(__file__).resolve().parent.parent
BINARY_NAMES = (
    "maibot-manager-linux-arm64",
    "maibot-manager-linux-x86_64",
    "maibot-manager-macos-arm64",
    "maibot-manager-windows-x86_64.exe",
)
INSTALLER_NAMES = ("install.sh", "install.ps1")
MAX_BINARY_BYTES = 512 * 1024 * 1024


def load_environment(path: Path) -> dict[str, str]:
    """Read literal KEY=value entries; process environment takes precedence."""
    values = {}
    if path.exists():
        for number, raw in enumerate(path.read_text(encoding="utf-8-sig").splitlines(), 1):
            line = raw.strip()
            if not line or line.startswith("#"):
                continue
            key, separator, value = line.partition("=")
            key, value = key.strip(), value.strip()
            if not separator or not re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", key):
                raise SyncError(f"本机环境文件第 {number} 行不是 KEY=value")
            if value.startswith(("'", '"')):
                if len(value) < 2 or value[-1] != value[0]:
                    raise SyncError(f"本机环境文件第 {number} 行引号未闭合")
                value = value[1:-1]
            else:
                value = value.split(" #", 1)[0].rstrip()
            if key in values:
                raise SyncError(f"本机环境文件第 {number} 行重复定义 {key}")
            values[key] = value
    return values | dict(os.environ)


@dataclass(frozen=True)
class Settings:
    ssh_target: str = field(repr=False)
    remote_directory: str = field(repr=False)
    cache_url: str = field(repr=False)
    zone_id: str = field(repr=False)
    repository: str = "WhiteCloudOL/MaiBot-Manager-TUI"
    ssh_config: Path | None = field(default=None, repr=False)
    proxy: str = field(default="", repr=False)
    github_token: str = field(default="", repr=False)
    command_timeout: int = 300
    purge_wait: int = 180

    @classmethod
    def load(cls, environment: dict[str, str], base: Path) -> Settings:
        def required(key: str) -> str:
            value = environment.get(key, "").strip()
            if not value:
                raise SyncError(f"缺少 {key}；请填写本机 .env.deploy 或设置环境变量")
            return value

        target = required("MMTUI_SSH_TARGET")
        if target.startswith("-") or re.search(r"[\s:/\x00-\x1f]", target):
            raise SyncError("MMTUI_SSH_TARGET 必须是有效的 SSH 别名或 user@host")
        remote = required("MMTUI_REMOTE_DIR").rstrip("/")
        remote_path = PurePosixPath(remote)
        if (
            not remote_path.is_absolute()
            or remote == "/"
            or ".." in remote_path.parts
            or str(remote_path) != remote
        ):
            raise SyncError("MMTUI_REMOTE_DIR 必须是规范的非根目录 Linux 绝对路径")
        cache = validate_cache_url(required("MMTUI_CACHE_URL"))
        zone = required("EDGEONE_ZONE_ID")
        if not re.fullmatch(r"zone-[a-z0-9]+", zone):
            raise SyncError("EDGEONE_ZONE_ID 格式不正确，不能使用 *")
        repository = environment.get("MMTUI_RELEASE_REPO", cls.repository).strip()
        if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repository):
            raise SyncError("MMTUI_RELEASE_REPO 必须是 owner/repository")
        proxy = environment.get("MMTUI_HTTP_PROXY", "").strip()
        if proxy and (
            urlsplit(proxy).scheme not in {"http", "https"} or not urlsplit(proxy).hostname
        ):
            raise SyncError("MMTUI_HTTP_PROXY 必须是 HTTP(S) 代理地址")
        config = environment.get("MMTUI_SSH_CONFIG_FILE", "").strip()
        ssh_config = Path(config).expanduser() if config else None
        if ssh_config is not None:
            if not ssh_config.is_absolute():
                ssh_config = base / ssh_config
            if not ssh_config.is_file():
                raise SyncError("MMTUI_SSH_CONFIG_FILE 指定的配置文件不存在")
        timeout = int(environment.get("MMTUI_COMMAND_TIMEOUT", "300"))
        wait = int(environment.get("MMTUI_PURGE_WAIT", "180"))
        if not 30 <= timeout <= 3600 or not 10 <= wait <= 600:
            raise SyncError("命令超时应为 30–3600 秒，缓存等待应为 10–600 秒")
        return cls(
            target,
            remote,
            cache,
            zone,
            repository,
            ssh_config,
            proxy,
            environment.get("GITHUB_TOKEN", ""),
            timeout,
            wait,
        )


class SafeRedirects(HTTPRedirectHandler):
    def redirect_request(
        self, request: Request, fp: Any, code: int, message: str, headers: Any, url: str
    ) -> Request | None:
        redirected = super().redirect_request(request, fp, code, message, headers, url)
        if redirected is not None and urlsplit(request.full_url).netloc != urlsplit(url).netloc:
            redirected.remove_header("Authorization")
        if urlsplit(url).scheme != "https":
            raise SyncError("拒绝将 HTTPS 下载重定向至非 HTTPS 地址")
        return redirected


def make_opener(settings: Settings) -> OpenerDirector:
    proxy = (
        ProxyHandler({"http": settings.proxy, "https": settings.proxy})
        if settings.proxy
        else ProxyHandler()
    )
    return build_opener(proxy, SafeRedirects())


class GitHubRelease:
    def __init__(self, settings: Settings, opener: OpenerDirector) -> None:
        self.settings = settings
        self.opener = opener

    def request(self, url: str) -> Request:
        headers = {"User-Agent": "MaiBot-Manager-Local-Sync/1.0"}
        if urlsplit(url).hostname == "api.github.com":
            headers["Accept"] = "application/vnd.github+json"
            if self.settings.github_token:
                headers["Authorization"] = f"Bearer {self.settings.github_token}"
        return Request(url, headers=headers)

    def api(self, suffix: str) -> dict[str, Any]:
        request = self.request(f"https://api.github.com/repos/{self.settings.repository}/{suffix}")
        try:
            with self.opener.open(request, timeout=30) as response:
                data = response.read(8 * 1024 * 1024 + 1)
            if len(data) > 8 * 1024 * 1024:
                raise SyncError("GitHub API 响应超过大小限制")
            result = json.loads(data)
        except HTTPError as error:
            raise SyncError(f"GitHub API HTTP {error.code}，请检查仓库、令牌或 API 限流") from error
        except (URLError, TimeoutError, OSError) as error:
            raise SyncError("无法连接 GitHub API，请检查网络或 MMTUI_HTTP_PROXY") from error
        if not isinstance(result, dict):
            raise SyncError("GitHub API 响应格式无效")
        return result

    def latest(self) -> dict[str, Any]:
        release = self.api("releases/latest")
        if release.get("draft") or release.get("prerelease") or not release.get("tag_name"):
            raise SyncError("未找到有效的最新正式版 Release")
        assets = {item["name"]: item for item in release.get("assets", [])}
        for name in BINARY_NAMES:
            item = assets.get(name, {})
            if (
                item.get("state") != "uploaded"
                or type(item.get("size")) is not int
                or not 0 < item["size"] <= MAX_BINARY_BYTES
                or not re.fullmatch(r"sha256:[a-f0-9]{64}", item.get("digest") or "")
                or not re.fullmatch(
                    rf"https://api\.github\.com/repos/{re.escape(self.settings.repository)}/releases/assets/[0-9]+",
                    item.get("url", ""),
                )
            ):
                raise SyncError(f"正式版缺少有效安装包或 SHA-256 摘要：{name}")
        return release

    def download(self, item: dict[str, Any], destination: Path) -> None:
        for attempt in range(3):
            try:
                digest = hashlib.sha256()
                size = 0
                request = self.request(item["url"])
                request.add_header("Accept", "application/octet-stream")
                with self.opener.open(request, timeout=60) as response:
                    with destination.open("wb") as stream:
                        for chunk in iter(lambda: response.read(1024 * 1024), b""):
                            size += len(chunk)
                            if size > item["size"]:
                                raise SyncError(f"下载大小超过发布记录：{item['name']}")
                            stream.write(chunk)
                            digest.update(chunk)
                if size != item["size"] or "sha256:" + digest.hexdigest() != item["digest"]:
                    raise SyncError(f"Release SHA-256 / 大小校验失败：{item['name']}")
                return
            except (URLError, TimeoutError, OSError) as error:
                if attempt == 2:
                    detail = (
                        f"HTTP {error.code}"
                        if isinstance(error, HTTPError)
                        else type(error).__name__
                    )
                    raise SyncError(
                        f"安装包下载失败：{item['name']}（{detail}），请检查网络"
                    ) from error
                time.sleep(attempt + 1)

    def package(self, release: dict[str, Any], directory: Path) -> tuple[Path, dict[str, Any]]:
        commit = self.api(f"commits/{quote(release['tag_name'], safe='')}").get("sha", "")
        if not re.fullmatch(r"[a-f0-9]{40}", commit):
            raise SyncError("无法确定正式版对应的 Git 提交")
        assets = {item["name"]: item for item in release["assets"]}
        for name in BINARY_NAMES:
            self.download(assets[name], directory / name)
            print(f"已下载并校验：{name}", flush=True)
        for name in INSTALLER_NAMES:
            blob = self.api(f"contents/scripts/{name}?ref={commit}")
            if blob.get("encoding") != "base64":
                raise SyncError(f"无法读取正式版安装脚本：{name}")
            content = base64.b64decode("".join(blob["content"].split()), validate=True)
            git_digest = hashlib.sha1(f"blob {len(content)}\0".encode() + content).hexdigest()
            if not 0 < len(content) <= 5 * 1024 * 1024 or git_digest != blob.get("sha"):
                raise SyncError(f"安装脚本 Git blob 校验失败：{name}")
            (directory / name).write_bytes(content)
        manifest = {"tag": release["tag_name"], "commit": commit, "files": {}}
        for name in BINARY_NAMES + INSTALLER_NAMES:
            data = (directory / name).read_bytes()
            manifest["files"][name] = {
                "size": len(data),
                "sha256": hashlib.sha256(data).hexdigest(),
            }
        (directory / "manifest.json").write_text(json.dumps(manifest), encoding="utf-8")
        archive = directory / "release.tar.gz"
        with tarfile.open(archive, "w:gz") as package:
            for name in BINARY_NAMES + INSTALLER_NAMES + ("manifest.json",):
                package.add(directory / name, arcname=name, recursive=False)
        return archive, manifest


class RemoteDirectory:
    def __init__(self, settings: Settings) -> None:
        self.settings = settings
        self.helper = Path(__file__).with_name("release_sync_remote.py").read_text(encoding="utf-8")

    def options(self) -> list[str]:
        options = [
            "-o",
            "BatchMode=yes",
            "-o",
            "StrictHostKeyChecking=yes",
            "-o",
            "ConnectTimeout=10",
            "-o",
            "ServerAliveInterval=15",
            "-o",
            "ServerAliveCountMax=3",
        ]
        if self.settings.ssh_config is not None:
            options += ["-F", str(self.settings.ssh_config)]
        return options

    def run(
        self,
        command: list[str],
        label: str,
        *,
        script: str | None = None,
        cwd: Path | None = None,
    ) -> str:
        try:
            result = subprocess.run(
                command,
                input=script,
                cwd=cwd,
                capture_output=True,
                text=True,
                encoding="utf-8",
                errors="replace",
                timeout=self.settings.command_timeout,
                check=False,
            )
        except subprocess.TimeoutExpired as error:
            raise SyncError(f"{label}超时；如果已进入远端替换阶段，请先检查源站文件") from error
        except OSError as error:
            raise SyncError(f"{label}无法启动，请确认 OpenSSH 已安装") from error
        if result.returncode:
            raise SyncError(f"{label}失败：{result.stderr.strip()}")
        return result.stdout

    def execute(self, action: str, *arguments: str) -> dict[str, Any]:
        remote = shlex.join(["python3", "-", action, self.settings.remote_directory, *arguments])
        text = self.run(
            ["ssh", *self.options(), self.settings.ssh_target, remote],
            "远端操作",
            script=self.helper,
        )
        return json.loads(text)

    def check(self) -> None:
        if not shutil.which("ssh") or not shutil.which("scp"):
            raise SyncError("本机需要 OpenSSH 的 ssh 和 scp 命令")
        if self.execute("check") != {"ready": True}:
            raise SyncError("远端目录检查未通过")

    def upload(self, archive: Path, manifest: dict[str, Any]) -> None:
        target = PurePosixPath(self.settings.remote_directory)
        remote_archive = str(target.parent / f".{target.name}.upload-{uuid.uuid4().hex}.tar.gz")
        self.run(
            [
                "scp",
                *self.options(),
                archive.name,
                f"{self.settings.ssh_target}:{remote_archive}",
            ],
            "上传同步包",
            cwd=archive.parent,
        )
        result = self.execute("apply", remote_archive)
        if result.get("tag") != manifest["tag"] or result.get("files") != manifest["files"]:
            raise SyncError("远端同步结果与本机清单不一致，未提交缓存刷新")


def main() -> int:
    for stream in (sys.stdout, sys.stderr):
        if isinstance(stream, io.TextIOWrapper):
            stream.reconfigure(encoding="utf-8", errors="replace")
    parser = argparse.ArgumentParser(
        description="本机同步最新正式版安装包和安装脚本，然后刷新 EdgeOne 目录缓存"
    )
    parser.add_argument("--env-file", type=Path, default=PROJECT_ROOT / ".env.deploy")
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="仅显示同步计划；不上传、不读取密钥、不调用 EdgeOne",
    )
    parser.add_argument(
        "--purge-only",
        action="store_true",
        help="只刷新已配置目录的缓存，用于上传成功后的重试",
    )
    args = parser.parse_args()
    try:
        environment = load_environment(args.env_file)
        settings = Settings.load(environment, args.env_file.resolve().parent)
        opener = make_opener(settings)
        release_client = GitHubRelease(settings, opener)
        release = None if args.purge_only else release_client.latest()
        if release is not None:
            print(f"最新正式版：{release['tag_name']}")
            print("同步文件：" + ", ".join(BINARY_NAMES + INSTALLER_NAMES))
            print(f"SSH 目标：{settings.ssh_target}\n目标目录：{settings.remote_directory}")
        print(
            f"EdgeOne 目录刷新：{settings.cache_url}（purge_prefix / delete）",
            flush=True,
        )
        if args.dry_run:
            print("预览完成，未修改服务器或缓存。")
            return 0
        credentials = load_credentials(environment, args.env_file.resolve().parent)
        client = EdgeOneClient(credentials, opener)
        if release is not None:
            remote = RemoteDirectory(settings)
            remote.check()
            with tempfile.TemporaryDirectory(prefix="maibot-release-sync-") as temporary:
                archive, manifest = release_client.package(release, Path(temporary))
                remote.upload(archive, manifest)
            print("六个文件同步及远端校验完成。", flush=True)
        job_id = client.purge_directory(settings.zone_id, settings.cache_url)
        print(f"已提交目录刷新任务：{job_id}", flush=True)
        client.wait_for_purge(settings.zone_id, job_id, settings.purge_wait)
        print("EdgeOne 目录缓存刷新成功。", flush=True)
        return 0
    except KeyboardInterrupt:
        print("\n操作已中断；如已提交缓存刷新，请在控制台查看原任务。", file=sys.stderr)
        return 130
    except (
        SyncError,
        OSError,
        ValueError,
        KeyError,
        TypeError,
        tarfile.TarError,
    ) as error:
        print(f"同步未完成：{error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
