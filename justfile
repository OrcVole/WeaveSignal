# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
# WeaveSignal tasks. `just --list` shows them all.

set shell := ["bash", "-euo", "pipefail", "-c"]

root := justfile_directory()
# The desk's build image; settable, so a worktree that changes it does not retag another's.
desk_image := env("WEAVESIGNAL_DESK_IMAGE", "localhost/weavesignal-desk-dev:m5")
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

# The profanity delay dumped: the listener never hears the audio it held
test-dump:
    TEST=dump {{root}}/tests/room/run-stack.sh

# The picture's quality at each rung: the desk's encoders and CMAF writer, decoded to raw frames,
# scored with VMAF (needs Xiph's clips in target/clips; WEAVESIGNAL_DESK_GPU=1 adds NVENC)
test-vmaf:
    {{in_desk}} env WEAVESIGNAL_CLIPS=/work/target/clips cargo test --release -p weavesignal-desk --test vmaf -- --ignored --nocapture

# The desk's CPU with 4, 8 and 13 people, a release build (a measurement, not in `ci`)
test-load:
    TEST=load {{root}}/tests/room/run-stack.sh

# Playout, then the desk, killed and restarted under a listener: the programme comes back
test-playout:
    TEST=playout {{root}}/tests/room/run-stack.sh

# The video's sound from the real playout, both real ends (ADR-0172; review 0028, A1)
test-feed:
    TEST=feed {{root}}/tests/room/run-stack.sh

# The chat with 4,000 sockets, 40 that never read and 200 that reconnect, at 7, 20 and 60
# messages a second (a release build on ports 37311 and 37432 by default; a measurement, not in `ci`)
test-chat-load:
    {{root}}/tests/chat/load.sh

# The playout layer (M4) on the stack: dead air to automation and back, playout killed under a
# listener who stays connected (the fallback, then the desk), the per-play log, the audience
# counts, now playing with and without up-next, the logger following retention
test-onair:
    {{root}}/tests/playout/run-playout.sh

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

# Validate the Compose bundle (and with its two local edges, edges.yaml) against the example settings, its secrets filled with throwaways
compose-check:
    cd {{root}}/deploy/compose && LIVEKIT_API_SECRET=check-only-check-only-check-only-0 CHAT_EMAIL_KEY=check-only-check-only-check-only-0 LISTENERS_SOURCE_PASSWORD=check-only POSTGRES_PASSWORD=check-only podman compose --env-file .env.example config --quiet
    cd {{root}}/deploy/compose && LIVEKIT_API_SECRET=check-only-check-only-check-only-0 CHAT_EMAIL_KEY=check-only-check-only-check-only-0 LISTENERS_SOURCE_PASSWORD=check-only POSTGRES_PASSWORD=check-only podman compose --env-file .env.example -f compose.yaml -f edges.yaml config --quiet
    cd {{root}}/deploy/compose && PUBLIC_DOMAIN=station.example LIVEKIT_API_SECRET=check-only-check-only-check-only-0 CHAT_EMAIL_KEY=check-only-check-only-check-only-0 LISTENERS_SOURCE_PASSWORD=check-only POSTGRES_PASSWORD=check-only podman compose --env-file .env.example -f compose.yaml -f public.yaml config --quiet
    cd {{root}}/deploy/compose && PUBLIC_DOMAIN=station.example LIVEKIT_API_SECRET=check-only-check-only-check-only-0 CHAT_EMAIL_KEY=check-only-check-only-check-only-0 LISTENERS_SOURCE_PASSWORD=check-only POSTGRES_PASSWORD=check-only podman compose --env-file .env.example -f compose.yaml -f public.yaml -f edges.yaml config --quiet

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
# the public API's tokens, webhooks and open routes (ADR-0195), and the chat (M6: sign-in, the rules, the hold, moderation with statements of reasons, the
# studio lane, recovery, erasure, the retention sweep, over real sockets) against a throwaway
# PostgreSQL 18 on loopback (port 17432, or WEAVESIGNAL_TEST_PG_PORT)
test-db:
    podman rm -f {{test_prefix}}-pg >/dev/null 2>&1 || true
    podman run -d --rm --name {{test_prefix}}-pg -p 127.0.0.1:{{test_pg_port}}:5432 -e POSTGRES_PASSWORD=test-only -e POSTGRES_DB=weavesignal docker.io/library/postgres:18 >/dev/null
    for i in $(seq 1 60); do podman exec {{test_prefix}}-pg pg_isready -h 127.0.0.1 -U postgres -d weavesignal >/dev/null 2>&1 && break; sleep 0.5; done
    status=0; WEAVESIGNAL_TEST_DATABASE_URL=postgres://postgres:test-only@127.0.0.1:{{test_pg_port}}/weavesignal cargo test -p weavesignal-model -- --ignored && WEAVESIGNAL_TEST_DATABASE_URL=postgres://postgres:test-only@127.0.0.1:{{test_pg_port}}/weavesignal cargo test -p weavesignal-control --test carts_db -- --ignored && WEAVESIGNAL_TEST_DATABASE_URL=postgres://postgres:test-only@127.0.0.1:{{test_pg_port}}/weavesignal cargo test -p weavesignal-control --test retention_db -- --ignored && WEAVESIGNAL_TEST_DATABASE_URL=postgres://postgres:test-only@127.0.0.1:{{test_pg_port}}/weavesignal cargo test -p weavesignal-control --test shows_db -- --ignored && WEAVESIGNAL_TEST_DATABASE_URL=postgres://postgres:test-only@127.0.0.1:{{test_pg_port}}/weavesignal cargo test -p weavesignal-control --test schedule_db -- --ignored && WEAVESIGNAL_TEST_DATABASE_URL=postgres://postgres:test-only@127.0.0.1:{{test_pg_port}}/weavesignal cargo test -p weavesignal-control --test staff_db -- --ignored && WEAVESIGNAL_TEST_DATABASE_URL=postgres://postgres:test-only@127.0.0.1:{{test_pg_port}}/weavesignal cargo test -p weavesignal-control --test tokens_db -- --ignored && WEAVESIGNAL_TEST_DATABASE_URL=postgres://postgres:test-only@127.0.0.1:{{test_pg_port}}/weavesignal cargo test -p weavesignal-chat --test chat_db -- --ignored || status=$?; podman rm -f {{test_prefix}}-pg >/dev/null; exit $status

# Every gate, in order, on this machine (there is no hosted CI): host crates, licences, the
# desk in its image, the Liquidsoap scripts, the migrations, the documentation, the stack
# tests (room, playout restart, carts, edges), the studio app, the public pages, the Compose bundle with its port scan, the installer and the Quadlets.
# Needs Podman, cargo-deny, pnpm 10, WebKitGTK 4.1, ffmpeg, and the images (`just desk-image server-image`).
ci: check openapi-check licences reuse-check desk-check liq-check test-db docs-build studio-check feed-test web-check compose-check boot-check public-check tools-check quadlet-check test-room test-contain test-churn test-empty test-dump test-playout test-feed test-onair test-carts test-edges test-relay-tls test-bundle test-install
