---
title: "Configure the station and the studio app"
description: "Where each setting lives: the studio app's own, the studio's Settings tab, the station's setup page, and, for the rest, the station's settings file."
type: howto
status: draft
last_verified: 2026-11-12
sidebar:
  order: 8
---

When you finish, you will know where to change any setting, the studio app's own included.
There is no single "back end": the station's settings live in three places, and you try them in this order.
To look underneath them (the logs, the files, the API), see [Reach the back end](/guides/reach-the-back-end/).

## The studio app itself

These belong to the app on your computer, not to the station:

- **Which station it talks to.** The **Station address** is asked when you sign in. To change it, press your name at the top right, choose **Sign out**, and sign in with the new address.
- **Who you are.** The same button shows your name and role; **Sign out** there lets someone else sign in, and **Change your password** is beside it.
- **Text size.** On a window bigger than 1,920 × 1,080 everything grows with the window by itself. **A−** and **A+** at the top right, or `Ctrl` with `-`, `+` and `0`, make it larger or smaller on top of that.
- **Theme.** **Dark**, **Light** or **Night** (warm, for late shows), beside the text size.
- **Updates.** **Settings**, then **Updates**.

The app keeps the station's address in a folder of its own (on Linux `~/.config/org.weavesignal.studio`, on Windows `%APPDATA%\org.weavesignal.studio`); your sign-in is kept in the computer's keychain.
You never need to edit these files.

## 1. The studio app: Settings

Most day-to-day settings are in the studio app.
Sign in as an admin, then press **Settings** at the right of the tabs.

There you will find:

- **Staff**: add people, give them a role, reset a password.
- **Station time zone**: the station's clock, for the schedule.
- **Retention and holds**: how long recordings and messages are kept.
- **API tokens**, **Send to another Icecast server** (such as an AzuraCast) and **RTMP out** (video platforms), for linking the station to other services.
- **Your show**: your own on-air name and your show.
- **Updates**: a new version of the studio app.
- **Chat pictures**: whether chatters may upload a picture of their own as their avatar.

Shows, their times and their cards are on the **Schedule** tab, not in Settings.

## 2. The station's setup page

The station's name, its contact address, its colours, icon and banner, and (on Cloudron) a password on the stream are on its **setup page**.
Open it in a web browser, not in the studio app, and sign in as an admin:

- on Cloudron: your studio's address with `/setup/` at the end, such as `https://studio.example.org/setup/`;
- on your own server: the station's address with `/setup/` at the end.

Changes there take effect at once, with no restart.

## 3. The settings file

Everything else (ports, mail, listener passwords, the delay, the default loop) is in the station's settings file.
You rarely need it.

On **Cloudron**:

1. Open the Cloudron dashboard and choose the WeaveSignal app.
2. Open its **File Manager**.
3. Open `weavesignal.env` in `/app/data/`, change the line you need, and save.
4. **Restart** the app.

The secrets the station made for itself, such as the station key, are in `secrets.env` beside it.
Copy them out when you need them; do not change them.

On **your own server**, the file is `deploy/compose/.env` in the folder you installed from.
After an edit, run `deploy/compose/weavesignal.sh` again: it changes only what you changed.

## If it does not work

- **Settings is missing some sections.** You are not signed in as an admin. Ask an admin, or sign in with the station key.
- **The setup page asks you to sign in to Cloudron.** That is expected: it is the studio's address, behind Cloudron's login.
- **A file change did nothing.** The app was not restarted, or the line has a typo: the app's log names a setting it cannot read.

## More detail

[Set up the station](/docs/how-to/set-up-the-station/), [Settings](/docs/reference/settings/), [Install on Cloudron](/docs/how-to/install-on-cloudron/) and [Reach the back end](/guides/reach-the-back-end/).
