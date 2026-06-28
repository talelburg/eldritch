//! Reactive client store: `ClientState` + the pure `ServerMessage` reducer.

use game_core::state::GameState;
use game_core::EngineOutcome;
use leptos::prelude::*;
use protocol::ServerMessage;

/// Connection lifecycle, set by the transport (not by `reduce`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ConnStatus {
    #[default]
    Connecting,
    Connected,
    Reconnecting,
    Failed,
    /// No saved game and no roster chosen yet — render the picker.
    AwaitingRoster,
    /// A server frame failed to deserialize — the client and server binaries
    /// disagree on the wire format. Terminal: restart the server and reload.
    VersionMismatch,
}

/// One applied submit's worth of events, for the event-log view (#505).
#[derive(Debug, Clone, PartialEq)]
pub struct LogBatch {
    /// Human label of the menu choice that produced this batch
    /// (e.g. "Play 01059 from hand"); a generic fallback when unknown.
    pub header: String,
    /// The events emitted by that submit, in order.
    pub events: Vec<game_core::Event>,
}

/// Everything the UI renders. `game`/`outcome`/`last_rejection` come
/// from `reduce`; `status` is driven by the transport.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ClientState {
    pub game: Option<GameState>,
    pub outcome: Option<EngineOutcome>,
    pub status: ConnStatus,
    pub last_rejection: Option<String>,
    /// The most recent `Applied` batch's events, retained for views that render
    /// from event history (the #478 skill-test result panel). Cleared by `Hello`.
    pub last_events: Vec<game_core::Event>,
    /// Difficulty of the most recently *started* skill test, captured from
    /// `Event::SkillTestStarted` (which arrives in an earlier batch than the
    /// resolution). The result panel pairs it with the resolution batch's
    /// `SkillTestSucceeded`/`Failed` margin to show total-vs-difficulty.
    /// Cleared by `Hello`.
    pub last_skill_test_difficulty: Option<i8>,
    /// Full accumulated event history, grouped per applied submit, oldest
    /// first. Cleared by `Hello`. The event-log panel (#505) renders this.
    pub log: Vec<LogBatch>,
    /// Header label for the *next* `Applied` batch, set by the input view at
    /// submit time and taken when that batch arrives. Cleared on `Rejected`
    /// (the submit produced no batch) and `Hello`.
    pub pending_label: Option<String>,
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
            state.last_events = Vec::new();
            state.last_skill_test_difficulty = None;
            state.log = Vec::new();
            state.pending_label = None;
        }
        ServerMessage::Applied {
            state: s,
            events,
            outcome,
        } => {
            state.game = Some(*s);
            state.outcome = Some(outcome);
            // Capture difficulty from `SkillTestStarted` (an earlier batch than the
            // resolution). Exact in current scope — `InFlightSkillTest.difficulty`
            // is never mutated post-creation, so it equals the margin basis. The
            // alternative is reading `game.current_skill_test().difficulty` off the
            // still-live in-flight frame; that would be immune to (a) a reconnect
            // mid-pause (`Hello` clears this cache) and (b) a future difficulty-
            // modifying card that mutates the in-flight difficulty mid-test.
            // Revisit if either lands.
            if let Some(difficulty) = events.iter().find_map(|e| match e {
                game_core::Event::SkillTestStarted { difficulty, .. } => Some(*difficulty),
                _ => None,
            }) {
                state.last_skill_test_difficulty = Some(difficulty);
            }
            let header = state
                .pending_label
                .take()
                .unwrap_or_else(|| "(action)".into());
            state.log.push(LogBatch {
                header,
                events: events.clone(),
            });
            state.last_events = events;
        }
        ServerMessage::Rejected { reason } => {
            state.last_rejection = Some(reason);
            state.pending_label = None;
        }
    }
}

/// The single reactive store handed through Leptos context.
pub type StoreSignal = RwSignal<ClientState>;

/// Provide a fresh store signal into context and return it.
pub fn provide_store() -> StoreSignal {
    let signal = RwSignal::new(ClientState::default());
    provide_context(signal);
    signal
}

/// Read the store signal from context.
///
/// # Panics
///
/// Panics if no store signal is in context — a programmer error (every view
/// lives under [`provide_store`]).
pub fn use_store() -> StoreSignal {
    use_context::<StoreSignal>().expect("store signal provided at App root")
}

#[cfg(test)]
mod tests {
    use super::*;
    use game_core::state::GameStateBuilder;
    use game_core::test_support::fixtures::test_investigator;

    fn sample_state() -> GameState {
        GameStateBuilder::new()
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
        // Seed a pending rejection to prove Applied leaves it untouched
        // (only Hello clears it).
        let mut s = ClientState {
            last_rejection: Some("stale".into()),
            ..Default::default()
        };
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
        assert_eq!(s.last_rejection.as_deref(), Some("stale"));
    }

    #[test]
    fn applied_retains_events_and_captures_difficulty() {
        use game_core::state::{InvestigatorId, SkillKind};
        use game_core::Event;

        let mut s = ClientState::default();
        reduce(
            &mut s,
            ServerMessage::Applied {
                state: Box::new(sample_state()),
                events: vec![Event::SkillTestStarted {
                    investigator: InvestigatorId(1),
                    skill: SkillKind::Willpower,
                    difficulty: 3,
                }],
                outcome: EngineOutcome::Done,
            },
        );
        assert_eq!(s.last_skill_test_difficulty, Some(3));
        assert_eq!(s.last_events.len(), 1);
    }

    #[test]
    fn hello_clears_retained_events_and_difficulty() {
        let mut s = ClientState {
            last_skill_test_difficulty: Some(3),
            ..Default::default()
        };
        // seed a non-empty last_events too
        s.last_events.push(game_core::Event::ScenarioStarted);
        reduce(
            &mut s,
            ServerMessage::Hello {
                state: Box::new(sample_state()),
                outcome: EngineOutcome::Done,
            },
        );
        assert!(
            s.last_events.is_empty(),
            "Hello clears the retained event batch"
        );
        assert_eq!(
            s.last_skill_test_difficulty, None,
            "Hello clears the retained difficulty"
        );
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

    #[test]
    fn applied_pushes_a_log_batch_using_pending_label() {
        let mut s = ClientState {
            pending_label: Some("Move to Cellar".into()),
            ..Default::default()
        };
        reduce(
            &mut s,
            ServerMessage::Applied {
                state: Box::new(sample_state()),
                events: vec![game_core::Event::ScenarioStarted],
                outcome: EngineOutcome::Done,
            },
        );
        assert_eq!(s.log.len(), 1);
        assert_eq!(s.log[0].header, "Move to Cellar");
        assert_eq!(s.log[0].events, vec![game_core::Event::ScenarioStarted]);
        assert_eq!(s.pending_label, None, "pending_label is consumed");
    }

    #[test]
    fn applied_without_pending_label_uses_generic_header() {
        let mut s = ClientState::default();
        reduce(
            &mut s,
            ServerMessage::Applied {
                state: Box::new(sample_state()),
                events: Vec::new(),
                outcome: EngineOutcome::Done,
            },
        );
        assert_eq!(s.log.len(), 1);
        assert_eq!(s.log[0].header, "(action)");
    }

    #[test]
    fn consecutive_applied_accumulate_in_order() {
        let mut s = ClientState::default();
        for label in ["first", "second"] {
            s.pending_label = Some(label.to_string());
            reduce(
                &mut s,
                ServerMessage::Applied {
                    state: Box::new(sample_state()),
                    events: Vec::new(),
                    outcome: EngineOutcome::Done,
                },
            );
        }
        let headers: Vec<&str> = s.log.iter().map(|b| b.header.as_str()).collect();
        assert_eq!(headers, vec!["first", "second"]);
    }

    #[test]
    fn rejected_clears_pending_label_without_pushing_a_batch() {
        let mut s = ClientState {
            pending_label: Some("Move to Cellar".into()),
            ..Default::default()
        };
        reduce(
            &mut s,
            ServerMessage::Rejected {
                reason: "nope".into(),
            },
        );
        assert!(s.log.is_empty(), "rejection pushes no batch");
        assert_eq!(s.pending_label, None, "rejection clears the stale label");
    }

    #[test]
    fn hello_clears_log_and_pending_label() {
        let mut s = ClientState {
            pending_label: Some("stale".into()),
            ..Default::default()
        };
        s.log.push(LogBatch {
            header: "old".into(),
            events: Vec::new(),
        });
        reduce(
            &mut s,
            ServerMessage::Hello {
                state: Box::new(sample_state()),
                outcome: EngineOutcome::Done,
            },
        );
        assert!(s.log.is_empty());
        assert_eq!(s.pending_label, None);
    }
}
