#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# The Compose bundle's public route (public.yaml, weavesignal.sh --public; ADR-0022), in three parts
# and with no public certificate, domain or open port:
#
#   1. weavesignal.sh with every system command stubbed: what --public writes and refuses, the
#      overlay it passes to Compose, the free-port checks, --renew-turn and --public off.
#   2. The settings it wrote rendered by the real `compose config`, with and without the overlay:
#      the proxy and LiveKit's TURN over TLS on, forwarded_for on in both services, the addresses
#      the pages are given, and the loopback bundle unchanged.
#   3. The rendered Caddyfile run for real, with Caddy's own CA (PUBLIC_TLS=internal) in front of
#      stand-in backends that say what reached them, and curl's --resolve for the name: the
#      routes, the headers dropped and set, what is never published, the plain-HTTP site; then
#      LiveKit's real container loads the certificate Caddy made from the proxy's volume and
#      offers it on the TURN port.
#
# Part 3 needs Podman and the Caddy and LiveKit images (pulled if absent). Ports are
# $((base))..$((base+19)), WEAVESIGNAL_PUBLIC_TEST_PORT sets the base (default 29940), and
# containers and the volume are named after WEAVESIGNAL_TEST_PREFIX. `just public-check`.
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
script="$root/deploy/compose/weavesignal.sh"
run="$root/tests/install/run/public"
prefix=${WEAVESIGNAL_TEST_PREFIX:-weavesignal-m1}-public
base=${WEAVESIGNAL_PUBLIC_TEST_PORT:-29940}
caddy_image=$(sed -n 's|^    image: \(docker.io/library/caddy:.*\)$|\1|p' "$root/deploy/compose/public.yaml")
lk_image=$(sed -n 's|^    image: \(docker.io/livekit/livekit-server:.*\)$|\1|p' "$root/deploy/compose/compose.yaml")
domain=station.test
rm -rf -- "$run"; mkdir -p "$run/bin"

cleanup() {
  [ -z "${KEEP:-}" ] || { echo "KEEP is set: containers and $run are left; remove them by hand" >&2; return 0; }
  for c in "$prefix-proxy" "$prefix-backends" "$prefix-livekit"; do podman rm -f "$c" >/dev/null 2>&1 || true; done
  podman volume rm -f "$prefix-data" >/dev/null 2>&1 || true
  rm -rf -- "$run"
}
trap cleanup EXIT
fail() { echo "FAIL  $*" >&2; exit 1; }
pass() { echo "pass  $*"; }

# --- 1. weavesignal.sh, every system command a stub ---------------------------------------------
calls="$run/calls.log"; : > "$calls"
stub() { # name body
  printf '#!/usr/bin/env bash\nprintf "%%s %%s\\n" "$(basename "$0")" "$*" >> %q\n%s\n' "$calls" "$2" > "$run/bin/$1"
  chmod +x "$run/bin/$1"
}
# STUB_PS makes `compose ps -q` report a running container (the station is already up).
for engine in podman docker; do stub "$engine" 'case "$*" in *" ps -q"*) [ -z "${STUB_PS:-}" ] || echo abc ;; esac; exit 0'; done
# ss reports whatever STUB_TAKEN lists as "proto:port" (space separated) as in use on every address.
stub ss 'for t in ${STUB_TAKEN:-}; do
  proto=${t%%:*}; p=${t#*:}
  [[ " $* " == *" sport = :$p "* ]] || continue
  case "$1" in
    -Hlun) [ "$proto" = udp ] && echo "UNCONN 0 0 0.0.0.0:$p 0.0.0.0:*" ;;
    -Hltn) [ "$proto" = tcp ] && echo "LISTEN 0 0 0.0.0.0:$p 0.0.0.0:*" ;;
  esac
done; exit 0'
stub curl 'echo "{\"database\":\"ok\"}"'
stub loginctl 'case "$*" in *show-user*) echo yes ;; esac'
stub systemctl 'case "$*" in *is-enabled*) exit 0 ;; esac'
low_file="$run/port-start"; echo 1024 > "$low_file"
n=0
install() { # arguments for weavesignal.sh; env in the caller; output in $out, the settings in $env_out
  n=$((n + 1)); : > "$calls"; out="$run/out-$n.txt"; env_out="$run/station.env"
  PATH="$run/bin:$PATH" WEAVESIGNAL_UNPRIV_PORT_FILE=$low_file "$script" --project public-check --env-file "$env_out" \
    --image localhost/stub:none --no-boot-start "$@" > "$out" 2>&1
}
expect_refusal() { # words: the output must hold them
  if install "${args[@]}" 2>/dev/null; then fail "was accepted: ${args[*]}"; fi
  grep -qF -- "$1" "$out" || { cat "$out" >&2; fail "refusal does not say '$1'"; }
}
setting() { sed -n "s/^$1=//p" "$env_out" | head -1; }
called() { grep -qF -- "$1" "$calls"; }

# A rootless Podman that may not bind 80 and 443: refused, with the sysctl named.
if [ "$(id -u)" != 0 ]; then
  args=(--public "$domain"); expect_refusal "net.ipv4.ip_unprivileged_port_start"
  grep -qF "443" "$out" || fail "the refusal does not name the port"
  pass "rootless Podman, 443 not bindable: refused, naming the sysctl"
fi
echo 80 > "$low_file"
install --public "$domain"
[ "$(setting PUBLIC_DOMAIN)" = "$domain" ] && [ "$(setting PROXY_HTTPS_PORT)" = 443 ] && [ "$(setting PROXY_HTTP_PORT)" = 80 ] \
  && [ "$(setting TURN_TLS_PORT)" = 5349 ] || fail "the public settings are not the defaults"
[ "$(setting PUBLIC_ORIGIN)" = "https://$domain" ] && [ "$(setting ROOM_PUBLIC_URL)" = "wss://$domain" ] \
  && [ "$(setting STREAM_URL)" = "https://$domain/programme.opus" ] && [ "$(setting FORWARDED_FOR)" = true ] \
  && [ "$(setting TURN_ENABLED)" = true ] || fail "the derived addresses are wrong"
[ "$(setting TURN_CERT_DIR)" = acme-v02.api.letsencrypt.org-directory ] || fail "acme's certificate directory is wrong"
[ "$(setting LIVEKIT_USE_EXTERNAL_IP)" = true ] && [ -z "$(setting LIVEKIT_NODE_IP)" ] || fail "no NODE_IP should leave the address to STUN"
[ "$(setting TURN_UDP_PORT)" = 0 ] && [ "$(setting LIVEKIT_TCP_PORT)" = 0 ] || fail "TURN over UDP and ICE over TCP are off by default"
[ "$(stat -c %a "$env_out")" = 600 ] || fail "the settings file is not mode 600"
called "compose --env-file $env_out -p public-check -f compose.yaml -f public.yaml up -d" || { cat "$calls" >&2; fail "compose did not get -f public.yaml"; }
grep -q "Open to the internet: TCP 80 and 443" "$out" && grep -qF "TCP 5349 (TURN over TLS)" "$out" || fail "the output does not list what to open"
grep -qF "renew-turn" "$out" || fail "the output does not mention renewal"
! grep -qF "$(setting CONTROL_ADMIN_TOKEN)" "$out" || fail "the admin token was printed"
pass "--public: the name, 443, 80 and 5349, derived addresses, forwarded_for, the overlay passed to Compose, mode 600"

cp "$env_out" "$run/first.env"
install --public "$domain"; cmp -s "$env_out" "$run/first.env" || fail "a second run changed the settings"
install; cmp -s "$env_out" "$run/first.env" || fail "a run without --public dropped the public route"
called "-f public.yaml" || fail "a run without --public did not keep the overlay"
pass "a second run changes nothing, and the route stays on without the flag"

install --status; called "-f public.yaml ps" || fail "--status does not see the proxy"
install --renew-turn; called "-f public.yaml restart livekit" || fail "--renew-turn did not restart LiveKit"
pass "--status and --renew-turn use the overlay"

# Taken ports: the proxy's are not moved to an alternative on their own.
n=$((n + 1)); : > "$calls"; out="$run/out-taken.txt"
if PATH="$run/bin:$PATH" STUB_TAKEN="tcp:443" WEAVESIGNAL_UNPRIV_PORT_FILE=$low_file "$script" --env-file "$run/taken.env" --image x --public "$domain" --no-start > "$out" 2>&1; then
  fail "443 taken was accepted"
fi
grep -qF "PROXY_HTTPS_PORT: 443 (tcp) is in use" "$out" && grep -qF "8443" "$out" && grep -qF "forward" "$out" || { cat "$out" >&2; fail "the refusal does not name the clash, 8443 and forwarding"; }
[ ! -e "$run/taken.env" ] || fail "settings were written for a refused port"
n=$((n + 1)); out="$run/out-taken2.txt"
PATH="$run/bin:$PATH" STUB_TAKEN="tcp:5349" WEAVESIGNAL_UNPRIV_PORT_FILE=$low_file "$script" --env-file "$run/taken2.env" --image x --public "$domain" --no-start > "$out" 2>&1 \
  || { cat "$out" >&2; fail "TURN over TLS on a taken 5349 should move to its alternative"; }
grep -q "^TURN_TLS_PORT=15349$" "$run/taken2.env" && grep -qF "TURN_TLS_PORT: 5349 (tcp) is in use, so 15349 is used instead" "$out" || fail "TURN over TLS did not move to 15349"
n=$((n + 1)); out="$run/out-taken3.txt"
if PATH="$run/bin:$PATH" WEAVESIGNAL_UNPRIV_PORT_FILE=$low_file TURN_UDP_PORT=3478 STUB_TAKEN="udp:3478" "$script" --env-file "$run/taken3.env" --image x --public "$domain" --no-start > "$out" 2>&1; then
  fail "a taken TURN_UDP_PORT was accepted"
fi
grep -qF "TURN_UDP_PORT=3478 (udp) is in use" "$out" || fail "the TURN UDP clash is not named"
pass "taken ports: the proxy's refused (8443 and forwarding named), TURN over TLS moves to 15349, a set TURN UDP port is checked"

# Refusals.
for bad in "https://station.example" "Station.Example" "station" "station.example:8443" "a b.example"; do
  args=(--env-file "$run/bad.env" --public "$bad" --no-start)
  if PATH="$run/bin:$PATH" "$script" --image x "${args[@]}" > "$run/bad.txt" 2>&1; then fail "the domain '$bad' was accepted"; fi
done
grep -qF "lower-case domain name" "$run/bad.txt" || fail "the domain refusal does not say what a domain is"
if PATH="$run/bin:$PATH" PUBLIC_TLS=lets WEAVESIGNAL_UNPRIV_PORT_FILE=$low_file "$script" --image x --env-file "$run/bad.env" --public "$domain" --no-start >/dev/null 2>&1; then fail "PUBLIC_TLS=lets was accepted"; fi
printf 'PROXY_HTTPS_PORT=443\nTURN_TLS_PORT=443\n' > "$run/shared.env"
if PATH="$run/bin:$PATH" STUB_PS=1 WEAVESIGNAL_UNPRIV_PORT_FILE=$low_file "$script" --image x --env-file "$run/shared.env" --public "$domain" --no-start >"$run/bad.txt" 2>&1; then fail "TURN over TLS sharing the proxy's port was accepted"; fi
grep -qF "needs a port of its own" "$run/bad.txt" || { cat "$run/bad.txt" >&2; fail "the shared-port refusal is not said"; }
rm -f "$run/bad.env" "$run/shared.env"
pass "a scheme, a port, capitals, a bare name, an unknown PUBLIC_TLS and a shared TURN port are refused"

# Options in the settings: a given address, the allow list, an email, a local CA, other ports.
rm -f "$run/opt.env"
PATH="$run/bin:$PATH" NODE_IP=203.0.113.9 TURN_ALLOW_CIDRS="10.0.0.0/8, 192.168.0.0/16" ACME_EMAIL=ops@station.test PUBLIC_TLS=internal \
  PROXY_HTTPS_PORT=8443 PROXY_HTTP_PORT=8080 WEAVESIGNAL_UNPRIV_PORT_FILE=$low_file "$script" --image x --env-file "$run/opt.env" --public "$domain" --no-start >/dev/null
o() { sed -n "s/^$1=//p" "$run/opt.env"; }
[ "$(o LIVEKIT_USE_EXTERNAL_IP)" = false ] && [ "$(o LIVEKIT_NODE_IP)" = 203.0.113.9 ] && [ "$(o PROXY_ACME_EMAIL)" = "email ops@station.test" ] \
  && [ "$(o PROXY_LOCAL_CERTS)" = local_certs ] && [ "$(o TURN_CERT_DIR)" = local ] && [ "$(o PUBLIC_ORIGIN)" = "https://$domain:8443" ] \
  && [ "$(o ROOM_PUBLIC_URL)" = "wss://$domain:8443" ] && [ "$(o TURN_ALLOW_CIDRS)" = "10.0.0.0/8, 192.168.0.0/16" ] || { cat "$run/opt.env" >&2; fail "the options did not become the derived settings"; }
pass "NODE_IP, TURN_ALLOW_CIDRS, ACME_EMAIL, PUBLIC_TLS=internal and other proxy ports become the derived settings (the port stays in every address)"

# Back to loopback.
install --public off
[ -z "$(setting PUBLIC_DOMAIN)" ] && [ -z "$(setting STREAM_URL)" ] && [ -z "$(setting TURN_TLS_PORT)" ] || fail "--public off left public settings"
called "-f public.yaml" && fail "--public off still passes the overlay"
grep -q "Every listener is on loopback" "$out" || fail "the loopback output is missing"
if install --renew-turn 2>/dev/null; then fail "--renew-turn worked on a loopback station"; fi
pass "--public off: loopback again, no overlay; --renew-turn refused"

# --- 2. The rendered bundle -------------------------------------------------------------------
command -v podman >/dev/null || { echo "skip  parts 2 and 3 need Podman" >&2; exit 0; }
p() { echo $((base + $1)); }
export CONTROL_PORT=$(p 5) CHAT_PORT=$(p 6) LISTENERS_PORT=$(p 7) SRT_INGEST_PORT=$(p 9) PLAYOUT_FEED_PORT=$(p 10) DESK_FEED_PORT=$(p 12) LIVEKIT_HTTP_PORT=$(p 3) LIVEKIT_MEDIA_PORT=$(p 4)
export PROXY_HTTP_PORT=$(p 0) PROXY_HTTPS_PORT=$(p 1) TURN_TLS_PORT=$(p 2) PUBLIC_TLS=internal NODE_IP=127.0.0.1 TURN_ALLOW_CIDRS=127.0.0.1/32
rm -f "$run/render.env" "$run/loopback.env"
"$script" --no-start --project public-check --env-file "$run/render.env" --image localhost/stub:none --public "$domain" >/dev/null
"$script" --no-start --project public-check --env-file "$run/loopback.env" --image localhost/stub:none >/dev/null
for v in CONTROL_PORT CHAT_PORT LISTENERS_PORT SRT_INGEST_PORT PLAYOUT_FEED_PORT DESK_FEED_PORT LIVEKIT_HTTP_PORT LIVEKIT_MEDIA_PORT PROXY_HTTP_PORT PROXY_HTTPS_PORT TURN_TLS_PORT PUBLIC_TLS NODE_IP TURN_ALLOW_CIDRS; do unset "$v"; done
render() { # settings file, compose files...: the resolved bundle as JSON
  local f=$1; shift
  (cd "$root/deploy/compose" && podman compose --env-file "$run/$f" -p public-check "$@" config --format json 2>/dev/null)
}
render render.env -f compose.yaml -f public.yaml > "$run/public.json"
render loopback.env -f compose.yaml > "$run/loopback.json"
RUN=$run python3 -I - <<'PY'
import json, os, re, sys
run = os.environ["RUN"]
def load(name): return json.load(open(f"{run}/{name}.json"))
def env():
    e = {}
    for line in open(f"{run}/render.env"):
        k, sep, v = line.rstrip("\n").partition("=")
        if sep and not k.startswith("#"): e[k] = v
    return e
e, pub, loop = env(), load("public"), load("loopback")
def fail(m): sys.exit("FAIL  " + m)
cfg = lambda c, n: c["configs"][n]["content"]
https, tls, lk = e["PROXY_HTTPS_PORT"], e["TURN_TLS_PORT"], e["LIVEKIT_HTTP_PORT"]

if "proxy" in loop["services"] or "proxy" in loop["configs"]: fail("the loopback bundle has a proxy")
if "enabled: false" not in cfg(loop, "livekit") or "udp_port: 0" not in cfg(loop, "livekit"): fail("loopback LiveKit has TURN on")
if "tcp_port: 0" not in cfg(loop, "livekit") or "node_ip: 127.0.0.1" not in cfg(loop, "livekit"): fail("loopback LiveKit is not loopback")
if "forwarded_for = false" not in cfg(loop, "control") or "forwarded_for = false" not in cfg(loop, "chat"): fail("loopback forwarded_for is on")
if 'public_url = "ws://127.0.0.1:' not in cfg(loop, "control") or 'stream_url = "http://127.0.0.1:' not in cfg(loop, "control"): fail("loopback addresses changed")
if '[publishing]\npublic_url = ""' not in cfg(loop, "control"): fail("loopback cards and feeds are not off")
print("pass  the loopback bundle: no proxy, TURN off, forwarded_for off, loopback addresses, cards and feeds off")

proxy = pub["services"]["proxy"]
if proxy["network_mode"] != "host" or not proxy["image"].startswith("docker.io/library/caddy:2."): fail("the proxy service is not pinned Caddy on the host network")
if re.search(r":(latest|2)$", proxy["image"]): fail("the proxy image is not pinned to a minor version")
dep = pub["services"]["livekit"]["depends_on"]
if dep.get("proxy", {}).get("condition") != "service_healthy": fail("LiveKit does not wait for the proxy's certificate")
if not any(v["source"] == "proxy-data" and v["target"] == "/caddy-data" and v.get("read_only") for v in pub["services"]["livekit"]["volumes"]): fail("LiveKit does not read the proxy's data volume read only")
if not any(v["source"] == "proxy-data" and v["target"] == "/data" for v in proxy["volumes"]): fail("the proxy does not keep its data")
hc = " ".join(proxy["healthcheck"]["test"])
if f"{e['PUBLIC_DOMAIN']}.crt" not in hc or f"{e['PUBLIC_DOMAIN']}.key" not in hc: fail("the proxy's health is not the certificate and key")
live = cfg(pub, "livekit")
for want in ["enabled: true", f"tls_port: {tls}", "udp_port: 0", f'domain: "{e["PUBLIC_DOMAIN"]}"', "external_tls: false",
             f"cert_file: /caddy-data/caddy/certificates/{e['TURN_CERT_DIR']}/{e['PUBLIC_DOMAIN']}/{e['PUBLIC_DOMAIN']}.crt",
             f"key_file: /caddy-data/caddy/certificates/{e['TURN_CERT_DIR']}/{e['PUBLIC_DOMAIN']}/{e['PUBLIC_DOMAIN']}.key",
             "allow_restricted_peer_cidrs: [127.0.0.1/32]", "use_external_ip: false", "node_ip: 127.0.0.1", "tcp_port: 0",
             "bind_addresses: [\"127.0.0.1\"]"]:
    if want not in live: fail(f"LiveKit's settings lack: {want}")
control, chat = cfg(pub, "control"), cfg(pub, "chat")
for want in [f"turn_tls = {tls}", f"proxy_https = {https}", f'public_url = "wss://{e["PUBLIC_DOMAIN"]}:{https}"',
             f'stream_url = "https://{e["PUBLIC_DOMAIN"]}:{https}/programme.opus"', "forwarded_for = true",
             f'[publishing]\npublic_url = "https://{e["PUBLIC_DOMAIN"]}:{https}"']:
    if want not in control: fail(f"the control plane's settings lack: {want}")
if "forwarded_for = true" not in chat: fail("the chat's forwarded_for is off behind the proxy")
ctl = pub["services"]["control"]["volumes"]
for src, target in (("show-cards", "/var/lib/weavesignal/show-cards"), ("episodes-media", "/var/lib/weavesignal/episodes")):
    if not any(v["source"] == src and v["target"] == target for v in ctl): fail(f"the control plane does not keep {target} in a volume")
print("pass  the public bundle: pinned Caddy, LiveKit waits for the certificate and reads it, TURN over TLS on",
      f"{tls}, forwarded_for on in the control plane and the chat, the pages, cards and feeds given {e['PUBLIC_DOMAIN']}:{https}")

caddy = cfg(pub, "proxy")
for never in ["twirp", "status.json", ":2019"]:
    if never in caddy.replace("admin off", ""): fail(f"the proxy's Caddyfile mentions {never}")
if "admin off" not in caddy: fail("Caddy's admin API is not off")
for port_key in ("PLAYOUT_FEED_PORT", "DESK_FEED_PORT", "SRT_INGEST_PORT"):
    if f":{e[port_key]}" in caddy: fail(f"the proxy reaches {port_key}")
open(f"{run}/Caddyfile", "w").write(caddy)
open(f"{run}/livekit.yaml", "w").write(live)
print("pass  the Caddyfile: admin off, and no LiveKit API, status page, feed or ingest port in it")
PY

# --- 3. The Caddyfile and LiveKit, for real ---------------------------------------------------
img() { podman image exists "$1" || podman pull -q "$1" >/dev/null; }
img "$caddy_image"; img "$lk_image"
http=$(p 0); https=$(p 1); turn=$(p 2)
# Stand-in backends: each says which it is, and what reached it.
cport=$(p 5); hport=$(p 6); lport=$(p 7); kport=$(p 3)
cat > "$run/backends.Caddyfile" <<EOF
{
	admin off
	auto_https off
}
http://:$cport {
	bind 127.0.0.1
	respond "control path={http.request.uri.path} xff={http.request.header.X-Forwarded-For} proto={http.request.header.X-Forwarded-Proto} host={http.request.host} method={http.request.method}"
}
http://:$hport {
	bind 127.0.0.1
	respond "chat path={http.request.uri.path} xff={http.request.header.X-Forwarded-For} method={http.request.method}"
}
http://:$lport {
	bind 127.0.0.1
	respond "listeners path={http.request.uri.path} icy=[{http.request.header.Icy-Metadata}] method={http.request.method}"
}
http://:$kport {
	bind 127.0.0.1
	respond "livekit path={http.request.uri.path} upgrade={http.request.header.Upgrade}"
}
EOF
podman run -d --name "$prefix-backends" --network host -v "$run/backends.Caddyfile:/etc/caddy/Caddyfile:ro,z" "$caddy_image" >/dev/null
podman volume create "$prefix-data" >/dev/null
podman run -d --name "$prefix-proxy" --network host -v "$run/Caddyfile:/etc/caddy/Caddyfile:ro,z" -v "$prefix-data:/data" "$caddy_image" >/dev/null
for i in $(seq 1 60); do
  podman exec "$prefix-proxy" test -s "/data/caddy/certificates/local/$domain/$domain.key" 2>/dev/null && break
  sleep 0.5; [ "$i" = 60 ] && { podman logs "$prefix-proxy" >&2; fail "Caddy made no certificate"; }
done
podman exec "$prefix-proxy" cat /data/caddy/pki/authorities/local/root.crt > "$run/root.crt"
for i in $(seq 1 40); do curl -fs --max-time 2 "http://127.0.0.1:$cport/" >/dev/null 2>&1 && break; sleep 0.25; done
c() { curl -sS --max-time 10 --cacert "$run/root.crt" --resolve "$domain:$https:127.0.0.1" --resolve "$domain:$http:127.0.0.1" "$@"; }
site="https://$domain:$https"
check() { # description, expected substring, curl args...
  local what=$1 want=$2; shift 2
  local got; got=$(c "$@") || fail "$what: curl failed"
  [[ $got == *"$want"* ]] || fail "$what: wanted '$want', got '$got'"
}
check "the pages"                "control path=/ "                 "$site/"
check "the API"                  "control path=/api/v1/station"    "$site/api/v1/station"
check "health"                   "control path=/health"            "$site/health"
check "HLS"                      "control path=/hls/video.m3u8"    "$site/hls/video.m3u8"
check "feeds"                    "control path=/feeds/kettle.xml"  "$site/feeds/kettle.xml"
check "the chat's socket route"  "chat path=/api/v1/chat/socket"   "$site/api/v1/chat/socket"
check "the chat's other routes"  "chat path=/api/v1/chat/info"     "$site/api/v1/chat/info"
check "LiveKit's signalling"     "livekit path=/rtc "              "$site/rtc"
check "LiveKit's validation"     "livekit path=/rtc/validate"      "$site/rtc/validate"
check "the stream"               "listeners path=/programme.opus"  "$site/programme.opus"
check "the stream, ICY dropped"  "icy=[]"                          -H "Icy-MetaData: 1" "$site/programme.opus"
check "the stream, read only"    "405"                             -o /dev/null -w '%{http_code}' -X POST "$site/programme.opus"
check "the MP3 stream"           "listeners path=/programme.mp3"   "$site/programme.mp3"
check "no status page"           "control path=/status.json"       "$site/status.json"
check "no LiveKit API"           "control path=/twirp/livekit.RoomService/ListRooms" "$site/twirp/livekit.RoomService/ListRooms"
check "the control plane learns it was https" "proto=https"        "$site/"
pass "https: pages, API, health, HLS and feeds to the control plane; chat; /rtc to LiveKit; the stream read only with ICY dropped; /status.json and /twirp never reach their services"

# A client's own X-Forwarded-For is never passed on: both services count one address.
for path in "/" "/api/v1/chat/info"; do
  got=$(c -H "X-Forwarded-For: 6.6.6.6, 7.7.7.7" "$site$path")
  [[ $got == *"xff=127.0.0.1 "* ]] || fail "a client's X-Forwarded-For reached the service for $path: $got"
done
pass "X-Forwarded-For: a client's own header is replaced by the connection's address (both services)"
c -sI "$site/" | grep -qi '^server:' && fail "the Server header is sent"
c -sI "$site/" | grep -qi '^strict-transport-security: max-age=31536000' || fail "no Strict-Transport-Security on https"
c -sI "http://$domain:$http/programme.opus" | grep -qi '^strict-transport-security' && fail "Strict-Transport-Security sent over plain HTTP"
pass "no Server header; HSTS on https only"

plain="http://$domain:$http"
check "plain HTTP serves the stream" "listeners path=/programme.opus" "$plain/programme.opus"
check "plain HTTP serves the MP3"    "listeners path=/programme.mp3"  "$plain/programme.mp3"
check "plain HTTP drops ICY"         "icy=[]"                          -H "Icy-MetaData: 1" "$plain/programme.opus"
loc=$(c -s -o /dev/null -w '%{http_code} %{redirect_url}' "$plain/health?x=1")
[ "$loc" = "301 $site/health?x=1" ] || fail "plain HTTP redirects to '$loc'"
loc=$(c -s -o /dev/null -w '%{http_code}' "$plain/api/v1/station")
[ "$loc" = 301 ] || fail "plain HTTP served the API ($loc)"
pass "plain HTTP: the stream served, everything else redirected to the https address with its port"

# The certificate is for the name, and LiveKit loads the same one for TURN over TLS. The
# stand-in for LiveKit's signalling held its port; it is done with.
podman rm -f "$prefix-backends" >/dev/null
podman run -d --name "$prefix-livekit" --network host -e "LIVEKIT_KEYS=check: check-check-check-check-check-check-0" \
  -v "$run/livekit.yaml:/etc/livekit.yaml:ro,z" -v "$prefix-data:/caddy-data:ro" "$lk_image" --config /etc/livekit.yaml >/dev/null
for i in $(seq 1 60); do
  ss -Hltn "sport = :$turn" | grep -q . && break
  sleep 0.5; [ "$i" = 60 ] && { podman logs "$prefix-livekit" >&2; fail "LiveKit did not offer TURN over TLS on $turn"; }
done
served=$(echo | openssl s_client -connect "127.0.0.1:$turn" -servername "$domain" 2>/dev/null | openssl x509 -noout -fingerprint -sha256)
made=$(podman exec "$prefix-proxy" cat "/data/caddy/certificates/local/$domain/$domain.crt" | openssl x509 -noout -fingerprint -sha256)
[ -n "$served" ] && [ "$served" = "$made" ] || fail "TURN over TLS does not serve Caddy's certificate ($served / $made)"
echo | openssl s_client -connect "127.0.0.1:$turn" -servername "$domain" -CAfile "$run/root.crt" -verify_hostname "$domain" 2>&1 | grep -q "Verify return code: 0" \
  || fail "the TURN certificate does not verify for $domain against Caddy's CA"
pass "LiveKit loaded Caddy's certificate from the volume, and TURN over TLS on $turn presents it, verified for $domain"
