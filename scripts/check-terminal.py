#!/usr/bin/env python3
"""Verify actual timed TTY frames, plus clean piped/JSON/quiet output (Unix)."""

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
                    if time.monotonic() - started > 15:
                        raise AssertionError("terminal demo exceeded 15 seconds")
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
    """Apply the small subset of terminal controls emitted by this example."""
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
    # Match complete UTF-8 glyphs rather than a byte character class.
    pattern = re.compile(("(" + "|".join(FRAMES) + ") " + re.escape(label)).encode())
    matches = list(pattern.finditer(raw))
    return ({match[1] for match in matches},
            [chunks[bisect.bisect_left(boundaries, match.end())][0] for match in matches])


code, chunks = terminal("--steps=8", "--delay=40ms", "--verbose")
assert code == 0, code
raw = b"".join(data for _, data in chunks)
distinct, times = spinner_frames(chunks, "Discovering inputs")
assert len(distinct) >= 3, f"only {len(distinct)} spinner frames"
assert times[-1] - times[0] >= 0.12, "spinner frames did not arrive over time"
progress = re.findall(rb"Building \[[= ]{20}\] +(\d+)% (\d+)/8", raw)
assert {int(current) for _, current in progress} == set(range(9)), progress
assert (b"100", b"8") in progress
visible = screen(raw.decode())
assert "✓ Environment ready\n" in visible
assert "Halfway through; status messages preserve the progress bar\n" in visible
assert "Built item 8/8\n✓ Built 8 items\n" in visible
assert re.search(
    r"✓ Verification passed\nPhase +Items +Status\nBuild +8 +passed\nVerify +8 +passed",
    visible,
), visible
assert not any("Building [" in line for line in visible.splitlines()), visible
print(
    f"TTY: {len(distinct)} distinct spinner frames across {times[-1] - times[0]:.2f}s; "
    "all 9 progress positions; logs and final table remain intact"
)

code, chunks = terminal("--steps=8", "--delay=20ms", "--fail-at=3")
failed = screen(b"".join(data for _, data in chunks).decode())
assert code == 2, code
assert "✗ Build stopped at item 3/8\nSimulated failure at item 3\n" in failed, failed
assert "Phase" not in failed
print("TTY failure: exit 2, failed activity cleared, no success table")

for mode in [[], ["--json"], ["--quiet", "--verbose"], ["--json", "--quiet"]]:
    result = subprocess.run(
        [str(BINARY), "--steps=2", "--delay=0ms", *mode],
        capture_output=True,
        text=True,
        timeout=5,
        check=True,
    )
    assert "\x1b" not in result.stdout + result.stderr
    assert not any(frame in result.stdout + result.stderr for frame in FRAMES)
    if "--json" in mode:
        assert json.loads(result.stdout)[0] == {
            "phase": "Build",
            "items": 2,
            "status": "passed",
        }
    if "--quiet" in mode:
        assert result.stderr == "", result.stderr
    code, chunks = terminal("--steps=2", "--delay=0ms", *mode)
    assert code == 0
    if mode:  # JSON and quiet must suppress animation even on a real TTY.
        assert b"\x1b" not in b"".join(data for _, data in chunks)
print("Modes: pipes, JSON, quiet, and JSON+quiet contain no animation controls")

if len(sys.argv) > 2:
    async_binary = Path(sys.argv[2])
    code, chunks = terminal("--jobs=8", "--delay=80ms", "--profile=check", binary=async_binary)
    assert code == 0, code
    raw = b"".join(data for _, data in chunks)
    distinct, times = spinner_frames(chunks, "Checking build environment")
    assert len(distinct) >= 3, f"only {len(distinct)} async spinner frames"
    assert times[-1] - times[0] >= 0.12, "async frames did not arrive over time"
    progress = re.findall(rb"Building \[[= ]{20}\] +(\d+)% (\d+)/8", raw)
    assert {int(current) for _, current in progress} == set(range(9)), progress
    visible = screen(raw.decode())
    assert "✓ Build complete\ncheck: 8 jobs passed\nBuild invocation finished\n" in visible, visible
    code, chunks = terminal("--jobs=2", "--delay=0ms", "--profile=check", "--quiet", "--json", binary=async_binary)
    assert code == 0
    raw = b"".join(data for _, data in chunks)
    assert b"\x1b" not in raw
    assert json.loads(raw) == {"profile": "check", "jobs": 2, "status": "passed"}
    print(f"Async TTY: {len(distinct)} spinner frames across {times[-1] - times[0]:.2f}s; "
          "all progress positions and clean quiet JSON output")
