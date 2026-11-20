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
#   SOURCE_ICE_PORT=27405 (tcp)  SOURCE_SRT_PORT=27410 (udp): hosts' own studios (1.1), enabled
#   SOURCE_WHIP_PORT=27412 (udp): WHIP from OBS (17d), enabled
#   VIDEO=false  (true: the video path on, ADR-0227: the HLS, the camera check, a guest's camera)
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
SOURCE_ICE_PORT=${SOURCE_ICE_PORT:-27405}
SOURCE_SRT_PORT=${SOURCE_SRT_PORT:-27410}
SOURCE_WHIP_PORT=${SOURCE_WHIP_PORT:-27412}
COMPAT=${COMPAT:-true}
VIDEO=${VIDEO:-false}
MEMORY=${MEMORY:-$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["memoryLimit"])' "$here/CloudronManifest.json")}
# A new listener hears what the listener process holds, about 20 s behind live (M4).
ROOM_SECONDS=${ROOM_SECONDS:-50}
p=weavesignal-cloudron-local
work=${WORK:-$here/.test-local}

fail() { echo "FAIL: $*" >&2; podman logs "$p-app" > "$work/app.log" 2>&1 || true; exit 1; }
ok() { echo "ok    $*"; }

down() {
    podman rm -f "$p-app" "$p-pg" "$p-tone" "$p-cam" "$p-smtp" >/dev/null 2>&1 || true
    podman network rm "$p-net" >/dev/null 2>&1 || true
    podman volume rm "$p-data" >/dev/null 2>&1 || true
}

up() {
    down
    local port
    for port in "$HTTP_PORT" "$LISTEN_PORT" "$ROOM_PORT" "$PUBLIC_PORT" "$TURN_TLS_PORT" "$SOURCE_ICE_PORT"; do
        ss -Hltn "sport = :$port" | grep -q . && fail "port $port/tcp is in use on this machine; set it"
    done
    ss -Hlun "sport = :$MEDIA_PORT" | grep -q . && fail "port $MEDIA_PORT/udp is in use; set MEDIA_PORT"
    ss -Hlun "sport = :$SOURCE_SRT_PORT" | grep -q . && fail "port $SOURCE_SRT_PORT/udp is in use; set SOURCE_SRT_PORT"
    ss -Hlun "sport = :$SOURCE_WHIP_PORT" | grep -q . && fail "port $SOURCE_WHIP_PORT/udp is in use; set SOURCE_WHIP_PORT"
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
        --memory "$MEMORY" --memory-swap "$MEMORY" --ulimit nofile=1024:524288 \
        -p "127.0.0.1:$HTTP_PORT:8000" -p "127.0.0.1:$LISTEN_PORT:8001" \
        -p "127.0.0.1:$ROOM_PORT:8002" -p "127.0.0.1:$PUBLIC_PORT:8003" \
        -p "127.0.0.1:$MEDIA_PORT:$MEDIA_PORT/udp" -p "127.0.0.1:$TURN_TLS_PORT:$TURN_TLS_PORT/tcp" \
        -p "127.0.0.1:$SOURCE_ICE_PORT:$SOURCE_ICE_PORT/tcp" -p "127.0.0.1:$SOURCE_SRT_PORT:$SOURCE_SRT_PORT/udp" \
        -p "127.0.0.1:$SOURCE_WHIP_PORT:$SOURCE_WHIP_PORT/udp" \
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
        -e "SOURCE_ICECAST_PORT=$SOURCE_ICE_PORT" -e "SOURCE_SRT_PORT=$SOURCE_SRT_PORT" -e "SOURCE_WHIP_PORT=$SOURCE_WHIP_PORT" \
        "$IMAGE" >/dev/null
    # NODE_IP for this run only: the first start writes weavesignal.env, so set it and restart.
    # The start measured below is that restart: a cold start of every process, the database
    # already migrated.
    for i in $(seq 1 30); do
        podman exec "$p-app" test -f /app/data/weavesignal.env 2>/dev/null && break
        sleep 1; [[ $i == 30 ]] && fail "start.sh wrote no weavesignal.env"
    done
    podman exec "$p-app" sed -i 's/^NODE_IP=$/NODE_IP=127.0.0.1/' /app/data/weavesignal.env
    # Both MP3 streams on for the main run (COMPAT=true), off for cold starts, as installed.
    [[ $COMPAT != true ]] || podman exec "$p-app" sed -i 's/^COMPAT_STREAM=false$/COMPAT_STREAM=true/; s/^COMPAT_HIGH_STREAM=false$/COMPAT_HIGH_STREAM=true/' /app/data/weavesignal.env
    # The video path (ADR-0227), off as installed; VIDEO=true turns it on for this run.
    [[ $VIDEO != true ]] || podman exec "$p-app" sed -i 's/^VIDEO=false$/VIDEO=true/' /app/data/weavesignal.env
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
    # Cloudron gives the container a soft open-file limit of 1,024 (the --ulimit above, as
    # measured on a Cloudron on 2026-11-06); supervisord's minfds must lift it for the listener
    # process, or the stream is capped below 1,024 listeners.
    nofile=$(podman exec "$p-app" sh -c 'grep "Max open files" /proc/$(pgrep -f listeners.liq | head -1)/limits' | awk '{print $4}')
    [[ ${nofile:-0} -ge 65536 ]] || fail "the listener process may open ${nofile:-?} files, not 65536"
    ok "the listener process may open $nofile files"
    # ADR-0290: no STUN server unless the station names one. LiveKit has an address of its own and
    # never asks for one (use_external_ip would ask Twilio's and Google's), the desk and the
    # control plane name no server, and the name this test gives (no A record anywhere) was not
    # looked up again: the run's NODE_IP is loopback.
    lk=$(podman exec "$p-app" cat /run/weavesignal/livekit.yaml)
    grep -q '^  use_external_ip: false$' <<<"$lk" && grep -q '^  node_ip: 127.0.0.1$' <<<"$lk" && grep -q '^  stun_servers: \[\]$' <<<"$lk" \
        || fail "LiveKit's settings ask a STUN server, or have no address of their own: $lk"
    podman exec "$p-app" grep -qx 'stun_servers = ""' /run/weavesignal/desk.toml \
        && podman exec "$p-app" grep -qx 'stun_servers = ""' /run/weavesignal/control.toml \
        || fail "the desk's or the control plane's settings name a STUN server by default"
    podman exec "$p-app" grep -qx 'congestion_control = "gcc"' /run/weavesignal/desk.toml \
        || fail "the desk's settings lack congestion_control = \"gcc\""
    ok "no STUN server: LiveKit asks none for its address (use_external_ip false, node_ip 127.0.0.1), and the desk and the control plane name none"
    code=$(curl -s -o /dev/null -w '%{http_code}' --max-time 3 "$mount" || true)
    [[ $code == 401 ]] || fail "the stream without the password answered $code, not 401"
    ok "stream without the password: 401"
    root=$(curl -s -o /dev/null -w '%{http_code} %{redirect_url}' --max-time 3 "http://127.0.0.1:$LISTEN_PORT/" || true)
    [[ $root == "302 https://weavesignal-public.weavesignal.test/" ]] || fail "the listener domain's / answered '$root', not a 302 to the public domain"
    ok "listener domain: / sends a visitor to the player on the public domain (302)"
    for path in /admin/stats /admin/listclients /status-json.xsl /status.json /feed; do
        code=$(curl -s -o /dev/null -w '%{http_code}' -u "listener:$lpass" --max-time 3 "http://127.0.0.1:$LISTEN_PORT$path" || true)
        [[ $code == 404 ]] || fail "listener domain $path answered $code, not 404"
    done
    ok "listener domain serves the mount only (/admin/*, /status.json, /feed: 404)"
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
    # A page is revalidated on every load and its hashed files kept for good: a page cached across
    # an update named code files the update had removed (the talk station, 2026-11-11).
    asset=$(curl -fsS "$pub/join/" | grep -oE '/assets/[^"]+\.js' | head -1)
    [[ -n $asset ]] || fail "the join page names no /assets/ file"
    for base in "$pub" "http://127.0.0.1:$HTTP_PORT"; do
        for path in / /join/ /echo-test/; do
            curl -s -D - -o /dev/null --max-time 5 "$base$path" | grep -qi '^cache-control: no-cache' \
                || fail "$base$path is not served no-cache"
        done
        curl -s -D - -o /dev/null --max-time 5 "$base$asset" | grep -qiE '^cache-control: max-age=[0-9]{8,}' \
            || fail "$base$asset is not cached for long"
        code=$(curl -s -o /dev/null -w '%{http_code}' --max-time 5 "$base/assets/no-such-file.js" || true)
        [[ $code == 404 ]] || fail "$base: a missing /assets/ file answered $code, not 404"
    done
    ok "caching: the pages no-cache, $asset kept for long, a missing asset 404, on both domains"
    # The front page (1.1): a station that still has the name the install gave it (This station)
    # and whose setup page has never been finished shows the not-set-up page at the app's own
    # address, still at `/`, and the player on the public domain, which is never changed. Naming
    # it on the setup page ends it at once (no restart); clearing the name brings it back.
    app=http://127.0.0.1:$HTTP_PORT
    title() { curl -fsS --max-time 5 "$1" | grep -o '<title>[^<]*</title>' | head -1; }
    [[ $(title "$app/") == '<title>Not set up yet</title>' ]] || fail "an unnamed station's own address is not the not-set-up page: $(title "$app/")"
    [[ $(title "$pub/") == '<title>Listen</title>' ]] || fail "the public domain's / is not the player: $(title "$pub/")"
    code=$(curl -s -o /dev/null -w '%{http_code}' --max-time 5 "$pub/not-set-up/" || true)
    [[ $code == 404 ]] || fail "the public domain serves /not-set-up/ ($code)"
    code=$(curl -s -o /dev/null -w '%{http_code}' --max-time 5 "$pub/api/v1/front" || true)
    [[ $code == 404 ]] || fail "the public domain passes /api/v1/front ($code)"
    curl -fsS -X PUT -H "Authorization: Bearer $admin" -H 'Content-Type: application/json' -d '{"name":"Test Radio"}' --max-time 5 "$app/api/v1/setup" >/dev/null || fail "the setup page did not take a name"
    [[ $(title "$app/") == '<title>Listen</title>' ]] || fail "a named station's own address is not the player: $(title "$app/")"
    curl -fsS -X PUT -H "Authorization: Bearer $admin" -H 'Content-Type: application/json' -d '{"name":""}' --max-time 5 "$app/api/v1/setup" >/dev/null || fail "the setup page did not give the name back"
    [[ $(title "$app/") == '<title>Not set up yet</title>' ]] || fail "the name given back did not bring the not-set-up page back"
    ok "the app's own domain shows the not-set-up page until the station is named (the public domain keeps the player and 404s /not-set-up/)"
    for path in /api/v1/chat/info /api/v1/player /api/v1/station /api/v1/now-playing; do
        code=$(curl -s -o /dev/null -w '%{http_code}' --max-time 5 "$pub$path" || true)
        [[ $code == 200 ]] || fail "public $path answered $code, not 200"
    done
    ok "public domain: /api/v1/chat/info, /api/v1/player, /api/v1/station, /api/v1/now-playing answer 200"
    [[ $(curl -fsS "$pub/api/v1/player" | python3 -c 'import json,sys; print(json.load(sys.stdin)["relay"]["stun_servers"])') == '[]' ]] \
        || fail "the player is told of a STUN server by default"
    ok "the player's relay names no STUN server (the pages then drop every one LiveKit sends)"
    # nginx passes none of the server's proxy headers to a location that sets its own, so a
    # location that does must restate the client's address, or every chatter and caller counts
    # as 127.0.0.1 against the per-address limits (found 2026-11-17). Only the station's and the
    # public domain's servers (8000, 8003) are checked: the stream and LiveKit keep no such limit.
    bad=$(podman exec "$p-app" awk '/listen /{skip=1} /listen 80(00|03)[ ;]/{skip=0} /location /{loc=$0; has=0; xff=0} /proxy_set_header/{has=1} /X-Forwarded-For/{xff=1} /^ *}$/{ if (loc!="" && has && !xff && !skip) print loc; loc="" }' /run/weavesignal/nginx.conf)
    [[ -z $bad ]] || fail "proxy locations that drop the client's address: $bad"
    ok "every proxy location on the station and public domains passes the client's address"
    # The studio's chat socket (1.1.0): a socket cannot send the bearer, so on a real Cloudron the
    # studio opens it on this public domain (the studio's own domain would redirect to the login).
    # curl stops waiting for frames after --max-time, so only the status line is read.
    code=$(curl -s -o /dev/null -w '%{http_code}' --max-time 3 \
        -H 'Connection: Upgrade' -H 'Upgrade: websocket' -H 'Sec-WebSocket-Version: 13' \
        -H 'Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==' "$pub/api/v1/chat/socket" || true)
    [[ $code == 101 ]] || fail "a WebSocket upgrade to /api/v1/chat/socket on the public domain answered $code, not 101"
    ok "public domain: a WebSocket upgrade to /api/v1/chat/socket answers 101 (the studio's chat socket opens here)"
    # Calendar files (ADR-0281): the schedule as .ics, with no bearer.
    headers=$(curl -fsS -D - -o /dev/null --max-time 5 "$pub/api/v1/public/schedule.ics") \
        || fail "public /api/v1/public/schedule.ics did not answer 200"
    grep -qi '^content-type: text/calendar' <<< "$headers" || fail "public schedule.ics is not text/calendar"
    code=$(curl -s -o /dev/null -w '%{http_code}' -X POST --max-time 5 "$pub/api/v1/public/schedule.ics" || true)
    [[ $code == 405 || $code == 403 ]] || fail "public schedule.ics took a POST ($code)"
    ok "public domain: /api/v1/public/schedule.ics answers as a calendar, GET only"
    # The read-only JSON for a station's own website (ADR-0282), GET only.
    for path in /api/v1/public/station /api/v1/public/schedule /api/v1/public/episodes; do
        code=$(curl -s -o /dev/null -w '%{http_code}' --max-time 5 "$pub$path" || true)
        [[ $code == 200 ]] || fail "public $path answered $code, not 200"
        code=$(curl -s -o /dev/null -w '%{http_code}' -X POST --max-time 5 "$pub$path" || true)
        [[ $code == 405 || $code == 403 ]] || fail "public $path took a POST ($code)"
    done
    ok "public domain: /api/v1/public/station, schedule and episodes answer 200, GET only"
    for path in /api/v1/studio /api/v1/chat/admin/holds /api/v1/chat/mod/log /api/v1/chat/mod/avatars/00000000-0000-0000-0000-000000000000 /api/v1/retention/setting /api/v1/carts /api/v1/staff /api/v1/sessions/me; do
        code=$(curl -s -o /dev/null -w '%{http_code}' -H "Authorization: Bearer $admin" --max-time 5 "$pub$path" || true)
        [[ $code == 404 ]] || fail "public $path answered $code, not 404"
    done
    ok "public domain: studio, chat admin, retention, carts and staff routes are 404 even with the bearer"
    # The edges' health check passes through to the station (ADR-0160): on the public domain,
    # with no bearer, since a login in front would answer 302.
    body=$(curl -fsS --max-time 5 "$pub/health") || fail "public /health did not answer 200 without a bearer"
    grep -q '"status":"ok"' <<< "$body" || fail "public /health said $body"
    ok "public domain: /health answers 200 with no bearer ($body)"
    # The studio's restart of playout (ADR-0289, ⌃R twice): playout shuts down cleanly and the
    # package's supervisord (autorestart=true) starts it again; the mount keeps answering.
    playout_pid() { podman exec "$p-app" pgrep -f 'playout/weavesignal.liq' | head -1 || true; }
    before=$(playout_pid)
    [[ -n $before ]] || fail "no playout process to restart"
    answer=$(curl -fsS -X POST -H "Authorization: Bearer $admin" --max-time 5 "http://127.0.0.1:$HTTP_PORT/api/v1/playout/restart") \
        || fail "POST /api/v1/playout/restart was refused or did not answer"
    grep -q '"restarting":true' <<< "$answer" || fail "the restart route said $answer"
    for i in $(seq 1 60); do
        now=$(playout_pid)
        [[ -n $now && $now != "$before" ]] && break
        sleep 1; [[ $i == 60 ]] && fail "playout did not come back as a new process after the studio's restart (was $before)"
    done
    for i in $(seq 1 60); do
        code=$(curl -s -o /dev/null -w '%{http_code}' -u "listener:$lpass" --max-time 3 "$mount" || true)
        [[ $code == 200 ]] && break
        sleep 1; [[ $i == 60 ]] && fail "the mount answered $code after playout's restart"
    done
    ok "playout restart: the studio's route shut it down and supervisord started it again (pid $before -> $now; the mount answers)"
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
    # The MP3 streams (ADR-0002, ADR-0232), when on: each its own mount on the listener domain,
    # at its own rate (128 and 320 kbit/s as installed).
    if [[ $COMPAT == true ]]; then
        for m in programme.mp3:128000 programme-high.mp3:320000; do
            mp3=http://127.0.0.1:$LISTEN_PORT/${m%%:*}
            code=$(curl -s -o /dev/null -w '%{http_code}' --max-time 3 "$mp3" || true)
            [[ $code == 401 ]] || fail "the MP3 stream ${m%%:*} without the password answered $code, not 401"
            curl -s -u "listener:$lpass" -D "$work/mp3.headers" --max-time 6 "$mp3" -o "$work/compat.mp3" || true
            ffprobe -v error -show_entries stream=codec_name,sample_rate,bit_rate -of csv=p=0 "$work/compat.mp3" > "$work/mp3.probe" 2>&1 || true
            grep -q "^mp3,44100,${m##*:}" "$work/mp3.probe" || fail "the MP3 stream ${m%%:*} does not decode as MP3 at 44.1 kHz and ${m##*:} bit/s: $(cat "$work/mp3.probe")"
            grep -qi '^icy-metaint' "$work/mp3.headers" && fail "the MP3 stream ${m%%:*} offered ICY metadata"
            ok "MP3 stream ${m%%:*}: 401 without the password; $(cat "$work/mp3.probe") ($(stat -c %s "$work/compat.mp3") bytes in 6 s)"
        done
        # The player offers all three, Opus first.
        curl -fs "$pub/api/v1/player" | python3 -I -c 'import json,sys; s=[(x["codec"],x["kbps"]) for x in json.load(sys.stdin)["streams"]]; sys.exit(0 if s==[("opus",96),("mp3",128),("mp3",320)] else f"the player offers {s}")' \
            || fail "the player does not offer Opus and both MP3 streams"
        ok "the player offers Opus 96, MP3 128 and MP3 320"
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

# Hosts in their own studios (1.1, ADR-0280): the ingest under supervisord, on the two ports the
# dashboard enables. Before any key it listens for nothing; the ingest's own routes are closed
# through nginx; with a show on air and a key, ffmpeg as a host's software sends a short MP3 tone
# over Icecast and AAC in MPEG-TS over SRT, each stands by (STANDBY) and is heard at the desk; a
# wrong key is refused, and the address the ingest saw is said.
sources_checks() {
    local api=http://127.0.0.1:$HTTP_PORT pub=http://127.0.0.1:$PUBLIC_PORT
    a() { curl -fsS -X "$1" -H "Authorization: Bearer $admin" -H 'Content-Type: application/json' "$api$2" ${3:+-d "$3"}; }
    podman logs "$p-app" 2>&1 | grep 'no show key yet: not listening' >/dev/null || fail "the ingest did not reach the control plane, or listens without a key"
    ok "the ingest runs under supervisord and listens for nothing without a show key"
    local token code
    token=$(secret INGEST_TOKEN)
    [[ ${#token} == 64 ]] || fail "secrets.env has no INGEST_TOKEN"
    for base in "$api" "$pub"; do
        code=$(curl -s -o /dev/null -w '%{http_code}' -H "Authorization: Bearer $token" "$base/api/v1/internal/sources/config")
        [[ $code == 404 ]] || fail "the ingest's route through nginx ($base) answered $code, not 404"
    done
    ok "the ingest's routes are closed through nginx on the station and public domains (404, with its own bearer)"

    # A show on air now (from ten minutes ago, for an hour), and a key of each kind for it.
    local air_at
    air_at="$(TZ=Europe/London date -d "@$(( $(date +%s) - 600 ))" +%Y-%m-%dT%H:%M:%S%:z)[Europe/London]"
    a PUT /api/v1/shows/steeped '{"name":"Steeped","colour_slot":3}' >/dev/null || fail "no show"
    # At Full, so the guests of the room test after this are not refused by the show's step.
    a PUT /api/v1/shows/steeped/step '{"step":"full"}' >/dev/null || fail "the show's step"
    a PUT /api/v1/shows/steeped/series/1 '{}' >/dev/null || fail "no series"
    a POST /api/v1/shows/steeped/series/1/episodes '{"count":1}' >/dev/null || fail "no episode"
    a PATCH /api/v1/shows/steeped/series/1/episodes/1 "{\"air_at\":\"$air_at\",\"minutes\":60}" >/dev/null || fail "the episode's time"
    local ice srt ice_id ice_pass srt_id srt_secret
    ice=$(a POST /api/v1/shows/steeped/keys '{"label":"Finlay Grant","hint":"icecast"}') || fail "no Icecast key"
    srt=$(a POST /api/v1/shows/steeped/keys '{"label":"Heather Douglas","hint":"srt"}') || fail "no SRT key"
    j() { python3 -c 'import json,sys; v=json.loads(sys.argv[1]);
for k in sys.argv[2].split("."): v=v[k]
print(v)' "$1" "$2"; }
    ice_id=$(j "$ice" key.key_id); ice_pass=$(j "$ice" details.icecast.password)
    srt_id=$(j "$srt" key.key_id); srt_secret=$(j "$srt" secret)
    [[ $(j "$ice" details.icecast.port) == "$SOURCE_ICE_PORT" && $(j "$srt" details.srt.port) == "$SOURCE_SRT_PORT" ]] \
        || fail "a key's details do not give the dashboard's ports: $(j "$ice" details)"
    ok "a show on air and two keys; their details give the dashboard's ports ($SOURCE_ICE_PORT/tcp, $SOURCE_SRT_PORT/udp)"
    # WHIP (17d): the key's details give the public domain's address and the dashboard's port, and
    # an offer with a wrong bearer goes through nginx's /whip/ and the control plane to the
    # ingest, which refuses it (401). An offer that is not WHIP's own media type is refused first.
    [[ $(j "$srt" details.whip.port) == "$SOURCE_WHIP_PORT" && $(j "$srt" details.whip.url) == "https://weavesignal-public.weavesignal.test/whip/$srt_id" ]] \
        || fail "the key's WHIP details: $(j "$srt" details)"
    for i in $(seq 1 30); do
        podman logs "$p-app" 2>&1 | grep 'taking WHIP from hosts' >/dev/null && break
        sleep 0.5; [[ $i == 30 ]] && fail "the ingest did not take WHIP once a key existed"
    done
    local whip_code
    whip_code=$(curl -s -o "$work/whip-wrong.txt" -w '%{http_code}' -X POST -H 'Content-Type: application/sdp' \
        -H 'Authorization: Bearer WRONGWRONGWRONGWRONGWRON' --data-binary $'v=0\r\nm=audio 9 UDP/TLS/RTP/SAVPF 111\r\na=rtpmap:111 opus/48000/2\r\n' \
        "$pub/whip/$srt_id")
    [[ $whip_code == 401 ]] || fail "a WHIP offer with a wrong bearer through the public domain: $whip_code $(cat "$work/whip-wrong.txt")"
    ok "WHIP: the key's details give https://<public domain>/whip/<key id> and $SOURCE_WHIP_PORT/udp; a wrong bearer through nginx reaches the ingest and is refused (401)"
    for i in $(seq 1 30); do
        podman logs "$p-app" 2>&1 | grep 'listening for SRT sources' >/dev/null && break
        sleep 0.5; [[ $i == 30 ]] && fail "the ingest did not start listening once a key existed"
    done

    state() { a GET /api/v1/sources | python3 -c 'import json,sys; s=[x for x in json.load(sys.stdin)["sources"] if x["key_id"]==sys.argv[1]]; print(s[0]["state"] if s else "none")' "$1"; }
    heard() { a GET /api/v1/studio | python3 -c 'import json,sys; s=[x for x in json.load(sys.stdin)["sources"] if x["identity"]==sys.argv[1]]; sys.exit(0 if s and s[0].get("lufs") is not None and s[0].get("slot") is not None else 1)' "source-$1"; }
    ffmpeg -hide_banner -loglevel error -y -f lavfi -i "sine=f=1000:sample_rate=44100:d=40" -ac 2 -c:a libmp3lame -b:a 128k "$work/tone.mp3"
    ffmpeg -hide_banner -loglevel error -re -i "$work/tone.mp3" -c:a copy -content_type audio/mpeg -f mp3 \
        "icecast://source:$ice_pass@127.0.0.1:$SOURCE_ICE_PORT/live" > "$work/ffmpeg-ice.log" 2>&1 &
    local ff_ice=$!
    for i in $(seq 1 40); do [[ $(state "$ice_id") == standby ]] && heard "$ice_id" && break
        sleep 0.5; [[ $i == 40 ]] && { kill "$ff_ice" 2>/dev/null; fail "the Icecast source did not stand by at the desk (state $(state "$ice_id"); $(cat "$work/ffmpeg-ice.log"))"; }
    done
    ok "Icecast: an MP3 source stands by (STANDBY) and the desk hears it: $(a GET /api/v1/sources | python3 -c 'import json,sys; s=[x for x in json.load(sys.stdin)["sources"] if x["key_id"]==sys.argv[1]][0]; print(s["way"], s["format"], "slot", s.get("slot"))' "$ice_id")"

    if ffmpeg -hide_banner -loglevel error -t 3 -i "$work/tone.mp3" -c:a copy -content_type audio/mpeg -f mp3 \
        "icecast://source:${ice_id}-wrongwrongwrongwrongwron@127.0.0.1:$SOURCE_ICE_PORT/live" > "$work/ffmpeg-wrong.log" 2>&1; then
        kill "$ff_ice" 2>/dev/null; fail "a wrong key was taken"
    fi
    local refusal seen own
    refusal=$(a GET /api/v1/shows/steeped/keys | python3 -c 'import json,sys; k=[x for x in json.load(sys.stdin)["keys"] if x["key_id"]==sys.argv[1]][0]; r=k.get("last_refusal") or {}; print(r.get("reason"))' "$ice_id") \
        || fail "the key list"
    [[ $refusal == wrong_key ]] || { kill "$ff_ice" 2>/dev/null; fail "the wrong key's refusal: $refusal"; }
    [[ $(state "$ice_id") == standby ]] || fail "the wrong key disturbed the source standing by"
    # The address the ingest counted the wrong key against (the API never shows it; the log keeps it).
    seen=$(podman exec "$p-pg" psql -U weavesignal -d weavesignal -tAc "SELECT address FROM source_log WHERE event = 'refused' ORDER BY at_ms DESC LIMIT 1")
    own=$(podman inspect "$p-app" -f '{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}')
    ok "a wrong key is refused (wrong_key) and the source standing by is undisturbed; the ingest counted it against $seen (the app's own address is $own: rootless Podman's port forwarder connects from inside the container, so here every host would share it; Cloudron's Docker is not this)"

    ffmpeg -hide_banner -loglevel error -re -t 25 -i "$work/tone.mp3" -c:a aac -b:a 128k -f mpegts \
        "srt://127.0.0.1:$SOURCE_SRT_PORT?streamid=$srt_id&passphrase=$srt_secret&pbkeylen=16" > "$work/ffmpeg-srt.log" 2>&1 &
    local ff_srt=$!
    for i in $(seq 1 40); do [[ $(state "$srt_id") == standby ]] && heard "$srt_id" && break
        sleep 0.5; [[ $i == 40 ]] && { kill "$ff_ice" "$ff_srt" 2>/dev/null; fail "the SRT source did not stand by at the desk (state $(state "$srt_id"); $(cat "$work/ffmpeg-srt.log"))"; }
    done
    ok "SRT: AAC in MPEG-TS with its passphrase stands by and the desk hears it"

    # Both keys ended: their connections close and the desk lets them go.
    a DELETE "/api/v1/shows/steeped/keys/$ice_id" >/dev/null && a DELETE "/api/v1/shows/steeped/keys/$srt_id" >/dev/null || fail "ending the keys"
    for i in $(seq 1 30); do [[ $(state "$ice_id") == none && $(state "$srt_id") == none ]] && break
        sleep 0.5; [[ $i == 30 ]] && fail "ended keys' sources did not go"
    done
    kill "$ff_ice" "$ff_srt" 2>/dev/null || true; wait "$ff_ice" "$ff_srt" 2>/dev/null || true
    ok "ended keys close their connections"
}

# The video path (ADR-0227). Off: the desk writes no HLS, the player is told there is none, and
# the HLS route says so. On: the player is told where the HLS is, the public domain serves the
# playlists and segments with the control plane's caching, the desk's log says SVT-AV1 writes
# them, and a guest's camera reaches the desk's decoder.
video_checks() {
    local pub=http://127.0.0.1:$PUBLIC_PORT
    if [[ $VIDEO != true ]]; then
        python3 -c 'import json,sys; sys.exit(0 if json.load(sys.stdin).get("video") is None else 1)' \
            < <(curl -fsS "$pub/api/v1/player") || fail "video is off, but the player is told of an HLS"
        code=$(curl -s -o /dev/null -w '%{http_code}' --max-time 5 "$pub/hls/live.m3u8" || true)
        [[ $code == 404 ]] || fail "video is off, but /hls/live.m3u8 answered $code, not 404"
        grep -q 'video false' < <(podman logs "$p-app" 2>&1) || fail "the start line does not say video false"
        ok "video off (the default): the player gets no HLS address, /hls/live.m3u8 is 404"
        return
    fi
    local i video=
    for i in $(seq 1 60); do
        video=$(curl -fsS "$pub/api/v1/player" | python3 -c 'import json,sys; v=json.load(sys.stdin).get("video"); print(v["live"] if v else "")')
        [[ -n $video ]] && break
        sleep 1; [[ $i == 60 ]] && { podman logs --tail 40 "$p-app"; fail "the desk wrote no HLS in 60 s"; }
    done
    ok "video on: the desk has HLS after ${i} s; the player is told $video"
    # The public domain serves the playlists and segments, with the control plane's caching.
    curl -fsS -D "$work/hls-live.headers" "$pub/hls/live.m3u8" -o "$work/live.m3u8" || fail "/hls/live.m3u8 is not served"
    head -1 "$work/live.m3u8" | grep -q '^#EXTM3U' || fail "live.m3u8 is not a playlist"
    grep -qi '^cache-control: no-cache' "$work/hls-live.headers" || fail "live.m3u8 is cached: $(grep -i '^cache-control' "$work/hls-live.headers")"
    local variant segment
    variant=$(grep -v '^#' "$work/live.m3u8" | head -1)
    [[ -n $variant ]] || fail "live.m3u8 lists no rung"
    curl -fsS "$pub/hls/$variant" -o "$work/variant.m3u8" || fail "the rung's playlist $variant is not served"
    segment=$(grep -v '^#' "$work/variant.m3u8" | head -1)
    [[ -n $segment ]] || fail "the rung's playlist lists no segment"
    local dir=${variant%/*}; [[ $dir != "$variant" ]] || dir=
    curl -fsS -D "$work/hls-seg.headers" "$pub/hls/${dir:+$dir/}$segment" -o "$work/segment.m4s" || fail "segment $segment is not served"
    grep -qi '^cache-control: public, max-age=30' "$work/hls-seg.headers" || fail "a segment's caching is $(grep -i '^cache-control' "$work/hls-seg.headers")"
    local init
    init=$(sed -n 's/^#EXT-X-MAP:URI="\(.*\)"$/\1/p' "$work/variant.m3u8" | head -1)
    [[ -n $init ]] || fail "the rung's playlist names no initialisation segment"
    curl -fsS "$pub/hls/${dir:+$dir/}$init" -o "$work/init.mp4" || fail "the initialisation segment $init is not served"
    cat "$work/init.mp4" "$work/segment.m4s" > "$work/rung.mp4"
    ffprobe -v error -show_entries stream=codec_name -of csv=p=0 "$work/rung.mp4" 2>/dev/null | grep -q av1 \
        || fail "the first segment ($segment) carries no AV1"
    code=$(curl -s -o /dev/null -w '%{http_code}' -X PUT "$pub/hls/live.m3u8" || true)
    [[ $code == 405 || $code == 404 || $code == 403 ]] || fail "PUT on /hls/ answered $code"
    ok "HLS on the public domain: live.m3u8 ($(wc -l < "$work/live.m3u8") lines, no-cache), rung $variant, segment $segment is AV1 ($(stat -c %s "$work/segment.m4s") bytes, max-age=30)"
    podman logs "$p-app" > "$work/app.log" 2>&1 || true
    grep -F 'video: AV1 to CMAF HLS' "$work/app.log" | grep -q '"encoders":"\[Svt\]"' || fail "the desk's log does not say SVT-AV1 writes the HLS"
    ok "the encoder: SVT-AV1 on the CPU, $(grep -F 'video: AV1 to CMAF HLS' "$work/app.log" | head -1 | sed 's/.*"rungs":"\(.*\)","target.*/\1/; s/\\//g') written to /run/weavesignal/hls"
    # The sound while nobody is live is playout's (ADR-0172): the desk has its feed.
    podman exec -i "$p-app" python3 - <<'PY' || fail "the desk's feed from playout is not connected"
import json, socket, sys
s = socket.socket(socket.AF_UNIX); s.connect("/run/weavesignal/desk.sock")
s.sendall(b'{"op": "status"}\n')
buf = b""
while not buf.endswith(b"\n"):
    buf += s.recv(65536)
sound = json.loads(buf)["sound"]
print("ok    desk status sound:", json.dumps(sound))
sys.exit(0 if sound.get("feed_connected") and sound.get("source") == "playout" else 1)
PY
    # A guest's camera: VP8 from a solid-colour test source, as the memory gate sends them.
    local token
    token=$(curl -fsS -X POST "http://127.0.0.1:$HTTP_PORT/api/v1/room/tokens" -H "Authorization: Bearer $admin" \
        -H 'content-type: application/json' -d '{"identity":"camera-guest","name":"Eilidh Camera","role":"guest","subscribe":false}' \
        | python3 -c 'import json,sys; print(json.load(sys.stdin)["token"])')
    podman rm -f -t 0 "$p-cam" >/dev/null 2>&1 || true
    podman run -d --name "$p-cam" --network "container:$p-app" "$DESK_IMAGE" gst-launch-1.0 -q \
        livekitwebrtcsink name=s video-caps=video/x-vp8 signaller::ws-url="ws://127.0.0.1:8002" \
        signaller::auth-token="$token" signaller::room-name=studio \
        audiotestsrc is-live=true wave=sine freq=700 volume=0.1 samplesperbuffer=480 \
        ! audio/x-raw,format=S16LE,rate=48000,channels=2 ! queue ! s. \
        videotestsrc is-live=true pattern=ball ! video/x-raw,width=640,height=360,framerate=20/1 ! queue ! s. >/dev/null
    for i in $(seq 1 30); do
        podman logs "$p-app" > "$work/camera.log" 2>&1 || true   # not a pipe into grep -q: SIGPIPE fails a pipefail pipeline
        grep -q 'decoding a camera' "$work/camera.log" && break
        sleep 1; [[ $i == 30 ]] && { podman logs --tail 40 "$p-cam"; fail "the desk never decoded the guest's camera"; }
    done
    ok "a guest's VP8 camera through the room: the desk decodes it ($(podman logs "$p-app" 2>&1 | grep -o 'decoding a camera[^"]*' | head -1))"
    podman rm -f -t 0 "$p-cam" >/dev/null 2>&1 || true
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
        up; checks; sources_checks; video_checks; room_test; restart_test; faults
        ;;
esac
echo "ok    running: http://127.0.0.1:$HTTP_PORT (public pages http://127.0.0.1:$PUBLIC_PORT; stop with: $0 down)"
