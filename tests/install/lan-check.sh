#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# The Compose bundle's listen-only route for a home network (lan.yaml, weavesignal.sh --lan; 1.3.1)
# and the installer's two checks before it pulls (Podman's compose provider and API socket), in
# three parts and with no real network address, no firewall and no second device:
#
#   1. weavesignal.sh with every system command stubbed (a made-up list of this machine's network
#      cards, a sudo that fails loudly): the address found or refused, what is written and kept,
#      the overlay passed to Compose, the port check, the firewall words printed and never run,
#      --lan off, and the provider and socket messages.
#   2. The settings it wrote rendered by the real `compose config`: one pinned Caddy on the
#      host network bound to that one address, the player's stream address, and every other
#      service still on loopback with cards and feeds left off.
#   3. The rendered Caddyfile run for real (on 127.0.0.2) in front of stand-in backends that say
#      what reached them: what is passed, what is refused (the studio, its sign-in, setup, call-in,
#      the chat, LiveKit, health, anything that writes), and the stream with ICY metadata dropped.
#
# Part 3 needs Podman and the Caddy image (pulled if absent). Ports are $((base))..$((base+19)),
# WEAVESIGNAL_LAN_TEST_PORT sets the base (default 29960); containers are named after
# WEAVESIGNAL_TEST_PREFIX. `just lan-check`.
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
export WEAVESIGNAL_CDI_DIRS="$root/tests/install/run/no-cdi"
script="$root/deploy/compose/weavesignal.sh"
run="$root/tests/install/run/lan"
prefix=${WEAVESIGNAL_TEST_PREFIX:-weavesignal-m1}-lan
base=${WEAVESIGNAL_LAN_TEST_PORT:-29960}
caddy_image=$(sed -n 's|^    image: \(docker.io/library/caddy:.*\)$|\1|p' "$root/deploy/compose/lan.yaml")
rm -rf -- "$run"; mkdir -p "$run/bin" "$run/net"

cleanup() {
  [ -z "${KEEP:-}" ] || { echo "KEEP is set: containers and $run are left; remove them by hand" >&2; return 0; }
  for c in "$prefix-lan" "$prefix-backends"; do podman rm -f "$c" >/dev/null 2>&1 || true; done
  rm -rf -- "$run"
}
trap cleanup EXIT
fail() { echo "FAIL  $*" >&2; exit 1; }
pass() { echo "pass  $*"; }
# Made-up addresses only: nothing here names a real machine.
lan_ip=192.168.1.20; vpn_ip=10.14.0.2; bridge_ip=192.168.122.1; cable_ip=192.168.50.7; ts_ip=100.64.0.9

# --- 1. weavesignal.sh, every system command a stub ---------------------------------------------
calls="$run/calls.log"; : > "$calls"
stub() { # name body
  printf '#!/usr/bin/env bash\nprintf "%%s %%s\\n" "$(basename "$0")" "$*" >> %q\n%s\n' "$calls" "$2" > "$run/bin/$1"
  chmod +x "$run/bin/$1"
}
# STUB_COMPOSE_FAIL=1 makes `compose version` fail for Podman (no provider); STUB_SOCKET is what
# `podman info` says about the API socket (true, false, or nothing: unreadable). STUB_DOCKER_FAIL=1
# takes Docker away.
stub podman 'case "$*" in
  "compose version"*) [ -z "${STUB_COMPOSE_FAIL:-}" ] || { echo "Error: looking up compose provider failed" >&2; exit 125; } ;;
  info*) echo "${STUB_SOCKET-true}" ;;
  *" ps -q"*) [ -z "${STUB_PS:-}" ] || echo abc ;;
esac; exit 0'
stub docker 'case "$*" in "compose version"*) [ -z "${STUB_DOCKER_FAIL:-}" ] || exit 1 ;; *" ps -q"*) [ -z "${STUB_PS:-}" ] || echo abc ;; esac; exit 0'
stub ss 'for t in ${STUB_TAKEN:-}; do
  proto=${t%%:*}; p=${t#*:}
  [[ " $* " == *" sport = :$p "* ]] || continue
  case "$1" in
    -Hlun) [ "$proto" = udp ] && echo "UNCONN 0 0 0.0.0.0:$p 0.0.0.0:*" ;;
    -Hltn) [ "$proto" = tcp ] && echo "LISTEN 0 0 0.0.0.0:$p 0.0.0.0:*" ;;
  esac
done; exit 0'
stub curl 'case "$*" in *api/v1/player*) echo "{\"stream_url\":\"x\"}" ;; *) echo "{\"database\":\"ok\"}" ;; esac'
stub getent 'exit 2'
stub loginctl 'case "$*" in *show-user*) echo yes ;; esac'
stub systemctl 'case "$*" in *is-enabled*) exit 0 ;; *"enable --now podman.socket"*) [ -z "${STUB_SYSTEMCTL_FAIL:-}" ] || exit 1 ;; esac'
stub sudo 'echo "sudo was called" >&2; exit 99'
stub firewall-cmd 'echo "firewall-cmd was called" >&2; exit 99'
stub ufw 'echo "ufw was called" >&2; exit 99'
# ip: one line per STUB_IP entry ("interface address"), in the form `ip -4 -o addr show scope global` prints.
stub ip 'n=2; for e in ${STUB_IP:-}; do dev=${e%%=*}; a=${e#*=}; echo "$n: $dev    inet $a/24 brd 0.0.0.0 scope global $dev"; n=$((n + 1)); done'
# The cards: wlan0, eth1 and a bonded eth have a device entry; a VPN, a bridge for virtual machines
# and a container network do not.
for d in wlan0 eth1; do mkdir -p "$run/net/$d"; : > "$run/net/$d/device"; done
for d in tun0 virbr0 docker0 br-0a1b2c3d veth9 tailscale0 bond0; do mkdir -p "$run/net/$d"; done
export WEAVESIGNAL_SYS_NET="$run/net"
low_file="$run/port-start"; echo 1024 > "$low_file"
n=0
all_cards="wlan0=$lan_ip tun0=$vpn_ip virbr0=$bridge_ip docker0=172.17.0.1 br-0a1b2c3d=172.18.0.1 veth9=10.88.0.2 tailscale0=$ts_ip"
install() { # arguments for weavesignal.sh; env in the caller; output in $out, the settings in $env_out
  n=$((n + 1)); : > "$calls"; out="$run/out-$n.txt"; env_out="$run/station.env"
  PATH="$run/bin:$PATH" WEAVESIGNAL_UNPRIV_PORT_FILE=$low_file "$script" --project lan-check --env-file "$env_out" \
    --image localhost/stub:none --no-boot-start "$@" > "$out" 2>&1
}
expect_refusal() { # words: the output must hold them
  if install "${args[@]}" 2>/dev/null; then fail "was accepted: ${args[*]}"; fi
  grep -qF -- "$1" "$out" || { cat "$out" >&2; fail "refusal does not say '$1'"; }
}
setting() { sed -n "s/^$1=//p" "$env_out" | head -1; }
called() { grep -qF -- "$1" "$calls"; }

# Auto: one real card among a VPN, a libvirt bridge, container networks, a veth and a Tailscale
# interface: it takes the card and says which.
export STUB_IP="$all_cards"
install --lan
[ "$(setting LAN_ADDRESS)" = "$lan_ip" ] && [ "$(setting LAN_PORT)" = 7300 ] \
  && [ "$(setting STREAM_URL)" = "http://$lan_ip:7300/programme.opus" ] || { cat "$env_out" >&2; fail "--lan did not write the card's address, port and stream address"; }
grep -qF "LAN address: $lan_ip (the network card wlan0)" "$out" || { cat "$out" >&2; fail "the output does not say which card gave the address"; }
[ -z "$(setting PUBLIC_ORIGIN)" ] || fail "--lan set PUBLIC_ORIGIN (it would turn on cards, feeds and invitation links the route does not serve)"
[ -z "$(setting PUBLIC_DOMAIN)" ] || fail "--lan set a public domain"
[ "$(setting FORWARDED_FOR)" = "" ] || fail "--lan turned forwarded_for on"
[ "$(stat -c %a "$env_out")" = 600 ] || fail "the settings file is not mode 600"
called "compose --env-file $env_out -p lan-check -f compose.yaml -f lan.yaml up -d" || { cat "$calls" >&2; fail "compose did not get -f lan.yaml"; }
! called "public.yaml" || fail "--lan passed the public overlay"
! called "sudo" && ! called "firewall-cmd" && ! called "ufw" || fail "the script ran sudo or a firewall command"
pass "--lan: the one real card's address (a VPN, a libvirt bridge, container networks and Tailscale left out), port 7300, the stream address derived, PUBLIC_ORIGIN left alone, the overlay passed, no sudo"

# What it says: where to listen, what it passes, the microphone, the firewall for both systems.
for want in "Player          http://$lan_ip:7300/" "Stream          http://$lan_ip:7300/programme.opus" \
  "for listening only" "a browser gives a microphone only over HTTPS or on localhost" "sudo firewall-cmd --add-port=7300/tcp" \
  "sudo firewall-cmd --runtime-to-permanent" "sudo ufw allow 7300/tcp" "never runs sudo" "fixed address"; do
  grep -qiF -- "$want" "$out" || { cat "$out" >&2; fail "the output does not say: $want"; }
done
grep -q "the studio and its sign-in, call-in, the chat and the live room stay" "$out" || fail "the output does not say what stays on this machine"
! grep -q "nobody off this machine can join or$" "$out" || fail "the output still says nobody off this machine can listen"
grep -q "except the listen-only address below" "$out" || fail "the output does not name the listen-only address as the exception"
! grep -qF "$(setting CONTROL_ADMIN_TOKEN)" "$out" || fail "the admin token was printed"
pass "the output gives the player and stream addresses, says it is listening only and why nobody can speak, and prints the firewalld and ufw commands for the port"

cp "$env_out" "$run/first.env"
install --lan; cmp -s "$env_out" "$run/first.env" || fail "a second --lan run changed the settings"
grep -q "Settings unchanged" "$out" || fail "a second run does not say the settings are unchanged"
install; cmp -s "$env_out" "$run/first.env" || fail "a run without --lan dropped the route"
called "-f lan.yaml" || fail "a run without --lan did not keep the overlay"
install --status; called "-f lan.yaml ps" || fail "--status does not see the route's web server"
pass "a second run changes nothing, the route stays on without the flag, and --status sees it"

# A given address: used; one that is not this machine's, public, or malformed is refused.
export STUB_IP="wlan0=$lan_ip eth1=$cable_ip"
args=(--lan); expect_refusal "found more than one address"
grep -qF "$lan_ip  (wlan0)" "$out" && grep -qF "$cable_ip  (eth1)" "$out" && grep -qF -- "--lan ADDRESS" "$out" || { cat "$out" >&2; fail "two cards: the refusal does not list them and say how to choose"; }
[ ! -e "$run/station.env" ] || [ "$(sed -n 's/^LAN_ADDRESS=//p' "$run/station.env")" = "$lan_ip" ] || fail "settings changed after a refusal"
rm -f "$run/station.env"
install --lan "$cable_ip"; [ "$(setting LAN_ADDRESS)" = "$cable_ip" ] && [ "$(setting STREAM_URL)" = "http://$cable_ip:7300/programme.opus" ] || fail "a given address was not used"
rm -f "$run/station.env"
args=(--lan 198.51.100.4 --no-start); export STUB_IP="wlan0=$lan_ip"; expect_refusal "not a private one"
grep -qF -- "--public" "$out" || fail "the public-address refusal does not point at --public"
args=(--lan 192.168.1.99 --no-start); expect_refusal "is not an address of this machine (its addresses: $lan_ip)"
args=(--lan 192.168.1.999 --no-start); expect_refusal "is not an IPv4 address"
export STUB_IP="tun0=$vpn_ip virbr0=$bridge_ip"; args=(--lan --no-start); expect_refusal "found no address to use"
export STUB_IP="wlan0=$lan_ip"
[ ! -e "$run/station.env" ] || fail "settings were written for a refusal"
pass "two cards: stops and lists them; a given address used; a public one, one that is not this machine's, a malformed one and no card at all are refused"

# A kept address that has gone from the machine: a clear message, not a bind failure later.
export STUB_IP="wlan0=$lan_ip"; install --lan
export STUB_IP="wlan0=192.168.1.77"
args=(); expect_refusal "is not an address of this machine any more"
grep -qF -- "--lan off" "$out" || fail "the gone-address refusal does not name --lan off"
install --lan; [ "$(setting LAN_ADDRESS)" = 192.168.1.77 ] || fail "--lan did not follow the new address"
export STUB_IP="wlan0=$lan_ip"
pass "a kept address that is gone is named, and --lan follows the machine to its new one"

# The port: a taken 7300 moves, and is said; a set one is kept.
rm -f "$run/station.env"
n=$((n + 1)); : > "$calls"; out="$run/out-taken.txt"
PATH="$run/bin:$PATH" STUB_TAKEN="tcp:7300" WEAVESIGNAL_UNPRIV_PORT_FILE=$low_file "$script" --env-file "$run/taken.env" --image x --lan --no-start > "$out" 2>&1 || { cat "$out" >&2; fail "a taken 7300 should move"; }
grep -q "^LAN_PORT=17300$" "$run/taken.env" && grep -qF "LAN_PORT: 7300 (tcp) is in use, so 17300 is used instead" "$out" || { cat "$out" >&2; fail "LAN_PORT did not move to 17300"; }
grep -qx "STREAM_URL=http://$lan_ip:17300/programme.opus" "$run/taken.env" || fail "the stream address does not follow the moved port"
PATH="$run/bin:$PATH" LAN_PORT=7390 WEAVESIGNAL_UNPRIV_PORT_FILE=$low_file "$script" --env-file "$run/given.env" --image x --lan --no-start >/dev/null 2>&1
grep -q "^LAN_PORT=7390$" "$run/given.env" || fail "a LAN_PORT given was not used"
rm -f "$run/taken.env" "$run/given.env"
pass "a taken 7300 moves to 17300 and the stream address follows; a given LAN_PORT is used"

# --lan with --public: refused both ways; the public route turns a kept LAN route off, saying so.
rm -f "$run/station.env"
args=(--lan --public station.test --no-start); expect_refusal "--lan does not go with the public route"
install --lan
n=$((n + 1)); : > "$calls"; out="$run/out-pub.txt"
echo 80 > "$low_file"
PATH="$run/bin:$PATH" NODE_IP=203.0.113.9 WEAVESIGNAL_UNPRIV_PORT_FILE=$low_file "$script" --project lan-check --env-file "$run/station.env" --image x --public station.test --no-start > "$out" 2>&1 || { cat "$out" >&2; fail "going public was refused"; }
echo 1024 > "$low_file"
grep -qF "LAN_ADDRESS=$lan_ip is not used while the station is public" "$out" || { cat "$out" >&2; fail "going public does not say the LAN route is off"; }
[ -z "$(sed -n 's/^LAN_ADDRESS=//p' "$run/station.env")" ] || fail "a public station kept a LAN address"
rm -f "$run/station.env"
pass "--lan with --public is refused, and going public drops a kept LAN route, said"

# --lan off.
export STUB_IP="$all_cards"
install --lan; install --lan off
[ -z "$(setting LAN_ADDRESS)" ] && [ -z "$(setting LAN_PORT)" ] && [ -z "$(setting STREAM_URL)" ] || fail "--lan off left LAN settings"
! called "-f lan.yaml" || fail "--lan off still passes the overlay"
grep -q "is on loopback: nobody off this machine can join or" "$out" || fail "the loopback output is missing"
grep -qF "run this again with --lan" "$out" || fail "the loopback output does not offer --lan"
install; [ -z "$(setting LAN_ADDRESS)" ] || fail "a run after --lan off brought the route back"
pass "--lan off: loopback again, no overlay, no LAN settings, and the next run keeps it off"

# Words that must not be in a plain loopback install.
! grep -q "firewall-cmd" "$out" || fail "a loopback install talks about the firewall"

# G9: the compose provider and Podman's API socket, before anything is pulled.
rm -f "$run/station.env"
install --engine podman
! called "enable --now podman.socket" || fail "a running socket was 'turned on'"
! grep -q "API socket" "$out" || fail "a running socket is mentioned"
STUB_SOCKET=false install --engine podman --no-boot-start
called "systemctl --user enable --now podman.socket" || { cat "$calls" >&2; fail "a missing socket was not turned on at user level"; }
grep -qF "Podman's API socket is not running" "$out" && grep -qF "turned it on (systemctl --user enable --now podman.socket)" "$out" || { cat "$out" >&2; fail "the socket message is missing"; }
! called "sudo" || fail "the socket step ran sudo"
if STUB_SOCKET=false STUB_SYSTEMCTL_FAIL=1 install --engine podman; then fail "a socket that cannot be turned on was ignored"; fi
grep -qF "Run 'systemctl --user enable --now podman.socket' yourself" "$out" || { cat "$out" >&2; fail "the failure does not give the command"; }
! called "pull" || fail "an image was pulled before the socket was ready"
STUB_SOCKET='' install --engine podman
! called "enable --now podman.socket" || fail "an unreadable answer turned the socket on"
STUB_SOCKET=false install --engine docker; ! called "podman.socket" || fail "Docker was asked about Podman's socket"
install --no-start --engine podman >/dev/null; STUB_SOCKET=false install --no-start --engine podman
! called "podman.socket" || fail "--no-start looked at the socket"
pass "G9: a missing Podman socket is turned on as a user service (no sudo) before the pull; when that fails the command is given; an unreadable answer or Docker or --no-start leaves it alone"

if STUB_COMPOSE_FAIL=1 install --engine podman; then fail "Podman with no compose provider was accepted"; fi
grep -qF "brew install docker-compose" "$out" && grep -qF "no compose provider" "$out" || { cat "$out" >&2; fail "the missing provider does not give the Bazzite line"; }
if STUB_COMPOSE_FAIL=1 STUB_DOCKER_FAIL=1 install; then fail "neither engine was accepted"; fi
grep -qF "brew install docker-compose" "$out" || { cat "$out" >&2; fail "with Podman only and no provider, the Bazzite line is missing"; }
pass "G9: Podman with no compose provider says so and gives the Bazzite line (brew install docker-compose) with the Fedora and Ubuntu ones"

# --- 2. The rendered bundle -------------------------------------------------------------------
command -v podman >/dev/null || { echo "skip  parts 2 and 3 need Podman" >&2; exit 0; }
p() { echo $((base + $1)); }
export CONTROL_PORT=$(p 5) CHAT_PORT=$(p 6) LISTENERS_PORT=$(p 7) SRT_INGEST_PORT=$(p 9) PLAYOUT_FEED_PORT=$(p 10) DESK_FEED_PORT=$(p 12) LIVEKIT_HTTP_PORT=$(p 3) LIVEKIT_MEDIA_PORT=$(p 4) LAN_PORT=$(p 1)
rm -f "$run/render.env"
# The documentation address is not private, so the rendering is given a stub card with a private one
# and the address swapped for the rendering's own afterwards (nothing here is a real address).
( PATH="$run/bin:$PATH" STUB_IP="wlan0=$lan_ip" "$script" --no-start --project lan-check --env-file "$run/render.env" --image localhost/stub:none --lan >/dev/null )
for v in CONTROL_PORT CHAT_PORT LISTENERS_PORT SRT_INGEST_PORT PLAYOUT_FEED_PORT DESK_FEED_PORT LIVEKIT_HTTP_PORT LIVEKIT_MEDIA_PORT LAN_PORT; do unset "$v"; done
(cd "$root/deploy/compose" && podman compose --env-file "$run/render.env" -p lan-check -f compose.yaml -f lan.yaml config --format json 2>/dev/null) > "$run/lan.json"
(cd "$root/deploy/compose" && podman compose --env-file "$run/render.env" -p lan-check -f compose.yaml config --format json 2>/dev/null) > "$run/nolan.json"
RUN=$run LAN_IP=$lan_ip python3 -I - <<'PY'
import json, os, re, sys
run, ip = os.environ["RUN"], os.environ["LAN_IP"]
def fail(m): sys.exit("FAIL  " + m)
def env():
    e = {}
    for line in open(f"{run}/render.env"):
        k, sep, v = line.rstrip("\n").partition("=")
        if sep and not k.startswith("#"): e[k] = v
    return e
e = env()
lan = json.load(open(f"{run}/lan.json")); no = json.load(open(f"{run}/nolan.json"))
cfg = lambda c, n: c["configs"][n]["content"]
if "lan" in no["services"] or "lan" in no["configs"]: fail("the loopback bundle has the LAN web server")
s = lan["services"]["lan"]
if s["network_mode"] != "host" or not s["image"].startswith("docker.io/library/caddy:2.") or re.search(r":(latest|2)$", s["image"]): fail("the LAN service is not pinned Caddy on the host network")
cf = cfg(lan, "lan")
port = e["LAN_PORT"]
for want in ["admin off", "auto_https off", f"http://{ip}:{port} {{", f"bind {ip}", f"reverse_proxy 127.0.0.1:{e['LISTENERS_PORT']}", f"reverse_proxy 127.0.0.1:{e['CONTROL_PORT']}"]:
    if want not in cf: fail(f"the Caddyfile lacks: {want}")
for never in ["twirp", "/rtc", "/api/v1/studio", "/api/v1/sessions", "/setup", "/join", "/api/v1/chat", "/health", "/feeds", "/cards", "/media", ":2019", e["CHAT_PORT"], e["LIVEKIT_HTTP_PORT"], e["PLAYOUT_FEED_PORT"], e["SRT_INGEST_PORT"], e["DESK_FEED_PORT"]]:
    if never in cf.replace("admin off", ""): fail(f"the LAN Caddyfile mentions {never}")
c = cfg(lan, "control")
if f'stream_url = "http://{ip}:{port}/programme.opus"' not in c: fail("the player's stream address is not the LAN one")
if '[publishing]\npublic_url = ""' not in c: fail("cards and feeds are not off on the LAN route")
if "forwarded_for = false" not in c or "forwarded_for = false" not in cfg(lan, "chat"): fail("forwarded_for is on")
# Everything else unchanged: the same configs byte for byte except the stream address.
for name in ("livekit", "playout", "desk", "chat"):
    if cfg(lan, name) != cfg(no, name): fail(f"the {name} settings differ with the LAN route on")
strip = lambda t: re.sub(r'stream_url = "[^"]*"', 'stream_url = X', t)
if strip(cfg(lan, "control")) != strip(cfg(no, "control")): fail("the control plane's settings differ beyond the stream address")
if 'bind_addresses: ["127.0.0.1"]' not in cfg(lan, "livekit") or '"bind": "127.0.0.1"' not in cfg(lan, "playout"): fail("a service left loopback")
for name in ("control", "chat", "desk", "livekit", "listeners", "playout"):
    if lan["services"][name] != no["services"][name] and name != "listeners": fail(f"the {name} service differs")
open(f"{run}/Caddyfile", "w").write(cf.replace(ip, "127.0.0.2"))
print(f"pass  the rendered bundle: one pinned Caddy on the host network, bound to the one address, no studio, setup, join, chat, LiveKit or health route in it, the stream address given, cards and feeds off, every other service unchanged")
PY

# --- 3. The Caddyfile, for real ------------------------------------------------------------------
img() { podman image exists "$1" || podman pull -q "$1" >/dev/null; }
img "$caddy_image"
cport=$(p 5); kport=$(p 7); hport=$(p 6)
cat > "$run/backends.Caddyfile" <<EOF
{
	admin off
	auto_https off
}
http://:$cport {
	bind 127.0.0.1
	respond "control path={http.request.uri.path} method={http.request.method}"
}
http://:$kport {
	bind 127.0.0.1
	respond "listeners path={http.request.uri.path} icy=[{http.request.header.Icy-Metadata}] method={http.request.method}"
}
http://:$hport {
	bind 127.0.0.1
	respond "chat path={http.request.uri.path} method={http.request.method}"
}
EOF
podman run -d --name "$prefix-backends" --network host -v "$run/backends.Caddyfile:/etc/caddy/Caddyfile:ro,z" "$caddy_image" >/dev/null
podman run -d --name "$prefix-lan" --network host -v "$run/Caddyfile:/etc/caddy/Caddyfile:ro,z" "$caddy_image" >/dev/null
site="http://127.0.0.2:$(p 1)"
for i in $(seq 1 40); do curl -fs --max-time 2 "$site/api/v1/player" >/dev/null 2>&1 && break; sleep 0.25; [ "$i" = 40 ] && { podman logs "$prefix-lan" >&2; fail "the LAN web server did not answer"; }; done
c() { curl -sS --max-time 10 "$@"; }
check() { # description, expected substring, curl args...
  local what=$1 want=$2; shift 2
  local got; got=$(c "$@") || fail "$what: curl failed"
  [[ $got == *"$want"* ]] || fail "$what: wanted '$want', got '$got'"
}
check "the player page"       "control path=/ "                     "$site/"
check "its files"             "control path=/assets/player-x.js"   "$site/assets/player-x.js"
check "the player's info"     "control path=/api/v1/player"        "$site/api/v1/player"
check "the look"              "control path=/api/v1/branding "     "$site/api/v1/branding"
check "the icon"              "control path=/api/v1/branding/icon" "$site/api/v1/branding/icon"
check "now playing"           "control path=/api/v1/now-playing "  "$site/api/v1/now-playing"
check "now playing's events"  "control path=/api/v1/now-playing/events" "$site/api/v1/now-playing/events"
check "the open routes"       "control path=/api/v1/public/schedule" "$site/api/v1/public/schedule"
check "the video"             "control path=/hls/video.m3u8"       "$site/hls/video.m3u8"
check "the stream"            "listeners path=/programme.opus"     "$site/programme.opus"
check "the stream, ICY dropped" "icy=[]"                           -H "Icy-MetaData: 1" "$site/programme.opus"
check "the MP3 stream"        "listeners path=/programme.mp3"      "$site/programme.mp3"
check "the second MP3 stream" "listeners path=/programme-high.mp3" "$site/programme-high.mp3"
check "HEAD of the stream"    "200"                                -o /dev/null -w '%{http_code}' -I "$site/programme.opus"
for path in /api/v1/studio /api/v1/studio/events /api/v1/sessions /api/v1/station /api/v1/room/tokens /api/v1/carts /api/v1/setup /api/v1/chat/info /api/v1/chat/socket \
  /api/v1/call-in /api/v1/call-in/challenge /api/v1/pass /api/v1/invites /api/v1/openapi.json /setup/ /join/ /echo-test/ /monitor/ /not-set-up/ /health /feeds/a.xml /cards/a.png /media/episodes/a.opus \
  /status.json /twirp/livekit.RoomService/ListRooms /rtc /rtc/validate /api/v1/sources/bridge /api/v1/branding/accent-x/../../studio /programme.opus/../api/v1/studio /assetsx /api/v1/players; do
  code=$(c -s -o "$run/body.txt" -w '%{http_code}' --path-as-is "$site$path")
  [ "$code" = 404 ] || fail "GET $path answered $code, not 404"
  ! grep -qE "control path=|chat path=|listeners path=" "$run/body.txt" || fail "GET $path reached a service"
done
for m in POST PUT DELETE PATCH; do
  for path in /api/v1/player /api/v1/branding /api/v1/branding/icon /api/v1/branding/accent /programme.opus /api/v1/studio /api/v1/sessions /; do
    got=$(c -s -o "$run/body.txt" -w '%{http_code}' -X $m -d x "$site$path")
    [ "$got" = 405 ] || fail "$m $path answered $got, not 405"
    ! grep -qE "control path=|chat path=|listeners path=" "$run/body.txt" || fail "$m $path reached a service"
  done
done
c -sI "$site/" | grep -qi '^server:' && fail "the Server header is sent"
c -sI "$site/" | grep -qi '^strict-transport-security' && fail "HSTS over plain HTTP"
pass "the real Caddyfile: the player, its files, the look, now playing, the open routes, the video and the three stream mounts reach their services (ICY dropped); the studio, its sign-in, setup, join, call-in, the chat, LiveKit, health, feeds, cards and a dotted path to the studio are 404; every write is 405 and never reaches a service"

# The server answers only on its own address: not on the machine's other loopback address.
other=$(curl -s -o /dev/null -w '%{http_code}' --max-time 3 "http://127.0.0.3:$(p 1)/" || true)
[ "$other" = 000 ] || fail "the LAN web server answered on another address ($other)"
pass "the LAN web server is bound to its one address and answers nowhere else"
