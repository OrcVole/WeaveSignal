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
# --image IMAGE (the server image; default forgejo.weavesignal.top/weavesignal/weavesignal-server:1.3.0, pulled when absent; a localhost/ image must be built first), --engine podman|docker.
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
# network or a test; browsers do not trust it), ACME_EMAIL, NODE_IP (LiveKit's public IPv4 address;
# else the A record of the name, read on each run: no STUN server is asked, ADR-0290; the Quadlets
# generated from this settings file keep the address it chose until this runs again),
# TURN_ALLOW_CIDRS (addresses TURN may relay to, for a private NODE_IP),
# PROXY_HTTP_PORT, PROXY_HTTPS_PORT, TURN_TLS_PORT, TURN_UDP_PORT and LIVEKIT_TCP_PORT (0 is off).
# STUN_SERVERS: STUN servers the station may ask (host:port each, comma-separated, at most four;
# LiveKit, the desk and the join pages all read it); empty, none is contacted (ADR-0290).
# The telephone line (17f): PHONE=true adds phone.yaml and checks PHONE_SIP_PORT (5060, UDP and
# TCP), the RTP range from PHONE_RTP_PORT (7340, two UDP ports per PHONE_LINES) and PHONE_REDIS_PORT
# (7321, loopback), and makes PHONE_REDIS_PASSWORD; the trunk's settings are in .env.example.
# The rehearsal studio (1.3): REHEARSAL=true adds rehearsal.yaml, a second desk with no way to the
# air, and turns the rehearsal studio on in the control plane; false (the default) leaves both off.
# It needs no port of its own.
# Hosts in their own studios (ADR-0280): SOURCE_ICECAST_PORT (TCP, 8005 suggested),
# SOURCE_SRT_PORT (UDP, 9710) and SOURCE_WHIP_PORT (UDP, 9712; WHIP from OBS, 17d, its offer through
# the public route), 0 or unset is off. Set, each is checked free and the ingest listens
# on it while a show key exists: on every address with --public, else on loopback.
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
usage() { sed -n '5,50p' "$0" | sed 's/^# \{0,1\}//'; }
a_record() { # name: its first IPv4 address, not a loopback or link-local one (a hosts-file entry)
  local ip _
  while read -r ip _; do
    [[ $ip =~ ^[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+$ && ! $ip =~ ^(0|127)\. && ! $ip =~ ^169\.254\. ]] && { echo "$ip"; return 0; }
  done < <(timeout 10 getent ahostsv4 "$1" 2>/dev/null)
  return 1
}

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
# Compose gives every container an oom_score_adj (compose.yaml: WEAVESIGNAL_OOM_SCORE_ADJ, 0 when
# unset). A user session that runs above 0 cannot exec into such a container on Podman ("crun:
# write to /proc/self/oom_score_adj: Permission denied"), so PostgreSQL's healthcheck never
# passes. On Podman, when the session's own value is above 0 and the user has not set one, the
# containers get that value. It is never lowered, and Docker keeps 0.
if [ "$engine" = podman ] && [ -z "${WEAVESIGNAL_OOM_SCORE_ADJ:-}" ]; then
  session_oom=$(cat /proc/self/oom_score_adj 2>/dev/null || echo 0)
  if [[ $session_oom =~ ^[0-9]+$ ]] && [ "$session_oom" -gt 0 ]; then export WEAVESIGNAL_OOM_SCORE_ADJ=$session_oom; fi
  # The rehearsal desk (1.3) runs at 500, or the session's own when that is higher.
  if [[ $session_oom =~ ^[0-9]+$ ]] && [ "$session_oom" -gt 500 ]; then export WEAVESIGNAL_REHEARSAL_OOM_SCORE_ADJ=$session_oom; fi
fi
# Said on install, as Podman with podman-compose beside Docker now means Docker.
[ "$action" != install ] || say "Container engine: $engine"
# The public route is the overlay public.yaml, used whenever the settings file names a domain, so
# --status and --uninstall see the proxy too. Install sets $public_domain from --public first.
public_domain=''
[ -f "$env_file" ] && public_domain=$(sed -n 's/^PUBLIC_DOMAIN=//p' "$env_file" | head -1)
# The telephone line (17f) is the overlay phone.yaml, used whenever the settings file says
# PHONE=true; install decides it below.
phone_on=false
[ -f "$env_file" ] && grep -qx 'PHONE=true' "$env_file" && phone_on=true
# The rehearsal studio (1.3) is the overlay rehearsal.yaml, used whenever the settings file says
# REHEARSAL=true; install decides it below.
rehearsal_on=false
[ -f "$env_file" ] && grep -qx 'REHEARSAL=true' "$env_file" && rehearsal_on=true
# A GPU for the desk (17e) is the overlay gpu.yaml, used when the settings file says GPU=nvidia
# and VIDEO=true (a desk without video has nothing to encode); install decides it below.
gpu_on=false
[ -f "$env_file" ] && grep -qx 'GPU=nvidia' "$env_file" && grep -qx 'VIDEO=true' "$env_file" && gpu_on=true
compose() {
  local files=(-f compose.yaml); [ -z "$public_domain" ] || files+=(-f public.yaml)
  [ "$phone_on" = false ] || files+=(-f phone.yaml)
  [ "$rehearsal_on" = false ] || files+=(-f rehearsal.yaml)
  [ "$gpu_on" = false ] || files+=(-f gpu.yaml)
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
station_name=$(keep STATION_NAME "This station")
room=$(keep ROOM studio)
# The demo station (the demo name) is given the demo slate picture once (ADR-0200); a station
# with its own name starts with none. A value already in the settings file is kept, empty or not.
if [ -n "${have[DEMO_STILL]+set}" ]; then demo_still=${have[DEMO_STILL]}
elif [ "$station_name" = "Kettle FM" ]; then demo_still=/opt/weavesignal/stills-demo/kettle-fm.png
else demo_still=''; fi
image=${image:-$(keep SERVER_IMAGE forgejo.weavesignal.top/weavesignal/weavesignal-server:1.3.0)}
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
# Hosts in their own studios (ADR-0280): the ingest's bearer, which only the ingest and the
# control plane are given, and the password the ingest sends the desk, which only those two hold.
# Either may be changed at any time (both ends read it at their next start).
ingest_token=${have[INGEST_TOKEN]:-$(secret)}
[ ${#ingest_token} -ge 32 ] || die "INGEST_TOKEN in $env_file is shorter than 32 characters"
desk_sources_pw=${have[DESK_SOURCES_PASSWORD]:-$(secret)}
mail_mode=$(keep MAIL_MODE off)
mail_from=$(keep MAIL_FROM chat@station.example)
avatar_uploads=$(keep AVATAR_UPLOADS true)
chat_attachments=$(keep CHAT_ATTACHMENTS false)
consent_contact=$(keep CONSENT_CONTACT "")
# The station's own consent words (issue #26); empty, the default words. The file holds them in
# single quotes with each apostrophe as \', the one form Compose reads back whole (unquoted, it
# would cut the words at " #" and read "$" as a variable). A value in the file without quotes,
# or in double quotes, is taken as written; one from the environment is the words themselves.
if [ -n "${have[CONSENT_TEXT]:-}" ]; then
  consent_text=${have[CONSENT_TEXT]}
  if [[ ${#consent_text} -ge 2 && $consent_text == \'*\' ]]; then
    consent_text=${consent_text:1:-1}; consent_text=${consent_text//\\\'/\'}
  elif [[ ${#consent_text} -ge 2 && $consent_text == \"*\" ]]; then
    consent_text=${consent_text:1:-1}; consent_text=${consent_text//\\\"/\"}
  fi
else
  consent_text=${CONSENT_TEXT:-}
fi
# A Quadlets unit would read a backslash or a per cent sign as its own (and drop or change the
# words), and "$" is not proved through it: refused on both routes, which share this file.
[[ $consent_text != *[\\\$%]* ]] \
  || die "CONSENT_TEXT holds a backslash, a dollar sign or a per cent sign, which the station's units cannot carry whole; leave them out (write per cent, or the currency's name)"
[[ $consent_text != *[[:cntrl:]]* ]] \
  || die "CONSENT_TEXT holds a line break, a tab or another control character; keep the words on one line"
search_listing=$(keep SEARCH_LISTING false)
directory_listing=$(keep DIRECTORY_LISTING false)
# The show and steering's settings (issue 5) go into the control plane's settings file inside quotes
# or as numbers, and Compose cannot escape them: refuse here rather than stop the control plane.
for key in STUDIO_SHOW STEERING_CLONE_FROM; do
  v=$(keep "$key" ""); v=${v#[\"\']}; v=${v%[\"\']}
  [[ $v != *[\"\\]* ]] || die "$key in $env_file holds a quote or a backslash, which the settings file cannot take; leave them out"
done
for key in STUDIO_LISTENER_DELAY_MS STUDIO_PLAYER_ALLOWANCE_MS STEERING_TTL_S STEERING_CHECK_EVERY_S STEERING_CHECK_TIMEOUT_MS STEERING_DOWN_AFTER; do
  v=$(keep "$key" ""); v=${v#[\"\']}; v=${v%[\"\']}
  [[ -z $v || $v =~ ^[0-9]+$ ]] || die "$key=$v in $env_file is not a whole number"
done

# --- STUN (ADR-0290): none unless the station names a server --------------------------------
# One setting for LiveKit, the desk and the join pages: host:port each, a DNS name or an IPv4
# address, at most four (the same rule as weavesignal-proto's parse_stun_servers).
stun_servers=''
raw=$(keep STUN_SERVERS ""); raw=${raw#[\"\']}; raw=${raw%[\"\']}
IFS=', ' read -ra stun_items <<< "$raw"
for s in "${stun_items[@]}"; do
  s=${s,,}
  [[ $s =~ ^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?(\.[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?)*:[0-9]{1,5}$ ]] && (( 10#${s##*:} >= 1 && 10#${s##*:} <= 65535 )) \
    || die "STUN_SERVERS in $env_file: '$s' is not host:port (a DNS name or an IPv4 address, a colon and a port, without stun:)"
  [[ ,$stun_servers, == *",$s,"* ]] || stun_servers+=${stun_servers:+,}$s
done
stun_commas=${stun_servers//[^,]/}
[ "${#stun_commas}" -lt 4 ] || die "STUN_SERVERS in $env_file names more than four servers"

# --- The desk's congestion control: gcc (the default), homegrown or disabled -----------------
congestion_control=$(keep CONGESTION_CONTROL gcc); congestion_control=${congestion_control#[\"\']}; congestion_control=${congestion_control%[\"\']}
congestion_control=${congestion_control,,}
case $congestion_control in
  gcc|homegrown|disabled) ;;
  *) die "CONGESTION_CONTROL in $env_file is '$congestion_control'; it must be gcc, homegrown or disabled" ;;
esac

# --- Video's encoder, and a GPU for the desk (17e, issue #8, ADR-0289) ------------------------
# VIDEO_ENCODER: auto (NVENC when the desk has a card, else SVT-AV1 on the CPU), nvenc or svt.
# GPU: nvidia hands the card to the desk by its CDI name (gpu.yaml, GPU_DEVICE), none keeps it on
# the CPU, and auto looks for an NVIDIA CDI spec now and writes what it found, so the next run
# changes nothing; set GPU=auto again to look again. The spec is what `nvidia-ctk cdi generate`
# writes; WEAVESIGNAL_CDI_DIRS names where to look (the tests point it at their own).
#
# With no GPU line at all, a new install looks (auto). A station that was installed before
# (its settings file already holds the database's password, which this script made on an
# earlier run) is never switched to the card by an update (ADR-0292): it keeps the CPU, GPU=none
# is written, and the card found is only said, here and in the studio app's Settings (GPU_FOUND,
# which the control plane reports). Setting GPU=nvidia or GPU=auto is the person's own act.
unquote() { local v=$1; v=${v#[\"\']}; printf '%s' "${v%[\"\']}"; }
video=$(unquote "$(keep VIDEO false)")
video_encoder=$(unquote "$(keep VIDEO_ENCODER auto)"); video_encoder=${video_encoder,,}
case $video_encoder in
  auto|nvenc|svt) ;;
  *) die "VIDEO_ENCODER in $env_file is '$video_encoder'; it must be auto, nvenc or svt" ;;
esac
cdi_dirs=${WEAVESIGNAL_CDI_DIRS:-/etc/cdi /var/run/cdi}
cdi_nvidia() { # the first CDI spec that declares NVIDIA GPUs, or nothing
  local d f
  for d in $cdi_dirs; do
    for f in "$d"/*.yaml "$d"/*.yml "$d"/*.json; do
      [ -f "$f" ] && grep -Eq '^[[:space:]]*"?kind"?[[:space:]]*:[[:space:]]*"?nvidia\.com/gpu"?' "$f" && { printf '%s' "$f"; return 0; }
    done
  done
  return 1
}
gpu=$(unquote "$(keep GPU "")"); gpu=${gpu,,}
# Whether this server has a card the desk could use, whatever GPU says: Settings offers it.
gpu_found=none; cdi_found=$(cdi_nvidia) && gpu_found=nvidia
if [ -z "$gpu" ]; then
  if [ -z "${have[POSTGRES_PASSWORD]:-}" ]; then
    gpu=auto
  else
    gpu=none
    [ "$gpu_found" != nvidia ] \
      || say "GPU: this server has an NVIDIA card (its CDI spec is at $cdi_found). This station was installed before WeaveSignal could use one, and an update never switches it: video stays on the CPU (GPU=none). To encode video on the card, set GPU=nvidia in $env_file and run this script again."
  fi
fi
case $gpu in
  auto)
    if [ "$gpu_found" = nvidia ]; then
      gpu=nvidia; say "GPU: an NVIDIA card's CDI spec is at $cdi_found, so GPU=nvidia (the desk encodes video on it with VIDEO=true; GPU=none turns it off)"
    else
      gpu=none
    fi ;;
  nvidia)
    cdi_nvidia >/dev/null || die "GPU=nvidia, but no CDI spec in $cdi_dirs declares nvidia.com/gpu. Install the NVIDIA Container Toolkit and run 'sudo nvidia-ctk cdi generate --output=/etc/cdi/nvidia.yaml', or set GPU=none" ;;
  none) ;;
  *) die "GPU in $env_file is '$gpu'; it must be auto, nvidia or none" ;;
esac
gpu_device=$(unquote "$(keep GPU_DEVICE nvidia.com/gpu=all)")
[[ $gpu_device =~ ^nvidia\.com/gpu=[A-Za-z0-9][A-Za-z0-9_.:-]*$ ]] \
  || die "GPU_DEVICE in $env_file is '$gpu_device'; it is a CDI name such as nvidia.com/gpu=all or nvidia.com/gpu=0"
nvenc_sessions=$(unquote "$(keep NVENC_SESSIONS 12)")
[[ $nvenc_sessions =~ ^[0-9]+$ ]] && [ "$nvenc_sessions" -le 64 ] \
  || die "NVENC_SESSIONS in $env_file is '$nvenc_sessions'; it is 0 to 64"
[ "$video_encoder" != nvenc ] || [ "$gpu" = nvidia ] \
  || say "VIDEO_ENCODER=nvenc with GPU=none: the desk has no card, so it encodes on the CPU with SVT-AV1 and says so in its log"

# --- VBAN (the presenter kit, 1.2): off unless KIT_VBAN=true -------------------------------
# It lands unquoted in the control plane's TOML, so it is true or false and nothing else.
kit_vban=$(unquote "$(keep KIT_VBAN false)"); kit_vban=${kit_vban,,}
case $kit_vban in true|false) ;; *) die "KIT_VBAN in $env_file is '$kit_vban'; it must be true or false" ;; esac

# --- The public route (opt-in): a name for the station, and what follows from it -------------
case $public_arg in
  '') ;;                                  # keep what the settings file says
  off) public_domain='' ;;
  *) public_domain=$public_arg ;;
esac
public_tls=''; acme_email=''; node_ip=''; turn_cidrs=''
# A loopback station's printed address (cards and feeds, `publishing.public_url`): kept when set
# by hand, as a station on a private network may be reached at http://192.168.1.20:7310. One the
# public route derived (the file named a domain when this run began) goes with the route.
hand_origin=''
if [ -z "$public_domain" ] && [ -z "${have[PUBLIC_DOMAIN]:-}" ]; then
  hand_origin=$(unquote "$(keep PUBLIC_ORIGIN "")"); hand_origin=${hand_origin%/}
  origin_form='^https?://[A-Za-z0-9.:-]+(/[A-Za-z0-9._~/-]*)?$'
  [[ -z $hand_origin || $hand_origin =~ $origin_form ]] \
    || die "PUBLIC_ORIGIN in $env_file is '$hand_origin'; it is an address such as http://192.168.1.20:7310 (a scheme, a host and a port, no quotes or spaces)"
fi
if [ -n "$public_domain" ]; then
  [[ $public_domain =~ ^[a-z0-9]([a-z0-9-]*[a-z0-9])?(\.[a-z0-9]([a-z0-9-]*[a-z0-9])?)+$ ]] \
    || die "--public needs a lower-case domain name such as station.example (no scheme, port or path), not '$public_domain'"
  public_tls=$(keep PUBLIC_TLS acme)
  case $public_tls in acme|internal) ;; *) die "PUBLIC_TLS is acme (a certificate from Let's Encrypt) or internal (Caddy's own CA)" ;; esac
  acme_email=$(keep ACME_EMAIL "")
  [[ -z $acme_email || $acme_email =~ ^[^[:space:]@]+@[^[:space:]@]+$ ]] || die "ACME_EMAIL is not an email address"
  node_ip=$(keep NODE_IP "")
  [[ -z $node_ip || $node_ip =~ ^[0-9a-fA-F:.,]+$ ]] || die "NODE_IP is an IP address (LiveKit's public one), or empty to use the A record of the name"
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
  "DESK_SOURCES_PORT tcp 7318 17318 27318"
  "INGEST_WHIP_PORT tcp 7319 17319 27319"
  "SOURCE_BRIDGE_PORT tcp 7320 17320 27320"
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
public_keys=" LIVEKIT_MEDIA_PORT PROXY_HTTPS_PORT PROXY_HTTP_PORT TURN_TLS_PORT TURN_UDP_PORT LIVEKIT_TCP_PORT SOURCE_ICECAST_PORT SOURCE_SRT_PORT SOURCE_WHIP_PORT "
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
  # The bridge (Phase 17g) alone may be 0, which turns it off.
  if [ "$key" = SOURCE_BRIDGE_PORT ] && [ "${have[$key]:-${!key:-}}" = 0 ]; then
    port[$key]=0; continue
  fi
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
# On a loopback station they are not used (LiveKit is reached on this machine only), so they are
# left out of the settings: said, not dropped in silence.
if [ -z "$public_domain" ]; then
  for key in TURN_UDP_PORT LIVEKIT_TCP_PORT; do
    v=$(keep "$key" 0)
    [ "$v" = 0 ] || say "$key=$v is for a public station (--public): a loopback one does not use it, so it is left out of the settings"
  done
fi
# Hosts in their own studios (ADR-0280): off (0) unless the station sets a port. A set
# port is checked like any public one, as the ingest takes every address on a public station.
for spec in "SOURCE_ICECAST_PORT tcp 8005 18005 28005" "SOURCE_SRT_PORT udp 9710 19710 29710" "SOURCE_WHIP_PORT udp 9712 19712 29712"; do
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
# The telephone line (17f): off unless PHONE=true. On, its SIP port (UDP and TCP, open to the
# provider), its RTP range (two ports per line, starting on an even port, UDP, open to the
# provider) and Valkey's loopback port are each free, or the next suggestion, as above; a port
# the settings file already holds is kept. Valkey's password is made here like the others.
phone=$(keep PHONE false)
case $phone in true|false) ;; *) die "PHONE is true or false, not '$phone'" ;; esac
phone_lines=$(keep PHONE_LINES 8)
# Written back with the line on, so a trunk given in the environment on the first run is kept.
phone_settings="PHONE_NUMBERS PHONE_ALLOWED_ADDRESSES PHONE_AUTH_USERNAME PHONE_PUBLIC_IP PHONE_COUNTRY_CODE PHONE_CONSENT_WORDS PHONE_DEFAULT_VOICE PHONE_SIP_LISTEN"
[[ $phone_lines =~ ^[0-9]+$ ]] && [ "$phone_lines" -ge 1 ] && [ "$phone_lines" -le 16 ] \
  || die "PHONE_LINES is 1 to 16 (calls at once), not '$phone_lines'"
phone_free() { # first protocols scope: the range of `count` ports from `first` is free on each
  local first=$1 protos=$2 scope=$3 count=${4:-1} i pr
  for ((i = 0; i < count; i++)); do
    for pr in $protos; do free "$pr" $((first + i)) "$scope" || return 1; done
  done
}
phone_pick() { # key protocols scope count default alternatives...
  local key=$1 protos=$2 scope=$3 count=$4 p i pr; shift 4
  p=$(keep "$key" "")
  if [ -n "$p" ]; then
    [[ $p =~ ^[0-9]+$ ]] && [ "$p" -ge 1024 ] && [ $((p + count - 1)) -le 65535 ] \
      || die "$key=$p is not a port from 1024 up with room for $count"
    if [ "$ours" = false ] || [ -z "${have[$key]:-}" ]; then
      phone_free "$p" "$protos" "$scope" "$count" \
        || die "$key=$p ($protos) is in use; free it, or choose another port ($* are the usual)"
    fi
  else
    for a in "$@"; do phone_free "$a" "$protos" "$scope" "$count" && { p=$a; break; }; done
    [ -n "$p" ] || die "$key: $* are all in use; set $key to any free port"
    [ "$p" = "$1" ] || say "$key: $1 is in use, so $p is used instead"
  fi
  for ((i = 0; i < count; i++)); do for pr in $protos; do claim "$pr" $((p + i)); done; done
  port[$key]=$p
}
if [ "$phone" = true ]; then
  # The control plane refuses the line on without a trunk (phone.enabled needs a number, the
  # provider's addresses or a digest user, and the station's public address), so the station
  # would not start: say so here instead.
  [ -n "$(keep PHONE_NUMBERS "")" ] && [ -n "$(keep PHONE_PUBLIC_IP "")" ] \
    && { [ -n "$(keep PHONE_ALLOWED_ADDRESSES "")" ] || [ -n "$(keep PHONE_AUTH_USERNAME "")" ]; } \
    || die "PHONE=true needs a trunk first: PHONE_NUMBERS, PHONE_PUBLIC_IP, and PHONE_ALLOWED_ADDRESSES or PHONE_AUTH_USERNAME (see .env.example and Take phone calls)"
  case $(keep PHONE_DEFAULT_VOICE alba) in alba|edinburgh|own) ;;
    *) die "PHONE_DEFAULT_VOICE is alba, edinburgh or own, not '$(keep PHONE_DEFAULT_VOICE alba)'" ;; esac
  # Where LiveKit's SIP service listens: every address unless PHONE_SIP_LISTEN names one of this
  # machine's (the one the provider reaches). It must face the provider, so it is never loopback
  # by default; a station on loopback only cannot take a provider's calls at all.
  phone_sip_listen=$(unquote "$(keep PHONE_SIP_LISTEN "")")
  [[ -z $phone_sip_listen || $phone_sip_listen =~ ^[0-9]{1,3}(\.[0-9]{1,3}){3}$ ]] \
    || die "PHONE_SIP_LISTEN is one of this machine's IPv4 addresses (the one the provider reaches), or empty for every address, not '$phone_sip_listen'"
  phone_pick PHONE_SIP_PORT "udp tcp" any 1 5060 15060 25060
  phone_pick PHONE_RTP_PORT udp any $((2 * phone_lines)) 7340 17340 27340
  [ $((port[PHONE_RTP_PORT] % 2)) = 0 ] || die "PHONE_RTP_PORT=${port[PHONE_RTP_PORT]} is odd; LiveKit's SIP service takes even ports, so the range starts on one"
  port[PHONE_RTP_LAST]=$((port[PHONE_RTP_PORT] + 2 * phone_lines - 1))
  phone_pick PHONE_REDIS_PORT tcp '' 1 7321 17321 27321
  phone_redis_pw=${have[PHONE_REDIS_PASSWORD]:-$(secret)}
fi
phone_on=$phone
# The rehearsal studio (1.3): off unless REHEARSAL=true. It opens no port, so nothing to check.
rehearsal=$(unquote "$(keep REHEARSAL false)")
case $rehearsal in true|false) ;; *) die "REHEARSAL is true or false, not '$rehearsal'" ;; esac
rehearsal_on=$rehearsal
gpu_on=false; [ "$gpu" != nvidia ] || [ "$video" != true ] || gpu_on=true
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
  # LiveKit: the address in its candidates is the one given (NODE_IP), else the A record of the
  # name, read now (ADR-0290). It always has an address of its own and use_external_ip off:
  # `use_external_ip` asks Twilio's and Google's STUN servers, even beside a node_ip, and a
  # LiveKit with no node_ip gives its own connections Google's. IPv4 only: TURN relays from it.
  lk_node=$node_ip; lk_from=NODE_IP
  if [ -z "$lk_node" ]; then
    lk_node=$(a_record "$public_domain") || die "LiveKit's public address is unknown: $public_domain has no IPv4 (A) record this machine can read. Give the name an A record that points at this machine's public address, or set NODE_IP=<that address> in $env_file. WeaveSignal no longer asks a STUN server for it (ADR-0290)."
    lk_from="the A record of $public_domain"
  fi
  use_external=false
  say "LiveKit's public address: $lk_node ($lk_from). STUN servers: ${stun_servers:-none}."
  if [ "$lk_from" != NODE_IP ]; then
    if [[ $lk_node =~ ^(10\.|192\.168\.|172\.(1[6-9]|2[0-9]|3[01])\.|100\.(6[4-9]|[7-9][0-9]|1[01][0-9]|12[0-7])\.) ]]; then
      say "  That is a private address: guests off this network reach it only if the router forwards it, and LiveKit's TURN"
      say "  refuses to relay to it unless TURN_ALLOW_CIDRS lists it ($lk_node/32). If the public address differs (NAT, or a name"
      say "  behind a proxy such as Cloudflare's), set NODE_IP to it."
    else
      say "  If this machine is behind NAT, or the name is behind a proxy (Cloudflare's orange cloud), that is not the address"
      say "  guests reach: set NODE_IP to the public one."
    fi
  fi
  derived="PUBLIC_ORIGIN=$origin
ROOM_PUBLIC_URL=$room_origin
STREAM_URL=$origin/programme.opus
FORWARDED_FOR=true
TURN_ENABLED=true
TURN_CERT_DIR=$cert_dir
PROXY_LOCAL_CERTS=$local_certs
PROXY_ACME_EMAIL=$acme_option
LIVEKIT_USE_EXTERNAL_IP=$use_external
LIVEKIT_NODE_IP=$lk_node
INGEST_BIND=0.0.0.0"
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
  say "# Hosts in their own studios (ADR-0280): the ingest's bearer for the control plane, and the"
  say "# password it sends the desk with each source. Either may be changed; restart the station."
  say "INGEST_TOKEN=$ingest_token"
  say "DESK_SOURCES_PASSWORD=$desk_sources_pw"
  say ""
  say "MAIL_MODE=$mail_mode"
  say "MAIL_FROM=$mail_from"
  say ""
  say "# Chat avatars (ADR-0235): true lets a signed-in chatter upload a picture, held until staff"
  say "# approve it; false leaves the shipped avatars only. Pictures are in the chat-avatars volume."
  say "AVATAR_UPLOADS=$avatar_uploads"
  say ""
  say "# Pictures and documents in the chat (ADR-0262): true lets a trusted chatter attach one, held"
  say "# until staff approve it (staff see every one); false, the default, words only. They are in"
  say "# the chat-attachments volume; the studio's switch decides once used."
  say "CHAT_ATTACHMENTS=$chat_attachments"
  say ""
  say "# How a caller asks for their call to be taken down (an email address or a link); call-in"
  say "# stays closed until it is set (ADR-0124)."
  say "CONSENT_CONTACT=$consent_contact"
  say "# The consent words a caller agrees to (issue #26); empty, the default words. In single quotes,"
  say "# each apostrophe written \\'; {station}, {kept}, {contact} and {presenter} are filled in. No"
  say "# backslash, dollar sign or per cent sign. A caller who agreed to other words is asked again."
  if [ -n "$consent_text" ]; then say "CONSENT_TEXT='${consent_text//\'/\\\'}'"; else say "CONSENT_TEXT="; fi
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
    say "# LiveKit's public IPv4 address; empty, the A record of the name, read on each run (no STUN"
    say "# server is asked, ADR-0290). TURN_ALLOW_CIDRS: addresses TURN may relay to besides public"
    say "# ones, e.g. 10.0.0.5/32 for a NODE_IP that is a private address."
    say "NODE_IP=$node_ip"
    say "TURN_ALLOW_CIDRS=$turn_cidrs"
    say ""
  fi
  say "# STUN servers the station may ask (ADR-0290): host:port each, comma-separated, at most four;"
  say "# LiveKit, the desk and the join pages all read it. Empty: none is contacted."
  say "STUN_SERVERS=$stun_servers"
  say ""
  say "# How the desk's senders control congestion: gcc (the default; GStreamer's free implementation of"
  say "# Google's published algorithm), homegrown (its own simple heuristic) or disabled."
  say "CONGESTION_CONTROL=$congestion_control"
  say ""
  say "# Video's encoder (with VIDEO=true): auto (NVENC when the desk has a card, else SVT-AV1 on the"
  say "# CPU), nvenc or svt. GPU: nvidia hands the card named by GPU_DEVICE to the desk (gpu.yaml, by"
  say "# its CDI name), none keeps video on the CPU, auto looks for an NVIDIA CDI spec on the next run."
  say "VIDEO_ENCODER=$video_encoder"
  say "GPU=$gpu"
  if [ "$gpu" = nvidia ]; then
    say "GPU_DEVICE=$gpu_device"
    say "# NVENC sessions the desk may open (0 to 64); a consumer card allows a few for the whole machine."
    say "NVENC_SESSIONS=$nvenc_sessions"
  fi
  say "# Whether this script found an NVIDIA card's CDI spec on this server (nvidia or none), looked"
  say "# for on every run. Not a setting: the studio app's Settings reads it to offer the card while"
  say "# GPU=none. An update never sets GPU=nvidia by itself; that line is yours to change."
  say "GPU_FOUND=$gpu_found"
  say ""
  say "# Ports (ADR-0075): any free port may be used instead."
  for spec in "${port_specs[@]}"; do read -r key _ <<< "$spec"; say "$key=${port[$key]}"; done
  say "# Hosts in their own studios (ADR-0280): Icecast (TCP), SRT (UDP) and WHIP (UDP, from OBS,"
  say "# its offer through the public route) from hosts' own software; 0 is off. Set, the ingest"
  say "# listens on them while a show key exists."
  say "SOURCE_ICECAST_PORT=${port[SOURCE_ICECAST_PORT]}"
  say "SOURCE_SRT_PORT=${port[SOURCE_SRT_PORT]}"
  say "SOURCE_WHIP_PORT=${port[SOURCE_WHIP_PORT]}"
  say "# The rehearsal studio (1.3): true runs rehearsal.yaml, a second desk with no way to the air,"
  say "# where a show's people rehearse before they move to the live studio. No port of its own."
  say "REHEARSAL=$rehearsal"
  say "# VBAN from a studio PC on this network (the presenter kit, 1.2): true lets the station take it"
  say "# (Settings, Presenter kit). It has no password and no encryption: only on a local network."
  say "KIT_VBAN=$kit_vban"
  # Written back only when set, so a second run keeps it (the same rule as KIT_VBAN, which is always written).
  pull_private=$(keep PULL_ALLOW_PRIVATE "")
  if [ -n "$pull_private" ]; then
    say "# Pulled sources (1.2): true lets a pull open an address inside the station's own network."
    say "PULL_ALLOW_PRIVATE=$pull_private"
  fi
  say "# The telephone line (17f): true runs phone.yaml (Valkey and LiveKit's SIP service); set the trunk"
  say "# first (PHONE_NUMBERS and the rest: see .env.example and Take phone calls), or it is refused."
  say "PHONE=$phone"
  if [ "$phone" = true ]; then
    say "# Calls at once; SIP (UDP and TCP) and the RTP range (UDP, two ports per line) open to the"
    say "# provider only; Valkey on loopback, with its password."
    say "PHONE_LINES=$phone_lines"
    for key in PHONE_SIP_PORT PHONE_RTP_PORT PHONE_RTP_LAST PHONE_REDIS_PORT; do say "$key=${port[$key]}"; done
    say "PHONE_REDIS_PASSWORD=$phone_redis_pw"
    say "# The trunk and the words (from this file, else the environment on the first run)."
    for key in $phone_settings; do v=$(keep "$key" ""); [ -z "$v" ] || say "$key=$v"; done
  fi
  if [ -n "$hand_origin" ]; then
    say "# The address cards and feeds print on this loopback station (set by hand; --public sets its own)."
    say "PUBLIC_ORIGIN=$hand_origin"
  fi
  if [ -n "$public_domain" ]; then
    say "# TURN over UDP and ICE over TCP: 0 is off (TURN over TLS reaches the same guests)."
    say "TURN_UDP_PORT=${port[TURN_UDP_PORT]}"
    say "LIVEKIT_TCP_PORT=${port[LIVEKIT_TCP_PORT]}"
    say ""
    say "# Derived from the settings above on every run; change those, not these."
    printf '%s\n' "$derived"
  fi
  # Anything else the file held is kept as it was.
  known=" STATION_ID STATION_NAME ROOM SERVER_IMAGE DEMO_STILL LIVEKIT_API_KEY LIVEKIT_API_SECRET LISTENERS_SOURCE_PASSWORD POSTGRES_PASSWORD CONTROL_ADMIN_TOKEN CHAT_EMAIL_KEY CONTROL_EMAIL_KEY CONTROL_TOKEN_KEY INGEST_TOKEN DESK_SOURCES_PASSWORD MAIL_MODE MAIL_FROM STUN_SERVERS CONGESTION_CONTROL AVATAR_UPLOADS CHAT_ATTACHMENTS CONSENT_CONTACT CONSENT_TEXT SEARCH_LISTING DIRECTORY_LISTING CONTROL_PORT CHAT_PORT LISTENERS_PORT SRT_INGEST_PORT PLAYOUT_FEED_PORT DESK_FEED_PORT DESK_SOURCES_PORT LIVEKIT_HTTP_PORT LIVEKIT_MEDIA_PORT SOURCE_ICECAST_PORT SOURCE_SRT_PORT SOURCE_BRIDGE_PORT KIT_VBAN SOURCE_WHIP_PORT INGEST_WHIP_PORT PUBLIC_DOMAIN PUBLIC_TLS ACME_EMAIL NODE_IP TURN_ALLOW_CIDRS PROXY_HTTPS_PORT PROXY_HTTP_PORT TURN_TLS_PORT TURN_UDP_PORT LIVEKIT_TCP_PORT PUBLIC_ORIGIN ROOM_PUBLIC_URL STREAM_URL FORWARDED_FOR TURN_ENABLED TURN_CERT_DIR PROXY_LOCAL_CERTS PROXY_ACME_EMAIL LIVEKIT_USE_EXTERNAL_IP LIVEKIT_NODE_IP INGEST_BIND VIDEO_ENCODER GPU GPU_FOUND PULL_ALLOW_PRIVATE "
  known+="PHONE REHEARSAL "
  [ "$gpu" != nvidia ] || known+="GPU_DEVICE NVENC_SESSIONS "
  [ "$phone" = false ] || known+="PHONE_LINES PHONE_SIP_PORT PHONE_RTP_PORT PHONE_RTP_LAST PHONE_REDIS_PORT PHONE_REDIS_PASSWORD $phone_settings "
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

# The phone line faces the provider, so it is the exception to loopback: said in the summary.
phone_note=''
if [ "$phone" = true ]; then
  phone_note="The phone line: SIP on UDP and TCP ${port[PHONE_SIP_PORT]} listens on ${phone_sip_listen:-every address} (PHONE_SIP_LISTEN names one), and its media on UDP ${port[PHONE_RTP_PORT]}-${port[PHONE_RTP_LAST]} on every address; open both to the provider's addresses only."
fi

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
  hosts=off
  if [ "${port[SOURCE_ICECAST_PORT]}" != 0 ] || [ "${port[SOURCE_SRT_PORT]}" != 0 ] || [ "${port[SOURCE_WHIP_PORT]}" != 0 ]; then
    ways=()
    [ "${port[SOURCE_ICECAST_PORT]}" = 0 ] || ways+=("TCP ${port[SOURCE_ICECAST_PORT]} (Icecast)")
    [ "${port[SOURCE_SRT_PORT]}" = 0 ] || ways+=("UDP ${port[SOURCE_SRT_PORT]} (SRT)")
    [ "${port[SOURCE_WHIP_PORT]}" = 0 ] || ways+=("UDP ${port[SOURCE_WHIP_PORT]} (WHIP)")
    hosts=${ways[0]}
    for ((i = 1; i < ${#ways[@]}; i++)); do
      if [ "$i" = $((${#ways[@]} - 1)) ]; then hosts="$hosts and ${ways[$i]}"; else hosts="$hosts, ${ways[$i]}"; fi
    done
    hosts="$hosts, listening once a show key exists"
  fi
  cat <<EOF

WeaveSignal is running, public (project '$project', $engine): $proxied.

  Station         $origin/            the player, call-in, chat and the studio's API
  Listeners       $origin/programme.opus   (plain HTTP on port ${port[PROXY_HTTP_PORT]} serves the mount too)
  Live room       ${origin/https:/wss:}  (media on UDP ${port[LIVEKIT_MEDIA_PORT]}; TURN over TLS on TCP ${port[TURN_TLS_PORT]})
  Control plane   http://127.0.0.1:${port[CONTROL_PORT]}/   (loopback, health: /health)

Sign in: the studio uses $origin with the admin token, which is CONTROL_ADMIN_TOKEN in
$env_file (mode 600). It is not shown here.
Set the station up: open $origin/setup/ and sign in with the same key. The page takes the
station's name, the takedown contact that opens call-in, the jurisdiction and the station's look.

Open to the internet: TCP ${port[PROXY_HTTP_PORT]} and ${port[PROXY_HTTPS_PORT]} (the proxy), UDP ${port[LIVEKIT_MEDIA_PORT]} (media) and TCP ${port[TURN_TLS_PORT]} (TURN over TLS);
TURN over UDP $turn_udp, ICE over TCP $ice_tcp; hosts' own studios $hosts.${phone_note:+
$phone_note} Nothing else is published. Call-in stays closed until
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
Set the station up: open http://127.0.0.1:${port[CONTROL_PORT]}/setup/ and sign in with the same key.
The page takes the station's name, the takedown contact that opens call-in, the jurisdiction
and the station's look.
Every listener but LiveKit's media port is on loopback: nobody off this machine can join or
listen. To publish the station, run this again with --public and the name that reaches this machine.${phone_note:+
$phone_note}
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
