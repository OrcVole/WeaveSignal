#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# A STUN server for `just test-no-stun`'s second phase (RFC 8489, binding only): answers each
# binding request with the address it came from (XOR-MAPPED-ADDRESS) and logs one line per
# request. Python by exception, like the other checks around the product.
#
#   python3 -I stun-responder.py 3478
import socket, struct, sys

COOKIE = 0x2112A442
port = int(sys.argv[1]) if len(sys.argv) > 1 else 3478
s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
s.bind(("0.0.0.0", port))
print(f"stun-local: listening on udp {port}", flush=True)
while True:
    data, (host, sport) = s.recvfrom(2048)
    if len(data) < 20:
        continue
    mtype, length, cookie = struct.unpack("!HHI", data[:8])
    if mtype != 0x0001 or cookie != COOKIE:
        continue
    txid = data[8:20]
    ip = struct.unpack("!I", socket.inet_aton(host))[0]
    # XOR-MAPPED-ADDRESS: family IPv4, the port and address XORed with the cookie.
    value = struct.pack("!BBHI", 0, 1, sport ^ (COOKIE >> 16), ip ^ COOKIE)
    attr = struct.pack("!HH", 0x0020, len(value)) + value
    s.sendto(struct.pack("!HHI", 0x0101, len(attr), COOKIE) + txid + attr, (host, sport))
    print(f"stun-local: binding request from {host}:{sport}", flush=True)
