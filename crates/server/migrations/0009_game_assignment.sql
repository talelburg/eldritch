-- A stored game's assignment: which account holds each of its investigators,
-- one row per investigator. It sits beside the `games` row rather than among
-- its creation inputs, so the format version doesn't move, and a reassignment
-- rewrites these rows and leaves the inputs and the log alone (ADR 0024).
--
-- This ends the interim 0008's header describes, in which a load gave the
-- creator every investigator. 0008 is left as it landed, since sqlx checksums
-- a migration's whole text, comments included. A load now reads the holders
-- from here, and a game with no rows here is unresumable: every game stored
-- before this migration has none, and nothing falls back to the creator.

CREATE TABLE game_assignments (
    game_id TEXT NOT NULL REFERENCES games (game_id),
    investigator INTEGER NOT NULL,
    account_id INTEGER NOT NULL REFERENCES accounts (account_id),
    PRIMARY KEY (game_id, investigator)
);
