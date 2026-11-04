#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# The Cloudron package run locally, with no Cloudron: the image as Cloudron runs it (read-only
# root, /run and /tmp as tmpfs, /app/data a volume, the manifest's memory limit), Cloudron's
# environment simulated, PostgreSQL 16 standing in for the addon and a small SMTP sink for the
# sendmail addon on a private network, and a self-signed certificate in /etc/certs standing in
# for the tls addon. Content is test tones only.
#
#   deploy/cloudron/test-local.sh            # up, check, room test, restart test; leave it running
#   deploy/cloudron/test-local.sh cold [N]   # N cold starts (default 3), each checked with the room test
#   deploy/cloudron/test-local.sh down       # remove this script's containers, network and volume
#
# Settings (every port a setting, ADR-0075); the host ports default to 274xx, inside 27000-27999:
#   IMAGE=localhost/weavesignal-cloudron:0.2.0-local  DESK_IMAGE=localhost/weavesignal-desk-dev:m1
#   HTTP_PORT=27400  LISTEN_PORT=27401  ROOM_PORT=27402  PUBLIC_PORT=27403
#   MEDIA_PORT=27482 (udp)  TURN_TLS_PORT=27449 (tcp)  MEMORY=<the manifest's memoryLimit>
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
IMAGE=${IMAGE:-localhost/weavesignal-cloudron:0.2.0-local}
DESK_IMAGE=${DESK_IMAGE:-localhost/weavesignal-desk-dev:m1}
PG_IMAGE=${PG_IMAGE:-docker.io/library/postgres:16}
HTTP_PORT=${HTTP_PORT:-27400}
LISTEN_PORT=${LISTEN_PORT:-27401}
ROOM_PORT=${ROOM_PORT:-27402}
PUBLIC_PORT=${PUBLIC_PORT:-27403}
MEDIA_PORT=${MEDIA_PORT:-27482}
TURN_TLS_PORT=${TURN_TLS_PORT:-27449}
COMPAT=${COMPAT:-true}
MEMORY=${MEMORY:-$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["memoryLimit"])' "$here/CloudronManifest.json")}
# A new listener hears what the listener process holds, about 20 s behind live (M4).
ROOM_SECONDS=${ROOM_SECONDS:-50}
p=weavesignal-cloudron-local
work=${WORK:-$here/.test-local}

fail() { echo "FAIL: $*" >&2; podman logs "$p-app" > "$work/app.log" 2>&1 || true; exit 1; }
ok() { echo "ok    $*"; }

down() {
    podman rm -f "$p-app" "$p-pg" "$p-tone" "$p-smtp" >/dev/null 2>&1 || true
    podman network rm "$p-net" >/dev/null 2>&1 || true
    podman volume rm "$p-data" >/dev/null 2>&1 || true
}

up() {
    down
    local port
    for port in "$HTTP_PORT" "$LISTEN_PORT" "$ROOM_PORT" "$PUBLIC_PORT" "$TURN_TLS_PORT"; do
        ss -Hltn "sport = :$port" | grep -q . && fail "port $port/tcp is in use on this machine; set it"
    done
    ss -Hlun "sport = :$MEDIA_PORT" | grep -q . && fail "port $MEDIA_PORT/udp is in use; set MEDIA_PORT"
    rm -rf "$work/certs" "$work/smtp"
    mkdir -p "$work/certs" "$work/smtp/mail"
    openssl req -x509 -newkey rsa:2048 -nodes -days 2 -subj "/CN=weavesignal.test" \
        -keyout "$work/certs/tls_key.pem" -out "$work/certs/tls_cert.pem" 2>/dev/null
    chmod 0600 "$work/certs/tls_key.pem"   # root-only in the container, as Cloudron's is

    podman network create "$p-net" >/dev/null
    podman volume create "$p-data" >/dev/null
    pg_password=$(openssl rand -hex 16)
    smtp_password=$(openssl rand -hex 16)
    podman run -d --name "$p-pg" --network "$p-net" --network-alias postgresql \
        -e POSTGRES_USER=weavesignal -e POSTGRES_DB=weavesignal -e "POSTGRES_PASSWORD=$pg_password" \
        "$PG_IMAGE" >/dev/null
    smtp_sink
    podman run -d --name "$p-smtp" --network "$p-net" --network-alias mail \
        -v "$work/smtp:/smtp:z" -e SINK_USER=weavesignal.app -e "SINK_PASS=$smtp_password" \
        --entrypoint python3 "$IMAGE" /smtp/sink.py >/dev/null
    for i in $(seq 1 60); do
        podman exec "$p-pg" psql -U weavesignal -d weavesignal -c 'SELECT 1' >/dev/null 2>&1 && break
        sleep 1; [[ $i == 60 ]] && fail "PostgreSQL did not start"
    done

    # The app, as Cloudron runs it. The public ports keep one number inside and out, as the
    # manifest's (no containerPort) do; NODE_IP is loopback so candidates point at this machine.
    podman run -d --name "$p-app" --network "$p-net" \
        --read-only --tmpfs /run:rw,exec,size=256m --tmpfs /tmp:rw,size=64m \
        -v "$p-data:/app/data" -v "$work/certs:/etc/certs:ro,z" \
        --memory "$MEMORY" --memory-swap "$MEMORY" \
        -p "127.0.0.1:$HTTP_PORT:8000" -p "127.0.0.1:$LISTEN_PORT:8001" \
        -p "127.0.0.1:$ROOM_PORT:8002" -p "127.0.0.1:$PUBLIC_PORT:8003" \
        -p "127.0.0.1:$MEDIA_PORT:$MEDIA_PORT/udp" -p "127.0.0.1:$TURN_TLS_PORT:$TURN_TLS_PORT/tcp" \
        -e CLOUDRON=1 -e CLOUDRON_APP_DOMAIN=weavesignal.test -e CLOUDRON_APP_ORIGIN=https://weavesignal.test \
        -e "CLOUDRON_POSTGRESQL_URL=postgres://weavesignal:$pg_password@postgresql:5432/weavesignal" \
        -e CLOUDRON_POSTGRESQL_HOST=postgresql -e CLOUDRON_POSTGRESQL_PORT=5432 \
        -e CLOUDRON_POSTGRESQL_USERNAME=weavesignal -e "CLOUDRON_POSTGRESQL_PASSWORD=$pg_password" \
        -e CLOUDRON_POSTGRESQL_DATABASE=weavesignal \
        -e CLOUDRON_MAIL_SMTP_SERVER=mail -e CLOUDRON_MAIL_SMTP_PORT=2525 -e CLOUDRON_MAIL_SMTPS_PORT=2465 \
        -e CLOUDRON_MAIL_SMTP_USERNAME=weavesignal.app -e "CLOUDRON_MAIL_SMTP_PASSWORD=$smtp_password" \
        -e CLOUDRON_MAIL_FROM=weavesignal.app@weavesignal.test -e CLOUDRON_MAIL_DOMAIN=weavesignal.test \
        -e LISTENERS_DOMAIN=weavesignal-listen.weavesignal.test -e ROOM_DOMAIN=weavesignal-room.weavesignal.test \
        -e PUBLIC_DOMAIN=weavesignal-public.weavesignal.test \
        -e "LIVEKIT_MEDIA_PORT=$MEDIA_PORT" -e "TURN_TLS_PORT=$TURN_TLS_PORT" \
        "$IMAGE" >/dev/null
    # NODE_IP for this run only: the first start writes weavesignal.env, so set it and restart.
    # The start measured below is that restart: a cold start of every process, the database
    # already migrated.
    for i in $(seq 1 30); do
        podman exec "$p-app" test -f /app/data/weavesignal.env 2>/dev/null && break
        sleep 1; [[ $i == 30 ]] && fail "start.sh wrote no weavesignal.env"
    done
    podman exec "$p-app" sed -i 's/^NODE_IP=$/NODE_IP=127.0.0.1/' /app/data/weavesignal.env
    # The MP3 stream on for the main run (COMPAT=true), off for cold starts, as installed.
    [[ $COMPAT != true ]] || podman exec "$p-app" sed -i 's/^COMPAT_STREAM=false$/COMPAT_STREAM=true/' /app/data/weavesignal.env
    # The stream's password on for the main run, so its refusal is checked; off, the default
    # since 1.0.0, for cold starts.
    [[ $COMPAT != true ]] || podman exec "$p-app" sed -i 's/^LISTENERS_AUTH=off$/LISTENERS_AUTH=on/' /app/data/weavesignal.env
    # A station installed before 0.8.0 has no email keys in its secrets: drop them, so the
    # restart below must add them (the chat refuses to start without its key).
    podman exec "$p-app" sed -i '/EMAIL_KEY/d; /Keys the hash of/d' /app/data/secrets.env
    local ts t_stop
    ts=$(date +%s.%N)
    podman stop -t 10 "$p-app" >/dev/null
    t_stop=$(python3 -c "import time; print(f'{time.time()-$ts:.1f}')")
    t0=$(date +%s.%N)
    podman start "$p-app" >/dev/null

    for i in $(seq 1 120); do
        curl -fsS "http://127.0.0.1:$HTTP_PORT/health" 2>/dev/null | grep -q '"database":"ok"' && break
        sleep 0.5; [[ $i == 120 ]] && { podman logs --tail 60 "$p-app"; fail "health did not turn ok"; }
    done
    t_health=$(python3 -c "import time; print(f'{time.time()-$t0:.1f}')")
    admin=$(secret CONTROL_ADMIN_TOKEN)
    lpass=$(secret LISTENERS_PASSWORD)
    mount=http://127.0.0.1:$LISTEN_PORT/programme.opus
    for i in $(seq 1 240); do
        code=$(curl -s -o /dev/null -w '%{http_code}' -u "listener:$lpass" --max-time 2 "$mount" || true)
        [[ $code == 200 ]] && break
        sleep 0.5; [[ $i == 240 ]] && fail "the mount did not come up (last answer $code)"
    done
    t_mount=$(python3 -c "import time; print(f'{time.time()-$t0:.1f}')")
    ok "the first start's stop took ${t_stop} s; cold start: health ok after ${t_health} s, the listener mount answers after ${t_mount} s"
}

secret() { podman exec "$p-app" sh -c ". /app/data/secrets.env; printf %s \"\$$1\""; }

# A minimal SMTP server standing in for Cloudron's mail: it takes AUTH PLAIN or LOGIN, checks
# the addon's credentials, and keeps each message as a file.
smtp_sink() {
    cat > "$work/smtp/sink.py" <<'PY'
import asyncio, base64, os
USER, PASS = os.environ["SINK_USER"], os.environ["SINK_PASS"]
n = 0
async def session(r, w):
    global n
    def say(s): w.write((s + "\r\n").encode())
    authed = False
    say("220 sink ESMTP")
    while line := await r.readline():
        cmd = line.decode(errors="replace").rstrip("\r\n")
        up = cmd.upper()
        if up.startswith(("EHLO", "HELO")):
            say("250-sink"); say("250-AUTH PLAIN LOGIN"); say("250 8BITMIME")
        elif up.startswith("AUTH PLAIN"):
            parts = cmd.split()
            if len(parts) < 3:
                say("334 "); parts.append((await r.readline()).decode().strip())
            _, u, pw = base64.b64decode(parts[2]).decode().split("\0")
            authed = (u, pw) == (USER, PASS); say("235 ok" if authed else "535 no")
        elif up.startswith("AUTH LOGIN"):
            say("334 VXNlcm5hbWU6"); u = base64.b64decode(await r.readline()).decode()
            say("334 UGFzc3dvcmQ6"); pw = base64.b64decode(await r.readline()).decode()
            authed = (u, pw) == (USER, PASS); say("235 ok" if authed else "535 no")
        elif up.startswith(("MAIL", "RCPT")):
            say("250 ok" if authed else "530 auth first")
        elif up == "DATA":
            say("354 go"); await w.drain(); body = b""
            while (l := await r.readline()) not in (b".\r\n", b".\n", b""):
                body += l
            n += 1
            open(f"/smtp/mail/{n}.eml", "wb").write(body); say("250 queued")
        elif up == "QUIT":
            say("221 bye"); await w.drain(); break
        else:
            say("250 ok")
        await w.drain()
    w.close()
async def main():
    s = await asyncio.start_server(session, "0.0.0.0", 2525)
    async with s:
        await s.serve_forever()
asyncio.run(main())
PY
}

checks() {
    ok "station: $(curl -fsS "http://127.0.0.1:$HTTP_PORT/api/v1/station")"
    code=$(curl -s -o /dev/null -w '%{http_code}' --max-time 3 "$mount" || true)
    [[ $code == 401 ]] || fail "the stream without the password answered $code, not 401"
    ok "stream without the password: 401"
    for path in / /admin/stats /admin/listclients /status-json.xsl /status.json /feed; do
        code=$(curl -s -o /dev/null -w '%{http_code}' -u "listener:$lpass" --max-time 3 "http://127.0.0.1:$LISTEN_PORT$path" || true)
        [[ $code == 404 ]] || fail "listener domain $path answered $code, not 404"
    done
    ok "listener domain serves the mount only (/, /admin/*, /status.json, /feed: 404)"
    code=$(curl -s -o /dev/null -w '%{http_code}' -X PUT -u "listener:$lpass" --max-time 3 "$mount" || true)
    [[ $code == 403 ]] || fail "a source PUT through the listener domain answered $code, not 403"
    ok "no source through the listener domain (PUT: 403)"

    # Nothing but nginx and LiveKit's public ports listens off loopback (ADR-0048 rule 3):
    # the feed (7315, 7316), icecast.server and its /status.json, the chat, the control plane.
    # One exception, named: playout's SRT caller has an ephemeral UDP socket on every address
    # (an outgoing connection's; no port of it is published, and SRT answers only its peer).
    podman exec -i --user cloudron "$p-app" python3 - "$MEDIA_PORT" "$TURN_TLS_PORT" <<'PY' || fail "a service listens off loopback"
import os, re, sys
allowed = {8000, 8001, 8002, 8003, int(sys.argv[1]), int(sys.argv[2])}
owner = {}
for pid in filter(str.isdigit, os.listdir("/proc")):
    try:
        cmd = open(f"/proc/{pid}/cmdline").read().replace("\0", " ").strip()
        for fd in os.listdir(f"/proc/{pid}/fd"):
            m = re.match(r"socket:\[(\d+)\]", os.readlink(f"/proc/{pid}/fd/{fd}"))
            if m:
                owner[m.group(1)] = cmd
    except OSError:
        pass
bad, off, loop_tcp, srt = [], set(), set(), []
for proto, f in (("tcp", "/proc/net/tcp"), ("tcp", "/proc/net/tcp6"), ("udp", "/proc/net/udp"), ("udp", "/proc/net/udp6")):
    for line in open(f).readlines()[1:]:
        cols = line.split()
        addr, port = cols[1].split(":")
        port = int(port, 16)
        if cols[3] != ("0A" if proto == "tcp" else "07"):
            continue          # listening TCP sockets, unconnected UDP sockets
        if addr in ("0100007F", "00000000000000000000000001000000"):
            if proto == "tcp":
                loop_tcp.add(port)
            continue
        who = owner.get(cols[9], "?")
        if port in allowed:
            off.add(port)
        elif proto == "udp" and port >= 32768 and who.endswith("playout/weavesignal.liq"):
            srt.append(port)
        else:
            bad.append(f"{proto}/{port} ({who})")
print(f"ok    off loopback: {sorted(off)}, and playout's SRT caller's ephemeral udp {srt}; on loopback tcp {sorted(loop_tcp)}")
if bad:
    print("FAIL: off loopback: " + ", ".join(bad))
sys.exit(1 if bad else 0)
PY
    # The ports the layout depends on are where they should be.
    podman exec -i "$p-app" python3 - <<'PY' || fail "the feed or the listener port is not on loopback"
lines = open("/proc/net/tcp").read()
for port in (7312, 7315, 7311, 7310):
    assert f"0100007F:{port:04X} 00000000:0000 0A" in lines, port
PY

    curl -s -u "listener:$lpass" -H 'Icy-MetaData: 1' -D "$work/stream.headers" --max-time 6 "$mount" -o "$work/late.opus" || true
    head -c 4 "$work/late.opus" | grep -q OggS || fail "the stream does not start with OggS"
    grep -qi '^icy-metaint' "$work/stream.headers" && fail "the stream offered ICY metadata"
    ffprobe -v error -show_entries stream=codec_name -of csv=p=0 "$work/late.opus" | grep -qx opus \
        || fail "a late listener cannot decode Opus"
    ok "late listener: OggS, Opus decodes ($(stat -c %s "$work/late.opus") bytes in 6 s), no ICY"

    # The public domain: the pages, the chat's and the control plane's public routes, the
    # feeds; never the studio's or the chat's admin routes, nor health.
    pub=http://127.0.0.1:$PUBLIC_PORT
    for path in / /join/ /echo-test/; do
        curl -fsS "$pub$path" | grep -qi '<html' || fail "public page $path is not served"
    done
    ok "public domain: the player, join and echo-test pages"
    for path in /api/v1/chat/info /api/v1/player /api/v1/station /api/v1/now-playing; do
        code=$(curl -s -o /dev/null -w '%{http_code}' --max-time 5 "$pub$path" || true)
        [[ $code == 200 ]] || fail "public $path answered $code, not 200"
    done
    ok "public domain: /api/v1/chat/info, /api/v1/player, /api/v1/station, /api/v1/now-playing answer 200"
    for path in /api/v1/studio /api/v1/chat/admin/holds /api/v1/chat/mod/log /api/v1/retention/setting /api/v1/carts /api/v1/staff /api/v1/sessions/me; do
        code=$(curl -s -o /dev/null -w '%{http_code}' -H "Authorization: Bearer $admin" --max-time 5 "$pub$path" || true)
        [[ $code == 404 ]] || fail "public $path answered $code, not 404"
    done
    ok "public domain: studio, chat admin, retention, carts and staff routes are 404 even with the bearer"
    # The edges' health check passes through to the station (ADR-0160): on the public domain,
    # with no bearer, since a login in front would answer 302.
    body=$(curl -fsS --max-time 5 "$pub/health") || fail "public /health did not answer 200 without a bearer"
    grep -q '"status":"ok"' <<< "$body" || fail "public /health said $body"
    ok "public domain: /health answers 200 with no bearer ($body)"
    # Unlisted by default (ADR-0133): one X-Robots-Tag on every answer, robots.txt disallows all.
    for url in "$pub/" "$pub/join/" "$pub/api/v1/player" "$pub/feeds/no-such-show.xml" "http://127.0.0.1:$HTTP_PORT/api/v1/station"; do
        n=$(curl -s -D - -o /dev/null --max-time 5 "$url" | grep -ci '^x-robots-tag: noindex, nofollow' || true)
        [[ $n == 1 ]] || fail "$url carries $n X-Robots-Tag noindex headers, not 1"
    done
    curl -fsS "$pub/robots.txt" | grep -qx 'Disallow: /' || fail "the public robots.txt does not disallow everything"
    ok "unlisted: one X-Robots-Tag noindex on the pages, the API and the feeds; robots.txt disallows /"
    # The email keys: generated (here, added to a secrets file that lacked them) and long enough.
    for k in CHAT_EMAIL_KEY CONTROL_EMAIL_KEY CONTROL_TOKEN_KEY; do
        v=$(secret "$k"); [[ ${#v} -ge 32 ]] || fail "$k in secrets.env has ${#v} characters"
    done
    ok "secrets: CHAT_EMAIL_KEY, CONTROL_EMAIL_KEY and CONTROL_TOKEN_KEY added to an older secrets file, 64 characters each"
    # An empty room: after the desk's dead air, the stream carries the default loop (ADR-0130),
    # not silence and not the room test's tone.
    python3 - "$mount" "$lpass" "$work/loop.opus" <<'PY' || fail "the default loop is not on the stream"
import subprocess, sys, time
import numpy as np
url, pw, path = sys.argv[1:4]
rms = tone = float("nan")
for attempt in range(8):
    subprocess.run(["curl", "-s", "-u", f"listener:{pw}", "--max-time", "8", url, "-o", path])
    pcm = subprocess.run(["ffmpeg", "-v", "error", "-i", path, "-ac", "1", "-ar", "48000", "-f", "f32le", "-"],
                         capture_output=True).stdout
    x = np.frombuffer(pcm, dtype=np.float32).astype(np.float64)[48000:]
    if len(x) < 48000 * 4:
        continue
    rms = 20 * np.log10(np.sqrt(np.mean(x ** 2)) + 1e-12)
    p = np.abs(np.fft.rfft(x * np.hanning(len(x)))) ** 2
    f = np.fft.rfftfreq(len(x), 1 / 48000)
    tone = p[(f > 980) & (f < 1020)].sum() / (p.sum() + 1e-30)
    if rms > -45 and tone < 0.2:
        print(f"ok    empty room: the default loop is on the stream ({rms:.1f} dBFS, attempt {attempt + 1})")
        sys.exit(0)
    time.sleep(4)
sys.exit(f"FAIL: the stream is {rms:.1f} dBFS with {100 * tone:.0f} % at 1 kHz, not the default loop")
PY
    # The MP3 compatibility stream (ADR-0002), when on: its own mount on the listener domain.
    if [[ $COMPAT == true ]]; then
        mp3=http://127.0.0.1:$LISTEN_PORT/programme.mp3
        code=$(curl -s -o /dev/null -w '%{http_code}' --max-time 3 "$mp3" || true)
        [[ $code == 401 ]] || fail "the MP3 stream without the password answered $code, not 401"
        curl -s -u "listener:$lpass" -D "$work/mp3.headers" --max-time 6 "$mp3" -o "$work/compat.mp3" || true
        ffprobe -v error -show_entries stream=codec_name,sample_rate,bit_rate -of csv=p=0 "$work/compat.mp3" > "$work/mp3.probe" 2>&1 || true
        grep -q '^mp3,44100' "$work/mp3.probe" || fail "the MP3 stream does not decode as MP3 at 44.1 kHz: $(cat "$work/mp3.probe")"
        grep -qi '^icy-metaint' "$work/mp3.headers" && fail "the MP3 stream offered ICY metadata"
        ok "MP3 stream: 401 without the password; $(cat "$work/mp3.probe") ($(stat -c %s "$work/compat.mp3") bytes in 6 s)"
    fi
    ctype=$(curl -s -o /dev/null -w '%{http_code} %{content_type}' "$pub/feeds/no-such-show.xml")
    [[ $ctype == 404\ application/json* ]] || fail "a feed request did not reach the control plane ($ctype)"
    ok "public domain: /feeds/ reaches the control plane (an unknown show: $ctype)"
    code=$(curl -s -o /dev/null -w '%{http_code}' -H "Authorization: Bearer $admin" "http://127.0.0.1:$HTTP_PORT/api/v1/chat/admin/holds")
    [[ $code == 200 ]] || fail "the chat's admin route on the station domain answered $code"
    ok "station domain: the chat's admin routes reach the chat with the bearer (200)"

    # Staff accounts (ADR-0127): the station key makes the first admin, who signs in on the
    # station domain (with the placeholder bearer the studio app sends past Cloudron's login)
    # and is refused nowhere the key is not; sign-in is not served on the public domain.
    code=$(curl -s -o "$work/staff.json" -w '%{http_code}' -X POST "http://127.0.0.1:$HTTP_PORT/api/v1/staff" \
        -H "Authorization: Bearer $admin" -H 'Content-Type: application/json' \
        -d '{"username":"morag","name":"Morag Campbell","role":"admin","password":"a long enough phrase"}')
    [[ $code == 201 ]] || fail "the station key could not make the first admin ($code): $(cat "$work/staff.json")"
    session=$(curl -fsS -X POST "http://127.0.0.1:$HTTP_PORT/api/v1/sessions" -H 'Authorization: Bearer sign-in' \
        -H 'Content-Type: application/json' -d '{"username":"morag","password":"a long enough phrase"}' \
        | python3 -c 'import json,sys; print(json.load(sys.stdin)["token"])') || fail "the first admin could not sign in"
    [[ $session == rss_* ]] || fail "the sign-in answered no session"
    curl -fsS -H "Authorization: Bearer $session" "http://127.0.0.1:$HTTP_PORT/api/v1/sessions/me" | grep -q '"role":"admin"' \
        || fail "the session is not the admin's"
    code=$(curl -s -o /dev/null -w '%{http_code}' -H "Authorization: Bearer $session" "http://127.0.0.1:$HTTP_PORT/api/v1/chat/admin/holds")
    [[ $code == 200 ]] || fail "the admin's session was refused by the chat ($code)"
    code=$(curl -s -o /dev/null -w '%{http_code}' -X POST "$pub/api/v1/sessions" -H 'Content-Type: application/json' \
        -d '{"username":"morag","password":"a long enough phrase"}')
    [[ $code == 404 ]] || fail "sign-in on the public domain answered $code, not 404"
    ok "staff: the station key makes the first admin, who signs in on the station domain and reaches the chat; no sign-in on the public domain"

    # Booths (ADR-0193): a caller's token names a room of their own, never the studio room.
    version=$(curl -fsS "$pub/api/v1/player" | python3 -c 'import json,sys; print(json.load(sys.stdin)["call_in"]["consent_version"])') \
        || fail "the player gave no consent version"
    booth=$(curl -fsS -X POST "http://127.0.0.1:$HTTP_PORT/api/v1/room/tokens" -H "Authorization: Bearer $admin" \
        -H 'Content-Type: application/json' \
        -d "{\"identity\":\"booth-check\",\"role\":\"caller\",\"name\":\"Ailsa\",\"consent\":{\"accepted\":true,\"version\":\"$version\"}}" \
        | python3 -c 'import json,sys; print(json.load(sys.stdin)["room"])') || fail "no caller token"
    [[ $booth == *-booth-booth-check ]] || fail "a caller's token names room $booth, not their booth"
    ok "booths: a caller's token names their own room ($booth)"

    # The chat's sign-in code goes out through the sendmail addon (msmtp to the sink).
    code=$(curl -s -o "$work/signin.json" -w '%{http_code}' -X POST "$pub/api/v1/chat/signin/start" \
        -H 'content-type: application/json' -d '{"email":"ishbel.tone@weavesignal.test"}')
    [[ $code == 2* ]] || fail "chat sign-in answered $code: $(cat "$work/signin.json")"
    for i in $(seq 1 20); do ls "$work/smtp/mail/"*.eml >/dev/null 2>&1 && break; sleep 0.5; done
    eml=$(ls "$work/smtp/mail/"*.eml 2>/dev/null | head -1)
    [[ -n $eml ]] || fail "no sign-in email reached the SMTP sink"
    grep -q '^To: ishbel.tone@weavesignal.test' "$eml" && grep -q '^From: weavesignal.app@weavesignal.test' "$eml" \
        || fail "the sign-in email has the wrong To or From"
    ok "chat sign-in email through msmtp and the addon's SMTP, authenticated: $(grep -m1 '^Subject:' "$eml")"
}

start_tone() {
    token=$(curl -fsS -X POST "http://127.0.0.1:$HTTP_PORT/api/v1/room/tokens" \
        -H "Authorization: Bearer $admin" -H 'content-type: application/json' \
        -d '{"identity":"tone-guest","name":"Ishbel Tone","role":"guest","subscribe":false}' \
        | python3 -c 'import json,sys; print(json.load(sys.stdin)["token"])')
    # The guest runs in the app's network namespace: rootless Podman does not carry WebRTC's
    # UDP from the host into the container the way Docker's published ports do on a Cloudron,
    # so this tests the stack and nginx's room proxy, not the port mapping.
    podman rm -f -t 0 "$p-tone" >/dev/null 2>&1 || true
    podman run -d --name "$p-tone" --network "container:$p-app" "$DESK_IMAGE" gst-launch-1.0 -q \
        audiotestsrc is-live=true wave=sine freq=1000 volume=0.3 ! audioconvert ! audioresample \
        ! livekitwebrtcsink name=s signaller::ws-url="ws://127.0.0.1:8002" \
          signaller::auth-token="$token" signaller::room-name=studio >/dev/null
}

# The room test: a guest publishes a 1 kHz tone through the room domain with a publish-only
# token from the control plane (ADR-0090); the listener stream must carry the tone.
room_test() {
    start_tone
    ok "room token issued (guest, publish-only); the tone guest publishes"
    sleep 8
    curl -s -u "listener:$lpass" --max-time "$ROOM_SECONDS" "$mount" -o "$work/room.opus" || true
    python3 - "$work/room.opus" <<'PY' || fail "the room test"
import subprocess, sys
import numpy as np
pcm = subprocess.run(["ffmpeg", "-v", "error", "-i", sys.argv[1], "-ac", "1", "-ar", "48000",
                      "-f", "f32le", "-"], capture_output=True, check=True).stdout
x = np.frombuffer(pcm, dtype=np.float32).astype(np.float64)
seconds = len(x) / 48000
if len(x) < 48000 * 10:
    sys.exit(f"FAIL: only {seconds:.1f} s of programme")
level = [20 * np.log10(np.sqrt(np.mean(x[i:i + 48000] ** 2)) + 1e-12) for i in range(0, len(x) - 47999, 48000)]
onset = next((i for i, l in enumerate(level) if l > -40), None)
win = x[-48000 * 6:]
rms = 20 * np.log10(np.sqrt(np.mean(win ** 2)) + 1e-12)
power = np.abs(np.fft.rfft(win * np.hanning(len(win)))) ** 2
freqs = np.fft.rfftfreq(len(win), 1 / 48000)
share = power[(freqs > 980) & (freqs < 1020)].sum() / (power.sum() + 1e-30)
if rms < -40 or share < 0.5:
    sys.exit(f"FAIL: no tone in the programme (level {rms:.1f} dBFS, 1 kHz share {share:.2f})")
print(f"ok    room test: the guest's 1 kHz tone is in the listener stream, programme {rms:.1f} dBFS, "
      f"{100 * share:.0f} % of its power at 1 kHz; heard from {onset} s into {seconds:.1f} s recorded")
PY
}

# Playout killed (by its PID) while a listener stays connected: the listener process holds the
# mount, so the same connection hears at most a short gap (the fallback, if playout is away
# long enough), then the programme again (M4).
restart_test() {
    [[ -n $(podman ps -q --filter "name=^$p-tone$") ]] || start_tone
    local listen_s=110 kill_at=45
    curl -s -u "listener:$lpass" --max-time "$listen_s" -o "$work/restart.opus" \
        -w '%{http_code} %{num_connects} %{size_download}\n' "$mount" > "$work/restart.curl" 2>&1 &
    local curl_pid=$! t_listen
    t_listen=$(date +%s.%N)
    sleep "$kill_at"
    local before after listeners_before listeners_after
    before=$(podman exec "$p-app" supervisorctl -c /app/code/supervisord.conf pid playout)
    listeners_before=$(podman exec "$p-app" supervisorctl -c /app/code/supervisord.conf pid listeners)
    podman exec "$p-app" kill -9 "$before"
    local t_kill
    t_kill=$(python3 -c "import time; print(f'{time.time()-$t_listen:.1f}')")
    for i in $(seq 1 60); do
        after=$(podman exec "$p-app" supervisorctl -c /app/code/supervisord.conf pid playout)
        [[ $after != 0 && $after != "$before" ]] && break
        sleep 0.5
    done
    local t_back
    t_back=$(python3 -c "import time; print(f'{time.time()-$t_listen:.1f}')")
    listeners_after=$(podman exec "$p-app" supervisorctl -c /app/code/supervisord.conf pid listeners)
    wait "$curl_pid" || true
    podman rm -f -t 0 "$p-tone" >/dev/null 2>&1 || true
    ok "playout (PID $before) killed ${t_kill} s into the listen; supervisord had it running again (PID $after) by ${t_back} s; listener process PID $listeners_before then $listeners_after"
    [[ $listeners_before == "$listeners_after" ]] || fail "the listener process restarted"
    read -r code connects bytes < "$work/restart.curl"
    [[ $code == 200 && $connects == 1 ]] || fail "the listener's connection: HTTP $code, $connects connections"
    python3 - "$work/restart.opus" "$listen_s" "$bytes" <<'PY' || fail "the restart test"
import subprocess, sys
import numpy as np
path, listen_s, nbytes = sys.argv[1], float(sys.argv[2]), int(sys.argv[3])
dec = subprocess.run(["ffmpeg", "-v", "error", "-i", path, "-ac", "1", "-ar", "48000", "-f", "f32le", "-"],
                     capture_output=True)
errors = dec.stderr.decode().strip()
x = np.frombuffer(dec.stdout, dtype=np.float32).astype(np.float64)
seconds = len(x) / 48000
w = 4800   # 100 ms windows
tone = []
for i in range(0, len(x) - w + 1, w):
    seg = x[i:i + w]
    rms = np.sqrt(np.mean(seg ** 2)) + 1e-12
    p = np.abs(np.fft.rfft(seg * np.hanning(w))) ** 2
    f = np.fft.rfftfreq(w, 1 / 48000)
    share = p[(f > 950) & (f < 1050)].sum() / (p.sum() + 1e-30)
    tone.append(20 * np.log10(rms) > -40 and share > 0.5)
first = next((i for i, t in enumerate(tone) if t), None)
if first is None:
    sys.exit("FAIL: the tone never reached the listener")
gaps, i = [], first
while i < len(tone):
    if not tone[i]:
        j = i
        while j < len(tone) and not tone[j]:
            j += 1
        if j - i >= 1:
            gaps.append((i / 10, (j - i) / 10, j < len(tone)))
        i = j
    else:
        i += 1
print(f"ok    one connection for {listen_s:.0f} s: {nbytes} bytes, {seconds:.1f} s decoded"
      f"{' with decoder errors: ' + errors[:200] if errors else ', no decoder errors'}; tone from {first / 10:.1f} s")
for at, length, back in gaps:
    print(f"ok    a gap in the tone at {at:.1f} s of the stream, {length:.1f} s long, "
          f"{'then the programme again' if back else 'to the end of the listen'}")
# Since 0.8.0 the listener process decodes the feed natively and playout is back within about
# 0.3 s, so the gap may be a single 100 ms window (or none at all): at most one, and the
# programme back after it.
if len(gaps) > 1 or (gaps and not gaps[0][2]):
    sys.exit(f"FAIL: expected at most one gap with the programme back after it, saw {gaps}")
if errors:
    sys.exit("FAIL: the stream did not decode cleanly")
PY
}

faults() {
    podman logs "$p-app" > "$work/app.log" 2>&1 || true
    local dts gen epipe oom
    dts=$(grep -c 'non-monotonic DTS' "$work/app.log" || true)
    gen=$(grep -c 'Generator max buffered length exceeded' "$work/app.log" || true)
    epipe=$(grep -c 'EPIPE' "$work/app.log" || true)
    oom=$(podman exec "$p-app" sh -c 'grep "^oom_kill " /sys/fs/cgroup/memory.events' | awk '{print $2}')
    ok "faults in the log: non-monotonic DTS $dts, Generator max buffered length exceeded $gen, EPIPE $epipe; oom_kill $oom"
    [[ $dts == 0 && $gen == 0 && $oom == 0 ]] || fail "a fault was logged"
}

mkdir -p "$work"
case ${1:-} in
    down) down; rm -rf "$work"; exit 0 ;;
    cold)
        n=${2:-3}
        COMPAT=false
        for run in $(seq 1 "$n"); do
            echo "--- cold start $run of $n"
            up; room_test; podman rm -f -t 0 "$p-tone" >/dev/null; faults
            cp "$work/app.log" "$work/app-cold-$run.log"
        done
        ;;
    *)
        up; checks; room_test; restart_test; faults
        ;;
esac
echo "ok    running: http://127.0.0.1:$HTTP_PORT (public pages http://127.0.0.1:$PUBLIC_PORT; stop with: $0 down)"
