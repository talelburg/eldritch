//! Outcome of a single [`apply`](super::apply) call.

use std::borrow::Cow;

use serde::{Deserialize, Serialize};

use crate::state::{CardInstanceId, EnemyId, InvestigatorId, LocationId};

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

/// The board surface an offered [`ChoiceOption`] acts on, letting a host render
/// the option on the entity it targets rather than in a flat list. `Global`
/// means no board anchor (e.g. End turn, a Confirm). Anchors are derived from
/// the engine's own action / candidate targets, so a host never re-computes
/// legality (#535, #206).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum OptionTarget {
    /// No board anchor — a global / contextual control.
    Global,
    /// A location on the map.
    Location(LocationId),
    /// An enemy.
    Enemy(EnemyId),
    /// A card in an investigator's hand, by zero-based hand index.
    HandCard {
        /// The hand's owner.
        investigator: InvestigatorId,
        /// Zero-based position in that investigator's hand.
        hand_index: u8,
    },
    /// An in-play / threat-area / investigator card instance.
    CardInstance(CardInstanceId),
    /// The current act.
    Act,
}

/// One selectable option in a structured choice prompt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChoiceOption {
    /// The id the host echoes back via
    /// [`InputResponse::PickSingle`](crate::action::InputResponse::PickSingle).
    pub id: OptionId,
    /// Human-readable label for the host to render (full and unambiguous,
    /// e.g. `"Fight Ghoul"`; a host may shorten it for display).
    pub label: String,
    /// The board surface this option acts on (`Global` if none).
    pub target: OptionTarget,
}

impl ChoiceOption {
    /// An option anchored to `target`.
    #[must_use]
    pub fn new(id: OptionId, label: impl Into<String>, target: OptionTarget) -> Self {
        Self {
            id,
            label: label.into(),
            target,
        }
    }

    /// An option with no board anchor ([`OptionTarget::Global`]).
    #[must_use]
    pub fn global(id: OptionId, label: impl Into<String>) -> Self {
        Self::new(id, label, OptionTarget::Global)
    }
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
        let req =
            InputRequest::pick_single("Choose one", vec![ChoiceOption::global(OptionId(0), "A")]);
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
                ChoiceOption::global(OptionId(0), "Take 2 horror"),
                ChoiceOption::global(OptionId(1), "Each discards 1"),
            ],
        )
        .skippable();
        let json = serde_json::to_string(&req).expect("serialize");
        let back: InputRequest = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, req);
        assert_eq!(back.kind, InputKind::PickSingle);
        assert!(back.skippable);
    }

    #[test]
    fn global_constructor_sets_global_target() {
        let opt = ChoiceOption::global(OptionId(3), "End turn");
        assert_eq!(opt.id, OptionId(3));
        assert_eq!(opt.label, "End turn");
        assert_eq!(opt.target, OptionTarget::Global);
    }

    #[test]
    fn awaiting_input_round_trips_option_target() {
        use crate::state::EnemyId;
        let outcome = EngineOutcome::AwaitingInput {
            request: InputRequest::pick_single(
                "Choose an action",
                vec![
                    ChoiceOption::global(OptionId(0), "End turn"),
                    ChoiceOption::new(OptionId(1), "Fight Ghoul", OptionTarget::Enemy(EnemyId(7))),
                ],
            ),
            resume_token: ResumeToken(0),
        };
        let json = serde_json::to_string(&outcome).expect("serialize");
        let back: EngineOutcome = serde_json::from_str(&json).expect("deserialize");
        let EngineOutcome::AwaitingInput { request, .. } = back else {
            panic!("expected AwaitingInput, got {back:?}");
        };
        assert_eq!(request.options[0].target, OptionTarget::Global);
        assert_eq!(request.options[1].target, OptionTarget::Enemy(EnemyId(7)));
    }
}
