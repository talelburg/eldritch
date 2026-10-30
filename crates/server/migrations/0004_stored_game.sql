-- A stored game is its creation inputs plus its action log (ADR 0022). No
-- column holds serialized engine state: a load re-runs setup from the inputs,
-- checks the result against the setup fingerprint, and replays the log.
--
-- Old rows are not converted. No real games exist yet (ADR 0002: "acceptable
-- now and will not be later"), so both tables are dropped and recreated.

DROP TABLE actions;
DROP TABLE games;

CREATE TABLE games (
    game_id                 TEXT PRIMARY KEY,
    -- Bumped whenever the shape of the creation inputs or of a log entry
    -- changes. Only the current version loads.
    format_version          INTEGER NOT NULL,
    -- protocol::build_id() of the build that created the game. Diagnostic
    -- only: a build mismatch alone never refuses a load.
    created_build           TEXT NOT NULL,
    scenario_id             TEXT NOT NULL,
    roster                  TEXT NOT NULL,   -- JSON array of roster entries
    lead                    INTEGER NOT NULL,
    -- The u64 setup seed, its bits stored as a signed 64-bit integer.
    rng_seed                INTEGER NOT NULL,
    interactive_acknowledge INTEGER NOT NULL,
    setup_fingerprint       TEXT NOT NULL,   -- JSON of card codes
    created_at              TEXT NOT NULL    -- unix millis
);

CREATE TABLE actions (
    game_id TEXT NOT NULL REFERENCES games (game_id),
    seq     INTEGER NOT NULL,
    action  TEXT NOT NULL,       -- serde_json of Action
    at      INTEGER NOT NULL,    -- unix millis when the action was applied
    PRIMARY KEY (game_id, seq)
);
