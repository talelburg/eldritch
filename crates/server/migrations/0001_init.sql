-- Event-sourced action log. A game is a seed GameState (the scenario
-- setup() output) plus an ordered sequence of applied actions; replay
-- folds the actions over the seed to reproduce live state bit-for-bit.

CREATE TABLE games (
    game_id     TEXT PRIMARY KEY,
    scenario_id TEXT NOT NULL,
    seed_state  TEXT NOT NULL,   -- serde_json of the setup() GameState
    created_at  TEXT NOT NULL
);

CREATE TABLE actions (
    game_id TEXT NOT NULL REFERENCES games (game_id),
    seq     INTEGER NOT NULL,
    action  TEXT NOT NULL,       -- serde_json of Action
    PRIMARY KEY (game_id, seq)
);
