"""Exercise generated completions through Zsh's real line editor and compinit."""

import json
import os
from pathlib import Path
import pty
import re
import select
import shlex
import subprocess
import sys
import time

script, cases = json.load(sys.stdin)
functions = Path.cwd() / "functions"
functions.mkdir(exist_ok=True)
(functions / "_demo").write_text(script)
master, slave = pty.openpty()
child = subprocess.Popen(
    ["zsh", "-f", "-i"], stdin=slave, stdout=slave, stderr=slave, close_fds=True
)
os.close(slave)


def read_until(marker):
    end = time.monotonic() + 10
    data = b""
    while time.monotonic() < end:
        ready, _, _ = select.select([master], [], [], max(0, end - time.monotonic()))
        if ready:
            data += os.read(master, 65536)
            if marker in data:
                return data.decode(errors="replace")
    raise AssertionError(f"Zsh did not report {marker!r}: {data!r}")


try:
    setup = (
        "PROMPT=''; RPROMPT=''; "
        f"fpath=({shlex.quote(str(functions))} $fpath); "
        "autoload -Uz compinit; compinit -D -i; "
        "completion-test() { zle complete-word; "
        'print -r -- "COMPLETED=$BUFFER"; print -r -- FINISHED; zle redisplay; }; '
        "zle -N completion-test; bindkey '^I' completion-test; "
        "print -r -- READY\n"
    )
    os.write(master, setup.encode())
    read_until(b"\r\nREADY\r\n")
    results = []
    for words in cases:
        line = " ".join(shlex.quote(word) if word else "" for word in words)
        os.write(master, b"\x15" + line.encode() + b"\t")
        output = read_until(b"\r\nFINISHED\r\n")
        matches = re.findall(r"COMPLETED=([^\r\n]*)", output)
        assert matches, output
        completed = matches[-1].rstrip(" ")
        if completed == line.rstrip(" "):
            results.append([])
        else:
            prefix = (
                line[: len(line) - len(shlex.quote(words[-1]))] if words[-1] else line
            )
            assert completed.startswith(prefix), (line, completed)
            results.append([completed[len(prefix) :]])
    print(json.dumps(results))
finally:
    os.write(master, b"\x15exit\n")
    try:
        child.wait(timeout=3)
    except subprocess.TimeoutExpired:
        child.kill()
        child.wait()
    os.close(master)
