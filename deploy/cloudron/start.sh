#!/bin/bash
# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# WeaveSignal on Cloudron: writes every service's settings from Cloudron's environment on every
# start (ports, domains, the database and mail can change between starts), checks them, then
# hands over to supervisord, which runs LiveKit, the listener process (icecast.server), playout,
# the control plane, the chat, the desk and nginx as the cloudron user.
#
# Cloudron provides (when the administrator leaves the port or domain enabled):
#   CLOUDRON_APP_DOMAIN, CLOUDRON_APP_ORIGIN, CLOUDRON_POSTGRESQL_URL
#   CLOUDRON_MAIL_*      the sendmail addon: the chat's sign-in codes go out through it
#   LISTENERS_DOMAIN     the listener stream's domain (httpPorts, container port 8001)
#   ROOM_DOMAIN          LiveKit's signalling domain (httpPorts, container port 8002)
#   PUBLIC_DOMAIN        the public pages, the chat, feeds, cards and episodes (httpPorts, 8003)
#   LIVEKIT_MEDIA_PORT   udp, LiveKit's one media port (ADR-0023)
#   TURN_TLS_PORT        tcp, LiveKit's TURN over TLS (ADR-0059, ADR-0080, ADR-0106)
#   TURN_UDP_PORT        udp, LiveKit's TURN over UDP (off by default)
#   LIVEKIT_TCP_PORT     tcp, LiveKit's ICE over TCP (off by default)
# The public ports have no containerPort in the manifest, so the number inside the container is
# the number outside: LiveKit advertises the port it binds, and guests must reach that number.
#
# The station's own settings, which the administrator may edit, are in /app/data/weavesignal.env;
# generated secrets are in /app/data/secrets.env.
set -euo pipefail

log() { echo "weavesignal-start: $*"; }

run=/run/weavesignal
data=/app/data
mkdir -p "$run"/{home,gst,tls,nginx} "$data"

# --- The station's settings: written once with every default, then the administrator's ------
if [[ ! -f $data/weavesignal.env ]]; then
    cat > "$data/weavesignal.env" <<'ENV'
# WeaveSignal settings. Restart the app after editing. Every port is a setting (ADR-0075); the
# public ports (media, TURN, ICE over TCP) are chosen in Cloudron's dashboard, not here.
STATION_NAME="Kettle FM"
ROOM_NAME=studio
# Listener stream access: "off" leaves the stream open but unlisted, as the Compose bundle does;
# "on" asks for the user `listener` and the password in secrets.env.
LISTENERS_AUTH=off
LISTENERS_MOUNT=programme.opus
# LiveKit's public address for ICE candidates. Empty: discovered by STUN at start. Set it to the
# server's public IPv4 address if discovery picks the wrong one.
NODE_IP=
# Internal ports, on loopback inside the container (ADR-0075). Change only on a clash.
CONTROL_PORT=7310
LISTENERS_PORT=7312
SRT_INGEST_PORT=7313
LIVEKIT_HTTP_PORT=7880
LOG_LEVEL=info
ENV
fi
# Settings added after a station was installed are appended with their defaults, so the file
# keeps showing every setting; the administrator's own lines are never changed.
added=
setting() {   # name, default, comment
    if ! grep -q "^$1=" "$data/weavesignal.env"; then
        [[ -n $added ]] || { printf '\n# Added by a later package; edit as the lines above.\n' >> "$data/weavesignal.env"; added=1; }
        printf '# %s\n%s=%s\n' "$3" "$1" "$2" >> "$data/weavesignal.env"
    fi
}
setting CHAT_PORT 7311 "The chat's port, on loopback (ADR-0075)."
setting PLAYOUT_FEED_PORT 7315 "Playout's programme to the listener process, on loopback (it also takes the next port)."
setting FEED_BUFFER_S 1.5 "The listener process's cushion of playout's feed, in seconds (ADR-0182). Each second is a second more listener delay; too little on a busy server and listeners hear the fallback for moments."
setting MAIL_MODE cloudron "The chat's sign-in codes: cloudron (Cloudron's mail, else the outbox), outbox (files in /app/data/chat-outbox), or off."
setting PUBLISHING_URL "" "The address cards and feeds print. Empty: https:// and the public domain, else the app's own address."
setting PUBLISHING_PLACE "" "Where the station is, printed on the show cards."
setting PUBLISHING_ZONE Europe/London "The station's time zone, for air times on cards and the series' weekly rule."
setting CONSENT_CONTACT "" "How a caller asks for their call to be taken down (an email address or a link), said in the consent words. Call-in stays closed until it is set."
setting SEARCH_LISTING false "Whether search engines may list the public pages, feeds and cards (ADR-0133): true or false. The studio, the invite page and the echo test are never listed."
setting DIRECTORY_LISTING false "Whether the stream asks internet radio directories to list it (ADR-0129): true or false."
setting COMPAT_STREAM false "The MP3 compatibility stream (ADR-0002) for players that do not take Opus, on the listener domain beside the Opus mount: true or false."
setting COMPAT_MOUNT programme.mp3 "The MP3 stream's mount."
setting COMPAT_BITRATE 128 "The MP3 stream's bitrate in kbit/s."
setting PROCESSING true "Playout's slow gain control towards a loudness target, before the limiter (ADR-0044): true or false (the limiter stays)."
setting PROCESSING_TARGET_LUFS -16 "The gain control's loudness target, in LUFS (ADR-0044: -16, the internet-radio norm)."
# 0.9.0 moves the target from the old default, -23, to -16 (ADR-0044), once: a station still at
# -23 gets -16 and a marker line, so -23 set again afterwards is kept.
if grep -q '^PROCESSING_TARGET_LUFS=-23$' "$data/weavesignal.env" && ! grep -q '^# weavesignal: loudness default moved to -16' "$data/weavesignal.env"; then
    sed -i 's/^PROCESSING_TARGET_LUFS=-23$/PROCESSING_TARGET_LUFS=-16/' "$data/weavesignal.env"
    printf '# weavesignal: loudness default moved to -16 (0.9.0); set -23 here to keep the old level\n' >> "$data/weavesignal.env"
elif ! grep -q '^# weavesignal: loudness default moved to -16' "$data/weavesignal.env"; then
    printf '# weavesignal: loudness default moved to -16 (0.9.0); set -23 here to keep the old level\n' >> "$data/weavesignal.env"
fi
setting PROCESSING_MAX_GAIN_DB 6 "The most the gain control may raise the programme, in dB."
setting DESK_FEED_PORT 7317 "Playout's programme back to the desk, the video's sound (ADR-0172), on loopback; used only with video on, which this package leaves off."
# shellcheck source=/dev/null
source "$data/weavesignal.env"
CHAT_PORT=${CHAT_PORT:-7311}
PLAYOUT_FEED_PORT=${PLAYOUT_FEED_PORT:-7315}
DESK_FEED_PORT=${DESK_FEED_PORT:-7317}
MAIL_MODE=${MAIL_MODE:-cloudron}
# Settings written into JSON and TOML as they are: a wrong value would stop both Liquidsoaps in
# a restart loop, so it stops the start here instead, with the setting named.
is_bool() { [[ ${!1} == true || ${!1} == false ]] || { log "$1=${!1} in weavesignal.env is not true or false"; exit 1; }; }
is_number() { [[ ${!1} =~ ^-?[0-9]+(\.[0-9]+)?$ ]] || { log "$1=${!1} in weavesignal.env is not a number"; exit 1; }; }
for b in SEARCH_LISTING DIRECTORY_LISTING COMPAT_STREAM PROCESSING; do is_bool "$b"; done
for n in COMPAT_BITRATE PROCESSING_TARGET_LUFS PROCESSING_MAX_GAIN_DB DESK_FEED_PORT FEED_BUFFER_S; do is_number "$n"; done
[[ $COMPAT_MOUNT =~ ^[A-Za-z0-9._-]+$ && $COMPAT_MOUNT != "$LISTENERS_MOUNT" ]] \
    || { log "COMPAT_MOUNT=$COMPAT_MOUNT is not a mount name (letters, digits, . _ -) other than LISTENERS_MOUNT"; exit 1; }
# Liquidsoap reads these two as floats.
float() { [[ $1 == *.* ]] && echo "$1" || echo "$1.0"; }

# --- Secrets: generated on first start, kept in /app/data (backed up) ------------------------
rand() { openssl rand -hex "$1"; }
if [[ ! -f $data/secrets.env ]]; then
    log "first start: generating the station identifier and secrets"
    station_id=$(/app/code/bin/weavesignal-control sample-config | sed -n 's/^id = "\(.*\)"$/\1/p' | head -1)
    [[ -n $station_id ]] || { log "could not make a station identifier"; exit 1; }
    umask 077
    cat > "$data/secrets.env" <<ENV
# Generated on first start. STATION_ID is the station's identity for its whole life (ADR-0047).
STATION_ID=$station_id
LIVEKIT_API_KEY=weavesignal
LIVEKIT_API_SECRET=$(rand 32)
LISTENERS_SOURCE_PASSWORD=$(rand 24)
CONTROL_ADMIN_TOKEN=$(rand 32)
LISTENERS_PASSWORD=$(rand 12)
ENV
    umask 022
fi
# Secrets added after a station was installed, generated once and then kept. The two email keys
# key the hashes of chatters' and staff's email addresses (ADR-0125, ADR-0127), so a new key
# would find none of the accounts made under the old one: they live here, backed up with the
# database.
secret_setting() {   # name, comment
    if ! grep -q "^$1=" "$data/secrets.env"; then
        log "generating $1"
        (umask 077; printf '# %s\n%s=%s\n' "$2" "$1" "$(rand 32)" >> "$data/secrets.env")
    fi
}
secret_setting CHAT_EMAIL_KEY "Keys the hash of chatters' email addresses (0.8.0). Never change it: the chat would find no account made before."
secret_setting CONTROL_EMAIL_KEY "Keys the hash of staff email addresses for emailed sign-in codes (0.8.0). Never change it."
secret_setting CONTROL_TOKEN_KEY "Keys API tokens and webhook secrets (0.9.2, ADR-0202). Never change it: every token would be revoked."
# shellcheck source=/dev/null
source "$data/secrets.env"

: "${CLOUDRON_APP_DOMAIN:?CLOUDRON_APP_DOMAIN is not set}"
: "${CLOUDRON_POSTGRESQL_URL:?the postgresql addon is missing}"
CLOUDRON_APP_ORIGIN=${CLOUDRON_APP_ORIGIN:-https://$CLOUDRON_APP_DOMAIN}
LISTENERS_DOMAIN=${LISTENERS_DOMAIN:-}
ROOM_DOMAIN=${ROOM_DOMAIN:-}
PUBLIC_DOMAIN=${PUBLIC_DOMAIN:-}
[[ -n $ROOM_DOMAIN ]] || log "ROOM_DOMAIN is not set: guests cannot reach LiveKit's signalling"
[[ -n $LISTENERS_DOMAIN ]] || log "LISTENERS_DOMAIN is not set: the listener stream is not published"

# --- The station's files, all under /app/data so they are backed up --------------------------
# carts: the media worker's FLAC (the control plane writes, the desk and playout read);
# library: the music library; fallback: files played while playout or the desk is away;
# recordings: retention's directory, the listener process's legal logger in logger/;
# show-cards and episodes: M8's cards and episode files; chat-outbox: sign-in codes as files.
mkdir -p "$data"/{carts,library,fallback,recordings/logger,show-cards,episodes,chat-outbox}

# --- The public address cards and feeds print (ADR-0056) -------------------------------------
# The public domain sits outside Cloudron's login, which a podcast app or a card's reader
# cannot pass; the app's own domain is behind it when the station uses Cloudron's users.
if [[ -n ${PUBLISHING_URL:-} ]]; then
    public_url=$PUBLISHING_URL
elif [[ -n $PUBLIC_DOMAIN ]]; then
    public_url=https://$PUBLIC_DOMAIN
else
    public_url=$CLOUDRON_APP_ORIGIN
    log "PUBLIC_DOMAIN is not set: cards and feeds print $public_url, which may be behind Cloudron's login"
fi

# --- Public ports: present only when the administrator enabled them --------------------------
LIVEKIT_MEDIA_PORT=${LIVEKIT_MEDIA_PORT:-}
LIVEKIT_TCP_PORT=${LIVEKIT_TCP_PORT:-}
TURN_TLS_PORT=${TURN_TLS_PORT:-}
TURN_UDP_PORT=${TURN_UDP_PORT:-}
if [[ -z $LIVEKIT_MEDIA_PORT ]]; then
    log "LIVEKIT_MEDIA_PORT is disabled: media only through TURN"
fi

# TURN over TLS needs the app's certificate (the tls addon). The key is root's; copy it while
# still root, for the cloudron user, into /run so it never enters a backup.
turn_tls=$TURN_TLS_PORT
if [[ -n $turn_tls ]]; then
    if [[ -r /etc/certs/tls_cert.pem && -r /etc/certs/tls_key.pem ]]; then
        install -m 0644 -o cloudron -g cloudron /etc/certs/tls_cert.pem "$run/tls/cert.pem"
        install -m 0640 -o cloudron -g cloudron /etc/certs/tls_key.pem "$run/tls/key.pem"
    else
        log "no certificate in /etc/certs: TURN over TLS is off"
        turn_tls=
    fi
fi

# --- LiveKit ----------------------------------------------------------------------------------
{
    echo "# Written by start.sh on every start; edit /app/data/weavesignal.env instead."
    echo "port: $LIVEKIT_HTTP_PORT"
    echo 'bind_addresses: ["127.0.0.1"]'
    echo "rtc:"
    echo "  udp_port: ${LIVEKIT_MEDIA_PORT:-0}"
    echo "  tcp_port: ${LIVEKIT_TCP_PORT:-0}"
    if [[ -n ${NODE_IP:-} ]]; then
        echo "  use_external_ip: false"
        echo "  node_ip: $NODE_IP"
    else
        echo "  use_external_ip: true"
    fi
    if [[ -n $turn_tls || -n $TURN_UDP_PORT ]]; then
        echo "turn:"
        echo "  enabled: true"
        echo "  domain: $CLOUDRON_APP_DOMAIN"
        if [[ -n $turn_tls ]]; then
            echo "  tls_port: $turn_tls"
            echo "  cert_file: $run/tls/cert.pem"
            echo "  key_file: $run/tls/key.pem"
        fi
        [[ -z $TURN_UDP_PORT ]] || echo "  udp_port: $TURN_UDP_PORT"
    fi
    echo "logging:"
    echo "  level: $LOG_LEVEL"
    echo "  json: true"
} > "$run/livekit.yaml"

# --- Playout and the listener process (one data file, read by the two fixed scripts) ---------
# Playout takes the desk's programme over SRT and sends it, as PCM in WAV, to the listener
# process on the feed port; the listener process holds the mount, its own fallback and the
# legal logger (M4). Both ports, and icecast.server's /status.json, stay on loopback. Both play
# the default loop shipped in the image while the fallback folder holds nothing (ADR-0130).
# The desk feed (playout back to the desk, ADR-0172) is for video, which this package leaves
# off; its port stays in the registry so check-ports knows it.
cat > "$run/playout.json" <<JSON
{
  "station_id": "$STATION_ID",
  "srt": { "host": "127.0.0.1", "port": $SRT_INGEST_PORT },
  "listeners": { "bind": "127.0.0.1", "host": "127.0.0.1", "port": $LISTENERS_PORT, "mount": "$LISTENERS_MOUNT", "feed_port": $PLAYOUT_FEED_PORT, "feed_buffer_s": $FEED_BUFFER_S, "directory": $DIRECTORY_LISTING, "compat": { "enabled": $COMPAT_STREAM, "mount": "$COMPAT_MOUNT", "bitrate": $COMPAT_BITRATE }, "control_socket": "$run/listeners.sock" },
  "desk_feed": { "host": "127.0.0.1", "port": $DESK_FEED_PORT, "enabled": false },
  "control_socket": "$run/playout.sock",
  "carts": { "dir": "$data/carts" },
  "library": { "dir": "$data/library" },
  "fallback": { "dir": "$data/fallback", "default_file": "/app/code/default-loop/default-loop.flac" },
  "processing": { "enabled": $PROCESSING, "target_lufs": $(float "$PROCESSING_TARGET_LUFS"), "max_gain_db": $(float "$PROCESSING_MAX_GAIN_DB") },
  "logger": { "dir": "$data/recordings/logger" }
}
JSON

# --- The desk -----------------------------------------------------------------------------------
cat > "$run/desk.toml" <<TOML
# Written by start.sh on every start.
[livekit]
url = "ws://127.0.0.1:$LIVEKIT_HTTP_PORT"
api_key = "$LIVEKIT_API_KEY"
room = "$ROOM_NAME"

[programme]
srt_bind = "127.0.0.1"
srt_port = $SRT_INGEST_PORT

[control]
socket = "$run/desk.sock"

# The cart files the media worker wrote: the desk decodes and plays them (ADR-0117).
[carts]
dir = "$data/carts"
TOML

# --- The control plane ------------------------------------------------------------------------
# Disabled public ports keep their defaults in the registry: nothing listens on them.
stream_url=
[[ -z $LISTENERS_DOMAIN ]] || stream_url="https://$LISTENERS_DOMAIN/$LISTENERS_MOUNT"
# The takedown contact, as a TOML string: backslashes and quotes escaped.
consent_contact=${CONSENT_CONTACT:-}
consent_contact=${consent_contact//\\/\\\\}
consent_contact=${consent_contact//\"/\\\"}
cat > "$run/control.toml" <<TOML
# Written by start.sh on every start.
[station]
id = "$STATION_ID"
name = "$STATION_NAME"

[listen]
internal = "127.0.0.1"

[ports]
control = $CONTROL_PORT
chat = $CHAT_PORT
listeners = $LISTENERS_PORT
srt_ingest = $SRT_INGEST_PORT
playout_feed = $PLAYOUT_FEED_PORT
desk_feed = $DESK_FEED_PORT
livekit_http = $LIVEKIT_HTTP_PORT
livekit_media = ${LIVEKIT_MEDIA_PORT:-7882}
livekit_tcp = ${LIVEKIT_TCP_PORT:-7881}
turn_udp = ${TURN_UDP_PORT:-3478}
turn_tls = ${TURN_TLS_PORT:-15349}

[room]
public_url = "wss://${ROOM_DOMAIN:-$CLOUDRON_APP_DOMAIN}"
api_key = "$LIVEKIT_API_KEY"
api_url = "http://127.0.0.1:$LIVEKIT_HTTP_PORT"
name = "$ROOM_NAME"

[desk]
socket = "$run/desk.sock"

[playout]
socket = "$run/playout.sock"
listeners_socket = "$run/listeners.sock"
library_dir = "$data/library"

[carts]
media_dir = "$data/carts"
demo_pack = "/app/code/carts-demo"

# nginx serves the pages from /app/code/www; listed is whether search engines may list the public
# address (ADR-0133), for the control plane's own answers (feeds, cards). nginx says the same of
# the pages.
[web]
listed = $SEARCH_LISTING

[join]
stream_url = "$stream_url"
consent_contact = "$consent_contact"
# nginx in this container resolves the client from Cloudron's X-Forwarded-For (the rightmost
# address that is not private) and sends each service that one address only, so it does not
# matter whether a service reads the header's first entry or its last.
forwarded_for = true

[publishing]
public_url = "$public_url"
place = "${PUBLISHING_PLACE:-}"
zone = "${PUBLISHING_ZONE:-Europe/London}"
cards_dir = "$data/show-cards"
episodes_dir = "$data/episodes"

[retention]
recordings_dir = "$data/recordings"

[log]
format = "json"
filter = "$LOG_LEVEL"
TOML

# --- The chat (M6): its own process on the station's database ----------------------------------
# Sign-in codes: through Cloudron's sendmail addon by msmtp (the chat pipes each email to a
# sendmail-compatible program), else as files in the outbox, else off.
mail_mode=$MAIL_MODE
mail_from=chat@$CLOUDRON_APP_DOMAIN
if [[ $mail_mode == cloudron ]]; then
    if [[ -n ${CLOUDRON_MAIL_SMTP_SERVER:-} ]]; then
        umask 077
        cat > "$run/msmtprc" <<MSMTP
# Written by start.sh on every start from Cloudron's sendmail addon.
defaults
# Cloudron's mail relay is inside the server, without TLS on this port; msmtp chooses no
# plain-text method by itself without TLS, so PLAIN is named.
auth plain
tls off
syslog off
account cloudron
host $CLOUDRON_MAIL_SMTP_SERVER
port $CLOUDRON_MAIL_SMTP_PORT
user ${CLOUDRON_MAIL_SMTP_USERNAME:-}
password ${CLOUDRON_MAIL_SMTP_PASSWORD:-}
from ${CLOUDRON_MAIL_FROM:-$mail_from}
account default : cloudron
MSMTP
        umask 022
        mail_mode="command"
        mail_from=${CLOUDRON_MAIL_FROM:-$mail_from}
    else
        log "the sendmail addon gave no SMTP server: the chat's sign-in codes go to $data/chat-outbox"
        mail_mode=outbox
    fi
fi
case $mail_mode in command|outbox|off) ;; *) log "MAIL_MODE=$MAIL_MODE is not cloudron, outbox or off"; exit 1 ;; esac
# Staff sign-in codes by email (ADR-0127) go out the same way, from the station's address; with
# mail off, staff sign in with a password only. The key that hashes staff addresses is
# CONTROL_EMAIL_KEY in secrets.env.
mkdir -p "$data/staff-outbox"
cat >> "$run/control.toml" <<TOML

[mail]
mode = "$mail_mode"
from = "$mail_from"
command = ["/usr/bin/msmtp", "-C", "$run/msmtprc", "-t"]
outbox_dir = "$data/staff-outbox"
TOML
cat > "$run/chat.toml" <<TOML
# Written by start.sh on every start.
[station]
id = "$STATION_ID"
name = "$STATION_NAME"

[listen]
address = "127.0.0.1"
port = $CHAT_PORT
forwarded_for = true

[control]
url = "http://127.0.0.1:$CONTROL_PORT"

[mail]
mode = "$mail_mode"
from = "$mail_from"
command = ["/usr/bin/msmtp", "-C", "$run/msmtprc", "-t"]
outbox_dir = "$data/chat-outbox"

[log]
format = "json"
filter = "$LOG_LEVEL"
TOML

# --- nginx: the container's one web listener, in front of loopback services ---------------------
listeners_auth=""
if [[ ${LISTENERS_AUTH:-off} == on ]]; then
    printf 'listener:%s\n' "$(openssl passwd -apr1 "$LISTENERS_PASSWORD")" > "$run/nginx/listeners.htpasswd"
    listeners_auth="auth_basic \"WeaveSignal\"; auth_basic_user_file $run/nginx/listeners.htpasswd;"
fi
# The chat's public routes (crates/chat/src/http.rs); its studio and admin routes are served on
# the station's own domain only.
chat_public='^/api/v1/chat/(info|socket|signin/start|signin/finish|guest|notices|me|signout|reports|statements/[^/]+/review)$'
# The control plane's public routes for the pages (crates/control/src/public.rs, now playing).
control_public='^/api/v1/(station|player|call-in|call-in/challenge|pass|pass/token|pass/events|pass/leave|now-playing|now-playing/events|steering)$'
# The listener stream's mounts: the Opus one, and the MP3 one when the station turns it on
# (ADR-0002). Each read only, behind the stream's password when it has one, without ICY.
stream_location() {
    cat <<LOC
        location = /$1 {
            limit_except GET { deny all; }
            $listeners_auth
            proxy_pass http://127.0.0.1:$LISTENERS_PORT;
            proxy_buffering off;
            proxy_request_buffering off;
            proxy_read_timeout 1d;
            proxy_send_timeout 1d;
            proxy_set_header Icy-MetaData "";
        }
LOC
}
stream_locations=$(stream_location "$LISTENERS_MOUNT")
[[ $COMPAT_STREAM != true ]] || stream_locations+=$'\n'$(stream_location "$COMPAT_MOUNT")
# Search engines (ADR-0133). nginx alone says it, for every domain: the control plane's own
# header is hidden so no answer carries it twice. Unlisted (the default), every answer says
# noindex and /robots.txt disallows everything. Listed, the public domain's player, feeds and
# cards may be listed; its API, invite page and echo test never are. The station, stream and
# room domains are never listed.
noindex='add_header X-Robots-Tag "noindex, nofollow" always;'
if [[ $SEARCH_LISTING == true ]]; then
    pub_noindex=        # on the server: nothing
    pub_never=$noindex  # on the API, the invite page and the echo test
    pub_robots='User-agent: *\nDisallow: /api/\nDisallow: /join/\nDisallow: /echo-test/\n'
else
    pub_noindex=$noindex
    pub_never=          # inherited from the server
    pub_robots='User-agent: *\nDisallow: /\n'
fi
cat > "$run/nginx.conf" <<NGINX
# Written by start.sh on every start.
worker_processes 2;
pid $run/nginx/nginx.pid;
error_log stderr warn;
events { worker_connections 4096; }
http {
    include /etc/nginx/mime.types;
    access_log off;
    server_tokens off;
    client_body_temp_path $run/nginx/body;
    proxy_temp_path $run/nginx/proxy;
    fastcgi_temp_path $run/nginx/fastcgi;
    uwsgi_temp_path $run/nginx/uwsgi;
    scgi_temp_path $run/nginx/scgi;
    map \$http_upgrade \$connection_upgrade { default upgrade; '' close; }

    # The client's address: Cloudron's proxy reaches this container from a private address
    # and appends the address it saw to X-Forwarded-For; the rightmost address that is not
    # private is the client's. Each service then gets that one address, never the client's own
    # header (call-in's per-address limit and the chat's address are counted on it).
    set_real_ip_from 10.0.0.0/8;
    set_real_ip_from 172.16.0.0/12;
    set_real_ip_from 192.168.0.0/16;
    set_real_ip_from 127.0.0.0/8;
    real_ip_header X-Forwarded-For;
    real_ip_recursive on;
    proxy_http_version 1.1;
    proxy_set_header X-Forwarded-For \$remote_addr;
    proxy_set_header Host \$host;
    proxy_hide_header X-Robots-Tag;

    # The station (Cloudron's httpPort): everything, behind Cloudron's login when the station
    # uses Cloudron's users. The studio app uses this domain with its bearer token.
    server {
        listen 8000 default_server;
        root /app/code/www;
        add_header X-Robots-Tag "noindex, nofollow" always;
        location = /health { proxy_pass http://127.0.0.1:$CONTROL_PORT; }
        location = /robots.txt { return 200 "User-agent: *\nDisallow: /\n"; }
        # The chat (its own process), the socket included; its studio routes too.
        location /api/v1/chat/ {
            proxy_pass http://127.0.0.1:$CHAT_PORT;
            proxy_set_header Upgrade \$http_upgrade;
            proxy_set_header Connection \$connection_upgrade;
            proxy_buffering off;
            proxy_read_timeout 1d;
            proxy_send_timeout 1d;
        }
        # The control plane. Its event streams are server-sent events: no buffering. Uploads:
        # carts to 64 MB, library tracks to 200 MB.
        location /api/ {
            proxy_pass http://127.0.0.1:$CONTROL_PORT;
            proxy_set_header Connection "";
            proxy_buffering off;
            proxy_request_buffering off;
            client_max_body_size 200m;
            proxy_read_timeout 1d;
        }
        location ~ ^/(feeds|cards|media/episodes)/ {
            limit_except GET { deny all; }
            proxy_pass http://127.0.0.1:$CONTROL_PORT;
        }
        location / { try_files \$uri \$uri/index.html =404; }
    }

    # The listener stream (httpPorts LISTENERS_DOMAIN): only the mount, read only, unlisted.
    # icecast.server's /status.json and admin pages are never served here. A client asking for
    # ICY metadata would get blocks spliced into the Ogg stream (bench 02), so the request
    # header is dropped.
    server {
        listen 8001;
        add_header X-Robots-Tag "noindex, nofollow" always;
        location = /robots.txt { return 200 "User-agent: *\nDisallow: /\n"; }
$stream_locations
        location / { return 404; }
    }

    # LiveKit's signalling and API (httpPorts ROOM_DOMAIN). Media goes straight to the UDP port.
    server {
        listen 8002;
        add_header X-Robots-Tag "noindex, nofollow" always;
        location / {
            proxy_pass http://127.0.0.1:$LIVEKIT_HTTP_PORT;
            proxy_set_header Upgrade \$http_upgrade;
            proxy_set_header Connection \$connection_upgrade;
            proxy_buffering off;
            proxy_read_timeout 1d;
            proxy_send_timeout 1d;
        }
    }

    # The public pages (httpPorts PUBLIC_DOMAIN), outside Cloudron's login: the player with
    # call-in and the chat, the join and echo-test pages, the feeds, cards and episode files,
    # and only the public routes behind them. Unlisted unless SEARCH_LISTING is true.
    server {
        listen 8003;
        root /app/code/www;
        $pub_noindex
        location = /robots.txt { $pub_never return 200 "$pub_robots"; }
        # The station's health, for the edges' pass-through check (ADR-0160): this domain is
        # outside Cloudron's login, and the answer says only ok, the version and the database.
        location = /health { $pub_never proxy_pass http://127.0.0.1:$CONTROL_PORT; }
        location ~ ^/(join|echo-test)(/|\$) { $pub_never try_files \$uri \$uri/index.html =404; }
        location ~ $chat_public {
            $pub_never
            proxy_pass http://127.0.0.1:$CHAT_PORT;
            proxy_set_header Upgrade \$http_upgrade;
            proxy_set_header Connection \$connection_upgrade;
            proxy_buffering off;
            proxy_read_timeout 1d;
            proxy_send_timeout 1d;
        }
        location ~ $control_public {
            $pub_never
            proxy_pass http://127.0.0.1:$CONTROL_PORT;
            proxy_set_header Connection "";
            proxy_buffering off;
            proxy_read_timeout 1d;
        }
        location ~ ^/(feeds|cards|media/episodes)/ {
            limit_except GET { deny all; }
            proxy_pass http://127.0.0.1:$CONTROL_PORT;
        }
        # The desk's HLS (M5): the control plane serves it only while the desk has video on.
        location /hls/ {
            limit_except GET { deny all; }
            proxy_pass http://127.0.0.1:$CONTROL_PORT;
            proxy_buffering off;
        }
        location /api/ { $pub_never return 404; }
        location / { try_files \$uri \$uri/index.html =404; }
    }
}
NGINX

# --- Ownership, every start (a restore can reset it) -----------------------------------------
chown -R cloudron:cloudron "$data" "$run"
chmod 0600 "$data/secrets.env"

# --- The environment the services inherit --------------------------------------------------------
export HOME=$run/home XDG_CACHE_HOME=$run/home/.cache
export GST_REGISTRY_1_0=$run/gst/registry.bin
export LIQ_CACHE_USER_DIR=/app/code/liq-cache   # built with the image; see the Dockerfile
export RUST_LOG=${RUST_LOG:-$LOG_LEVEL}
export WEAVESIGNAL_DATABASE_URL=$CLOUDRON_POSTGRESQL_URL
export WEAVESIGNAL_LIVEKIT_API_SECRET=$LIVEKIT_API_SECRET
export WEAVESIGNAL_CONTROL_ADMIN_TOKEN=$CONTROL_ADMIN_TOKEN
export WEAVESIGNAL_CHAT_EMAIL_KEY=$CHAT_EMAIL_KEY WEAVESIGNAL_CONTROL_EMAIL_KEY=$CONTROL_EMAIL_KEY
export WEAVESIGNAL_CONTROL_TOKEN_KEY=$CONTROL_TOKEN_KEY
export WEAVESIGNAL_PLAYOUT_DATA=$run/playout.json
export WEAVESIGNAL_LISTENERS_SOURCE_PASSWORD=$LISTENERS_SOURCE_PASSWORD
export LIVEKIT_KEYS="$LIVEKIT_API_KEY: $LIVEKIT_API_SECRET"

# The settings must parse, and every port must be free, before anything starts: a mistake here
# would otherwise be a restart loop in one service's log.
if ! out=$(/app/code/bin/weavesignal-control --config "$run/control.toml" check-ports 2>&1); then
    log "the control plane's settings or ports are wrong:"; echo "$out"; exit 1
fi

log "station $STATION_ID; room wss://${ROOM_DOMAIN:-?}; media udp ${LIVEKIT_MEDIA_PORT:-off}; TURN/TLS ${turn_tls:-off}; TURN/UDP ${TURN_UDP_PORT:-off}; ICE/TCP ${LIVEKIT_TCP_PORT:-off}; listeners https://${LISTENERS_DOMAIN:-?}/$LISTENERS_MOUNT (auth ${LISTENERS_AUTH:-off}); public pages ${PUBLIC_DOMAIN:-off}; cards and feeds $public_url; chat mail $mail_mode"
exec /usr/bin/supervisord --configuration /app/code/supervisord.conf --nodaemon
