#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# The relay image's encrypted hop (ADR-0162), against a stand-in HTTPS station on loopback with a
# throwaway certificate authority:
#   1. with the authority trusted, the relay carries the stream (Ogg from the relay's own port),
#      and the station saw the listener login and its own name in Host, over TLS;
#   2. the hop answers 404 to any path but the relay's mounts, and listens on loopback only;
#   3. with the authority not trusted, or the certificate for another name, the relay carries
#      nothing and the station sees no request (the certificate is checked).
# Ports 22501 (the station), 22502 (the relay) and 22503 (its hop) by default; no other test's.
#   RELAY_IMAGE=localhost/weavesignal-relay:w-deploy tests/edges/relay-tls.sh
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
run="$here/run-tls"
prefix=${WEAVESIGNAL_TEST_PREFIX:-weavesignal-m1}-relaytls
image=${RELAY_IMAGE:-localhost/weavesignal-relay:m1}
origin=${TLS_ORIGIN_PORT:-22501} relay=${TLS_RELAY_PORT:-22502} hop=$((relay + 1))
login=listener:s3cret-listener-pw
rm -rf -- "$run"; mkdir -p "$run"
srv=''
cleanup() {
  podman rm -f "$prefix" >/dev/null 2>&1 || true
  [ -z "$srv" ] || kill "$srv" 2>/dev/null || true
  rm -rf -- "$run"
}
trap cleanup EXIT
fail() { echo "FAIL  $*" >&2; podman logs "$prefix" 2>&1 | tail -15 >&2 || true; exit 1; }

# A throwaway authority, a certificate for localhost, and one for another name.
openssl req -x509 -newkey rsa:2048 -nodes -keyout "$run/ca.key" -out "$run/ca.pem" -days 2 -subj "/CN=relay-tls test CA" 2>/dev/null
for name in localhost other.invalid; do
  openssl req -newkey rsa:2048 -nodes -keyout "$run/$name.key" -out "$run/$name.csr" -subj "/CN=$name" 2>/dev/null
  printf 'subjectAltName=DNS:%s\n' "$name" > "$run/$name.ext"
  openssl x509 -req -in "$run/$name.csr" -CA "$run/ca.pem" -CAkey "$run/ca.key" -CAcreateserial -out "$run/$name.pem" -days 2 -extfile "$run/$name.ext" 2>/dev/null
done
ffmpeg -v error -f lavfi -i "sine=frequency=440:duration=60" -c:a libopus -b:a 96k -f ogg "$run/tone.opus"
chmod -R a+rX "$run"

serve() { # certificate name
  [ -z "$srv" ] || { kill "$srv" 2>/dev/null || true; wait "$srv" 2>/dev/null || true; }
  : > "$run/requests.log"
  python3 -I "$here/tls-origin.py" "$origin" "$run/$1.pem" "$run/$1.key" "$run/tone.opus" "$login" "$run/requests.log" &
  srv=$!
  for _ in $(seq 1 50); do [ -n "$(ss -Hltn "sport = :$origin")" ] && return; sleep 0.1; done
  fail "the stand-in station did not listen on $origin"
}
start_relay() { # RELAY_ORIGIN_HOST, then "ca" to trust the test authority (copied in: a bind mount
  # under a home directory is unreadable to the image's user under rootless Podman)
  local host=$1 ca=${2:-}
  local args=()
  [ "$ca" != ca ] || args=(-e RELAY_ORIGIN_CA_FILE=/ca.pem)
  podman rm -f "$prefix" >/dev/null 2>&1 || true
  podman create --name "$prefix" --network host -e "RELAY_ORIGIN_HOST=$host" -e "RELAY_ORIGIN_PORT=$origin" \
    -e RELAY_ORIGIN_USER=listener -e RELAY_ORIGIN_PASSWORD="${login#*:}" -e RELAY_MOUNTS=/programme.opus \
    -e "RELAY_PORT=$relay" -e RELAY_BIND=127.0.0.1 "${args[@]}" "$image" >/dev/null
  [ "$ca" != ca ] || podman cp "$run/ca.pem" "$prefix:/ca.pem"
  podman start "$prefix" >/dev/null
}
carried() { # seconds: 0 when the relay's port serves Ogg within the time
  local end=$((SECONDS + $1))
  while [ $SECONDS -lt $end ]; do
    if [ "$(curl -s -m 8 "http://127.0.0.1:$relay/programme.opus" 2>/dev/null | head -c 4 || true)" = OggS ]; then return 0; fi
    sleep 1
  done
  return 1
}

# 1. The authority trusted: the stream arrives, with the login and the station's own name.
serve localhost
start_relay localhost ca
carried 40 || fail "the relay did not carry the stream through the hop"
bytes=$(curl -s -m 10 "http://127.0.0.1:$relay/programme.opus" | wc -c || true)
[ "$bytes" -gt 20000 ] || fail "the relay carried only $bytes bytes in 10 s"
python3 -I - "$run/requests.log" "$login" "$origin" <<'PY'
import base64, json, sys
log, login, port = sys.argv[1:4]
rows = [json.loads(l) for l in open(log) if l.strip()]
assert rows, "the station saw no request"
h = {k.lower(): v for k, v in rows[-1]["headers"].items()}
assert h.get("authorization") == "Basic " + base64.b64encode(login.encode()).decode(), "no listener login: %r" % h
assert h.get("host") == "localhost:" + port, "Host is %r" % h.get("host")
assert "icy-metadata" not in h or h["icy-metadata"] in ("0", ""), "the relay asked for ICY metadata: %r" % h
print("the station saw %d request(s) over TLS, with the listener login and Host %s" % (len(rows), h["host"]))
PY
echo "pass  the relay carries the stream through the hop ($bytes bytes in 10 s), the station sees the login and its own name"

# 2. The hop is the relay's alone: other paths are refused; it listens on loopback only.
[ "$(curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:$hop/other.opus")" = 404 ] || fail "the hop answered a path that is not a mount"
[ "$(curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:$hop/")" = 404 ] || fail "the hop answered /"
[ -z "$(ss -Hltn "sport = :$hop" | awk '{print $4}' | grep -v '^127\.0\.0\.1:')" ] || fail "the hop listens off loopback"
echo "pass  the hop answers 404 to anything but a mount and listens on 127.0.0.1 only"

# 3. A certificate that fails the check: nothing carried, nothing sent.
refused() { # why, then start_relay's arguments
  local why=$1; shift
  : > "$run/requests.log"
  start_relay "$@"
  sleep 8
  ! carried 1 || fail "the relay carried the stream with $why"
  [ ! -s "$run/requests.log" ] || fail "the station saw a request although $why"
  logs=$(podman logs "$prefix" 2>&1)
  grep -qiE "verif|does not match" <<< "$logs" || fail "no certificate error logged for $why"
  echo "pass  $why: nothing carried, no request reached the station"
}
refused "an authority the relay does not trust" localhost
serve other.invalid
refused "a certificate for another name" localhost ca
