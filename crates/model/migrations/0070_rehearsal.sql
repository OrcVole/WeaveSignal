-- SPDX-License-Identifier: AGPL-3.0-only

-- The rehearsal studio (1.3.0, #23; docs/reference/design/rehearsal-mode-build-plan.md, sections
-- 1.5, 1.6 and 1.11). Numbered 0070 provisionally on branch `p130-rehearsal-control`, while
-- `m1`'s highest is 0047 and 1.2.1's branches may add 0048 onwards: renumber it to the next free
-- number when it is merged.

-- A rehearsal session: one show's time in the rehearsal studio, from Open to End rehearsal, Move
-- to the live studio, or the idle end. Kept so that a restart of the control plane keeps the
-- session and any move. A session's passes name its id, so a session that ended retires its
-- links for good, and reopening the studio for the same show makes a new session.
CREATE TABLE rehearsal_session (
    id          uuid        PRIMARY KEY,
    station_id  uuid        NOT NULL REFERENCES station (id),
    show_id     uuid        NOT NULL REFERENCES show (id) ON DELETE CASCADE,
    -- How the studio names whoever opened it ("Kirsty is rehearsing …").
    opened_by   text        NOT NULL CHECK (length(opened_by) BETWEEN 1 AND 120),
    opened_at   timestamptz NOT NULL DEFAULT now(),
    -- Null while the session is open.
    ended_at    timestamptz,
    -- `ended` (End rehearsal), `moved` (Move to the live studio) or `idle` (nobody in it for
    -- `rehearsal.idle_end_s`).
    ended_how   text        CHECK (ended_how IN ('ended', 'moved', 'idle')),
    -- The move (section 1.6), written before anything else is done: the identities moved, and
    -- the opening state for the show's airing ({airing_start_ms, standby_until_ms, host, on,
    -- mute, applied_at_ms}).
    moved_at    timestamptz,
    moved       jsonb       CHECK (jsonb_typeof(moved) = 'array'),
    opening     jsonb       CHECK (jsonb_typeof(opening) = 'object'),
    CHECK ((ended_at IS NULL) = (ended_how IS NULL)),
    CHECK (ended_at IS NULL OR ended_at >= opened_at),
    CHECK ((moved_at IS NULL) = (moved IS NULL) AND (moved_at IS NULL) = (opening IS NULL))
);

-- One open session per station: a second Open is refused (409) by this index, so two people
-- pressing Open at once cannot both have the studio.
CREATE UNIQUE INDEX rehearsal_session_one_open ON rehearsal_session (station_id)
    WHERE ended_at IS NULL;
CREATE INDEX rehearsal_session_by_time ON rehearsal_session (station_id, opened_at);

-- A booking holds the rehearsal studio for one show for a stretch of time (question 6, answer
-- a: first come, with optional bookings). The table is made now so that the bookings' routes
-- (step R14) need no migration of their own; nothing writes it yet. Overlapping bookings are
-- refused in code, inside a transaction that locks the station's row, which avoids needing
-- `btree_gist` (the Cloudron package's PostgreSQL may not offer it).
CREATE TABLE rehearsal_booking (
    id          uuid        PRIMARY KEY,
    station_id  uuid        NOT NULL REFERENCES station (id),
    show_id     uuid        NOT NULL REFERENCES show (id) ON DELETE CASCADE,
    starts_at   timestamptz NOT NULL,
    ends_at     timestamptz NOT NULL,
    booked_by   text        NOT NULL CHECK (length(booked_by) BETWEEN 1 AND 120),
    created_at  timestamptz NOT NULL DEFAULT now(),
    CHECK (ends_at > starts_at)
);
CREATE INDEX rehearsal_booking_by_time ON rehearsal_booking (station_id, starts_at);
