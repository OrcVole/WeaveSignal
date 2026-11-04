---
title: "Install the studio app on Windows"
description: "Check the installer, get past Windows' unknown-publisher warning, install the studio app for your user, and remove it again."
type: howto
status: draft
last_verified: 2026-11-04
sidebar:
  order: 36
---

> **Not run on Windows yet.** The installer has been built on Linux and inspected, but installing it on a Windows machine is [planned](/docs/reference/release-one-status/).

The studio app's Windows installer is `WeaveSignal_<version>_x64-setup.exe`, for 64-bit Windows 10 and 11 (ADR-0015).
It is an NSIS installer and it is not signed (ADR-0076).

## Download it

Download `WeaveSignal_1.0.0_x64-setup.exe` and the checksum file `SHA256SUMS-1.0.0-windows` from the assets of [release v1.0.0 on the Forgejo](https://forgejo.weavesignal.top/WeaveSignal/weavesignal/releases/tag/v1.0.0); the updater reads the same files from `https://weavesignal.top/updates/`.

To build the installer yourself instead, run `just package-windows` in the repository on Linux ([Packages and install files](/docs/reference/packages/)).

## Check the download

The installer is not signed, so its checksum is how you know it is the one the project published.
The release lists it in `SHA256SUMS-<version>-windows`.
In PowerShell, in the folder you saved it to:

```powershell
Get-FileHash .\WeaveSignal_1.0.0_x64-setup.exe -Algorithm SHA256
```

The `Hash` it prints must match the line for the installer, letter for letter (case does not matter).
If it does not, delete the file and do not run it.

## Get past "Windows protected your PC"

Because the installer carries no publisher signature, Microsoft Defender SmartScreen stops it the first time:

1. Double-click the installer. A blue window says **Windows protected your PC** and names the publisher as **Unknown publisher**.
2. Choose **More info**. The window now shows the app's name and a **Run anyway** button.
3. Choose **Run anyway**.

If there is no **More info** link, your organisation's policy blocks unsigned programs; ask whoever manages the computer.
Your browser may also warn when downloading; choose to keep the file.

## Install

1. Past SmartScreen, the installer runs. It asks for no administrator's password: it installs the app for your user only, in your user's programs folder.
2. When it finishes, start **WeaveSignal** from the Start menu entry it added.

The app draws its window with Microsoft Edge WebView2, which Windows 10 and 11 normally have.
If it is missing, the installer downloads Microsoft's WebView2 installer and runs it, which needs an internet connection.

## Connect to your station

The first time the app opens, it asks for your station's address and who you are.
Type the address as you would in a browser, with no path: `https://radio.example.org`.
Then give your username and password, or choose "Use the station key instead" (the first admin does, before any account exists).

- **https works for any host and any port**, such as `https://radio.example.org:8443`.
- **Plain `http://` works only for this computer and private networks**: `localhost`, `127.0.0.1`, `192.168.x.x`, `10.x.x.x`, `172.16.x.x` to `172.31.x.x`, link-local addresses, IPv6 `fc00::/7`, and `100.64.x.x` to `100.127.x.x`, which Tailscale gives each machine on a tailnet.
  A host name other than `localhost` is never counted as nearby over plain http, because a name can point anywhere and the app does not look names up. Use https for it.
  The reason: your password or key goes with every request, and it must not cross the internet in clear.
- The key or session goes into Windows Credential Manager, and the window never holds it.
  Signing out keeps the address, so the next sign-in only asks for your name and password.

## Remove it

Open **Settings**, then **Apps**, then **Installed apps**, find **WeaveSignal**, and choose **Uninstall**.

Verified on a development workstation · 2026-10-26: the installer was cross-built on Linux for the GNU target with NSIS 3.11, and it carries the app and Microsoft's `WebView2Loader.dll`; the app links only Windows' own libraries besides it.
It was not run on Windows, so the SmartScreen steps above follow Microsoft's documented behaviour, not a test.
