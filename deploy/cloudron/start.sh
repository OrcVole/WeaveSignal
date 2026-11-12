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
# generated secrets are in /app/data/secrets.env. The station's setup page (ADR-0237) keeps its
# answers in the database, and the stream password's in /app/data/setup.env (read below).
set -euo pipefail

log() { echo "weavesignal-start: $*"; }

run=/run/weavesignal
data=/app/data
mkdir -p "$run"/{home,gst,tls,nginx,hls} "$data"

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
setting AVATAR_UPLOADS true "Chat avatars (ADR-0235): true lets a signed-in chatter upload a picture, held until staff approve it on the Screener tab; false leaves the shipped avatars only. It is the default until an admin uses the switch in Settings > Chat pictures, which then decides (ADR-0259)."
setting AVATARS_DIR /app/data/chat-avatars "Where chatters' uploaded pictures are kept: under /app/data, so they are in Cloudron's backups."
setting CHAT_ATTACHMENTS false "Pictures and documents in the chat (ADR-0262): true lets a trusted chatter attach a picture (PNG, JPEG, WebP) or a document (PDF, OpenDocument, Markdown, text), each held until staff or a moderator approve it, so staff will see every one: add a line to your risk assessment first. false, the default: words only. The studio's switch in Settings > Chat pictures decides once used."
setting ATTACHMENTS_DIR /app/data/chat-attachments "Where pictures and documents posted in the chat are kept: under /app/data, so they are in Cloudron's backups."
setting PUBLISHING_URL "" "The address cards and feeds print. Empty: https:// and the public domain, else the app's own address."
setting PUBLISHING_PLACE "" "Where the station is, printed on the show cards."
setting PUBLISHING_ZONE Europe/London "The station's time zone, for air times on cards and the series' weekly rule."
setting CONSENT_CONTACT "" "How a caller asks for their call to be taken down (an email address or a link), said in the consent words. Call-in stays closed until it is set."
setting SEARCH_LISTING false "Whether search engines may list the public pages, feeds and cards (ADR-0133): true or false. The studio, the invite page and the echo test are never listed."
setting DIRECTORY_LISTING false "Whether the stream asks internet radio directories to list it (ADR-0129): true or false."
setting COMPAT_STREAM false "The MP3 compatibility stream (ADR-0002) for players that do not take Opus, on the listener domain beside the Opus mount: true or false."
setting COMPAT_MOUNT programme.mp3 "The MP3 stream's mount."
setting COMPAT_BITRATE 128 "The MP3 stream's bitrate in kbit/s."
# --- 1.0.1: the Opus stream's rate (ADR-0233) and a second MP3 stream (ADR-0232) ---
setting OPUS_BITRATE 96 "The Opus stream's constant bit rate in kbit/s (ADR-0233): 64 to 256. Each listener costs this much bandwidth."
setting COMPAT_HIGH_STREAM false "A second MP3 stream (ADR-0232), at a higher rate for music, beside the first on the listener domain; the player offers each stream that is on: true or false."
setting COMPAT_HIGH_MOUNT programme-high.mp3 "The second MP3 stream's mount."
setting COMPAT_HIGH_BITRATE 320 "The second MP3 stream's bitrate in kbit/s (up to 2.5 times the bandwidth of 128 for each listener on it)."
# --- 1.0.1: the chat's hours (ADR-0234) ---
setting CHAT_HOURS always "When listeners may post in the chat (ADR-0234): always, or shows (while a scheduled episode is on air by the schedule, and the station's staff at any time; between shows the chat is read-only and the player says when the next show starts)."
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
setting DESK_FEED_PORT 7317 "Playout's programme back to the desk, the video's sound (ADR-0172), on loopback; used only with VIDEO=true."
# --- cloudron-video (ADR-0227): the video path, off by default ---------------------------------
setting VIDEO false "The desk's video path (ADR-0009, ADR-0227): true or false. On, the desk takes guests' cameras, mixes the picture, encodes AV1 on the CPU with SVT-AV1 and writes CMAF HLS that the public domain serves at /hls/; the join pages offer a camera check. It costs memory and CPU (see the install guide); restart the app after changing it."
# --- icecast-out (ADR-0226): send the programme to another Icecast server ---------------------
setting ICECAST_OUT false "Send the programme to another Icecast server as a source, for a DJ's AzuraCast (ADR-0226): true or false. Needs the server below and its password in secrets.env (ICECAST_OUT_PASSWORD). The studio's Settings choose when it sends."
setting ICECAST_OUT_HOST "" "The other server's address, such as radio.example.org (no https://)."
setting ICECAST_OUT_PORT 8005 "Its port for sources. For AzuraCast: the Port under Connection Information on the station's Streamers/DJ Accounts page."
setting ICECAST_OUT_MOUNT / "The mount to send to. For AzuraCast: the Mount Name there, likely /"
setting ICECAST_OUT_USER source "The user name: for AzuraCast, the Streamer/DJ account's username."
setting ICECAST_OUT_TLS false "Connect over TLS (true or false). AzuraCast's DJ port is plain unless its administrator set it up otherwise."
setting ICECAST_OUT_BITRATE 128 "The MP3 bit rate sent, in kbit/s."
# --- rtmp-out (ADR-0230) ---
# RTMP out to Rumble, Odysee, PeerTube or any RTMP server: its destinations and stream keys are
# set in the studio app (Settings, RTMP out). It needs VIDEO=true; VIDEO_CANVAS is the desk's
# [video] canvas.
setting VIDEO_CANVAS "" "RTMP out (with video on): the picture composed; empty is 480p20; 720p30, 720p25 or 1080p30 lets RTMP send more."
setting RTMP_RUNG "" "RTMP out: the picture sent, at most the canvas; empty is the canvas."
setting RTMP_H264 openh264 "RTMP out's H.264 encoder: openh264 on the CPU (a Cloudron app has no GPU), auto or nvenc."
setting RTMP_VIDEO_KBPS 0 "RTMP out's H.264 bitrate in kbit/s; 0 is the picture's default (480p20: 700, 720p30: 2500)."
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
for b in SEARCH_LISTING DIRECTORY_LISTING COMPAT_STREAM COMPAT_HIGH_STREAM PROCESSING VIDEO; do is_bool "$b"; done
for n in COMPAT_BITRATE PROCESSING_TARGET_LUFS PROCESSING_MAX_GAIN_DB DESK_FEED_PORT FEED_BUFFER_S; do is_number "$n"; done
# rtmp-out
is_number RTMP_VIDEO_KBPS
[[ $RTMP_H264 == openh264 || $RTMP_H264 == auto || $RTMP_H264 == nvenc ]] || { log "RTMP_H264=$RTMP_H264 in weavesignal.env is not openh264, auto or nvenc"; exit 1; }
# The desk refuses a picture it does not know and would restart in a loop, video on or off.
for r in VIDEO_CANVAS RTMP_RUNG; do
    [[ ${!r} =~ ^(|480p20|360p20|240p15|720p30|720p25|1080p30)$ ]] \
        || { log "$r=${!r} in weavesignal.env is not empty or one of 480p20, 360p20, 240p15, 720p30, 720p25, 1080p30"; exit 1; }
done
[[ $COMPAT_MOUNT =~ ^[A-Za-z0-9._-]+$ && $COMPAT_MOUNT != "$LISTENERS_MOUNT" ]] \
    || { log "COMPAT_MOUNT=$COMPAT_MOUNT is not a mount name (letters, digits, . _ -) other than LISTENERS_MOUNT"; exit 1; }
# The rates go into JSON and TOML as whole numbers; the listener process refuses an Opus rate
# outside 64 to 256 and would restart in a loop.
is_int_in() { [[ ${!1} =~ ^[0-9]+$ ]] && (( ${!1} >= $2 && ${!1} <= $3 )) || { log "$1=${!1} in weavesignal.env is not a whole number from $2 to $3"; exit 1; }; }
is_int_in OPUS_BITRATE 64 256
[[ $CHAT_HOURS == always || $CHAT_HOURS == shows ]] || { log "CHAT_HOURS=$CHAT_HOURS in weavesignal.env is not always or shows"; exit 1; }
is_int_in COMPAT_HIGH_BITRATE 32 320
[[ $COMPAT_HIGH_MOUNT =~ ^[A-Za-z0-9._-]+$ && $COMPAT_HIGH_MOUNT != "$LISTENERS_MOUNT" ]] \
    && [[ $COMPAT_HIGH_STREAM != true || $COMPAT_STREAM != true || $COMPAT_HIGH_MOUNT != "$COMPAT_MOUNT" ]] \
    || { log "COMPAT_HIGH_MOUNT=$COMPAT_HIGH_MOUNT is not a mount name (letters, digits, . _ -) other than LISTENERS_MOUNT and COMPAT_MOUNT"; exit 1; }
# icecast-out: words that go into JSON as they are.
is_bool ICECAST_OUT; is_bool ICECAST_OUT_TLS
for n in ICECAST_OUT_PORT ICECAST_OUT_BITRATE; do is_number "$n"; done
for w in ICECAST_OUT_HOST ICECAST_OUT_MOUNT ICECAST_OUT_USER; do
    [[ ${!w} =~ ^[^\"\\[:space:]]*$ ]] || { log "$w in weavesignal.env holds a quote, a backslash or a space"; exit 1; }
done
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
# icecast-out (ADR-0226): the other server's password is the administrator's to type here; it
# is a secret, so it lives in this file (mode 0600), not in weavesignal.env.
if ! grep -q '^ICECAST_OUT_PASSWORD=' "$data/secrets.env"; then
    (umask 077; printf '# The password of the Icecast server the programme is sent to (ICECAST_OUT in weavesignal.env; ADR-0226). Type it after the = and restart the app.\nICECAST_OUT_PASSWORD=\n' >> "$data/secrets.env")
fi
# The generated secrets are hex, safe for the shell to read. ICECAST_OUT_PASSWORD is typed by
# hand and is someone else's password: it is taken as written (an & or a space sourced as shell
# stopped the start, and $$ became a process number), less one pair of quotes around it.
# shellcheck source=/dev/null
source <(grep -v '^ICECAST_OUT_PASSWORD=' "$data/secrets.env")
ICECAST_OUT_PASSWORD=$(sed -n 's/^ICECAST_OUT_PASSWORD=//p' "$data/secrets.env" | tail -n 1)
ICECAST_OUT_PASSWORD=${ICECAST_OUT_PASSWORD%$'\r'}
if [[ $ICECAST_OUT_PASSWORD =~ ^\"(.*)\"$ || $ICECAST_OUT_PASSWORD =~ ^\'(.*)\'$ ]]; then
    ICECAST_OUT_PASSWORD=${BASH_REMATCH[1]}
fi

# --- The station's setup page (ADR-0237): the stream password's answer ------------------------
# The page (served at /setup/ on the app's own domain, an admin's) keeps what it can in the
# database. The stream's password cannot live there: nginx, written below, enforces it. So the
# control plane writes the page's answer to setup.env, and it applies here, at the next start,
# over weavesignal.env and secrets.env. The file is written by a web service, so it is never
# sourced: the two keys are read as text and each is checked; anything else in it is ignored.
setup_env=$data/setup.env
if [[ -f $setup_env ]]; then
    v=$(sed -n 's/^LISTENERS_AUTH=//p' "$setup_env" | tail -n 1)
    if [[ $v == on || $v == off ]]; then
        LISTENERS_AUTH=$v
    elif [[ -n $v ]]; then
        log "setup.env: LISTENERS_AUTH is not on or off; ignored"
    fi
    v=$(sed -n 's/^LISTENERS_PASSWORD=//p' "$setup_env" | tail -n 1)
    if [[ $v =~ ^[A-Za-z0-9._~-]{8,64}$ ]]; then
        LISTENERS_PASSWORD=$v
    elif [[ -n $v ]]; then
        log "setup.env: LISTENERS_PASSWORD is not 8 to 64 letters, digits, . _ ~ or -; ignored"
    fi
fi
stream_auth=false; [[ ${LISTENERS_AUTH:-off} != on ]] || stream_auth=true

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
# Chatters' uploaded pictures (ADR-0235); the root is read-only, so anywhere else fails.
case $AVATARS_DIR in /app/data/*) ;; *) log "AVATARS_DIR=$AVATARS_DIR is outside /app/data (not writable, not backed up): using /app/data/chat-avatars"; AVATARS_DIR=/app/data/chat-avatars ;; esac
mkdir -p "$AVATARS_DIR"
# Pictures and documents posted in the chat (ADR-0262), likewise.
case $ATTACHMENTS_DIR in /app/data/*) ;; *) log "ATTACHMENTS_DIR=$ATTACHMENTS_DIR is outside /app/data (not writable, not backed up): using /app/data/chat-attachments"; ATTACHMENTS_DIR=/app/data/chat-attachments ;; esac
mkdir -p "$ATTACHMENTS_DIR"

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
    # With video on, the room offers what the desk decodes and the pages publish (0004 §4):
    # Opus, and VP8, VP9 and AV1 for cameras; no H.264 or H.265 (ADR-0227, as the Compose bundle).
    if [[ $VIDEO == true ]]; then
        echo "room:"
        echo "  enabled_codecs:"
        echo "    - mime: audio/opus"
        echo "    - mime: audio/red"
        echo "    - mime: video/vp8"
        echo "    - mime: video/vp9"
        echo "    - mime: video/av1"
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
# The desk feed (playout back to the desk, ADR-0172) is for video: enabled with VIDEO=true; its
# port stays in the registry either way so check-ports knows it.
cat > "$run/playout.json" <<JSON
{
  "station_id": "$STATION_ID",
  "srt": { "host": "127.0.0.1", "port": $SRT_INGEST_PORT },
  "listeners": { "bind": "127.0.0.1", "host": "127.0.0.1", "port": $LISTENERS_PORT, "mount": "$LISTENERS_MOUNT", "feed_port": $PLAYOUT_FEED_PORT, "feed_buffer_s": $FEED_BUFFER_S, "directory": $DIRECTORY_LISTING, "bitrate": $OPUS_BITRATE, "compat": { "enabled": $COMPAT_STREAM, "mount": "$COMPAT_MOUNT", "bitrate": $COMPAT_BITRATE }, "compat_high": { "enabled": $COMPAT_HIGH_STREAM, "mount": "$COMPAT_HIGH_MOUNT", "bitrate": $COMPAT_HIGH_BITRATE }, "control_socket": "$run/listeners.sock" },
  "desk_feed": { "host": "127.0.0.1", "port": $DESK_FEED_PORT, "enabled": $VIDEO },
  "control_socket": "$run/playout.sock",
  "carts": { "dir": "$data/carts" },
  "library": { "dir": "$data/library" },
  "fallback": { "dir": "$data/fallback", "default_file": "/app/code/default-loop/default-loop.flac" },
  "processing": { "enabled": $PROCESSING, "target_lufs": $(float "$PROCESSING_TARGET_LUFS"), "max_gain_db": $(float "$PROCESSING_MAX_GAIN_DB") },
  "icecast_out": { "enabled": $ICECAST_OUT, "host": "$ICECAST_OUT_HOST", "port": $ICECAST_OUT_PORT, "mount": "$ICECAST_OUT_MOUNT", "user": "$ICECAST_OUT_USER", "tls": $ICECAST_OUT_TLS, "bitrate": $ICECAST_OUT_BITRATE, "name": "WeaveSignal" },
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
# cloudron-video (ADR-0227): the video path, with the Compose bundle's defaults (the 480p20 rung,
# SVT-AV1, 2 s segments). The HLS is written under /run, which is never backed up, and served
# by the control plane (`join.hls_dir`) at /hls/. The slate's picture is the control plane's
# `video.still_file`, a file the desk watches.
station_toml=${STATION_NAME//\\/\\\\}
station_toml=${station_toml//\"/\\\"}
cat >> "$run/desk.toml" <<TOML

[video]
enabled = $VIDEO
station = "$station_toml"
still_file = "$data/carts/stills/slate.png"
canvas = "$VIDEO_CANVAS"

[video.hls]
dir = "$run/hls"

# Playout's output back to the desk: the video's sound while nobody is live (ADR-0172).
[video.playout_feed]
enabled = $VIDEO
port = $DESK_FEED_PORT

# rtmp-out: RTMP out's picture and H.264 encoder (ADR-0230); read only with video on.

[video.rtmp]
rung = "$RTMP_RUNG"

[video.rtmp.compatible]
h264 = "$RTMP_H264"
video_kbps = $RTMP_VIDEO_KBPS
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

# The slate's picture (ADR-0010, ADR-0200): the control plane writes it where the desk looks.
# Under /app/data, not the image's default under /var/lib, which the read-only root lacks.
[video]
still_file = "$data/carts/stills/slate.png"

# nginx serves the pages from /app/code/www; listed is whether search engines may list the public
# address (ADR-0133), for the control plane's own answers (feeds, cards). nginx says the same of
# the pages.
[web]
listed = $SEARCH_LISTING

# The setup page's one answer that is not in the database (ADR-0237): see setup.env above.
[setup]
overrides_file = "$setup_env"
stream_auth = $stream_auth

[join]
hls_dir = "$run/hls"
stream_url = "$stream_url"
# The player offers the Opus stream and each MP3 stream that is on (ADR-0232, ADR-0233).
stream_kbps = $OPUS_BITRATE
mp3 = [
  { enabled = $COMPAT_STREAM, mount = "$COMPAT_MOUNT", kbps = $COMPAT_BITRATE },
  { enabled = $COMPAT_HIGH_STREAM, mount = "$COMPAT_HIGH_MOUNT", kbps = $COMPAT_HIGH_BITRATE },
]
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

# When listeners may post (ADR-0234): always, or while a scheduled show is on air.
[hours]
open = "$CHAT_HOURS"

[mail]
mode = "$mail_mode"
from = "$mail_from"
command = ["/usr/bin/msmtp", "-C", "$run/msmtprc", "-t"]
outbox_dir = "$data/chat-outbox"

[avatars]
uploads = $AVATAR_UPLOADS
dir = "$AVATARS_DIR"

[attachments]
uploads = $CHAT_ATTACHMENTS
dir = "$ATTACHMENTS_DIR"

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
chat_public='^/api/v1/chat/(info|socket|signin/start|signin/finish|guest|notices|me|me/avatar|me/avatar/stock|avatars/[^/]+|attachments|pictures/[^/]+|pictures/[^/]+/thumb|files/[^/]+|signout|reports|statements/[^/]+/review)$'
# The control plane's public routes for the pages (crates/control/src/public.rs, now playing).
# The station's look (ADR-0237) is read by every public page: the three branding routes, GET only
# (their PUT and DELETE are an admin's, on the app's own domain).
control_public='^/api/v1/(station|player|call-in|call-in/challenge|pass|pass/token|pass/events|pass/leave|now-playing|now-playing/events|steering)$'
branding_public='^/api/v1/branding(/icon|/banner)?$'
# The listener stream's mounts: the Opus one, and each MP3 one the station turns on
# (ADR-0002, ADR-0232). Each read only, behind the stream's password when it has one, without ICY.
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
# The listener domain's bare address: someone who types it meets the player on the public
# domain, not a 404 (noted 2026-11-04). Without a public domain it stays 404: the app's own
# domain is behind Cloudron's login.
if [[ -n $PUBLIC_DOMAIN ]]; then
    listener_root="location = / { return 302 https://$PUBLIC_DOMAIN/; }"
else
    listener_root=""
fi
[[ $COMPAT_STREAM != true ]] || stream_locations+=$'\n'$(stream_location "$COMPAT_MOUNT")
[[ $COMPAT_HIGH_STREAM != true ]] || stream_locations+=$'\n'$(stream_location "$COMPAT_HIGH_MOUNT")
# Search engines (ADR-0133). nginx alone says it, for every domain: the control plane's own
# header is hidden so no answer carries it twice. Unlisted (the default), every answer says
# noindex and /robots.txt disallows everything. Listed, the public domain's player, feeds and
# cards may be listed; its API, invite page and echo test never are. The station, stream and
# room domains are never listed.
noindex='add_header X-Robots-Tag "noindex, nofollow" always;'
if [[ $SEARCH_LISTING == true ]]; then
    pub_noindex=        # on the server: nothing
    pub_never=$noindex  # on the API, the invite page and the echo test
    pub_robots='User-agent: *\nDisallow: /api/\nDisallow: /join/\nDisallow: /echo-test/\nDisallow: /setup/\n'
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
        # The chat (its own process), the socket included; its studio routes too. A chatter's
        # picture or a file posted in the chat: the chat itself takes at most avatars.max_upload_kb
        # and attachments.max_picture_kb (32 MB at the most).
        location /api/v1/chat/ {
            client_max_body_size 33m;
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
        # The pages are revalidated on every load and their hashed files kept for good, as the
        # control plane serves them (web.rs): a page cached across an update named code files the
        # update had removed (the talk station, 2026-11-11). By expires, not add_header, so the
        # server's own headers still apply.
        location /assets/ { expires max; try_files \$uri =404; }
        location / { expires epoch; try_files \$uri \$uri/index.html =404; }
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
        $listener_root
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
        location ~ ^/(join|echo-test)(/|\$) { $pub_never expires epoch; try_files \$uri \$uri/index.html =404; }
        location ~ $chat_public {
            $pub_never
            # A chatter's picture (ADR-0235) or a file posted in the chat (ADR-0262): the chat
            # itself takes at most avatars.max_upload_kb and attachments.max_picture_kb (32 MB at
            # the most), route by route.
            client_max_body_size 33m;
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
        location ~ $branding_public {
            $pub_never
            limit_except GET { deny all; }
            proxy_pass http://127.0.0.1:$CONTROL_PORT;
        }
        # The setup page is the admin's, on the app's own domain only (ADR-0237).
        location ~ ^/setup(/|\$) { return 404; }
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
        location /assets/ { expires max; try_files \$uri =404; }
        location / { expires epoch; try_files \$uri \$uri/index.html =404; }
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
# The station's own [studio] (the show, the rundown, the banned list) and [steering], which
# this script does not write: /app/data/station.toml, read by the control plane when it
# exists (issue 5). It may hold only those two sections.
export WEAVESIGNAL_CONTROL_EXTRA=/app/data/station.toml
export WEAVESIGNAL_LIVEKIT_API_SECRET=$LIVEKIT_API_SECRET
export WEAVESIGNAL_CONTROL_ADMIN_TOKEN=$CONTROL_ADMIN_TOKEN
export WEAVESIGNAL_CHAT_EMAIL_KEY=$CHAT_EMAIL_KEY WEAVESIGNAL_CONTROL_EMAIL_KEY=$CONTROL_EMAIL_KEY
export WEAVESIGNAL_CONTROL_TOKEN_KEY=$CONTROL_TOKEN_KEY
export WEAVESIGNAL_PLAYOUT_DATA=$run/playout.json
export WEAVESIGNAL_LISTENERS_SOURCE_PASSWORD=$LISTENERS_SOURCE_PASSWORD
export WEAVESIGNAL_PLAYOUT_FEED_PASSWORD=$LISTENERS_SOURCE_PASSWORD   # the desk's video feed (ADR-0172)
export WEAVESIGNAL_ICECAST_OUT_PASSWORD=${ICECAST_OUT_PASSWORD:-}   # icecast-out (ADR-0226)
export LIVEKIT_KEYS="$LIVEKIT_API_KEY: $LIVEKIT_API_SECRET"

# The settings must parse, and every port must be free, before anything starts: a mistake here
# would otherwise be a restart loop in one service's log.
if ! out=$(/app/code/bin/weavesignal-control --config "$run/control.toml" check-ports 2>&1); then
    log "the control plane's settings or ports are wrong:"; echo "$out"; exit 1
fi

log "station $STATION_ID; room wss://${ROOM_DOMAIN:-?}; media udp ${LIVEKIT_MEDIA_PORT:-off}; TURN/TLS ${turn_tls:-off}; TURN/UDP ${TURN_UDP_PORT:-off}; ICE/TCP ${LIVEKIT_TCP_PORT:-off}; listeners https://${LISTENERS_DOMAIN:-?}/$LISTENERS_MOUNT (auth ${LISTENERS_AUTH:-off}); public pages ${PUBLIC_DOMAIN:-off}; cards and feeds $public_url; chat mail $mail_mode; video $VIDEO"
exec /usr/bin/supervisord --configuration /app/code/supervisord.conf --nodaemon
