# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# Simulated HLS viewers for `just test-edges`: bench 02's hls-viewers.py (research/bench/
# 2026-10-24 in the project folder), copied as the release-one plan says, with a content
# steering client added. Python by exception to the house stack, as bench 02's was.
#
# Each viewer reads the multivariant playlist, then the steering manifest its
# EXT-X-CONTENT-STEERING tag names (with _HLS_pathway and _HLS_throughput, as RFC 8216bis
# 7.4 says), applies pathway clones (URI-REPLACEMENT HOST, which replaces the host name and
# keeps the port, as hls.js 1.7.3 does), and plays the first pathway that
# is not penalised: it polls the media playlist every target duration over one keep-alive
# connection, joins three segments back from live and fetches each new segment. A network
# error penalises the pathway for PENALTY_S (two minutes, RFC 8216bis 7.5) and moves to the
# next; the manifest is reloaded every TTL, and the viewer moves when its first pathway
# changes. Every move and failure is recorded with its wall-clock time.
#
# usage: hls-viewers.py <n> <seconds> <multivariant URL> [ramp seconds]
import asyncio, json, os, random, sys, time
from urllib.parse import urlsplit, urljoin, urlencode

n, secs, master = int(sys.argv[1]), float(sys.argv[2]), sys.argv[3]
ramp = float(sys.argv[4]) if len(sys.argv) > 4 else 0.0
PENALTY_S = float(os.environ.get("PENALTY_S", "120"))
st = {"viewers": n, "req": 0, "bytes": 0, "seg": 0, "late": 0, "err": 0, "failover": 0,
      "steered": 0, "steering_req": 0, "fetch_ms": []}
events = []  # [wall time, viewer, kind, pathway]


async def get(conn, host, uri):
    r, w = conn
    w.write(f"GET {uri} HTTP/1.1\r\nHost: {host}\r\nUser-Agent: weavesignal-test-edges\r\n\r\n".encode())
    await w.drain()
    head = await asyncio.wait_for(r.readuntil(b"\r\n\r\n"), 10)
    lines = head.decode("latin-1").split("\r\n")
    status = int(lines[0].split()[1])
    hdr = {l.split(":", 1)[0].lower(): l.split(":", 1)[1].strip() for l in lines[1:] if ":" in l}
    if "content-length" in hdr:
        body = await asyncio.wait_for(r.readexactly(int(hdr["content-length"])), 10)
    else:
        body = b""
        while True:
            size = int((await r.readuntil(b"\r\n")).strip(), 16)
            if size == 0:
                await r.readuntil(b"\r\n"); break
            body += await r.readexactly(size); await r.readexactly(2)
    st["req"] += 1; st["bytes"] += len(body)
    return status, body


async def fetch(url):
    """One request on its own connection (the multivariant playlist, the steering manifest)."""
    u = urlsplit(url)
    conn = await asyncio.wait_for(asyncio.open_connection(u.hostname, u.port or 80), 5)
    try:
        path = u.path + ("?" + u.query if u.query else "")
        return await get(conn, u.netloc, path)
    finally:
        conn[1].close()


def attrs(line):
    out, key, val, quoted, cur = {}, "", "", False, ""
    for ch in line.split(":", 1)[1] + ",":
        if ch == '"': quoted = not quoted
        elif ch == "=" and not quoted and not key: key, cur = cur, ""
        elif ch == "," and not quoted: out[key] = cur; key, cur = "", ""
        else: cur += ch
    return out


async def viewer(i):
    await asyncio.sleep(random.uniform(0, ramp) if ramp else random.uniform(0, 2))
    end = time.monotonic() + secs
    s, body = await fetch(master)
    text = body.decode()
    steering, initial, variant = None, ".", None
    for line in text.splitlines():
        if line.startswith("#EXT-X-CONTENT-STEERING:"):
            a = attrs(line); steering = urljoin(master, a["SERVER-URI"]); initial = a.get("PATHWAY-ID", ".")
        elif line and not line.startswith("#") and variant is None:
            variant = urljoin(master, line)
    origin, port = urlsplit(variant).hostname, urlsplit(variant).port or 80
    path = urlsplit(variant).path
    hosts = {".": origin}           # pathway -> host name; the port is always the playlist's
    priority, penalised = ["."], {}  # pathway -> until (monotonic)
    current, ttl, next_reload = initial, 300.0, 0.0
    conn, seen, have_init, target = None, set(), False, 2.0

    def first_ok():
        now = time.monotonic()
        for p in priority:
            if p in hosts and penalised.get(p, 0) <= now: return p
        return None

    async def reload(throughput):
        nonlocal priority, ttl, next_reload, steering
        if steering is None: return
        q = urlencode({"_HLS_pathway": current, "_HLS_throughput": throughput})
        url = steering + ("&" if "?" in steering else "?") + q
        try:
            s, body = await fetch(url); st["steering_req"] += 1
            if s == 200:
                m = json.loads(body)
                for c in m.get("PATHWAY-CLONES", []):
                    base = hosts.get(c["BASE-ID"])
                    if base is not None:
                        hosts[c["ID"]] = c.get("URI-REPLACEMENT", {}).get("HOST", base)
                priority = m["PATHWAY-PRIORITY"]; ttl = float(m["TTL"])
                if "RELOAD-URI" in m: steering = urljoin(steering, m["RELOAD-URI"])
        except Exception:
            pass  # keep the last manifest and try again after the TTL (the specification's rule 7)
        next_reload = time.monotonic() + ttl

    def apply(kind):
        nonlocal current, conn
        best = first_ok()
        if best is not None and best != current:
            current = best
            if conn: conn[1].close()
            conn = None
            events.append([time.time(), i, kind, current])
            return True
        return False

    await reload(0)
    apply("start")
    events.append([time.time(), i, "joined", current])
    while time.monotonic() < end:
        if time.monotonic() >= next_reload:
            await reload(1_000_000)
            if apply("steered"): st["steered"] += 1
        h = hosts.get(current, origin)
        host = f"{h}:{port}"
        try:
            if conn is None:
                conn = await asyncio.wait_for(asyncio.open_connection(h, port), 5)
            s, body = await get(conn, host, path)
            if s != 200:
                st["err"] += 1; await asyncio.sleep(1); continue
            segs, dur = [], 2.0
            for line in body.decode().splitlines():
                if line.startswith("#EXT-X-TARGETDURATION:"): target = float(line.split(":")[1])
                elif line.startswith("#EXT-X-MAP:") and not have_init:
                    uri = line.split('URI="')[1].split('"')[0]
                    await get(conn, host, path.rsplit("/", 1)[0] + "/" + uri); have_init = True
                elif line.startswith("#EXTINF:"): dur = float(line[8:].split(",")[0])
                elif line and not line.startswith("#"): segs.append((line, dur))
            if not seen:  # join near the live edge, three segments back, as players do
                seen.update(u for u, _ in segs[:-3])
            for u, d in segs:
                if u in seen: continue
                t0 = time.monotonic()
                s, _ = await get(conn, host, path.rsplit("/", 1)[0] + "/" + u)
                ms = (time.monotonic() - t0) * 1000
                seen.add(u)
                if s == 200:
                    st["seg"] += 1; st["fetch_ms"].append(ms)
                    if ms > d * 1000: st["late"] += 1
                    events.append([time.time(), i, "segment", current])
                else: st["err"] += 1
            await asyncio.sleep(target)
        except Exception:
            st["err"] += 1
            if conn: conn[1].close()
            conn = None
            events.append([time.time(), i, "error", current])
            penalised[current] = time.monotonic() + PENALTY_S
            if apply("failover"): st["failover"] += 1
            await asyncio.sleep(0.5)
    events.append([time.time(), i, "end", current])
    if conn: conn[1].close()


async def main():
    await asyncio.gather(*(viewer(i) for i in range(n)))
t = time.monotonic(); asyncio.run(main())
f = sorted(st.pop("fetch_ms")) or [0]
st.update(wall=round(time.monotonic() - t, 1), seg_ms_p50=round(f[len(f)//2], 1), seg_ms_p99=round(f[int(len(f)*.99)], 1),
          kbit_per_viewer=round(st["bytes"] * 8 / 1000 / secs / max(n, 1), 1), events=events)
print(json.dumps(st))
