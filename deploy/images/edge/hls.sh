#!/bin/sh
# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# WeaveSignal's HLS edge: writes nginx's configuration from the environment, then runs nginx.
#
#   EDGE_ORIGIN         required: the station as the edge reaches it, `https://host` or
#                       `http://host:port` (no path). The edge pulls the HLS path from it.
#   EDGE_HTTP_PORT      plain HTTP: viewers, and the steering endpoint's health check (80). With
#                       a certificate mounted it answers `/health` only and redirects the rest
#                       to HTTPS (ADR-0161)
#   EDGE_HTTPS_PORT     HTTPS, only when a certificate is mounted (443)
#   EDGE_BIND           the address both listen on (every interface; 127.0.0.1 on a test bench)
#   EDGE_TLS_DIR        holds fullchain.pem and privkey.pem: the edge pool's wildcard
#                       certificate (/etc/weavesignal-edge/tls)
#   EDGE_HLS_PATH       the only path the edge serves from the origin (/hls/)
#   EDGE_HEALTH_PATH    the station's own health address, which `/health` passes through to
#                       (/health): an edge is healthy only while the station answers it
#   EDGE_PLAYLIST_TTL   how long a playlist is cached (1s: half a 2 s segment, bench 02)
#   EDGE_SEGMENT_TTL    how long a segment is cached (60s; a live window is about 12 s)
#   EDGE_CACHE_MAX      the cache's size on disk (2g)
#   EDGE_RELAY          optional `host:port` of an audio relay beside this edge; its mounts are
#                       then served under /listen/, so HTTPS players reach them too
#   EDGE_WORKERS        nginx worker processes (auto)
#
# Nothing here is a secret of the station's: the edge fetches what any viewer may fetch, and
# drops cookies and credentials both ways. `/health` is the station's health answer fetched
# through the edge (ADR-0160), so it fails when nginx, the edge's route to the station or the
# station itself is down.
set -eu

: "${EDGE_ORIGIN:?set EDGE_ORIGIN to the station's address, such as https://station.example}"
http_port=${EDGE_HTTP_PORT:-80}
https_port=${EDGE_HTTPS_PORT:-443}
bind=${EDGE_BIND:-}
tls_dir=${EDGE_TLS_DIR:-/etc/weavesignal-edge/tls}
hls_path=${EDGE_HLS_PATH:-/hls/}
health_path=${EDGE_HEALTH_PATH:-/health}
playlist_ttl=${EDGE_PLAYLIST_TTL:-1s}
segment_ttl=${EDGE_SEGMENT_TTL:-60s}
cache_max=${EDGE_CACHE_MAX:-2g}
workers=${EDGE_WORKERS:-auto}
relay=${EDGE_RELAY:-}

case $EDGE_ORIGIN in
  http://*|https://*) ;;
  *) echo "weavesignal-edge: EDGE_ORIGIN must start with https:// (or http:// on a test bench)" >&2; exit 2 ;;
esac
origin=${EDGE_ORIGIN%/}
case $origin in
  *://*/*) echo "weavesignal-edge: EDGE_ORIGIN is a scheme and host only, without a path" >&2; exit 2 ;;
esac
case $hls_path in
  /*/) ;;
  *) echo "weavesignal-edge: EDGE_HLS_PATH starts and ends with /, such as /hls/" >&2; exit 2 ;;
esac
case $health_path in
  /*[!A-Za-z0-9/._~-]*|'') echo "weavesignal-edge: EDGE_HEALTH_PATH starts with / and holds letters, digits and /._~- only, such as /health" >&2; exit 2 ;;
  /*) ;;
  *) echo "weavesignal-edge: EDGE_HEALTH_PATH starts with /, such as /health" >&2; exit 2 ;;
esac
for v in "$http_port" "$https_port"; do
  case $v in ''|*[!0-9]*) echo "weavesignal-edge: ports are numbers ($v)" >&2; exit 2 ;; esac
done
at() { if [ -n "$bind" ]; then echo "$bind:$1"; else echo "$1"; fi; }

plain="listen $(at "$http_port") reuseport;"
secure=""
tls=""
port_suffix=""
if [ -f "$tls_dir/fullchain.pem" ] && [ -f "$tls_dir/privkey.pem" ]; then
  secure="listen $(at "$https_port") ssl reuseport;"
  tls="ssl_certificate $tls_dir/fullchain.pem;
    ssl_certificate_key $tls_dir/privkey.pem;
    ssl_protocols TLSv1.2 TLSv1.3;
    ssl_session_cache shared:tls:10m;"
  [ "$https_port" = 443 ] || port_suffix=":$https_port"
  echo "weavesignal-edge: HTTPS on $https_port with the certificate in $tls_dir; plain HTTP on $http_port answers /health and redirects the rest"
else
  echo "weavesignal-edge: no certificate in $tls_dir; plain HTTP only"
fi

relay_block=""
if [ -n "$relay" ]; then
  # Listener streams are long responses: unbuffered, never cached, no time limit while bytes flow.
  relay_block="location ^~ /listen/ {
      proxy_pass http://$relay/;
      proxy_buffering off;
      proxy_cache off;
      proxy_read_timeout 30s;
      proxy_set_header Icy-MetaData \"\";
    }"
fi

# The edge's /health is the station's own health answer, fetched through the edge (ADR-0160): it
# fails, quickly, while nginx, the edge's route to the station or the station is down. The
# answer is the station's (a 2xx while it is well), never cached.
health_block="location = /health {
      access_log off;
      proxy_pass $origin$health_path;
      proxy_http_version 1.1;
      proxy_set_header Connection \"\";
      proxy_set_header Host \$proxy_host;
      proxy_ssl_server_name on;
      proxy_ssl_verify on;
      proxy_ssl_trusted_certificate /etc/ssl/certs/ca-certificates.crt;
      proxy_set_header Cookie \"\";
      proxy_set_header Authorization \"\";
      proxy_cache off;
      proxy_connect_timeout 2s; proxy_send_timeout 2s; proxy_read_timeout 2s;
      add_header Cache-Control \"no-store\" always;
    }"

# What the edge serves to viewers: the HLS path, the relay's mounts, nothing else.
content="    location ^~ $hls_path {
      proxy_pass $origin;
      proxy_http_version 1.1;
      proxy_set_header Connection \"\";
      proxy_set_header Host \$proxy_host;
      proxy_ssl_server_name on;
      proxy_ssl_verify on;
      proxy_ssl_trusted_certificate /etc/ssl/certs/ca-certificates.crt;
      # Nothing personal crosses the edge, and the edge's rules, not the origin's headers,
      # decide what is cached. The query string is not part of the key: it cannot bust the cache.
      proxy_set_header Cookie \"\";
      proxy_set_header Authorization \"\";
      proxy_hide_header Set-Cookie;
      proxy_ignore_headers Cache-Control Expires Set-Cookie;
      # And viewers get the edge's Cache-Control only: passed on, the origin's sat beside it
      # (a playlist said both no-cache and max-age=1, a segment max-age=30 and max-age=60).
      proxy_hide_header Cache-Control;
      proxy_hide_header Expires;
      proxy_cache hls;
      proxy_cache_key \$uri;
      # One fill per key: simultaneous misses wait for it (bench 02's cold start: 46 origin
      # requests for 4,000 viewers arriving in 0.2 s).
      proxy_cache_lock on; proxy_cache_lock_timeout 3s; proxy_cache_lock_age 3s;
      proxy_cache_use_stale updating error timeout http_502 http_503 http_504;
      proxy_cache_background_update on;
      add_header X-Cache \$upstream_cache_status always;
      add_header Access-Control-Allow-Origin \"*\" always;
      location ~ \.m3u8\$ {
        proxy_pass $origin;
        proxy_cache_valid 200 $playlist_ttl; proxy_cache_valid 404 1s;
        add_header Cache-Control \"max-age=1\" always;
        add_header X-Cache \$upstream_cache_status always;
        add_header Access-Control-Allow-Origin \"*\" always;
      }
      location ~ \.(m4s|mp4|m4a|ts|vtt)\$ {
        proxy_pass $origin;
        proxy_cache_valid 200 $segment_ttl; proxy_cache_valid 404 1s;
        add_header Cache-Control \"max-age=60\" always;
        add_header X-Cache \$upstream_cache_status always;
        add_header Access-Control-Allow-Origin \"*\" always;
      }
      return 404;
    }
    $relay_block
    location / { return 404; }"

if [ -n "$secure" ]; then
  # Plain HTTP answers the health check and sends everyone else to HTTPS (ADR-0161).
  servers="server {
    $plain
    $health_block
    location / { return 301 https://\$host$port_suffix\$request_uri; }
  }
  server {
    $secure
    $tls
    $health_block
$content
  }"
else
  servers="server {
    $plain
    $health_block
$content
  }"
fi

cat > /etc/nginx/nginx.conf <<CONF
# Written by weavesignal-edge at start; edit the environment, not this file.
worker_processes $workers;
pid /run/nginx/nginx.pid;
error_log /dev/stderr warn;
events { worker_connections 16384; }
http {
  sendfile on; tcp_nopush on; keepalive_requests 100000;
  server_tokens off;
  types {
    application/vnd.apple.mpegurl m3u8;
    video/mp4 mp4 m4s;
    audio/mp4 m4a;
    video/mp2t ts;
    text/vtt vtt;
  }
  access_log off;
  proxy_cache_path /var/cache/weavesignal-edge levels=1:2 keys_zone=hls:16m max_size=$cache_max inactive=5m use_temp_path=off;
  $servers
}
CONF
nginx -t -q
echo "weavesignal-edge: pulling $hls_path from $origin, listening on $http_port"
exec nginx -g 'daemon off;'
