---
title: "Send your show to YouTube, Owncast, Rumble and other platforms"
description: "Send the studio's picture and sound, behind the delay, to YouTube, Owncast, X, Rumble, Odysee, PeerTube or any RTMP server: turn video on, find the platform's server URL and stream key, add the destination, and go live."
type: howto
status: draft
last_verified: 2026-11-11
sidebar:
  order: 37
---

WeaveSignal can send your show to a video platform as well as to its own player: the same picture and sound, behind the same delay, so a dump keeps the held words off the platform too (ADR-0230).
You need no OBS for it.
To keep streaming from OBS and take calls beside it instead, see [Take calls on a video stream with OBS](/docs/how-to/take-calls-with-obs/).
The reference, with what each format carries and what it costs, is [RTMP out](/docs/reference/rtmp-out/).

You need:

- an admin account in the studio app (or the station key);
- video on for the station;
- an account on the platform that may go live, and its server URL and stream key.

## 1. Turn video on

RTMP out sends the desk's picture, so the desk must run video.

- **Compose or `weavesignal.sh`.** In the station's `.env` set `VIDEO=true`, and restart the station.
- **For 720p** (Rumble's 720p tier, for example) set `VIDEO_CANVAS=720p30` as well: the desk then composes every picture at 1280 × 720 and scales its own player's picture down from it. It costs more CPU: about 5 points of one core for the picture, then 9 for the H.264 encode, on the test workstation.
- **Cloudron.** The package runs video from 1.0.1 ([Install on Cloudron](/docs/how-to/install-on-cloudron/#video)) and its image carries RTMP out's encoders (Ubuntu's builds of OpenH264 and vo-aacenc), but no Cloudron station has sent to an RTMP server yet: send a short test stream with a test key first ([Known limits](/docs/reference/release-one-status/#known-limits)). A Cloudron app has no GPU, so H.264 is OpenH264 on the CPU (`RTMP_H264=openh264`).

The Settings panel says so when the desk runs without video.

## 2. Find the server URL and the stream key

Each platform shows two things on its go-live page: a **server URL** (`rtmp://…` or `rtmps://…`) and a **stream key**.
Copy both from the platform's own page: the URLs below are the usual ones, but the platform's page has the last word.

| Platform | Where | Usual server URL |
|---|---|---|
| YouTube | YouTube Studio: **Create**, **Go live**, **Stream** | `rtmps://a.rtmps.youtube.com:443/live2` |
| Owncast | its admin: **Server Setup**, **Stream Keys**; the server is your Owncast's address and RTMP port (on Cloudron, the app's RTMP port) | `rtmp://<your owncast>:1935/live` |
| X | X Premium: **Creator Studio**, **Live Studio**, **New Livestream**, an RTMP source; then start the broadcast on X | per source, shown by X |
| Rumble | **Go Live**, then **Get the Streamer Configuration** | `rtmp://rtmp.rumble.com/live` |
| Odysee | **Go Live**, your livestream, the **RTMP Setup** tab | `rtmp://publish.odysee.live/live` |
| PeerTube | **Publish**, **Go live** (the instance's admin must have turned live on); the live's page shows its RTMP URL (and RTMPS URL, port 1936, where the admin turned RTMPS on) and its stream key | `rtmp://<your instance>:1935/live` |
| Anything else | your server's documentation | `rtmp://server/app` |

The stream key lets anyone who has it stream as you.
Keep it out of chats, screenshots and shared screens.

## 3. Add the destination

1. In the studio app, open **Settings** and find **RTMP out**.
2. Choose **Add a destination…**, then the platform. The form fills in a name, and says where that platform shows its URL and key.
3. Paste the **server URL** and the **stream key**.
   The key goes in its own field, never on the end of the URL; it is kept sealed by the station and never shown again.
4. Leave **Formats** at **H.264 and AAC**.
   That is what Owncast, X, Rumble, Odysee and PeerTube take, and YouTube too.
   Choose **AV1 and Opus** only for a server that says it takes them over Enhanced RTMP: in local tests PeerTube 8.3.1 showed nothing, and OvenMediaEngine (which Odysee appears to run) kept the picture and dropped the sound.
5. Leave **On** unticked until the show should go there, and choose **Add the destination**.

## 4. Go live

1. Choose **Turn on** beside the destination.
   Within a few seconds its status reads **Connecting**, then **Live**, with how long it has been sending and how much.
2. Check the platform's own preview: **Live** in WeaveSignal means the server took the connection, not that the platform shows it.
3. At the end of the show, choose **Turn off**.

Several destinations can be on at once; the ones with the same format share one encode.

## When it goes wrong

| The status says | What to do |
|---|---|
| **Retrying**, with `'publish' cmd failed` or `connection closed remotely` | the server refused the key: copy the key again from the platform (some platforms give a new one for each stream) and paste it with **Change…** |
| **Retrying**, with `Connection refused` or a name that will not resolve | the server URL is wrong, or the platform is down: check the URL; WeaveSignal keeps trying, after 2, 4, 8 … s and then every minute |
| **Key needed** | the station's token key changed, so the stored key can no longer be read: enter the key again with **Change…** |
| **Waiting**, and the panel says the desk runs without video | turn video on (step 1) |

A destination that fails never touches the others, the station's own player, the room or what listeners hear.

## What has been tried

RTMP out was tested against servers on the test machine (MediaMTX, OvenMediaEngine and PeerTube 8.3.1; [What each platform took](/docs/reference/rtmp-out/#what-each-platform-took)) and, on 2026-11-11, from a Cloudron station to **Owncast 0.3.0** on another Cloudron: **H.264 and AAC** went live and played; with **AV1 and Opus** Owncast closed every connection within a second.

On 2026-11-12 the test station went live on **YouTube** (`rtmps://a.rtmps.youtube.com:443/live2`, Compatible: YouTube showed the picture and called the stream's health excellent) and on **X** (`rtmps://a.live.x.com/x`, Compatible), both added as **Another server** before the YouTube and X forms could be saved.

- **YouTube** takes AV1 video over Enhanced RTMP, but its sound must be AAC or MP3, not Opus (YouTube's live encoder settings, read 2026-11-12), so it refuses **AV1 and Opus**; keep **Compatible**. YouTube re-encodes every stream, and gives viewers its VP9 or AV1 copies mostly from 1440p up. It warns when the bitrate is under 1,000 kbit/s, which a still slate can be.
- **X** needs X Premium. X ends a stream after 24 hours (reported, 2026-11-12), so a station on all day needs a new broadcast on X each day; leave X's **Use authentication** off.
- **Gab** is not offered: its live streaming is not open to streamers yet (2026-11-12, its **Go Live Now** failed), and Gab says it is working on it.

Not tried yet (check again before each release):

- **Rumble and Odysee themselves.** The formats come from their own setup pages: Rumble asks for H.264 and AAC, and Odysee's tips are H.264 settings.
- **PeerTube's codecs** are not in its documentation; PeerTube 8.3.1 took **H.264 and AAC** and showed nothing for **AV1 and Opus**.
- **NVENC** for H.264, on a GPU.

Send a short test stream to the platform with a test key before the first show.
