---
title: "RTMP out"
description: "The desk sending the programme's picture and sound, behind the delay, to RTMP servers: the two profiles and their formats, the picture, the settings, the status, the API, the encoders and their licences, what each platform took in tests, the failures handled, and what it costs."
type: reference
status: draft
last_verified: 2026-11-06
---

RTMP out sends the programme's picture and sound, behind the profanity delay, to RTMP or RTMPS servers such as Rumble, Odysee and PeerTube (ADR-0229, ADR-0230).
It is part of the [video path](/docs/reference/desk-video/): it needs video on (`[video] enabled = true`, or `VIDEO=true` in the Compose bundle), and it is off until an admin turns a destination on.
How to set one up: [Send your show to Rumble, Odysee or PeerTube](/docs/how-to/send-to-video-platforms/).

## What is sent

- **The picture** is the frame the desk sends its own HLS, after the delay: the vision mixer's shot, the slate and the captions.
- **The sound** is the video's sound after the delay: the desk's programme while someone is live, playout's output while nobody is ([The sound](/docs/reference/desk-video/#the-sound)).
- **A dump** empties both, so a destination never gets what was dumped. The tests check this both in the desk and as read back from a server.

## The two profiles

Each destination has one profile.
The encode for a profile runs only while one of its destinations is on, and every destination of that profile shares it.

| Profile | Picture | Sound | Carried as | For |
|---|---|---|---|---|
| `compatible` (the default) | H.264, constant bitrate, a keyframe every 2 s, no B-frames, `high` profile (OpenH264 makes it without B-frames) | AAC-LC, 128 kbit/s, 44.1 kHz, stereo | classic RTMP (FLV) | Rumble, Odysee, PeerTube, and most other servers |
| `open` | AV1 (NVENC, or SVT-AV1 on the CPU, as for HLS), a keyframe every 2 s | Opus, 128 kbit/s, 48 kHz, stereo | Enhanced RTMP (E-RTMP v1, the `av01` and `Opus` FourCCs) | servers that take AV1 and Opus over RTMP, such as MediaMTX |

`compatible` is a [documented exception](/docs/reference/licence/) to WeaveSignal's media rule (Opus, FLAC and WebM), for RTMP out only (ADR-0230, as ADR-0002 is for the MP3 stream).
Nothing else in WeaveSignal uses H.264 or AAC: the room, the desk's HLS, recordings and archives do not.

The desk writes the FLV tags itself (`crates/desk/src/flv.rs`), because GStreamer 1.28's FLV muxers take neither AV1 nor Opus.
It hands them, one tag per buffer, to GStreamer's `rtmp2sink`.
A destination that connects, or connects again, is sent the stream's metadata and both sequence headers first, and then nothing until a picture keyframe, with its timestamps counted from that keyframe.

## The picture's size

RTMP sends the picture the desk composes, the **canvas**, or a smaller one.
The canvas is the first HLS rung (480p20) unless `[video] canvas` says otherwise.

| `canvas` | Size and rate | Notes |
|---|---|---|
| empty | the first HLS rung, 480p20 by default | as before RTMP out |
| `720p30`, `720p25` | 1280 × 720 | Rumble's 720p tier; every camera, slate and caption is made at this size, and HLS is scaled down from it |
| `1080p30` | 1920 × 1080 | the most the desk composes |

`[video.rtmp] rung` picks a smaller picture for RTMP than the canvas (empty: the canvas).
Nothing is ever scaled up: a rung larger or faster than the canvas, or a canvas smaller than the first HLS rung, is refused when the desk starts.

## Settings

In the desk's `desk.toml` (all optional):

| Section | Key | Default | Meaning |
|---|---|---|---|
| `[video]` | `canvas` | empty | the picture composed (above) |
| `[video.rtmp]` | `rung` | empty | the picture sent; empty is the canvas |
| | `keyframe_s` | `2` | a picture keyframe this often (Rumble asks for 2 s) |
| | `retry_min_s`, `retry_max_s` | `2`, `60` | a destination that fails is tried again after 2 s, then 4, 8, 16 … up to 60 |
| | `backlog_ms` | `4000` | a destination this far behind is dropped and connects again |
| `[video.rtmp.compatible]` | `h264` | `auto` | `auto`: NVENC if a session is free, else OpenH264; `openh264`; `nvenc` |
| | `h264_profile` | `high` | `high`, `main` or `baseline` |
| | `video_kbps` | `0` | 0: about 0.09 bit a pixel (480p20: 700, 720p30: 2500, 1080p30: 5600); at most 8000, Rumble's ceiling |
| | `aac_kbps`, `aac_rate` | `128`, `44100` | the AAC; 44100 or 48000 |
| `[video.rtmp.open]` | `video_kbps` | `0` | 0: the HLS figure (480p20: 600, 720p30: 1500) |
| | `opus_kbps` | `128` | the Opus |

The Compose bundle (and the Quadlets made from it) set these from `.env`: `VIDEO_CANVAS`, `RTMP_RUNG`, `RTMP_H264` and `RTMP_VIDEO_KBPS`.
The Cloudron package takes the same four in `weavesignal.env`, with `RTMP_H264` at `openh264` (a Cloudron app has no GPU).
The package runs video from 1.0.1, but its image does not carry OpenH264 or vo-aacenc yet, so `compatible` cannot run there, and `open` has not been tried in the package: RTMP out on Cloudron waits for a later package.

The destinations are not in any settings file.
An admin adds them in the studio app (Settings, RTMP out) or through the API below, and the control plane keeps them in its database.

## The stream key

A stream key lets anyone who has it stream as you, so:

- it is sent once, when a destination is added or its key changed, and no route, page or status shows it again;
- the database keeps it sealed (ChaCha20-Poly1305, with the destination's id bound in) under a key derived from `WEAVESIGNAL_CONTROL_TOKEN_KEY` (or, without it, from the station key), so a copy of the database alone does not hold it. Without either key the routes answer 503;
- changing that key leaves the stream keys unreadable: the list shows `key_readable: false` and the panel asks for the key again;
- the desk gets it over its Unix socket only, puts it on the end of the server URL only inside `rtmp2sink`, and cuts it out of every error before the error is logged or shown.

## The API

All admin only (`x-admin`); an API token never reaches them. The full description is in [the API reference](/docs/reference/api/).

| Route | Does |
|---|---|
| `GET /api/v1/rtmp` | every destination (never its key) with the desk's view of it (`desk`), whether the desk runs video, the encoders running, and each platform's form |
| `POST /api/v1/rtmp/destinations` | add one: `name`, `platform` (`rumble`, `odysee`, `peertube`, `custom`), `url` (`rtmp://` or `rtmps://`, without the key), `key`, `profile` (`compatible` or `open`), `enabled` (off unless asked); at most 10 |
| `PUT /api/v1/rtmp/destinations/{id}` | change any of them; a field left out is kept, the key too |
| `PUT /api/v1/rtmp/destinations/{id}/enabled` | `{"enabled": true}` or `false`: on or off |
| `DELETE /api/v1/rtmp/destinations/{id}` | remove it, with its key |

The control plane hands the desk the destinations, keys included, over the desk's socket (`{"op": "rtmp", "set": "…", "destinations": […]}`).
It names each set by an HMAC of what it sends, and sends it again when the desk's status names another set, within 3 s: after a change, and after the desk restarts.

## Status

The desk's status (`{"op": "status"}`), and the studio view's picture, carry `video.rtmp`:

```json
{
  "set": "3f9c2a1b7d4e6c08",
  "destinations": [
    {"name": "Rumble", "profile": "compatible", "enabled": true, "state": "live",
     "connected": true, "connected_ms": 125000, "last_error": null, "last_error_ms_ago": null,
     "bytes": 48300000, "bytes_total": 48300000, "connects": 1, "retry_in_ms": null}
  ],
  "encoders": [
    {"profile": "compatible", "encoder": "openh264", "width": 1280, "height": 720, "fps": 30,
     "video_kbps": 2500, "ready": true, "tags": 18112}
  ]
}
```

`state` is `off`, `waiting` (on, waiting for its encoder's first keyframe), `connecting`, `live` (the server has answered) or `retrying` (it failed; `last_error` says why, without the key, and `retry_in_ms` when it is tried again).
`live` means the server took the connection; whether the platform then shows the stream is the platform's matter (PeerTube, below, took `open` and showed nothing).

## What happens when something fails

| Failure | What happens |
|---|---|
| a destination refuses the key, or is not there | that destination's own pipeline fails; it is tried again after 2, 4, 8 … s (at most `retry_max_s`); the other destinations, HLS, the room and the sound go on |
| a server that goes away mid-show | the same; it is found again at the next try after it comes back (MediaMTX stopped 5 s: connected again 0.3 to 0.8 s after it came back, in three runs) |
| a destination that cannot keep up (`backlog_ms` behind) | it is dropped and connects again, so it never holds the others back |
| a profile's encoder fails | its pipeline is made again after 2 s and its destinations connect again; the other profile and HLS go on |
| the desk restarts | the control plane sends the destinations again within 3 s |

## The encoders and their licences

| Does | Software | Licence |
|---|---|---|
| H.264 | OpenH264 2.6.0 (`openh264enc`), Cisco's binary as Fedora's `fedora-cisco-openh264` repository ships it | BSD-2-Clause; Cisco's AVC patent licence covers that binary only "for the personal use of a consumer or other uses in which it does not receive remuneration", and only as Cisco distributes it |
| H.264 on an NVIDIA GPU | NVENC (`nvh264enc`), the host's driver | the driver is NVIDIA's and never in our images |
| AAC | vo-aacenc 0.1.3 (`voaacenc`), built in the desk image | Apache-2.0 |
| AV1, Opus | SVT-AV1 or NVENC, libopus, as for HLS | see [Licence](/docs/reference/licence/) |

fdk-aac is not free software and is never used: the server image removes Fedora's `fdk-aac-free` and its GStreamer plugin.
FFmpeg's AAC encoder is not used either, because Fedora's `libavcodec-free` links fdk-aac.
H.264 is still under patent in many countries (in the US until about November 2027 by most reports); a station that streams H.264 for money may need a licence from the patent pool, whatever the encoder.

## What each platform took

Tested against local servers on loopback only, never a real platform (`just test-rtmp`, 2026-11-06):

| Server | `compatible` (H.264, AAC) | `open` (AV1, Opus) |
|---|---|---|
| MediaMTX 1.21.3 | taken, read back over RTSP as H.264 High and AAC-LC 44.1 kHz | taken, read back as AV1 Main and Opus 48 kHz |
| OvenMediaEngine 0.21.0, Enhanced RTMP on | taken, H.264 and AAC passed through to its LL-HLS and WebRTC | the AV1 taken, the Opus refused (`Not supported audio codec`); it then closed the connection, again at each try |
| PeerTube 8.3.1, live transcoding on | published, transcoded to 480p H.264, with the AAC in an audio rendition of its own | connected, but its probe found no picture and the live never started |
| PeerTube 8.3.1, live transcoding off | published as sent, H.264 and AAC | as with transcoding on |

Rumble's own settings page asks for H.264 and AAC; Odysee's setup page gives H.264 settings, and its server appears to be OvenMediaEngine (research `2026-11-06-rtmp-out.md`).
PeerTube takes RTMP on port 1935, and RTMPS on 1936 where its admin turns it on, with a stream key; its documentation names no codec.
So `open` is for servers that say they take AV1 and Opus over Enhanced RTMP, now or later.

Not tried yet: any real platform, RTMPS against a server (`rtmp2sink` speaks it, but no RTMPS server was run), NVENC for H.264, and RTMP out in the Cloudron package.

## What it costs

Measured on the workstation (`just test-rtmp-cost`, a release build, one person on air with a moving 1280 × 720 camera; the desk's share of one core over 20 s, including the test's own sources):

| | Canvas 480p20 | Canvas 720p30 |
|---|---|---|
| video on, HLS, no RTMP | 20.7 % | 26.1 % |
| `compatible` on (OpenH264, AAC) | 25.1 % (+4.4) | 35.3 % (+9.2) |
| `open` on (SVT-AV1, Opus) | 43.6 % (+22.9) | 58.8 % (+32.8) |
| both | 44.0 % (+23.3) | 64.2 % (+38.1) |

The 720p30 canvas itself costs about 5 points more than 480p20 before RTMP, because every frame is composed at that size.
With NVENC (the desk given the GPU) the encodes move off the CPU, counted against the station's NVENC sessions (`nvenc_sessions`).

## Tests

| Command | What |
|---|---|
| `just desk-check` | both profiles to files, read back by FFmpeg and GStreamer (the picture, the tone); a dump keeps the held picture and sound out; a destination nobody answers is tried again with a growing wait, its key never in its error or the status, while a good one, HLS, the picture pass and the mixing pass go on; a failed encoder comes back alone; nothing is encoded while every destination is off; the FLV tags, the `av1C` record, the interleaving, the back-off and the key's redaction |
| `just test-rtmp` | MediaMTX: both profiles read back, a refused key, the server stopped for 5 s and found again, a dump read back from the server; OvenMediaEngine and PeerTube (transcoding on and off): both profiles, and what each kept (`PEERTUBE=0` leaves PeerTube out) |
| `just test-db` | the API against PostgreSQL: added, refused, changed, turned, removed; the key sealed and never in an answer; the desk sent its set, again after a change and after it restarts |
| `just test-rtmp-cost` | the table above |
