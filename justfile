# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
# WeaveSignal tasks. `just --list` shows them all.

set shell := ["bash", "-euo", "pipefail", "-c"]

root := justfile_directory()
# The Compose bundle gives each container oom_score_adj ${WEAVESIGNAL_OOM_SCORE_ADJ:-0}. A session
# that runs above 0 cannot `podman exec` into a container made with 0 (crun: write to
# /proc/self/oom_score_adj: Permission denied), so PostgreSQL's healthcheck never passes. Every
# recipe gets the session's own value when it is above 0 (never lower); set the variable to override.
export WEAVESIGNAL_OOM_SCORE_ADJ := env("WEAVESIGNAL_OOM_SCORE_ADJ", `v=$(cat /proc/self/oom_score_adj 2>/dev/null || echo 0); case $v in ''|*[!0-9]*) echo 0 ;; *) echo $v ;; esac`)
# The desk's build image; settable, so a worktree that changes it does not retag another's. `just
# desk-image` builds it under this tag; the old default `:m5` (2026-11-01) predated RTMP out and the
# AV1 camera fixes and ran silently where it still existed, so a missing `:current` fails instead.
desk_image := env("WEAVESIGNAL_DESK_IMAGE", "localhost/weavesignal-desk-dev:current")
liquidsoap_image := "docker.io/savonet/liquidsoap:v2.5.0-rc1"
livekit_image := "docker.io/livekit/livekit-server:v1.13.10"
# Test containers' names and the throwaway database's port: settable, so two worktrees can run
# their gates at once (`WEAVESIGNAL_TEST_PREFIX=weavesignal-carts WEAVESIGNAL_TEST_PG_PORT=31432`).
test_prefix := env("WEAVESIGNAL_TEST_PREFIX", "weavesignal-m1")
test_pg_port := env("WEAVESIGNAL_TEST_PG_PORT", "17432")
# The server image's tag; the Compose bundle runs SERVER_IMAGE (deploy/compose/.env.example).
server_image := env("WEAVESIGNAL_SERVER_IMAGE", "localhost/weavesignal-server:m1")
# The edge images (M7): the HLS edge and the audio relay.
edge_image := env("WEAVESIGNAL_EDGE_IMAGE", "localhost/weavesignal-edge:m1")
relay_image := env("WEAVESIGNAL_RELAY_IMAGE", "localhost/weavesignal-relay:m1")
# The studio app's Linux build image (Ubuntu 24.04: the packages' oldest distribution, ADR-0126).
desktop_image := env("WEAVESIGNAL_DESKTOP_IMAGE", "localhost/weavesignal-desktop-build:m1")
# The Windows installer's cross-build image (Debian 13, MinGW-w64, NSIS 3.11).
desktop_windows_image := env("WEAVESIGNAL_DESKTOP_WINDOWS_IMAGE", "localhost/weavesignal-desktop-windows:m1")

# Run inside the desk's build image: the worktree at /work, its own target directory
# (the image's Rust is not the host's), and the host's crate cache.
# WEAVESIGNAL_DESK_GPU=1 hands the container the NVIDIA GPU (NVENC); without it the desk falls back
# to SVT-AV1 on the CPU, and the GPU tests skip.
desk_gpu := if env("WEAVESIGNAL_DESK_GPU", "") == "1" { "--device nvidia.com/gpu=all " } else { "" }
in_desk := "podman run --rm --network host " + desk_gpu + "--security-opt label=disable -v " + root + ":/work -v " + env("HOME") + "/.cargo/registry:/root/.cargo/registry -w /work -e CARGO_TARGET_DIR=/work/target/container " + desk_image
# Xiph's clips for test-vmaf, mounted read-only at /clips.
clips := env("WEAVESIGNAL_CLIPS", root + "/target/clips")
in_desk_with_clips := "podman run --rm --network host " + desk_gpu + "--security-opt label=disable -v " + root + ":/work -v " + clips + ":/clips:ro -v " + env("HOME") + "/.cargo/registry:/root/.cargo/registry -w /work -e CARGO_TARGET_DIR=/work/target/container " + desk_image

default:
    @just --list

# Build every crate except the desk (it needs GStreamer; see desk-check)
build:
    cargo build

test:
    cargo test

# Clippy with warnings as errors, and rustfmt, on the host's crates
lint:
    cargo clippy --all-targets -- -D warnings
    cargo fmt --all --check

fmt:
    cargo fmt --all

# Every host gate: build, test, lint
check: build test lint

# The desk image's base: GStreamer with the patched LiveKit signaller (spike/desk-image), built
# once; delete the image to build it again
desk-spike:
    podman image exists localhost/weavesignal-desk-spike:dev || podman build -t localhost/weavesignal-desk-spike:dev {{root}}/spike/desk-image

# Build the desk's build-and-run image (derived from spike 01's desk image)
desk-image: desk-spike
    podman build -t {{desk_image}} -f {{root}}/deploy/images/desk/Containerfile {{root}}/deploy/images/desk

# Build the desk in its image
desk-build:
    {{in_desk}} cargo build -p weavesignal-desk --all-targets

# Build, test and lint the desk in its image
desk-check:
    {{in_desk}} sh -c 'cargo build -p weavesignal-desk --all-targets && cargo test -p weavesignal-desk && cargo clippy -p weavesignal-desk --all-targets -- -D warnings'

# Type-check both Liquidsoap scripts against the sample data
liq-check:
    podman run --rm --security-opt label=disable -v {{root}}/playout:/p:ro -e WEAVESIGNAL_PLAYOUT_DATA=/p/playout.example.json -e WEAVESIGNAL_LISTENERS_SOURCE_PASSWORD=check-only --entrypoint /usr/bin/liquidsoap {{liquidsoap_image}} --check /p/weavesignal.liq
    podman run --rm --security-opt label=disable -v {{root}}/playout:/p:ro -e WEAVESIGNAL_PLAYOUT_DATA=/p/playout.example.json -e WEAVESIGNAL_LISTENERS_SOURCE_PASSWORD=check-only --entrypoint /usr/bin/liquidsoap {{liquidsoap_image}} --check /p/listeners.liq
    # No lookahead in playout's Liquidsoap: its buffer starts as uninitialised memory and can leave the limiter silent for good
    ! grep -nE 'lookahead *= *[0-9.]*[1-9]' {{root}}/playout/*.liq
    # The desk's SRT input names its container: FFmpeg's probe of it stayed as about 3 s of listener delay (2026-11-03)
    grep -q 'srt_format = data.srt.format ?? "matroska"' {{root}}/playout/weavesignal.liq
    # The guard against NaN that left staging silent: Liquidsoap's clip must still turn it into 0, through the whole chain
    podman run --rm --security-opt label=disable -v {{root}}/tests/playout:/r/tests/playout:ro -v {{root}}/playout:/r/playout:ro --entrypoint /usr/bin/liquidsoap {{liquidsoap_image}} /r/tests/playout/nan.liq
    # The processing chain that plays (playout/chain.liq) on tones: raised, lowered, left alone, not pumped, never over the limiter's ceiling
    podman run --rm --security-opt label=disable -v {{root}}/tests/playout:/r/tests/playout:ro -v {{root}}/playout:/r/playout:ro --entrypoint /usr/bin/liquidsoap {{liquidsoap_image}} /r/tests/playout/chain.liq
    # The video's sound from playout is on playout's own clock: on a clock of its own it fell behind on a busy server and filled with silence (2026-11-12)
    ! grep -q 'to_desk_buffer' {{root}}/playout/weavesignal.liq
    # The outgoing Icecast source (ADR-0226) stays on a clock of its own, so a remote that stalls cannot hold up our listeners
    grep -q 'buffer(id="icecast_out_buffer"' {{root}}/playout/listeners.liq
    # The default loop plays three laps with no gap at the join (ADR-0130)
    podman run --rm --security-opt label=disable -v {{root}}/tests/playout:/t:ro -v {{root}}/assets/default-loop:/a:ro --entrypoint /usr/bin/liquidsoap {{liquidsoap_image}} /t/default-loop.liq

# The desk against LiveKit and Liquidsoap on loopback, on ports clear of the defaults
test-room:
    TEST=room {{root}}/tests/room/run-stack.sh

# People join and leave in cycles; the desk's elements, pads, threads and idle CPU return to baseline
test-churn:
    TEST=churn {{root}}/tests/room/run-stack.sh

# One person's pipeline failure stays theirs; twelve people join at once
test-contain:
    TEST=contain {{root}}/tests/room/run-stack.sh

# The listener stream keeps a steady byte rate with the room empty
test-empty:
    TEST=empty {{root}}/tests/room/run-stack.sh

# A digitally silent microphone stays linked and is never subscribed again (issue 13)
test-quiet:
    TEST=quiet {{root}}/tests/room/run-stack.sh

# The profanity delay dumped: the listener never hears the audio it held
test-dump:
    TEST=dump {{root}}/tests/room/run-stack.sh

# Xiph's FourPeople_1280x720_60.y4m and KristenAndSara_1280x720_60.y4m (media.xiph.org/video/derf/y4m/)
# in WEAVESIGNAL_CLIPS, target/clips by default (a folder outside target/ survives its clearing);
# WEAVESIGNAL_DESK_GPU=1 adds NVENC.
# The picture's quality at each rung: the desk's encoders and CMAF writer, decoded to raw frames, scored with VMAF
test-vmaf:
    test -s "{{clips}}/FourPeople_1280x720_60.y4m" && test -s "{{clips}}/KristenAndSara_1280x720_60.y4m" || { echo "Xiph's two clips are not in {{clips}}: set WEAVESIGNAL_CLIPS"; exit 1; }
    {{in_desk_with_clips}} env WEAVESIGNAL_CLIPS=/clips cargo test --release -p weavesignal-desk --test vmaf -- --ignored --nocapture

# The desk's CPU with 4, 8 and 13 people, a release build (a measurement, not in `ci`)
test-load:
    TEST=load {{root}}/tests/room/run-stack.sh

# The rehearsal studio's canary (1.3): 1,733 Hz in the rehearsal room never reaches the listener mount, the
# rehearsal desk holds no listening port, moved people stand by until a press; prints its cost (a release build)
test-rehearsal:
    TEST=rehearsal {{root}}/tests/room/run-stack.sh

# A rehearsal desk's CPU with 4 and 6 people and carts sounding (a measurement, not in `ci`)
test-load-rehearsal:
    TEST=load WEAVESIGNAL_LOAD_MODE=rehearsal LOAD_STEPS=4,6 {{root}}/tests/room/run-stack.sh

# Hosts in their own studios (ADR-0280) end to end on loopback (25xxx): ffmpeg as a host's software over
# Icecast (MP3 at 44.1 kHz) and SRT (AAC in MPEG-TS with a passphrase) into the ingest, the control plane
# and the desk in its image (`just desk-build` first): standing by in the slot and heard at the desk, a
# wrong key and a wrong passphrase refused, a test outside the slot never on the desk, an ended key closed,
# the slot's door closing (standing by becomes a private test, or is disconnected with tests off; on air is never touched)
test-sources:
    WEAVESIGNAL_DESK_IMAGE={{desk_image}} DESK=real {{root}}/tests/sources/run-sources.sh

# The same with a stand-in desk that keeps what the ingest hands it (no desk image; about 40 s)
test-sources-probe:
    DESK=stub {{root}}/tests/sources/run-sources.sh

# WHIP for remote DJs (17d) with a client not ours: GStreamer's whipclientsink from the desk image,
# behind rootless Podman's NAT, sending a live tone to /whip/<key id>; a stand-in desk, PostgreSQL
# in a container (22xxx, loopback). A wrong bearer refused, the source standing by with way whip
# and heard, its DELETE ending it.
# Not in `ci` yet. A real OBS stays a person's try (docs: Send from OBS over WHIP).
test-whip:
    WEAVESIGNAL_DESK_IMAGE={{desk_image}} {{root}}/tests/sources/whip.sh

# A host's monitor link (ADR-0280, design 4.5) end to end on loopback (26xxx): two hosts' software sending tones
# over Icecast, the real desk in its image (`just desk-build` first), LiveKit, and headless Chromium as a monitor
# page (needs PLAYWRIGHT_CORE and `pnpm install` in apps/web): the monitor mix carries the other host and never the
# host's own tone, the cue bus and talk addressed to them, over two seats and no third; the receiver's concealment
# stays near 0 %; a stranger in the monitor room hears nothing; the mix goes when the last listener does. Not in
# `ci`: it needs a browser outside the repository.
test-monitor:
    WEAVESIGNAL_DESK_IMAGE={{desk_image}} {{root}}/tests/monitor/run-monitor.sh

# Playout, then the desk, killed and restarted under a listener: the programme comes back
test-playout:
    TEST=playout {{root}}/tests/room/run-stack.sh

# The video's sound from the real playout, both real ends (ADR-0172; review 0028, A1)
test-feed:
    TEST=feed {{root}}/tests/room/run-stack.sh

# Build the image first (`just server-image`); the run's output goes under ~/Code/weavesignal-soak-runs.
# The Compose bundle on its own project and ports (31410 to 31482) with a seeded week, samples and a held
# listener, then RESULT.md; `DURATION=20m just soak-24h` for a short one; not in `ci` (issue 12)
soak-24h:
    {{root}}/tests/soak/run-24h.sh

# The chat with 4,000 sockets, 40 that never read and 200 that reconnect, at 7, 20 and 60
# messages a second (a release build on ports 37311 and 37432 by default; a measurement, not in `ci`)
test-chat-load:
    {{root}}/tests/chat/load.sh

# The playout layer (M4) on the stack: dead air to automation and back, playout killed under a
# listener who stays connected (the fallback, then the desk), the per-play log, the audience
# counts, now playing with and without up-next, the logger following retention
test-onair:
    {{root}}/tests/playout/run-playout.sh

# Keyed shows end to end (1.3.2): the real playout (its chain on) and listener process, a stand-in
# desk, PostgreSQL and the control plane, which keeps playout's public gate on the schedule; a
# public listener and a key holder at once, the key revoked halfway; the public must hear nothing
# of the keyed show. Then the edge's njs check against the control plane's real token. Ports in
# 264xx. `NEGATIVE=1` shows the test catches a leak. Then what the public hears meanwhile, for a
# station with a slate file, with fallback files, and with only the default loop (an empty folder,
# or one with nothing that plays): real sound, never silence (tests/keyed/run-slate.sh, 266xx).
test-keyed: edge-image
    EDGE_IMAGE={{edge_image}} {{root}}/tests/keyed/run-stack.sh
    {{root}}/tests/keyed/run-slate.sh

# Playout's public gate and the keyed mounts alone (the 1.3.2 spike's test): keyed automation items
# and a keyed live show, the public Opus and MP3 and a late listener, no keyed sample, no gap
test-keyed-switch:
    {{root}}/tests/keyed/run-switch.sh

# The programme sent to another Icecast server (ADR-0226; branch icecast-out): Icecast 2.5 stands in for a DJ's
# AzuraCast on ports 223xx. The MP3 arrives and decodes; the source follows the desk and the manual choice;
# the remote killed and a wrong password leave our own listeners unbroken; the right password is taken again
test-icecast-out: relay-image
    RELAY_IMAGE={{relay_image}} {{root}}/tests/playout/run-icecast-out.sh

# Carts fired over HTTP heard in the listener stream (Liquidsoap and PostgreSQL on 31xxx ports,
# the demo pack loaded through the media worker); FILE MISSING; a bed ducked by a sweeper
test-carts:
    {{root}}/tests/carts/run-carts.sh

# M3's browser runs (needs PLAYWRIGHT_CORE: an installed playwright-core with its Chromium): the
# join stack with LiveKit's TURN over TLS and the pages from the control plane, a caller whose
# browser blocks WebRTC's UDP joining through the relay alone and heard, then the whole join flow
# at 390 px with touch. Ports in 39000-39999 (M3_*_PORT to move them). Not in `ci`: it needs a
# browser outside the repository.
test-m3:
    {{root}}/tests/m3/run-relay.sh

# The caller's page on a phone with no stack (needs PLAYWRIGHT_CORE): the player, the lobby and
# the call panel at 390 and 320 px against a station that is only answers and a room that never
# answers. The pages stay as wide as the window, Reconnect through the relay takes a tap (the
# step test-m3 timed out on), and the station's banner picture keeps its 3:1 shape. Not in `ci`.
test-join-layout:
    cd {{root}}/apps/web && pnpm install --frozen-lockfile && pnpm build
    node {{root}}/tests/m3/layout.mjs

# A browser's camera at the desk (needs PLAYWRIGHT_CORE): headless Chromium publishes a camera as
# the join page does (AV1, the talk station's Brave fault of 2026-11-11), and the desk keeps that
# person's one subscription, hears them and shows their camera. Not in `ci`, for the same reason;
# the images' element checks (rtpav1depay) are.
test-browsercam:
    TEST=browsercam {{root}}/tests/room/run-stack.sh

# Congestion control at the desk (issue 1), rootless: the real desk, LiveKit and a headless guest
# on a podman network of their own, the links shaped with tc, the desk's rtpgccbwe on and off
# (needs PLAYWRIGHT_CORE and a release desk for the cost rows). A measurement, not a pass mark,
# and not in `ci`: about two hours in all. `just test-cc audio1` runs one group of about ten.
test-cc group="all":
    {{root}}/tests/cc/matrix.sh {{group}}

# The edge images (M7): nginx as the HLS pull-through cache, Icecast 2.5 as the audio relay
edge-image: relay-image
    podman build --target hls -t {{edge_image}} {{root}}/deploy/images/edge

# The relay image alone (Icecast 2.5 with the encrypted hop, ADR-0162)
relay-image:
    podman build --target relay -t {{relay_image}} {{root}}/deploy/images/edge

# The relay image's encrypted hop (ADR-0162) against a stand-in HTTPS station with a throwaway
# authority on ports 22501-22503: the stream and the login arrive, other paths are refused, and a
# certificate that fails the check carries nothing
test-relay-tls: relay-image
    RELAY_IMAGE={{relay_image}} {{root}}/tests/edges/relay-tls.sh

# Two edges and two relays in front of a stand-in HLS origin and the listener process: origin
# against edge egress, an edge killed and steered around, audio and metadata through the relays
test-edges: edge-image
    EDGE_IMAGE={{edge_image}} RELAY_IMAGE={{relay_image}} {{root}}/tests/edges/run-edges.sh

# The Schedule tab and the card editor against the control plane on loopback (PostgreSQL on
# 26432 and ports in 26xxx by default; no room, no playout): seeded with Kettle FM's week, then
# waits for a browser, or for tests/screens/drive.mjs (Playwright, not a dependency; a
# measurement with screenshots, not in `ci`)
test-screens:
    {{root}}/tests/screens/run-screens.sh

# The Screener tab's layout against the stand-in station (no stack): at 1,440 by 900 up to 3,840 by
# 2,160 and at 100, 120 and 150 % text, every panel is whole inside the window, none is squeezed
# under its floor or covers another, and nothing is cut at a panel's or the status bar's side. In
# WebKitGTK 4.1, the app's own engine (python3 with GTK 3 and WebKit2 4.1), and in Chromium
# (Playwright: set PLAYWRIGHT_CORE); an engine not on this machine is left out and said.
# Screenshots into target/shots-screener-layout. Not in `ci`.
test-screener-layout:
    cd {{root}}/apps/studio && pnpm install --frozen-lockfile && pnpm build
    node {{root}}/tests/screens/screener-layout.mjs

# The monitor link's page (ADR-0280) in a real browser against a stand-in station: each lamp
# state, the counts on the station's clock and in their last seconds, Start listening off where
# nothing can play, every press and what it sends, an ended key and a wrong secret; screenshots in Dark, Light and Night at 1440×900 and 390×844 into
# target/shots-monitor-page. No stack. Needs Playwright, which is not a dependency of the
# repository: set PLAYWRIGHT_CORE to a playwright-core directory. Not in `ci`.
test-monitor-page:
    cd {{root}}/apps/web && pnpm install --frozen-lockfile
    node {{root}}/tests/monitor-page/shoot.mjs

# The page `/` shows until the station is set up (1.1) in a real browser: its words, its links,
# the pages' Content-Security-Policy and a phone's width; screenshots in Dark and Light at 1440×900
# and 390×844 into target/shots-not-set-up. No stack. Needs Playwright (set PLAYWRIGHT_CORE to a
# playwright-core directory). Not in `ci`.
test-not-set-up:
    node {{root}}/tests/not-set-up/shoot.mjs

# Every Rust dependency's licence is one an AGPL-3.0-only work can carry (deny.toml)
licences:
    cargo deny check licenses

# Every file says its copyright and licence, in its header or in REUSE.toml (the REUSE
# specification; the tool pinned, run through uv)
reuse-check:
    uvx reuse@6.3.0 lint

# Write the API's OpenAPI description and the documentation's reference page from the handlers (ADR-0195)
openapi:
    mkdir -p {{root}}/docs/openapi
    cargo run -q -p weavesignal-control -- openapi > {{root}}/docs/openapi/weavesignal-v1.json
    cargo run -q -p weavesignal-control -- openapi --markdown > {{root}}/docs/src/content/docs/docs/reference/api.md

# Fail when the committed description or reference page differs from what the handlers generate
openapi-check:
    mkdir -p {{root}}/target/openapi-check
    cargo run -q -p weavesignal-control -- openapi > {{root}}/target/openapi-check/weavesignal-v1.json
    cargo run -q -p weavesignal-control -- openapi --markdown > {{root}}/target/openapi-check/api.md
    diff -u {{root}}/docs/openapi/weavesignal-v1.json {{root}}/target/openapi-check/weavesignal-v1.json || { echo "docs/openapi/weavesignal-v1.json is out of date: run 'just openapi' and commit it"; exit 1; }
    diff -u {{root}}/docs/src/content/docs/docs/reference/api.md {{root}}/target/openapi-check/api.md || { echo "docs/src/content/docs/docs/reference/api.md is out of date: run 'just openapi' and commit it"; exit 1; }
    @echo "the API description is current"

# Validate the Compose bundle (with its two local edges, edges.yaml, the public route, the phone line, phone.yaml, and a GPU for the desk, gpu.yaml, handed over or only found, 1.2.1) against the example settings, its secrets filled with throwaways; one server image for every service, and PULL_ALLOW_PRIVATE passed to the control plane
compose-check:
    cd {{root}}/deploy/compose && LIVEKIT_API_SECRET=check-only-check-only-check-only-0 CHAT_EMAIL_KEY=check-only-check-only-check-only-0 LISTENERS_SOURCE_PASSWORD=check-only POSTGRES_PASSWORD=check-only podman compose --env-file .env.example config --quiet
    cd {{root}}/deploy/compose && LIVEKIT_API_SECRET=check-only-check-only-check-only-0 CHAT_EMAIL_KEY=check-only-check-only-check-only-0 LISTENERS_SOURCE_PASSWORD=check-only POSTGRES_PASSWORD=check-only podman compose --env-file .env.example -f compose.yaml -f edges.yaml config --quiet
    cd {{root}}/deploy/compose && PUBLIC_DOMAIN=station.example LIVEKIT_API_SECRET=check-only-check-only-check-only-0 CHAT_EMAIL_KEY=check-only-check-only-check-only-0 LISTENERS_SOURCE_PASSWORD=check-only POSTGRES_PASSWORD=check-only podman compose --env-file .env.example -f compose.yaml -f public.yaml config --quiet
    cd {{root}}/deploy/compose && PUBLIC_DOMAIN=station.example LIVEKIT_API_SECRET=check-only-check-only-check-only-0 CHAT_EMAIL_KEY=check-only-check-only-check-only-0 LISTENERS_SOURCE_PASSWORD=check-only POSTGRES_PASSWORD=check-only podman compose --env-file .env.example -f compose.yaml -f public.yaml -f edges.yaml config --quiet
    cd {{root}}/deploy/compose && PHONE=true PHONE_REDIS_PASSWORD=check-only LIVEKIT_API_SECRET=check-only-check-only-check-only-0 CHAT_EMAIL_KEY=check-only-check-only-check-only-0 LISTENERS_SOURCE_PASSWORD=check-only POSTGRES_PASSWORD=check-only podman compose --env-file .env.example -f compose.yaml -f phone.yaml config --quiet
    cd {{root}}/deploy/compose && PUBLIC_DOMAIN=station.example PHONE=true PHONE_REDIS_PASSWORD=check-only LIVEKIT_API_SECRET=check-only-check-only-check-only-0 CHAT_EMAIL_KEY=check-only-check-only-check-only-0 LISTENERS_SOURCE_PASSWORD=check-only POSTGRES_PASSWORD=check-only podman compose --env-file .env.example -f compose.yaml -f public.yaml -f phone.yaml config --quiet
    cd {{root}}/deploy/compose && REHEARSAL=true LIVEKIT_API_SECRET=check-only-check-only-check-only-0 CHAT_EMAIL_KEY=check-only-check-only-check-only-0 LISTENERS_SOURCE_PASSWORD=check-only POSTGRES_PASSWORD=check-only podman compose --env-file .env.example -f compose.yaml -f rehearsal.yaml config --quiet
    {{root}}/tests/install/rehearsal-compose.sh
    cd {{root}}/deploy/compose && KIT_VBAN=true PUBLIC_ORIGIN=http://192.0.2.10:7310 LIVEKIT_API_SECRET=check-only-check-only-check-only-0 CHAT_EMAIL_KEY=check-only-check-only-check-only-0 LISTENERS_SOURCE_PASSWORD=check-only POSTGRES_PASSWORD=check-only podman compose --env-file .env.example config --format json | python3 -I -c 'import json,sys,tomllib; c=tomllib.loads(json.load(sys.stdin)["configs"]["control"]["content"]); assert c["kit"]["vban"] is True and c["publishing"]["public_url"] == "http://192.0.2.10:7310", (c["kit"], c["publishing"]); print("compose-check: KIT_VBAN and PUBLIC_ORIGIN reach the control plane")'
    cd {{root}}/deploy/compose && VIDEO=true GPU=nvidia LIVEKIT_API_SECRET=check-only-check-only-check-only-0 CHAT_EMAIL_KEY=check-only-check-only-check-only-0 LISTENERS_SOURCE_PASSWORD=check-only POSTGRES_PASSWORD=check-only podman compose --env-file .env.example -f compose.yaml -f gpu.yaml config --quiet
    cd {{root}}/deploy/compose && PUBLIC_DOMAIN=station.example VIDEO=true GPU=nvidia GPU_DEVICE=nvidia.com/gpu=0 LIVEKIT_API_SECRET=check-only-check-only-check-only-0 CHAT_EMAIL_KEY=check-only-check-only-check-only-0 LISTENERS_SOURCE_PASSWORD=check-only POSTGRES_PASSWORD=check-only podman compose --env-file .env.example -f compose.yaml -f public.yaml -f gpu.yaml config --format json | python3 -I -c 'import json,sys; d=json.load(sys.stdin)["services"]["desk"]; assert [x["source"] for x in d["devices"]] == ["nvidia.com/gpu=0"] and "label=disable" in d["security_opt"], d; print("compose-check: gpu.yaml hands the desk nvidia.com/gpu=0")'
    cd {{root}}/deploy/compose && VIDEO=true GPU=none GPU_FOUND=nvidia LIVEKIT_API_SECRET=check-only-check-only-check-only-0 CHAT_EMAIL_KEY=check-only-check-only-check-only-0 LISTENERS_SOURCE_PASSWORD=check-only POSTGRES_PASSWORD=check-only podman compose --env-file .env.example -f compose.yaml config --format json | python3 -I -c 'import json,sys,tomllib; c=json.load(sys.stdin); s=tomllib.loads(c["configs"]["control"]["content"])["setup"]; assert (s["gpu_found"], s["gpu"], s["video"]) == ("nvidia", "none", True), s; assert not c["services"]["desk"].get("devices"), c["services"]["desk"]; print("compose-check: a card found and not asked for is told to the control plane and not handed to the desk")'
    # Every service that runs the server image falls back to the same one (the ingest's said 1.0.4 until 1.2.1)
    test "$(grep -o 'SERVER_IMAGE:-[^}]*' {{root}}/deploy/compose/compose.yaml | sort -u | wc -l)" = 1 || { grep -n 'SERVER_IMAGE:-' {{root}}/deploy/compose/compose.yaml; echo 'compose-check: the services fall back to different server images'; exit 1; }
    cd {{root}}/deploy/compose && PULL_ALLOW_PRIVATE=true LIVEKIT_API_SECRET=check-only-check-only-check-only-0 CHAT_EMAIL_KEY=check-only-check-only-check-only-0 LISTENERS_SOURCE_PASSWORD=check-only POSTGRES_PASSWORD=check-only podman compose --env-file .env.example config --format json | python3 -I -c 'import json,sys; e=json.load(sys.stdin)["services"]["control"]["environment"]; assert e.get("WEAVESIGNAL_PULL_ALLOW_PRIVATE") == "true", e.get("WEAVESIGNAL_PULL_ALLOW_PRIVATE"); print("compose-check: PULL_ALLOW_PRIVATE reaches the control plane")'

# The server image the Compose bundle runs (the desk, the control plane and the chat)
server-image:
    podman build --build-arg BUILD={{desk_image}} -t {{server_image}} -f {{root}}/deploy/images/server/Containerfile {{root}}

# Measured in M10 on a 32-core amd64 workstation: 41, 2 and 48 minutes. A native arm64 builder is the release route.
# The server image for linux/arm64 under QEMU emulation, its two base images first
server-image-arm64:
    podman build --platform linux/arm64 -t localhost/weavesignal-desk-spike:arm64 {{root}}/spike/desk-image
    podman build --platform linux/arm64 --build-arg BASE=localhost/weavesignal-desk-spike:arm64 -t localhost/weavesignal-desk-dev:arm64 -f {{root}}/deploy/images/desk/Containerfile {{root}}/deploy/images/desk
    podman build --platform linux/arm64 --build-arg BUILD=localhost/weavesignal-desk-dev:arm64 --build-arg RUNTIME=localhost/weavesignal-desk-spike:arm64 -t {{server_image}}-arm64 -f {{root}}/deploy/images/server/Containerfile {{root}}

# The Compose bundle up on test ports, then the port scan: only LiveKit's media port off loopback.
# The image is rebuilt first, so the bundle never runs a stale one.
test-bundle: server-image
    {{root}}/tests/ports/bundle.sh

# The phone acceptance run's words check alone, no station: the words as a caller heard them match,
# a ringback, silence and another voice do not (needs mojo and ffmpeg)
phone-words-check:
    {{root}}/tests/phone/acceptance/words-check.sh

# The phone line end to end on a real Compose station (1.2.1): the station makes its own SIP trunk
# and dispatch rule, baresip rings it from the key to the air, and both survive restarts
test-phone: server-image
    WEAVESIGNAL_SERVER_IMAGE={{server_image}} {{root}}/tests/phone/acceptance/stack.sh

# No outside STUN by default (ADR-0290), proved from what leaves the station (tests/no-stun/README.md).
# About 150 s. In `ci`: the promise is for every station, and the test reuses the image test-bundle builds.
test-no-stun: server-image
    WEAVESIGNAL_SERVER_IMAGE={{server_image}} {{root}}/tests/no-stun/run.sh

# Pulled sources (1.2) from a local Icecast and an SRT listener, end to end (tests/pull/README.md).
# Passed on 2026-11-21 after the caller announces MSS 1500. Not in `ci`; the 1.2.1 gate queue runs it.
test-pull: server-image relay-image
    WEAVESIGNAL_SERVER_IMAGE={{server_image}} RELAY_IMAGE={{relay_image}} {{root}}/tests/pull/run.sh

# weavesignal.sh end to end on its own project and ports (29xxx): a taken port refused with its
# alternative, install, a second run that changes nothing, uninstall keeping the data, purge
test-install: server-image
    {{root}}/tests/install/weavesignal-sh.sh

# weavesignal.sh's start at boot on Podman (ADR-0138) with loginctl, systemctl and the engines stubbed:
# both turned on and said, left alone with the flag, Docker unaffected; no system state changes
boot-check:
    {{root}}/tests/install/start-at-boot.sh

# The public route (public.yaml, weavesignal.sh --public; ADR-0022) with no public certificate:
# weavesignal.sh's settings and refusals with the engines stubbed, the rendered bundle, then the
# Caddyfile run against stand-in backends with Caddy's own CA, and LiveKit loading its certificate.
# Needs Podman and the Caddy and LiveKit images; WEAVESIGNAL_PUBLIC_TEST_PORT sets the port base.
public-check:
    {{root}}/tests/install/public-check.sh

# The listen-only route for a home network (lan.yaml, weavesignal.sh --lan; 1.3.1) and the installer's
# provider and socket checks: the script with the engines and this machine's network cards stubbed,
# the rendered bundle, then the Caddyfile run for real on 127.0.0.2 against stand-in backends.
# Needs Podman and the Caddy image; WEAVESIGNAL_LAN_TEST_PORT sets the port base.
lan-check:
    {{root}}/tests/install/lan-check.sh

# The pinned tools the Tauri bundler downloads (deploy/desktop/tools.lock): checked, kept, and a
# wrong checksum refused, with curl stubbed (no network)
tools-check:
    {{root}}/tests/install/fetch-tools.sh

# The Podman Quadlets generated from the Compose bundle, converted by Podman's own generator
# (a dry run: nothing is installed into systemd or started)
quadlet-check:
    {{root}}/tests/install/quadlets.sh

# The studio app: Svelte checks and tests, the interface build, then the Tauri crate built
# and linted (needs WebKitGTK 4.1's development files; pnpm 10 first on PATH)
studio-check:
    cd {{root}}/apps/studio && pnpm install --frozen-lockfile && pnpm check && pnpm test && pnpm build
    cargo clippy -p weavesignal-studio --all-targets -- -D warnings

# The studio app's Linux build image: Ubuntu 24.04, WebKitGTK 4.1, the pinned Rust and Tauri CLI
desktop-image:
    podman build -t {{desktop_image}} {{root}}/deploy/desktop

# Built in the desktop image, pnpm 10 first on PATH; the updater is on only when
# WEAVESIGNAL_UPDATER_PUBKEY is set (deploy/desktop/build.sh says how).
# The studio app's AppImage, .deb and .rpm (ADR-0015) in dist/desktop/ with their checksums
package-desktop:
    WEAVESIGNAL_DESKTOP_IMAGE={{desktop_image}} {{root}}/deploy/desktop/build.sh

# The Windows cross-build image: Debian 13, MinGW-w64, NSIS 3.11, the pinned Rust and Tauri CLI
desktop-windows-image:
    podman build -t {{desktop_windows_image}} {{root}}/deploy/desktop/windows

# Cross-built on Linux for the GNU target, unsigned (ADR-0076); pnpm 10 first on PATH.
# The studio app's Windows installer (NSIS) in dist/desktop/ with its checksum
package-windows:
    WEAVESIGNAL_DESKTOP_WINDOWS_IMAGE={{desktop_windows_image}} {{root}}/deploy/desktop/build.sh windows

# Takes the signed packages in dist/desktop/ (both platforms' builds) and writes the feed's tree
# to dist/updates/: latest.json and a folder per release. Pass the options through, at least
# --base-url (deploy/desktop/latest-json.sh says what it refuses). Needs no key.
# The studio app's update feed (ADR-0140) from the signed packages
package-feed *args:
    {{root}}/deploy/desktop/latest-json.sh {{args}}

# A throwaway key under target/ signs stand-in packages; the feed is built, served on loopback
# (WEAVESIGNAL_FEED_TEST_PORT, default 23010) and its signatures verified with the updater's own
# crate. Builds no package and starts no container; pnpm 10 first on PATH.
# The update feed's script and signatures, end to end
feed-test:
    {{root}}/deploy/desktop/feed-test.sh

# The installed app's `--version` is run, and the packages removed again; no display is started.
# Each desktop package installed in a throwaway container (Debian, Ubuntu, Fedora)
package-desktop-test:
    {{root}}/deploy/desktop/install-test.sh

# A clean run, a failed parallel encode, an Xid during an encode and one the follower missed.
# tests/gpu/measure.sh's control flow with every command stubbed: no card, no container
gpu-measure-check:
    {{root}}/tests/gpu/measure-check.sh

# Every rung, several at once, SVT-AV1 beside it, the kernel log watched. Needs the card free; not in `ci`.
# The desk's AV1 encode measured on this machine's NVIDIA card, stopped at the first Xid (17e)
gpu-measure *args:
    {{root}}/tests/gpu/measure.sh {{args}}

# The install wizards' shell parts (ADR-0237), with no container: the generator's Compose file
# through the real weavesignal.sh (engines stubbed) and `compose config`, and the Cloudron start
# script's setup.env reader and nginx routes. Needs Node 22.18 or later. The generator's own
# tests run in `web-check`, the setup routes' in `test-db`; the browser check is
# tests/wizard/check.mjs against tests/wizard/run-station.sh.
wizard-check:
    {{root}}/tests/wizard/compose-env.sh
    {{root}}/tests/wizard/start-sh.sh

# The public pages (player, call-in, join, echo test): Svelte checks, tests and the build
# (pnpm 10 first on PATH)
web-check:
    cd {{root}}/apps/web && pnpm install --frozen-lockfile && pnpm check && pnpm test && pnpm build

# Build the website (the landing site and the documentation; pnpm 10 must be first on PATH),
# then check every internal link, image and source in the build, the redirect pages included
docs-build:
    cd {{root}}/docs && pnpm install --frozen-lockfile && ASTRO_TELEMETRY_DISABLED=1 pnpm build
    node {{root}}/tests/docs-links/check.mjs {{root}}/docs/dist

# The link check alone, over an existing build
docs-links:
    node {{root}}/tests/docs-links/check.mjs {{root}}/docs/dist

# The website's "Try the desk" (ADR-0212): the studio app built with the station simulated in
# the page, for the root of demo.weavesignal.top, into apps/studio/dist-sandbox; its sounds
# (/media/sandbox/: the voices, the carts, the default loop) are copied in at deploy, git-ignored.
# The sandbox desk's build; tests/sandbox/drive.mjs runs its show in a browser
sandbox-build:
    cd {{root}}/apps/studio && pnpm install --frozen-lockfile && pnpm test && pnpm build:sandbox

# The model's migrations, the control plane's cart, retention, show, card and feed routes (M8),
# the public API's tokens, webhooks and open routes (ADR-0195), show keys and the ingest's routes
# (ADR-0280), and the chat (M6: sign-in, the rules, the hold, moderation with statements of reasons, the
# studio lane, recovery, erasure, the retention sweep, over real sockets) against a throwaway
# PostgreSQL 18 on loopback (port 17432, or WEAVESIGNAL_TEST_PG_PORT)
test-db:
    podman rm -f {{test_prefix}}-pg >/dev/null 2>&1 || true
    podman run -d --rm --name {{test_prefix}}-pg -p 127.0.0.1:{{test_pg_port}}:5432 -e POSTGRES_PASSWORD=test-only -e POSTGRES_DB=weavesignal docker.io/library/postgres:18 >/dev/null
    for i in $(seq 1 60); do podman exec {{test_prefix}}-pg pg_isready -h 127.0.0.1 -U postgres -d weavesignal >/dev/null 2>&1 && break; sleep 0.5; done
    status=0; WEAVESIGNAL_TEST_DATABASE_URL=postgres://postgres:test-only@127.0.0.1:{{test_pg_port}}/weavesignal cargo test -p weavesignal-model -- --ignored && WEAVESIGNAL_TEST_DATABASE_URL=postgres://postgres:test-only@127.0.0.1:{{test_pg_port}}/weavesignal cargo test -p weavesignal-control --test carts_db -- --ignored && WEAVESIGNAL_TEST_DATABASE_URL=postgres://postgres:test-only@127.0.0.1:{{test_pg_port}}/weavesignal cargo test -p weavesignal-control --test retention_db -- --ignored && WEAVESIGNAL_TEST_DATABASE_URL=postgres://postgres:test-only@127.0.0.1:{{test_pg_port}}/weavesignal cargo test -p weavesignal-control --test shows_db -- --ignored && WEAVESIGNAL_TEST_DATABASE_URL=postgres://postgres:test-only@127.0.0.1:{{test_pg_port}}/weavesignal cargo test -p weavesignal-control --test consent_db -- --ignored && WEAVESIGNAL_TEST_DATABASE_URL=postgres://postgres:test-only@127.0.0.1:{{test_pg_port}}/weavesignal cargo test -p weavesignal-control --test rtmp_db -- --ignored && WEAVESIGNAL_TEST_DATABASE_URL=postgres://postgres:test-only@127.0.0.1:{{test_pg_port}}/weavesignal cargo test -p weavesignal-control --test schedule_db -- --ignored && WEAVESIGNAL_TEST_DATABASE_URL=postgres://postgres:test-only@127.0.0.1:{{test_pg_port}}/weavesignal cargo test -p weavesignal-control --test staff_db -- --ignored && WEAVESIGNAL_TEST_DATABASE_URL=postgres://postgres:test-only@127.0.0.1:{{test_pg_port}}/weavesignal cargo test -p weavesignal-control --test tokens_db -- --ignored && WEAVESIGNAL_TEST_DATABASE_URL=postgres://postgres:test-only@127.0.0.1:{{test_pg_port}}/weavesignal cargo test -p weavesignal-control --test setup_db -- --ignored && WEAVESIGNAL_TEST_DATABASE_URL=postgres://postgres:test-only@127.0.0.1:{{test_pg_port}}/weavesignal cargo test -p weavesignal-control --test steps_db -- --ignored && WEAVESIGNAL_TEST_DATABASE_URL=postgres://postgres:test-only@127.0.0.1:{{test_pg_port}}/weavesignal cargo test -p weavesignal-control --test sources_db -- --ignored && WEAVESIGNAL_TEST_DATABASE_URL=postgres://postgres:test-only@127.0.0.1:{{test_pg_port}}/weavesignal cargo test -p weavesignal-control --test monitor_db -- --ignored && WEAVESIGNAL_TEST_DATABASE_URL=postgres://postgres:test-only@127.0.0.1:{{test_pg_port}}/weavesignal cargo test -p weavesignal-control --test monitor_api_db -- --ignored && WEAVESIGNAL_TEST_DATABASE_URL=postgres://postgres:test-only@127.0.0.1:{{test_pg_port}}/weavesignal cargo test -p weavesignal-control --test rehearsal_db -- --ignored && WEAVESIGNAL_TEST_DATABASE_URL=postgres://postgres:test-only@127.0.0.1:{{test_pg_port}}/weavesignal cargo test -p weavesignal-control --test rehearsal_columns_db -- --ignored && WEAVESIGNAL_TEST_DATABASE_URL=postgres://postgres:test-only@127.0.0.1:{{test_pg_port}}/weavesignal cargo test -p weavesignal-control --test keys_db -- --ignored && WEAVESIGNAL_TEST_DATABASE_URL=postgres://postgres:test-only@127.0.0.1:{{test_pg_port}}/weavesignal cargo test -p weavesignal-chat --test chat_db -- --ignored || status=$?; podman rm -f {{test_prefix}}-pg >/dev/null; exit $status

# Every gate, in order, on this machine (there is no hosted CI): host crates, licences, the
# desk in its image, the Liquidsoap scripts, the migrations, the documentation, the stack
# tests (room, playout restart, carts, edges), the studio app, the public pages, the Compose bundle with its port scan, the installer and the Quadlets.
# Needs Podman, cargo-deny, pnpm 10, WebKitGTK 4.1, ffmpeg, tcpdump, jq, and the images (`just desk-image server-image`).
# The desk image first, under this worktree's tag (ADR-0243): a gate that ran a stale image
# (the default :m5, older than RTMP out) failed desk-check, or passed what it should not.
ci: desk-image check openapi-check licences reuse-check desk-check liq-check test-db docs-build studio-check feed-test web-check wizard-check compose-check boot-check public-check lan-check tools-check quadlet-check test-room test-rehearsal test-contain test-churn test-empty test-quiet test-dump test-sources-probe test-playout test-feed test-onair test-keyed test-keyed-switch test-icecast-out test-carts test-edges test-relay-tls test-bundle test-no-stun test-install

# --- rtmp-out (ADR-0230) -------------------------------------------------------------------

# RTMP out against real servers on loopback (24xxx), never a real platform: MediaMTX (both
# profiles read back, a refused key, the server going away, a dump), OvenMediaEngine and PeerTube
# (both profiles; PEERTUBE=0 skips it). Not in `ci`: it pulls three servers' images.
test-rtmp:
    WEAVESIGNAL_DESK_IMAGE={{desk_image}} {{root}}/tests/rtmp/run.sh

# RTMP out's extra encode, measured: the desk's CPU with each profile sending, at the 480p20 and
# 720p30 canvases (a release build; a measurement, not in `ci`)
test-rtmp-cost:
    {{in_desk}} env RUST_LOG=error cargo test --release -p weavesignal-desk --test rtmp_stack -- --ignored --nocapture rtmp_cost
