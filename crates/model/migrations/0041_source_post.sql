-- SPDX-License-Identifier: AGPL-3.0-only

-- The post's rule (ADR-0280; design sections 3.3 to 3.6 and 10): the control plane works out, every
-- second, who goes on air at a show's post from the schedule and this log, so it survives a restart.
-- The log gains the words for what the schedule does and sees: on air, off air, link lost, back,
-- place let go, covered (automation took a lost link's air), go and not yet (the late count),
-- and hold (a producer holds the grace for one handover). `reason` is `schedule` for the
-- schedule's own acts, `desk` for a press it only saw, or the person's own press.
ALTER TABLE source_log DROP CONSTRAINT source_log_event_check;
ALTER TABLE source_log ADD CONSTRAINT source_log_event_check CHECK (event IN (
    'admitted', 'test', 'refused', 'ended', 'disconnected', 'let_back',
    'on_air', 'off_air', 'link_lost', 'back', 'place_let_go', 'covered',
    'go', 'not_yet', 'hold', 'hold_released'));

-- The rule reads the station's recent lines in one query.
CREATE INDEX source_log_station_at ON source_log (station_id, at_ms DESC);

-- The grace is Off, a minute, two or five in the studio; the rule takes any length up to ten
-- minutes, so a test can run in seconds.
ALTER TABLE source_settings DROP CONSTRAINT source_settings_grace_s_check;
ALTER TABLE source_settings ADD CONSTRAINT source_settings_grace_s_check CHECK (grace_s BETWEEN 0 AND 600);
