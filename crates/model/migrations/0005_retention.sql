-- SPDX-License-Identifier: AGPL-3.0-only
-- Retention (ADRs 0113-0116, 0107, 0039): the station's Jurisdiction preset, the recordings the
-- sweeper deletes, consent linked to the recordings it covers, holds on complaint, and a log of
-- each sweep. The presets themselves are data in the model crate (weavesignal_model::retention),
-- versioned with the release, so a change of law reaches a station in a release note.

-- The station's retention setting. No row means No retention, the default (ADR-0113).
-- `periods` is the Custom periods as JSON (one entry per record kind); a preset's own periods
-- come from the release. `based_on` is the preset whose floors Custom keeps as warnings.
CREATE TABLE retention_setting (
    station_id           uuid        PRIMARY KEY REFERENCES station (id) ON DELETE CASCADE,
    preset               text        NOT NULL CHECK (preset ~ '^[a-z_]{1,40}$'),
    based_on             text        CHECK (based_on ~ '^[a-z_]{1,40}$'),
    periods              jsonb,
    -- Setup's question: "Does the station hold a broadcast licence?" and "Does it webcast under
    -- the US statutory licence?" Null until answered.
    broadcast_licence    boolean,
    us_statutory_licence boolean,
    updated_by           text        NOT NULL CHECK (length(updated_by) BETWEEN 1 AND 120),
    updated_at           timestamptz NOT NULL DEFAULT now(),
    CHECK ((preset = 'custom') = (periods IS NOT NULL))
);

-- Every change to the setting, with who and when: switching to a shorter preset deletes
-- records at the next sweep, so the log says who chose it.
CREATE TABLE retention_change (
    id          bigserial   PRIMARY KEY,
    station_id  uuid        NOT NULL REFERENCES station (id) ON DELETE CASCADE,
    preset      text        NOT NULL,
    periods     jsonb       NOT NULL,
    changed_by  text        NOT NULL CHECK (length(changed_by) BETWEEN 1 AND 120),
    changed_at  timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX retention_change_station ON retention_change (station_id, changed_at DESC);

-- A recording the station keeps: the programme logger and archive (`programme`), or the
-- screener's off-air call audio (`call`, 0 days in every preset). `file` is relative to the
-- control plane's `retention.recordings_dir`; nothing outside it can be named. An open
-- recording (no `ended_at`) is never swept. The sweeper deletes the file and sets
-- `deleted_at`; the row stays while a consent links it, because consent is kept 42 days after
-- the last recording it covers is deleted (ADR-0107), then the row goes too.
CREATE TABLE recording (
    id          bigserial   PRIMARY KEY,
    station_id  uuid        NOT NULL REFERENCES station (id) ON DELETE CASCADE,
    kind        text        NOT NULL CHECK (kind IN ('programme', 'call')),
    label       text        NOT NULL DEFAULT '' CHECK (length(label) <= 200),
    file        text        CHECK (
                    length(file) BETWEEN 1 AND 300
                    AND file !~ '(^/|\\|(^|/)\.\.(/|$)|[[:cntrl:]])'
                ),
    started_at  timestamptz NOT NULL,
    ended_at    timestamptz CHECK (ended_at >= started_at),
    deleted_at  timestamptz,
    created_at  timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX recording_due ON recording (station_id, kind, ended_at) WHERE deleted_at IS NULL;

-- When the call a consent was given for ended (the caller hung up, or the screener dropped
-- them). Null while the call may still be going: the pass lives at most a day, so a call is
-- taken to have ended a day after consent at the latest.
ALTER TABLE caller_consent ADD COLUMN call_ended_at timestamptz;
CREATE INDEX caller_consent_open ON caller_consent (station_id, accepted_at) WHERE call_ended_at IS NULL;

-- Consent linked to the recordings it covers (ADR-0107): the programme archive, a podcast
-- episode, the logger hour the call went out in.
CREATE TABLE consent_recording (
    consent_id   bigint NOT NULL REFERENCES caller_consent (id) ON DELETE CASCADE,
    recording_id bigint NOT NULL REFERENCES recording (id) ON DELETE CASCADE,
    PRIMARY KEY (consent_id, recording_id)
);
CREATE INDEX consent_recording_recording ON consent_recording (recording_id);

-- A hold (ADR-0116): a complaint or report pauses deletion of the records it concerns until
-- the hold is released, then the normal period resumes. A hold names records, or a stretch of
-- broadcast time (every recording and consent that overlaps it), or both. Holds are never
-- deleted: they are the log of who held what, when and why, and who released it.
CREATE TABLE retention_hold (
    id             bigserial   PRIMARY KEY,
    station_id     uuid        NOT NULL REFERENCES station (id) ON DELETE CASCADE,
    reason         text        NOT NULL CHECK (length(reason) BETWEEN 1 AND 1000),
    placed_by      text        NOT NULL CHECK (length(placed_by) BETWEEN 1 AND 120),
    placed_at      timestamptz NOT NULL DEFAULT now(),
    window_from    timestamptz,
    window_to      timestamptz,
    released_by    text        CHECK (length(released_by) BETWEEN 1 AND 120),
    released_at    timestamptz,
    release_reason text        CHECK (length(release_reason) BETWEEN 1 AND 1000),
    CHECK ((window_from IS NULL) = (window_to IS NULL)),
    CHECK (window_to > window_from),
    CHECK ((released_at IS NULL) = (released_by IS NULL)),
    CHECK ((released_at IS NULL) = (release_reason IS NULL))
);
CREATE INDEX retention_hold_open ON retention_hold (station_id) WHERE released_at IS NULL;

-- The records a hold names one by one. `record_kind` is the hold's own vocabulary, wider than
-- what this release keeps, so the chat's and the per-play log's tables can join it later.
CREATE TABLE retention_hold_record (
    hold_id     bigint NOT NULL REFERENCES retention_hold (id) ON DELETE CASCADE,
    record_kind text   NOT NULL CHECK (record_kind IN ('recording', 'consent', 'chat_message', 'moderation_record', 'per_play')),
    record_id   bigint NOT NULL,
    PRIMARY KEY (hold_id, record_kind, record_id)
);
CREATE INDEX retention_hold_record_target ON retention_hold_record (record_kind, record_id);

-- Whether an open hold covers a record: by name, or by a window its time span overlaps.
CREATE FUNCTION retention_held(
    p_station uuid, p_kind text, p_id bigint, p_from timestamptz, p_to timestamptz
) RETURNS boolean LANGUAGE sql STABLE AS $$
    SELECT EXISTS (
        SELECT 1 FROM retention_hold h
        WHERE h.station_id = p_station
          AND h.released_at IS NULL
          AND (
              EXISTS (SELECT 1 FROM retention_hold_record r
                      WHERE r.hold_id = h.id AND r.record_kind = p_kind AND r.record_id = p_id)
              OR (h.window_from IS NOT NULL
                  AND p_from < h.window_to
                  AND coalesce(p_to, 'infinity'::timestamptz) > h.window_from)
          )
    )
$$;

-- One row per sweep: when, what it deleted, what it could not. No record's content is kept.
CREATE TABLE retention_sweep (
    id                  bigserial   PRIMARY KEY,
    station_id          uuid        NOT NULL REFERENCES station (id) ON DELETE CASCADE,
    started_at          timestamptz NOT NULL,
    finished_at         timestamptz NOT NULL DEFAULT now(),
    preset              text        NOT NULL,
    recordings_deleted  integer     NOT NULL DEFAULT 0,
    files_missing       integer     NOT NULL DEFAULT 0,
    consents_deleted    integer     NOT NULL DEFAULT 0,
    held                integer     NOT NULL DEFAULT 0,
    errors              text        NOT NULL DEFAULT ''
);
CREATE INDEX retention_sweep_station ON retention_sweep (station_id, finished_at DESC);
