//! Reactive client store: `ClientState` + the pure `ServerMessage` reducer.

use game_core::state::{ChaosToken, GameState, TokenResolution};
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
    /// Difficulty of the most recently *started* skill test, captured from
    /// `Event::SkillTestStarted` (which arrives in an earlier batch than the
    /// resolution). The result panel pairs it with the latched
    /// `SkillTestSucceeded`/`Failed` margin to show total-vs-difficulty.
    /// Cleared by `Hello`.
    pub last_skill_test_difficulty: Option<i8>,
    /// The chaos token drawn by the most recent reveal, captured from
    /// `Event::ChaosTokenRevealed` with how it resolved. Latched for the same
    /// reason as the difficulty: a symbol token whose ST.4 effect suspends for
    /// input puts the reveal in an earlier batch than the outcome (#787).
    /// Cleared by a new test's `SkillTestStarted` and by `Hello`.
    pub last_revealed_token: Option<(ChaosToken, TokenResolution)>,
    /// The most recent skill test's resolution — the `SkillTestSucceeded` or
    /// `SkillTestFailed` event itself — held from the batch that determined it
    /// until the batch that tears the test down. Latched for the same reason as
    /// the token, and for a sharper case (#853): a reaction in the `when` cell of
    /// `SkillTestResolved` (Lita Chantler 01117, Dr. Milan 01033) suspends in the
    /// *same* batch as the outcome, so by the time the acknowledge pause arrives
    /// the live batch is empty and the result panel had nothing to render.
    /// Cleared by `SkillTestEnded`, by a new test's `SkillTestStarted`, and by
    /// `Hello`.
    pub last_skill_test_result: Option<game_core::Event>,
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
        ServerMessage::Hello {
            state: s,
            outcome,
            events,
        } => {
            state.game = Some(*s);
            state.outcome = Some(outcome);
            state.last_rejection = None;
            state.last_skill_test_difficulty = None;
            state.last_revealed_token = None;
            state.last_skill_test_result = None;
            state.log = Vec::new();
            state.pending_label = None;
            if !events.is_empty() {
                state.log.push(LogBatch {
                    header: "Setup".to_string(),
                    events,
                });
            }
        }
        ServerMessage::Applied {
            state: s,
            events,
            outcome,
        } => {
            state.game = Some(*s);
            state.outcome = Some(outcome);
            // Walk the batch in order, latching what the result panel renders
            // from. Order matters within a batch as much as across batches:
            // `SkillTestStarted` clears the *previous* test's token and result,
            // and a batch may legitimately carry the end of one test and the
            // start of the next.
            //
            // Why latch at all: a resolution can span several apply batches, so
            // the batch holding the outcome need not be the one holding the
            // reveal, nor the one the acknowledge pause arrives on. Two known
            // splitters — an ST.4 effect that suspends for input, The
            // Gathering's Tablet dealing damage with a soak target in play
            // (#787), and a reaction in the `when` cell of `SkillTestResolved`,
            // which fires in the same apply as the outcome and pushes the
            // acknowledge into a batch of its own (#853).
            for event in &events {
                match event {
                    // The difficulty announced at ST.1. Exact in current scope:
                    // since #677 the difficulty is a live query rather than a
                    // stored number, so this is the difficulty *at ST.1* and the
                    // margin is computed against the ST.6 re-read — but no
                    // in-corpus card changes an investigated location's shroud or
                    // an attacked enemy's fight or evade mid-test, so the two
                    // agree. The alternative is reading the modified difficulty
                    // off the still-live in-flight frame; that would be immune to
                    // (a) a reconnect mid-pause (`Hello` clears this cache) and
                    // (b) the first card that does move it mid-test. Revisit when
                    // either lands.
                    game_core::Event::SkillTestStarted { difficulty, .. } => {
                        state.last_skill_test_difficulty = Some(*difficulty);
                        // A new test invalidates the previous test's token and
                        // result. The difficulty needs no such clear — every test
                        // re-announces its own — but a test whose determination is
                        // already known draws no token at all (ST.3 and ST.4 do
                        // not happen), so without this the panel would name the
                        // *previous* test's token instead of the em dash (#787).
                        state.last_revealed_token = None;
                        state.last_skill_test_result = None;
                    }
                    game_core::Event::ChaosTokenRevealed { token, resolution } => {
                        state.last_revealed_token = Some((*token, *resolution));
                    }
                    game_core::Event::SkillTestSucceeded { .. }
                    | game_core::Event::SkillTestFailed { .. } => {
                        state.last_skill_test_result = Some(event.clone());
                    }
                    // ST.8 teardown: the test is over, so its result stops being
                    // the one the panel would render. Without this the modal's
                    // other guard — an un-anchored `Confirm` (ADR 0011) — would be
                    // all that stands between a stale result and the next
                    // acknowledge-shaped pause.
                    game_core::Event::SkillTestEnded { .. } => {
                        state.last_skill_test_result = None;
                    }
                    _ => {}
                }
            }
            let header = state
                .pending_label
                .take()
                .unwrap_or_else(|| "(action)".into());
            state.log.push(LogBatch { header, events });
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
                events: Vec::new(),
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
    fn applied_logs_the_batch_and_captures_difficulty() {
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
        assert_eq!(s.log.len(), 1, "the batch is logged");
    }

    /// #787: the reveal and the outcome can land in different batches when an
    /// ST.4 effect suspends for input. The latch is what carries the token over.
    #[test]
    fn applied_latches_the_revealed_token_across_batches() {
        use game_core::state::{InvestigatorId, SkillKind};
        use game_core::{Event, FailureReason};

        let mut s = ClientState::default();
        reduce(
            &mut s,
            ServerMessage::Applied {
                state: Box::new(sample_state()),
                events: vec![Event::ChaosTokenRevealed {
                    token: ChaosToken::Tablet,
                    resolution: TokenResolution::Modifier(-2),
                }],
                outcome: EngineOutcome::Done,
            },
        );
        assert_eq!(
            s.last_revealed_token,
            Some((ChaosToken::Tablet, TokenResolution::Modifier(-2)))
        );
        // The next batch carries the outcome and no reveal; the latch survives.
        reduce(
            &mut s,
            ServerMessage::Applied {
                state: Box::new(sample_state()),
                events: vec![Event::SkillTestFailed {
                    investigator: InvestigatorId(1),
                    skill: SkillKind::Combat,
                    reason: FailureReason::Total,
                    by: 1,
                }],
                outcome: EngineOutcome::Done,
            },
        );
        assert_eq!(
            s.last_revealed_token,
            Some((ChaosToken::Tablet, TokenResolution::Modifier(-2))),
            "a batch without a reveal leaves the latch alone"
        );
    }

    /// A test whose determination is already known draws no token at all, so the
    /// previous test's token must not survive into it (#787).
    #[test]
    fn a_new_test_clears_the_previous_tests_token() {
        use game_core::state::{InvestigatorId, SkillKind};
        use game_core::Event;

        let mut s = ClientState {
            last_revealed_token: Some((ChaosToken::Tablet, TokenResolution::Modifier(-2))),
            ..Default::default()
        };
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
        assert_eq!(
            s.last_revealed_token, None,
            "a started test clears the token the previous one drew"
        );
    }

    /// A batch carrying the start *and* the reveal — the common, non-suspending
    /// case — still ends with the token latched.
    #[test]
    fn a_start_and_reveal_in_one_batch_latches_the_token() {
        use game_core::state::{InvestigatorId, SkillKind};
        use game_core::Event;

        let mut s = ClientState::default();
        reduce(
            &mut s,
            ServerMessage::Applied {
                state: Box::new(sample_state()),
                events: vec![
                    Event::SkillTestStarted {
                        investigator: InvestigatorId(1),
                        skill: SkillKind::Willpower,
                        difficulty: 3,
                    },
                    Event::ChaosTokenRevealed {
                        token: ChaosToken::Numeric(1),
                        resolution: TokenResolution::Modifier(1),
                    },
                ],
                outcome: EngineOutcome::Done,
            },
        );
        assert_eq!(
            s.last_revealed_token,
            Some((ChaosToken::Numeric(1), TokenResolution::Modifier(1)))
        );
    }

    #[test]
    fn hello_clears_the_retained_latches() {
        let mut s = ClientState {
            last_skill_test_difficulty: Some(3),
            last_revealed_token: Some((ChaosToken::Tablet, TokenResolution::Modifier(-2))),
            ..Default::default()
        };
        reduce(
            &mut s,
            ServerMessage::Hello {
                state: Box::new(sample_state()),
                outcome: EngineOutcome::Done,
                events: Vec::new(),
            },
        );
        assert_eq!(
            s.last_skill_test_difficulty, None,
            "Hello clears the retained difficulty"
        );
        assert_eq!(
            s.last_revealed_token, None,
            "Hello clears the retained token, so a reconnect mid-pause shows no stale draw"
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
                events: Vec::new(),
            },
        );
        assert!(s.log.is_empty());
        assert_eq!(s.pending_label, None);
    }

    #[test]
    fn hello_with_events_pushes_setup_batch() {
        let mut s = ClientState::default();
        reduce(
            &mut s,
            ServerMessage::Hello {
                state: Box::new(sample_state()),
                outcome: EngineOutcome::Done,
                events: vec![game_core::Event::ScenarioStarted],
            },
        );
        assert_eq!(s.log.len(), 1, "one Setup batch expected");
        assert_eq!(s.log[0].header, "Setup");
        assert_eq!(s.log[0].events, vec![game_core::Event::ScenarioStarted]);
    }

    #[test]
    fn hello_with_empty_events_leaves_log_empty() {
        let mut s = ClientState::default();
        reduce(
            &mut s,
            ServerMessage::Hello {
                state: Box::new(sample_state()),
                outcome: EngineOutcome::Done,
                events: Vec::new(),
            },
        );
        assert!(s.log.is_empty(), "no setup batch when events is empty");
    }

    #[test]
    fn hello_with_events_clears_prior_batches_first() {
        let mut s = ClientState::default();
        // Seed a prior action batch.
        s.log.push(LogBatch {
            header: "prior action".into(),
            events: vec![game_core::Event::ScenarioStarted],
        });
        // Hello with setup events: prior batch gone, Setup batch present.
        reduce(
            &mut s,
            ServerMessage::Hello {
                state: Box::new(sample_state()),
                outcome: EngineOutcome::Done,
                events: vec![game_core::Event::ScenarioStarted],
            },
        );
        assert_eq!(s.log.len(), 1, "only the Setup batch should remain");
        assert_eq!(s.log[0].header, "Setup");
    }
}
