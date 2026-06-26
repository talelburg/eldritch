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

/// Which [`InputResponse`](crate::action::InputResponse) variant the host must
/// echo back for a prompt. The variant names mirror `InputResponse` 1:1, so the
/// `kind` *is* the expected response — the host renders the matching control
/// without inspecting the prompt text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum InputKind {
    /// Pick exactly one offered [`option`](InputRequest::options) →
    /// [`InputResponse::PickSingle`](crate::action::InputResponse::PickSingle).
    PickSingle,
    /// Pick a subset (possibly empty) →
    /// [`InputResponse::PickMultiple`](crate::action::InputResponse::PickMultiple).
    PickMultiple,
    /// A binary acknowledge with no choice →
    /// [`InputResponse::Confirm`](crate::action::InputResponse::Confirm).
    Confirm,
}

/// A prompt the engine emits when it needs player input.
///
/// Carries free-form [`prompt`](Self::prompt) text, a [`kind`](Self::kind)
/// discriminator naming the [`InputResponse`](crate::action::InputResponse) the
/// host must send back, an optional structured [`options`](Self::options) list
/// (for [`PickSingle`](InputKind::PickSingle)), and a
/// [`skippable`](Self::skippable) flag for windows that may also be passed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct InputRequest {
    /// Human-readable text describing what the player must choose.
    pub prompt: String,
    /// Offered options for a [`PickSingle`](InputKind::PickSingle) prompt.
    /// Empty for [`PickMultiple`](InputKind::PickMultiple) (host derives
    /// hand-card candidates) and [`Confirm`](InputKind::Confirm).
    pub options: Vec<ChoiceOption>,
    /// Which response variant the host must send back.
    pub kind: InputKind,
    /// When true the host also offers a Skip/Pass control →
    /// [`InputResponse::Skip`](crate::action::InputResponse::Skip). Orthogonal
    /// to `kind` (e.g. a `PickSingle` reaction window that may also be passed).
    pub skippable: bool,
}

impl InputRequest {
    /// A single-selection choice over `options` →
    /// [`InputResponse::PickSingle`](crate::action::InputResponse::PickSingle).
    #[must_use]
    pub fn pick_single(text: impl Into<String>, options: Vec<ChoiceOption>) -> Self {
        Self {
            prompt: text.into(),
            options,
            kind: InputKind::PickSingle,
            skippable: false,
        }
    }

    /// A subset-selection prompt →
    /// [`InputResponse::PickMultiple`](crate::action::InputResponse::PickMultiple).
    ///
    /// `options` is left empty: every current consumer (skill-test commit,
    /// setup mulligan, hand-size discard) picks a subset of the *prompted
    /// investigator's hand*, and the host derives candidates from the hand,
    /// treating each `OptionId(i)` as hand index `i`. This hand-index
    /// convention only holds while `PickMultiple` decisions are hand-scoped; a
    /// future subset-pick over non-hand candidates (e.g. revealed cards,
    /// enemies) would need to carry them in `options` and render from there,
    /// like [`pick_single`](Self::pick_single).
    #[must_use]
    pub fn pick_multiple(text: impl Into<String>) -> Self {
        Self {
            prompt: text.into(),
            options: Vec::new(),
            kind: InputKind::PickMultiple,
            skippable: false,
        }
    }

    /// A binary acknowledge prompt →
    /// [`InputResponse::Confirm`](crate::action::InputResponse::Confirm).
    #[must_use]
    pub fn confirm(text: impl Into<String>) -> Self {
        Self {
            prompt: text.into(),
            options: Vec::new(),
            kind: InputKind::Confirm,
            skippable: false,
        }
    }

    /// Mark this prompt skippable (host renders a Skip/Pass control →
    /// [`InputResponse::Skip`](crate::action::InputResponse::Skip)).
    #[must_use]
    pub fn skippable(mut self) -> Self {
        self.skippable = true;
        self
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
    fn pick_single_sets_kind_and_not_skippable() {
        let req = InputRequest::pick_single(
            "Choose one",
            vec![ChoiceOption {
                id: OptionId(0),
                label: "A".into(),
            }],
        );
        assert_eq!(req.kind, InputKind::PickSingle);
        assert!(!req.skippable);
        assert_eq!(req.options.len(), 1);
    }

    #[test]
    fn pick_multiple_sets_kind_and_empty_options() {
        let req = InputRequest::pick_multiple("Commit cards");
        assert_eq!(req.kind, InputKind::PickMultiple);
        assert!(!req.skippable);
        assert!(req.options.is_empty());
    }

    #[test]
    fn confirm_sets_kind_and_empty_options() {
        let req = InputRequest::confirm("Draw");
        assert_eq!(req.kind, InputKind::Confirm);
        assert!(!req.skippable);
        assert!(req.options.is_empty());
    }

    #[test]
    fn skippable_flips_only_the_flag() {
        let base = InputRequest::pick_single("w", vec![]);
        let skip = InputRequest::pick_single("w", vec![]).skippable();
        assert!(!base.skippable);
        assert!(skip.skippable);
        assert_eq!(skip.kind, InputKind::PickSingle);
    }

    #[test]
    fn input_request_round_trips_with_kind_and_skippable() {
        let req = InputRequest::pick_single(
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
        )
        .skippable();
        let json = serde_json::to_string(&req).expect("serialize");
        let back: InputRequest = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, req);
        assert_eq!(back.kind, InputKind::PickSingle);
        assert!(back.skippable);
    }
}
