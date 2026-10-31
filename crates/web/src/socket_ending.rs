//! How a game's WebSocket ended, sorted from what the client saw of it.

/// How one socket lifetime ended, which decides what the transport does next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SocketEnding {
    /// The server closed it with
    /// [`CLOSE_UNAUTHENTICATED`](protocol::CLOSE_UNAUTHENTICATED): the
    /// session is missing or has ended. Show the sign-in screen.
    Unauthenticated,
    /// It closed before any `Hello` without that code. A known game sends
    /// `Hello` at once, so the server doesn't know the id.
    StaleId,
    /// It closed after `Hello`: an ordinary disconnect. Reconnect.
    Disconnected,
}

impl SocketEnding {
    /// Whether the saved game id survives this ending. Only a stale id is
    /// dropped. Signing back in returns to the same game.
    #[must_use]
    pub fn keeps_saved_game(self) -> bool {
        self != Self::StaleId
    }
}

/// Sort a socket's ending from whether `Hello` arrived and the close code, if
/// the socket reported one. The unauthenticated code wins either way: it says
/// the session is gone whenever it comes.
#[must_use]
pub fn sort(saw_hello: bool, close_code: Option<u16>) -> SocketEnding {
    if close_code == Some(protocol::CLOSE_UNAUTHENTICATED) {
        SocketEnding::Unauthenticated
    } else if saw_hello {
        SocketEnding::Disconnected
    } else {
        SocketEnding::StaleId
    }
}

#[cfg(test)]
mod tests {
    use protocol::CLOSE_UNAUTHENTICATED;

    use super::*;

    #[test]
    fn unauthenticated_before_hello_keeps_the_saved_game() {
        let ending = sort(false, Some(CLOSE_UNAUTHENTICATED));
        assert_eq!(ending, SocketEnding::Unauthenticated);
        assert!(ending.keeps_saved_game());
    }

    #[test]
    fn any_other_close_before_hello_is_a_stale_id() {
        // 1006 is what a browser reports for a socket that dropped without a
        // close frame, which is how the server ends an unknown game's socket.
        for code in [Some(1006), Some(1000), None] {
            let ending = sort(false, code);
            assert_eq!(ending, SocketEnding::StaleId, "{code:?}");
            assert!(!ending.keeps_saved_game());
        }
    }

    #[test]
    fn a_close_after_hello_is_a_disconnect() {
        for code in [Some(1006), Some(1000), None] {
            let ending = sort(true, code);
            assert_eq!(ending, SocketEnding::Disconnected, "{code:?}");
            assert!(ending.keeps_saved_game());
        }
    }
}
