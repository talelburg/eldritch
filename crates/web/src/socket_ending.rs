//! How a game's WebSocket ended, sorted from what the client saw of it, and
//! how long to wait before the next attempt.

use std::time::Duration;

use crate::url::Ended;

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
    /// The server closed it with
    /// [`CLOSE_NO_LONGER_A_MEMBER`](protocol::CLOSE_NO_LONGER_A_MEMBER): the
    /// account left the lobby or was removed from it. Go to the home page,
    /// saying so. A reconnect would make it a member again.
    NoLongerMember,
    /// The server closed it with
    /// [`CLOSE_GAME_DELETED`](protocol::CLOSE_GAME_DELETED): the creator
    /// deleted the game. Go to the home page, saying so.
    Deleted,
    /// Any other ending: a network drop, or a server down or going down
    /// without saying why. Reconnect.
    Disconnected,
}

/// What the transport does after a socket ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NextStep {
    /// Reconnect to the same game, after a backoff.
    Reconnect,
    /// Stop, and show the sign-in screen.
    SignIn,
    /// Stop, and say the game is unresumable.
    Unresumable,
    /// Stop, and go to "Your games", saying why when there is an ending to
    /// name.
    GoHome(Option<Ended>),
}

impl SocketEnding {
    /// What the transport does after this ending.
    #[must_use]
    pub fn next_step(self) -> NextStep {
        match self {
            SocketEnding::Restarting | SocketEnding::Disconnected => NextStep::Reconnect,
            SocketEnding::Unauthenticated => NextStep::SignIn,
            SocketEnding::Unresumable => NextStep::Unresumable,
            SocketEnding::StaleId => NextStep::GoHome(None),
            SocketEnding::NoLongerMember => NextStep::GoHome(Some(Ended::NotMember)),
            SocketEnding::Deleted => NextStep::GoHome(Some(Ended::Deleted)),
        }
    }
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
        Some(protocol::CLOSE_NO_LONGER_A_MEMBER) => SocketEnding::NoLongerMember,
        Some(protocol::CLOSE_GAME_DELETED) => SocketEnding::Deleted,
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
        CLOSE_GAME_DELETED, CLOSE_GAME_NOT_FOUND, CLOSE_NO_LONGER_A_MEMBER,
        CLOSE_SERVER_RESTARTING, CLOSE_UNAUTHENTICATED, CLOSE_UNRESUMABLE,
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
    fn no_longer_a_member_is_sorted_by_its_code() {
        assert_eq!(
            sort(Some(CLOSE_NO_LONGER_A_MEMBER)),
            SocketEnding::NoLongerMember
        );
    }

    #[test]
    fn no_longer_a_member_goes_home_saying_why_and_never_reconnects() {
        assert_eq!(
            SocketEnding::NoLongerMember.next_step(),
            NextStep::GoHome(Some(Ended::NotMember))
        );
    }

    #[test]
    fn deleted_is_sorted_by_its_code() {
        assert_eq!(sort(Some(CLOSE_GAME_DELETED)), SocketEnding::Deleted);
    }

    #[test]
    fn deleted_goes_home_saying_why_and_never_reconnects() {
        assert_eq!(
            SocketEnding::Deleted.next_step(),
            NextStep::GoHome(Some(Ended::Deleted))
        );
    }

    #[test]
    fn a_stale_id_goes_home_saying_nothing() {
        assert_eq!(SocketEnding::StaleId.next_step(), NextStep::GoHome(None));
    }

    #[test]
    fn only_a_disconnect_or_a_restart_reconnects() {
        let reconnecting: Vec<_> = [
            SocketEnding::Unauthenticated,
            SocketEnding::Restarting,
            SocketEnding::Unresumable,
            SocketEnding::StaleId,
            SocketEnding::NoLongerMember,
            SocketEnding::Deleted,
            SocketEnding::Disconnected,
        ]
        .into_iter()
        .filter(|ending| ending.next_step() == NextStep::Reconnect)
        .collect();
        assert_eq!(
            reconnecting,
            [SocketEnding::Restarting, SocketEnding::Disconnected]
        );
        assert_eq!(SocketEnding::Unauthenticated.next_step(), NextStep::SignIn);
        assert_eq!(SocketEnding::Unresumable.next_step(), NextStep::Unresumable);
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
