#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# The port scan gate (release-one plan, M1): every socket the given containers listen on,
# and a failure for any that is not on loopback unless it is named as public. An unconnected
# UDP socket on an ephemeral port is a client's (an SRT caller's, say): it is reported as a
# warning, not a failure, because no setting binds it; it is reachable from the network all
# the same, so a host firewall must keep the ephemeral range closed.
# Python by exception, like the bench scripts: a check around the product, run under
# `podman unshare` so it can read the containers' processes (rootless Podman).
#
#   podman unshare python3 -I tests/ports/scan.py --public 7882/udp weavesignal-livekit-1 ...
import argparse, ipaddress, json, os, subprocess, sys

def hexaddr(h):
    ip, port = h.split(":")
    raw = bytes.fromhex(ip)
    if len(raw) == 4:
        addr = ipaddress.IPv4Address(raw[::-1])
    else:  # four little-endian 32-bit words
        addr = ipaddress.IPv6Address(b"".join(raw[i:i + 4][::-1] for i in range(0, 16, 4)))
    return addr, int(port, 16)

def sockets(pid):
    """Listening TCP and bound UDP sockets in the process's network namespace: inode -> (proto, addr, port)."""
    out = {}
    for proto, f, listen_state in (("tcp", "tcp", "0A"), ("tcp", "tcp6", "0A"), ("udp", "udp", None), ("udp", "udp6", None)):
        try:
            lines = open(f"/proc/{pid}/net/{f}").read().splitlines()[1:]
        except OSError:
            continue
        for line in lines:
            cols = line.split()
            if listen_state and cols[3] != listen_state:
                continue
            addr, port = hexaddr(cols[1])
            out[cols[9]] = (proto, addr, port)
    return out

def tree(pid):
    pids, todo = [], [pid]
    while todo:
        p = todo.pop()
        pids.append(p)
        try:
            for t in os.listdir(f"/proc/{p}/task"):
                todo += open(f"/proc/{p}/task/{t}/children").read().split()
        except OSError:
            pass
    return pids

def ephemeral():
    lo, hi = open("/proc/sys/net/ipv4/ip_local_port_range").read().split()
    return int(lo), int(hi)

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--public", action="append", default=[], help="PORT/PROTO allowed off loopback")
    ap.add_argument("containers", nargs="+")
    a = ap.parse_args()
    public = {tuple(x.split("/")) for x in a.public}
    bad, warn, seen = [], [], []
    lo, hi = ephemeral()
    for c in a.containers:
        pid = subprocess.run(["podman", "inspect", "-f", "{{.State.Pid}}", c], capture_output=True, text=True).stdout.strip()
        if not pid or pid == "0":
            sys.exit(f"{c} is not running")
        socks = sockets(pid)
        found = set()
        for p in tree(pid):
            try:
                fds = os.listdir(f"/proc/{p}/fd")
            except OSError:
                continue
            for fd in fds:
                try:
                    link = os.readlink(f"/proc/{p}/fd/{fd}")
                except OSError:
                    continue
                if link.startswith("socket:[") and link[8:-1] in socks:
                    found.add(socks[link[8:-1]])
        for proto, addr, port in sorted(found, key=lambda s: (s[2], s[0], str(s[1]))):
            ok = addr.is_loopback or (str(port), proto) in public
            client = not ok and proto == "udp" and lo <= port <= hi
            seen.append({"container": c, "proto": proto, "address": str(addr), "port": port,
                         "ok": ok, "client": client})
            if client:
                warn.append(f"{c}: {proto} {addr}:{port}")
            elif not ok:
                bad.append(f"{c}: {proto} {addr}:{port}")
    print(json.dumps(seen, indent=1))
    if warn:
        print("warning: client sockets on ephemeral ports, reachable unless firewalled:\n  " + "\n  ".join(warn))
    if bad:
        sys.exit("listening off loopback and not public:\n  " + "\n  ".join(bad))
    print(f"port scan: {len(seen)} sockets; only the public ones named are off loopback")

main()
