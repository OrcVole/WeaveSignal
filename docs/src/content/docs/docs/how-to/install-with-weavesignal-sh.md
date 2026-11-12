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
> `--public station.example` also puts a reverse proxy with a certificate in front of it and turns on TURN over TLS: [Publish a station from the Compose bundle](/docs/how-to/publish-with-compose/).

> **New to this?** The [install wizard](/docs/install-wizard/) asks the questions on this page and the next and gives you the settings file and the commands (from 1.0.1).

`weavesignal.sh` is the Compose route's installer (ADR-0022).
It finds Podman or Docker, makes the station's secrets on your machine, picks free ports (ADR-0075), writes the settings file, starts the bundle, and tells you where to sign in.
Running it again changes nothing that is already set.

## Before you start

- **Podman 5 with `docker-compose` as its compose provider**, or **Docker Engine with the Compose plugin**.
  On Fedora, `dnf install docker-compose`; on Ubuntu, `apt install docker-compose-v2`.
  The script never uses `sudo`.
- **Not `podman-compose`.** It ignores the settings file the bundle gives each service (the top-level `configs`), so the station would start without them.
  The script refuses it, says why, and names the fix: install `docker-compose`, which `podman compose` then prefers, or point Podman at it with `PODMAN_COMPOSE_PROVIDER=/path/to/docker-compose`, or use Docker.
- On Podman, `docker-compose` reaches Podman through your user's API socket. If `podman compose` cannot connect, turn the socket on with `systemctl --user enable --now podman.socket`.
- `ss` (iproute2), `curl` and `bash`, which most Linux systems have.
- The repository, checked out where it will stay: the script and the bundle are in `deploy/compose/`, and the bundle mounts playout's scripts (`playout/`) and the default loop (`assets/default-loop/`) from it.
- **A server that is not overloaded.** Live audio needs CPU on time: on a busy shared server the listener stream runs dry ([Memory and CPU](/docs/reference/resources/#what-this-means-for-a-host)).

### The server image

The bundle runs the published server image, `forgejo.weavesignal.top/weavesignal/weavesignal-server:1.0.2`.
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

It ends like this:

```text
WeaveSignal is running (project 'weavesignal', podman).

  Control plane   http://127.0.0.1:7310/   (health: /health)
  Chat            http://127.0.0.1:7311/
  Listeners       http://127.0.0.1:7312/programme.opus
  Live room       ws://127.0.0.1:7880  (media on UDP 7882)

Sign in: the studio uses the control plane's address above with the admin token, which is
CONTROL_ADMIN_TOKEN in deploy/compose/.env (mode 600). It is not shown here.
Every listener is on loopback: nobody off this machine can join or listen. To publish the
station, run this again with --public and the name that reaches this machine.
```

The SRT link (7313), the playout feed (7315), the desk's feed (7317), and the ports above the playout feed and above the listener port are internal: the script checks they are free but never shows or publishes them.

Sign in to the studio app with the station key, then add yourself as staff: [Add staff and sign in](/docs/how-to/add-staff/).

From 1.0.1 the script also names the station's **setup page** (`/setup/` on the control plane's address): sign in there with the same key to give the station its name, its takedown contact and its look, with no file to edit ([Set up the station and its look](/docs/how-to/set-up-the-station/)).

### Open call-in

Call-in stays closed until the station says how a caller can ask for their call to be taken down (ADR-0124).

1. In `deploy/compose/.env`, set `CONSENT_CONTACT` to an email address or a link, such as `CONSENT_CONTACT=calls@station.example`.
2. Run `deploy/compose/weavesignal.sh` again.

The player page then offers call-in, and the consent words name that contact.
From 1.0.1 the [setup page](/docs/how-to/set-up-the-station/#name-and-takedown-contact) sets it on the running station instead, with no second run.

### Choose ports yourself

Give any port in the environment:

```sh
CONTROL_PORT=17310 LIVEKIT_MEDIA_PORT=52000 deploy/compose/weavesignal.sh
```

If a port you give is taken, the script stops, names it and suggests a free alternative.
The names are those in `deploy/compose/.env.example`: `CONTROL_PORT`, `CHAT_PORT`, `LISTENERS_PORT`, `SRT_INGEST_PORT`, `PLAYOUT_FEED_PORT`, `DESK_FEED_PORT`, `LIVEKIT_HTTP_PORT` and `LIVEKIT_MEDIA_PORT`.

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

## How it is tested

`just test-install` runs the script end to end on its own project and ports: a taken port refused with an alternative, a taken default moved to its alternative, the install answering, no secret in the output, the settings file mode 600, a second run unchanged, `--uninstall` keeping the database, and `--purge` leaving no container, volume, network or setting.
It passes `--no-boot-start`, and `just boot-check` tests start at boot with `loginctl`, `systemctl` and the engines stubbed, so neither changes anything on the machine it runs on.
Both are part of `just ci`, which runs on Podman 5.8 with docker-compose.
The public route has also run on a dedicated public server with rootless Podman and docker-compose (2026-11-03).
Docker Engine has not been tried.
