---
title: "Listen on a phone or another computer"
description: "Let a phone or another computer on your home network hear a station installed with weavesignal.sh: the --lan option, the firewall command it prints, and why nobody can speak through it; and the public route when they can."
type: howto
status: draft
last_verified: 2026-11-23
sidebar:
  order: 35
---

A station installed with `weavesignal.sh` listens on its own machine only: the player at `http://127.0.0.1:7310/` opens there, and a phone on the same wi-fi cannot reach it.
This page opens it to your home network for **listening**, or, when people need to speak, points you to the public route.

| You want | Use | A phone can |
|---|---|---|
| to hear the station on a phone or a second computer at home | [`--lan`](#listen-on-your-home-network) | listen |
| people anywhere to listen, call in, chat and speak | [`--public`](#let-people-speak-the-public-route) | listen, call, chat and speak |

## Listen on your home network

You need:

- a station installed with [`weavesignal.sh`](/docs/how-to/install-with-weavesignal-sh/) that has been [set up](/docs/how-to/set-up-the-station/) (until it has a name, the address shows a page that says so);
- the phone or computer on the **same network** as the machine that runs the station. A guest wi-fi that keeps devices apart from each other will not do.

1. Run the installer again with `--lan`, from the repository:

   ```sh
   deploy/compose/weavesignal.sh --lan
   ```

   The script finds this machine's address on your network, says which network card gave it, and ends with the links:

   ```text
   LAN address: 192.168.1.20 (the network card wlan0). Give another with --lan ADDRESS.
   …
   Listening on your network (--lan): the player answers on this machine at http://192.168.1.20:7300/.

     Player          http://192.168.1.20:7300/
     Stream          http://192.168.1.20:7300/programme.opus   (for a media player; the player page uses it)
   ```

   If this machine has more than one network address of the right kind (wi-fi and a cable, say), the script stops, lists them and asks which one your phone's network reaches: run it again with `--lan 192.168.1.20`, your own address in place of that one.
   It never takes a VPN, a container's network or a virtual machine's bridge by itself.

2. On the phone, open the **Player** link in a browser and press **Listen live**.
   The page's sound comes from the stream on the same address.
   Some seconds pass before you hear the studio: the desk's clock says how many (*about 24 s behind*).

3. If the page does not open, this machine's **firewall** is probably closing the port.
   The script cannot open it for you (it never runs `sudo`) and prints the lines to run yourself.
   On Fedora and Bazzite, with firewalld:

   ```sh
   sudo firewall-cmd --add-port=7300/tcp
   sudo firewall-cmd --runtime-to-permanent
   ```

   The first line opens the port now, in the default zone (`firewall-cmd --get-active-zones` lists the zones; add `--zone=NAME` to use another); the second keeps it after a restart.
   On Ubuntu, with ufw: `sudo ufw allow 7300/tcp`.
   Open that one port and nothing else.

4. Give this machine a **fixed address** in your router (most routers call it a reservation or static lease).
   Home routers hand out addresses that change; when this one does, the links stop working and a kept address that is gone is named in the script's next run.
   Run `deploy/compose/weavesignal.sh --lan` again and it follows the machine to the new address.

### What it opens

`--lan` starts one small web server beside the station, on that one address and one port (`LAN_PORT`, 7300; 17300 or 27300 where it is taken), in plain HTTP (`lan.yaml`).
It passes **only** what listening needs, read only:

- the player page and its files,
- the player's own information (the station's name and look, what is playing now, the open routes for a station's website, the video stream when video is on),
- the listener stream, `/programme.opus`, and the two MP3 streams when they are on.

Everything else answers *404* or, for anything that writes, *405*: the studio's API and its sign-in, the setup page, invitation and call-in pages, the chat, the live room, health and the rest.
The studio, its sign-in and the station key stay on this machine: nothing in `--lan` makes them reachable from your network.
Anyone on your network can hear the stream, as on any radio.

The player page on this address leaves the chat out, and its **Talk to the studio** panel says that calling in needs a secure page and keeps **Call the studio** switched off, because a call needs the microphone (below).

### Why nobody can speak

A browser gives a page the microphone only over **HTTPS**, or on `localhost`.
A plain-HTTP address on your network is neither, so the browser refuses the microphone: nobody can call in or take part from a phone through `--lan`.
Putting HTTPS on a private address needs a certificate that every phone trusts, which a home network does not have.
For speaking, use the public route below.

### What it changes in the settings file

`weavesignal.sh --lan` writes three lines into `deploy/compose/.env`: `LAN_ADDRESS`, `LAN_PORT` and `STREAM_URL` (the player's audio source, `http://192.168.1.20:7300/programme.opus`).
It leaves `PUBLIC_ORIGIN` alone: that setting also turns on show cards, podcast feeds and invitation links, none of which this address serves ([Publish a station from the Compose bundle](/docs/how-to/publish-with-compose/#what-the-proxy-publishes) shows how to give a private station its own address for cards).
Running it again keeps all of this; `--lan` without an address looks again for the machine's address.

`weavesignal.sh --lan off` goes back to this machine only and removes the web server.
`--lan` and `--public` do not go together: the public route already serves the player and the stream.
`--lan` is for the Compose route; a station installed as Quadlets or on Cloudron has its own ways ([Run with Quadlets](/docs/how-to/run-with-quadlets/), [Install on Cloudron](/docs/how-to/install-on-cloudron/)).

The port is listed in [Ports](/docs/reference/ports/#with-weavesignalsh).

## Let people speak: the public route

When people need to call in, chat or speak from their phones, the station needs a name on the internet and a certificate, so that browsers give the microphone: [Publish a station from the Compose bundle](/docs/how-to/publish-with-compose/).
The command is `weavesignal.sh --public station.example`; it needs a domain name that reaches this machine, and ports 80, 443 and LiveKit's media port open to the internet.

## If it does not work

- **The link does not open on the phone.** Check the phone is on the same network, not on mobile data and not on a guest network. Then the firewall (step 3). On this machine, `curl http://192.168.1.20:7300/api/v1/player` (your address) answers when the web server runs; if it answers here and not on the phone, it is the firewall or the network.
- **The page says *This station is not set up yet*.** Finish [setting up the station](/docs/how-to/set-up-the-station/) on this machine; the setup page itself is not served on the network.
- **The page opens, but pressing Listen live gives no sound for a while.** The stream starts some seconds late after a start; try again after a minute. See also [No sound?](/guides/no-sound/).
- **The links stopped working after a router restart.** The machine has a new address. Run `deploy/compose/weavesignal.sh --lan` again and reserve the address in the router.
- **A message says the address is not an address of this machine.** The one in the settings file is gone; run with `--lan` to look again, or `--lan off`.
