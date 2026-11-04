-- A lobby's picks (ADR 0024): the investigators its members have taken, in
-- seating order, until Start seats them. A restart mid-lobby keeps them.

-- One row per pick. The game id and investigator are unique together, which
-- backstops the lobby's own "already taken" refusal (Appendix III step 1:
-- "Each player chooses a different investigator"). `position` is the pick's
-- place in the seating, from 0. `weakness` is how its default deck meets
-- "1 random basic weakness": 'fixed' (a new pick's) or 'random'.

CREATE TABLE lobby_picks (
    game_id      TEXT NOT NULL REFERENCES lobbies (game_id),
    investigator TEXT NOT NULL,
    account_id   INTEGER NOT NULL REFERENCES accounts (account_id),
    weakness     TEXT NOT NULL DEFAULT 'fixed',
    position     INTEGER NOT NULL,
    UNIQUE (game_id, investigator)
);

-- The lead investigator's card code: one of the lobby's picks, or NULL while
-- it has none. Stored apart from the seating, since the creator can move the
-- lead to any pick (Appendix III step 3: "Choose one of those investigators to
-- be the lead investigator for this game.").

ALTER TABLE lobbies ADD COLUMN lead TEXT;
