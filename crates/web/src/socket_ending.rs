//! How a game's WebSocket ended, sorted from what the client saw of it, and
//! how long to wait before the next attempt.

use std::time::Duration;

/// How one socket lifetime ended, which decides what the transport does next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SocketEnding {
    /// The server closed it with
    /// [`CLOSE_UNAUTHENTICATED`](protocol::CLOSE_UNAUTHENTICATED): the
    /// session is missing or has ended. Show the sign-in screen.
    Unauthenticated,
    /// The server closed it with
    /// [`CLOSE_SERVER_RESTARTING`](protocol::CLOSE_SERVER_RESTARTING): it is
    /// shutting down for a restart. Reconnect, saying so until the next
    /// `Hello`.
    Restarting,
    /// The server closed it with
    /// [`CLOSE_UNRESUMABLE`](protocol::CLOSE_UNRESUMABLE): the game exists
    /// but this server's build can't rebuild it. Stop.
    Unresumable,
    /// The server closed it with
    /// [`CLOSE_GAME_NOT_FOUND`](protocol::CLOSE_GAME_NOT_FOUND): the server
    /// has no game at this link. Go to the home page.
    StaleId,
    /// Any other ending: a network drop, or a server down or going down
    /// without saying why. Reconnect.
    Disconnected,
}

/// Sort a socket's ending from the close code, if the socket reported one.
///
/// Only the server's explicit codes say anything about the game, the session
/// or the server, and each means the same whether or not `Hello` arrived.
/// Every other ending is a disconnect: a server that is down refuses the
/// connect before `Hello` too, and must not send the player away from their
/// game.
#[must_use]
pub fn sort(close_code: Option<u16>) -> SocketEnding {
    match close_code {
        Some(protocol::CLOSE_UNAUTHENTICATED) => SocketEnding::Unauthenticated,
        Some(protocol::CLOSE_SERVER_RESTARTING) => SocketEnding::Restarting,
        Some(protocol::CLOSE_UNRESUMABLE) => SocketEnding::Unresumable,
        Some(protocol::CLOSE_GAME_NOT_FOUND) => SocketEnding::StaleId,
        _ => SocketEnding::Disconnected,
    }
}

/// The longest wait between reconnect attempts.
const BACKOFF_CAP: Duration = Duration::from_secs(10);

/// How long to wait before reconnect attempt `attempt`, counted from 0 since
/// the last `Hello`: 1 s, doubling each attempt, capped at 10 s. No jitter:
/// one small host makes a reconnect stampede harmless.
#[must_use]
pub fn backoff(attempt: u32) -> Duration {
    // 2^4 s already passes the cap, so a larger shift is never needed.
    Duration::from_secs(1 << attempt.min(4)).min(BACKOFF_CAP)
}

#[cfg(test)]
mod tests {
    use protocol::{
        CLOSE_GAME_NOT_FOUND, CLOSE_SERVER_RESTARTING, CLOSE_UNAUTHENTICATED, CLOSE_UNRESUMABLE,
    };

    use super::*;

    #[test]
    fn unauthenticated_is_sorted_by_its_code() {
        assert_eq!(
            sort(Some(CLOSE_UNAUTHENTICATED)),
            SocketEnding::Unauthenticated
        );
    }

    #[test]
    fn unresumable_is_sorted_by_its_code() {
        assert_eq!(sort(Some(CLOSE_UNRESUMABLE)), SocketEnding::Unresumable);
    }

    #[test]
    fn server_restarting_is_sorted_by_its_code() {
        assert_eq!(
            sort(Some(CLOSE_SERVER_RESTARTING)),
            SocketEnding::Restarting
        );
    }

    #[test]
    fn game_not_found_is_a_stale_id() {
        assert_eq!(sort(Some(CLOSE_GAME_NOT_FOUND)), SocketEnding::StaleId);
    }

    #[test]
    fn any_other_close_is_a_disconnect() {
        // 1006 is what a browser reports for a socket that dropped without a
        // close frame, which is how a server that is down ends it.
        for code in [Some(1006), Some(1000), None] {
            assert_eq!(sort(code), SocketEnding::Disconnected, "{code:?}");
        }
    }

    #[test]
    fn backoff_doubles_from_one_second_to_a_ten_second_cap() {
        let waits: Vec<u64> = (0..6).map(|n| backoff(n).as_secs()).collect();
        assert_eq!(waits, [1, 2, 4, 8, 10, 10]);
        assert_eq!(backoff(u32::MAX), Duration::from_secs(10));
    }
}
