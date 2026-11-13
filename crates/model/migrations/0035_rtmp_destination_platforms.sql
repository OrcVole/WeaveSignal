-- SPDX-License-Identifier: AGPL-3.0-only

-- RTMP out's platforms (1.0.2's presets, issue found on the test station 2026-11-12): 1.0.2 added
-- YouTube, Owncast, X and Gab to the studio app's forms, but migration 0024's check still allowed
-- only `rumble`, `odysee`, `peertube` and `custom`, so saving one of the new forms answered 500
-- ("the database refused that"). The check now allows every form `rtmp::PLATFORMS` offers;
-- `rtmp_db.rs` saves each of them, so the two cannot part again unnoticed.
ALTER TABLE rtmp_destination DROP CONSTRAINT rtmp_destination_platform_check;
ALTER TABLE rtmp_destination ADD CONSTRAINT rtmp_destination_platform_check
    CHECK (platform IN ('youtube', 'owncast', 'x', 'gab', 'rumble', 'odysee', 'peertube', 'custom'));
