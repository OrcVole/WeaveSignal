# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# A stand-in listener mount for tests/keyed/run-edge.sh: any GET is an endless audio/ogg body,
# 2 KB every 100 ms, so a test can see when the edge ends a stream.
#   python3 -I stream.py PORT
import http.server, sys, time


class Mount(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.0"

    def do_GET(self):
        self.send_response(200)
        self.send_header("Content-Type", "audio/ogg")
        self.end_headers()
        try:
            while True:
                self.wfile.write(b"O" * 2048)
                self.wfile.flush()
                time.sleep(0.1)
        except (BrokenPipeError, ConnectionResetError):
            pass

    def log_message(self, *a):
        pass


http.server.ThreadingHTTPServer(("127.0.0.1", int(sys.argv[1])), Mount).serve_forever()
