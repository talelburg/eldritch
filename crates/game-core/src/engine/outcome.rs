//! Outcome of a single [`apply`](super::apply) call.

use std::borrow::Cow;

use serde::{Deserialize, Serialize};

/// The terminal status of an [`apply`](super::apply) call.
///
/// After the engine finishes applying an action, it is in one of three
/// states:
///
/// - [`Done`](EngineOutcome::Done) — the action resolved fully and the
///   engine is ready for the next action.
/// - [`AwaitingInput`](EngineOutcome::AwaitingInput) — the action
///   triggered a choice point and needs the active player to respond
///   before the engine can continue. The next action must be a
///   [`PlayerAction::ResolveInput`](crate::PlayerAction::ResolveInput).
/// - [`Rejected`](EngineOutcome::Rejected) — the action was illegal in
///   the current state (e.g. trying to investigate during the Mythos
///   phase) and was not applied. The state and event list returned
///   alongside this outcome are unchanged from the input.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum EngineOutcome {
    /// Action resolved fully; engine ready for next action.
    Done,
    /// Engine paused mid-resolution waiting for a player choice.
    AwaitingInput {
        /// Description of the prompt to show the player.
        request: InputRequest,
        /// Opaque token the engine uses to resume from this point when
        /// the response arrives.
        resume_token: ResumeToken,
    },
    /// Action was illegal; nothing changed.
    Rejected {
        /// Human-readable reason for rejection. `Cow` so static TODO
        /// strings cost no allocation while dynamic ones (formatted with
        /// runtime data) remain expressible.
        reason: Cow<'static, str>,
    },
}

/// Stable id for one offered option, scoped to a single
/// [`AwaitingInput`](EngineOutcome::AwaitingInput) prompt: the index into
/// the request's [`options`](InputRequest::options) (and the matching
/// `ChoiceFrame` offered set). A `u32` newtype for a
/// host-pointer-width-independent wire format; resume validates membership
/// rather than trusting it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OptionId(pub u32);

/// One selectable option in a structured choice prompt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChoiceOption {
    /// The id the host echoes back via
    /// [`InputResponse::PickSingle`](crate::action::InputResponse::PickSingle).
    pub id: OptionId,
    /// Human-readable label for the host to render.
    pub label: String,
}

/// A prompt the engine emits when it needs player input.
///
/// Carries free-form [`prompt`](Self::prompt) text plus, for the
/// single-selection choice contract (Axis A + the Axis-C reaction-window
/// migration), a structured [`options`](Self::options) list. Remaining
/// prompt-only callers (commit windows, hand-size discard) leave `options`
/// empty via [`InputRequest::prompt`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct InputRequest {
    /// Human-readable text describing what the player must choose.
    pub prompt: String,
    /// Structured options for a single-selection choice. Empty for the
    /// legacy free-form prompts that have not migrated to the structured
    /// contract.
    pub options: Vec<ChoiceOption>,
}

impl InputRequest {
    /// A legacy prompt-only request (no structured options).
    #[must_use]
    pub fn prompt(text: impl Into<String>) -> Self {
        Self {
            prompt: text.into(),
            options: Vec::new(),
        }
    }

    /// A structured single-selection choice request.
    #[must_use]
    pub fn choice(text: impl Into<String>, options: Vec<ChoiceOption>) -> Self {
        Self {
            prompt: text.into(),
            options,
        }
    }
}

/// Opaque continuation token returned alongside [`AwaitingInput`].
///
/// The engine uses this to identify which choice point a
/// [`ResolveInput`](crate::PlayerAction::ResolveInput) is answering.
/// The inner field is `pub(crate)` so external crates cannot fabricate
/// tokens; they receive them from the engine and pass them back.
///
/// [`AwaitingInput`]: EngineOutcome::AwaitingInput
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ResumeToken(pub(crate) u64);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choice_input_request_round_trips() {
        let req = InputRequest::choice(
            "Choose one",
            vec![
                ChoiceOption {
                    id: OptionId(0),
                    label: "Take 2 horror".into(),
                },
                ChoiceOption {
                    id: OptionId(1),
                    label: "Each discards 1".into(),
                },
            ],
        );
        let json = serde_json::to_string(&req).expect("serialize");
        let back: InputRequest = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, req);
        assert_eq!(back.options.len(), 2);
        assert_eq!(back.options[1].id, OptionId(1));
    }

    #[test]
    fn prompt_only_request_has_no_options() {
        let req = InputRequest::prompt("Submit PickIndex");
        assert!(req.options.is_empty());
    }
}
