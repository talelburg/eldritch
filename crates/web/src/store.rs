//! Reactive client store: `ClientState` + the pure `ServerMessage` reducer.

use game_core::state::GameState;
use game_core::EngineOutcome;
use protocol::ServerMessage;

/// Connection lifecycle, set by the transport (not by `reduce`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ConnStatus {
    #[default]
    Connecting,
    Connected,
    Reconnecting,
    Failed,
}

/// Everything the UI renders. `game`/`outcome`/`last_rejection` come
/// from `reduce`; `status` is driven by the transport.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ClientState {
    pub game: Option<GameState>,
    pub outcome: Option<EngineOutcome>,
    pub status: ConnStatus,
    pub last_rejection: Option<String>,
}

/// Fold one server message into the client state. Data only — never
/// touches `status`. Mirrors the server: a `Rejected` leaves
/// `game`/`outcome` unchanged (the rejection was sender-only).
pub fn reduce(state: &mut ClientState, msg: ServerMessage) {
    match msg {
        ServerMessage::Hello { state: s, outcome } => {
            state.game = Some(*s);
            state.outcome = Some(outcome);
            state.last_rejection = None;
        }
        ServerMessage::Applied {
            state: s, outcome, ..
        } => {
            state.game = Some(*s);
            state.outcome = Some(outcome);
        }
        ServerMessage::Rejected { reason } => {
            state.last_rejection = Some(reason);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use game_core::test_support::builder::TestGame;
    use game_core::test_support::fixtures::test_investigator;

    fn sample_state() -> GameState {
        TestGame::new()
            .with_investigator(test_investigator(1))
            .build()
    }

    #[test]
    fn hello_sets_game_and_clears_rejection() {
        let mut s = ClientState {
            last_rejection: Some("stale".into()),
            ..Default::default()
        };
        reduce(
            &mut s,
            ServerMessage::Hello {
                state: Box::new(sample_state()),
                outcome: EngineOutcome::Done,
            },
        );
        assert!(s.game.is_some());
        assert_eq!(s.outcome, Some(EngineOutcome::Done));
        assert_eq!(s.last_rejection, None);
    }

    #[test]
    fn applied_updates_game_and_outcome() {
        let mut s = ClientState::default();
        reduce(
            &mut s,
            ServerMessage::Applied {
                state: Box::new(sample_state()),
                events: Vec::new(),
                outcome: EngineOutcome::Done,
            },
        );
        assert!(s.game.is_some());
        assert_eq!(s.outcome, Some(EngineOutcome::Done));
    }

    #[test]
    fn rejected_sets_reason_without_touching_game() {
        let mut s = ClientState {
            game: Some(sample_state()),
            outcome: Some(EngineOutcome::Done),
            ..Default::default()
        };
        let before = s.game.clone();
        reduce(
            &mut s,
            ServerMessage::Rejected {
                reason: "not your turn".into(),
            },
        );
        assert_eq!(s.last_rejection.as_deref(), Some("not your turn"));
        assert_eq!(s.game, before);
        assert_eq!(s.outcome, Some(EngineOutcome::Done));
    }
}
