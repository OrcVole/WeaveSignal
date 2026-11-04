---
title: "Update the studio app, or roll it back"
description: "How the studio app finds and installs a signed update, why it waits for a quiet desk, and how to go back to the version before: in the app for an AppImage, by hand for the other packages. For the person who publishes releases: the update feed."
type: howto
status: draft
last_verified: 2026-11-04
sidebar:
  order: 36
---

The desktop studio app (the AppImage, `.deb`, `.rpm` or Windows installer) checks a signed feed for a newer version (ADR-0015).
The project's packages read `https://weavesignal.top/updates/latest.json`, on the project's own domain (ADR-0140).
It never installs anything by itself.

> **Not yet run on a real install.** The feed, its signatures and the update screen are tested; an update and a rollback on an installed copy are [planned](/reference/release-one-status/).
> A package built without the project's update key (your own `just package-desktop` without one) never updates itself; **Settings > Updates** then says so.

## Find and install an update

1. About 20 seconds after the app opens, it looks at the feed once.
   If a newer version is out, a button such as **Update 1.0.1 available** appears in the frame.
   It opens Settings; nothing else happens.
2. **Settings > Updates** (panel `settings.updates`) shows the version, its date and its notes.
   **Check again** (or **Check now**, when you have the latest) looks at the feed at any time.
3. **Download and install** downloads the package, checks its signature against the key built into your copy of the app, installs it and restarts the app.
   A package that does not carry the project's signature for that exact version is thrown away and never installed.

What the install does depends on how you installed the app:

| Installed as | The update |
|---|---|
| AppImage | replaces the AppImage file in place, then the app restarts as the new file |
| `.deb` or `.rpm` | asks for an administrator's password (a system prompt, then `dpkg` or `rpm`), then the app restarts |
| Windows installer | starts the new installer, which closes the app and reopens it |

## It waits while the station is on

The app does not download, install or roll back while:

- a strip is open on the desk, that is, a source whose fader is open and not muted (listeners hear it, or the room does while playout is down), or
- a talk key is held.

The panel says which, in words, and the button is off until the desk is quiet.
It asks again after the download, so a show that starts while the file comes down is not interrupted: the file waits, and **Install and restart** is offered once nothing is open.
Just before an install or a rollback restarts the app, the app also asks the station for the desk's state itself, so a screen that has fallen behind cannot let one through; a held talk key is known only to the screen.

When the app cannot see the desk (no connection to the station yet), it cannot tell, so it says so and lets you go ahead.
A desk that is down is not on air.

## Roll back

A rollback goes back to the version you had before the last update.
**The feed cannot do it for you:** the app only ever accepts a version newer than its own, and it rejects a package signed for a different version than the feed announces, so serving an older release does not move anyone backwards (this is on purpose; it also stops an attacker passing off an old, flawed release as a new one).

**AppImage.**
Before it replaces itself, the app copies the running file (the AppImage you started) aside, and keeps the copy once the update has installed: `~/.local/share/org.weavesignal.studio/previous.AppImage`, as large as the AppImage.
If the update fails, the copy kept by an earlier update stays.
**Settings > Updates** then offers **Roll back to 1.0.0** (the version you had), which puts that copy back and restarts.
It is refused while a strip is open, like an update.
The version you left is not announced in the frame again; Settings still offers it, and a newer one is announced as usual.
Only one earlier version is kept.

**`.deb`, `.rpm` and Windows.**
The system's package tools replace these, so the app keeps nothing.
Install the earlier package over the new one, from its release's folder on the update host (`https://weavesignal.top/updates/<version>/`; every release stays there), after checking its SHA-256 as the [install page](/how-to/install-studio-linux/#check-the-download) says:

```sh
sudo apt install --allow-downgrades ./WeaveSignal_1.0.0_amd64.deb     # Debian, Ubuntu
sudo dnf downgrade ./WeaveSignal-1.0.0-1.x86_64.rpm                   # Fedora
```

On Windows, run the earlier `WeaveSignal_1.0.0_x64-setup.exe`.
Not tried on Windows yet ([planned](/reference/release-one-status/)).

## Publish a release

For the person who builds the packages.
The feed is a folder of static files that any web server can serve; the project's own domain serves it (`https://weavesignal.top/updates/`, ADR-0140), and no other server is needed.

1. Build and sign both platforms' packages with the updater key set (`just package-desktop`, `just package-windows`; see [Packages and install files](/reference/packages/)).
2. Make the feed tree from the signed packages in `dist/desktop/`:

   ```sh
   just package-feed --base-url https://weavesignal.top/updates --notes release-notes.txt
   ```

   This writes `dist/updates/latest.json` and a folder `dist/updates/<version>/` with each package, its `.sig` and its checksums.
   Folders of earlier releases stay.
3. Upload the new `<version>/` folder first and `latest.json` last, so nobody is told about files that are not there yet.
4. The address `--base-url` plus `/latest.json` is the `WEAVESIGNAL_UPDATER_ENDPOINT` the packages were built with; it cannot change for an installed copy, so move the host behind the same address.

The script refuses a release whose packages lack a `.sig`, whose signature was made for another version, that lacks a platform (a copy of the app on that platform would show an error instead of "up to date"; `--allow-missing` says it is deliberate), or that would move `latest.json` backwards.

**Pulling a bad release.**
Run the script again for the last good version with `--allow-older`: copies that have not updated stop being offered the bad one.
Copies that already installed it are not moved back by the feed; publish a fixed release with a higher version number (a rebuild of the good code under a new version is signed like any other), or have them roll back as above.
