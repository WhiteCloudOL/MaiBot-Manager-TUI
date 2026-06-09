#!/usr/bin/env python3
import argparse
import json
import os
import pty
import re
import shlex
import select
import struct
import subprocess
import termios
import time
import unicodedata
from fcntl import ioctl


CSI_RE = re.compile(r"\x1b\[([0-?]*)([ -/]*)([@-~])")


def cell_width(ch: str) -> int:
    if unicodedata.combining(ch):
        return 0
    return 2 if unicodedata.east_asian_width(ch) in ("W", "F") else 1


def visible_width(line: str) -> int:
    return sum(cell_width(ch) for ch in line)


def set_winsize(fd: int, cols: int, rows: int) -> None:
    ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))


def csi_numbers(params: str, defaults: list[int]) -> list[int]:
    params = params.lstrip("?")
    if not params:
        return defaults
    values = []
    for part in params.split(";"):
        if part == "":
            values.append(0)
        else:
            try:
                values.append(int(part))
            except ValueError:
                values.append(0)
    return values or defaults


def render_screen(raw: bytes, cols: int, rows: int) -> list[str]:
    text = raw.decode("utf-8", "ignore")
    screen = [[" "] * cols for _ in range(rows)]
    row = 0
    col = 0
    idx = 0

    def clear_line(target_row: int) -> None:
        if 0 <= target_row < rows:
            screen[target_row] = [" "] * cols

    while idx < len(text):
        ch = text[idx]
        if ch == "\x1b":
            match = CSI_RE.match(text, idx)
            if match:
                params, _, command = match.groups()
                nums = csi_numbers(params, [1])
                if command in ("H", "f"):
                    row = max(0, min(rows - 1, (nums[0] or 1) - 1))
                    col_value = nums[1] if len(nums) > 1 else 1
                    col = max(0, min(cols - 1, (col_value or 1) - 1))
                elif command == "J":
                    mode = nums[0] if nums else 0
                    if mode == 2:
                        screen = [[" "] * cols for _ in range(rows)]
                        row = 0
                        col = 0
                elif command == "K":
                    mode = nums[0] if nums else 0
                    if mode == 2:
                        clear_line(row)
                    elif mode == 0 and 0 <= row < rows:
                        for pos in range(col, cols):
                            screen[row][pos] = " "
                elif command == "A":
                    row = max(0, row - (nums[0] or 1))
                elif command == "B":
                    row = min(rows - 1, row + (nums[0] or 1))
                elif command == "C":
                    col = min(cols - 1, col + (nums[0] or 1))
                elif command == "D":
                    col = max(0, col - (nums[0] or 1))
                idx = match.end()
                continue
            idx += 1
            continue

        if ch == "\r":
            col = 0
        elif ch == "\n":
            row = min(rows - 1, row + 1)
        elif ch == "\b":
            col = max(0, col - 1)
        elif ch >= " ":
            width = max(1, cell_width(ch))
            if 0 <= row < rows and col < cols:
                screen[row][col] = ch
                if width == 2 and col + 1 < cols:
                    screen[row][col + 1] = ""
            col += width
            if col >= cols:
                col = 0
                row = min(rows - 1, row + 1)
        idx += 1

    return ["".join(cell for cell in line if cell != "").rstrip() for line in screen]


def run_capture(exe: str, cwd: str, cols: int, rows: int, timeline: list[tuple[float, bytes]]) -> dict:
    env = os.environ.copy()
    env["TERM"] = "xterm-256color"
    env["LANG"] = "C.UTF-8"
    env["COLUMNS"] = str(cols)
    env["LINES"] = str(rows)
    master, slave = pty.openpty()
    set_winsize(master, cols, rows)
    set_winsize(slave, cols, rows)
    command = f"stty rows {rows} cols {cols}; exec {shlex.quote(exe)} tui"

    def attach_controlling_terminal() -> None:
        os.setsid()
        try:
            ioctl(slave, getattr(termios, "TIOCSCTTY", 0x540E), 0)
        except OSError:
            pass

    proc = subprocess.Popen(
        ["bash", "-lc", command],
        cwd=cwd,
        env=env,
        stdin=slave,
        stdout=slave,
        stderr=slave,
        close_fds=True,
        preexec_fn=attach_controlling_terminal,
    )
    os.close(slave)

    captured = bytearray()
    started = time.time()
    idx = 0
    try:
        while True:
            elapsed = time.time() - started
            while idx < len(timeline) and elapsed >= timeline[idx][0]:
                os.write(master, timeline[idx][1])
                idx += 1

            ready, _, _ = select.select([master], [], [], 0.1)
            if master in ready:
                try:
                    captured.extend(os.read(master, 65536))
                except OSError:
                    break

            if proc.poll() is not None:
                break
            if elapsed > 8:
                proc.terminate()
                break
    finally:
        try:
            proc.wait(timeout=2)
        except Exception:
            proc.kill()
        os.close(master)

    lines = [line for line in render_screen(bytes(captured), cols, rows) if line.strip()]
    widths = [visible_width(line) for line in lines]
    return {
        "line_count": len(lines),
        "max_width": max(widths) if widths else 0,
        "overflow": any(width > cols for width in widths),
        "tail": lines[-24:],
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--cwd", required=True)
    parser.add_argument("--exe", required=True)
    parser.add_argument("--cols", type=int, default=128)
    parser.add_argument("--rows", type=int, default=40)
    parser.add_argument("--mode", choices=["wide", "narrow", "tabs", "deploy"], default="wide")
    args = parser.parse_args()

    if args.mode == "wide":
        timeline = [(3.0, b"\x03")]
    elif args.mode == "narrow":
        timeline = [(3.0, b"\x03")]
    elif args.mode == "tabs":
        timeline = [
            (0.8, b"\x1b[C"),
            (1.2, b"\x1b[C"),
            (1.6, b"\x1b[C"),
            (2.0, b"\t"),
            (2.4, b"\x1b[B"),
            (2.8, b"\x1b[B"),
            (3.2, b"\x1b[C"),
            (3.6, b"\x1b[C"),
            (4.4, b"\x03"),
        ]
    else:
        timeline = [
            (0.8, b"\x1b[B"),
            (1.2, b"\t"),
            (1.6, b"\x1b[C"),
            (2.0, b"\x1b[B"),
            (3.2, b"\x03"),
        ]

    result = run_capture(args.exe, args.cwd, args.cols, args.rows, timeline)
    result["mode"] = args.mode
    result["cols"] = args.cols
    result["rows"] = args.rows
    print(json.dumps(result, ensure_ascii=True))


if __name__ == "__main__":
    main()
