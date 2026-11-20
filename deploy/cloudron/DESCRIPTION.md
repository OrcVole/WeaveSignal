<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
WeaveSignal is talk-radio software: presenters, guests and callers meet in a live room, a desk
mixes the programme with a mix-minus for each person, and playout sends it to listeners.

This package runs release one's server in one app: the control plane (Rust, Axum), the live
chat (Rust, Axum), the desk (Rust, GStreamer), Liquidsoap 2.5 for playout and for the listener
process that holds the stream with `icecast.server`, LiveKit with its own TURN, and the
station's public pages. It uses Cloudron's PostgreSQL and mail.

A Cloudron app is given no GPU, so with video on the desk encodes AV1 on the CPU with SVT-AV1
(`VIDEO_ENCODER=svt`). A station that wants NVENC runs the Compose bundle or the Quadlets on a
server with an NVIDIA card instead.

The studio app for presenters and screeners (Linux and Windows) connects to it over https.
Documentation: https://weavesignal.top. Source: https://forgejo.weavesignal.top/WeaveSignal/weavesignal.
