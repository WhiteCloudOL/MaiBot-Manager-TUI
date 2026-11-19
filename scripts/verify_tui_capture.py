"""Capture the TUI in an isolated workspace using a controlling Linux PTY."""

import argparse
import codecs
import fcntl
import json
import os
import pty
import select
import signal
import sqlite3
import struct
import sys
import tempfile
import termios
import time
from pathlib import Path

import pyte
from wcwidth import wcwidth


class BoundedScreen(pyte.Screen):
    def __init__(self, columns: int, lines: int) -> None:
        super().__init__(columns, lines)
        self.overflow = False

    def draw(self, data: str) -> None:
        for character in data:
            if self.cursor.x + max(wcwidth(character), 0) > self.columns:
                self.overflow = True
            super().draw(character)


def capture(mode: str, columns: int, rows: int, binary: Path, destination: Path) -> None:
    with tempfile.TemporaryDirectory(prefix="maibot-tui-") as temporary:
        home = Path(temporary)
        install = home / "install"
        workspace = install / "MaiBot"
        bot = workspace / "bots" / "sample"
        (workspace / "host").mkdir(parents=True)
        (bot / "config").mkdir(parents=True)
        (bot / "config/bot_config.toml").write_text("[plugin]\nenabled = true\n", encoding="utf-8")
        (workspace / "host/config.toml").write_text(
            '[webui]\nhosts = ["127.0.0.1"]\nport = 8123\n', encoding="utf-8"
        )
        (workspace / "host/webui.json").write_text(
            '{"access_token":"test-token"}', encoding="utf-8"
        )
        python = workspace / ".venv/bin/python"
        python.parent.mkdir(parents=True)
        python.symlink_to(sys.executable)
        with sqlite3.connect(workspace / "host/registry.sqlite3") as connection:
            connection.executescript(
                "PRAGMA user_version = 1; CREATE TABLE bots "
                "(bot_id TEXT, name TEXT, directory TEXT, initialization TEXT, created_at TEXT);"
            )
            connection.execute(
                "INSERT INTO bots VALUES ('sample', 'Sample', 'bots/sample', 'initialized', '')"
            )
        (home / ".maibot_config").write_text(
            f'MAI_PATH="{install}"\nMAI_PYTHON_ENV="uv"\nBOT_PROTOCOLS="none"\n',
            encoding="utf-8",
        )
        if mode == "fresh":
            (home / ".maibot_config").unlink()
        child, descriptor = pty.fork()
        if child == 0:
            os.environ.update(HOME=str(home), TERM="xterm-256color", LANG="C.UTF-8")
            fcntl.ioctl(0, termios.TIOCSWINSZ, struct.pack("HHHH", rows, columns, 0, 0))
            os.execv(str(binary), [str(binary), "tui"])
        screen = BoundedScreen(columns, rows)
        stream = pyte.Stream(screen)
        decoder = codecs.getincrementaldecoder("utf-8")("replace")
        timeline = []
        if mode == "tabs":
            timeline = [(0.7 + index * 0.3, b"\x1b[B") for index in range(6)]
        elif mode in {"deploy", "fresh"}:
            timeline = [
                (0.7, b"\x1b[B"),
                (1.0, b"\t"),
                (1.2, b"\x1b[C"),
                (1.4, b"\x1b[B"),
                (1.6, b"\x1b[A"),
                (1.8, b"\r"),
            ]
        elif mode == "access":
            timeline = [(0.6 + index * 0.2, b"\x1b[B") for index in range(5)] + [
                (1.8, b"\t"),
                (2.0, b"\r"),
            ]
        started = time.monotonic()
        raw = bytearray()
        try:
            while time.monotonic() - started < 3.3:
                elapsed = time.monotonic() - started
                while timeline and elapsed >= timeline[0][0]:
                    _, keys = timeline.pop(0)
                    os.write(descriptor, keys)
                if select.select([descriptor], [], [], 0.05)[0]:
                    try:
                        data = os.read(descriptor, 65536)
                    except OSError:
                        break
                    if not data:
                        break
                    raw.extend(data)
                    stream.feed(decoder.decode(data))
            destination.mkdir(parents=True, exist_ok=True)
            (destination / f"{mode}.txt").write_text("\n".join(screen.display), encoding="utf-8")
            (destination / f"{mode}.ansi").write_bytes(raw)
            display = "".join(screen.display)
            if (
                screen.overflow
                or "╭" not in display
                or not any(key in display for key in ("[Tab]", "[F5]", "[Esc]"))
            ):
                raise RuntimeError(f"{mode}: TUI layout or startup failed; inspect {destination}")
            if mode == "fresh" and (home / ".maibot_config").exists():
                raise RuntimeError("Fresh startup created an installation configuration")
            os.write(descriptor, b"\x03")
            deadline = time.monotonic() + 2
            while time.monotonic() < deadline:
                exited, status = os.waitpid(child, os.WNOHANG)
                if exited:
                    child = 0
                    if os.waitstatus_to_exitcode(status) != 0:
                        raise RuntimeError(f"{mode}: Ctrl+C did not exit cleanly")
                    break
                time.sleep(0.05)
            restored = termios.tcgetattr(descriptor)[3]
            terminal_restored = bool(restored & termios.ECHO and restored & termios.ICANON)
            if child or not terminal_restored:
                raise RuntimeError(f"{mode}: terminal was not restored after Ctrl+C")
            print(
                json.dumps(
                    {
                        "mode": mode,
                        "columns": columns,
                        "rows": rows,
                        "overflow": screen.overflow,
                        "rounded_blocks": True,
                        "terminal_restored": terminal_restored,
                    },
                    ensure_ascii=False,
                )
            )
        finally:
            os.close(descriptor)
            if child:
                os.kill(child, signal.SIGTERM)
                os.waitpid(child, 0)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--exe", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    for mode, columns, rows in [
        ("fresh", 100, 34),
        ("wide", 132, 42),
        ("narrow", 72, 28),
        ("tabs", 132, 42),
        ("deploy", 100, 34),
        ("access", 132, 30),
    ]:
        capture(mode, columns, rows, args.exe.resolve(), args.output)


if __name__ == "__main__":
    main()
