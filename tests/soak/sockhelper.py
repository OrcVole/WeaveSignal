# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# A line server for the two control sockets that live in Podman volumes, run once under
# `podman unshare` so the soak's sampler can ask them every second without a process each time
# (the volume's full path is too long for a Unix socket's address, so it works from inside it).
#
#   podman unshare python3 -I sockhelper.py <listeners volume dir> <desk volume dir>
#
# Reads one request a line on stdin, answers one JSON line on stdout:
#   L <command>   the listener process's socket (listeners.sock): its first answer line
#   D             the desk's status (desk.sock)
import json
import os
import socket
import sys

LISTENERS, DESK = sys.argv[1], sys.argv[2]


def listeners(cmd):
    os.chdir(LISTENERS)
    s = socket.socket(socket.AF_UNIX)
    s.settimeout(5)
    s.connect("listeners.sock")
    s.sendall(cmd.encode() + b"\n")
    out = b""
    while b"END" not in out:
        chunk = s.recv(256)
        if not chunk:
            break
        out += chunk
    s.close()
    return out.decode().split("\r\n")[0]


def desk():
    os.chdir(DESK)
    s = socket.socket(socket.AF_UNIX)
    s.settimeout(5)
    s.connect("desk.sock")
    s.sendall(b'{"op":"status"}\n')
    out = b""
    while not out.endswith(b"\n"):
        chunk = s.recv(65536)
        if not chunk:
            break
        out += chunk
    s.close()
    d = json.loads(out)
    return {k: d[k] for k in ("ok", "playout", "mode", "delay", "mix") if k in d}


for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    try:
        if line.startswith("L "):
            answer = {"ok": True, "v": listeners(line[2:])}
        elif line == "D":
            answer = {"ok": True, "v": desk()}
        else:
            answer = {"ok": False, "error": "unknown request"}
    except Exception as e:  # a socket that is away is an answer, not the end of the helper
        answer = {"ok": False, "error": f"{type(e).__name__}: {e}"}
    print(json.dumps(answer), flush=True)
