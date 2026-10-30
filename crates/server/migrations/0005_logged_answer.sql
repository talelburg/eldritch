-- A log entry is a server-owned logged answer, not the engine's `Action` JSON
-- (ADR 0022): the response with each picked option's key, the identity of the
-- prompt it answered, and the RNG checkpoint after it applied.
--
-- Old rows are not converted. No real games exist yet (ADR 0002: "acceptable
-- now and will not be later"), so the table is dropped and recreated. A game
-- left without its log loads as a version error: its `format_version` is 1.

DROP TABLE actions;

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
