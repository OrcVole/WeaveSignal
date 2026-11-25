# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# A stand-in for playout's keyed gate on its control socket, for tests/keys/run-real.sh: the
# Schedule's stack has no playout, and since 1.3.2's integration the station refuses to key a
# show while playout does not answer that it can keep it from the public. This answers
# `keyed.status`, `keyed.window` and `keyed.mute` as playout/weavesignal.liq does (keyed shows
# on), and `error` to anything else. It keeps nothing from anyone: it only lets the screens and
# the control plane be driven together.
#
#   python3 -I tests/keys/playout-standin.py /path/to/playout.sock
import json
import os
import socketserver
import sys

state = {"window": False}


class Handler(socketserver.StreamRequestHandler):
    def handle(self):
        for raw in self.rfile:
            line = raw.decode("utf-8", "replace").strip()
            if line in ("", "quit"):
                return
            if line == "keyed.status":
                answer = json.dumps({"enabled": True, "window": state["window"], "item": False,
                                     "shut": state["window"], "muted": False, "slate": "fallback"},
                                    separators=(",", ":"))
            elif line in ("keyed.window on", "keyed.window off"):
                state["window"] = line.endswith("on")
                answer = "ok " + line.split()[-1]
            elif line.startswith("keyed.mute "):
                answer = "ok muted"
            else:
                answer = "error not in the stand-in"
            self.wfile.write(f"{answer}\nEND\n".encode())


path = sys.argv[1]
if os.path.exists(path):
    os.unlink(path)
with socketserver.UnixStreamServer(path, Handler) as server:
    server.serve_forever()
