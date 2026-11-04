# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# A stand-in for a station's HTTPS listener address, for relay-tls.sh: serves one Ogg Opus file
# at /programme.opus over TLS at about 100 kbit/s, but only to a request with the expected
# listener login (401 otherwise); every request's line and headers go to a JSON-lines log, so
# the test can see what the relay's hop sent. Usage: tls-origin.py PORT CERT KEY OGG USER:PASSWORD LOG
import base64
import http.server
import json
import socketserver
import ssl
import sys
import time

port, cert, key, ogg, login, log = sys.argv[1:7]
data = open(ogg, "rb").read()
want = "Basic " + base64.b64encode(login.encode()).decode()


class Handler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.0"

    def log_message(self, *_):
        pass

    def do_GET(self):
        with open(log, "a") as f:
            f.write(json.dumps({"path": self.path, "headers": dict(self.headers)}) + "\n")
        if self.headers.get("Authorization") != want:
            self.send_response(401)
            self.end_headers()
            return
        if self.path != "/programme.opus":
            self.send_response(404)
            self.end_headers()
            return
        self.send_response(200)
        self.send_header("Content-Type", "application/ogg")
        self.end_headers()
        try:
            for i in range(0, len(data), 1200):
                self.wfile.write(data[i : i + 1200])
                self.wfile.flush()
                time.sleep(0.1)
        except (BrokenPipeError, ConnectionResetError, ssl.SSLError):
            pass


class Server(socketserver.ThreadingMixIn, http.server.HTTPServer):
    daemon_threads = True
    allow_reuse_address = True

    def get_request(self):
        sock, addr = self.socket.accept()
        return ctx.wrap_socket(sock, server_side=True, do_handshake_on_connect=False), addr

    def handle_error(self, *_):
        pass


ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
ctx.load_cert_chain(cert, key)
Server(("127.0.0.1", int(port)), Handler).serve_forever()
