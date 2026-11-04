-- A game starts as a lobby (ADR 0024): a scenario, a creator and a link, and
-- nothing seated. Its row lives here until Start, which writes the `games` row
-- under the same game id. A `games` row still means "started and loadable".

CREATE TABLE lobbies (
    game_id     TEXT PRIMARY KEY,
    scenario_id TEXT NOT NULL,
    creator     INTEGER NOT NULL REFERENCES accounts (account_id),
    created_at  INTEGER NOT NULL      -- unix millis; diagnostic, never read back
);

-- Every account that has opened a game's link, lobby or started game alike,
-- in the order they joined (rowid). A membership outlives Start, so `game_id`
-- refers to neither `lobbies` nor `games`: a game's row is in one, then the
-- other.

CREATE TABLE game_members (
    game_id    TEXT NOT NULL,
    account_id INTEGER NOT NULL REFERENCES accounts (account_id),
    PRIMARY KEY (game_id, account_id)
);
