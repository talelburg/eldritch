-- A stored game records the account that created it. Until the lobby (#1239)
-- stores an assignment, a load gives the creator every investigator, and any
-- other account that opens the game is a viewer.
--
-- SQLite can't add a NOT NULL column that references another table to a table
-- with rows, so both tables are dropped and recreated, as 0004 did. Their rows
-- record no creator, and every one was stored in format version 2 or earlier,
-- which this build loads as an unresumable game anyway.

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
    -- The account that created the game, which holds every investigator.
    creator                 INTEGER NOT NULL REFERENCES accounts (account_id),
    scenario_id             TEXT NOT NULL,
    roster                  TEXT NOT NULL,   -- JSON array of roster entries
    lead                    INTEGER NOT NULL,
    -- The u64 setup seed, its bits stored as a signed 64-bit integer.
    rng_seed                INTEGER NOT NULL,
    interactive_acknowledge INTEGER NOT NULL,
    setup_fingerprint       TEXT NOT NULL,   -- JSON of card codes
    created_at              INTEGER NOT NULL -- unix millis
);

CREATE TABLE actions (
    game_id TEXT NOT NULL REFERENCES games (game_id),
    seq     INTEGER NOT NULL,
    answer  TEXT NOT NULL,       -- JSON of the logged answer
    -- RngState.draws after the action applied, its u64 bits stored as a
    -- signed 64-bit integer.
    draws   INTEGER NOT NULL,
    at      INTEGER NOT NULL,    -- unix millis when the action was applied
    PRIMARY KEY (game_id, seq)
);
