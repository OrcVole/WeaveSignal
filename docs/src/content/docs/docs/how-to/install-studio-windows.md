---
title: "Install the studio app on Windows"
description: "Check the installer, get past Windows' unknown-publisher warning, install the studio app for your user, and remove it again."
type: howto
status: draft
last_verified: 2026-11-23
sidebar:
  order: 36
---

The studio app's Windows installer is `WeaveSignal_<version>_x64-setup.exe`, for 64-bit Windows 10 and 11 (ADR-0015).
It is an NSIS installer and it is not signed (ADR-0076).

## Download it

Download `WeaveSignal_1.3.2_x64-setup.exe` and the checksum file `SHA256SUMS-1.3.2-windows` from the assets of [release v1.3.2 on the Forgejo](https://forgejo.weavesignal.top/WeaveSignal/weavesignal/releases/tag/v1.3.2); the updater reads the same files from `https://weavesignal.top/updates/`.

Edge holds the installer at first: it says the file *isn't commonly downloaded*. To keep it:

1. In Edge's downloads, right-click the installer and choose **Keep**.
2. A window says *Make sure you trust WeaveSignal_<version>_x64-setup.exe before you open it*, with the publisher **Unknown**. Open the arrow beside **Delete** and choose **Keep anyway**.

To build the installer yourself instead, run `just package-windows` in the repository on Linux ([Packages and install files](/docs/reference/packages/)).

## Check the download

The installer is not signed, so its checksum is how you know it is the one the project published.
The release lists it in `SHA256SUMS-<version>-windows`.
In PowerShell, in the folder you saved it to:

```powershell
Get-FileHash .\WeaveSignal_1.3.2_x64-setup.exe -Algorithm SHA256
```

The `Hash` it prints must match the line for the installer, letter for letter (case does not matter).
If it does not, delete the file and do not run it.

## Get past "Windows protected your PC"

Because the installer carries no publisher signature, Microsoft Defender SmartScreen may stop it the first time:

1. Double-click the installer. A blue window says **Windows protected your PC** and names the publisher as **Unknown publisher**.
2. Choose **More info**. The window now shows the app's name and a **Run anyway** button.
3. Choose **Run anyway**.

If there is no **More info** link, your organisation's policy blocks unsigned programs; ask whoever manages the computer.
If you kept the download in Edge as above, the installer may simply open, with no blue window (Windows 11, 2026-11-15 and 2026-11-23: with Edge's **Keep anyway**, double-clicking the installer in Downloads opened it at once).

## Install

1. Past SmartScreen, the installer runs. It asks for no administrator's password: it installs the app for your user only, in your user's programs folder.
   Its pages are **Welcome**, **License Agreement** (the AGPL; choose **I Agree**), **Choose Install Location** (the default folder is inside your user folder; **Next**), the progress page (a few seconds), and **Completing WeaveSignal Setup**.
2. The last page has two boxes, both ticked: **Run WeaveSignal** and **Create desktop shortcut**. Choose **Finish**, and the app opens on its sign-in screen.
   Later, start **WeaveSignal** from the Start menu entry the installer added, or from the desktop shortcut.

The app draws its window with Microsoft Edge WebView2, which Windows 10 and 11 normally have.
If it is missing, the installer downloads Microsoft's WebView2 installer and runs it, which needs an internet connection.

## Connect to your station

The first time the app opens, it asks for your station's address and who you are.
Type the address as you would in a browser, with no path: `https://radio.example.org`.
Then give your username and password, or choose "Email me a code instead", or "Use the station key instead" (the first admin does, before any account exists).
An address nothing answers at gives *No station answered at that address.* under the password box.

- **https works for any host and any port**, such as `https://radio.example.org:8443`.
- **Plain `http://` works only for this computer and private networks**: `localhost`, `127.0.0.1`, `192.168.x.x`, `10.x.x.x`, `172.16.x.x` to `172.31.x.x`, link-local addresses, IPv6 `fc00::/7`, and `100.64.x.x` to `100.127.x.x`, which Tailscale gives each machine on a tailnet.
  A host name other than `localhost` is never counted as nearby over plain http, because a name can point anywhere and the app does not look names up. Use https for it.
  The reason: your password or key goes with every request, and it must not cross the internet in clear.
- The key or session goes into Windows Credential Manager, and the window never holds it.
  Signing out keeps the address, so the next sign-in only asks for your name and password.

## Sound: why the app is silent

The studio app makes no sound of its own, and has no setting for a microphone or speakers.
It shows the desk and sends your keys; you hear the station, and speak into the studio, from a browser on the same computer, through the audio devices the browser and Windows choose.
[Get your first sound out in ten minutes](/docs/tutorials/first-sound/) is the next step, and [Choose your microphone and speakers](/docs/how-to/choose-your-microphone-and-speakers/) says where the devices are chosen.

## Remove it

Open **Settings**, then **Apps**, then **Installed apps**, find **WeaveSignal** (it lists the version, 1.3.2 here), open the **…** menu and choose **Uninstall**, then **Uninstall** again.
The uninstaller opens a window with a box, **Delete the application data**, which is not ticked: tick it to remove the app's saved browser data as well.
If the app is still open, a message says *WeaveSignal is running! Click OK to kill it*: close the app first, or choose **OK**.
Without the box ticked, the folder `org.weavesignal.studio` stays in your user's `AppData\Local` (the window's saved data); the program folder, the Start menu entry and the desktop shortcut are gone.
Whether the saved sign-in in Windows Credential Manager goes too was not tested.

Verified on Windows 11 (build 26100, en-GB) · 2026-11-15: the 1.0.4 installer downloaded with Edge and kept as above, its checksum matched, it installed for the user with no administrator prompt and no SmartScreen window, and the app signed in to a station with its key, which Credential Manager kept across a restart.
The installer is cross-built on Linux for the GNU target with NSIS 3.11, and carries the app and Microsoft's `WebView2Loader.dll`.

Verified on Windows 11 (build 26100, en-GB) with Edge 158 · 2026-11-23: the 1.3.0 installer was downloaded with Edge from the release page, kept as above, and its SHA-256 matched `SHA256SUMS-1.3.1-windows`; double-clicking it opened the installer with no SmartScreen window; it installed for the user in about 6 seconds (WebView2 was already present, so no download); the app opened on its sign-in screen and, for an address nothing answers at, said *No station answered at that address.*; the Start menu entry and the desktop shortcut were there (a second install, with the boxes left ticked); it uninstalled from **Settings › Apps** as above, which removed the program folder, the Start menu entry and the desktop shortcut.
Not run: a sign-in to a real station on 1.3.0 (the 1.0.4 run above did), SmartScreen's blue window, an installer on a computer without WebView2, Windows 10, and the app updating itself.
