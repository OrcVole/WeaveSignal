-- SPDX-License-Identifier: AGPL-3.0-only
-- Shows, series and episodes (A42, A43; ADR-0055): three levels, each field on a lower level
-- either inherits from the level above (NULL) or is set on its own level. What has aired is
-- frozen: `episode.frozen` keeps the values as broadcast, and the cards rendered then.
-- Every table carries the station (ADR-0047).

-- The remembered guest list: a returning guest is one pick (0008 section 4).
CREATE TABLE person (
    id         uuid        PRIMARY KEY,
    station_id uuid        NOT NULL REFERENCES station (id) ON DELETE CASCADE,
    name       text        NOT NULL CHECK (length(name) BETWEEN 1 AND 128),
    href       text        NOT NULL DEFAULT '' CHECK (length(href) <= 300),
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (station_id, name)
);

-- A show. Its colour is its bank's slot in the twelve-colour palette (ADR-0066); its card
-- layout is Poster or Slate (ADR-0067). The weekly slot is a weekday (1 Monday to 7 Sunday),
-- a start in minutes after midnight in the station's zone, and a length in minutes.
CREATE TABLE show (
    id            uuid        PRIMARY KEY,
    station_id    uuid        NOT NULL REFERENCES station (id) ON DELETE CASCADE,
    slug          text        NOT NULL CHECK (slug ~ '^[a-z0-9]+(-[a-z0-9]+)*$' AND length(slug) <= 60),
    name          text        NOT NULL CHECK (length(name) BETWEEN 1 AND 160),
    description   text        NOT NULL DEFAULT '' CHECK (length(description) <= 4000),
    colour_slot   smallint    NOT NULL CHECK (colour_slot BETWEEN 1 AND 12),
    layout        text        NOT NULL DEFAULT 'poster' CHECK (layout IN ('poster', 'slate')),
    slot_weekday  smallint    CHECK (slot_weekday BETWEEN 1 AND 7),
    slot_start    smallint    CHECK (slot_start BETWEEN 0 AND 1439),
    slot_minutes  smallint    CHECK (slot_minutes BETWEEN 1 AND 1440),
    hosts         uuid[]      NOT NULL DEFAULT '{}',
    tags          text[]      NOT NULL DEFAULT '{}',
    explicit      boolean     NOT NULL DEFAULT false,
    -- Assigned once, from the feed's first address, and kept (the namespace's podcast:guid).
    feed_guid     uuid,
    -- The current cards: {"link": file, "post": file, "square": file, "alt": text}.
    cards         jsonb,
    created_at    timestamptz NOT NULL DEFAULT now(),
    updated_at    timestamptz NOT NULL DEFAULT now(),
    UNIQUE (station_id, slug),
    UNIQUE (station_id, id),
    CHECK ((slot_weekday IS NULL) = (slot_start IS NULL))
);

-- A series ("Series 3"). NULL inherits the show's value.
CREATE TABLE series (
    id               uuid        PRIMARY KEY,
    station_id       uuid        NOT NULL,
    show_id          uuid        NOT NULL,
    number           integer     NOT NULL CHECK (number BETWEEN 1 AND 9999),
    title            text        CHECK (length(title) BETWEEN 1 AND 160),
    description      text        CHECK (length(description) <= 4000),
    hosts            uuid[],
    slot_weekday     smallint    CHECK (slot_weekday BETWEEN 1 AND 7),
    slot_start       smallint    CHECK (slot_start BETWEEN 0 AND 1439),
    slot_minutes     smallint    CHECK (slot_minutes BETWEEN 1 AND 1440),
    -- The first episode's date, `2026-09-05`, from which the schedule rule counts.
    starts_on        text        CHECK (starts_on ~ '^[0-9]{4}-[0-9]{2}-[0-9]{2}$'),
    planned_episodes integer     CHECK (planned_episodes BETWEEN 1 AND 520),
    tags             text[],
    cards            jsonb,
    created_at       timestamptz NOT NULL DEFAULT now(),
    updated_at       timestamptz NOT NULL DEFAULT now(),
    UNIQUE (show_id, number),
    UNIQUE (station_id, id),
    FOREIGN KEY (station_id, show_id) REFERENCES show (station_id, id) ON DELETE CASCADE,
    CHECK ((slot_weekday IS NULL) = (slot_start IS NULL))
);

-- An episode. Its number and air time are filled in when it is created (the last plus one,
-- the next slot from the series' rule) and are its own from then; hosts, tags and length
-- inherit (NULL) until someone sets them. Guests are per episode.
CREATE TABLE episode (
    id          uuid        PRIMARY KEY,
    station_id  uuid        NOT NULL,
    series_id   uuid        NOT NULL,
    number      integer     NOT NULL CHECK (number BETWEEN 1 AND 99999),
    title       text        NOT NULL DEFAULT '' CHECK (length(title) <= 200),
    description text        NOT NULL DEFAULT '' CHECK (length(description) <= 4000),
    air_at      timestamptz,
    minutes     smallint    CHECK (minutes BETWEEN 1 AND 1440),
    hosts       uuid[],
    guests      uuid[]      NOT NULL DEFAULT '{}',
    tags        text[],
    -- Set when it goes on air: the values as broadcast. A frozen episode is not edited.
    aired_at    timestamptz,
    frozen      jsonb,
    -- The published files: {"enclosure": {...}, "opus": {...}} (ADR-0018).
    files       jsonb,
    cards       jsonb,
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now(),
    UNIQUE (series_id, number),
    FOREIGN KEY (station_id, series_id) REFERENCES series (station_id, id) ON DELETE CASCADE,
    CHECK ((aired_at IS NULL) = (frozen IS NULL))
);

CREATE INDEX episode_air_at ON episode (station_id, air_at);
