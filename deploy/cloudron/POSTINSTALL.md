<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
Your station is installed. The guide is at https://weavesignal.top/docs/how-to/install-on-cloudron/.

**First steps.** The dashboard's checklist for this app lists them too.

1. **Set the station up** at `$CLOUDRON-APP-ORIGIN/setup/`: sign in with `CONTROL_ADMIN_TOKEN`
   from `/app/data/secrets.env` (open it with the File Manager). Give the station its name, the
   takedown contact that opens call-in, its jurisdiction and its look (colour, icon, banner).
   The install wizard at https://weavesignal.top/docs/install-wizard/ makes `weavesignal.env`
   and the DNS list.
2. **Make yourself an admin.** In the studio app choose **Use the station key instead** with
   `CONTROL_ADMIN_TOKEN`, add yourself under **Settings → Staff**, then sign in as yourself.
   Each member of staff then gets their own account there.
3. **Choose who hears the stream.** It is unlisted but open to anyone with its address. To ask
   for a password, set `LISTENERS_AUTH=on` in `weavesignal.env` and restart the app: it then
   asks for the user `listener` and `LISTENERS_PASSWORD` from `secrets.env`. The player page
   cannot play a stream that has a password.
4. **Open the public ports** (below) in the server's firewall and any cloud firewall.
5. **Hosts in their own studios.** If hosts will send their shows from their own
   software, turn on the two source ports, Icecast (TCP) and SRT (UDP), in this app's
   **Location** settings. They are off until you do, and the station listens on them only once
   a show has a key. Open them in the firewall too.

* The station's settings are in `/app/data/weavesignal.env`, and its generated secrets (the
  station identifier, LiveKit's key, the control plane's admin token, the listener password,
  and the keys that hash chatters' and staff's email addresses) are in `/app/data/secrets.env`.
  Keep the email keys: a new one finds none of the accounts made before. Open them with the File Manager; restart the app after an edit.
* The consent words a caller agrees to are the shipped two sentences until you set
  `CONSENT_TEXT` in `weavesignal.env`, in double quotes (`\"` for a quote); they may name
  `{station}`, `{kept}`, `{contact}` and the show's `{presenter}`.
* The listener stream is at `https://<listener domain>/programme.opus`. It is in no directory
  and open to anyone with the address; set `LISTENERS_AUTH=on` in `weavesignal.env` to ask for
  the user `listener` and `LISTENERS_PASSWORD` from `secrets.env`.
* The player page, with call-in and the live chat, is at `https://<public domain>/`; podcast
  feeds, show cards and episode files are served there too. The chat's sign-in codes are sent
  through Cloudron's mail (`MAIL_MODE` in `weavesignal.env`).
* Room tokens: `POST /api/v1/room/tokens` with a staff sign-in or `CONTROL_ADMIN_TOKEN` as the bearer.
* The public ports (room media over UDP, TURN over TLS, the optional TURN over UDP and ICE
  over TCP, and the two ports for hosts' own studios if you turned them on) must be open in the server's firewall and any cloud firewall.
<sso>
* The station's own domain is behind Cloudron's login; the listener, room and public domains
  are not, and are protected by the listener password, by room tokens, and by serving only
  the public routes.
</sso>
