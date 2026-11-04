# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# One Icecast listener for `just test-edges`: reads an Ogg Opus stream for a while, without
# asking for ICY metadata, and reports what it heard as JSON: bytes and kbit/s, the Opus
# headers seen (one per chained stream), each TITLE in the OpusTags with when it arrived, and
# when the server closed the stream, if it did. Python by exception, as bench 01's listeners.
#
# usage: ogg-listen.py <host> <port> <path> <seconds>
import json, socket, struct, sys, time

host, port, path, secs = sys.argv[1], int(sys.argv[2]), sys.argv[3], float(sys.argv[4])
out = {"status": None, "bytes": 0, "heads": 0, "titles": [], "closed_at": None}
t0 = time.monotonic()
try:
    s = socket.create_connection((host, port), 5); s.settimeout(5)
    s.sendall(f"GET {path} HTTP/1.0\r\nHost: {host}\r\nUser-Agent: weavesignal-test-edges\r\n\r\n".encode())
    buf = b""
    while b"\r\n\r\n" not in buf:
        d = s.recv(4096)
        if not d: break
        buf += d
    head, _, buf = buf.partition(b"\r\n\r\n")
    out["status"] = int(head.split()[1]) if head else None
    out["bytes"] = len(buf)
    while time.monotonic() - t0 < secs:
        try: d = s.recv(65536)
        except socket.timeout: out["closed_at"] = round(time.monotonic() - t0, 1); out["timeout"] = True; break
        if not d: out["closed_at"] = round(time.monotonic() - t0, 1); break
        buf += d; out["bytes"] += len(d)
        while True:
            i = buf.find(b"OggS")
            if i < 0 or len(buf) < i + 27: break
            nseg = buf[i + 26]
            if len(buf) < i + 27 + nseg: break
            size = sum(buf[i + 27:i + 27 + nseg])
            if len(buf) < i + 27 + nseg + size: break
            body = buf[i + 27 + nseg:i + 27 + nseg + size]
            if body.startswith(b"OpusHead"): out["heads"] += 1
            if body.startswith(b"OpusTags"):
                p = 12 + struct.unpack("<I", body[8:12])[0]
                count = struct.unpack("<I", body[p:p + 4])[0]; p += 4
                for _ in range(count):
                    l = struct.unpack("<I", body[p:p + 4])[0]
                    c = body[p + 4:p + 4 + l].decode("utf-8", "replace"); p += 4 + l
                    if c.lower().startswith("title="):
                        out["titles"].append([round(time.monotonic() - t0, 1), c[6:]])
            buf = buf[i + 27 + nseg + size:]
except OSError as e:
    out["error"] = str(e)
wall = time.monotonic() - t0
out["kbit_s"] = round(out["bytes"] * 8 / 1000 / max(wall, 0.001), 1)
print(json.dumps(out))
