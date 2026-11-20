-- SPDX-License-Identifier: AGPL-3.0-only

-- The handover window (Phase 17c, ADR-0231; design note `handover-window.md`): the stretch before a
-- show's post in which the desk says who is next, from where and whether they are ready, and the
-- incoming host is given one heads-up. Its length is a station rule beside the grace: 300 s by
-- default, 0 turns it off, at most 30 minutes.
ALTER TABLE source_settings
    ADD COLUMN window_s integer NOT NULL DEFAULT 300 CHECK (window_s BETWEEN 0 AND 1800);

-- The heads-up is given once an airing: the control plane writes `heads_up` under the post's own
-- key before it tells anyone, so a restart reads the line and does not tell them again.
ALTER TABLE source_log DROP CONSTRAINT source_log_event_check;
ALTER TABLE source_log ADD CONSTRAINT source_log_event_check CHECK (event IN (
    'admitted', 'test', 'refused', 'ended', 'disconnected', 'let_back',
    'on_air', 'off_air', 'link_lost', 'back', 'place_let_go', 'covered',
    'go', 'not_yet', 'hold', 'hold_released', 'heads_up'));
