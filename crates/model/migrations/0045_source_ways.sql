-- SPDX-License-Identifier: AGPL-3.0-only

-- Phase 17g: two more ways in for hosts' own studios, and the presenter kit's settings.
--
-- NUMBERING: 0051 on branch `bridge`, renumbered 0045 when the 1.2 branches merged. 0042 (17b's
-- `pull`) and 0044 (17d's `whip`) change the same two CHECKs (`show_key_hint_check`,
-- `source_log_way_check`, the names Postgres gave the inline CHECKs of migration 0040, as 0041
-- relies on for `source_log_event_check`), so the two lists here keep `pull` and `whip`.
--
-- `bridge`: the studio app carries a host's software over one WebSocket on the station's public
-- address (the Icecast password as its key). `vban`: a studio PC on the station's network sends
-- VBAN, bound to a key in the kit's settings; it has no secret of its own.
ALTER TABLE show_key DROP CONSTRAINT show_key_hint_check;
ALTER TABLE show_key ADD CONSTRAINT show_key_hint_check
    CHECK (hint IN ('icecast', 'srt', 'pull', 'whip', 'bridge', 'vban'));

ALTER TABLE source_log DROP CONSTRAINT source_log_way_check;
ALTER TABLE source_log ADD CONSTRAINT source_log_way_check
    CHECK (way IN ('icecast', 'srt', 'pull', 'whip', 'bridge', 'vban'));

-- The presenter kit's server half (Settings › Presenter kit): the preset, VBAN in (each bound to
-- a show key) and VBAN out, the address they use, as one document the control plane checks
-- (`crates/control/src/kit.rs`). No row: the kit is off. Whether VBAN may be used at all is the
-- installation's (`[kit] vban` in the settings file; never on Cloudron), not this row's.
CREATE TABLE kit_settings (
    station_id uuid   PRIMARY KEY REFERENCES station (id) ON DELETE CASCADE,
    body       jsonb  NOT NULL,
    updated_by text   NOT NULL CHECK (length(updated_by) BETWEEN 1 AND 120),
    updated_ms bigint NOT NULL
);
