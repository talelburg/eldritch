//! Websocket wire protocol: the JSON messages exchanged between a
//! client and the server. The server is authoritative; clients submit
//! [`PlayerAction`]s and render the state the server broadcasts.

use game_core::action::RosterEntry;
use game_core::state::GameState;
use game_core::{EngineOutcome, Event, PlayerAction};
use serde::{Deserialize, Serialize};

/// A message sent from a client to the server.
///
/// Externally tagged (`{"submit": {...}}`) rather than internally
/// tagged: the `Hello` baseline carries a [`GameState`] whose
/// integer-keyed maps round-trip through `serde_json` only when the
/// value is deserialized directly, not buffered through an
/// internally-tagged `Content`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientMessage {
    /// Submit a player action for validation and application.
    Submit {
        /// The action to apply.
        action: PlayerAction,
    },
}

/// A message sent from the server to a client. Externally tagged for
/// the same reason as [`ClientMessage`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServerMessage {
    /// The full render baseline, sent on connect/reconnect. Carries the
    /// current state and the outcome of the most recent apply (including
    /// any pending [`AwaitingInput`](EngineOutcome::AwaitingInput)).
    Hello {
        /// Current derived game state. Boxed to keep the enum (and the
        /// per-game broadcast ring buffer) small — `GameState` dwarfs
        /// the other variants.
        state: Box<GameState>,
        /// Outcome of the most recent apply.
        outcome: EngineOutcome,
        /// Events emitted during scenario setup (`seat_and_open`), so
        /// the client's event log can show the opening draws, shuffles,
        /// and weakness set-aside. Empty for a session reloaded from the
        /// DB — setup already ran.
        events: Vec<Event>,
    },
    /// Broadcast to every connection of a game after an accepted action.
    Applied {
        /// The authoritative game state after the action resolved.
        /// Boxed for the same reason as [`Hello`](ServerMessage::Hello)'s
        /// `state`: `GameState` dwarfs the other variants. The client
        /// renders this snapshot directly (events are for log/animation,
        /// not state reconstruction).
        state: Box<GameState>,
        /// Events emitted by the action's resolution.
        events: Vec<Event>,
        /// Outcome of the apply.
        outcome: EngineOutcome,
    },
    /// Sent only to the submitting client when its action is refused
    /// (engine rejection, a malformed frame, or a persistence error).
    Rejected {
        /// Human-readable reason.
        reason: String,
    },
}

/// Stable identifier for a persisted game. Part of the client/server
/// contract: returned by `POST /games` and used in the WebSocket path
/// `/ws/{game_id}`. Lives here (not in `game-core`) because it is a
/// host/transport concept, not a kernel domain id like `ScenarioId`.
/// Transparent over `String` — serializes as a bare JSON string, binds to
/// a `SQLite` TEXT column, and extracts from a URL path segment. Id
/// *minting* (`random_game_id`) lives server-side to keep `uuid` out of
/// this wasm-safe crate.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct GameId(String);

impl GameId {
    /// Wrap an existing id string.
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
    /// Borrow the underlying string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for GameId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// Body of `POST /games`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateGameRequest {
    /// The scenario module to set up.
    pub scenario_id: String,
    /// The investigators to seat at creation, each paired with the deck the
    /// player chose. Seated into the persisted seed (#459); a rejected
    /// seating fails creation with no game row.
    pub roster: Vec<RosterEntry>,
}

/// Response to a successful `POST /games`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateGameResponse {
    /// The newly created game's id.
    pub game_id: GameId,
}

#[cfg(test)]
mod id_tests {
    use super::GameId;

    #[test]
    fn serializes_as_a_bare_string() {
        let id = GameId::new("abc");
        assert_eq!(serde_json::to_string(&id).unwrap(), "\"abc\"");
        let back: GameId = serde_json::from_str("\"abc\"").unwrap();
        assert_eq!(back, id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use game_core::state::GameStateBuilder;
    use game_core::test_support::fixtures::test_investigator;

    #[test]
    fn hello_round_trips_through_json() {
        use game_core::Event;

        let state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .build();
        let events = vec![Event::ScenarioStarted];
        let msg = ServerMessage::Hello {
            state: Box::new(state.clone()),
            outcome: EngineOutcome::Done,
            events: events.clone(),
        };

        let json = serde_json::to_string(&msg).expect("serialize");
        let back: ServerMessage = serde_json::from_str(&json).expect("deserialize");

        match back {
            ServerMessage::Hello {
                state: s,
                events: e,
                ..
            } => {
                assert_eq!(*s, state);
                assert_eq!(e, events, "events round-trip through JSON");
            }
            other => panic!("expected Hello, got {other:?}"),
        }
    }

    #[test]
    fn applied_round_trips_through_json() {
        let state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .build();
        let msg = ServerMessage::Applied {
            state: Box::new(state.clone()),
            events: Vec::new(),
            outcome: EngineOutcome::Done,
        };

        let json = serde_json::to_string(&msg).expect("serialize");
        let back: ServerMessage = serde_json::from_str(&json).expect("deserialize");

        match back {
            ServerMessage::Applied { state: s, .. } => assert_eq!(*s, state),
            other => panic!("expected Applied, got {other:?}"),
        }
    }

    #[test]
    fn create_game_request_round_trips_with_a_roster() {
        use game_core::action::RosterEntry;
        use game_core::state::CardCode;
        let req = CreateGameRequest {
            scenario_id: "the-gathering".into(),
            roster: vec![RosterEntry {
                investigator: CardCode::new("01001"),
                deck: vec![],
            }],
        };
        let json = serde_json::to_string(&req).expect("serialize");
        let back: CreateGameRequest = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.roster.len(), 1);
        assert_eq!(back.scenario_id, "the-gathering");
    }
}
