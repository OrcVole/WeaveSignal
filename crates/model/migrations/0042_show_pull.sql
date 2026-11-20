-- SPDX-License-Identifier: AGPL-3.0-only

-- A pulled source (ADR-0280, ADR-0291; PLAN 17b): the station opens a partner's or a host's
-- stream itself, from an address kept with a show key, instead of waiting for the host's software
-- to connect. The key is an ordinary show key (its id names the source on the desk, its monitor
-- link works as any other's), with `hint` `pull` and the address in `pull_url`; the ingest opens
-- it while the slot's door is open, so the tile, STANDBY, the post, the grace, LINK LOST and the
-- monitor link are the pushed source's own.
--
-- The address is kept checked and normalised (`weavesignal_proto::pull`): HTTP or HTTPS
-- (Icecast or any HTTP audio, or an HLS playlist) or an SRT listener's address with its stream id.
-- It never holds a user name, a password or an SRT passphrase: the control plane refuses those.
ALTER TABLE show_key ADD COLUMN pull_url text CHECK (length(pull_url) BETWEEN 1 AND 2048);

ALTER TABLE show_key DROP CONSTRAINT show_key_hint_check;
ALTER TABLE show_key ADD CONSTRAINT show_key_hint_check CHECK (hint IN ('icecast', 'srt', 'pull'));
-- A pull has an address, and only a pull has one.
ALTER TABLE show_key ADD CONSTRAINT show_key_pull_has_url CHECK ((hint = 'pull') = (pull_url IS NOT NULL));

-- Where a show comes from: the room, a host's own software (`push`), or a stream the station
-- pulls (`pull`, when the show has a live pull key).
ALTER TABLE show DROP CONSTRAINT show_source_check;
ALTER TABLE show ADD CONSTRAINT show_source_check CHECK (source IN ('room', 'push', 'pull'));

ALTER TABLE source_log DROP CONSTRAINT source_log_way_check;
ALTER TABLE source_log ADD CONSTRAINT source_log_way_check CHECK (way IN ('icecast', 'srt', 'pull'));
