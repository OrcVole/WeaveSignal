---
title: "The station's look"
description: "The accent colour, brand icon and banner a station sets for its public pages: what each takes, how the colour is kept readable in both themes, what the station does to an uploaded picture, and the routes."
type: reference
status: draft
last_verified: 2026-11-10
sidebar:
  order: 8
---

> **From 1.0.1.** Not released yet.

A station can give its public pages its own look (ADR-0237): an **accent colour**, a **brand icon** and a **banner**.
An admin sets them on [the station's setup page](/docs/how-to/set-up-the-station/); they are kept in the station's database, so a backup of the database carries them.
With none set, the pages are WeaveSignal's own, unchanged.

## What is branded

| Page | Accent | Icon | Banner |
|---|---|---|---|
| The player, with call-in and the chat (`/`) | yes | yes, and the favicon | yes |
| The invite page (`/join/`) | yes | yes, and the favicon | no |
| The echo test (`/echo-test/`) | yes | yes, and the favicon | no |
| The setup page (`/setup/`) | no: it shows a preview | no | no |
| The studio app | no | no | no |

The studio app is not branded on purpose: its colours are a working vocabulary ([State words](/docs/reference/states/)), and a desk should read the same at every station.
Show cards and podcast feeds are not changed by the look either.

## The three parts

| Part | Takes | Stored as | Used for |
|---|---|---|---|
| Accent colour | one sRGB colour, six hex digits (`#b4532a`) | the six digits | links, the main button, the focus ring, the tick of a checkbox |
| Brand icon | PNG or JPEG, at most 1 MB, each side 64 to 4,096 pixels | PNG, fitted inside 256 × 256 on a transparent square, transparency kept, never enlarged | beside the station's name; the browser tab's icon and the home-screen icon |
| Banner | PNG or JPEG, at most 4 MB, at least 600 × 200 pixels, each side at most 8,192 | PNG, cropped about its middle to three to one, fitted inside 1,200 × 400, never enlarged | across the top of the player, above the station's name |

The icon and the banner are decorative: the station's name is always in text beside them, so neither has alternative text.

## The accent stays readable

The pages have a dark and a light theme and follow the listener's device.
One colour cannot read on both, so the pages never use your colour as given.
For each theme they keep its hue and its strength and move only its lightness, as little as possible, until:

- as link text it reads on each of the theme's three backgrounds at **4.5:1** or better (WCAG 2.1 AA, 1.4.3), which also clears the 3:1 a focus ring needs (1.4.11);
- the words on the main button read on it, and on its hover colour, at 4.5:1: the page takes dark or white words, whichever reads.

| You choose | Dark theme shows | Light theme shows |
|---|---|---|
| `#b4532a` | `#d16d45`, dark words | `#b15128`, white words |
| `#ffff00` | `#ffff00`, dark words | `#717100`, white words |
| `#000000` | `#898989`, dark words | `#000000`, white words |
| `#00a86b` | `#00a86b`, dark words | `#007d4e`, white words |

Only the accent and the words on it change. Text, state words, status colours and everything else stay the product's.
The setup page and the [install wizard](/docs/install-wizard/) show both shades before you save.

## What the station does to a picture

- **Only PNG and JPEG are read**, recognised by their first bytes, never by a file's name or declared type. **SVG is refused**: it is a document that can carry script.
- **The upload is never stored or served.** The station's media worker, a separate, restricted process, decodes it and writes a clean PNG; that PNG is what the database keeps and the pages show. No metadata survives: no location, no camera, no comment.
- **Sizes are checked twice**: the request is refused before it is read if it is over the cap (413), and the worker checks the picture's pixel size from its header before it decodes a pixel.
- A picture that is damaged, too small or not a picture is refused with a sentence saying which (422).

## Routes

All under `/api/v1`; the full description is in [the API reference](/docs/reference/api/).

| Route | Who | What |
|---|---|---|
| `GET /branding` | anyone | the accent and the addresses of the icon and the banner, each `null` until set |
| `GET /branding/icon`, `GET /branding/banner` | anyone | the PNG; `404` when none is set |
| `PUT /branding/accent` | an admin | set the colour, or clear it with an empty value |
| `PUT /branding/icon`, `PUT /branding/banner` | an admin | the request's body is the picture itself |
| `DELETE /branding/icon`, `DELETE /branding/banner` | an admin | remove it |
| `GET /setup`, `PUT /setup`, `POST /setup/finish` | an admin | the setup page's own: the name, the takedown contact, whether setup is still offered |
| `PUT /setup/stream-password` | an admin | the stream's password, where the install has one (the Cloudron package; `409` elsewhere) |

An admin is the station key or a signed-in member of staff whose role is admin. An API token is never an admin, so no token reaches the admin routes ([Integrations](/docs/reference/integrations/)).

A picture's address carries its version (`/api/v1/branding/icon?v=3f9a…`). Under that address it never changes, and a browser may keep it for a year; a new upload has a new address.

The pages ask `GET /branding` once at each load and remember the answer in the browser, so a return visit is branded from its first frame.
On Cloudron the public pages' domain passes the three `GET` routes and nothing else of these; the setup page and its routes are on the app's own domain only.

## Where it is kept

| Table | Holds |
|---|---|
| `station_setup` (migration 0028) | the name and the takedown contact set on the setup page, and when setup was finished |
| `station_branding` (migration 0029) | the accent, and each picture's PNG, size and version |
