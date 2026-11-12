-- SPDX-License-Identifier: AGPL-3.0-only

-- A cart wall per show (ADR-0276, the operator's answer of 2026-11-12): beside the station's
-- banks, which every show shares, a show can have banks of its own, so the next DJ preparing
-- their show sets up their carts without touching the show on air. The station's banks and the
-- show's are numbered apart, 1 to 5 each: `show_id` is NULL for a station bank. Every bank that
-- exists when this migration runs is the station's, so an upgrade loses nothing.
--
-- A bank now has an id of its own, and a cart names its bank by it: the station's bank 2 and a
-- show's bank 2 are different banks.
ALTER TABLE cart_bank ADD COLUMN id uuid;
UPDATE cart_bank SET id = gen_random_uuid();
ALTER TABLE cart_bank ALTER COLUMN id SET NOT NULL;
ALTER TABLE cart_bank ADD COLUMN show_id uuid REFERENCES show (id) ON DELETE CASCADE;

ALTER TABLE cart ADD COLUMN bank_id uuid;
UPDATE cart c SET bank_id = b.id FROM cart_bank b
    WHERE b.station_id = c.station_id AND b.number = c.bank;
ALTER TABLE cart ALTER COLUMN bank_id SET NOT NULL;

ALTER TABLE cart DROP CONSTRAINT cart_station_id_bank_fkey;
ALTER TABLE cart DROP CONSTRAINT cart_station_id_bank_slot_key;
ALTER TABLE cart_bank DROP CONSTRAINT cart_bank_pkey;
ALTER TABLE cart_bank ADD PRIMARY KEY (id);

-- One station bank per number, and one bank per number for each show.
CREATE UNIQUE INDEX cart_bank_station_number ON cart_bank (station_id, number) WHERE show_id IS NULL;
CREATE UNIQUE INDEX cart_bank_show_number ON cart_bank (show_id, number) WHERE show_id IS NOT NULL;

-- `cart.bank` stays the bank's number, beside the bank it belongs to.
ALTER TABLE cart ADD CONSTRAINT cart_bank_id_fkey
    FOREIGN KEY (bank_id) REFERENCES cart_bank (id) ON DELETE CASCADE;
ALTER TABLE cart ADD CONSTRAINT cart_bank_id_slot_key UNIQUE (bank_id, slot);
