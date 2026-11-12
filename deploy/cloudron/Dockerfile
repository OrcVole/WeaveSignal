# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# WeaveSignal's release-one server as one Cloudron app (ADR-0022): the control plane, the chat,
# the desk, Liquidsoap 2.5 playout, the listener process with icecast.server, LiveKit with its
# own TURN, and the public pages, supervised inside the container, with Cloudron's PostgreSQL
# addon (ADR-0082). amd64 only for now.
#
# Build from the repository's root (the context needs crates/ and playout/):
#   podman build -t <image>:<tag> -f deploy/cloudron/Dockerfile .
#
# Every stage is Ubuntu 26.04 (cloudron/base 6.0.1), so what the build stage links is what
# the final stage runs: GStreamer 1.28.2 from Ubuntu, gst-plugins-rs's webrtc plugin (LiveKit)
# and, for the video path (ADR-0227), its isobmff, hlssink3 and dav1d plugins, all built
# against it at one tag, and libsrt 1.5.7 built from source in place of Ubuntu's 1.5.4
# (ADR-0048: libsrt 1.5.7 or later in every image). SVT-AV1 (svtav1enc) comes from Ubuntu's
# gst-plugins-bad.

ARG BASE=docker.io/cloudron/base:6.0.1@sha256:7b1e729960c3ca312d26e886917ba89483241bba0219ff292858e10d9d5768ee
ARG LIVEKIT=docker.io/livekit/livekit-server:v1.13.10@sha256:89ca9ce0791e18d6e03bf6e2c49ceb27e1c754eae8bd50c989386c1a5963791a
ARG NODE=docker.io/library/node:24.22.1-bookworm-slim@sha256:2dd9a4f758e173b65fee15388f7706af6fb01ef50cf7a2c0538711d67a6981ae

########################################################################
FROM ${LIVEKIT} AS livekit

########################################################################
# The station's public pages (apps/web, M3 and M6): the player with call-in and the chat, the
# join page, the echo test. Static files; nginx serves them. They import the shared tokens
# and chat shapes from packages/ui and two components from the studio app. The Node image is
# pinned by digest and the pages' dependencies by their lockfile; pnpm itself by version.
FROM ${NODE} AS web
ARG PNPM_VERSION=10.30.2
RUN npm install -g pnpm@${PNPM_VERSION}
WORKDIR /src
COPY packages/ui packages/ui
# The station's look and setup page take the accent and the shared questions (ADR-0237).
COPY packages/wizard packages/wizard
COPY apps/studio/src/lib apps/studio/src/lib
COPY apps/web apps/web
RUN rm -rf apps/web/node_modules apps/web/dist \
    && cd apps/web && pnpm install --frozen-lockfile && pnpm build \
    && test -f dist/index.html && test -f dist/join/index.html

########################################################################
FROM ${BASE} AS build
ARG RUST_VERSION=1.98.1
ARG RUSTUP_VERSION=1.28.2
ARG RUSTUP_SHA256=20a06e644b0d9bd2fbdbfd52d42540bdde820ea7df86e92e533c073da0cdd43c
ARG SRT_VERSION=1.5.7
ARG SRT_SHA256=017cd1e437ef2073a4dd10ddf7b55e86bc3d6ebac0393d13bd22f6a57055d32b
ARG GST_RS_TAG=0.15.4+fixup
ARG GST_RS_COMMIT=c164a6696fcb0408f72a108e5f35ec25eb047464
ARG CARGO_C_VERSION=0.10.16

ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        build-essential pkg-config cmake git curl ca-certificates tcl \
        libssl-dev libgnutls28-dev nettle-dev libgmp-dev \
        libglib2.0-dev libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev \
        libgstreamer-plugins-bad1.0-dev libdav1d-dev \
    && rm -rf /var/lib/apt/lists/*

# Rust, pinned: the workspace needs 1.98 and Ubuntu 26.04 carries 1.93.
ENV RUSTUP_HOME=/opt/rustup CARGO_HOME=/opt/cargo PATH=/opt/cargo/bin:$PATH
RUN curl -fsSL -o /tmp/rustup-init \
        https://static.rust-lang.org/rustup/archive/${RUSTUP_VERSION}/x86_64-unknown-linux-gnu/rustup-init \
    && echo "${RUSTUP_SHA256}  /tmp/rustup-init" | sha256sum -c - \
    && chmod +x /tmp/rustup-init \
    && /tmp/rustup-init -y --no-modify-path --profile minimal --default-toolchain ${RUST_VERSION} \
    && rm /tmp/rustup-init

# libsrt 1.5.7, built as Ubuntu builds it (GnuTLS, the library named srt-gnutls) so it drops in
# for Ubuntu's 1.5.4 under the same soname; GStreamer's srt plugin then loads it. Liquidsoap's
# .deb carries its own libsrt 1.5.7 (checked with `strings`), so playout needs nothing here.
RUN curl -fsSL -o /tmp/srt.tar.gz https://github.com/Haivision/srt/archive/refs/tags/v${SRT_VERSION}.tar.gz \
    && echo "${SRT_SHA256}  /tmp/srt.tar.gz" | sha256sum -c - \
    && mkdir /src && tar -C /src -xzf /tmp/srt.tar.gz && rm /tmp/srt.tar.gz \
    && cmake -S /src/srt-${SRT_VERSION} -B /src/srt-build -DCMAKE_BUILD_TYPE=Release \
         -DCMAKE_INSTALL_PREFIX=/usr -DCMAKE_INSTALL_LIBDIR=lib/x86_64-linux-gnu \
         -DUSE_ENCLIB=gnutls -DTARGET_srt=srt-gnutls \
         -DENABLE_APPS=OFF -DENABLE_STATIC=OFF -DENABLE_SHARED=ON -DENABLE_UNITTESTS=OFF \
    && cmake --build /src/srt-build -j"$(nproc)" \
    && install -D -m 0644 /src/srt-build/libsrt-gnutls.so.${SRT_VERSION} \
         /out/srt/libsrt-gnutls.so.${SRT_VERSION} \
    && strings /out/srt/libsrt-gnutls.so.${SRT_VERSION} | grep -qx "${SRT_VERSION}"

# gst-plugins-rs's webrtc plugin with LiveKit (livekitwebrtcsrc and livekitwebrtcsink), at the
# tag the M1 desk was tested with, with WeaveSignal's patches, built against Ubuntu's GStreamer 1.28.2.
RUN --mount=type=cache,target=/opt/cargo/registry \
    cargo install --locked cargo-c --version ${CARGO_C_VERSION}
# WeaveSignal's patches (spike/desk-image/patches): the signaller's `allowed-subscribers`, so each
# mix-minus reaches only its own person (ADR-0118). The desk refuses to start without it.
COPY spike/desk-image/patches/ /src/patches/
RUN git clone --depth 1 --branch "${GST_RS_TAG}" \
        https://gitlab.freedesktop.org/gstreamer/gst-plugins-rs.git /src/gst-plugins-rs \
    && test "$(git -C /src/gst-plugins-rs rev-parse HEAD)" = "${GST_RS_COMMIT}" \
    && git -C /src/gst-plugins-rs apply --verbose /src/patches/*.patch
WORKDIR /src/gst-plugins-rs
RUN --mount=type=cache,target=/opt/cargo/registry \
    --mount=type=cache,target=/src/gst-plugins-rs/target \
    cargo cinstall --locked -p gst-plugin-webrtc --features livekit --release \
        --prefix /usr --libdir /usr/lib/x86_64-linux-gnu --destdir /stage \
    && install -D -m 0644 /stage/usr/lib/x86_64-linux-gnu/gstreamer-1.0/libgstrswebrtc.so \
         /out/gst/libgstrswebrtc.so

# The video path's plugins from the same tree, so the same tag as the webrtc plugin (ADR-0227):
# isobmff (`cmafmux`), hlssink3 (`hlscmafsink`, the CMAF HLS writer), dav1d (`dav1ddec`,
# the guests' AV1 cameras) and rtp (`rtpav1depay`: those cameras out of RTP; without it a
# Chromium camera failed at the desk, 2026-11-11). They are installed whether or not a station turns video on
# (`VIDEO` in weavesignal.env); a separate step keeps the webrtc plugin's layer cached.
RUN --mount=type=cache,target=/opt/cargo/registry \
    --mount=type=cache,target=/src/gst-plugins-rs/target \
    cargo cinstall --locked -p gst-plugin-isobmff -p gst-plugin-hlssink3 -p gst-plugin-dav1d -p gst-plugin-rtp --release \
        --prefix /usr --libdir /usr/lib/x86_64-linux-gnu --destdir /stage \
    && for p in isobmff hlssink3 dav1d rsrtp; do \
         install -D -m 0644 /stage/usr/lib/x86_64-linux-gnu/gstreamer-1.0/libgst$p.so /out/gst/libgst$p.so; \
       done

# The desk, the control plane and the chat (M6).
WORKDIR /src/weavesignal
COPY Cargo.toml Cargo.lock ./
COPY crates crates
# The chat builds in the list of shipped avatars the pages draw from (ADR-0235).
COPY packages/ui/avatars/manifest.json packages/ui/avatars/manifest.json
COPY apps/studio/src-tauri/Cargo.toml apps/studio/src-tauri/Cargo.toml
RUN mkdir -p apps/studio/src-tauri/src && echo 'fn main() {}' > apps/studio/src-tauri/src/main.rs
RUN --mount=type=cache,target=/opt/cargo/registry \
    --mount=type=cache,target=/src/weavesignal/target \
    cargo build --release --locked -p weavesignal-desk -p weavesignal-control -p weavesignal-chat \
        --bin weavesignal-desk --bin weavesignal-control --bin weavesignal-chat \
    && install -D -m 0755 target/release/weavesignal-desk /out/bin/weavesignal-desk \
    && install -D -m 0755 target/release/weavesignal-control /out/bin/weavesignal-control \
    && install -D -m 0755 target/release/weavesignal-chat /out/bin/weavesignal-chat

########################################################################
FROM ${BASE}
ARG SRT_VERSION=1.5.7
ARG LIQUIDSOAP_DEB=liquidsoap_2.5.0-ubuntu-resolute-ocaml5.5.1-1_amd64.deb
ARG LIQUIDSOAP_TAG=v2.5.0-rc1
ARG LIQUIDSOAP_SHA256=f19fd30cf82ce843ee6a8b924b2097e3259b8aa56887952609f09b4541ebeebb

ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        gstreamer1.0-plugins-base gstreamer1.0-plugins-good gstreamer1.0-plugins-bad \
        gstreamer1.0-nice gstreamer1.0-tools libsrt1.5-gnutls ca-certificates msmtp \
        gstreamer1.0-x libdav1d7 fontconfig \
    && curl -fsSL -o /tmp/${LIQUIDSOAP_DEB} \
        https://github.com/savonet/liquidsoap/releases/download/${LIQUIDSOAP_TAG}/${LIQUIDSOAP_DEB} \
    && echo "${LIQUIDSOAP_SHA256}  /tmp/${LIQUIDSOAP_DEB}" | sha256sum -c - \
    && apt-get install -y --no-install-recommends /tmp/${LIQUIDSOAP_DEB} \
    && rm -f /tmp/${LIQUIDSOAP_DEB} \
    && rm -rf /var/lib/apt/lists/*

# libsrt 1.5.7 in place of Ubuntu's 1.5.4, diverted so dpkg knows (ADR-0048).
COPY --from=build /out/srt/libsrt-gnutls.so.${SRT_VERSION} /usr/lib/x86_64-linux-gnu/
RUN set -e; lib=/usr/lib/x86_64-linux-gnu; \
    for f in $(dpkg -L libsrt1.5-gnutls | grep -E '/libsrt-gnutls\.so\.[0-9.]+$'); do \
      [ -L "$f" ] || dpkg-divert --quiet --local --rename --add "$f"; \
    done; \
    ln -sf libsrt-gnutls.so.${SRT_VERSION} $lib/libsrt-gnutls.so.1.5; ldconfig; \
    readlink -f $lib/libsrt-gnutls.so.1.5 | grep -q "${SRT_VERSION}$"

COPY --from=build /out/gst/ /usr/lib/x86_64-linux-gnu/gstreamer-1.0/
# The slate's text is drawn with Noto Sans (the desk asks pango for it by name); without it
# pango falls back to DejaVu. The three files are the ones compiled into the control plane for
# the show cards, unmodified, with the licence text (SIL OFL 1.1, NOTICES.md).
COPY crates/cards/fonts/NotoSans-Regular.ttf crates/cards/fonts/NotoSans-SemiBold.ttf \
     crates/cards/fonts/NotoSans-Bold.ttf LICENSES/OFL-1.1.txt /usr/local/share/fonts/noto-sans/
RUN fc-cache -f && fc-match "Noto Sans" | grep -q '^NotoSans-'
COPY --from=build /out/bin/ /app/code/bin/
COPY --from=livekit /livekit-server /app/code/bin/livekit-server

# Every element the desk makes (carts included: filesrc, flacparse, flacdec; and the video path's
# writer, encoder, decoders and slate, ADR-0227; RTMP out's encoders and sender, ADR-0230), and
# srtsrc for the check below, must load; then the registry is dropped, since the image is
# read-only at run time and the registry lives in /run. `svtav1enc` is Ubuntu's (libsvtav1enc2);
# `nvav1enc` and `nvh264enc` are not here (no GPU). `openh264enc` and `voaacenc` come with
# Ubuntu's gstreamer1.0-plugins-bad, over Ubuntu's own builds of OpenH264 (`libopenh264-8`)
# and vo-aacenc (`libvo-aacenc0`): see NOTICES.md for what that means for H.264's patents.
RUN for e in livekitwebrtcsrc livekitwebrtcsink webrtcbin srtsink srtsrc matroskamux audiomixer \
             audioconvert audioresample audiotestsrc capsfilter opusenc opusdec queue tee fakesink \
             filesrc flacparse flacdec jpegenc \
             cmafmux hlscmafsink dav1ddec av1dec av1parse rtpav1depay svtav1enc vp8dec vp9dec textoverlay \
             videotestsrc videoconvert videoscale videorate \
             openh264enc h264parse voaacenc aacparse appsrc rtmp2sink; do \
        gst-inspect-1.0 --exists "$e" || { echo "MISSING: $e"; exit 1; }; \
    done \
    && ldd /usr/lib/x86_64-linux-gnu/gstreamer-1.0/libgstsrt.so | grep -q 'libsrt-gnutls.so.1.5 => /usr/lib/x86_64-linux-gnu/libsrt-gnutls.so.1.5 ' \
    && /app/code/bin/livekit-server --version \
    && liquidsoap --version | head -1 \
    && msmtp --version | head -1 \
    && rm -rf /root/.cache

COPY playout/weavesignal.liq playout/listeners.liq playout/chain.liq /app/code/playout/
# Liquidsoap's script cache, built now: with no cache, each Liquidsoap typechecks at start and
# peaks near 650 MB before settling near 130 MB, and the two starting together passed a 1 GiB
# limit (measured 2026-10-25). From this cache each peaks near 170 MB. Read-only at run time.
RUN mkdir -p /app/code/liq-cache \
    && for s in listeners weavesignal; do \
         LIQ_CACHE_USER_DIR=/app/code/liq-cache liquidsoap --cache-only /app/code/playout/$s.liq; \
       done \
    && chmod 0755 /app/code/liq-cache && chmod 0444 /app/code/liq-cache/*
COPY deploy/cloudron/start.sh deploy/cloudron/supervisord.conf /app/code/
COPY --from=web /src/apps/web/dist /app/code/www
# The placeholder cart pack (ADR-0020), loaded as the demo banks when a station has none.
COPY assets/carts-demo /app/code/carts-demo
# The default loop (ADR-0130, CC0-1.0): playout and the listener process play it while the
# fallback folder holds nothing, so a new station is never silent.
COPY assets/default-loop/default-loop.flac /app/code/default-loop/default-loop.flac
RUN chmod 0755 /app/code/start.sh

LABEL org.opencontainers.image.title="weavesignal-cloudron" \
      org.opencontainers.image.description="WeaveSignal release one as one Cloudron app: control plane, chat, desk, Liquidsoap 2.5 playout and listener process, LiveKit with TURN, public pages" \
      org.opencontainers.image.licenses="AGPL-3.0-only"

WORKDIR /app/code
CMD ["/app/code/start.sh"]
