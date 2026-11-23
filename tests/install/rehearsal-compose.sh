#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# The rehearsal studio's Compose overlay (1.3): rendered with video on and a hosts' sources
# password set, as a station that has both would render it, the rehearsal desk must still have
# no way to the air. Its settings say rehearsal mode and carry no [video], no [sources] and no
# [programme]; it is given neither the sources password nor playout's feed password, mounts
# neither the HLS volume nor the phone words, publishes no port, and runs at oom_score_adj 500.
# The control plane's rehearsal studio follows REHEARSAL, off by default. Nothing is started.
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
cd "$root/deploy/compose"
common=(LIVEKIT_API_SECRET=check-only-check-only-check-only-0 CHAT_EMAIL_KEY=check-only-check-only-check-only-0
        LISTENERS_SOURCE_PASSWORD=check-only POSTGRES_PASSWORD=check-only)
render() { env "${common[@]}" "$@" podman compose --env-file .env.example "${files[@]}" config --format json; }
files=(-f compose.yaml -f rehearsal.yaml)
on=$(render REHEARSAL=true VIDEO=true DESK_SOURCES_PASSWORD=rehearsal-check-sources WEAVESIGNAL_REHEARSAL_OOM_SCORE_ADJ=)
files=(-f compose.yaml)
off=$(render)
ON="$on" OFF="$off" python3 -I - <<'PY'
import json, os, sys

def fail(why):
    sys.exit(f"rehearsal-compose: {why}")

on, off = json.loads(os.environ["ON"]), json.loads(os.environ["OFF"])
d = on["services"].get("desk-rehearsal") or fail("no desk-rehearsal service with the overlay")
toml = on["configs"]["desk-rehearsal"]["content"]
for want in ('mode = "rehearsal"', 'identity = "desk-rehearsal"', 'room = "studio-rehearsal"',
             'socket = "/run/weavesignal/desk-rehearsal.sock"', 'realtime = false'):
    if want not in toml:
        fail(f"its settings lack {want}")
for banned in ("[video", "[sources", "[programme", "[kit", "[phone"):
    if banned in toml:
        fail(f"its settings carry {banned}]: the desk would refuse to start, or reach the air")
env = d.get("environment") or {}
for key in ("WEAVESIGNAL_DESK_SOURCES_PASSWORD", "WEAVESIGNAL_PLAYOUT_FEED_PASSWORD"):
    if key in env:
        fail(f"it is given {key}")
if any("rehearsal-check-sources" in str(v) for v in env.values()):
    fail("the sources password reaches it")
mounts = {v["source"]: v for v in d.get("volumes", [])}
for vol in ("hls-media", "phone-words"):
    if vol in mounts:
        fail(f"it mounts {vol}")
if "desk-control" not in mounts or not mounts.get("carts-media", {}).get("read_only"):
    fail("it lacks the control socket's volume, or the carts read-only")
if d.get("ports"):
    fail(f"it publishes ports: {d['ports']}")
if d.get("network_mode") != "host":
    fail("it is not on the host's network, as the live desk is")
if int(d.get("oom_score_adj", 0)) != 500:
    fail(f"its oom_score_adj is {d.get('oom_score_adj')}, not 500")
live = on["services"]["desk"]
if int(live.get("oom_score_adj", 0)) >= 500:
    fail("the live desk would be stopped first")
ctl = on["configs"]["control"]["content"]
if "[rehearsal]\nenabled = true" not in ctl.replace("      ", ""):
    fail("REHEARSAL=true does not turn the rehearsal studio on in the control plane")
if "desk-rehearsal" in off["services"]:
    fail("the bundle without the overlay runs a rehearsal desk")
if "[rehearsal]\nenabled = false" not in off["configs"]["control"]["content"].replace("      ", ""):
    fail("the rehearsal studio is not off by default")
print("pass  the rehearsal desk: rehearsal mode, no video, no sources, no programme, no port, oom 500; off by default")
PY
