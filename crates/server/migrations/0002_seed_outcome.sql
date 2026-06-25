-- The seed GameState may itself be paused at AwaitingInput (seating now runs
-- at creation: the seed is seated + mulligan-pending, #459). The action log is
-- ResolveInput-only and may be empty, so `load` cannot reconstruct the seed's
-- outcome by replay — persist it alongside the seed.
ALTER TABLE games ADD COLUMN seed_outcome TEXT NOT NULL DEFAULT '"Done"';
