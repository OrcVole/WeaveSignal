---
title: "Run a station as Podman Quadlets"
description: "Generate systemd units for each container from the Compose bundle, store the secrets as Podman secrets, and let systemd start the station."
type: howto
status: draft
last_verified: 2026-11-10
sidebar:
  order: 34
---

> **New to this?** The [install wizard](/docs/install-wizard/) asks the questions and gives you the settings file, the DNS record and these commands filled in (from 1.0.1).

> **A documented route** (ADR-0022): the units are generated and checked by Podman's own generator on every `just ci`, but the tests do not install them into systemd or start them, as they run Compose end to end.
> Like the Compose bundle, the station listens on loopback only unless its settings name a domain (`weavesignal.sh --public`, [Publish a station from the Compose bundle](/docs/how-to/publish-with-compose/)); the generator then adds the proxy, whose units were converted by Podman's generator but not run.

Quadlets are Podman's systemd units for containers.
WeaveSignal's are generated from the Compose bundle, so `deploy/compose/compose.yaml` stays the one description of the station.

## Before you start

- Podman 5 with `podman compose` through `docker-compose` (not `podman-compose`, which ignores the bundle's settings files), and systemd for your user.
- The repository, checked out where it will stay: the units mount files from it (below).
- The server image: the published one is pulled at the first start; to use one built from the repository, see [The server image](/docs/how-to/install-with-weavesignal-sh/#the-server-image).

## Write the settings

`weavesignal.sh` makes the secrets and chooses the ports without starting anything:

```sh
mkdir -p ~/.config/weavesignal
deploy/compose/weavesignal.sh --no-start --env-file ~/.config/weavesignal/station.env
```

## Generate the units

```sh
deploy/quadlets/generate.sh --env-file ~/.config/weavesignal/station.env \
  --out ~/.config/weavesignal/quadlets --create-secrets
```

This writes, under `~/.config/weavesignal/quadlets`:

| Path | What |
|---|---|
| `containers/systemd/weavesignal-*.container` | one unit per service: LiveKit, the desk, playout, the listener process, the control plane, the chat, PostgreSQL, and the proxy on a public station |
| `containers/systemd/weavesignal-*.volume` | one unit per volume, named as Compose names them (`weavesignal_postgres-data`), so a station moved from Compose keeps its data |
| `config/` | the services' settings files, mounted read-only |
| `systemd/weavesignal.target` | starts every service |

No secret is written into a unit or a settings file.
`--create-secrets` stores each one as a Podman secret (`weavesignal-postgres-password` and the rest, listed in [Packages and install files](/docs/reference/packages/#the-quadlets)), passing the value on standard input; the units hand them to the containers as environment variables.
Without it, the script lists the secrets the units need.
PostgreSQL's unit reports healthy only once the database answers, and the control plane and the chat wait for it, as in Compose.

## Install and start

```sh
cp ~/.config/weavesignal/quadlets/containers/systemd/* ~/.config/containers/systemd/
cp ~/.config/weavesignal/quadlets/systemd/* ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now weavesignal.target
loginctl enable-linger
```

The last line keeps your user's services running after you log out and starts them at boot.
Playout's and the listener process's units bind-mount the Liquidsoap scripts (`playout/`) and the default loop (`assets/default-loop/`) from the repository you generated from, so keep it in place.

## Change something

Change the settings file, generate again, copy the units again, and run `systemctl --user daemon-reload` and `systemctl --user restart weavesignal.target`.
Run `generate.sh` with `--create-secrets` again only if a secret changed.

## Remove it

```sh
systemctl --user disable --now weavesignal.target
rm ~/.config/containers/systemd/weavesignal-* ~/.config/systemd/user/weavesignal.target
systemctl --user daemon-reload
```

The volumes and secrets stay; `podman volume rm` and `podman secret rm` delete them.

`just quadlet-check`, part of `just ci`, generates the units from throwaway settings for a loopback station and a public one, and Podman's own generator (`quadlet -dryrun -user`) turns every container and volume into a service, with no secret in any file, the database's health check in place and, on a public station, LiveKit waiting for the proxy.
The units are not installed into systemd or started by any test.
