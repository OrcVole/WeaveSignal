---
title: "Change the station's settings"
description: "Where each of the station's settings lives: the studio app, the station's setup page, and, for the rest, the station's settings file."
type: howto
status: draft
last_verified: 2026-11-11
sidebar:
  order: 8
---

When you finish, you will know where to change any of the station's settings.
There is no single "back end": settings live in three places, and you try them in this order.

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

[Set up the station](/docs/how-to/set-up-the-station/), [Settings](/docs/reference/settings/) and [Install on Cloudron](/docs/how-to/install-on-cloudron/).
