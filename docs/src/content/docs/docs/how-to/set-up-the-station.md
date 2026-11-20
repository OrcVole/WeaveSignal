---
title: "Set up the station and its look"
description: "Use the station's setup page to give it its name, the takedown contact that opens call-in, its jurisdiction, and its own colour, icon and banner on the public pages."
type: howto
status: draft
last_verified: 2026-11-17
sidebar:
  order: 30
---

> **From 1.0.1.**

A new station is called *This station* (1.1; earlier installs started as Kettle FM) and takes no calls.
Until it is named (a station still called *Kettle FM*, the old default, counts as not named while its setup is unfinished), its own address (`/` on the app's own domain on Cloudron, the one address on Compose) shows a short page that says the station is not set up yet and lists the first steps, in place of the player; the page goes as soon as the station has a name or setup is finished, with no restart. The public pages' domain on Cloudron keeps the player.
Its **setup page** changes that on the running station, with no file to edit and no restart (ADR-0237).
If you have not installed yet, start with the [install wizard](/docs/install-wizard/): it writes the settings file, the DNS records and the commands, and gives you an answers file for this page.

## Open the page

| Install | Address |
|---|---|
| Compose or Quadlets, public | `https://station.example/setup/` |
| Compose or Quadlets, this machine only | `http://127.0.0.1:7310/setup/` (the control plane's port) |
| Cloudron | `https://studio.station.example/setup/`: the app's own domain, never the public pages' |

Sign in with **the station key**, or as a member of staff whose role is **admin** ([Add staff and sign in](/docs/how-to/add-staff/)).

- Cloudron: it is `CONTROL_ADMIN_TOKEN` in `/app/data/secrets.env`. Open the app's File Manager and copy the value after the `=`.
- Linux with Compose or Quadlets: it is `CONTROL_ADMIN_TOKEN` in the settings file `weavesignal.sh` wrote: `deploy/compose/.env`, or the file you gave `--env-file` (the [Quadlets guide](/docs/how-to/run-with-quadlets/) uses `~/.config/weavesignal/station.env`). It is never printed. With Quadlets it is also the Podman secret `weavesignal-control-admin-token`, which `podman secret inspect --showsecret` shows.
- Any other install: it is the control plane's environment variable `WEAVESIGNAL_CONTROL_ADMIN_TOKEN`, which you or your installer set; it is never in the settings file. If it is not set, set it to a long random string and restart the control plane.

The not-set-up page says the same, beside the step that asks for the key, and shows this address for the studio app to connect to: copy it from the browser's address bar or from the page.

The page shows nothing of the station until you are signed in, and keeps your sign-in only while the tab is open: reload and it asks again. **Sign out** forgets it at once.

## Bring the wizard's answers

If you used the install wizard, choose **Open an answers file** and pick `weavesignal-setup.json`.
The page fills in the name, the contact, the jurisdiction and the colour.
Nothing is saved yet: check each part and save it.

## Name and takedown contact

1. Give the **station name** as listeners should read it.
2. Give the **takedown contact**: an email address or a link where a caller can ask for their call to be taken down. Call-in stays closed until there is one (ADR-0124). A role address that someone reads (`calls@…`) serves; whether your station must also name a person, and whom, is a step-by-step chart in [Decide whose contact to give callers](/docs/how-to/decide-whose-contact-to-give/).
3. Choose **Save name and contact**.

Both are in force at once: the player, the consent words, podcast feeds, show cards and the studio's view.
The live chat reads the name when it next starts.

Changing either changes the words callers agree to, so each caller is asked again; calls already agreed keep the words they were given.

What you save here wins over `STATION_NAME` and `CONSENT_CONTACT` in the settings file. Clear a box and save to go back to the file's value.

## Jurisdiction

Choose where the station answers to and **Save jurisdiction**.
It decides how long recordings, logs and chat are kept, and so what callers are told ([Set retention](/docs/how-to/set-retention/)).
The periods one by one, holds and Custom stay in the studio app, under Settings.

## The look

Your colour, icon and banner go on the player, the call-in panel, the guest page and the echo test.
The studio app keeps its own colours, so a desk reads the same at every station.

1. **Accent colour.** Pick it or type six hex digits, such as `#b4532a`, and choose **Save colour**. The two small players show it in the dark and the light theme; each theme uses the nearest shade of your colour that stays readable, and the words under the field say which.
2. **Brand icon.** Choose a PNG or a JPEG. It stands beside the station's name and becomes the icon of the browser tab and the phone's home screen.
3. **Banner.** Choose a PNG or a JPEG. It runs across the top of the player.

Each is live as soon as it is saved. **Remove** and **Use WeaveSignal's own** take them off again.
The sizes, and what the station does to a picture before it shows it, are in [The station's look](/docs/reference/branding/).

## Stream password (Cloudron)

On the Cloudron package the page can ask listeners for a password, for a station not ready to be heard.

1. Tick **Ask listeners for a password**, give a new password, and **Save stream password**.
2. Restart the app in Cloudron's dashboard: the change applies at the next start.

Listeners use the name `listener` and the password.
**The player page cannot play a stream with a password**: a browser does not ask for one for sound from another address. Radio apps and players can.

The Compose bundle has no stream password; the page says so.

## Finish

**Finish setup** stops the station offering setup.
The page stays at the same address for admins, to change the name, the contact or the look later.

A station whose settings file already gives it a name of its own and a takedown contact (the install wizard's file does, and so does a station upgraded from 1.0.0 with both set) is not offered setup at all; the page still opens for the look.

## What the page does not change

The station's names and DNS, its ports, video, mail and search listing are fixed when the station is installed.
The page shows them, and **How to change them** says where each is set.

Until the setup page is finished, the studio app shows **Finish the station’s setup** in its top bar to admins. It names what is still missing and copies the page's address to open in a browser (from 1.0.1).
