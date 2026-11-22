---
title: "Video in the desk"
description: "The video path: cameras subscribed by the desk, the vision mixer following the audio, the slate and captions, AV1 to CMAF HLS behind the same delay as the sound, the picture in the room for the people on the show, the player's audio-only toggle, and what it costs."
type: reference
status: draft
last_verified: 2026-11-20
---

Video is off unless a station turns it on (ADR-0009): `[video] enabled = true` in the desk's settings, `VIDEO=true` in the Compose bundle's `.env`, or `VIDEO=true` in the Cloudron package's `weavesignal.env` ([Install on Cloudron](/docs/how-to/install-on-cloudron/#video)).
Sound comes first: nothing on the video path runs in the desk's audio pipeline or its mixing pass, and a video failure takes down only the video pipeline it happened in.

## The path

1. **Cameras.** The desk subscribes to each person as before; their camera arrives on the same room connection and is handed over still encoded (VP8, VP9 or AV1; never H.264 or H.265, which LiveKit is told to refuse) to a small decoding pipeline of its own, which scales it to the picture's size with borders kept.
2. **The vision mixer.** Twenty times a second one thread picks the shot: the loudest person on air with a camera, cut to once they have been at least 3 dB louder than the shot for 600 ms and the shot has held 2 s. Taking the shot's person off air cuts away at once. Nobody on air on camera: the slate.
3. **The delay.** Every frame goes into a short history; the frame sent on is the one from the profanity delay's length ago, read afresh each frame, so pictures are dumped with the sound and build back up with it (ADR-0008).
4. **The encoders.** AV1, one encoder per rung: NVENC where the desk can reach an NVIDIA GPU, SVT-AV1 on the CPU otherwise, a keyframe every 2 s, no B-frames. From 1.2 the Compose bundle and the Quadlets hand the desk's container an NVIDIA card by its CDI name when the station says `GPU=nvidia` or `weavesignal.sh` finds one ([A GPU](#a-gpu)). The Cloudron package is SVT-AV1, Ubuntu's 2.3.0 (a Cloudron app cannot reach a GPU, ADR-0228).
5. **CMAF HLS.** `hlscmafsink` writes 2 s segments per rung and for the sound: Opus at 96 kbit/s, stamped on the same clock as the pictures. The sound is the desk's delayed programme while someone is live, and playout's output while nobody is ([The sound](#the-sound)). The desk writes `live.m3u8` (each rung, with the audio) and `audio.m3u8` (the audio alone). While [content steering](/docs/reference/steering/) has edges, the control plane adds the steering line to both as it serves them.
6. **The room.** The live picture, before the delay, is published into the room as `desk-pgm` in VP8, with small pictures of the other cameras along the bottom: what the people on the show see (ADR-0028).

Playout's link is unchanged: the programme still goes to Liquidsoap as Opus in Matroska over SRT, sound only (ADR-0010: video never passes through Liquidsoap).

## The sound

Viewers, and anyone using the player's audio-only toggle, hear what listeners hear (ADR-0172).

- **While someone is live** the sound is the desk's own programme after the profanity delay, so lips and voices stay together. Someone is live when a person whose role can go on air has their strip open on air, muted or not: a cough key does not switch the sound away.
- **While nobody is**, it is playout's output, the music, adverts and fallback that never pass through the desk. Playout sends it back to the desk as it sends the listener process its feed: PCM in WAV over loopback, on the `desk_feed` port (7317, [Ports](/docs/reference/ports/)), with the listener process's source password. The desk listens; playout connects, and tries again every 3 s while the desk is away.
- **Live is judged after the delay**, as the sound is: a strip opened now reaches viewers once the delay has played out what came before it, and until then they hear playout, which goes on with automation until the desk's sound reaches it too. A dump brings both forward together.
- **After the last strip closes** the desk's sound stays for 3 s more (`off_air_hold_ms`), because playout's copy of the desk arrives a little after the desk's own: switching at once would play the show's last words twice.
- **Each switch** is an equal-power crossfade of 300 ms (`crossfade_ms`), then the programme's limiter at −1 dBFS, so two loud sources mid-way are limited, not clipped.
- **With playout away** (stopped, restarting, or its feed run dry) the sound is the desk's programme, never nothing; the desk's sound runs from its first moment whether playout is there or not.

- **In auto mode** (the station's mode, which the control plane tells the desk: [The station's mode](/docs/reference/desk-control/#the-stations-mode)) automation holds the air and playout never takes the desk, so the sound is playout's whoever is live at the desk. Playout switches the moment the mode changes, so the desk's sound does too, through the same crossfade, with no wait for the delay and no hold. Back in live mode a strip already open (as heard after the delay) is the desk's sound at once.

Playout sends the feed on its own clock, as it sends the listener process's, with a 1 s write timeout: a desk that stops reading holds listeners up for at most a second, and is then dropped until it reads again.

The desk places what arrives 200 ms ahead of the mixing pass (`cushion_ms`), so a pause in the feed up to that long is not heard. A longer pause is heard twice, as a gap as long as the pause and then a skip of about the cushion, because the backlog that follows is placed afresh; `feed_replaced` counts these. On a busy host this happens often: see the counters below.

The desk's status (`{"op": "status"}`) carries `sound`: `source` (`desk` or `playout`), `live`, `feed_connected`, `feed_connections`, `feed_replaced` (pieces of the feed placed afresh after a pause or a burst longer than the cushion), and the feed's buffer as for a person's voice: `feed_underruns`, `feed_late_ms` and `feed_ring_ms`.

## The picture's controls

| Word on the desk's socket | Through the control plane | Does |
|---|---|---|
| `{"op": "picture", "mode": "auto"}` | `POST /api/v1/desk/commands` | the vision mixer chooses (the default) |
| `{"op": "picture", "mode": "slate"}` | the same; **F7** at the desk | the slate, whoever speaks, until handed back |
| `{"op": "picture", "mode": "person", "identity": "…"}` | the same | one camera held; the slate while it has no picture |
| `{"op": "caption", "text": "…", "ms": 8000}` | the same | a line over any shot for `ms` (at most 60 s, 160 characters); an empty `text` takes it down |
| `{"op": "now_playing", "station": "…", "show": "…", "title": "…", "artist": "…"}` | sent by the control plane itself ([Now playing on the slate](#now-playing-on-the-slate)) | the slate's text (ADR-0010) |
| `{"op": "thumbs"}` | `GET /api/v1/video/thumbs` | a small JPEG of each camera with a picture, in base64, by identity ([Camera thumbnails](#camera-thumbnails)) |

A person cut to gets their name as a lower third for 6 s (`lower_third_ms`).
A caption sits over the slate as well as over a camera, and goes through the delay with the picture: the chat's featured message can be put up with it.
The desk's status carries `video` (the mode, the shot, whose cameras have a picture, each rung's encoder, whether HLS and the room picture run) and each person's `camera`; the studio view carries it as `desk.picture`.

## The slate's picture

The slate shows a picture (ADR-0010): the station's art, or any image the studio chooses, above the words.
Without one the words fill the screen, as before.

1. In the studio app, on the desk's `desk.picture` panel, **Choose a slate picture** takes a PNG or JPEG (at most 24 MB, 8192 pixels on a side and 36 million pixels).
2. The control plane gives the file to the media worker, the process that reads cart uploads ([Carts in the desk](/docs/reference/desk-carts/)): it runs under Landlock with no file system, network or new processes, decodes the picture with memory-safe Rust, lays any transparency on the slate's colour, fits it inside `still_max_width` by `still_max_height` (1280 × 720) without enlarging it, and writes a clean 8-bit PNG. The control plane never parses the upload, and the desk never reads it.
3. The PNG is kept as the station's (`stills/station.png`, beside `video.still_file` in the carts' directory, which the desk already mounts read-only), and the control plane copies the one the slate should show to `video.still_file` (`stills/slate.png`). The desk looks at that file once a second; when it appears, changes or goes, it draws the slate again, so a desk that restarts shows the picture without being told.
4. On the slate the picture takes the upper part, fitted and centred on the slate's colour and never enlarged, and the station, the show and the track playing sit in a band under it. **Remove** takes the picture away.

**A show may have a picture of its own** (ADR-0200), set the same way on the show (`PUT /api/v1/shows/{show}/still`) and kept as `stills/show-<id>.png`.
In the studio app it is the show editor's **Slate picture** field (the Schedule tab, the show's **Details**; panel `calendar.show-still`): a preview of what the slate shows, **Choose…** or **Replace…** for a PNG or JPEG, **Remove**, and the station's sentence when it refuses one.
The slate shows the picture of the show on air, else the station's, else words: every five seconds the control plane looks at the schedule for the episode whose air time holds now (it lasts its own length, else its series' or show's slot, else an hour) and copies that show's picture to the desk's file, all or nothing.
A station that had one picture before keeps it as the station's.
The demo station starts with the kettle of `assets/stills-demo/` as its picture: `video.demo_still` names a PNG the control plane makes the station's at its start when the station has none, and only once, so a station that removes it does not get it back. `weavesignal.sh` sets it only for a station keeping the demo name, Kettle FM. The night show's moon in the same folder is for uploading by hand; no demo show is made.

A picture for each episode, and the slide schedule (timed images for the player, a chapters file and the audio playlist's images), are [planned](/docs/reference/release-one-status/#not-in-release-one).
The desk's status carries `still`: whether the slate has a picture.
The player's page and the audio stream show no picture; stills beside the audio for everyone else (ADR-0010) wait on the slide schedule.

## Now playing on the slate

The control plane tells the desk what playout is playing, once a second at most, and again every 15 s so a desk that restarted learns it: the station's name, the show's title from the studio's settings, and the track (title and artist) while a track is playing. A cart, a fallback loop or a live show leaves the track line out.
The text comes from playout as it says it, not after the listeners' delay: the video's sound while nobody is live is playout's own (ADR-0172), and the picture then goes through the desk's delay as well, so the words change a few seconds after the sound does. The delay is the desk's, not the listeners'.
With video off the desk refuses the words and nothing else happens.

## Camera thumbnails

The studio shows each camera the desk sees as a small picture on `desk.picture`, so the presenter sees what they can cut to; the one on air is marked.
The desk makes a 192-pixel JPEG of each camera when asked, through one `jpegenc` pipeline, and not more than about once a second for a camera however often it is asked; the control plane passes them to staff as base64 in one answer, and the app asks every two seconds while a camera is there. Nothing is written to disk, nothing is served without a bearer, and nothing is made while nobody asks. The strip is informational: the picture is still cut by the rule or held with F7 and the picture command.

## Rungs

| Rung | Picture | Bitrate | VMAF, NVENC | VMAF, SVT-AV1 |
|---|---|---|---|---|
| `480p20` (the default, ADR-0027) | 854 × 480, 20 fps | 600 kbit/s | 94.4 to 96.6 | 93.9 to 96.2 |
| `360p20` | 640 × 360, 20 fps | 350 kbit/s | 93.2 to 96.1 | 93.3 to 95.6 |
| `240p15` | 426 × 240, 15 fps | 160 kbit/s | 90.3 to 94.1 | 92.0 to 94.3 |

Measured with `just test-vmaf` (2026-10-26): Xiph's `FourPeople` and `KristenAndSara`, through the desk's own encoder settings and CMAF writer, the segments decoded to raw frames before scoring, each rung against the same frames before encoding.
`rungs = ["480p20", "360p20"]` writes both as renditions of `live.m3u8`; each rung is one encoder and, with NVENC, one of the 12 sessions a GeForce card allows per machine (`nvenc_sessions`). A rung past the limit, or every rung after NVENC fails, is encoded by SVT-AV1, and the log says so.

## The player

The player page plays `live.m3u8` with hls.js (loaded only when play is pressed), through Media Source Extensions or Safari's ManagedMediaSource, never Safari's own HLS player, which does not play Opus (ADR-0025).

| The browser | Plays |
|---|---|
| decodes AV1 and Opus in fMP4 | the picture, with **Audio only** to switch to `audio.m3u8` and back; the switch resumes at the same instant by the segments' program date-time |
| decodes Opus in fMP4 but not AV1 (older Apple devices) | the sound from `audio.m3u8`, and says why (ADR-0024) |
| has no Media Source or no Opus in fMP4 | the station's Icecast stream, as with video off (ADR-0026) |

The control plane serves the desk's HLS at `/hls/` (`[join] hls_dir`, `hls_url` for an edge or a CDN): playlists not cached, segments for 30 s, and nothing while the desk has video off.
Whether Safari plays Opus in fMP4 this way is not measured on a real device ([Not in release one](/docs/reference/release-one-status/#not-in-release-one)).

## The join page

While the desk has video on, the join page shows presenters, DJs and guests their own camera before they join (panel `join.camera-check`), asked for when the page opens. Nothing leaves the browser until they press **Join the studio**; they can untick **Camera on when I join**. A camera the browser refuses or cannot find costs only the picture: they still join, with sound, and the page says why. Callers are heard, not seen.
Once joined, the page sends the camera they were shown as AV1 from Chromium and VP8 elsewhere, one layer, on by default, and offers **Turn your camera off**.
It watches `desk-pgm` and no other camera, so one guest's browser cannot pull everyone's camera down to VP8.

## RTMP out

The same delayed picture and sound can also go to RTMP servers such as Rumble, Odysee and PeerTube, as H.264 and AAC or as AV1 and Opus over Enhanced RTMP, each destination on its own and off until an admin turns it on (ADR-0230).
`[video] canvas` lets the desk compose a larger picture than the first HLS rung (720p30, say), which HLS is scaled down from.
See [RTMP out](/docs/reference/rtmp-out/).

## A GPU

The desk's `[video] encoder` chooses: `auto` (the default) uses NVENC's AV1 (`nvav1enc`, with `cudaupload`) when GStreamer finds both, else SVT-AV1; `nvenc` asks for NVENC and falls back the same way, with a warning; `svt` keeps every rung on the CPU.
`nvenc_sessions` (12) caps the NVENC sessions the desk opens, one per rung and one for RTMP out's encode; a rung past it is encoded by SVT-AV1, and an NVENC encoder that fails while running hands its rungs to SVT-AV1 for the rest of the desk's run.
RTMP out's H.264 follows its own `h264` setting ([RTMP out](/docs/reference/rtmp-out/)).

How the card reaches the desk, on each route ([Use a GPU for video](/docs/how-to/use-a-gpu-for-video/)):

| Route | The card | Settings |
|---|---|---|
| Compose (`weavesignal.sh`) | `gpu.yaml`: `devices: [${GPU_DEVICE}]` and SELinux labelling off for the desk, added when `GPU=nvidia` and `VIDEO=true` | `GPU` (`auto`, `nvidia`, `none`), `GPU_DEVICE` (`nvidia.com/gpu=all`), `VIDEO_ENCODER` (`auto`), `NVENC_SESSIONS` (12) |
| Quadlets (`generate.sh`) | the same overlay, as the desk unit's `AddDevice=` and `SecurityLabelDisable=true` | the same settings file |
| Cloudron | none: a Cloudron app is given no GPU | `VIDEO_ENCODER` (`svt`; `auto` and `nvenc` fall back) |

`weavesignal.sh` with `GPU` unset or `auto` looks for a CDI spec that declares `nvidia.com/gpu` in `/etc/cdi` and `/var/run/cdi` and writes `GPU=nvidia` or `GPU=none`; `GPU=nvidia` with no spec stops it.
The image needs no driver of its own: Fedora's GStreamer `nvcodec` plugin opens the driver's libraries that the CDI spec mounts, and the image keeps no plugin registry, so a run with the card finds the plugin.

Measured on a card so far only by the M5 bench (below); `tests/gpu/measure.sh` runs the desk's chain on a card at each rung and several at once, with the kernel log watched for the driver's Xid faults, and stops at the first.

## A host's own picture

**Designed, not built (1.2).** A host in their own studio often encodes a picture already, with their own card. The design is that the desk passes that picture through to viewers without encoding it again, where the codecs allow, so the host's GPU does the work and the station's server does none.

What the desk does today: a host's software reaches the ingest over Icecast, SRT, WHIP, the bridge or VBAN; the ingest takes **the sound only** (an SRT stream's picture is skipped by its MPEG-TS reader, OBS's WHIP picture is accepted and thrown away), decodes it to 48 kHz and hands the desk PCM over one loopback connection per source. The picture viewers see is always the desk's own: the vision mixer's shot, or the slate, encoded at the station.

How it would work:

1. **In.** SRT carrying MPEG-TS with the picture beside the sound: H.264, or AV1 where the host's encoder can put AV1 in MPEG-TS (which OBS versions do is not checked yet). The ingest would keep the picture's elementary stream and hand it to the desk on a second loopback connection beside the sound, with each access unit's timestamp on the source's clock.
2. **At the desk.** While that source is the only one on air, HLS's rung (and RTMP out's open profile) would take the host's AV1 as it is, re-timed onto the desk's clock and held behind the profanity delay with the sound; the vision mixer, captions and the slate would not touch it. Anyone else on air with them, or a dump, would switch back to the desk's own encode at the next keyframe.
3. **Where the codecs do not allow it.** H.264 from the host can go to RTMP out's compatible profile as it is, but HLS here is AV1 only, so it would be decoded and encoded again (on the station's card if it has one). A picture larger than the rung, or a keyframe interval other than the segment length, also needs encoding again.

What is missing before it can be built: the picture through the ingest (its MPEG-TS reader skips every picture stream, and the source wire to the desk carries PCM only); a picture path in the desk that bypasses the vision mixer and switches at keyframes without a gap; the delay's picture history for compressed frames (it holds raw frames now); segmenting a stream the desk did not encode (the host's keyframes must land on segment boundaries, which the host's settings decide); the rules for a host whose picture stops while their sound goes on; and tests with OBS's own output. Until then a host's picture is not used, and the station's own card ([A GPU](#a-gpu)) is the way to take encoding off the server's CPU.

## What it costs

Measured on the workstation (`TEST=cameras`, a release build, four people with solid-colour VP8 cameras; easy pictures, so real cameras cost more in the encoders):

| | Desk CPU, share of one core | Desk threads |
|---|---|---|
| four people, video off | 11 % | 85 |
| four people with cameras, NVENC | 22 % | 159 |
| four people with cameras, SVT-AV1 | 27 % | 230 |

The room test's figures with every person on camera and video on (`WEAVESIGNAL_ROOM_VIDEO=1 TEST=room`) are within 1.5 ms of video off: a guest hears the presenter 80 ms after they speak (79 with video off), the programme 72 ms (71).

## Tests

| Command | What |
|---|---|
| `just desk-check` | local cameras: a camera reaches the HLS with the programme's sound; the slate with nobody on camera; the mixer follows the speaker and F7; picture and sound leave a 2 s delay within a frame of each other; a failed encoder leaves the sound on air and comes back; every rung written; captions; the slate's picture comes, changes and goes; a thumbnail for each camera |
| `TEST=cameras tests/room/run-stack.sh` | cameras through LiveKit to the HLS and to the room picture a guest sees; what four cameras cost |
| `WEAVESIGNAL_ROOM_VIDEO=1 TEST=room tests/room/run-stack.sh` | the room test with cameras and video on |
| `just test-vmaf` | VMAF at each rung (needs Xiph's `FourPeople_1280x720_60.y4m` and `KristenAndSara_1280x720_60.y4m` in `target/clips/`, or in the folder `WEAVESIGNAL_CLIPS` names) |

`WEAVESIGNAL_DESK_GPU=1` hands the test container the GPU (`--device nvidia.com/gpu=all`), and NVENC is used; without it, SVT-AV1.
