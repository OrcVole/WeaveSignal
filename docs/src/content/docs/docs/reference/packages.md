---
title: "Packages and install files"
description: "Every package and install route the repository builds: the desktop packages and their files, the build settings, the updater's key, weavesignal.sh's settings file and the generated Quadlets."
type: reference
status: draft
last_verified: 2026-11-21
sidebar:
  order: 12
---

What the repository builds, where each piece lives, and where the 1.0.0 builds are published.

| What | Published at |
|---|---|
| The studio app's packages, with their `.sig` files and checksums | the assets of [release v1.0.0 on the Forgejo](https://forgejo.weavesignal.top/WeaveSignal/weavesignal/releases/tag/v1.1.1), and the updater's tree under `https://weavesignal.top/updates/` |
| The updater's feed | `https://weavesignal.top/updates/latest.json` |
| The server image the Compose bundle and the Quadlets run | `forgejo.weavesignal.top/weavesignal/weavesignal-server:1.3.2` |
| The edge and relay images | `forgejo.weavesignal.top/weavesignal/weavesignal-edge:1.3.2`, `forgejo.weavesignal.top/weavesignal/weavesignal-relay:1.3.2` |
| The Cloudron package's image | `forgejo.weavesignal.top/weavesignal/weavesignal-cloudron:1.3.2` ([Install on Cloudron](/docs/how-to/install-on-cloudron/)) |

The repository's root also holds `CHANGELOG.md`, what each release changed, and `SECURITY.md`, how to report a security fault.

## The desktop packages

Built by `just package-desktop` (Linux) and `just package-windows` (Windows) into `dist/desktop/`, with their SHA-256 checksums.

| File | For | Size (measured at 0.1.0) | Package name |
|---|---|---|---|
| `WeaveSignal_<v>_amd64.AppImage` | any Linux desktop, amd64 | 80 MiB | none: a single file |
| `WeaveSignal_<v>_amd64.deb` | Debian 13 and later, Ubuntu 24.04 and later | 3.6 MiB | `weave-signal` |
| `WeaveSignal-<v>-1.x86_64.rpm` | Fedora 43 and later | 3.6 MiB | `weave-signal` |
| `WeaveSignal_<v>_x64-setup.exe` | Windows 10 and 11, x64; unsigned (ADR-0076) | 2.9 MiB | WeaveSignal |
| `SHA256SUMS-<v>`, `SHA256SUMS-<v>-windows` | the checksums, published beside the packages | | |
| `*.sig` | each package's update signature, when the updater is on | | |

For 1.0.0 the files are `WeaveSignal_1.0.0_amd64.AppImage`, `WeaveSignal_1.0.0_amd64.deb`, `WeaveSignal-1.0.0-1.x86_64.rpm` and `WeaveSignal_1.0.0_x64-setup.exe`, with `SHA256SUMS-1.0.0` and `SHA256SUMS-1.0.0-windows`.
`weave-signal` is the Tauri bundler's spelling of the product name, WeaveSignal.
No macOS package yet (ADR-0015).

The `.deb` and `.rpm` install:

| Path | What |
|---|---|
| `/usr/bin/weavesignal-studio` | the app |
| `/usr/share/applications/WeaveSignal.desktop` | the menu entry; it starts the app with `WEBKIT_DISABLE_DMABUF_RENDERER=1` |
| `/usr/share/icons/hicolor/*/apps/weavesignal-studio.png` | the icon at 32 to 512 pixels |

They depend on WebKitGTK 4.1 and GTK 3 (`libwebkit2gtk-4.1-0` and `libgtk-3-0`; `libwebkit2gtk-4.1.so.0` and `libgtk-3.so.0` in the `.rpm`).
The AppImage carries them, leaving out only the libraries every desktop has (fontconfig, FreeType, HarfBuzz, FriBidi, X11, EGL and GL, GBM).
The Windows installer installs for the current user only, with Microsoft's `WebView2Loader.dll` beside the app, and fetches the WebView2 runtime if Windows lacks it.

`weavesignal-studio --version` (or `-V`) prints the version and exits before any window opens; a Windows build has no console, so it prints nothing there.

## Building them

| Recipe | Does |
|---|---|
| `just desktop-image` | the Linux build image: Ubuntu 24.04, WebKitGTK 4.1, Rust 1.98.1, Tauri CLI 2.12.4 (`deploy/desktop/Containerfile`) |
| `just desktop-windows-image` | the Windows build image: Debian 13, MinGW-w64, NSIS 3.11 (`deploy/desktop/windows/Containerfile`) |
| `just package-desktop` | the interface built with pnpm 10, then the AppImage, `.deb` and `.rpm` in the Linux image |
| `just package-windows` | the installer, cross-built for the `x86_64-pc-windows-gnu` target |
| `just package-desktop-test` | each Linux package installed in Debian 13, Ubuntu 24.04 and 26.04, Fedora 43 and 44 containers; `--version`; the window on Xvfb for three of them; removal |

The Linux packages are built on Ubuntu 24.04, the oldest system they support (ADR-0126), on purpose: a package built on a newer system needs that system's C library, and the AppImage built on Fedora 44 ran nowhere older.
`deploy/desktop/build.sh` is what the recipes run; `WEAVESIGNAL_DESKTOP_IMAGE=` (empty) builds Linux packages on the host instead, for development.

## The updater

The app checks for signed updates with Tauri's updater (ADR-0015).
The repository holds no key and no address: a build turns the updater on only when given them.
The 1.0.0 packages read `https://weavesignal.top/updates/latest.json`.

| Setting at build time | What |
|---|---|
| `WEAVESIGNAL_UPDATER_PUBKEY` | the public key (the `.pub` file `cargo tauri signer generate` writes), built into the app |
| `WEAVESIGNAL_UPDATER_ENDPOINT` | where the app reads `latest.json`, over HTTPS |
| `WEAVESIGNAL_UPDATER_KEY_FILE` | the private key file that signs each package's `.sig`; mounted read-only into the build container for that run |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | its password, passed to the container by name |

Without a public key the packages have no `.sig` files and the app never updates itself.
The app rejects an update whose signature does not name the version it was signed for (`requireSignedVersion`), so an old signed package cannot be served as a new one.

The private key is guarded like the release key (ADR-0015): it never enters the repository, a container image or a build log, and a build sees it only as a read-only file for that run.
Where the two keys are kept, and their backup, is the release maintainer's to decide; it is not written down here.
Losing it strands every installed studio: none can update again until it is reinstalled by hand.

### The update screen

**Settings > Updates** (panel `settings.updates`) shows a newer version with its notes, downloads it, installs it and restarts the app, only when asked.
It refuses while a strip is open or a talk key is held, before the download and again before the install; the install and rollback commands also ask the station's view themselves (`desk_busy` in `apps/studio/src-tauri/src/station.rs`), since the page's can be stale.
Its commands are the app's own (`update_check`, `update_download`, `update_install`, `update_rollback` in `apps/studio/src-tauri/src/update.rs`); the window is not granted the updater plugin's.
[Update the studio app, or roll it back](/docs/how-to/update-studio-app/) says how it behaves.

An AppImage keeps one copy of the version before an update (in `~/.local/share/org.weavesignal.studio/`) and can roll back from Settings.
The updater accepts only a newer version (`allowDowngrades` is off), so the feed cannot roll anyone back.

### The feed

A static tree in Tauri's `latest.json` format, built from the signed packages by `deploy/desktop/latest-json.sh` (`just package-feed`):

| File | What |
|---|---|
| `latest.json` | the newest release: `version`, `notes`, `pub_date` and one entry per platform |
| `<version>/<package>`, `<package>.sig`, `SHA256SUMS-<version>*` | each release's files, kept for every release |

The platform keys are `linux-x86_64-appimage`, `linux-x86_64-deb`, `linux-x86_64-rpm` and `windows-x86_64-nsis`; each entry holds the package's URL and the contents of its `.sig` file.
The updater looks a platform up by the kind of package the app was installed from, so there is no plain `linux-x86_64` entry: it would hand an AppImage to a `.deb` install.
The packages are served as they are, not as archives.

`just feed-test` checks the script and the signatures with no package build: it makes a throwaway key under `target/`, signs stand-in packages, builds the feed, serves it on a loopback port (23010 by default) and verifies every entry with the crate the updater itself uses.
The key is never committed.
The feed's address is not in the repository: a build is given it as `WEAVESIGNAL_UPDATER_ENDPOINT` (ADR-0140).

## Pinned build tools

Everything the build downloads is fixed to an exact version, and checked against a SHA-256 where the source offers a file to check.
A build that finds a different file stops; it never runs one it has not checked.

| Tool | Where it is used | Pinned by |
|---|---|---|
| rustup 1.29.2 (the `rustup-init` binary) | the two desktop build images | its SHA-256, as `RUSTUP_INIT_SHA256`; not the `sh.rustup.rs` script, which fetches whatever is current |
| Rust 1.98.1 | the two desktop build images | its version (`RUST_VERSION`); rustup checks each component against the channel manifest |
| Tauri CLI 2.12.4 | the two desktop build images | the SHA-256 of its crate (`TAURI_CLI_SHA256`), built with `--locked` from the lock file inside the crate, whose checksums pin every dependency |
| `AppRun`, linuxdeploy (07333c6) and linuxdeploy's AppImage plugin (1-alpha-20250213-1) | the Linux packages' AppImage | their SHA-256 in `deploy/desktop/tools.lock`; the bundler downloads them when its cache lacks them, so `deploy/desktop/fetch-tools.sh` puts the checked copies there first |
| `nsis_tauri_utils.dll` 0.5.3 | the Windows installer | the bundler's own SHA-1 of it; the lock file's SHA-256 checks the same file, but on a Linux host the bundler replaces its NSIS directory and downloads the DLL again, so its SHA-1 is the check that counts |
| pnpm 10.30.2 | the server image's public-pages stage | the SHA-256 of its tarball (`PNPM_SHA256`) |
| gst-plugins-rs 0.15.4+fixup | the desk images | the commit its tag names (`GST_RS_COMMIT`); a tag that moves fails the build |
| gst-plugins-bad 1.28.8 | the desk spike image | the SHA-256 of its tarball (already pinned) |

The bundler would fetch the AppImage plugin's moving `continuous` build, but only when its cache lacks the file: the lock file pins the plugin's fixed release 1-alpha-20250213-1 in its place, so `continuous` is never fetched.
The bundler treats the plugin as optional, so its line can be dropped to use linuxdeploy's built-in copy.
To move a pin, change the version and the checksum together in one commit, with the checksum taken from a download you have inspected.

Not pinned: the base images (`ubuntu:24.04`, `debian:13`, `fedora:44`, `node`, `alpine`) are named by tag, not by digest, and the distributions' packages come from their repositories.

## `weavesignal.sh`'s settings file

[`deploy/compose/weavesignal.sh`](/docs/how-to/install-with-weavesignal-sh/) writes one file, `deploy/compose/.env` by default (`--env-file` names another), mode 600:

| Setting | Made by the script |
|---|---|
| `STATION_ID` | a fresh identifier |
| `LIVEKIT_API_SECRET`, `LISTENERS_SOURCE_PASSWORD`, `POSTGRES_PASSWORD`, `CONTROL_ADMIN_TOKEN`, `CHAT_EMAIL_KEY`, `CONTROL_EMAIL_KEY`, `CONTROL_TOKEN_KEY`, `INGEST_TOKEN`, `DESK_SOURCES_PASSWORD` | 40 random letters and digits each, made once, kept on later runs and never printed; secrets are never taken from the environment. The three keys key the chat's and the staff's email hashes and the API tokens ([The email key](/docs/reference/chat/#the-email-key), [Integrations](/docs/reference/integrations/#the-key)) |
| `CONTROL_PORT`, `CHAT_PORT`, `LISTENERS_PORT`, `SRT_INGEST_PORT`, `PLAYOUT_FEED_PORT`, `DESK_FEED_PORT`, `DESK_SOURCES_PORT`, `LIVEKIT_HTTP_PORT`, `LIVEKIT_MEDIA_PORT` | free ports ([Ports](/docs/reference/ports/#with-weavesignalsh)) |
| `STATION_NAME`, `ROOM`, `SERVER_IMAGE`, `LIVEKIT_API_KEY`, `MAIL_MODE`, `MAIL_FROM` | the defaults, or what the environment gives; `--image` sets `SERVER_IMAGE` |
| `CONSENT_CONTACT` | empty unless set: call-in stays closed until it is ([Open call-in](/docs/how-to/install-with-weavesignal-sh/#open-call-in)) |
| `CONSENT_TEXT` | empty unless set: the default consent words. Set, the station's own words (from 1.2), which may use `{station}`, `{kept}`, `{contact}` and `{presenter}` ([The consent words](/docs/reference/public-pages/#the-consent-words)). The script writes them in single quotes with each apostrophe as `\'`, the one form Compose reads back whole, and refuses a backslash, a dollar sign, a per cent sign or a line break, which the Quadlets' units cannot carry whole |
| `DEMO_STILL` | the demo slate picture for a station left with the demo name, Kettle FM; empty for any other name; kept as written on later runs (ADR-0200) |
| `SEARCH_LISTING`, `DIRECTORY_LISTING` | `false` unless set (ADR-0133, ADR-0129) |
| `STUN_SERVERS` | empty unless set: the STUN servers the station may ask ([STUN servers](#stun-servers)) |
| `CONGESTION_CONTROL` | `gcc` unless set: how the desk's senders control congestion; `homegrown` or `disabled` ([`desk.toml`](/docs/reference/settings/#desktoml)); refused if it is none of the three |
| `VIDEO_ENCODER`, `GPU`, and with `GPU=nvidia` `GPU_DEVICE`, `NVENC_SESSIONS` | `auto` unless set; `GPU` unset or `auto` is what the script finds, written as `nvidia` (an NVIDIA CDI spec in `/etc/cdi` or `/var/run/cdi`) or `none`; `GPU=nvidia` with no spec is refused; `nvidia.com/gpu=all` and 12 ([Use a GPU for video](/docs/how-to/use-a-gpu-for-video/)). With `VIDEO=true` and `GPU=nvidia` the bundle adds `gpu.yaml`, and the Quadlets give the desk's unit `AddDevice=` |
| `KIT_VBAN` | `false` unless set: `true` lets the station take VBAN from a studio PC on its own network ([Connect a studio PC over VBAN](/docs/how-to/connect-a-studio-pc-over-vban/)); refused if it is neither. 1.2.0 removed the line on every run; from 1.2.1 it is kept |
| `PUBLIC_ORIGIN` without `--public` | empty unless set: the address a station without `--public` prints on its cards and feeds, such as `http://192.168.1.20:7310` on a private network; kept as written (from 1.2.1). With `--public` it is worked out from the name instead, and `--public off` removes it |
| With `--public`: `PUBLIC_DOMAIN`, `PUBLIC_TLS`, `ACME_EMAIL`, `NODE_IP`, `TURN_ALLOW_CIDRS`, `PROXY_HTTPS_PORT`, `PROXY_HTTP_PORT`, `TURN_TLS_PORT`, `TURN_UDP_PORT`, `LIVEKIT_TCP_PORT` | the name and what the environment gives; free ports; then, worked out from them on every run and not to be edited, the addresses the services are given (`PUBLIC_ORIGIN`, `ROOM_PUBLIC_URL`, `STREAM_URL`), `FORWARDED_FOR`, `TURN_ENABLED` and the proxy's and LiveKit's derived lines (`LIVEKIT_NODE_IP` is `NODE_IP` if set, else the A record of `PUBLIC_DOMAIN` read on that run, and `LIVEKIT_USE_EXTERNAL_IP` is always `false`: [LiveKit's public address](#livekits-public-address)) ([Publish a station from the Compose bundle](/docs/how-to/publish-with-compose/)) |

Any other line in the file is kept.
The containers and volumes carry the Compose project's name (`weavesignal` by default, `--project` names another).

## STUN servers

A WeaveSignal station contacts **no STUN server** unless its operator names one (ADR-0290).
A STUN server tells a program the public address it is seen from; whoever runs one sees that a program at that address asked.
Three parts of a station would otherwise ask one, and each is set not to:

| Part | What it would do | What WeaveSignal does |
|---|---|---|
| The desk's WebRTC connections | GStreamer's `webrtcsink` and `webrtcsrc` default to `stun://stun.l.google.com:19302`, so every connection the desk made asked Google's server | The desk sets `stun-server` itself on every WebRTC element it makes (`livekit.stun_servers`, empty by default); with none named, the element's own default is cleared |
| LiveKit, for its public address | With `use_external_ip: true` it asks Twilio's and Google's servers at start, even when `node_ip` is also set; with no `node_ip` it gives its own connections Google's | It is always started with an address of its own and `use_external_ip: false` ([LiveKit's public address](#livekits-public-address)) |
| Browsers joining the room | With no STUN server and no UDP TURN configured, LiveKit tells every client to use Twilio's and Google's, and no setting makes that list empty | The pages remove every STUN server LiveKit sends that the station did not name, whatever the TURN port is and even when the station's facts did not load |

LiveKit's list reaches any client that is not one of the station's pages (a LiveKit SDK in your own program, for instance) as LiveKit sends it: those programs are yours, and you decide what they ask.
Before 1.2.0 (up to and including 1.1.1) all three held: the desk asked Google's server for each connection, LiveKit asked Twilio's and Google's at start when `NODE_IP` was empty, and the pages passed on whatever LiveKit sent.
Where `NODE_IP` was set, LiveKit's own lookup did not run, but the desk and the pages still asked.

`just test-no-stun` checks this from what leaves the station, not from its settings.
It runs the Compose bundle with the example settings in a network namespace of its own and captures every packet that leaves it.
A person joins the room, so the desk really makes its WebRTC connections.
The test fails on any request to a STUN server, on any port, and on any lookup of a name with `stun` in it.
ICE connectivity checks between the desk and LiveKit are STUN messages too; they carry a user name and are told apart.
It then names a STUN server of its own in `STUN_SERVERS` and must see the station ask it, so a quiet first run cannot come from a capture that sees nothing.
It covers the Compose bundle on its own, with LiveKit's example address on loopback, not a public `NODE_IP`.
The public route, the phone line, Cloudron's package and browsers are not captured; the pages are checked only through what the station tells them (`relay.stun_servers`).

### Naming a STUN server

Set `STUN_SERVERS` to up to four `host:port`, separated by commas, in the station's settings: `/app/data/weavesignal.env` on Cloudron, the settings file on Compose and the Quadlets.
Each is a DNS name or an IPv4 address and a port, with no `stun:` in front: `stun.example.org:3478`.
The same text goes to the three parts: LiveKit tells browsers to use them, the pages are allowed to ask them (the control plane reports them to the pages as `relay.stun_servers`), and the desk uses the first (a `webrtcbin` takes one).
A server that is not in the right form stops the start (Cloudron) or `weavesignal.sh` (Compose and the Quadlets), naming it.
Restart the station after a change; on Compose and the Quadlets, run `weavesignal.sh` again first.

On Cloudron an admin can also name them from the studio (`PUT /api/v1/setup/stun`, with a list of `host:port`; an empty list is none).
The answer is written to `/app/data/setup.env` beside the stream password's and applies to all three parts at the next restart, over `weavesignal.env`, never to one alone; the setup's view (`GET /api/v1/setup`, `stun`) says what runs now (`in_use`), what was asked and whether a restart is waited for.
To go back to `weavesignal.env`, delete the `STUN_SERVERS=` line from `setup.env` and restart.
A line there that is not up to four `host:port` is ignored, and the log says so.
On Compose and the Quadlets the route answers 409: the settings file decides there.

You rarely need one.
A station with a public address does not: a guest behind a NAT reaches LiveKit's public address directly, or through TURN, without any STUN server.
Whichever server you name sees the station's address and when it asks, and nothing else.
Naming one does not make LiveKit ask it for its own address: that is `NODE_IP` or the name's A record ([LiveKit's public address](#livekits-public-address)).

These answered a STUN Binding request (RFC 8489: the reply carried the request's transaction ID and the magic cookie, and an `XOR-MAPPED-ADDRESS`) on 2026-11-18, twice, from one machine; an answer then is not a promise, and none of these projects has agreed to be listed here.

| Server | Run by | Source of that |
|---|---|---|
| `stun.nextcloud.com:443` and `stun.nextcloud.com:3478` | Nextcloud GmbH | Nextcloud Talk's own default (`DEFAULT_STUN_SERVER = 'stun.nextcloud.com:443'` in its source) and its documentation, which says the preconfigured STUN server is operated by Nextcloud GmbH. Port 443 gets through firewalls that block 3478 |
| `stun.freeswitch.org:3478` | The FreeSWITCH project | The project's own domain, and the STUN server in FreeSWITCH's default configuration (`conf/vanilla/vars.xml`) |

Only servers whose project could be confirmed are listed.
Google's and Twilio's are not on this list on purpose.
Pick one near your listeners, or run your own: the `coturn` package is free software and answers STUN on UDP and TCP 3478.

## LiveKit's public address

LiveKit puts one address in the candidates it offers a browser and the desk: the station's public IPv4 address.
From 1.2.0 it never asks a STUN server for it.
It is, in this order:

1. **`NODE_IP`**, if set: the address you give.
2. **The A record of the station's own domain**, read when the station starts. On Cloudron that is the room's domain, else the app's own; on Compose and the Quadlets, `PUBLIC_DOMAIN`, read each time `weavesignal.sh` runs (it is written to the generated settings as `LIVEKIT_NODE_IP`, and the Quadlets keep it until you run the script and generate them again). An address from a hosts file (loopback or link-local) is ignored.

An A record is not always the address guests reach.
Set `NODE_IP` to the server's public IPv4 address where:

- the name is behind a proxy that hides the server (Cloudflare's orange cloud, for example): the record is the proxy's, and WebRTC's UDP does not go through it;
- the machine is behind NAT and the name points at the router's public address: this works if the router forwards LiveKit's media port to the machine, and needs `NODE_IP` only if it does not;
- the name's record is private (a station on a private network): the address is used, `TURN_ALLOW_CIDRS` must list it (such as `192.168.1.5/32`) because LiveKit's TURN refuses to relay to private addresses, and `weavesignal.sh` says so.

When the name has no A record this machine can read: **Compose and the Quadlets** stop before writing anything, naming `NODE_IP`; **Cloudron** tries three times, then starts anyway (the stream, the chat and the studio need no address) and logs a warning. It uses the last address the name gave, kept in `/app/data/livekit-node-ip`, so a moment without DNS while a server restarts does not break calls. With no address kept it gives LiveKit the loopback address, and calls from outside do not connect until the name has an A record or `NODE_IP` is set, and the app is restarted.
The log's line `LiveKit's public address is … (…)` says which it used.

An existing station that left `NODE_IP` empty keeps working: its name already has the A record guests use.
On Compose and the Quadlets, run `weavesignal.sh` again on updating (it recomputes the address and the settings), and generate the Quadlets again; until then the old settings, which asked STUN servers, stay in force.

## The Quadlets

`deploy/quadlets/generate.sh` writes [the Quadlet route's](/docs/how-to/run-with-quadlets/) units from the Compose bundle and a settings file: a `.container` per service, a `.volume` per volume (named `<project>_<volume>`, as Compose names them), the services' settings files, and `<project>.target`.
The secrets become Podman secrets named `<project>-livekit-api-secret`, `<project>-listeners-source-password`, `<project>-postgres-password`, `<project>-control-admin-token`, `<project>-chat-email-key`, `<project>-control-email-key`, `<project>-control-token-key`, `<project>-ingest-token` and `<project>-desk-sources-password` (hosts' own studios: each given to its two units only), and `<project>-livekit-livekit-keys` (LiveKit's key and secret together, as LiveKit reads them).
