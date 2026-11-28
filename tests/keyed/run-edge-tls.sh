#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# The HLS edge in front of a station it reaches over HTTPS by host name, as real edges do (the other
# edge tests give it http://127.0.0.1, so neither the certificate nor the name was ever tried). A
# stand-in origin (nginx) answers as origin.test on a network of its own, with a certificate behind
# two intermediates, signed by a root made here and given to the edge as its only trusted root.
# It fails if the edge's /health or a playlist does not come through, if a made-up token opens a
# keyed path, or if the revoked keys' list is not read.
#
# Port 26512 (edge). Run it inside the stack lock:
#   flock ~/Code/WeaveSignal/tools/1.1.1/stack.lock tests/keyed/run-edge-tls.sh
# EDGE_IMAGE names the image; SKIP_BUILD=1 tries one that is already built.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
run="$here/run-edge-tls"
prefix=${WEAVESIGNAL_TEST_PREFIX:-ws}-edge-tls
image=${EDGE_IMAGE:-localhost/weavesignal-edge:test-edge-tls}
EDGE_PORT=${EDGE_PORT:-26512}
edge="http://127.0.0.1:$EDGE_PORT"

if ss -H -ln -t "sport = :$EDGE_PORT" | grep -q .; then echo "port $EDGE_PORT/tcp is in use; set EDGE_PORT" >&2; exit 1; fi
rm -rf "$run"; mkdir -p "$run/pki" "$run/log"
cleanup() {
  podman logs "$prefix-edge" > "$run/edge.log" 2>&1 || true
  podman rm -f "$prefix-edge" "$prefix-origin" >/dev/null 2>&1 || true
  podman network rm -f "$prefix-net" >/dev/null 2>&1 || true
}
trap cleanup EXIT
podman rm -f "$prefix-edge" "$prefix-origin" >/dev/null 2>&1 || true
podman network rm -f "$prefix-net" >/dev/null 2>&1 || true
[ "${SKIP_BUILD:-0}" = 1 ] || nice -n 19 podman build -q --target hls -t "$image" "$root/deploy/images/edge" >/dev/null

# root -> int1 -> int2 -> origin.test, all P-256.
cd "$run/pki"
key() { openssl ecparam -name prime256v1 -genkey -noout -out "$1.key"; }
printf 'basicConstraints=critical,CA:true\nkeyUsage=critical,keyCertSign\n' > ca.ext
printf 'basicConstraints=critical,CA:false\nsubjectAltName=DNS:origin.test\nextendedKeyUsage=serverAuth\n' > leaf.ext
key root
openssl req -x509 -new -key root.key -days 2 -subj "/CN=Test Root" -out root.crt \
  -addext "basicConstraints=critical,CA:true" -addext "keyUsage=critical,keyCertSign"
sign() { # name issuer extfile
  key "$1"
  openssl req -new -key "$1.key" -subj "/CN=$1" -out "$1.csr"
  openssl x509 -req -in "$1.csr" -CA "$2.crt" -CAkey "$2.key" -set_serial "$RANDOM" -days 2 -extfile "$3" -out "$1.crt" 2>/dev/null
}
sign int1 root ca.ext
sign int2 int1 ca.ext
sign leaf int2 leaf.ext
cat leaf.crt int2.crt int1.crt > chain.pem
openssl verify -CAfile root.crt -untrusted <(cat int1.crt int2.crt) leaf.crt >/dev/null
# A token-signing key's public half, as the station gives it (87 characters of base64url).
key station
pub=$(openssl ec -in station.key -pubout -outform DER 2>/dev/null | tail -c 65 | basenc --base64url -w0 | tr -d '=')
chmod -R a+rX "$run/pki"; cd "$run"

cat > origin.conf <<'CONF'
events {}
http {
  log_format uri '$request_method $request_uri';
  server {
    listen 443 ssl;
    ssl_certificate /pki/chain.pem;
    ssl_certificate_key /pki/leaf.key;
    access_log /log/origin.log uri;
    location = /health { return 200 "well\n"; }
    location = /hls/live.m3u8 { return 200 "#EXTM3U\n"; }
    location = /api/v1/keyed/revoked { default_type application/json; return 200 '{"revoked": ["k-gone"]}'; }
  }
}
CONF
podman network create "$prefix-net" >/dev/null
podman run -d --name "$prefix-origin" --network "$prefix-net" --network-alias origin.test --security-opt label=disable \
  -v "$run/origin.conf:/etc/nginx/nginx.conf:ro" -v "$run/pki:/pki:ro" -v "$run/log:/log" \
  docker.io/library/nginx:1.27-alpine >/dev/null
sleep 1
podman run -d --name "$prefix-edge" --network "$prefix-net" -p "127.0.0.1:$EDGE_PORT:80" --security-opt label=disable \
  -v "$run/pki/root.crt:/etc/ssl/certs/ca-certificates.crt:ro" \
  -e EDGE_ORIGIN=https://origin.test -e EDGE_KEYED_PUBLIC_KEY="$pub" \
  -e EDGE_KEYED_REVOKED_URL=https://origin.test/api/v1/keyed/revoked -e EDGE_KEYED_REVOKED_EVERY=1s \
  "$image" >/dev/null
for _ in $(seq 1 40); do curl -s -m 2 -o /dev/null "$edge/health" && break; sleep 0.25; done
sleep 4

fail=0
check() { if eval "$2"; then echo "ok    $1"; else echo "FAIL  $1"; fail=1; fi; }
code() { curl -s -m 10 -o /dev/null -w '%{http_code}' "$@"; }
podman logs "$prefix-edge" > edge.log 2>&1
check "/health comes through over HTTPS, two intermediates behind it" '[ "$(code "$edge/health")" = 200 ]'
check "a public playlist comes through" '[ "$(curl -s -m 10 "$edge/hls/live.m3u8")" = "#EXTM3U" ]'
check "a made-up token opens no keyed path" '[ "$(code "$edge/hls/k/show-a/ks1.made.up/live.m3u8")" = 403 ]'
check "the revoked keys' list is fetched from the host name" 'grep -q "GET /api/v1/keyed/revoked" log/origin.log'
check "and read without a complaint" '! grep -q "the revoked list could not be read" edge.log'
check "no certificate was refused" '! grep -q "certificate verify error" edge.log'
if [ "$fail" = 0 ]; then echo "the edge reaches an HTTPS station by name"; else echo "--- the edge's log"; tail -8 edge.log | cut -c1-300; fi
exit "$fail"
