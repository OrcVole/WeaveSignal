---
title: "Packages and install files"
description: "Every package and install route the repository builds: the desktop packages and their files, the build settings, the updater's key, weavesignal.sh's settings file and the generated Quadlets."
type: reference
status: draft
last_verified: 2026-11-04
sidebar:
  order: 12
---

What the repository builds, where each piece lives, and where the 1.0.0 builds are published.

| What | Published at |
|---|---|
| The studio app's packages, with their `.sig` files and checksums | the assets of [release v1.0.0 on the Forgejo](https://forgejo.weavesignal.top/WeaveSignal/weavesignal/releases/tag/v1.0.2), and the updater's tree under `https://weavesignal.top/updates/` |
| The updater's feed | `https://weavesignal.top/updates/latest.json` |
| The server image the Compose bundle and the Quadlets run | `forgejo.weavesignal.top/weavesignal/weavesignal-server:1.0.2` |
| The edge and relay images | `forgejo.weavesignal.top/weavesignal/weavesignal-edge:1.0.2`, `forgejo.weavesignal.top/weavesignal/weavesignal-relay:1.0.2` |
| The Cloudron package's image | `forgejo.weavesignal.top/weavesignal/weavesignal-cloudron:1.0.2` ([Install on Cloudron](/docs/how-to/install-on-cloudron/)) |

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
| `LIVEKIT_API_SECRET`, `LISTENERS_SOURCE_PASSWORD`, `POSTGRES_PASSWORD`, `CONTROL_ADMIN_TOKEN`, `CHAT_EMAIL_KEY`, `CONTROL_EMAIL_KEY`, `CONTROL_TOKEN_KEY` | 40 random letters and digits each, made once, kept on later runs and never printed; secrets are never taken from the environment. The three keys key the chat's and the staff's email hashes and the API tokens ([The email key](/docs/reference/chat/#the-email-key), [Integrations](/docs/reference/integrations/#the-key)) |
| `CONTROL_PORT`, `CHAT_PORT`, `LISTENERS_PORT`, `SRT_INGEST_PORT`, `PLAYOUT_FEED_PORT`, `DESK_FEED_PORT`, `LIVEKIT_HTTP_PORT`, `LIVEKIT_MEDIA_PORT` | free ports ([Ports](/docs/reference/ports/#with-weavesignalsh)) |
| `STATION_NAME`, `ROOM`, `SERVER_IMAGE`, `LIVEKIT_API_KEY`, `MAIL_MODE`, `MAIL_FROM` | the defaults, or what the environment gives; `--image` sets `SERVER_IMAGE` |
| `CONSENT_CONTACT` | empty unless set: call-in stays closed until it is ([Open call-in](/docs/how-to/install-with-weavesignal-sh/#open-call-in)) |
| `DEMO_STILL` | the demo slate picture for a station left with the demo name, Kettle FM; empty for any other name; kept as written on later runs (ADR-0200) |
| `SEARCH_LISTING`, `DIRECTORY_LISTING` | `false` unless set (ADR-0133, ADR-0129) |
| With `--public`: `PUBLIC_DOMAIN`, `PUBLIC_TLS`, `ACME_EMAIL`, `NODE_IP`, `TURN_ALLOW_CIDRS`, `PROXY_HTTPS_PORT`, `PROXY_HTTP_PORT`, `TURN_TLS_PORT`, `TURN_UDP_PORT`, `LIVEKIT_TCP_PORT` | the name and what the environment gives; free ports; then, worked out from them on every run and not to be edited, the addresses the services are given (`PUBLIC_ORIGIN`, `ROOM_PUBLIC_URL`, `STREAM_URL`), `FORWARDED_FOR`, `TURN_ENABLED` and the proxy's and LiveKit's derived lines ([Publish a station from the Compose bundle](/docs/how-to/publish-with-compose/)) |

Any other line in the file is kept.
The containers and volumes carry the Compose project's name (`weavesignal` by default, `--project` names another).

## The Quadlets

`deploy/quadlets/generate.sh` writes [the Quadlet route's](/docs/how-to/run-with-quadlets/) units from the Compose bundle and a settings file: a `.container` per service, a `.volume` per volume (named `<project>_<volume>`, as Compose names them), the services' settings files, and `<project>.target`.
The secrets become Podman secrets named `<project>-livekit-api-secret`, `<project>-listeners-source-password`, `<project>-postgres-password`, `<project>-control-admin-token`, `<project>-chat-email-key`, `<project>-control-email-key`, `<project>-control-token-key` and `<project>-livekit-livekit-keys` (LiveKit's key and secret together, as LiveKit reads them).
