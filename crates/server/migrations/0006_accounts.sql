-- The allowlist (ADR 0023). An entry is an Account: whoever signs in with a
-- provider-verified email the entry holds is that account. The host edits the
-- list with `server accounts`.
--
-- An entry is disabled rather than deleted, so stored games that refer to its
-- id stay intact. AUTOINCREMENT keeps an id from ever being reused.

CREATE TABLE accounts (
    account_id   INTEGER PRIMARY KEY AUTOINCREMENT,
    display_name TEXT NOT NULL,      -- set by the host, shown at the table
    disabled     INTEGER NOT NULL DEFAULT 0
);

-- One row per address. Stored lowercased, so the uniqueness check and a
-- sign-in's lookup agree whatever case a provider reports. Unique across all
-- accounts, so a sign-in never matches two.
CREATE TABLE account_emails (
    email      TEXT PRIMARY KEY,
    account_id INTEGER NOT NULL REFERENCES accounts (account_id)
);
