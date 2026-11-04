#!/bin/sh
# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# WeaveSignal's audio edge: writes Icecast 2.5's configuration from the environment, then runs
# Icecast as a relay of the station's listener mounts.
#
# Icecast's relay cannot use TLS, so by default the stream crosses the internet through an
# encrypted hop (ADR-0162): nginx, in this image, listens on loopback, fetches each mount from
# the station's HTTPS address (checking its certificate) and hands the bytes to Icecast, which
# is configured to relay from that loopback address. The station then needs no plain-HTTP
# listener port for relays, and neither the stream nor a listener login crosses the internet in
# clear. RELAY_ORIGIN_SCHEME=http skips the hop (a bench, or a trusted private link).
#
#   RELAY_ORIGIN_HOST       required: the station's host as the relay reaches it: its public
#                           HTTPS name (the one its certificate is for), never the loopback
#                           icecast.server behind the station's proxy
#   RELAY_ORIGIN_SCHEME     https (the default: the encrypted hop) or http (no hop: Icecast pulls
#                           from the station's plain-HTTP listener port directly)
#   RELAY_ORIGIN_PORT       the station's port (443 with https, 80 with http)
#   RELAY_ORIGIN_CA_FILE    https only: a file of certificate authorities to trust instead of
#                           the image's (a station with its own authority; a test)
#   RELAY_HOP_PORT          https only: the loopback port the hop listens on, for Icecast alone
#                           (RELAY_PORT + 1)
#   RELAY_MOUNTS            the mounts to relay, space-separated (/programme.opus)
#   RELAY_PORT              where the relay's listeners connect (8000)
#   RELAY_BIND              the address it listens on (every interface; 127.0.0.1 on a bench)
#   RELAY_RETRY_S           how often a missing origin mount is tried again (15, Icecast 2.5's
#                           least; a relay that finds no mount waits this long, so an edge
#                           started before the station picks the programme up within it)
#   RELAY_MAX_LISTENERS     listeners this relay takes (5000)
#   RELAY_ORIGIN_USER       only when the station's streams need a listener login (a private
#   RELAY_ORIGIN_PASSWORD   station): the listener credential, or RELAY_ORIGIN_PASSWORD_FILE
#   RELAY_ADMIN_PASSWORD    Icecast's admin password, or RELAY_ADMIN_PASSWORD_FILE; unset, a
#                           random one is made and the admin pages are effectively closed
#
# The hop takes only the mounts listed here and answers 404 to anything else. The relay takes nothing else of the station's: it pulls each mount as any listener does,
# without asking for ICY metadata. Its own source and relay passwords are random at every
# start, and it accepts no source but its relays, so nobody can push a stream to an edge.
set -eu

: "${RELAY_ORIGIN_HOST:?set RELAY_ORIGIN_HOST to the station's host}"
scheme=${RELAY_ORIGIN_SCHEME:-https}
case $scheme in
  https) origin_port=${RELAY_ORIGIN_PORT:-443} ;;
  http) origin_port=${RELAY_ORIGIN_PORT:-80} ;;
  *) echo "weavesignal-relay: RELAY_ORIGIN_SCHEME is https or http ($scheme)" >&2; exit 2 ;;
esac
ca_file=${RELAY_ORIGIN_CA_FILE:-/etc/ssl/certs/ca-certificates.crt}
mounts=${RELAY_MOUNTS:-/programme.opus}
port=${RELAY_PORT:-8000}
bind=${RELAY_BIND:-}
retry=${RELAY_RETRY_S:-15}
max=${RELAY_MAX_LISTENERS:-5000}
user=${RELAY_ORIGIN_USER:-}

# A secret from VAR, or from the file VAR_FILE names (for container secrets).
secret() {
  value=$(printenv "$1" || true)
  file=$(printenv "$1_FILE" || true)
  if [ -n "$value" ]; then printf %s "$value"; elif [ -n "$file" ]; then cat "$file"; fi
}
random() { head -c 24 /dev/urandom | od -An -tx1 | tr -d ' \n'; }
xml() { printf %s "$1" | sed -e 's/&/\&amp;/g' -e 's/</\&lt;/g' -e 's/>/\&gt;/g' -e 's/"/\&quot;/g'; }

case $port in ''|*[!0-9]*) echo "weavesignal-relay: RELAY_PORT is a number ($port)" >&2; exit 2 ;; esac
hop_port=${RELAY_HOP_PORT:-$((port + 1))}
for v in "$origin_port" "$port" "$hop_port" "$retry" "$max"; do
  case $v in ''|*[!0-9]*) echo "weavesignal-relay: ports, RELAY_RETRY_S and RELAY_MAX_LISTENERS are numbers ($v)" >&2; exit 2 ;; esac
done
if [ "$scheme" = https ]; then
  # These go into the hop's configuration: a host name and mount paths, nothing else.
  case $RELAY_ORIGIN_HOST in *[!A-Za-z0-9.-]*) echo "weavesignal-relay: RELAY_ORIGIN_HOST is a host name (letters, digits, . and -)" >&2; exit 2 ;; esac
  [ -r "$ca_file" ] || { echo "weavesignal-relay: no certificate authorities at $ca_file" >&2; exit 2; }
  # The shape of a relay set up before the hop (plain HTTP was the only way, on port 80): it now
  # speaks TLS to a plain port and carries nothing.
  if [ "$origin_port" = 80 ] && [ -z "${RELAY_ORIGIN_SCHEME:-}" ]; then
    echo "weavesignal-relay: RELAY_ORIGIN_PORT=80 with the default RELAY_ORIGIN_SCHEME=https: the relay now pulls over TLS (ADR-0162); use the station's HTTPS port (443, the default), or set RELAY_ORIGIN_SCHEME=http to keep the plain pull" >&2
  fi
fi
[ "$retry" -ge 15 ] || { echo "weavesignal-relay: RELAY_RETRY_S is at least 15 (Icecast 2.5 refuses less)" >&2; exit 2; }
admin=$(secret RELAY_ADMIN_PASSWORD)
[ -n "$admin" ] || { admin=$(random); echo "weavesignal-relay: no RELAY_ADMIN_PASSWORD; the admin pages are closed"; }
origin_password=$(secret RELAY_ORIGIN_PASSWORD)

# With the hop, the hop sends the listener login (so Icecast's own needs none, and the password
# is written only to the hop's configuration, mode 600); without it, Icecast does.
login=""
if [ -n "$user" ] && [ "$scheme" = http ]; then
  login="<username>$(xml "$user")</username><password>$(xml "$origin_password")</password>"
fi
# Where Icecast pulls from: the station itself over plain HTTP, or the hop on loopback.
if [ "$scheme" = https ]; then
  up_server=127.0.0.1; up_port=$hop_port
else
  up_server=$(xml "$RELAY_ORIGIN_HOST"); up_port=$origin_port
fi
relays=""
count=0
for m in $mounts; do
  case $m in /*) ;; *) echo "weavesignal-relay: mounts start with / ($m)" >&2; exit 2 ;; esac
  case $m in *[!A-Za-z0-9._/~-]*) echo "weavesignal-relay: a mount holds letters, digits and . _ ~ - / only ($m)" >&2; exit 2 ;; esac
  count=$((count + 1))
  # relay-shoutcast-metadata off (ADR-0052, bench 02); Ogg carries its own metadata in-band.
  # Icecast 2.5.0 ignores it inside <upstream type="normal"> and still asks for ICY metadata,
  # and the relay then passes nothing (M7, 2026-10-26): it must sit in the default block.
  relays="$relays
  <relay>
    <local-mount>$(xml "$m")</local-mount>
    <on-demand>false</on-demand>
    <upstream type=\"default\">
      <relay-shoutcast-metadata>false</relay-shoutcast-metadata>
    </upstream>
    <upstream type=\"normal\">
      <server>$up_server</server>
      <port>$up_port</port>
      <mount>$(xml "$m")</mount>
      $login
    </upstream>
  </relay>"
done
bind_xml=""
[ -n "$bind" ] && bind_xml="<bind-address>$(xml "$bind")</bind-address>"

conf=/run/weavesignal-relay/icecast.xml
umask 077
# A restarted container keeps its writable layer: drop the last run's pid files, or the wait for
# the hop below would end at once on the old one and Icecast would ask before nginx listens.
rm -f /run/weavesignal-relay/hop.pid /run/weavesignal-relay/icecast.pid
cat > "$conf" <<XML
<icecast>
  <!-- Written by weavesignal-relay at start; edit the environment, not this file. -->
  <location>WeaveSignal edge</location>
  <admin>nobody@invalid</admin>
  <hostname>localhost</hostname>
  <limits>
    <clients>$max</clients>
    <sources>$count</sources>
    <queue-size>524288</queue-size>
    <burst-size>65535</burst-size>
    <client-timeout>30</client-timeout>
    <header-timeout>15</header-timeout>
    <source-timeout>10</source-timeout>
  </limits>
  <authentication>
    <source-password>$(random)</source-password>
    <relay-password>$(random)</relay-password>
    <admin-user>admin</admin-user>
    <admin-password>$(xml "$admin")</admin-password>
  </authentication>
  <listen-socket><port>$port</port>$bind_xml</listen-socket>
  <master-update-interval>$retry</master-update-interval>
  $relays
  <fileserve>0</fileserve>
  <paths>
    <basedir>/usr/share/icecast</basedir>
    <logdir>/run/weavesignal-relay</logdir>
    <pidfile>/run/weavesignal-relay/icecast.pid</pidfile>
    <webroot>/usr/share/icecast/web</webroot>
    <adminroot>/usr/share/icecast/admin</adminroot>
  </paths>
  <logging><accesslog>-</accesslog><errorlog>-</errorlog><loglevel>information</loglevel></logging>
  <security><chroot>0</chroot></security>
</icecast>
XML

# The hop (https): nginx on loopback, one exact-match location per mount, each proxied to the
# station's HTTPS address with its certificate checked against the name asked for. The Host
# header is the station's, so its reverse proxy routes the request as it would a browser's. The
# listener login is added by the hop (any Authorization header from Icecast is replaced), so it
# only ever crosses the network encrypted. Nothing is buffered or cached: it is a live stream.
hop_pid=''
if [ "$scheme" = https ]; then
  host_header=$RELAY_ORIGIN_HOST
  [ "$origin_port" = 443 ] || host_header=$RELAY_ORIGIN_HOST:$origin_port
  # nginx resolves the station's name when it starts or reloads, as the container does (its DNS
  # and /etc/hosts); the supervisor below reloads it when the name's addresses change.
  # Basic authentication: base64 of user:password, a safe alphabet for the configuration.
  auth='""'
  if [ -n "$user" ]; then auth="\"Basic $(printf %s "$user:$origin_password" | base64 | tr -d '\n')\""; fi
  locations=""
  for m in $mounts; do
    locations="$locations
    location = $m {
      proxy_pass https://$RELAY_ORIGIN_HOST:$origin_port;
    }"
  done
  hop=/run/weavesignal-relay/hop.conf
  cat > "$hop" <<NGINX
worker_processes 1;
pid /run/weavesignal-relay/hop.pid;
error_log stderr warn;
events { worker_connections 256; }
http {
  access_log off;
  client_body_temp_path /run/weavesignal-relay/tmp-body;
  proxy_temp_path /run/weavesignal-relay/tmp-proxy;
  fastcgi_temp_path /run/weavesignal-relay/tmp-fastcgi;
  uwsgi_temp_path /run/weavesignal-relay/tmp-uwsgi;
  scgi_temp_path /run/weavesignal-relay/tmp-scgi;
  server {
    listen 127.0.0.1:$hop_port;
    proxy_http_version 1.1;
    proxy_buffering off;
    proxy_request_buffering off;
    proxy_set_header Host $host_header;
    proxy_set_header Authorization $auth;
    # Never ask the station for ICY metadata, whatever Icecast sends (ADR-0052, bench 02).
    proxy_set_header Icy-MetaData "";
    proxy_ssl_server_name on;
    proxy_ssl_name $RELAY_ORIGIN_HOST;
    proxy_ssl_protocols TLSv1.2 TLSv1.3;
    proxy_ssl_verify on;
    proxy_ssl_verify_depth 3;
    proxy_ssl_trusted_certificate $ca_file;
    proxy_connect_timeout 10s;
    proxy_read_timeout 60s;
    $locations
    location / { return 404; }
  }
}
NGINX
  nginx -e stderr -c "$hop" -g 'daemon off;' &
  hop_pid=$!
  # Icecast tries its relays as it starts: wait for the hop to be listening.
  for _ in $(seq 1 50); do
    [ -s /run/weavesignal-relay/hop.pid ] && break
    kill -0 "$hop_pid" 2>/dev/null || { echo "weavesignal-relay: the TLS hop did not start" >&2; exit 1; }
    sleep 0.1
  done
  [ -s /run/weavesignal-relay/hop.pid ] || { echo "weavesignal-relay: the TLS hop did not start" >&2; exit 1; }
fi
echo "weavesignal-relay: relaying$(printf ' %s' $mounts) from $scheme://$RELAY_ORIGIN_HOST:$origin_port${hop_pid:+ through the hop on 127.0.0.1:$hop_port} on $port"
if [ -z "$hop_pid" ]; then exec icecast -c "$conf"; fi

# With the hop there are two processes: if either ends, so does the container, and a stop signal
# reaches both, so the supervisor (Podman, systemd) sees the failure and restarts the relay.
icecast -c "$conf" &
ice_pid=$!
stopping=false
trap 'stopping=true; kill -TERM "$hop_pid" "$ice_pid" 2>/dev/null || true' TERM INT
resolve() { getent hosts "$RELAY_ORIGIN_HOST" 2>/dev/null | awk '{ print $1 }' | sort | tr '\n' ' '; }
known=$(resolve); tick=0
while kill -0 "$hop_pid" 2>/dev/null && kill -0 "$ice_pid" 2>/dev/null; do
  sleep 1
  tick=$((tick + 1))
  if [ "$tick" -ge 30 ]; then
    # The station moved to another address: the hop looks the name up again (a reload keeps the
    # streams running on the old connections until they end).
    tick=0; now=$(resolve)
    if [ -n "$now" ] && [ "$now" != "$known" ]; then known=$now; kill -HUP "$hop_pid" 2>/dev/null || true; fi
  fi
done
kill -TERM "$hop_pid" "$ice_pid" 2>/dev/null || true
wait "$hop_pid" 2>/dev/null || true
wait "$ice_pid" 2>/dev/null || true
[ "$stopping" = true ] && exit 0
echo "weavesignal-relay: the hop or Icecast ended" >&2
exit 1
