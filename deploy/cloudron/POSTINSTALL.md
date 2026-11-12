<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
Your station is installed. The guide is at https://weavesignal.top/docs/how-to/install-on-cloudron/.

* **Set the station up** at `https://<this app's domain>/setup/`: sign in with
  `CONTROL_ADMIN_TOKEN` from `/app/data/secrets.env` and give the station its name, the takedown
  contact that opens call-in, its jurisdiction and its look (colour, icon, banner). The install
  wizard at https://weavesignal.top/docs/install-wizard/ makes `weavesignal.env` and the DNS list.
* The station's settings are in `/app/data/weavesignal.env`, and its generated secrets (the
  station identifier, LiveKit's key, the control plane's admin token, the listener password,
  and the keys that hash chatters' and staff's email addresses) are in `/app/data/secrets.env`.
  Keep the email keys: a new one finds none of the accounts made before. Open them with the File Manager; restart the app after an edit.
* The listener stream is at `https://<listener domain>/programme.opus`. It is in no directory
  and open to anyone with the address; set `LISTENERS_AUTH=on` in `weavesignal.env` to ask for
  the user `listener` and `LISTENERS_PASSWORD` from `secrets.env`.
* The player page, with call-in and the live chat, is at `https://<public domain>/`; podcast
  feeds, show cards and episode files are served there too. The chat's sign-in codes are sent
  through Cloudron's mail (`MAIL_MODE` in `weavesignal.env`).
* Staff: in the studio app, choose **Use the station key instead** with `CONTROL_ADMIN_TOKEN`,
  add yourself as an admin under **Settings → Staff**, then sign in as yourself. Each member
  of staff then gets their own account there.
* Room tokens: `POST /api/v1/room/tokens` with a staff sign-in or `CONTROL_ADMIN_TOKEN` as the bearer.
* The public ports (room media over UDP, TURN over TLS, and the optional TURN over UDP and ICE
  over TCP) must be open in the server's firewall and any cloud firewall.
<sso>
* The station's own domain is behind Cloudron's login; the listener, room and public domains
  are not, and are protected by the listener password, by room tokens, and by serving only
  the public routes.
</sso>
