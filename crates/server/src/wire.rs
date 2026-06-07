//! Websocket wire protocol: the JSON messages exchanged between a
//! client and the server. The server is authoritative; clients submit
//! [`PlayerAction`]s and render the event stream the server broadcasts.

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
    },
    /// Broadcast to every connection of a game after an accepted action.
    Applied {
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
