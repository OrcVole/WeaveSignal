---
title: "Use a GPU for video"
description: "Let the desk encode the station's picture on an NVIDIA card with NVENC, on the Compose bundle or the Quadlets: the card's CDI spec, GPU=nvidia (or found by weavesignal.sh), the encoder setting, and how to check it. Cloudron has no GPU; SVT-AV1 on the CPU is the fallback everywhere."
type: howto
status: draft
last_verified: 2026-11-20
sidebar:
  order: 41
---

With video on, the desk encodes the station's picture as AV1, one encoder per rung ([Video in the desk](/docs/reference/desk-video/)).
On the CPU that is SVT-AV1, which costs the server the cores the sound needs.
An NVIDIA card with an AV1 encoder (NVIDIA's Ada generation or later: the GeForce RTX 40 series, or the matching workstation cards) does it with **NVENC** instead, and RTMP out's H.264 too ([RTMP out](/docs/reference/rtmp-out/)).

From 1.2 the Compose bundle and the Quadlets hand the desk a card, by its **CDI** name (the Container Device Interface), when the station says so or `weavesignal.sh` finds one.
The image carries no driver: the host's driver comes in with the device.

**A Cloudron app has no GPU.** The package encodes on the CPU with SVT-AV1 (`VIDEO_ENCODER=svt`), and `auto` or `nvenc` there find no card and fall back with a line in the desk's log ([Install on Cloudron](/docs/how-to/install-on-cloudron/#video)).

## What you need

- The station on the [Compose bundle](/docs/how-to/install-with-weavesignal-sh/) or the [Quadlets](/docs/how-to/run-with-quadlets/), with video on (`VIDEO=true`).
- NVIDIA's driver on the server, and the NVIDIA Container Toolkit, which writes the CDI spec:

  ```sh
  sudo nvidia-ctk cdi generate --output=/etc/cdi/nvidia.yaml
  nvidia-ctk cdi list    # nvidia.com/gpu=0, nvidia.com/gpu=all, ...
  ```

  Run it again after a driver update: the spec names the driver's files.
- A Podman that takes CDI device names (the routes were written against Podman 6.1), or Docker with CDI turned on: `docker info` lists *CDI spec directories* when it is.

## Turn it on

1. In the settings file `weavesignal.sh` writes (`deploy/compose/.env`, or the one you keep for the Quadlets), set:

   ```sh
   VIDEO=true
   GPU=auto
   ```

   `GPU=auto` asks the script to look for an NVIDIA CDI spec in `/etc/cdi` and `/var/run/cdi`.
   It writes what it found, `GPU=nvidia` or `GPU=none`, and says so; the next run changes nothing until you set `auto` again.
   A new install with no `GPU` line looks the same way.
2. Run the script again, from the folder it is in:

   ```sh
   cd deploy/compose
   ./weavesignal.sh
   ```

   With `GPU=nvidia` and `VIDEO=true` it adds `gpu.yaml`, which hands the desk's container the card and turns SELinux labelling off for that container only.
   `GPU=nvidia` with no spec stops the script, naming the command above.
3. On the Quadlets, generate the units again (`deploy/quadlets/generate.sh`, [Run with Quadlets](/docs/how-to/run-with-quadlets/)): the desk's unit gains `AddDevice=nvidia.com/gpu=all`.

The install wizard asks the same question under *A graphics card for video* when video is ticked ([Install wizard](/docs/install-wizard/)).

## The settings

| Setting | Default | Meaning |
|---|---|---|
| `GPU` | `auto` (written as what was found) | `nvidia`: hand the desk the card; `none`: the CPU only; `auto`: look for a CDI spec on the next run |
| `GPU_DEVICE` | `nvidia.com/gpu=all` | which card, by its CDI name: `nvidia.com/gpu=0` for the first of several |
| `VIDEO_ENCODER` | `auto` (Cloudron: `svt`) | `auto`: NVENC when the desk has a card, else SVT-AV1; `nvenc`; `svt` keeps the picture on the CPU with the card there |
| `NVENC_SESSIONS` | `12` | NVENC sessions the desk may open, 0 to 64: one per rung and one for RTMP out. A consumer card's driver limits sessions for the whole machine, other programs included |

Each is the desk's `[video] encoder` and `nvenc_sessions` in `desk.toml` ([Settings](/docs/reference/settings/#desktoml)).
A rung past the session limit, or one NVENC refuses, is encoded by SVT-AV1, and the desk's log says which.

## Check it

- The desk's log line *video: AV1 to CMAF HLS* names each rung's encoder, `Nvenc` or `Svt`, when the picture starts (`podman logs weavesignal-desk-1` on Compose, `journalctl --user -u weavesignal-desk` on the Quadlets); a rung that fell back to the CPU has a warning above it saying why.
- The desk's status over its [control socket](/docs/reference/desk-control/) lists each rung's encoder.
- `nvidia-smi` lists the desk's process while video runs.
- **The driver's faults are Xid lines in the kernel log**: `journalctl -k | grep Xid`. A card that has logged one can stop the whole machine; set `GPU=none`, run the script again, and look at the card before trying again.

To measure the card before a show, the repository has `tests/gpu/measure.sh` (`just gpu-measure`): it runs the desk's AV1 chain from the server image on the card at each rung, several at once, and on the CPU for comparison, watches the kernel log from its start, and stops at the first Xid.
It needs the card free: it refuses to start while another program uses it.

## A host's own GPU

A host in their own studio may already encode a picture with their own card (OBS with NVENC, say).
In 1.2 the station takes only the sound from a host's software, over Icecast, SRT, WHIP, the bridge or VBAN, and the picture viewers see is the desk's own, encoded on the station's server.
Passing a host's picture through without encoding it again is designed but not built: see [A host's own picture](/docs/reference/desk-video/#a-hosts-own-picture).
