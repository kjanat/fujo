#!/usr/bin/env python3
import bisect
import errno
import fcntl
import json
import os
from pathlib import Path
import pty
import re
import select
import struct
import subprocess
import sys
import termios
import time


ROOT = Path(__file__).resolve().parent.parent
BINARY = (
    Path(sys.argv[1]) if len(sys.argv) > 1 else ROOT / "target/debug/examples/terminal"
)
FRAMES = "⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏"


def check(condition, message):
    if not condition:
        raise SystemExit(f"check-terminal: {message}")


def terminal(*args, binary=BINARY):
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 30, 100, 0, 0))
    chunks = []
    started = time.monotonic()
    try:
        with subprocess.Popen(
            [str(binary), *args], stdin=subprocess.DEVNULL, stdout=slave, stderr=slave
        ) as process:
            os.close(slave)
            slave = None
            try:
                while True:
                    check(
                        time.monotonic() - started <= 15,
                        "terminal demo exceeded 15 seconds",
                    )
                    ready, _, _ = select.select([master], [], [], 0.1)
                    if ready:
                        try:
                            data = os.read(master, 65536)
                        except OSError as error:
                            if error.errno != errno.EIO:
                                raise
                            break  # Linux PTYs signal EOF with EIO.
                        if not data:
                            break
                        chunks.append((time.monotonic() - started, data))
                process.wait(timeout=2)
            finally:
                if process.poll() is None:
                    process.kill()
                    process.wait()
        return process.returncode, chunks
    finally:
        os.close(master)
        if slave is not None:
            os.close(slave)


def screen(text):
    lines, line, cursor = [], [], 0
    for token in re.findall(r"\x1b\[[0-9;]*[mK]|\r|\n|[^\x1b\r\n]+", text):
        if token == "\r":
            cursor = 0
        elif token == "\n":
            lines.append("".join(line))
            line, cursor = [], 0
        elif token == "\x1b[2K":
            line = []
        elif not token.startswith("\x1b"):
            for char in token:
                if cursor < len(line):
                    line[cursor] = char
                else:
                    line.append(char)
                cursor += 1
    return "\n".join([*lines, "".join(line)])


def spinner_frames(chunks, label):
    raw = b"".join(data for _, data in chunks)
    boundaries, size = [], 0
    for _, data in chunks:
        size += len(data)
        boundaries.append(size)
    pattern = re.compile(("(" + "|".join(FRAMES) + ") " + re.escape(label)).encode())
    matches = list(pattern.finditer(raw))
    return (
        {match[1] for match in matches},
        [chunks[bisect.bisect_left(boundaries, match.end())][0] for match in matches],
    )


code, chunks = terminal("--steps=8", "--delay=40ms", "--verbose")
check(code == 0, f"exit code {code}")
raw = b"".join(data for _, data in chunks)
distinct, times = spinner_frames(chunks, "Discovering inputs")
check(len(distinct) >= 3, f"only {len(distinct)} spinner frames")
check(times[-1] - times[0] >= 0.12, "spinner frames did not arrive over time")
progress = re.findall(rb"Building \[[= ]{20}\] +(\d+)% (\d+)/8", raw)
check({int(current) for _, current in progress} == set(range(9)), progress)
check((b"100", b"8") in progress, progress)
visible = screen(raw.decode())
check("✓ Environment ready\n" in visible, visible)
check(
    "Halfway through; status messages preserve the progress bar\n" in visible, visible
)
check("Built item 8/8\n✓ Built 8 items\n" in visible, visible)
check(
    re.search(
        r"✓ Verification passed\nPhase +Items +Status\nBuild +8 +passed\nVerify +8 +passed",
        visible,
    ),
    visible,
)
check(not any("Building [" in line for line in visible.splitlines()), visible)
print(
    f"TTY: {len(distinct)} distinct spinner frames across {times[-1] - times[0]:.2f}s; "
    "all 9 progress positions; logs and final table remain intact"
)

code, chunks = terminal("--steps=8", "--delay=20ms", "--fail-at=3")
failed = screen(b"".join(data for _, data in chunks).decode())
check(code == 2, f"exit code {code}")
check("✗ Build stopped at item 3/8\nSimulated failure at item 3\n" in failed, failed)
check("Phase" not in failed, failed)
print("TTY failure: exit 2, failed activity cleared, no success table")

for mode in [[], ["--json"], ["--quiet", "--verbose"], ["--json", "--quiet"]]:
    result = subprocess.run(
        [str(BINARY), "--steps=2", "--delay=0ms", *mode],
        capture_output=True,
        text=True,
        timeout=5,
        check=True,
    )
    check("\x1b" not in result.stdout + result.stderr, mode)
    check(not any(frame in result.stdout + result.stderr for frame in FRAMES), mode)
    if "--json" in mode:
        check(
            json.loads(result.stdout)[0]
            == {"phase": "Build", "items": 2, "status": "passed"},
            result.stdout,
        )
    if "--quiet" in mode:
        check(result.stderr == "", result.stderr)
    code, chunks = terminal("--steps=2", "--delay=0ms", *mode)
    check(code == 0, f"exit code {code}")
    if mode:
        check(b"\x1b" not in b"".join(data for _, data in chunks), mode)
print("Modes: pipes, JSON, quiet, and JSON+quiet contain no animation controls")

if len(sys.argv) > 2:
    async_binary = Path(sys.argv[2])
    code, chunks = terminal(
        "--jobs=8", "--delay=80ms", "--profile=check", binary=async_binary
    )
    check(code == 0, f"exit code {code}")
    raw = b"".join(data for _, data in chunks)
    distinct, times = spinner_frames(chunks, "Checking build environment")
    check(len(distinct) >= 3, f"only {len(distinct)} async spinner frames")
    check(times[-1] - times[0] >= 0.12, "async frames did not arrive over time")
    progress = re.findall(rb"Building \[[= ]{20}\] +(\d+)% (\d+)/8", raw)
    check({int(current) for _, current in progress} == set(range(9)), progress)
    visible = screen(raw.decode())
    check(
        "✓ Build complete\ncheck: 8 jobs passed\nBuild invocation finished\n"
        in visible,
        visible,
    )
    code, chunks = terminal(
        "--jobs=2",
        "--delay=0ms",
        "--profile=check",
        "--quiet",
        "--json",
        binary=async_binary,
    )
    check(code == 0, f"exit code {code}")
    raw = b"".join(data for _, data in chunks)
    check(b"\x1b" not in raw, raw)
    check(json.loads(raw) == {"profile": "check", "jobs": 2, "status": "passed"}, raw)
    print(
        f"Async TTY: {len(distinct)} spinner frames across {times[-1] - times[0]:.2f}s; "
        "all progress positions and clean quiet JSON output"
    )
