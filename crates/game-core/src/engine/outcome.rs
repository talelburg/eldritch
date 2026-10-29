//! Outcome of a single [`apply`](crate::engine::apply) call.

use std::borrow::Cow;

use serde::{Deserialize, Serialize};

use crate::engine::ability_source;
use crate::state::{
    AbilitySource, CardCode, CardInstanceId, EnemyId, GameState, InvestigatorId, LocationId,
};

/// The terminal status of an [`apply`](crate::engine::apply) call.
///
/// After the engine finishes applying an action, it is in one of three
/// states:
///
/// - [`Done`](EngineOutcome::Done) — the game is over. The continuation
///   stack is empty, so nothing is left for a next action to answer. A turn
///   menu is a prompt, so an action that ends back at one returns
///   `AwaitingInput`, not `Done`.
/// - [`AwaitingInput`](EngineOutcome::AwaitingInput) — the action
///   triggered a choice point and needs the active player to respond
///   before the engine can continue. The next action must be a
///   [`PlayerAction::ResolveInput`](crate::action::PlayerAction::ResolveInput).
/// - [`Rejected`](EngineOutcome::Rejected) — the action was illegal in
///   the current state (e.g. trying to investigate during the Mythos
///   phase) and was not applied. The state and event list returned
///   alongside this outcome are unchanged from the input.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum EngineOutcome {
    /// From [`apply`](crate::engine::apply), the game is over and the
    /// continuation stack is empty. A dispatch step inside the engine returns
    /// it to mean only that the step did not suspend; what that means after an
    /// emit is on `emit::queue_event`.
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
/// [`AwaitingInput`](EngineOutcome::AwaitingInput) prompt.
///
/// It is the index into the request's [`options`](InputRequest::options) (and
/// the matching `ChoiceFrame` offered set). A `u32` newtype for a
/// host-pointer-width-independent wire format; resume validates membership
/// rather than trusting it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OptionId(pub u32);

/// The board surface a prompt or an offered [`ChoiceOption`] acts on, letting a
/// host render it on the entity it targets rather than in a flat list.
///
/// Anchors are derived from the engine's own action / candidate targets, so a
/// host never re-computes legality. **Un-anchored is spelled `None` at the
/// [`Option`] wrapping this enum**, not a variant here — see ADR 0011; a host
/// that reads a `None` anchor falls back to the prompt banner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum OptionTarget {
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
    /// A card in an investigator's hand, matched by **code** (every copy) — for a
    /// queued Fast reaction event, which is code-identified (either copy plays), so
    /// all matching hand cards are actionable.
    HandCardByCode {
        /// The hand's owner.
        investigator: InvestigatorId,
        /// The card code; all hand cards of this code are actionable.
        code: CardCode,
    },
    /// An in-play / threat-area / investigator card instance.
    CardInstance(CardInstanceId),
    /// The current act.
    Act,
    /// The current agenda.
    Agenda,
    /// An investigator's turn control (End turn) — the panel affordance that is
    /// always present and disabled when nothing anchors to it.
    TurnControl(InvestigatorId),
    /// An investigator's resource pool (Gain resource).
    ResourcePool(InvestigatorId),
    /// An investigator's own player deck (Draw).
    PlayerDeck(InvestigatorId),
    /// The scenario's encounter deck (the Mythos step-1.4 draw).
    EncounterDeck,
}

impl From<AbilitySource> for OptionTarget {
    /// The board surface an ability source is rendered on — **the one map from
    /// a source to an anchor**.
    ///
    /// Both paths that offer an ability go through it: the turn menu
    /// (`TurnAction::target`) and the forced / reaction resolution options
    /// (`reaction_windows::candidate_anchor`), so neither keeps a copy that
    /// could drift from the other.
    ///
    /// The `match` is exhaustive on purpose: a sixth
    /// [`AbilitySource`](crate::state::AbilitySource) kind should stop this
    /// compiling rather than quietly anchor itself somewhere wrong.
    ///
    /// **This map says nothing about whether the source is still on the board**,
    /// and a caller may only use it when it holds that guarantee itself. Two do:
    /// the turn menu enumerates from `ability_source::reachable_sources`, and the
    /// forced / reaction candidates are re-probed against
    /// `ability_source::source_card` by the lapse sweep at every prompt
    /// site. A caller holding an anchor that was **snapshotted before arbitrary
    /// mutation** has no such guarantee and must use the crate-internal
    /// `OptionTarget::for_live_source` instead.
    fn from(source: AbilitySource) -> Self {
        match source {
            AbilitySource::InPlay(instance_id) => OptionTarget::CardInstance(instance_id),
            AbilitySource::Location(location) => OptionTarget::Location(location),
            AbilitySource::Enemy(enemy) => OptionTarget::Enemy(enemy),
            AbilitySource::Act => OptionTarget::Act,
            AbilitySource::Agenda => OptionTarget::Agenda,
        }
    }
}

impl OptionTarget {
    /// The board surface `source` renders on, or `None` when `source` has left
    /// the board — the **liveness-checked** form of the `From<AbilitySource>`
    /// map above.
    ///
    /// ADR 0011 makes an unrenderable anchor a deadlock rather than a cosmetic
    /// slip: *"an option the engine anchors to a surface the client does not
    /// render is unreachable rather than merely misplaced"*, and the obligation
    /// it puts on the engine is to *"either anchor it to a surface that exists,
    /// or leave it un-anchored and accept the banner"*. `None` is that second
    /// branch, and the prompt banner is what renders it.
    ///
    /// The gate is `ability_source::source_card`, the same board-wide
    /// existence probe the reaction path's `SourceGone`
    /// lapse uses — *existence, not reachability*, which is the right question
    /// here: an ability resolves to completion even from a source its controller
    /// could no longer legally activate (RR Appendix I, *"the sequence does not
    /// stop from completing if that card leaves play during the sequence"*), so
    /// the prompt is still owed a home.
    pub(crate) fn for_live_source(source: AbilitySource, state: &GameState) -> Option<Self> {
        ability_source::source_card(state, source)?;
        Some(source.into())
    }
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
    /// The board surface this option acts on; `None` is un-anchored — the host
    /// renders it in the prompt banner (ADR 0011).
    pub target: Option<OptionTarget>,
    /// The investigator this option is offered to, in a prompt
    /// [answered by several](Responder::Several): a reaction window or a
    /// framework Fast window. Two options on one anchor offered to two
    /// investigators differ here. `None` everywhere else, where the
    /// request's [`responder`](InputRequest::responder) answers every option.
    ///
    /// Required on the wire, `null` for `None`.
    #[serde(deserialize_with = "crate::wire::required_option")]
    pub investigator: Option<InvestigatorId>,
}

impl ChoiceOption {
    /// An **un-anchored** option. Anchor it with [`at`](Self::at).
    #[must_use]
    pub fn new(id: OptionId, label: impl Into<String>) -> Self {
        Self {
            id,
            label: label.into(),
            target: None,
            investigator: None,
        }
    }

    /// Name the investigator this option is offered to, in a window several
    /// answer.
    #[must_use]
    pub fn offered_to(mut self, investigator: InvestigatorId) -> Self {
        self.investigator = Some(investigator);
        self
    }

    /// Anchor this option to `target` — the same `.at(…)` idiom
    /// [`InputRequest::at`] uses, so a prompt and its options read alike.
    #[must_use]
    pub fn at(mut self, target: OptionTarget) -> Self {
        self.target = Some(target);
        self
    }

    /// Anchor this option to `target` when there is one. For the callers that
    /// build options from an anchor that is *already* optional — every site that
    /// maps a [`TurnAction::target`](crate::engine::enumerate::TurnAction::target) or an
    /// evaluator-supplied anchor — so none of them re-writes the same
    /// `match … { Some(t) => opt.at(t), None => opt }` by hand.
    #[must_use]
    pub fn maybe_at(self, target: Option<OptionTarget>) -> Self {
        match target {
            Some(target) => self.at(target),
            None => self,
        }
    }
}

/// Which [`InputResponse`](crate::action::InputResponse) variant the host must
/// echo back for a prompt.
///
/// The variant names mirror `InputResponse` 1:1, so the `kind` *is* the
/// expected response — the host renders the matching control without inspecting
/// the prompt text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum InputKind {
    /// Pick exactly one offered [`option`](InputRequest::options) →
    /// [`InputResponse::PickSingle`](crate::action::InputResponse::PickSingle).
    PickSingle,
    /// Pick a subset of the offered options, as many as the
    /// [`PickCount`] says →
    /// [`InputResponse::PickMultiple`](crate::action::InputResponse::PickMultiple).
    PickMultiple(PickCount),
    /// A binary acknowledge with no choice →
    /// [`InputResponse::Confirm`](crate::action::InputResponse::Confirm).
    Confirm,
}

/// What a prompt is asking the player to choose *between* — never how it looks.
///
/// ADR 0011's claim one layer in: the engine names the nature, the host maps
/// nature to presentation. A `Presentation::Modal` on the wire would put UI
/// vocabulary in the kernel, and a host re-deriving the nature from the prompt's
/// shape ("all options anchored to the same card") is the same re-derivation
/// ADR 0011 rejects for targets read off `label` strings.
///
/// [`Selection`](Self::Selection) is the default every constructor reaches, so a
/// builder that never considered its nature produces the status quo. The type
/// will not catch an omission — tests do (ADR 0015).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub enum PromptNature {
    /// Options are board entities; the anchor disambiguates which one.
    #[default]
    Selection,
    /// Options are alternatives printed on one card; the anchor is provenance.
    Decision,
}

/// How many of a [`PickMultiple`](InputKind::PickMultiple) prompt's options a
/// response may select: `min..=max`, both inclusive.
///
/// A skill-test commit and a setup mulligan take any number of their options,
/// none included ([`up_to`](Self::up_to) the option count). A hand-size
/// discard takes exactly the number it must discard
/// ([`exactly`](Self::exactly)). A host can hold its Confirm until the
/// selection is in range. A constraint no count expresses — a skill's printed
/// "Max N committed per skill test" (Guts 01089) — is still checked when the
/// response arrives, and a response breaking it is rejected.
///
/// `u32` for the reason [`OptionId`] is: a host-pointer-width-independent wire
/// format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PickCount {
    /// The fewest options a response may select.
    pub min: u32,
    /// The most options a response may select.
    pub max: u32,
}

impl PickCount {
    /// Any number from none to `max`.
    #[must_use]
    pub fn up_to(max: usize) -> Self {
        Self {
            min: 0,
            max: Self::wire(max),
        }
    }

    /// Exactly `count`.
    #[must_use]
    pub fn exactly(count: usize) -> Self {
        let count = Self::wire(count);
        Self {
            min: count,
            max: count,
        }
    }

    /// Whether selecting `selected` options is in range.
    #[must_use]
    pub fn admits(self, selected: usize) -> bool {
        u32::try_from(selected).is_ok_and(|n| (self.min..=self.max).contains(&n))
    }

    fn wire(count: usize) -> u32 {
        u32::try_from(count).expect("an offered option count fits in u32")
    }
}

/// Who answers a prompt (ADR 0019), read off the frame that asks it.
///
/// A host shows who the game is waiting on from this, without matching over
/// engine frames. TODO(#581): a seat check compares the answering seat against
/// it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Responder {
    /// One named investigator: the one whose turn, test, cost, queue slot or
    /// effect the prompt belongs to.
    Investigator(InvestigatorId),
    /// The lead investigator, resolved to who that is now, answering a group
    /// decision: `glossary/Lead_Investigator.md`, *"the lead investigator is
    /// the final arbiter in choosing among those options"*. Kept apart from
    /// [`Investigator`](Self::Investigator) so a host can show that the lead
    /// is the one deciding.
    Lead(InvestigatorId),
    /// Several answerers: a reaction window or a framework Fast window, where
    /// each option names the investigator it is offered to
    /// ([`ChoiceOption::investigator`]).
    Several,
}

/// A prompt the engine emits when it needs player input.
///
/// Carries free-form [`prompt`](Self::prompt) text, a [`kind`](Self::kind)
/// discriminator naming the [`InputResponse`](crate::action::InputResponse) the
/// host must send back, an optional structured [`options`](Self::options) list
/// (for [`PickSingle`](InputKind::PickSingle)), a
/// [`skippable`](Self::skippable) flag for windows that may also be passed, the
/// board [`target`](Self::target) the prompt itself renders on, and the
/// [`nature`](Self::nature) of what is being chosen between.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct InputRequest {
    /// Human-readable text describing what the player must choose.
    pub prompt: String,
    /// The offered options: the one a [`PickSingle`](InputKind::PickSingle)
    /// response names, or the candidates a
    /// [`PickMultiple`](InputKind::PickMultiple) response selects a subset of.
    /// Empty for [`Confirm`](InputKind::Confirm).
    pub options: Vec<ChoiceOption>,
    /// Which response variant the host must send back.
    pub kind: InputKind,
    /// When true the host also offers a Skip/Pass control →
    /// [`InputResponse::Skip`](crate::action::InputResponse::Skip). Orthogonal
    /// to `kind` (e.g. a `PickSingle` reaction window that may also be passed).
    pub skippable: bool,
    /// The board surface this **prompt** renders on; `None` is un-anchored.
    ///
    /// A [`Confirm`](InputKind::Confirm) prompt has no options by construction,
    /// so without this the Mythos encounter draw and the cosmetic skill-test
    /// acknowledge pause are byte-identical on the wire (ADR 0011). A host reads
    /// this anchor for option-less prompts; a `PickSingle`/`PickMultiple` routes
    /// per-option, except that the banner reads it to suppress the open-turn
    /// menu's text.
    pub target: Option<OptionTarget>,
    /// What this prompt asks the player to choose between (ADR 0015).
    ///
    /// It rides on the request rather than on each option because every branch
    /// of one choice shares it.
    pub nature: PromptNature,
    /// Who answers this prompt (ADR 0019).
    ///
    /// The constructors leave it [`Several`](Responder::Several) as a
    /// placeholder. The engine's prompt table attaches the asking frame's
    /// responder to every request it surfaces, so no request the engine
    /// returns carries the placeholder unless several really do answer.
    pub responder: Responder,
}

impl InputRequest {
    /// A single-selection choice over `options` →
    /// [`InputResponse::PickSingle`](crate::action::InputResponse::PickSingle).
    ///
    /// Its [`responder`](Self::responder) is a placeholder, as every
    /// constructor's is: the prompt table (`prompt::request`, which
    /// `prompt::suspend` calls) overwrites it with the asking frame's, so
    /// every request the engine returns carries the table's responder.
    #[must_use]
    pub fn pick_single(text: impl Into<String>, options: Vec<ChoiceOption>) -> Self {
        Self {
            prompt: text.into(),
            options,
            kind: InputKind::PickSingle,
            skippable: false,
            target: None,
            nature: PromptNature::Selection,
            responder: Responder::Several,
        }
    }

    /// A subset-selection prompt over `options`, selecting `count` of them →
    /// [`InputResponse::PickMultiple`](crate::action::InputResponse::PickMultiple).
    ///
    /// Each selected [`OptionId`] names one of `options`, exactly as a
    /// [`PickSingle`](InputKind::PickSingle) response does, and the host
    /// renders each candidate on the surface its option anchors to.
    ///
    /// Its [`responder`](Self::responder) is a placeholder the prompt table
    /// overwrites, as for [`pick_single`](Self::pick_single).
    #[must_use]
    pub fn pick_multiple(
        text: impl Into<String>,
        options: Vec<ChoiceOption>,
        count: PickCount,
    ) -> Self {
        Self {
            prompt: text.into(),
            options,
            kind: InputKind::PickMultiple(count),
            skippable: false,
            target: None,
            nature: PromptNature::Selection,
            responder: Responder::Several,
        }
    }

    /// A binary acknowledge prompt →
    /// [`InputResponse::Confirm`](crate::action::InputResponse::Confirm).
    ///
    /// Its [`responder`](Self::responder) is a placeholder the prompt table
    /// overwrites, as for [`pick_single`](Self::pick_single).
    #[must_use]
    pub fn confirm(text: impl Into<String>) -> Self {
        Self {
            prompt: text.into(),
            options: Vec::new(),
            kind: InputKind::Confirm,
            skippable: false,
            target: None,
            nature: PromptNature::Selection,
            responder: Responder::Several,
        }
    }

    /// Mark this prompt skippable (host renders a Skip/Pass control →
    /// [`InputResponse::Skip`](crate::action::InputResponse::Skip)).
    #[must_use]
    pub fn skippable(mut self) -> Self {
        self.skippable = true;
        self
    }

    /// Anchor this prompt to `target` — the board surface it renders on. The
    /// same `.at(…)` idiom as [`ChoiceOption::at`]. Un-anchored (the default)
    /// means the prompt banner.
    #[must_use]
    pub fn at(mut self, target: OptionTarget) -> Self {
        self.target = Some(target);
        self
    }

    /// Anchor this prompt to `target` when there is one. The request-level twin
    /// of [`ChoiceOption::maybe_at`], for a caller holding an anchor that may
    /// have degraded to `None` because its source left play.
    #[must_use]
    pub fn maybe_at(self, target: Option<OptionTarget>) -> Self {
        match target {
            Some(target) => self.at(target),
            None => self,
        }
    }

    /// Mark this prompt a [`Decision`](PromptNature::Decision) — its options are
    /// alternatives printed on one card, not board entities to disambiguate
    /// between. The same opt-in builder idiom as [`at`](Self::at), and for the
    /// same reason: the default is the status quo (ADR 0015).
    #[must_use]
    pub fn deciding(mut self) -> Self {
        self.nature = PromptNature::Decision;
        self
    }

    /// Name who answers this prompt. The engine's prompt table attaches the
    /// asking frame's responder itself; a host calls this only to build a
    /// request in a test.
    #[must_use]
    pub fn answered_by(mut self, responder: Responder) -> Self {
        self.responder = responder;
        self
    }
}

/// Opaque continuation token returned alongside [`AwaitingInput`].
///
/// The engine uses this to identify which choice point a
/// [`ResolveInput`](crate::action::PlayerAction::ResolveInput) is answering.
/// The inner field is `pub(crate)` so external crates cannot fabricate
/// tokens; they receive them from the engine and pass them back.
///
/// [`AwaitingInput`]: EngineOutcome::AwaitingInput
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ResumeToken(pub(crate) u64);

// The prompt types and their `OptionId`s are these tests' subject, so they
// build options by hand rather than reading them off a session.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pick_single_sets_kind_and_not_skippable() {
        let req =
            InputRequest::pick_single("Choose one", vec![ChoiceOption::new(OptionId(0), "A")]);
        assert_eq!(req.kind, InputKind::PickSingle);
        assert!(!req.skippable);
        assert_eq!(req.options.len(), 1);
    }

    #[test]
    fn pick_multiple_carries_its_options_and_count() {
        let req = InputRequest::pick_multiple(
            "Discard",
            vec![
                ChoiceOption::new(OptionId(0), "A"),
                ChoiceOption::new(OptionId(1), "B"),
            ],
            PickCount::exactly(1),
        );
        assert_eq!(
            req.kind,
            InputKind::PickMultiple(PickCount { min: 1, max: 1 })
        );
        assert!(!req.skippable);
        assert_eq!(req.options.len(), 2);
    }

    #[test]
    fn a_pick_count_admits_the_selections_in_its_range() {
        assert!(PickCount::up_to(2).admits(0));
        assert!(PickCount::up_to(2).admits(2));
        assert!(!PickCount::up_to(2).admits(3));
        assert!(!PickCount::exactly(2).admits(1));
        assert!(PickCount::exactly(2).admits(2));
    }

    #[test]
    fn confirm_sets_kind_and_empty_options() {
        let req = InputRequest::confirm("Draw");
        assert_eq!(req.kind, InputKind::Confirm);
        assert!(!req.skippable);
        assert!(req.options.is_empty());
    }

    #[test]
    fn every_constructor_defaults_to_selection_and_deciding_opts_in() {
        // The silent default is the status quo: a prompt whose author never
        // considered its nature renders as today's context menu (ADR 0015).
        assert_eq!(
            InputRequest::pick_single("w", vec![]).nature,
            PromptNature::Selection
        );
        assert_eq!(
            InputRequest::pick_multiple("w", vec![], PickCount::up_to(0)).nature,
            PromptNature::Selection
        );
        assert_eq!(InputRequest::confirm("w").nature, PromptNature::Selection);
        assert_eq!(
            InputRequest::pick_single("w", vec![]).deciding().nature,
            PromptNature::Decision
        );
    }

    #[test]
    fn request_maybe_at_anchors_only_when_there_is_an_anchor() {
        let req = InputRequest::pick_single("w", vec![]);
        assert_eq!(req.clone().maybe_at(None).target, None);
        assert_eq!(
            req.maybe_at(Some(OptionTarget::Agenda)).target,
            Some(OptionTarget::Agenda)
        );
    }

    /// A request missing its `nature` or its `responder` does not deserialize:
    /// neither field has a default, and a game persisted before one existed
    /// is discarded (ADR 0010).
    #[test]
    fn a_request_serialized_without_a_nature_or_a_responder_does_not_read_back() {
        let full = r#"{"prompt":"w","options":[],"kind":"PickSingle","skippable":false,
            "target":null,"nature":"Selection","responder":"Several"}"#;
        assert_eq!(
            serde_json::from_str::<InputRequest>(full).expect("deserialize"),
            InputRequest::pick_single("w", vec![])
        );
        let no_nature = r#"{"prompt":"w","options":[],"kind":"PickSingle","skippable":false,
            "target":null,"responder":"Several"}"#;
        assert!(serde_json::from_str::<InputRequest>(no_nature).is_err());
        let no_responder = r#"{"prompt":"w","options":[],"kind":"PickSingle","skippable":false,
            "target":null,"nature":"Selection"}"#;
        assert!(serde_json::from_str::<InputRequest>(no_responder).is_err());
    }

    /// An option with no `investigator` does not deserialize: `None` is
    /// written out as `null`, and a missing field is refused rather than
    /// read as `None` (ADR 0010).
    #[test]
    fn an_option_serialized_without_an_investigator_does_not_read_back() {
        let json = r#"{"id":0,"label":"A","target":null}"#;
        assert!(serde_json::from_str::<ChoiceOption>(json).is_err());
        let json = r#"{"id":0,"label":"A","target":null,"investigator":null}"#;
        assert_eq!(
            serde_json::from_str::<ChoiceOption>(json).expect("deserialize"),
            ChoiceOption::new(OptionId(0), "A")
        );
    }

    #[test]
    fn a_responder_and_an_option_investigator_round_trip() {
        let request = InputRequest::pick_single(
            "w",
            vec![ChoiceOption::new(OptionId(0), "A").offered_to(InvestigatorId(2))],
        )
        .answered_by(Responder::Lead(InvestigatorId(2)));
        let back: InputRequest =
            serde_json::from_str(&serde_json::to_string(&request).expect("serialize"))
                .expect("deserialize");
        assert_eq!(back, request);
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
                ChoiceOption::new(OptionId(0), "Take 2 horror"),
                ChoiceOption::new(OptionId(1), "Each discards 1"),
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
    fn new_is_unanchored_and_at_anchors() {
        let opt = ChoiceOption::new(OptionId(3), "End turn");
        assert_eq!(opt.id, OptionId(3));
        assert_eq!(opt.label, "End turn");
        assert_eq!(opt.target, None, "`new` is un-anchored (ADR 0011)");

        let anchored = ChoiceOption::new(OptionId(3), "End turn")
            .at(OptionTarget::TurnControl(InvestigatorId(1)));
        assert_eq!(
            anchored.target,
            Some(OptionTarget::TurnControl(InvestigatorId(1)))
        );
    }

    #[test]
    fn maybe_at_anchors_only_when_there_is_an_anchor() {
        let opt = ChoiceOption::new(OptionId(0), "x");
        assert_eq!(opt.clone().maybe_at(None).target, None);
        assert_eq!(
            opt.maybe_at(Some(OptionTarget::EncounterDeck)).target,
            Some(OptionTarget::EncounterDeck)
        );
    }

    #[test]
    fn request_constructors_are_unanchored_and_at_anchors() {
        assert_eq!(InputRequest::confirm("Draw").target, None);
        assert_eq!(
            InputRequest::pick_multiple("Commit", vec![], PickCount::up_to(0)).target,
            None
        );
        assert_eq!(InputRequest::pick_single("Choose", vec![]).target, None);

        let req = InputRequest::confirm("Draw").at(OptionTarget::EncounterDeck);
        assert_eq!(req.target, Some(OptionTarget::EncounterDeck));
        assert_eq!(req.kind, InputKind::Confirm);
    }

    #[test]
    fn request_anchor_round_trips_on_the_wire() {
        // The two option-less `Confirm` prompts differ only by this field
        // (ADR 0011), so it has to survive serialization.
        let req = InputRequest::confirm("Draw an encounter card").at(OptionTarget::EncounterDeck);
        let json = serde_json::to_string(&req).expect("serialize");
        let back: InputRequest = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.target, Some(OptionTarget::EncounterDeck));
        assert_eq!(back, req);
    }

    #[test]
    fn awaiting_input_round_trips_option_target() {
        let outcome = EngineOutcome::AwaitingInput {
            request: InputRequest::pick_single(
                "Choose an action",
                vec![
                    ChoiceOption::new(OptionId(0), "End turn"),
                    ChoiceOption::new(OptionId(1), "Fight Ghoul")
                        .at(OptionTarget::Enemy(EnemyId(7))),
                ],
            ),
            resume_token: ResumeToken(0),
        };
        let json = serde_json::to_string(&outcome).expect("serialize");
        let back: EngineOutcome = serde_json::from_str(&json).expect("deserialize");
        let EngineOutcome::AwaitingInput { request, .. } = back else {
            panic!("expected AwaitingInput, got {back:?}");
        };
        assert_eq!(request.options[0].target, None);
        assert_eq!(
            request.options[1].target,
            Some(OptionTarget::Enemy(EnemyId(7)))
        );
    }
}
