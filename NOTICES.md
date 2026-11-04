<!-- SPDX-License-Identifier: AGPL-3.0-only -->
# Notices

WeaveSignal's own code is AGPL-3.0-only (ADR-0073); the documentation's text is CC BY-SA 4.0
(ADR-0081). This file records where anything else comes from: code ported into this
repository, software WeaveSignal runs beside its own, and data in its tests. It is updated in the
same change that brings something new in.

## Code ported into this repository

Code may be ported from AzuraCast, LibreTime, Liquidsoap and Castopod with its notices kept and
its origin recorded here (ADR-0001, ADR-0014). Nothing is ever copied from Icecast, which is
GPL-2.0 only.

| Into | From | Licence | What |
|---|---|---|---|
| `crates/feeds/src/rss.rs` | Castopod, `app/Helpers/rss_helper.php` (`get_rss_feed`, `add_category_tag`), `https://code.castopod.org/adaures/castopod`, branch `develop` at commit `e439b31ac2471f39de11fb9db0c82a23c88c1bb5` (2026-06-05), Copyright 2020 Ad Aures | AGPL-3.0-or-later, taken as AGPL-3.0-only | the order of a feed's channel and item elements; `podcast:guid` seeded from the feed address with the namespace UUID (corrected: without the scheme or trailing slash, as the namespace says); `podcast:locked` with its owner; one `podcast:person` per role; the Apple category with its plain RSS twin; the `image` block. Not ported: premium feeds, plugins, the fediverse and comment tags. The file keeps Castopod's copyright line. |

## Data from other projects

- **The Podcast Taxonomy** (`crates/feeds/data/taxonomy.json`) and **the Podcasting 2.0
  namespace's XML schema** (`crates/feeds/tests/podcast-namespace/`, used by the feed tests)
  are from `https://github.com/Podcastindex-org/podcast-namespace` at commit `d4036dc`
  (2026-10-10), unmodified, CC0-1.0.

## Artwork

- **The WeaveSignal icon** (ADR-0094) is Boxicons' "broadcast" glyph from boxicons-core
  (`https://github.com/box-icons/boxicons-core`), MIT, Copyright (c) 2026 Boxicons; the licence
  text is in `LICENSES/MIT.txt`. `packages/ui/icon/weavesignal-icon-broadcast.svg` is the glyph
  as published; `weavesignal-app-icon.svg` sets it in the dark theme's ink on its surface, and the
  studio app's window icons (`apps/studio/src-tauri/icons/`) and the documentation site's
  favicon (`docs/public/favicon.svg`) are made from that.
- **Two state glyphs** in the studio app (`apps/studio/src/lib/Glyphs.svelte`) and the
  documentation's state pictures (`docs/src/assets/states/`) are third-party icons, taken
  unmodified from Iconify on 2026-11-02 and drawn at 14 px: DESK BUSY is Reicon's "hand"
  (`https://github.com/dqev/reicon`), MIT, Copyright (c) 2025 REICON; LINK DEGRADED is with
  icons' "signal-blueprint" (`https://github.com/withevergrow/withicons`), MIT, Copyright (c)
  2026 with icons contributors. The licence text is in `LICENSES/MIT.txt`.
- **Two demo slate pictures** (`assets/stills-demo/`, ADR-0200) are third-party icons, each
  centred on the design system's dark surface with no words: Kettle FM's is Material Design
  Icons' "kettle-outline" (`https://github.com/Templarian/MaterialDesign`), Apache-2.0,
  Copyright Pictogrammers, unmodified; the demo night show's is Material Line Icons' "moon"
  (`https://github.com/cyberalien/line-md`), MIT, Copyright Vjacheslav Trushkin, at the
  animation's final frame. Both were taken from Iconify on 2026-11-03; the licence texts are in
  `LICENSES/Apache-2.0.txt` and `LICENSES/MIT.txt`, and the table is
  `assets/stills-demo/README.md`.

## Fonts

- **The show cards' faces** (`crates/cards/fonts/`, ADR-0053) are Noto Sans 2.015 (Regular,
  SemiBold, Bold), Noto Sans Arabic 2.013 (Regular, SemiBold, Bold) and Noto Sans Mono 2.014
  (Regular), the unhinted TTFs from the Noto Project's releases
  (`https://github.com/notofonts/latin-greek-cyrillic/releases` and
  `https://github.com/notofonts/arabic/releases`), taken unmodified on 2026-10-26. Copyright
  2022 The Noto Project Authors; SIL Open Font License 1.1, whose text is in
  `LICENSES/OFL-1.1.txt`. They are compiled into the control plane so every server draws the
  same card; they are not sold on their own, and "Noto" is not used as the name of anything
  derived from them. A binary distribution must carry `LICENSES/OFL-1.1.txt` with this notice.
- **The station's mark on a card** is the WeaveSignal icon's Boxicons "broadcast" glyph (MIT,
  above), drawn in the show's colour until a station uploads its own logo.

## Software WeaveSignal runs beside its own

Each keeps its own licence and runs as its own process or library; none is modified.

| Component | Version tested | Licence | Used for |
|---|---|---|---|
| LiveKit server | 1.13.10 | Apache-2.0 | the live room, its TURN |
| Liquidsoap | 2.5.0-rc1 | GPL-2.0-or-later | playout and `icecast.server`, running the fixed scripts in `playout/` |
| GStreamer core, base, good, bad | 1.28.8 | LGPL-2.1-or-later | the desk's media pipeline |
| gst-plugins-rs (`webrtc`, with LiveKit) | 0.15.4 | MPL-2.0 | `livekitwebrtcsrc` and `livekitwebrtcsink` |
| gst-plugins-rs (`isobmff`, `hlssink3`, `dav1d`) | 0.15.4 | MPL-2.0 | the video path (M5): `cmafmux`, `hlscmafsink`, `dav1ddec` |
| SVT-AV1 | 3.1.2 (Fedora's) | BSD-3-Clause-Clear (with the AOM patent licence) | AV1 encoding without a GPU, through `svtav1enc` |
| dav1d | as packaged | BSD-2-Clause | decoding guests' AV1 cameras |
| libvpx | as packaged | BSD-3-Clause | VP8 and VP9 cameras, and the programme picture in the room |
| NVIDIA driver (NVENC, CUDA) | the host's | proprietary, not shipped | AV1 encoding where the station has an NVIDIA GPU (`nvav1enc`); never in our images |
| libsrt | 1.5.7 | MPL-2.0 | SRT between the desk and playout |
| libopus | as packaged | BSD-3-Clause | Opus, through GStreamer and Liquidsoap |
| LAME (`libmp3lame`) | 3.100 (in Liquidsoap's image) | LGPL-2.0-or-later | the MP3 compatibility stream (ADR-0002), off unless the station turns it on; linked by Liquidsoap, not by our code. MP3's patents have expired (2017) |
| PostgreSQL | 18 | PostgreSQL | the control plane's data (ADR-0082) |
| nginx | 1.30 (Alpine 3.24's package) | BSD-2-Clause | the HLS edge image (`deploy/images/edge`), a pull-through cache configured at start |
| Icecast | 2.5.0 (Alpine 3.24's package) | GPL-2.0-only | the audio relay image (`deploy/images/edge`), configured at start; none of its code is in this repository |
| Astro, Starlight | 7.4.2, 0.42.7 | MIT | the documentation site |
| Tauri | 2.12.5 | MIT OR Apache-2.0 | the studio app's shell |
| WebKitGTK | 2.54.1 (the system's) | LGPL-2.1-or-later and BSD | the studio app's webview on Linux |
| Svelte, Vite | 5.57.2, 8.3.5 | MIT | the studio app's interface and the public pages, and their builds |
| Event Calendar (`@event-calendar/core`) | 5.18.1 | MIT | the Schedule tab's week grid, behind the adapter in `apps/studio/src/schedule/calendar/` (ADR-0185) |
| hls.js | 1.7.3 | Apache-2.0 | the player's HLS (AV1 and Opus in fMP4, the audio-only toggle), bundled unmodified, loaded only when play is pressed |
| livekit-client | 2.23.0 | Apache-2.0 | the join pages' room client (`apps/web`), bundled unmodified; it brings `@livekit/protocol`, `@livekit/mutex` and `rxjs` (Apache-2.0), `@bufbuild/protobuf` (Apache-2.0 and BSD-3-Clause), `webrtc-adapter` (BSD-3-Clause), `events`, `jose`, `loglevel`, `sdp`, `sdp-transform` (MIT), `tslib` (0BSD) and `machina` (MIT or GPL, taken as MIT); `apps/web/pnpm-lock.yaml` pins each |

### What the desktop packages carry (M10)

The `.deb` and `.rpm` hold only the studio app and its icons; WebKitGTK and GTK come from the
system. Two packages carry other people's code, unmodified, each under its own licence:

| Package | Carries | Licence |
|---|---|---|
| AppImage | WebKitGTK 4.1, GTK 3, GLib, libsoup, GStreamer and the other libraries `ldd` finds, as packaged by Ubuntu 22.04 when built; linuxdeploy's GTK plugin's start-up hook; AppImageKit's `AppRun` | LGPL-2.1-or-later and LGPL-2.0-or-later for the libraries, with others as each library says; MIT for linuxdeploy's hook and `AppRun`. The source of each library is the one Ubuntu 22.04 publishes for that package version |
| Windows installer | Microsoft's `WebView2Loader.dll`, from the `webview2-com-sys` crate 0.39.1; `nsis_tauri_utils.dll` 0.5.3 (Tauri's NSIS plugin); the NSIS installer stub 3.11 | Microsoft's WebView2 SDK licence (redistributable; to be confirmed before the first release); MIT or Apache-2.0; zlib/libpng for NSIS |

The build downloads linuxdeploy, its AppImage plugin, `AppRun` and `nsis_tauri_utils.dll` from
the Tauri and linuxdeploy projects' release pages when it first runs (`deploy/desktop/build.sh`).

### GStreamer elements the desk uses

The desk uses only these elements (ADR-0003 asks for the list); none is GPL.

| Element | Plugin set | Licence |
|---|---|---|
| `audiomixer`, `audiotestsrc`, `audioconvert`, `audioresample`, `volume`, `opusenc`, `opusdec`, `oggdemux` | base | LGPL-2.1-or-later |
| `tee`, `queue`, `capsfilter`, `fakesink` | core | LGPL-2.1-or-later |
| `matroskamux`, `matroskademux`, `souphttpsrc` | good | LGPL-2.1-or-later |
| `srtsink`, `srtsrc` | bad (with libsrt, MPL-2.0) | LGPL-2.1-or-later |
| `livekitwebrtcsrc`, `livekitwebrtcsink`, `webrtcbin` and the elements inside them | gst-plugins-rs `webrtc`; bad | MPL-2.0; LGPL-2.1-or-later |
| `videotestsrc`, `videoconvert`, `videoscale`, `videorate`, `textoverlay`, `appsrc`, `appsink` | base | LGPL-2.1-or-later |
| `vp8dec`, `vp9dec`, `qtdemux` | good | LGPL-2.1-or-later |
| `av1parse`, `av1dec`, `svtav1enc`, `nvav1enc`, `cudaupload` | bad | LGPL-2.1-or-later |
| `cmafmux`, `hlscmafsink`, `dav1ddec` | gst-plugins-rs `isobmff`, `hlssink3`, `dav1d` | MPL-2.0 |

`souphttpsrc`, `oggdemux`, `opusdec`, `srtsrc`, `matroskademux` and `qtdemux` appear in tests only. The desk image also carries FFmpeg (Fedora's `ffmpeg-free`, LGPL) for the VMAF measurement; the server image does not.

## Rust dependencies

Every crate the workspace depends on is under a licence in `deny.toml`'s list (MIT,
Apache-2.0, BSD-3-Clause, BSL-1.0, ISC, MPL-2.0, Unicode-3.0, Unlicense, Zlib, or a choice among
them; the studio app's updater, tauri-plugin-updater 2.14.0, is MIT OR Apache-2.0 and uses the
platform's TLS rather than rustls. The control plane's webhook deliveries (ADR-0195) do use rustls, through ureq's `rustls` feature (and so does the chat's ureq when the two are built together, though the chat calls only the control plane over http): rustls (Apache-2.0 OR ISC OR MIT), ring (Apache-2.0 AND ISC), rustls-webpki and untrusted (ISC), and webpki-roots, Mozilla's root certificates as data (CDLA-Permissive-2.0), all unmodified, so `deny.toml` allows ISC and CDLA-Permissive-2.0 and nothing else came in with them (`cargo deny list` names only these five). Both are permissive and compatible with distributing the whole under AGPL-3.0-only; a binary distribution carries their notices as these crates ship them (`LICENSE` in each, and for webpki-roots the CDLA-Permissive-2.0 text, its section 3.1), as it does every crate's; MPL-2.0 arrives with the studio app's Tauri and WebKitGTK crates, and with Symphonia in
the media worker, all unmodified); `just licences` checks it with `cargo deny check licenses`, and `Cargo.lock` names each crate
and version.

The media worker (`crates/media`) parses untrusted audio with these, all pure Rust and
unmodified: Symphonia 0.6.1 (MPL-2.0; WAV, FLAC, MP3, Vorbis, AAC), opus-decoder 0.1.1 (MIT OR
Apache-2.0), ogg 0.9.2 (BSD-3-Clause), ebur128 0.1.10 (MIT), flacenc 0.5.1 (Apache-2.0), and
landlock 0.4.7 and rustix 1.1.5 (MIT OR Apache-2.0) for its sandbox. Two faults found in them
are worked around in our code, not patched: opus-decoder's multistream packet split
(`crates/media/src/multistream.rs`) and flacenc's STREAMINFO minimum block size.

## The default loop in `assets/default-loop/`

`default-loop.flac` is the loop a new station plays until it adds its own fallback files (ADR-0130).
It is WeaveSignal's own work: 32 seconds of sine tones, synthesised by `make.sh` beside it, with no
sample or recording of anyone else's in it. The recording is released under CC0-1.0
(`LICENSES/CC0-1.0.txt`), so a station may keep it, change it or replace it freely; `make.sh` is
code and AGPL-3.0-only.

## In `spike/`

Spike 01's harness and results are measurements, not product code.

- `spike/harness/web/livekit-client.umd.js` is LiveKit's JavaScript client as published,
  Apache-2.0, unmodified.
- The synthetic talk show (`spike/talkshow/`) builds its voices locally and keeps them out of
  git: two consented personal-use voice clones, and four speakers from the CSTR VCTK Corpus
  (ODC-By 1.0; attribution in the ignored `voices/ATTRIBUTION.txt`). Nothing derived from
  them is committed.

## Sound effects in `assets/carts-demo/`

Demo sounds for the cart wall, cut, loudness-set and re-encoded to Ogg Opus. Most are CC0 or public domain (Wikimedia Commons, Kenney.nl, OpenGameArt); the full table is `assets/carts-demo/CREDITS.md`. These need credit (CC BY):

- `laugh-1.opus`: "72842 lonemonk approx-800-laugh-1.wav" by lonemonk (via Freesound, sound 72842), <https://commons.wikimedia.org/wiki/File:72842_lonemonk_approx-800-laugh-1.wav>, CC BY 3.0 (<https://creativecommons.org/licenses/by/3.0>); cut, loudness-set and re-encoded.
- `laugh-2.opus`: "72843 lonemonk approx-800-laughter-only-1.wav" by lonemonk (via Freesound, sound 72843), <https://commons.wikimedia.org/wiki/File:72843_lonemonk_approx-800-laughter-only-1.wav>, CC BY 3.0 (<https://creativecommons.org/licenses/by/3.0>); cut, loudness-set and re-encoded.
- `laugh-3.opus`: "72844 lonemonk approx-800-laughter-and-clapter-1.wav" by lonemonk (via Freesound, sound 72844), <https://commons.wikimedia.org/wiki/File:72844_lonemonk_approx-800-laughter-and-clapter-1.wav>, CC BY 3.0 (<https://creativecommons.org/licenses/by/3.0>); cut, loudness-set and re-encoded.
- `rimshot-1.opus`: "Kevin MacLeod assorted rimshots - Bah-dum.wav" by Kevin MacLeod (incompetech.com), <https://commons.wikimedia.org/wiki/File:Kevin_MacLeod_assorted_rimshots_-_Bah-dum.wav>, CC BY 3.0 (<https://creativecommons.org/licenses/by/3.0>); cut, loudness-set and re-encoded.
- `rimshot-2.opus`: "Kevin MacLeod assorted rimshots - Bu-doo.wav" by Kevin MacLeod (incompetech.com), <https://commons.wikimedia.org/wiki/File:Kevin_MacLeod_assorted_rimshots_-_Bu-doo.wav>, CC BY 3.0 (<https://creativecommons.org/licenses/by/3.0>); cut, loudness-set and re-encoded.
- `rimshot-3.opus`: "Kevin MacLeod assorted rimshots - Bah-dum-bum-ding.wav" by Kevin MacLeod (incompetech.com), <https://commons.wikimedia.org/wiki/File:Kevin_MacLeod_assorted_rimshots_-_Bah-dum-bum-ding.wav>, CC BY 3.0 (<https://creativecommons.org/licenses/by/3.0>); cut, loudness-set and re-encoded.
- `boing-1.opus`: "Boing raw.ogg" by cfork, <https://commons.wikimedia.org/wiki/File:Boing_raw.ogg>, CC BY 4.0 (<https://creativecommons.org/licenses/by/4.0>); cut, loudness-set and re-encoded.
- `sad-trombone-1.opus`: "Sad Trombone-Joe Lamb-665429450.ogg" by Joe Lamb, <https://commons.wikimedia.org/wiki/File:Sad_Trombone-Joe_Lamb-665429450.ogg>, CC BY 3.0 (<https://creativecommons.org/licenses/by/3.0>); cut, loudness-set and re-encoded.
- `phone-ring-1.opus`: "WWS AutomatictelephoneIskraEta80ringing.ogg" by Work With Sounds / Technical Museum of Slovenia, <https://commons.wikimedia.org/wiki/File:WWS_AutomatictelephoneIskraEta80ringing.ogg>, CC BY 4.0 (<https://creativecommons.org/licenses/by/4.0>); cut, loudness-set and re-encoded.
- `phone-ring-2.opus`: "Zero-project - Metamorphosis (ringtone).ogg" by zero-project, <https://commons.wikimedia.org/wiki/File:Zero-project_-_Metamorphosis_%28ringtone%29.ogg>, CC BY 4.0 (<https://creativecommons.org/licenses/by/4.0>); cut, loudness-set and re-encoded.
- `drumroll-2.opus`: "Kevin MacLeod assorted rimshots - 4-second roll.wav" by Kevin MacLeod (incompetech.com), <https://commons.wikimedia.org/wiki/File:Kevin_MacLeod_assorted_rimshots_-_4-second_roll.wav>, CC BY 3.0 (<https://creativecommons.org/licenses/by/3.0>); cut, loudness-set and re-encoded.
- `fanfare-2.opus`: "Kevin MacLeod - Fanfare for Space.ogg" by Kevin MacLeod (incompetech.com), <https://commons.wikimedia.org/wiki/File:Kevin_MacLeod_-_Fanfare_for_Space.ogg>, CC BY 4.0 (<https://creativecommons.org/licenses/by/4.0>); cut, loudness-set and re-encoded.
- `news-sting-1.opus`: "NewsSting (ISRC USUAN1100361).mp3" by Kevin MacLeod (incompetech.com), <https://commons.wikimedia.org/wiki/File:NewsSting_%28ISRC_USUAN1100361%29.mp3>, CC BY 3.0 (<https://creativecommons.org/licenses/by/3.0>); cut, loudness-set and re-encoded.
- `news-sting-2.opus`: "Light Sting (ISRC USUAN1100433).mp3" by Kevin MacLeod (incompetech.com), <https://commons.wikimedia.org/wiki/File:Light_Sting_%28ISRC_USUAN1100433%29.mp3>, CC BY 3.0 (<https://creativecommons.org/licenses/by/3.0>); cut, loudness-set and re-encoded.
- `thunder-1.opus`: "Rhumphries - rbh-thunder-storm.ogg" by RHumphries, <https://commons.wikimedia.org/wiki/File:Rhumphries_-_rbh-thunder-storm.ogg>, CC BY 3.0 (<https://creativecommons.org/licenses/by/3.0>); cut, loudness-set and re-encoded.
- `bed-1.opus`: "Wholesome by Kevin MacLeod.ogg" by Kevin MacLeod (incompetech.com), <https://commons.wikimedia.org/wiki/File:Wholesome_by_Kevin_MacLeod.ogg>, CC BY 4.0 (<https://creativecommons.org/licenses/by/4.0>); cut, loudness-set and re-encoded.
- `bed-2.opus`: "Chill Background Music (Antti Luode).mp3" by Antti Luode, <https://commons.wikimedia.org/wiki/File:Chill_Background_Music_%28Antti_Luode%29.mp3>, CC BY 3.0 (<https://creativecommons.org/licenses/by/3.0>); cut, loudness-set and re-encoded.
