-- SPDX-License-Identifier: AGPL-3.0-only

-- The station's look on its public pages (ADR-0237): an accent colour, a brand icon (also the
-- favicon) and a banner. The pictures are the PNGs the media worker wrote from an upload,
-- never the upload; each has a version (the first 16 hex digits of its SHA-256) that its
-- address carries, so a browser may keep it for good. Kept here, not in a volume, so the
-- database's backup carries the look. No row, or NULLs, means the product's own look.
CREATE TABLE station_branding (
    station_id     uuid        PRIMARY KEY REFERENCES station (id) ON DELETE CASCADE,
    accent         text        CHECK (accent IS NULL OR accent ~ '^#[0-9a-f]{6}$'),
    icon           bytea       CHECK (icon IS NULL OR octet_length(icon) BETWEEN 1 AND 1048576),
    icon_version   text        CHECK (icon_version IS NULL OR icon_version ~ '^[0-9a-f]{16}$'),
    icon_width     integer,
    icon_height    integer,
    banner         bytea       CHECK (banner IS NULL OR octet_length(banner) BETWEEN 1 AND 8388608),
    banner_version text        CHECK (banner_version IS NULL OR banner_version ~ '^[0-9a-f]{16}$'),
    banner_width   integer,
    banner_height  integer,
    updated_by     text        NOT NULL CHECK (length(updated_by) BETWEEN 1 AND 120),
    updated_at     timestamptz NOT NULL DEFAULT now(),
    CHECK ((icon IS NULL) = (icon_version IS NULL)),
    CHECK ((banner IS NULL) = (banner_version IS NULL))
);
