-- Persist the events emitted during scenario setup (seat_and_open), so a
-- session reloaded from the DB can still surface them in the client's event
-- log (#512). Existing rows predate the feature; default to an empty JSON
-- array so they load as "no setup events" rather than failing NOT NULL.
ALTER TABLE games ADD COLUMN setup_events TEXT NOT NULL DEFAULT '[]';
