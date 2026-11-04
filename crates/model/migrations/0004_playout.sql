-- SPDX-License-Identifier: AGPL-3.0-only
-- M4, playout and the schedule (ADR-0044, ADR-0045, ADR-0046): the music library, clocks and
-- the week's grid, the generated log, the per-play log, anonymous listener counts, and the
-- station's playout settings. Every row carries the station (ADR-0047).

-- The station's playout settings: one row a station, made with the defaults when first read.
-- What is kept, and for how long (the legal logger, the per-play log), and whether the station
-- webcasts under the US statutory licence, are retention's (migration 0005, ADR-0113).
CREATE TABLE playout_settings (
    station_id            uuid     PRIMARY KEY REFERENCES station (id) ON DELETE CASCADE,
    time_zone             text     NOT NULL DEFAULT 'UTC' CHECK (length(time_zone) BETWEEN 1 AND 64),
    artist_separation_min integer  NOT NULL DEFAULT 60 CHECK (artist_separation_min BETWEEN 0 AND 1440),
    title_separation_min  integer  NOT NULL DEFAULT 180 CHECK (title_separation_min BETWEEN 0 AND 1440),
    album_separation_min  integer  NOT NULL DEFAULT 60 CHECK (album_separation_min BETWEEN 0 AND 1440),
    -- live: the desk, with automation on dead air; auto: the scheduled log only.
    mode                  text     NOT NULL DEFAULT 'live' CHECK (mode IN ('live', 'auto')),
    default_clock         uuid,
    updated_at            timestamptz NOT NULL DEFAULT now()
);

-- A track in the library: FLAC the media worker wrote (a UUID name), at a stored gain that brings
-- it to the station's loudness target. ISRC, album and label are what the music reports ask for.
CREATE TABLE track (
    id             uuid        PRIMARY KEY,
    station_id     uuid        NOT NULL REFERENCES station (id) ON DELETE CASCADE,
    title          text        NOT NULL CHECK (length(title) BETWEEN 1 AND 300),
    artist         text        NOT NULL CHECK (length(artist) BETWEEN 1 AND 300),
    album          text        NOT NULL DEFAULT '' CHECK (length(album) <= 300),
    label          text        NOT NULL DEFAULT '' CHECK (length(label) <= 200),
    isrc           text        CHECK (isrc ~ '^[A-Z]{2}[A-Z0-9]{3}[0-9]{7}$'),
    category       text        NOT NULL CHECK (category ~ '^[a-z0-9_-]{1,32}$'),
    file           text        NOT NULL CHECK (file ~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}[.]flac$'),
    duration_ms    integer     NOT NULL CHECK (duration_ms > 0),
    gain_db        real        NOT NULL DEFAULT 0 CHECK (gain_db BETWEEN -40 AND 20),
    loudness_lufs  real,
    true_peak_dbtp real,
    enabled        boolean     NOT NULL DEFAULT true,
    created_at     timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX track_station_category ON track (station_id, category);

-- A clock: an hour's slots in order, as JSON: [{"category": "a"}, {"cart": "<uuid>"}, ...].
CREATE TABLE clock (
    id         uuid  PRIMARY KEY,
    station_id uuid  NOT NULL REFERENCES station (id) ON DELETE CASCADE,
    name       text  NOT NULL CHECK (length(name) BETWEEN 1 AND 80),
    slots      jsonb NOT NULL CHECK (jsonb_typeof(slots) = 'array' AND jsonb_array_length(slots) <= 60),
    UNIQUE (station_id, name)
);

-- Which clock runs each local hour of the week (Monday is 0); an hour left out runs the
-- default clock.
CREATE TABLE clock_hour (
    station_id uuid     NOT NULL REFERENCES station (id) ON DELETE CASCADE,
    weekday    smallint NOT NULL CHECK (weekday BETWEEN 0 AND 6),
    hour       smallint NOT NULL CHECK (hour BETWEEN 0 AND 23),
    clock_id   uuid     NOT NULL REFERENCES clock (id) ON DELETE CASCADE,
    PRIMARY KEY (station_id, weekday, hour)
);

-- The generated log, which the log editor changes and automation plays in order.
CREATE TABLE schedule_item (
    id          bigserial   PRIMARY KEY,
    station_id  uuid        NOT NULL REFERENCES station (id) ON DELETE CASCADE,
    planned_at  timestamptz NOT NULL,
    hour_start  timestamptz NOT NULL,
    clock_id    uuid,
    slot        smallint    NOT NULL DEFAULT 0,
    kind        text        NOT NULL CHECK (kind IN ('track', 'cart')),
    track_id    uuid        REFERENCES track (id) ON DELETE SET NULL,
    cart_id     uuid,
    category    text        NOT NULL DEFAULT '',
    duration_ms integer     NOT NULL CHECK (duration_ms >= 0),
    status      text        NOT NULL DEFAULT 'planned' CHECK (status IN ('planned', 'queued', 'played', 'skipped')),
    note        text        NOT NULL DEFAULT '' CHECK (length(note) <= 300)
);
CREATE INDEX schedule_item_station_planned ON schedule_item (station_id, planned_at);

-- The per-play log: every track, cart and fallback file that went out, with its times (as
-- listeners heard them) and how many listeners were connected as it started. The words are copied, so a report still names a
-- track after it leaves the library. `source` is playout's event (boot and number), so an event
-- read twice is kept once.
CREATE TABLE play_log (
    id          bigserial   PRIMARY KEY,
    station_id  uuid        NOT NULL REFERENCES station (id) ON DELETE CASCADE,
    started_at  timestamptz NOT NULL,
    ended_at    timestamptz,
    kind        text        NOT NULL CHECK (kind IN ('track', 'cart', 'fallback')),
    track_id    uuid,
    cart_id     uuid,
    item_id     bigint,
    title       text        NOT NULL CHECK (length(title) <= 300),
    artist      text        NOT NULL DEFAULT '' CHECK (length(artist) <= 300),
    album       text        NOT NULL DEFAULT '' CHECK (length(album) <= 300),
    label       text        NOT NULL DEFAULT '' CHECK (length(label) <= 200),
    isrc        text,
    duration_ms integer     NOT NULL DEFAULT 0,
    listeners   integer     NOT NULL DEFAULT 0 CHECK (listeners >= 0),
    -- False for a desk cart a profanity dump threw away before listeners heard it.
    aired       boolean     NOT NULL DEFAULT true,
    source      text        CHECK (length(source) <= 80),
    UNIQUE (station_id, source)
);
CREATE INDEX play_log_station_started ON play_log (station_id, started_at);

-- Anonymous listening totals per hour (UTC): nothing here identifies a person, so they are kept
-- under every retention preset (ADR-0115). Only sessions of at least a minute count, and their
-- listening time (ADR-0046).
CREATE TABLE listener_hour (
    station_id       uuid        NOT NULL REFERENCES station (id) ON DELETE CASCADE,
    hour             timestamptz NOT NULL,
    listener_seconds bigint      NOT NULL DEFAULT 0 CHECK (listener_seconds >= 0),
    sessions         integer     NOT NULL DEFAULT 0 CHECK (sessions >= 0),
    peak             integer     NOT NULL DEFAULT 0 CHECK (peak >= 0),
    PRIMARY KEY (station_id, hour)
);
