---
title: "Licences"
description: "Which licence covers what: the code is AGPL-3.0-only; the documentation's text and images are CC BY-SA 4.0; code samples in the documentation are AGPL-3.0-only."
type: reference
status: decided
last_verified: 2026-11-04
sidebar:
  order: 11
---

| What | Licence | Decided in |
|---|---|---|
| WeaveSignal's code: every crate, script, configuration file and the documentation site's own code | AGPL-3.0-only (`AGPL-3.0-only`), without "or later"; the text is `LICENSE` | ADR-0073 |
| The documentation's text and images, on this site and in `llms.txt`, screenshots and state pictures included | Creative Commons Attribution-ShareAlike 4.0 International (`CC-BY-SA-4.0`) | ADR-0081 |
| Code samples and snippets inside the documentation: commands, settings files, code blocks | AGPL-3.0-only, as the code is | ADR-0081 |
| Translations of the documentation | the same licences as the original | ADR-0081 |

The repository is REUSE-compliant: every file carries its licence and copyright in a header, or in `REUSE.toml` when it cannot carry one (lock files, JSON, pictures, these pages).
`just reuse-check` checks it, and it is part of `just ci`.
The licence texts are in the repository's `LICENSES/` folder.

## Other people's work in the repository

Each keeps its own licence; `NOTICES.md` in the repository records where each comes from.

| What | Licence |
|---|---|
| The WeaveSignal icon and the station's mark on show cards: Boxicons' `broadcast` (ADR-0094) | MIT |
| Two state glyphs: DESK BUSY (Reicon's `hand`) and LINK DEGRADED (with icons' `signal-blueprint`) | MIT |
| The show cards' fonts: Noto Sans, Noto Sans Arabic and Noto Sans Mono, compiled into the control plane | OFL-1.1 |
| The two demo slate pictures in `assets/stills-demo/` | Apache-2.0 and MIT, one each |
| The demo cart sounds in `assets/carts-demo/` | each its own: CC0-1.0, CC-BY-3.0, CC-BY-4.0 or public domain (`assets/carts-demo/CREDITS.md`) |
| The default loop in `assets/default-loop/`, made by WeaveSignal from sine tones | CC0-1.0, so a station can keep, change or replace it freely |
| The Podcast Taxonomy and the Podcasting 2.0 schema, in the feeds crate | CC0-1.0 |
| The patch to gst-plugins-rs's LiveKit signaller that the desk image carries (`spike/desk-image/patches/`), so each return reaches only its own person | MPL-2.0, as the code it patches |
| The order of a podcast feed's elements, ported from Castopod into `crates/feeds/src/rss.rs` | AGPL-3.0-or-later, taken as AGPL-3.0-only |

Nothing is copied from Icecast, which is GPL-2.0 only.

## Software WeaveSignal runs beside its own

LiveKit, Liquidsoap, GStreamer, PostgreSQL and the rest run as their own processes or libraries, unmodified, each under its own licence; `NOTICES.md` lists each with the version tested.
Every Rust crate the workspace depends on is under a licence on `deny.toml`'s list, and `just licences` checks it.

The diagrams on this site are documentation images, CC BY-SA 4.0.
