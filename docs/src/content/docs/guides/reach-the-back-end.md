---
title: "Reach the back end"
description: "When the studio app is not enough: the station's logs, its files, a terminal on it, and its API, on Cloudron and on your own server."
type: howto
status: draft
last_verified: 2026-11-12
sidebar:
  order: 9
---

When you finish, you will know how to look underneath the studio app: to read what the station is doing, change a file, or drive it from another program.
For everyday settings you do not need any of this: [Configure the station and the studio app](/guides/change-the-settings/) comes first.

## What is there

| You want to | Use |
|---|---|
| see why something failed | the station's **log** |
| change a setting the studio app does not have | the station's **files** |
| copy the station key, or run a command on the station | a **terminal** on the station |
| let another program drive the station | the station's **API**, with a token |

## On Cloudron

Open the Cloudron dashboard and choose the WeaveSignal app.

1. **Log.** Press **Logs**. Every part of the station writes here: the desk, playout, the chat. A setting the station cannot read is named in it.
2. **Files.** Press **File Manager** and open `/app/data/`. The station's settings are in `weavesignal.env`; a show set by file is in `station.toml`; the secrets the station made for itself, such as the station key, are in `secrets.env`. After a change, restart the app.
3. **Terminal.** Press **Terminal** for a command line inside the app. Read files there; change them in the File Manager.

The studio's own address (the one with `/setup/`) is behind Cloudron's login; the public pages are not.

## On your own server

1. **Log and status.** In the folder you installed from, `deploy/compose/weavesignal.sh --status` lists each part of the station and whether it runs; your container tool's `logs` command shows each part's log.
2. **Files.** The settings are in `deploy/compose/.env`. After a change, run `deploy/compose/weavesignal.sh` again: it changes only what you changed.

## The API

Everything the studio app does, it does through the station's API, so another program can do it too.

1. In the studio app, sign in as an admin and press **Settings**, then **API tokens**, and make a token for that program.
2. Give the program the token and the station's address. The list of what it can ask is at the station's address followed by `/api/v1/openapi.json`, and in [the API reference](/docs/reference/api/).
3. Remove the token in **Settings** when the program no longer needs it.

The **station key** (in `secrets.env`) also opens the API, with every admin right.
Keep it for emergencies and for the first sign-in; give programs a token instead.

## If it does not work

- **Cloudron sends you to its login page.** You used the studio's address without signing in to Cloudron; sign in, or use a token, which passes the login.
- **A file change did nothing.** The station was not restarted, or the line has a typo: the log names the setting it cannot read.
- **The API answers 401.** The token was removed, or it is for another station.

## More detail

[Settings](/docs/reference/settings/), [Control plane routes](/docs/reference/control-api/), [Install on Cloudron](/docs/how-to/install-on-cloudron/) and [Run with Compose](/docs/how-to/run-with-compose/).
