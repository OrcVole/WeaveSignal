---
title: "Install a station with weavesignal.sh"
description: "Install, update and remove a station with the Compose bundle in one command: secrets made locally, free ports chosen, the data kept on uninstall."
type: howto
status: draft
last_verified: 2026-11-10
sidebar:
  order: 33
---

> **Loopback only, unless you ask.** `weavesignal.sh` installs the Compose bundle, which keeps every listener on loopback.
> `--lan` lets a phone or another computer on your home network **listen**: [Listen on a phone or another computer](/docs/how-to/listen-on-another-device/).
> `--public station.example` also puts a reverse proxy with a certificate in front of it and turns on TURN over TLS, so people can listen, call and speak: [Publish a station from the Compose bundle](/docs/how-to/publish-with-compose/).

> **New to this?** The [install wizard](/docs/install-wizard/) asks the questions on this page and the next and gives you the settings file and the commands (from 1.0.1).

`weavesignal.sh` is the Compose route's installer (ADR-0022).
It finds Podman or Docker, makes the station's secrets on your machine, picks free ports (ADR-0075), writes the settings file, starts the bundle, and tells you where to sign in.
Running it again changes nothing that is already set.

## Before you start

- **Podman 5 with `docker-compose` as its compose provider**, or **Docker Engine with the Compose plugin**.
  On Fedora, `dnf install docker-compose`; on Ubuntu, `apt install docker-compose-v2`.
  On **Bazzite** and other image-based Fedora systems (Silverblue, Kinoite), `dnf` does not install programs and Podman comes without a compose provider: `podman compose version` says *looking up compose provider failed*. Install it with Homebrew, which Bazzite carries (other image-based systems: only if you have installed Homebrew), and which needs no `sudo`: `brew install docker-compose` (8 seconds, 31 MB, checked on Bazzite 45). Homebrew puts `docker-compose` on your `PATH`, and `podman compose` finds it there.
  The script never uses `sudo`.
- **Not `podman-compose`.** It ignores the settings file the bundle gives each service (the top-level `configs`), so the station would start without them.
  The script refuses it, says why, and names the fix: install `docker-compose`, which `podman compose` then prefers, or point Podman at it with `PODMAN_COMPOSE_PROVIDER=/path/to/docker-compose`, or use Docker.
  When Podman has no compose provider at all, the script says so before it pulls anything and gives the line for each system, the Bazzite one first (`brew install docker-compose`).
- On Podman, `docker-compose` reaches Podman through your user's API socket. From 1.3.1 the script looks at it before it pulls the images and, if it is not running, turns it on for you as your user's own service (`systemctl --user enable --now podman.socket`, no `sudo`), and says so; if that fails, it prints the command to run. Before 1.3.1 the install stopped after the pull with *failed to connect to the docker API at unix:///run/user/…/podman/podman.sock*, and the same command put it right.
- `ss` (iproute2), `curl` and `bash`, which most Linux systems have.
- The repository, checked out where it will stay: the script and the bundle are in `deploy/compose/`, and the bundle mounts playout's scripts (`playout/`) and the default loop (`assets/default-loop/`) from it. Get it with `git clone https://forgejo.weavesignal.top/WeaveSignal/weavesignal.git` (60 MB, about 7 seconds), then `cd weavesignal`; the clone is at the latest release, and `git checkout v1.3.0` names one.
- **A server that is not overloaded.** Live audio needs CPU on time: on a busy shared server the listener stream runs dry ([Memory and CPU](/docs/reference/resources/#what-this-means-for-a-host)).

### The server image

The bundle runs the published server image, `forgejo.weavesignal.top/weavesignal/weavesignal-server:1.3.1`.
The script pulls it when it is not on the machine.

To run an image built from the repository instead, build it, then give its tag with `--image`:

```sh
podman build -t localhost/weavesignal-desk-spike:dev spike/desk-image
just desk-image server-image
deploy/compose/weavesignal.sh --image localhost/weavesignal-server:m1
```

The first line builds the base image that `just desk-image` builds on.
`just server-image` tags `localhost/weavesignal-server:m1`; set `WEAVESIGNAL_SERVER_IMAGE` to build another tag.
The image is kept in the settings file (`SERVER_IMAGE`), so later runs use it without `--image`.

## Install

From the repository:

```sh
deploy/compose/weavesignal.sh
```

The script:

1. Uses Podman if `podman compose` works through `docker-compose`, else Docker with `docker compose`, and says which. `--engine podman` or `--engine docker` chooses.
2. Makes the station's identifier and its secrets from the kernel's random source: LiveKit's API secret, the listener source password, PostgreSQL's password, the station key (`CONTROL_ADMIN_TOKEN`), and three keys: the chat's email key, the staff email key and the API token key.
3. Gives each port its default, or the next suggested alternative if the default is taken, and says so ([Ports](/docs/reference/ports/#with-weavesignalsh)).
4. Writes `deploy/compose/.env` with mode 600. The secrets are written there and nowhere else, and are never printed.
5. Starts the bundle, waits up to 90 s for the control plane and its database, and prints the addresses.

On a first run with Podman it pulls the images first (about 2.5 GB in four images, a minute on a fast line) and `podman compose` prints a line saying it runs an external compose provider, `docker-compose`: that is normal. A first install on a clean Bazzite machine, with `docker-compose` and the Podman socket already in place, took 99 seconds from `git clone` to the control plane answering, images pulled.

It ends like this:

```text
WeaveSignal is running (project 'weavesignal', podman).

  Control plane   http://127.0.0.1:7310/   (health: /health)
  Chat            http://127.0.0.1:7311/
  Listeners       http://127.0.0.1:7312/programme.opus
  Live room       ws://127.0.0.1:7880  (media on UDP 7882)

Sign in: the studio uses the control plane's address above with the admin token, which is
CONTROL_ADMIN_TOKEN in /home/demo/weavesignal/deploy/compose/.env (mode 600). It is not shown here.
Set the station up: open http://127.0.0.1:7310/setup/ and sign in with the same key.
The page takes the station's name, the takedown contact that opens call-in, the jurisdiction
and the station's look.
Every listener but LiveKit's media port is on loopback: nobody off this machine can join or
listen. To let a phone or another computer on your own network listen, run this again with --lan;
to publish the station, run it with --public and the name that reaches this machine.
```

The station key is not printed; to see it, run `grep CONTROL_ADMIN_TOKEN deploy/compose/.env` in the repository and copy what follows the `=`.

With the phone line on, it adds a line saying where its SIP and media ports listen: they face
the phone provider, so they are not on loopback (`PHONE_SIP_LISTEN` narrows SIP to one address).

The SRT link (7313), the playout feed (7315), the desk's feed (7317), and the ports above the playout feed and above the listener port are internal: the script checks they are free but never shows or publishes them.

Sign in to the studio app with the station key, then add yourself as staff: [Add staff and sign in](/docs/how-to/add-staff/).
Until the station is named, the control plane's address (`/`) shows a page that says the station is not set up yet and lists these steps, in place of the player (1.1); it goes as soon as the station has a name or its setup is finished. A station still called *Kettle FM* (the old default) with its setup unfinished shows it too; a station that already has a name of its own keeps its player.

From 1.0.1 the script also names the station's **setup page** (`/setup/` on the control plane's address): sign in there with the same key to give the station its name, its takedown contact and its look, with no file to edit ([Set up the station and its look](/docs/how-to/set-up-the-station/)).

### Open call-in

Call-in stays closed until the station says how a caller can ask for their call to be taken down (ADR-0124).

1. In `deploy/compose/.env`, set `CONSENT_CONTACT` to an email address or a link, such as `CONSENT_CONTACT=calls@station.example`.
2. Run `deploy/compose/weavesignal.sh` again.

The player page then offers call-in, and the consent words name that contact.
From 1.0.1 the [setup page](/docs/how-to/set-up-the-station/#name-and-takedown-contact) sets it on the running station instead, with no second run.

From 1.2 the station may write its own consent words as `CONSENT_TEXT` in the same file, then run the script again; empty keeps the default words.
Write them in single quotes with each apostrophe as `\'`, such as `CONSENT_TEXT='You\'ll be live with {presenter} on {station}. To have it taken down, contact {contact}.'`, and leave out backslashes, dollar signs and per cent signs, which the script refuses.
[The consent words](/docs/reference/public-pages/#the-consent-words) lists the fill-ins; changing the words asks every caller to agree again.

### Choose ports yourself

Give any port in the environment:

```sh
CONTROL_PORT=17310 LIVEKIT_MEDIA_PORT=52000 deploy/compose/weavesignal.sh
```

If a port you give is taken, the script stops, names it and suggests a free alternative.
The names are those in `deploy/compose/.env.example`: `CONTROL_PORT`, `CHAT_PORT`, `LISTENERS_PORT`, `SRT_INGEST_PORT`, `PLAYOUT_FEED_PORT`, `DESK_FEED_PORT`, `DESK_SOURCES_PORT`, `LIVEKIT_HTTP_PORT` and `LIVEKIT_MEDIA_PORT`.

### Hosts' own studios

A host can send their show from their own software ([Set up a host in their own studio](/docs/how-to/set-up-a-host-in-their-own-studio/)).
The station takes it on ports of its own, all off until you set them:

```sh
SOURCE_ICECAST_PORT=8005 SOURCE_SRT_PORT=9710 SOURCE_WHIP_PORT=9712 deploy/compose/weavesignal.sh --public station.example
```

`SOURCE_ICECAST_PORT` is TCP (8005, or 18005, 28005), `SOURCE_SRT_PORT` is UDP (9710, or 19710, 29710) and `SOURCE_WHIP_PORT` is UDP (9712, or 19712, 29712); 0 turns any of them off.
WHIP, for hosts sending from OBS, takes its offer through the public route, so it needs `--public`; it sends OBS to LiveKit's public address (`NODE_IP`, or the A record) ([Send from OBS over WHIP](/docs/how-to/send-from-obs-over-whip/)).
The script checks a port you set as it checks the others, keeps it, and names it in what to open.
The `ingest` service listens on them only while a show key exists: on every address on a public station, on loopback otherwise.
The station's rules show these ports and cannot change them: change them here and run the script again.
The script also makes two secrets for this, `INGEST_TOKEN` (the ingest's and the control plane's only) and `DESK_SOURCES_PASSWORD` (the ingest's and the desk's only).

### A graphics card for video

With `VIDEO=true` the desk encodes the picture. From 1.2 the script looks for an NVIDIA card's CDI spec on a new install whose settings file has no `GPU` line (or on any station that says `GPU=auto`), writes `GPU=nvidia` or `GPU=none`, and with `nvidia` hands the card to the desk (`gpu.yaml`).
From 1.3.0 an update never moves a station to the card: a station installed before, with no `GPU` line, gets `GPU=none`, and the script and the studio app's Settings say the card is there (`GPU_FOUND`).
`VIDEO_ENCODER` (`auto`, `nvenc`, `svt`) chooses the encoder: see [Use a GPU for video](/docs/how-to/use-a-gpu-for-video/).

### The rehearsal studio

With `REHEARSAL=true` (from 1.3.0; off by default, `true` or `false`) the script adds `rehearsal.yaml`, a `desk-rehearsal` service, and turns the rehearsal studio on in the control plane.
It opens no port, so the script has nothing to check for it ([The rehearsal studio](/docs/reference/rehearsal-studio/#turning-it-on)).

### Settings only

`--no-start` writes the settings file and stops there (and changes nothing about start at boot); the [Quadlet route](/docs/how-to/run-with-quadlets/) starts from it.

### Publish it

`--public station.example` is the public route, in [its own page](/docs/how-to/publish-with-compose/); `--public off` takes it away, and `--renew-turn` restarts LiveKit after the proxy has renewed the certificate.

## Update

1. Give the new image: `--image` with its name, or `SERVER_IMAGE` in the settings file (for an image you build, rebuild it under the same tag).
2. Run the script again.

It keeps the settings file's secrets and ports, says `Settings unchanged`, and recreates only the containers whose image or settings changed.

## Start again after a reboot

The bundle's containers restart unless stopped, and with Docker they come back at boot because Docker starts with the machine.
Podman restarts nothing at boot by itself, so on Podman the script turns two things on after the station is up (ADR-0138), and says which it changed:

- **lingering** for your user (`loginctl enable-linger`), so your user's services run without a login session;
- your user's **`podman-restart.service`** (`systemctl --user enable podman-restart.service`), which starts the containers again after a reboot.
  It is enabled, not started: the station is already up, and starting it now would also start your other stopped containers that have a restart policy.

```text
Start at boot (Podman, user kettle):
  turned on lingering (loginctl enable-linger): your services now run without a login session
  enabled podman-restart.service (systemctl --user enable): the containers start again after a reboot
  --no-boot-start skips both; --uninstall leaves them as they are.
```

Something already on is left as it was and reported (`lingering was already on`).
If a step is refused (turning on lingering can need an administrator; the script never waits at a password prompt for it), the script says so, prints the command to run by hand, and finishes the install: the station is already up.
Without lingering, `podman-restart.service` runs only when you log in, and the script says the containers start again at your next login rather than at boot.
Docker is untouched.

`--no-boot-start` skips both steps:

```sh
deploy/compose/weavesignal.sh --no-boot-start
```

Neither step is undone by `--uninstall`, because other containers of yours may rely on them; turn them off by hand (`systemctl --user disable podman-restart.service`, `loginctl disable-linger`) if you want.
Or run the station as [Quadlets](/docs/how-to/run-with-quadlets/), which systemd starts.

## See the station

```sh
deploy/compose/weavesignal.sh --status
```

## Remove it

```sh
deploy/compose/weavesignal.sh --uninstall
```

This stops and removes the containers and keeps the data volumes (the database, carts, library, recordings) and the settings file, so running the script again brings the same station back.

To delete everything, including the database and the secrets:

```sh
deploy/compose/weavesignal.sh --uninstall --purge --yes
```

Without `--yes`, a terminal asks you to type `purge`.

## More than one station on a machine

Give each its own project name, settings file and ports:

```sh
deploy/compose/weavesignal.sh --project kettle --env-file ~/stations/kettle.env
```

The volumes are named after the project (`kettle_postgres-data`), so stations never share data.

## If the database never turns healthy

On Podman, in a session whose OOM score adjustment is above 0 (`cat /proc/self/oom_score_adj`), a container Compose made with an adjustment of 0 cannot be entered with `podman exec`, and PostgreSQL's healthcheck never passes.
The script sets `WEAVESIGNAL_OOM_SCORE_ADJ` to your session's value, which the bundle gives every service, so it needs nothing from you.
Export your own value first to choose another.
Station containers made by an older bundle keep their old value: run `--uninstall` (the data stays) and install again.
When you run `podman compose` by hand instead, see [Run with Compose](/docs/how-to/run-with-compose/#if-up-fails-because-postgresql-never-turns-healthy).

## How it is tested

`just test-install` runs the script end to end on its own project and ports: a taken port refused with an alternative, a taken default moved to its alternative, the install answering, no secret in the output, the settings file mode 600, a second run unchanged, `--uninstall` keeping the database, and `--purge` leaving no container, volume, network or setting.
It passes `--no-boot-start`, and `just boot-check` tests start at boot with `loginctl`, `systemctl` and the engines stubbed, so neither changes anything on the machine it runs on.
Both are part of `just ci`, which runs on Podman 5.8 with docker-compose.
The public route has also run on a dedicated public server with rootless Podman and docker-compose (2026-11-03).
Docker Engine has not been tried.
