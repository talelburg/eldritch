-- A stored game records the account that created it. Until the lobby stores an
-- assignment, a load gives the creator every investigator, and any other
-- account that opens the game is a viewer.
--
-- The column is nullable because SQLite can add a REFERENCES column to a table
-- that already has rows only with a NULL default. Every game this build stores
-- writes it. A row from before it has no creator, and was stored in format
-- version 2 or earlier, so a load refuses it by format version as an
-- unresumable game. Its rows are kept, so rolling the deploy back recovers it
-- (ADR 0022).

ALTER TABLE games ADD COLUMN creator INTEGER REFERENCES accounts (account_id);
