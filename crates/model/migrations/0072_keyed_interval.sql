-- SPDX-License-Identifier: AGPL-3.0-only

-- Keyed shows enforced (1.3.2, PLAN.md Phase 19b): when the control plane held playout's public
-- gate shut for a keyed show. Recorded before the gate is told, closed after it opens; an open
-- row (end_at NULL) is a window still shut, also after a restart. The public HLS is judged by
-- these (a segment written while or just after a show's gate was shut is that show's, and is
-- never served on the public path), so they must outlive the control plane. Rows older than a
-- day are not read; the sweep deletes them after a week.
CREATE TABLE keyed_interval (
    id         bigserial   PRIMARY KEY,
    station_id uuid        NOT NULL REFERENCES station (id) ON DELETE CASCADE,
    show_id    uuid        NOT NULL,
    start_at   timestamptz NOT NULL,
    end_at     timestamptz,
    CHECK (end_at IS NULL OR end_at >= start_at)
);
CREATE INDEX keyed_interval_recent ON keyed_interval (station_id, start_at DESC);
CREATE INDEX keyed_interval_open ON keyed_interval (station_id) WHERE end_at IS NULL;
