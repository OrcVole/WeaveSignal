#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# `just test-no-stun`'s judge: reads what left the station's network namespace (a pcap of its
# eth0, written by tcpdump) and says whether the station asked a STUN server (ADR-0290).
# Python by exception, like the other checks around the product; the standard library only.
#
# It finds, among the packets the station sent (source MAC its own):
#   - STUN requests and indications (RFC 8489: the first two bits zero and the magic cookie
#     0x2112A442 at bytes 4-8) in any UDP payload, on any port (Google's is 19302, not 3478), and
#     at the start of any TCP payload. A request that carries USERNAME is an ICE connectivity
#     check between two peers (the desk and LiveKit), not a question to a STUN server, and one
#     addressed to loopback cannot leave the machine: both are listed, not judged;
#   - TCP connections opened to the usual STUN and TURN ports (3478, 5349, 19302-19309);
#   - the names it looked up (DNS, mDNS and LLMNR queries), and among them any with a label
#     containing "stun" (stun.l.google.com, global.stun.twilio.com, stun.cloudflare.com ...).
#
#   analyse.py PCAP --own-ip 10.89.0.2 --canary 192.0.2.1 [--senders SS.txt]
#              [--expect-stun IP --expect-name stun-local] [--report OUT.json]
#
# Without --expect-stun (the default settings) it fails on any STUN request or STUN name. With
# it (a STUN server named), it fails unless the station asked that server, and on any other. Either
# way the canary (one binding request the test sends to TEST-NET-1 at the end) must be found: it
# proves the capture was live and the detector works, so a quiet result is not a dead capture.
import argparse, ipaddress, json, re, struct, sys
from collections import Counter

COOKIE = b"\x21\x12\xa4\x42"
STUN_TCP_PORTS = {3478, 5349} | set(range(19302, 19310))
STUN_NAME = re.compile(r"(^|\.)[^.]*stun[^.]*(\.|$)", re.I)


def packets(path):
    with open(path, "rb") as f:
        head = f.read(24)
        if len(head) < 24:
            return
        magic = head[:4]
        if magic in (b"\xd4\xc3\xb2\xa1", b"\x4d\x3c\xb2\xa1"):
            endian = "<"
        elif magic in (b"\xa1\xb2\xc3\xd4", b"\xa1\xb2\x3c\x4d"):
            endian = ">"
        else:
            sys.exit(f"{path} is not a pcap file")
        nano = magic in (b"\x4d\x3c\xb2\xa1", b"\xa1\xb2\x3c\x4d")
        link = struct.unpack(endian + "I", head[20:24])[0]
        if link != 1:
            sys.exit(f"{path}: link type {link}, not Ethernet")
        while True:
            h = f.read(16)
            if len(h) < 16:
                return
            sec, frac, incl, _ = struct.unpack(endian + "IIII", h)
            data = f.read(incl)
            yield sec + frac / (1e9 if nano else 1e6), data


def decode(frame):
    """(src_mac, proto, src, dst, sport, dport, payload, tcp_flags) or None."""
    if len(frame) < 14:
        return None
    src_mac = frame[6:12].hex(":")
    etype = struct.unpack("!H", frame[12:14])[0]
    off = 14
    if etype == 0x8100 and len(frame) >= 18:
        etype = struct.unpack("!H", frame[16:18])[0]
        off = 18
    if etype == 0x0800:
        ip = frame[off:]
        if len(ip) < 20:
            return None
        ihl = (ip[0] & 0x0F) * 4
        proto = ip[9]
        src, dst = ipaddress.ip_address(ip[12:16]), ipaddress.ip_address(ip[16:20])
        l4 = ip[ihl:]
    elif etype == 0x86DD:
        ip = frame[off:]
        if len(ip) < 40:
            return None
        proto = ip[6]
        src, dst = ipaddress.ip_address(ip[8:24]), ipaddress.ip_address(ip[24:40])
        l4 = ip[40:]
        # One hop-by-hop header (multicast listener reports carry one); others are left as they are.
        if proto == 0 and len(l4) >= 8:
            proto, l4 = l4[0], l4[(l4[1] + 1) * 8:]
    else:
        return None
    if proto == 17 and len(l4) >= 8:
        sport, dport = struct.unpack("!HH", l4[:4])
        return src_mac, "udp", src, dst, sport, dport, l4[8:], 0
    if proto == 6 and len(l4) >= 20:
        sport, dport = struct.unpack("!HH", l4[:4])
        doff = (l4[12] >> 4) * 4
        return src_mac, "tcp", src, dst, sport, dport, l4[doff:], l4[13]
    return src_mac, {1: "icmp", 58: "icmp6", 2: "igmp"}.get(proto, f"ip{proto}"), src, dst, 0, 0, b"", 0


def stun_request(payload):
    """The STUN method and class of a request or indication, or None."""
    if len(payload) < 20 or payload[0] & 0xC0 or payload[4:8] != COOKIE:
        return None
    mtype = struct.unpack("!H", payload[:2])[0]
    cls = ((mtype >> 4) & 1) | ((mtype >> 7) & 2)  # 0 request, 1 indication, 2 success, 3 error
    if cls not in (0, 1):
        return None
    method = (mtype & 0x000F) | ((mtype >> 1) & 0x0070) | ((mtype >> 2) & 0x0F80)
    return {1: "binding", 3: "allocate"}.get(method, f"method {method}") + (" request" if cls == 0 else " indication")


def has_username(payload):
    """Whether a STUN message carries USERNAME (0x0006): an ICE connectivity check between two
    peers does, a request to a STUN server for the address it is seen from does not."""
    length = struct.unpack("!H", payload[2:4])[0]
    i, end = 20, min(len(payload), 20 + length)
    while i + 4 <= end:
        t, n = struct.unpack("!HH", payload[i:i + 4])
        if t == 0x0006:
            return True
        i += 4 + (n + 3) // 4 * 4
    return False


def query_names(payload):
    """The names asked in a DNS-format query (DNS, mDNS, LLMNR)."""
    if len(payload) < 12 or payload[2] & 0x80:  # a response
        return []
    qd = struct.unpack("!H", payload[4:6])[0]
    names, i = [], 12
    for _ in range(min(qd, 8)):
        labels = []
        while i < len(payload):
            n = payload[i]
            if n == 0 or n & 0xC0:
                i += 1 if n == 0 else 2
                break
            labels.append(payload[i + 1:i + 1 + n].decode("ascii", "replace"))
            i += 1 + n
        i += 4  # type and class
        names.append(".".join(labels).lower())
    return names


def senders(path):
    """Local UDP port -> process, from `ss -H -uanp` run in the namespace."""
    out = {}
    if not path:
        return out
    for line in open(path):
        cols = line.split()
        if len(cols) < 5:
            continue
        local = cols[3]
        port = local.rsplit(":", 1)[-1]
        procs = re.findall(r'\("([^"]+)",pid=(\d+)', line)
        if port.isdigit() and procs:
            out[int(port)] = ",".join(sorted({p for p, _ in procs}))
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("pcap")
    ap.add_argument("--own-ip", required=True)
    ap.add_argument("--canary", required=True)
    ap.add_argument("--senders")
    ap.add_argument("--expect-stun")
    ap.add_argument("--expect-name")
    ap.add_argument("--report")
    a = ap.parse_args()
    own_ip, canary = ipaddress.ip_address(a.own_ip), ipaddress.ip_address(a.canary)
    expect = ipaddress.ip_address(a.expect_stun) if a.expect_stun else None
    who = senders(a.senders)

    frames = [(t, d) for t, d in ((t, decode(f)) for t, f in packets(a.pcap)) if d]
    own_macs = {d[0] for _, d in frames if d[2] == own_ip}
    out = [(t, d) for t, d in frames if d[0] in own_macs or d[2] == own_ip]
    flows, names, stun, tcp_stun, checks = Counter(), Counter(), [], [], []
    for t, (_, proto, src, dst, sport, dport, payload, flags) in out:
        flows[(proto, str(dst), dport)] += 1
        if proto == "udp" and dport in (53, 5353, 5355):
            for n in query_names(payload):
                names[n] += 1
        if proto == "tcp" and dport == 53 and len(payload) > 2:
            for n in query_names(payload[2:]):
                names[n] += 1
        kind = stun_request(payload) if proto in ("udp", "tcp") else None
        if kind:
            # Addressed to loopback, a packet cannot leave the machine (the gateway drops it):
            # libnice binds its sockets to eth0, so its checks toward a 127.0.0.1 candidate
            # (LiveKit's node_ip in the example settings) are written there all the same.
            ice = has_username(payload)
            entry = {"t": round(t, 3), "proto": proto, "to": f"{dst}:{dport}", "from_port": sport,
                     "process": who.get(sport, "?") if proto == "udp" else "?", "what": kind,
                     "ice_check": ice, "loopback": dst.is_loopback}
            (checks if ice or dst.is_loopback else stun).append(entry)
        if proto == "tcp" and flags & 0x02 and not flags & 0x10 and dport in STUN_TCP_PORTS:
            tcp_stun.append({"t": round(t, 3), "to": f"{dst}:{dport}"})

    is_canary = lambda s: s["to"].rsplit(":", 1)[0] == str(canary)
    to_expected = lambda s: expect is not None and s["to"].rsplit(":", 1)[0] == str(expect)
    canaries = [s for s in stun if is_canary(s)]
    expected = [s for s in stun if to_expected(s)]
    stray = [s for s in stun if not is_canary(s) and not to_expected(s)]
    stun_names = sorted(n for n in names if STUN_NAME.search(n) and n.split(".")[0] != (a.expect_name or "\0"))
    expected_names = sorted(n for n in names if a.expect_name and n.split(".")[0] == a.expect_name)

    report = {
        "packets_out": len(out),
        "flows_out": [{"proto": p, "to": d, "port": port, "packets": n} for (p, d, port), n in sorted(flows.items())],
        "names_asked": dict(sorted(names.items())),
        "stun_requests": stun,
        "ice_checks_and_loopback": checks,
        "tcp_to_stun_ports": tcp_stun,
    }
    if a.report:
        with open(a.report, "w") as f:
            json.dump(report, f, indent=2)

    print(f"  {len(out)} packets left the station's namespace; to:")
    for f in report["flows_out"]:
        print(f"    {f['proto']:5} {f['to']}:{f['port']}  {f['packets']} packets")
    print(f"  names looked up: {', '.join(f'{n} ({c})' for n, c in sorted(names.items())) or 'none'}")
    seen = Counter((c["to"], c["process"], "ICE check" if c["ice_check"] else "to loopback") for c in checks)
    for (to, proc, what), n in sorted(seen.items()):
        print(f"  not a STUN server: {n} {what}(s) to {to} ({proc})")
    for s in stun:
        print(f"  STUN {s['what']} {s['proto']} to {s['to']} from port {s['from_port']} ({s['process']})"
              + (" [the test's canary]" if is_canary(s) else ""))

    bad = []
    if not canaries:
        bad.append("the canary binding request was not in the capture: the capture is not live, or the detector is broken")
    if stray:
        bad.append(f"{len(stray)} STUN request(s) to a server not named: " + ", ".join(sorted({s['to'] for s in stray})))
    if tcp_stun and not expect:
        bad.append("TCP connection(s) to STUN or TURN ports: " + ", ".join(sorted({s['to'] for s in tcp_stun})))
    if stun_names:
        bad.append("STUN server name(s) looked up: " + ", ".join(stun_names))
    if expect is not None:
        if not expected:
            bad.append(f"no STUN request reached the named server {expect}: the setting is not used, or the capture misses it")
        elif a.senders and not any(s["process"] != "?" for s in expected):
            print("  (the requests' sending process could not be named from the namespace's sockets)")
    for b in bad:
        print(f"FAIL: {b}")
    if not bad:
        if expect is not None:
            print(f"  ok: {len(expected)} STUN request(s) to the named server {expect}"
                  + (f" (looked up as {', '.join(expected_names)})" if expected_names else "")
                  + ", none to any other; the canary seen")
        else:
            print("  ok: no STUN request and no STUN name left the station; the canary seen")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
