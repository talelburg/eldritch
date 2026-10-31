-- Sign-in sessions. The session cookie holds `session_id`, and a request
-- whose cookie names a live row signs in as that row's account.
--
-- A session lasts 30 days from the last request it authenticated, so each
-- such request moves `expires_at`. Removing an account deletes its rows.
-- Times are unix milliseconds, as in `games` and `actions`.

CREATE TABLE sessions (
    session_id TEXT PRIMARY KEY,     -- 256 random bits, hex-encoded
    account_id INTEGER NOT NULL REFERENCES accounts (account_id),
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL
);

CREATE INDEX sessions_by_account ON sessions (account_id);
