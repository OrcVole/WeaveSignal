# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# Requests per second for one path over one kept-alive connection, and the time each took:
# the cost of the edge's token check, compared on a cache hit with the public path's.
#   python3 -I bench.py HOST PORT PATH COUNT
import http.client, json, sys, time

host, port, path, count = sys.argv[1], int(sys.argv[2]), sys.argv[3], int(sys.argv[4])
c = http.client.HTTPConnection(host, port)
codes, took = {}, []
for _ in range(count):
    t = time.perf_counter()
    c.request("GET", path)
    r = c.getresponse()
    r.read()
    took.append(time.perf_counter() - t)
    codes[r.status] = codes.get(r.status, 0) + 1
took.sort()
print(json.dumps({"codes": codes, "rps": round(count / sum(took)), "p50_ms": round(took[len(took) // 2] * 1000, 3), "p99_ms": round(took[int(len(took) * 0.99)] * 1000, 3)}))
