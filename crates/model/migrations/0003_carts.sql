-- SPDX-License-Identifier: AGPL-3.0-only
-- The cart wall (ADR-0020): banks of up to 24 carts, each a row with a file, a gain, a ramp and
-- a mode (report 08), scoped to the station (ADR-0047).

-- A bank: Ctrl+F1 to Ctrl+F5 on the studio app; its colour is a slot of the twelve-colour
-- cart palette, the show's colour everywhere (ADR-0066).
CREATE TABLE cart_bank (
    station_id  uuid     NOT NULL REFERENCES station (id) ON DELETE CASCADE,
    number      smallint NOT NULL CHECK (number BETWEEN 1 AND 5),
    name        text     NOT NULL CHECK (length(name) BETWEEN 1 AND 80),
    colour_slot smallint NOT NULL CHECK (colour_slot BETWEEN 1 AND 12),
    PRIMARY KEY (station_id, number)
);

-- A cart: slot 1 to 24 is its key on the wall (1-6, Q-Y, A-H, Z-N). `file` is the FLAC the
-- media worker wrote, named by a UUID; no file, or a file gone from the disk, is FILE MISSING.
-- `gain_db` is applied when it fires, so it sounds at the station's loudness target.
CREATE TABLE cart (
    id             uuid        PRIMARY KEY,
    station_id     uuid        NOT NULL,
    bank           smallint    NOT NULL,
    slot           smallint    NOT NULL CHECK (slot BETWEEN 1 AND 24),
    label          text        NOT NULL CHECK (length(label) BETWEEN 1 AND 120),
    detail         text        NOT NULL DEFAULT '' CHECK (length(detail) <= 120),
    mode           text        NOT NULL CHECK (mode IN ('one_shot', 'bed', 'sweeper')),
    file           text        CHECK (file ~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}[.]flac$'),
    duration_ms    integer     NOT NULL DEFAULT 0 CHECK (duration_ms >= 0),
    gain_db        real        NOT NULL DEFAULT 0 CHECK (gain_db BETWEEN -40 AND 20),
    loudness_lufs  real,
    true_peak_dbtp real,
    ramp_ms        integer     CHECK (ramp_ms >= 0),
    origin         text        NOT NULL DEFAULT '' CHECK (length(origin) <= 300),
    licence        text        NOT NULL DEFAULT '' CHECK (length(licence) <= 80),
    created_at     timestamptz NOT NULL DEFAULT now(),
    updated_at     timestamptz NOT NULL DEFAULT now(),
    UNIQUE (station_id, bank, slot),
    FOREIGN KEY (station_id, bank) REFERENCES cart_bank (station_id, number) ON DELETE CASCADE
);
