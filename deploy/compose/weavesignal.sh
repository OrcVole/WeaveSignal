#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# weavesignal.sh: install, start and remove a WeaveSignal station with the Compose bundle (ADR-0022).
#
#   ./weavesignal.sh                  install or update: settings, secrets, free ports, then start
#   ./weavesignal.sh --no-start       write the settings only
#   ./weavesignal.sh --no-boot-start  install, but on Podman leave start at boot alone (below)
#   ./weavesignal.sh --public station.example
#                                   also publish the station: a reverse proxy with a certificate for
#                                   that name, and LiveKit's TURN over TLS (below); --public off
#                                   goes back to loopback only
#   ./weavesignal.sh --renew-turn     restart LiveKit so TURN over TLS reads a renewed certificate
#   ./weavesignal.sh --status         show the station's containers
#   ./weavesignal.sh --uninstall      stop and remove the containers; keep the data and settings
#   ./weavesignal.sh --uninstall --purge --yes
#                                   also delete the data volumes and the settings with their secrets
#
# Options: --project NAME (default weavesignal), --env-file PATH (default .env beside this script),
# --image IMAGE (the server image; default forgejo.weavesignal.top/weavesignal/weavesignal-server:1.0.0, pulled when absent; a localhost/ image must be built first), --engine podman|docker.
# Each port may be given in the environment (CONTROL_PORT=17310 ./weavesignal.sh); see .env.example.
#
# It runs Podman (with `podman compose`) or Docker (with `docker compose`); it never uses sudo.
# On Podman, after the station is up, it turns on start at boot (ADR-0138): lingering for your
# user (`loginctl enable-linger`) and your user's podman-restart.service, and says which it
# changed. --no-boot-start leaves both alone; --uninstall never turns them off (other containers
# may rely on them). Docker needs nothing: its daemon starts with the machine.
# Secrets are made here, from the kernel's random source, and written only to the settings file
# (mode 600); they are never printed. Running it again keeps every secret and port already chosen.
# Without --public every listener is on loopback, so nobody off this machine can join or listen.
# With it (ADR-0022) public.yaml adds Caddy, which gets and renews the certificate for the name
# and serves the station on 443 (80 redirects), and LiveKit gains TURN over TLS (5349) with that
# certificate. The name must already reach this machine, and ports 80 and 443 (TCP), LiveKit's
# media port (UDP) and the TURN port (TCP) must be open to the internet. A rootless Podman cannot
# bind 80 and 443 unless `net.ipv4.ip_unprivileged_port_start` allows it: the script says so.
# Settings (environment or the settings file): PUBLIC_TLS=internal (Caddy's own CA, for a private
# network or a test; browsers do not trust it), ACME_EMAIL, NODE_IP (LiveKit's public address;
# else found by STUN), TURN_ALLOW_CIDRS (addresses TURN may relay to, for a private NODE_IP),
# PROXY_HTTP_PORT, PROXY_HTTPS_PORT, TURN_TLS_PORT, TURN_UDP_PORT and LIVEKIT_TCP_PORT (0 is off).
set -euo pipefail
umask 077

here=$(cd "$(dirname "$0")" && pwd)
project=${WEAVESIGNAL_PROJECT:-weavesignal}
env_file=${WEAVESIGNAL_ENV_FILE:-$here/.env}
image=${SERVER_IMAGE:-}
engine=${WEAVESIGNAL_ENGINE:-}
action=install
purge=false
yes=false
start=true
boot_start=true
public_arg=''      # --public's value; "off" turns the public route off

say() { printf '%s\n' "$*"; }
die() { printf 'weavesignal.sh: %s\n' "$*" >&2; exit 1; }
usage() { sed -n '5,40p' "$0" | sed 's/^# \{0,1\}//'; }

while [ $# -gt 0 ]; do
  case $1 in
    --uninstall) action=uninstall ;;
    --purge) purge=true ;;
    --yes|-y) yes=true ;;
    --status) action=status ;;
    --renew-turn) action=renew-turn ;;
    --public) public_arg=${2:?--public needs a domain name (or off)}; shift ;;
    --no-start) start=false ;;
    --no-boot-start) boot_start=false ;;
    --project) project=${2:?--project needs a name}; shift ;;
    --env-file) env_file=${2:?--env-file needs a path}; shift ;;
    --image) image=${2:?--image needs an image}; shift ;;
    --engine) engine=${2:?--engine needs podman or docker}; shift ;;
    -h|--help) usage; exit 0 ;;
    *) die "unknown option $1 (--help lists them)" ;;
  esac
  shift
done
[ "$purge" = false ] || [ "$action" = uninstall ] || die "--purge goes with --uninstall"
[[ $project =~ ^[a-z0-9][a-z0-9_-]*$ ]] || die "the project name may hold only a-z, 0-9, - and _"
# Compose runs from this script's directory, so the settings file's path is made absolute.
case $env_file in /*) ;; *) env_file=$PWD/$env_file ;; esac

# --- The container engine ------------------------------------------------------------------
# podman-compose (1.6) ignores the top-level `configs`, which carry every service's settings
# file (playout's, LiveKit's, the desk's, the control plane's, the chat's and the proxy's):
# the bundle would start without them (the off-site test station, 2026-11-03: Caddy ran its
# default site, no certificate, and `up` waited for ever). Podman runs the bundle with
# docker-compose as its compose provider.
podman_compose_provider() {
  local v
  v=$(podman compose version 2>&1) || true
  grep -Eq '^podman-compose version|provider "[^"]*podman-compose"' <<<"$v"
}
no_podman_compose="Podman's compose provider here is podman-compose, which ignores the settings
files this bundle gives each service. Install docker-compose (Fedora: 'dnf install docker-compose';
Ubuntu: 'apt install docker-compose-v2'), which 'podman compose' then prefers, or
point Podman at it with PODMAN_COMPOSE_PROVIDER=/path/to/docker-compose; or use Docker."
if [ -z "$engine" ]; then
  if command -v podman >/dev/null 2>&1 && podman compose version >/dev/null 2>&1 && ! podman_compose_provider; then engine=podman
  elif command -v docker >/dev/null 2>&1 && docker compose version >/dev/null 2>&1; then engine=docker
  elif command -v podman >/dev/null 2>&1 && podman_compose_provider; then die "$no_podman_compose"
  else
    die "neither Podman with 'podman compose' nor Docker with 'docker compose' was found.
Install Podman 5 with docker-compose as its compose provider, or Docker Engine with the Compose
plugin, then run this again."
  fi
fi
case $engine in
  podman|docker) command -v "$engine" >/dev/null 2>&1 || die "$engine is not installed" ;;
  *) die "--engine is podman or docker" ;;
esac
"$engine" compose version >/dev/null 2>&1 || die "'$engine compose' does not work here"
[ "$engine" = docker ] || ! podman_compose_provider || die "$no_podman_compose"
# Said on install, as Podman with podman-compose beside Docker now means Docker.
[ "$action" != install ] || say "Container engine: $engine"
# The public route is the overlay public.yaml, used whenever the settings file names a domain, so
# --status and --uninstall see the proxy too. Install sets $public_domain from --public first.
public_domain=''
[ -f "$env_file" ] && public_domain=$(sed -n 's/^PUBLIC_DOMAIN=//p' "$env_file" | head -1)
compose() {
  local files=(-f compose.yaml); [ -z "$public_domain" ] || files+=(-f public.yaml)
  (cd "$here" && "$engine" compose --env-file "$env_file" -p "$project" "${files[@]}" "$@")
}
running() { [ -f "$env_file" ] && [ -n "$(compose ps -q 2>/dev/null)" ]; }

# --- Status and removal --------------------------------------------------------------------
if [ "$action" = status ]; then
  [ -f "$env_file" ] || die "no settings at $env_file: the station is not installed"
  compose ps
  exit 0
fi
if [ "$action" = renew-turn ]; then
  [ -f "$env_file" ] || die "no settings at $env_file: the station is not installed"
  [ -n "$public_domain" ] || die "this station is not public (PUBLIC_DOMAIN is not set): it has no certificate for TURN over TLS"
  compose restart livekit
  say "Restarted LiveKit: it read the proxy's current certificate, and everyone in the room was dropped."
  exit 0
fi
if [ "$action" = uninstall ]; then
  [ -f "$env_file" ] || die "no settings at $env_file: nothing to remove"
  if [ "$purge" = true ]; then
    if [ "$yes" != true ]; then
      [ -t 0 ] || die "--purge deletes the station's data; add --yes to confirm"
      read -r -p "Delete the station's database, carts and settings for '$project'? Type purge: " answer
      [ "$answer" = purge ] || die "nothing deleted"
    fi
    compose down --volumes --remove-orphans
    rm -f -- "$env_file"
    say "Removed the station '$project', its data volumes and its settings."
  else
    compose down --remove-orphans
    say "Stopped and removed the station '$project'. Its data volumes and $env_file are kept;"
    say "run this script again to start it, or add --purge --yes to delete them."
  fi
  exit 0
fi

# --- Settings and secrets ------------------------------------------------------------------
declare -A have=()
if [ -f "$env_file" ]; then
  while IFS= read -r line; do
    [[ $line =~ ^([A-Z_][A-Z0-9_]*)=(.*)$ ]] && have[${BASH_REMATCH[1]}]=${BASH_REMATCH[2]}
  done < "$env_file"
fi
secret() { # 40 characters from the kernel's random source, letters and digits only
  local s=''
  while [ ${#s} -lt 40 ]; do s+=$(head -c 64 /dev/urandom | base64 | tr -dc 'A-Za-z0-9'); done
  printf '%s' "${s:0:40}"
}
keep() { # key default: the value in the settings file, else the environment, else the default
  local key=$1 def=$2
  if [ -n "${have[$key]:-}" ]; then printf '%s' "${have[$key]}"
  elif [ -n "${!key:-}" ]; then printf '%s' "${!key}"
  else printf '%s' "$def"; fi
}
station_id=$(keep STATION_ID "$(cat /proc/sys/kernel/random/uuid)")
[ "$station_id" != 00000000-0000-0000-0000-000000000000 ] || station_id=$(cat /proc/sys/kernel/random/uuid)
station_name=$(keep STATION_NAME "Kettle FM")
room=$(keep ROOM studio)
# The demo station (the demo name) is given the demo slate picture once (ADR-0200); a station
# with its own name starts with none. A value already in the settings file is kept, empty or not.
if [ -n "${have[DEMO_STILL]+set}" ]; then demo_still=${have[DEMO_STILL]}
elif [ "$station_name" = "Kettle FM" ]; then demo_still=/opt/weavesignal/stills-demo/kettle-fm.png
else demo_still=''; fi
image=${image:-$(keep SERVER_IMAGE forgejo.weavesignal.top/weavesignal/weavesignal-server:1.0.0)}
lk_key=$(keep LIVEKIT_API_KEY weavesignal)
# Secrets are never taken from the environment: only from the settings file, or made here.
lk_secret=${have[LIVEKIT_API_SECRET]:-$(secret)}
[ ${#lk_secret} -ge 32 ] || die "LIVEKIT_API_SECRET in $env_file is shorter than 32 characters"
src_pw=${have[LISTENERS_SOURCE_PASSWORD]:-$(secret)}
pg_pw=${have[POSTGRES_PASSWORD]:-$(secret)}
admin=${have[CONTROL_ADMIN_TOKEN]:-$(secret)}
# Keys the hash of chatters' email addresses (ADR-0125). Made once and then kept: a new one would
# lose every chat account made under the old.
chat_key=${have[CHAT_EMAIL_KEY]:-$(secret)}
[ ${#chat_key} -ge 32 ] || die "CHAT_EMAIL_KEY in $env_file is shorter than 32 characters"
# Keys the hash of staff email addresses and sign-in codes (ADR-0127), made and kept the same way.
control_key=${have[CONTROL_EMAIL_KEY]:-$(secret)}
[ ${#control_key} -ge 32 ] || die "CONTROL_EMAIL_KEY in $env_file is shorter than 32 characters"
# Keys API tokens and webhook secrets (ADR-0195, ADR-0202), made once and then kept: a new one
# revokes every token. An older station gains one on this run, which revokes the tokens it had.
token_key=${have[CONTROL_TOKEN_KEY]:-$(secret)}
[ ${#token_key} -ge 32 ] || die "CONTROL_TOKEN_KEY in $env_file is shorter than 32 characters"
mail_mode=$(keep MAIL_MODE off)
mail_from=$(keep MAIL_FROM chat@station.example)
consent_contact=$(keep CONSENT_CONTACT "")
search_listing=$(keep SEARCH_LISTING false)
directory_listing=$(keep DIRECTORY_LISTING false)

# --- The public route (opt-in): a name for the station, and what follows from it -------------
case $public_arg in
  '') ;;                                  # keep what the settings file says
  off) public_domain='' ;;
  *) public_domain=$public_arg ;;
esac
public_tls=''; acme_email=''; node_ip=''; turn_cidrs=''
if [ -n "$public_domain" ]; then
  [[ $public_domain =~ ^[a-z0-9]([a-z0-9-]*[a-z0-9])?(\.[a-z0-9]([a-z0-9-]*[a-z0-9])?)+$ ]] \
    || die "--public needs a lower-case domain name such as station.example (no scheme, port or path), not '$public_domain'"
  public_tls=$(keep PUBLIC_TLS acme)
  case $public_tls in acme|internal) ;; *) die "PUBLIC_TLS is acme (a certificate from Let's Encrypt) or internal (Caddy's own CA)" ;; esac
  acme_email=$(keep ACME_EMAIL "")
  [[ -z $acme_email || $acme_email =~ ^[^[:space:]@]+@[^[:space:]@]+$ ]] || die "ACME_EMAIL is not an email address"
  node_ip=$(keep NODE_IP "")
  [[ -z $node_ip || $node_ip =~ ^[0-9a-fA-F:.]+$ ]] || die "NODE_IP is an IP address (LiveKit's public one), or empty to find it by STUN"
  turn_cidrs=$(keep TURN_ALLOW_CIDRS "")
  [[ -z $turn_cidrs || $turn_cidrs =~ ^[0-9a-fA-F:./,\ ]+$ ]] || die "TURN_ALLOW_CIDRS is a comma-separated list of addresses such as 10.0.0.0/8, 192.168.0.0/16"
fi

# --- Ports (ADR-0075): each free, or the next suggested alternative ------------------------
# key protocol default alternatives..., as the control plane's own check suggests them
# (crates/control/src/ports.rs; docs: reference/ports)
port_specs=(
  "CONTROL_PORT tcp 7310 17310 27310"
  "CHAT_PORT tcp 7311 17311 27311"
  "LISTENERS_PORT tcp 7312 17312 27312"
  "SRT_INGEST_PORT udp 7313 17313 27313"
  "PLAYOUT_FEED_PORT tcp 7315 17315 27315"
  "DESK_FEED_PORT tcp 7317 17317 27317"
  "LIVEKIT_HTTP_PORT tcp 7880 17880 27880"
  # 443, the control plane's other suggestion, needs privileges a rootless engine lacks.
  "LIVEKIT_MEDIA_PORT udp 7882 52000"
)
# The public route's ports (a public station only): the proxy's two, which must stay the ones a
# certificate authority reaches (443 and 80) unless the machine forwards them, so a taken one
# stops the script rather than moving; and TURN over TLS. 443, the control plane's other
# suggestion for TURN, is the proxy's.
strict_keys=" PROXY_HTTPS_PORT PROXY_HTTP_PORT "
if [ -n "$public_domain" ]; then
  port_specs+=(
    "PROXY_HTTPS_PORT tcp 443 8443"
    "PROXY_HTTP_PORT tcp 80 8080"
    "TURN_TLS_PORT tcp 5349 15349"
  )
fi
# The bundle binds every port to loopback except LiveKit's media port, which takes every
# address (and, on a public station, the proxy's and TURN's). A loopback port clashes with a listener on loopback or on every address; one bound to
# another interface's address does not stop it.
public_keys=" LIVEKIT_MEDIA_PORT PROXY_HTTPS_PORT PROXY_HTTP_PORT TURN_TLS_PORT TURN_UDP_PORT LIVEKIT_TCP_PORT "
in_use() { # protocol port [any]: true when a listener would stop the bundle binding it
  local flag=-Hltn; [ "$1" = udp ] && flag=-Hlun
  local addrs
  addrs=$(ss "$flag" "sport = :$2" 2>/dev/null | awk '{print $4}')
  [ -n "$addrs" ] || return 1
  [ "${3:-}" = any ] && return 0
  grep -qE '^(127\.0\.0\.1|0\.0\.0\.0|\*|\[::\]|\[::1\]|\[::ffff:127\.0\.0\.1\]):[0-9]+$' <<< "$addrs"
}
command -v ss >/dev/null 2>&1 || die "ss (iproute2) is needed to check the ports"
ours=false; running && ours=true
declare -A port=() taken=()
claim() { taken["$1/$2"]=1; }
free() { # protocol port [any]: nothing listens where the bundle would, and no other setting has chosen it
  local proto=$1 p=$2
  [ -z "${taken["$proto/$p"]:-}" ] && ! in_use "$proto" "$p" "${3:-}"
}
# A rootless Podman binds ports below net.ipv4.ip_unprivileged_port_start only if that allows
# them (1024 by default); a Docker daemon binds for us. WEAVESIGNAL_UNPRIV_PORT_FILE lets the
# tests point at a file of their own.
low_ok() { # port: can this user's engine bind it?
  local p=$1 start
  [ "$p" -ge 1024 ] && return 0
  [ "$(id -u)" = 0 ] && return 0
  [ "$engine" = podman ] || return 0
  start=$(cat "${WEAVESIGNAL_UNPRIV_PORT_FILE:-/proc/sys/net/ipv4/ip_unprivileged_port_start}" 2>/dev/null || echo 1024)
  [ "$p" -ge "$start" ]
}
low_hint() { # key port
  printf '%s=%s is below 1024, which a rootless Podman may not bind. Allow it with `sudo sysctl -w net.ipv4.ip_unprivileged_port_start=%s` (and the same line in /etc/sysctl.d/ to keep it), or use Docker, or choose %s and forward the machine\x27s ports 80 and 443 to the proxy\x27s.' "$1" "$2" "$2" "${3:-another port}"
}
for spec in "${port_specs[@]}"; do
  read -r key proto def alts <<< "$spec"
  scope=''; [[ $public_keys == *" $key "* ]] && scope=any
  strict=false; [[ $strict_keys == *" $key "* ]] && strict=true
  if [ -n "${have[$key]:-}" ]; then
    p=${have[$key]}
    # The station's own containers hold its ports while it runs.
    if [ "$ours" = false ] && ! free "$proto" "$p" "$scope"; then
      die "$key=$p ($proto) in $env_file is in use by another program; free it, or set another port there"
    fi
  elif [ -n "${!key:-}" ]; then
    p=${!key}
    if ! free "$proto" "$p" "$scope"; then
      next=''; for a in $alts; do free "$proto" "$a" "$scope" && low_ok "$a" && { next=$a; break; }; done
      die "$key=$p ($proto) is in use${next:+; $next is free and suggested}"
    fi
  else
    p=''
    if [ "$strict" = true ]; then
      # Not moved to an alternative on its own: the certificate and the addresses printed depend on it.
      if ! free "$proto" "$def" "$scope"; then
        die "$key: $def ($proto) is in use by another program, and a public station needs it; free it, or set $key=${alts%% *} here and forward the machine's port $def to it"
      fi
      p=$def
    else
      for a in $def $alts; do free "$proto" "$a" "$scope" && low_ok "$a" && { p=$a; break; }; done
      [ -n "$p" ] || die "$key: $def and its alternatives ($alts) are all in use; set $key to any free port"
      [ "$p" = "$def" ] || say "$key: $def ($proto) is in use, so $p is used instead"
    fi
  fi
  [[ $p =~ ^[0-9]+$ ]] && [ "$p" -ge 1 ] && [ "$p" -le 65535 ] || die "$key=$p is not a port"
  if [ "$proto" = tcp ] && [ -n "$scope" ] && ! low_ok "$p"; then
    die "$(low_hint "$key" "$p" "${alts%% *}")"
  fi
  port[$key]=$p; claim "$proto" "$p"
done
# TURN over UDP and ICE over TCP are off (0) unless the station sets a port; a set port is
# checked like any other.
if [ -n "$public_domain" ]; then
  for spec in "TURN_UDP_PORT udp 3478 3479 13478" "LIVEKIT_TCP_PORT tcp 7881 17881 27881"; do
    read -r key proto def alts <<< "$spec"
    p=$(keep "$key" 0)
    [[ $p =~ ^[0-9]+$ ]] && [ "$p" -le 65535 ] || die "$key=$p is not a port (0 turns it off)"
    if [ "$p" != 0 ]; then
      if [ "$ours" = false ] || [ -z "${have[$key]:-}" ]; then
        free "$proto" "$p" any || die "$key=$p ($proto) is in use; free it, or choose another port ($def or ${alts%% *} are the usual)"
      fi
      low_ok "$p" || die "$(low_hint "$key" "$p")"
      claim "$proto" "$p"
    fi
    port[$key]=$p
  done
  [ "${port[TURN_TLS_PORT]}" != "${port[PROXY_HTTPS_PORT]}" ] || die "TURN_TLS_PORT and PROXY_HTTPS_PORT are both ${port[TURN_TLS_PORT]}: TURN over TLS needs a port of its own (the proxy does not share 443 with it)"
fi
# Liquidsoap also opens TCP on the port above two of its listeners, on loopback, with no setting
# to turn it off: icecast.server's legacy source port (LISTENERS_PORT + 1) and the playout feed's
# harbor (PLAYOUT_FEED_PORT + 1). Both, like the feed itself, stay internal: never published.
for key in LISTENERS_PORT PLAYOUT_FEED_PORT; do
  above=$(( ${port[$key]} + 1 ))
  if [ "$ours" = false ] && in_use tcp "$above"; then
    die "$key=${port[$key]} needs TCP $above free too (Liquidsoap opens the port above it); choose another $key"
  fi
  [ -z "${taken["tcp/$above"]:-}" ] || die "$key + 1 ($above) clashes with another TCP port chosen"
  claim tcp "$above"
done

# --- What the public route derives from the name and the ports ------------------------------
# Written to the settings file under "derived" and recomputed on every run, so the services'
# addresses always follow the name and the proxy's port. Empty on a loopback station: the
# bundle's own defaults then apply.
origin=''; derived=''
if [ -n "$public_domain" ]; then
  [ "${port[PROXY_HTTPS_PORT]}" = 443 ] && origin=https://$public_domain || origin=https://$public_domain:${port[PROXY_HTTPS_PORT]}
  room_origin=wss://${origin#https://}
  cert_dir=acme-v02.api.letsencrypt.org-directory   # where Caddy stores a Let's Encrypt certificate
  local_certs=''; acme_option=''
  if [ "$public_tls" = internal ]; then cert_dir=local; local_certs=local_certs; fi
  [ -z "$acme_email" ] || acme_option="email $acme_email"
  # LiveKit: the address in its candidates is the one given, else found by STUN. Without a
  # given address it is not a private one, so nothing is added to its TURN allow list.
  if [ -n "$node_ip" ]; then use_external=false; lk_node=$node_ip; else use_external=true; lk_node=''; fi
  derived="PUBLIC_ORIGIN=$origin
ROOM_PUBLIC_URL=$room_origin
STREAM_URL=$origin/programme.opus
FORWARDED_FOR=true
TURN_ENABLED=true
TURN_CERT_DIR=$cert_dir
PROXY_LOCAL_CERTS=$local_certs
PROXY_ACME_EMAIL=$acme_option
LIVEKIT_USE_EXTERNAL_IP=$use_external
LIVEKIT_NODE_IP=$lk_node"
fi

# --- The settings file: written whole, atomically, mode 600 --------------------------------
mkdir -p "$(dirname "$env_file")"
tmp=$(mktemp "$env_file.XXXXXX")
trap 'rm -f "$tmp"' EXIT
{
  # REUSE-IgnoreStart (the settings file's own header, not this script's licence)
  say "# SPDX-License-Identifier: AGPL-3.0-only"
  # REUSE-IgnoreEnd
  say "# Written by weavesignal.sh for the project '$project'. It holds this station's secrets:"
  say "# keep it private (mode 600). Running weavesignal.sh again keeps every value here."
  say "STATION_ID=$station_id"
  say "STATION_NAME=$station_name"
  say "ROOM=$room"
  say "SERVER_IMAGE=$image"
  say "DEMO_STILL=$demo_still"
  say ""
  say "LIVEKIT_API_KEY=$lk_key"
  say "LIVEKIT_API_SECRET=$lk_secret"
  say "LISTENERS_SOURCE_PASSWORD=$src_pw"
  say "POSTGRES_PASSWORD=$pg_pw"
  say "CONTROL_ADMIN_TOKEN=$admin"
  say "# Keys the hash of chatters' email addresses. Keep it with the database's backups: a different"
  say "# key finds none of the existing chat accounts."
  say "CHAT_EMAIL_KEY=$chat_key"
  say "# Keys the hash of staff email addresses for emailed sign-in codes. Keep it with the backups."
  say "CONTROL_EMAIL_KEY=$control_key"
  say "# Keys API tokens and webhook secrets. Keep it with the backups: a different key revokes"
  say "# every token and ends every webhook's signatures."
  say "CONTROL_TOKEN_KEY=$token_key"
  say ""
  say "MAIL_MODE=$mail_mode"
  say "MAIL_FROM=$mail_from"
  say ""
  say "# How a caller asks for their call to be taken down (an email address or a link); call-in"
  say "# stays closed until it is set (ADR-0124)."
  say "CONSENT_CONTACT=$consent_contact"
  say ""
  say "# Listing, off until the station goes public: search engines may list the public address"
  say "# (ADR-0133); the stream asks internet radio directories to list it (ADR-0129)."
  say "SEARCH_LISTING=$search_listing"
  say "DIRECTORY_LISTING=$directory_listing"
  say ""
  if [ -n "$public_domain" ]; then
    say "# The public route (weavesignal.sh --public): the station is served at https://$public_domain."
    say "PUBLIC_DOMAIN=$public_domain"
    say "# acme: a certificate from Let's Encrypt. internal: Caddy's own CA, for a private network or a"
    say "# test (browsers do not trust it)."
    say "PUBLIC_TLS=$public_tls"
    say "ACME_EMAIL=$acme_email"
    say "# LiveKit's public IP address; empty, it is found by STUN. TURN_ALLOW_CIDRS: addresses TURN may"
    say "# relay to besides public ones, e.g. 10.0.0.5/32 for a NODE_IP that is a private address."
    say "NODE_IP=$node_ip"
    say "TURN_ALLOW_CIDRS=$turn_cidrs"
    say ""
  fi
  say "# Ports (ADR-0075): any free port may be used instead."
  for spec in "${port_specs[@]}"; do read -r key _ <<< "$spec"; say "$key=${port[$key]}"; done
  if [ -n "$public_domain" ]; then
    say "# TURN over UDP and ICE over TCP: 0 is off (TURN over TLS reaches the same guests)."
    say "TURN_UDP_PORT=${port[TURN_UDP_PORT]}"
    say "LIVEKIT_TCP_PORT=${port[LIVEKIT_TCP_PORT]}"
    say ""
    say "# Derived from the settings above on every run; change those, not these."
    printf '%s\n' "$derived"
  fi
  # Anything else the file held is kept as it was.
  known=" STATION_ID STATION_NAME ROOM SERVER_IMAGE DEMO_STILL LIVEKIT_API_KEY LIVEKIT_API_SECRET LISTENERS_SOURCE_PASSWORD POSTGRES_PASSWORD CONTROL_ADMIN_TOKEN CHAT_EMAIL_KEY CONTROL_EMAIL_KEY CONTROL_TOKEN_KEY MAIL_MODE MAIL_FROM CONSENT_CONTACT SEARCH_LISTING DIRECTORY_LISTING CONTROL_PORT CHAT_PORT LISTENERS_PORT SRT_INGEST_PORT PLAYOUT_FEED_PORT DESK_FEED_PORT LIVEKIT_HTTP_PORT LIVEKIT_MEDIA_PORT PUBLIC_DOMAIN PUBLIC_TLS ACME_EMAIL NODE_IP TURN_ALLOW_CIDRS PROXY_HTTPS_PORT PROXY_HTTP_PORT TURN_TLS_PORT TURN_UDP_PORT LIVEKIT_TCP_PORT PUBLIC_ORIGIN ROOM_PUBLIC_URL STREAM_URL FORWARDED_FOR TURN_ENABLED TURN_CERT_DIR PROXY_LOCAL_CERTS PROXY_ACME_EMAIL LIVEKIT_USE_EXTERNAL_IP LIVEKIT_NODE_IP "
  extra=''
  for k in "${!have[@]}"; do [[ $known == *" $k "* ]] || extra+="$k=${have[$k]}"$'\n'; done
  [ -z "$extra" ] || { say ""; printf '%s' "$extra" | sort; }
} > "$tmp"
chmod 600 "$tmp"
if [ -f "$env_file" ] && cmp -s "$tmp" "$env_file"; then
  rm -f "$tmp"; say "Settings unchanged: $env_file"
else
  mv -f "$tmp" "$env_file"; say "Settings written: $env_file (mode 600; it holds the secrets)"
fi
trap - EXIT

[ "$start" = true ] || { say "Not started (--no-start)."; exit 0; }

# --- Start at boot, on Podman (ADR-0138) ----------------------------------------------------
# Lingering lets this user's services run (and start at boot) with no login session;
# podman-restart.service starts the containers whose restart policy is always or unless-stopped
# (the bundle's), once the user's manager is up. Each step says what it did. A step that fails
# is reported with the command to run by hand and never stops the install: the station is up.
boot_start_podman() {
  local user linger=false; user=$(id -un)
  say "Start at boot (Podman, user $user):"
  if ! command -v loginctl >/dev/null 2>&1; then
    say "  could not turn on lingering: loginctl was not found; run 'loginctl enable-linger' where systemd is"
  elif [ "$(loginctl show-user "$user" -p Linger --value 2>/dev/null || true)" = yes ]; then
    linger=true; say "  lingering was already on"
  # --no-ask-password: where lingering needs an administrator, say so rather than stop the
  # install at a password prompt.
  elif loginctl --no-ask-password enable-linger "$user" 2>/dev/null; then
    linger=true; say "  turned on lingering (loginctl enable-linger): your services now run without a login session"
  else
    say "  could not turn on lingering; run 'loginctl enable-linger' yourself (it may need an administrator)"
  fi
  # Enabled, not started: the station is already up, and starting the service now would also
  # start your other stopped containers that have a restart policy.
  local when="after a reboot"
  [ "$linger" = true ] || when="at your next login after a reboot (at boot once lingering is on)"
  if ! command -v systemctl >/dev/null 2>&1; then
    say "  could not enable podman-restart.service: systemctl was not found"
  elif systemctl --user is-enabled --quiet podman-restart.service 2>/dev/null; then
    say "  podman-restart.service was already enabled: the containers start again $when"
  elif systemctl --user enable podman-restart.service >/dev/null 2>&1; then
    say "  enabled podman-restart.service (systemctl --user enable): the containers start again $when"
  else
    say "  could not enable podman-restart.service; run 'systemctl --user enable podman-restart.service' yourself"
  fi
  say "  --no-boot-start skips both; --uninstall leaves them as they are."
}

# --- Start -----------------------------------------------------------------------------------
if ! "$engine" image inspect "$image" >/dev/null 2>&1; then
  case $image in
    localhost/*) die "the server image $image is not on this machine.
Build it from the repository's root with 'just desk-image server-image' (WEAVESIGNAL_SERVER_IMAGE
names the tag), or give another with --image." ;;
    *) say "Pulling the server image $image..."
       "$engine" pull "$image" >/dev/null || die "could not pull the server image $image; check the name and this machine's network, or give another with --image." ;;
  esac
fi
if [ -n "$public_domain" ]; then
  say "Starting with the public route: Caddy asks for the certificate for $public_domain first (up to a few minutes)..."
  compose up -d --remove-orphans || {
    compose logs --tail 20 proxy >&2 || true
    die "the proxy did not get a certificate for $public_domain (logs above). The name must reach this machine, and TCP ports ${port[PROXY_HTTP_PORT]} and ${port[PROXY_HTTPS_PORT]} must be open to the internet (forwarded to those ports when they are not 80 and 443). PUBLIC_TLS=internal uses Caddy's own CA instead. Run this again once it is fixed."
  }
else
  compose up -d --remove-orphans
fi
say "Waiting for the control plane and its database..."
health=''
for _ in $(seq 1 90); do
  health=$(curl -fs "http://127.0.0.1:${port[CONTROL_PORT]}/health" 2>/dev/null || true)
  [[ $health == *'"database":"ok"'* ]] && break
  health=''
  sleep 1
done
[ -n "$health" ] || { compose logs --tail 20 control >&2 || true; die "the control plane did not come up within 90 s (logs above)"; }

if [ -n "$public_domain" ]; then
  # Through the proxy on this machine (the name may not reach it from inside, and a local CA is
  # not trusted here), so a failure points at the proxy's routes rather than at DNS.
  if curl -fsk --resolve "$public_domain:${port[PROXY_HTTPS_PORT]}:127.0.0.1" "$origin/health" 2>/dev/null | grep -q '"database":"ok"'; then
    proxied="the proxy answers /health"
  else
    proxied="WARNING: the proxy did not answer /health on this machine; see '$engine compose -p $project logs proxy'"
  fi
  turn_udp=off; [ "${port[TURN_UDP_PORT]}" = 0 ] || turn_udp="UDP ${port[TURN_UDP_PORT]}"
  ice_tcp=off; [ "${port[LIVEKIT_TCP_PORT]}" = 0 ] || ice_tcp="TCP ${port[LIVEKIT_TCP_PORT]}"
  cat <<EOF

WeaveSignal is running, public (project '$project', $engine): $proxied.

  Station         $origin/            the player, call-in, chat and the studio's API
  Listeners       $origin/programme.opus   (plain HTTP on port ${port[PROXY_HTTP_PORT]} serves the mount too)
  Live room       ${origin/https:/wss:}  (media on UDP ${port[LIVEKIT_MEDIA_PORT]}; TURN over TLS on TCP ${port[TURN_TLS_PORT]})
  Control plane   http://127.0.0.1:${port[CONTROL_PORT]}/   (loopback, health: /health)

Sign in: the studio uses $origin with the admin token, which is CONTROL_ADMIN_TOKEN in
$env_file (mode 600). It is not shown here.

Open to the internet: TCP ${port[PROXY_HTTP_PORT]} and ${port[PROXY_HTTPS_PORT]} (the proxy), UDP ${port[LIVEKIT_MEDIA_PORT]} (media) and TCP ${port[TURN_TLS_PORT]} (TURN over TLS);
TURN over UDP $turn_udp, ICE over TCP $ice_tcp. Nothing else is published. Call-in stays closed until
CONSENT_CONTACT is set in $env_file (ADR-0124).
The station is unlisted by default (SEARCH_LISTING=false, DIRECTORY_LISTING=false); turn them on in
$env_file when it should be found.
TURN over TLS reads the certificate when LiveKit starts. Caddy renews it about thirty days before it
expires; run '$0 --renew-turn' between shows within those thirty days (it drops the room).
EOF
else
  cat <<EOF

WeaveSignal is running (project '$project', $engine).

  Control plane   http://127.0.0.1:${port[CONTROL_PORT]}/   (health: /health)
  Chat            http://127.0.0.1:${port[CHAT_PORT]}/
  Listeners       http://127.0.0.1:${port[LISTENERS_PORT]}/programme.opus
  Live room       ws://127.0.0.1:${port[LIVEKIT_HTTP_PORT]}  (media on UDP ${port[LIVEKIT_MEDIA_PORT]})

Sign in: the studio uses the control plane's address above with the admin token, which is
CONTROL_ADMIN_TOKEN in $env_file (mode 600). It is not shown here.
Every listener is on loopback: nobody off this machine can join or listen. To publish the
station, run this again with --public and the name that reaches this machine.
EOF
fi
if [ "$engine" = podman ]; then
  echo
  if [ "$boot_start" = true ]; then
    boot_start_podman
  else
    say "Start at boot left alone (--no-boot-start). The containers return after a reboot only if"
    say "podman-restart.service is enabled for your user ('systemctl --user enable podman-restart.service'"
    say "and, for a server without a login session, 'loginctl enable-linger'), or if you run the"
    say "station as Quadlets instead."
  fi
fi
