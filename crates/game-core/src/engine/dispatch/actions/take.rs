//! **Taking an action** (see `GLOSSARY.md`): deciding whether an investigator
//! may take it, paying its actions, and deciding whether it provokes attacks of
//! opportunity.
//!
//! Every action is taken through [`take`]: the basic actions, a non-fast play,
//! and an action-cost ability. A fast play or a zero-action ability spends no
//! action, so it takes none and never reaches the step. The turn menu asks
//! [`check`] what taking a basic action would cost, and the play and activation
//! validators ask it whether a non-fast play or an action-cost ability can be
//! afforded.
//!
//! `glossary/Action.md`: *"When performing an action, all costs of the action
//! are first paid. Then, the consequences of the action resolve."* And
//! `glossary/Attack_of_Opportunity.md` puts the attacks between the two:
//! *"An attack of opportunity is made immediately after all costs of initiating
//! the action that provoked the attack have been paid, but before the
//! application of that action's effect upon the game state."* [`take`] is the
//! one place that order is written: pay the actions, pay the caller's own
//! costs, then the attacks, then perform.
//!
//! # The action pool
//!
//! An investigator's actions are their **standard** actions
//! ([`Investigator::actions_remaining`](crate::state::Investigator::actions_remaining))
//! plus their **additional** actions (see **Standard action / additional
//! action** in `GLOSSARY.md`). The additional ones are never stored as a count:
//! [`additional_actions`] works the list out each time it is needed, from the
//! cards in play whose grant has not been used this turn (Leo De Luca 01048)
//! and the grants effects pushed this turn ("Skids" O'Toole 01003). FAQ 1.10
//! (`glossary/Action.md`): *"If an investigator has one or more additional
//! actions during his or her turn, the first action he or she takes that is
//! able to qualify as that additional action automatically uses that
//! additional action."* So [`quote`] counts the entries that admit the action
//! being taken, and [`take`] spends those before any standard action.
//! [`ActionDescription`] is what an entry's restriction is tested against:
//! Daisy Walker 01002's additional action pays only for a Tome's `[action]`
//! ability. The same entry: *"If an action qualifies as more than one of an
//! investigator’s additional actions, he or she may choose which additional
//! action is used."* Where the choice changes anything, [`take`] asks before it
//! pays.

use std::borrow::Cow;
use std::fmt;

use card_dsl::dsl::{ActionClass, ActionDesignator, AdditionalActionRestriction, Effect};

use crate::action::InputResponse;
use crate::card_registry::{self, CardRegistry};
use crate::engine::dispatch::actions::evade::EvadeModification;
use crate::engine::dispatch::actions::fight::FightModification;
use crate::engine::dispatch::actions::{draw, engage, evade, fight, investigate, r#move, resource};
use crate::engine::dispatch::{abilities, cards, choice, combat, cursor};
use crate::engine::outcome::{
    ChoiceOption, EngineOutcome, InputRequest, OptionId, OptionTarget, ResumeToken,
};
use crate::engine::{abilities_in_effect, board, evaluator, Cx};
use crate::event::Event;
use crate::state::{
    AbilityAddress, AbilitySource, ActionResolutionFrame, ActionResume,
    AdditionalActionChoiceFrame, AdditionalActionGrant, CardCode, CardInstanceId, GameState,
    InvestigatorId, Phase, Status,
};

/// What kind of action is being taken: one of the seven basic actions
/// (`glossary/Action.md`, FAQ 1.30: *"The following are basic actions:
/// **Draw**, **Resource**, **Move**, **Investigate**, **Fight**, **Engage**,
/// and **Evade**."*), a **Play** of the card `code`, or an **Activate** of an
/// ability on `source`. The last two carry their source so a rule that applies
/// to some actions by what they are taken from can be tested against the
/// description (Daisy Walker 01002's Tome-only additional action). An
/// activation also names which of the source's abilities it is, so it can be
/// taken again once the player has chosen which additional action pays.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ActionKind {
    Draw,
    Resource,
    Move,
    Investigate,
    Fight,
    Engage,
    Evade,
    Play {
        code: CardCode,
    },
    Activate {
        source: AbilitySource,
        address: AbilityAddress,
    },
}

impl ActionKind {
    /// The [`ActionClass`] a surcharge keys on, for the three kinds one names.
    /// A play or an activation names none of its own; an activation's class
    /// comes from its designator ([`ActionDescription::action_class`]).
    fn action_class(&self) -> Option<ActionClass> {
        match self {
            Self::Move => Some(ActionClass::Move),
            Self::Fight => Some(ActionClass::Fight),
            Self::Evade => Some(ActionClass::Evade),
            Self::Draw
            | Self::Resource
            | Self::Investigate
            | Self::Engage
            | Self::Play { .. }
            | Self::Activate { .. } => None,
        }
    }
}

/// The action's name, for refusal reasons.
impl fmt::Display for ActionKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Draw => f.write_str("Draw"),
            Self::Resource => f.write_str("Resource"),
            Self::Move => f.write_str("Move"),
            Self::Investigate => f.write_str("Investigate"),
            Self::Fight => f.write_str("Fight"),
            Self::Engage => f.write_str("Engage"),
            Self::Evade => f.write_str("Evade"),
            Self::Play { code } => write!(f, "Play {code}"),
            Self::Activate { source, .. } => write!(f, "Activate {source:?}"),
        }
    }
}

/// The action being taken: its kind, its printed action cost, and its bold
/// action designator if it has one. Everything [`check`] and [`take`] decide is
/// read from this alone, never from the board's attackers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ActionDescription {
    kind: ActionKind,
    action_cost: u8,
    designator: Option<ActionDesignator>,
    /// The player's answer to the FAQ 1.10 choice, once asked: the index of
    /// the chosen option among [`additional_action_options`]. `None` until
    /// the player has been asked.
    chosen_additional_action: Option<usize>,
}

impl ActionDescription {
    /// A basic action, which costs one action and prints no designator.
    pub(crate) fn basic(kind: ActionKind) -> Self {
        Self {
            kind,
            action_cost: 1,
            designator: None,
            chosen_additional_action: None,
        }
    }

    /// A non-fast play of the card `code`, which costs one action, printing
    /// `designator` on its `OnPlay` ability if it prints one. Playing such an
    /// event *is* the designated action, so it is exempt from attacks of
    /// opportunity and surcharged as that action. Official FAQ,
    /// `Frequently_Asked_Questions.md`, on Backstab 01051: *"Abilities with a
    /// bold action designator (like Fight, Evade or Investigate) count as an
    /// action of that type. In this case, since Backstab counts as a Fight
    /// action, no attacks of opportunity are made"*. Blinding Light 01066's
    /// **Evade** is the one built.
    pub(crate) fn play(code: CardCode, designator: Option<ActionDesignator>) -> Self {
        Self {
            kind: ActionKind::Play { code },
            action_cost: 1,
            designator,
            chosen_additional_action: None,
        }
    }

    /// An activation of the ability at `address` on `source`, printing
    /// `action_cost` actions (at least one; a zero-action ability is not
    /// taken) and `designator`.
    pub(crate) fn activate(
        source: AbilitySource,
        address: AbilityAddress,
        action_cost: u8,
        designator: Option<ActionDesignator>,
    ) -> Self {
        Self {
            kind: ActionKind::Activate { source, address },
            action_cost,
            designator,
            chosen_additional_action: None,
        }
    }

    /// This action, paid for with the additional action the player chose: the
    /// `index`th option of the prompt [`take`] put to them.
    pub(crate) fn paid_with_chosen_additional_action(mut self, index: usize) -> Self {
        self.chosen_additional_action = Some(index);
        self
    }

    /// The class a surcharge keys on: the designator's if it prints one,
    /// otherwise the kind's.
    fn action_class(&self) -> Option<ActionClass> {
        match &self.designator {
            Some(designator) => designator.action_class(),
            None => self.kind.action_class(),
        }
    }

    /// Whether taking this action provokes attacks of opportunity.
    /// `glossary/Attack_of_Opportunity.md`, verbatim:
    ///
    /// > Each time an investigator is engaged with one or more ready enemies and
    /// > takes an action other than to **fight**, to **evade**, or to activate a
    /// > **parley** or **resign** ability, each of those enemies makes an attack
    /// > of opportunity against the investigator...
    ///
    /// and, added in FAQ 1.1, *"Attacks of Opportunity are only triggered when 1
    /// or more of an investigator’s actions are being spent or used to trigger
    /// an ability or action."* So an action costing nothing never provokes.
    ///
    /// The same entry: *"An ability that costs more than one action only
    /// provokes one attack of opportunity from each engaged enemy."* So the
    /// answer is whether, never how many; `drive_aoo` attacks once per enemy.
    ///
    /// A function of the description, not of the board: a provoking action
    /// parks behind its attacks even with no enemy engaged.
    fn provokes(&self) -> bool {
        let exempt_kind = matches!(self.kind, ActionKind::Fight | ActionKind::Evade);
        let exempt_designator = matches!(
            self.designator,
            Some(
                ActionDesignator::Fight { .. }
                    | ActionDesignator::Evade { .. }
                    | ActionDesignator::AutomaticEvade
                    | ActionDesignator::Parley
                    | ActionDesignator::Resign
            )
        );
        self.action_cost > 0 && !exempt_kind && !exempt_designator
    }
}

/// One additional action an investigator holds right now: what it may be used
/// for, and where it comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
struct AdditionalAction {
    restriction: Option<AdditionalActionRestriction>,
    origin: AdditionalActionOrigin,
}

impl AdditionalAction {
    /// Whether taking `description` may use this additional action. An
    /// unrestricted one admits every action.
    ///
    /// [`ActivatedAbilityOn`](AdditionalActionRestriction::ActivatedAbilityOn)
    /// admits only an activation that costs an action, of an ability on a card
    /// in play whose printed card matches the filter: Daisy Walker 01002's
    /// *"which can only be used on \[\[Tome\]\] \[action\] abilities"*. A
    /// play, a basic action, and an ability on a location, enemy, act or agenda
    /// never qualify.
    fn admits(
        &self,
        state: &GameState,
        cards: &CardRegistry,
        description: &ActionDescription,
    ) -> bool {
        match &self.restriction {
            None => true,
            Some(AdditionalActionRestriction::ActivatedAbilityOn(filter)) => {
                let ActionKind::Activate {
                    source: AbilitySource::InPlay(instance),
                    ..
                } = description.kind
                else {
                    return false;
                };
                description.action_cost >= 1
                    && board::find_instance(state, instance).is_some_and(|(card, _)| {
                        card_registry::card_matches(cards, &card.code, filter)
                    })
            }
        }
    }
}

/// Where an [`AdditionalAction`] comes from, which decides what using it
/// changes. Ordered as [`additional_actions`] sorts the entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum AdditionalActionOrigin {
    /// An [`AdditionalActionWhileInPlay`](card_dsl::dsl::Effect::AdditionalActionWhileInPlay)
    /// grant on this card in play (Leo De Luca 01048). Using it marks the
    /// instance in
    /// [`Investigator::grants_spent_this_turn`](crate::state::Investigator::grants_spent_this_turn).
    /// Declared ahead of `Effect`, so it sorts first.
    InPlay(CardInstanceId),
    /// The `index`th entry of
    /// [`Investigator::additional_actions_this_turn`](crate::state::Investigator::additional_actions_this_turn).
    /// Using it removes the entry.
    Effect { index: usize },
}

/// The additional actions `investigator` holds right now, in the order they are
/// used. Empty unless it is their turn and the turn is not ending
/// ([`cursor::turn_open`]): every grant is *"during your turn"* or *"this
/// turn"*, and FAQ 1.31 (`glossary/Action.md`): *"Any unused actions are
/// forfeited at the end of an investigator's turn"*.
///
/// Sorted restricted before unrestricted, then a grant from a card in play
/// before one an effect granted. Spending takes entries from the front, which
/// settles the choice the rules give the player wherever it changes nothing.
/// FAQ 1.10: *"If an action qualifies as more than one of an investigator’s
/// additional actions, he or she may choose which additional action is used."*
/// Leo De Luca 01048's ruling says the same of Leo and Quick Thinking (*"you
/// may choose which additional action you are actually using"*).
///
/// Among entries with the same restriction the order dominates any choice:
/// they buy the same actions, and spending a card-in-play one first protects
/// the action from the card leaving play. Between entries with different
/// restrictions it does not. Spending Daisy Walker 01002's Tome-only action
/// first keeps Leo De Luca 01048's for anything, but Leo can leave play and
/// take his action with him, and hers cannot. So where an action qualifies as
/// entries with more than one restriction, [`take`] asks the player
/// ([`additional_action_options`]), and the sort only orders the options.
fn additional_actions(
    state: &GameState,
    cards: &CardRegistry,
    investigator: InvestigatorId,
) -> Vec<AdditionalAction> {
    let Some(inv) = state.investigators.get(&investigator) else {
        return Vec::new();
    };
    if !cursor::turn_open(state, investigator) {
        return Vec::new();
    }
    // The constant sweep: each card whose while-in-play grant has not been
    // used this turn. Read off the card every time, so the grant is gone the
    // moment the card leaves play. Through the ADR 0014 funnel, so a grant the
    // board gives one of the investigator's cards counts too (no corpus card
    // grants one).
    let mut pool: Vec<AdditionalAction> =
        abilities_in_effect::constants_of(state, cards, investigator)
            .into_iter()
            .filter(|(instance, _)| !inv.grants_spent_this_turn.contains(instance))
            .filter_map(|(instance, ability)| match ability.effect {
                Effect::AdditionalActionWhileInPlay { restriction } => Some(AdditionalAction {
                    restriction,
                    origin: AdditionalActionOrigin::InPlay(instance),
                }),
                _ => None,
            })
            .collect();
    pool.extend(
        inv.additional_actions_this_turn
            .iter()
            .enumerate()
            .map(|(index, grant)| AdditionalAction {
                restriction: grant.restriction.clone(),
                origin: AdditionalActionOrigin::Effect { index },
            }),
    );
    pool.sort_by_key(|action| (action.restriction.is_none(), action.origin));
    pool
}

/// How many actions `investigator` can still take this turn: their standard
/// actions plus every additional action they hold, whatever it is restricted
/// to.
///
/// What a client shows as "actions remaining". `0` for an investigator not in
/// state.
///
/// Whether a particular action can be afforded is the take step's question, not
/// this count's: a restricted additional action counts here but pays only for
/// the actions it admits.
#[must_use]
pub fn actions_available(
    state: &GameState,
    cards: &CardRegistry,
    investigator: InvestigatorId,
) -> u8 {
    let Some(inv) = state.investigators.get(&investigator) else {
        return 0;
    };
    let additional = additional_actions(state, cards, investigator).len();
    inv.actions_remaining
        .saturating_add(u8::try_from(additional).unwrap_or(u8::MAX))
}

/// Use the additional actions `origins` names for `investigator`: remove each
/// effect grant, and mark each card in play used for the turn. `origins` comes
/// from [`additional_actions`] on the same state.
fn use_additional_actions(
    state: &mut GameState,
    investigator: InvestigatorId,
    origins: &[AdditionalActionOrigin],
) {
    let Some(inv) = state.investigators.get_mut(&investigator) else {
        return;
    };
    let mut effect_indices: Vec<usize> = Vec::new();
    for origin in origins {
        match origin {
            AdditionalActionOrigin::InPlay(instance) => {
                inv.grants_spent_this_turn.insert(*instance);
            }
            AdditionalActionOrigin::Effect { index } => effect_indices.push(*index),
        }
    }
    // Highest index first, so each removal leaves the lower indices in place.
    effect_indices.sort_unstable_by(|a, b| b.cmp(a));
    for index in effect_indices {
        inv.additional_actions_this_turn.remove(index);
    }
}

/// `investigator` gains an additional action for this turn, which `restriction`
/// limits to the actions it admits ([`Effect::GainAdditionalAction`]).
///
/// A no-op unless their turn is open and not ending. Outside their turn the
/// grant has nothing to be used on, and stored it would outlive the turn it was
/// granted in. "Skids" O'Toole 01003's grant is gated on *"During your turn"*,
/// so it only gets here during his turn. The one case it reaches here with
/// nothing to use the grant on is the end-of-turn window, after the unused
/// actions were forfeited, and there it is forfeited at once.
///
/// [`Effect::GainAdditionalAction`]: card_dsl::dsl::Effect::GainAdditionalAction
pub(crate) fn gain_additional_action(
    cx: &mut Cx,
    investigator: InvestigatorId,
    restriction: Option<AdditionalActionRestriction>,
) {
    debug_assert_eq!(
        cx.state.active_investigator,
        Some(investigator),
        "gain_additional_action: a this-turn grant outside {investigator:?}'s turn"
    );
    if !cursor::turn_open(cx.state, investigator) {
        return;
    }
    if let Some(inv) = cx.state.investigators.get_mut(&investigator) {
        inv.additional_actions_this_turn
            .push(AdditionalActionGrant { restriction });
    }
}

/// What taking an action costs: the total actions, the additional actions that
/// pay for part of it, the standard actions that pay the rest, and the
/// surcharge sources to mark spent once they are paid.
struct Quote {
    total: u8,
    additional: Vec<AdditionalActionOrigin>,
    /// `total` less the additional actions: what the standard actions pay.
    from_standard: u8,
    surcharge_sources: Vec<CardInstanceId>,
    /// The additional actions the player must choose between before the
    /// action is paid for, or empty when there is no choice to put
    /// ([`additional_action_options`]). The cost is the same whichever is
    /// chosen, so `additional` is the unchosen order's.
    choice: Vec<AdditionalActionOrigin>,
}

/// The FAQ 1.10 choice among `qualifying`, the additional actions an action
/// qualifies as, in [`additional_actions`] order: the first entry of each
/// distinct restriction, as indices into `qualifying`. A choice exists only
/// when there are two or more. Within one restriction the sort's front entry
/// dominates the rest (see [`additional_actions`]), so it stands for them.
///
/// **One pick, so only for an action costing 1.** [`quote`] asks only when
/// the action leaves a qualifying entry unspent: an action that spends every
/// one of them has nothing to choose, and pays as the sort orders it. An
/// action costing 2 or more that leaves one unspent really asks for a *set*
/// of entries, which one pick followed by the sort's order cannot express.
/// No corpus card reaches that case (no Tome prints a multi-action ability,
/// and Frozen in Fear 01164's surcharge prices only move, fight and evade), so
/// the set choice is not built and [`quote`] trips a `debug_assert!` there.
/// The first card that reaches it fails its own test, rather than inheriting
/// a guess at what the player meant.
fn additional_action_options(qualifying: &[AdditionalAction]) -> Vec<usize> {
    let mut options: Vec<usize> = Vec::new();
    for (index, action) in qualifying.iter().enumerate() {
        if !options
            .iter()
            .any(|&seen| qualifying[seen].restriction == action.restriction)
        {
            options.push(index);
        }
    }
    options
}

/// Whether `investigator` may take the action `description` describes, and if so
/// how many actions it costs: its printed cost plus any surcharge (Frozen in
/// Fear 01164). Pure, so the turn menu can price an action without taking it.
///
/// Checks, in order: the investigator exists, is Active, and it is the
/// Investigation phase and their turn; then that they can afford the total,
/// from their standard actions plus the additional actions that admit it. An
/// unknown investigator is refused, never a panic, since the id can arrive in a
/// client message.
pub(crate) fn check(
    state: &GameState,
    cards: &CardRegistry,
    investigator: InvestigatorId,
    description: &ActionDescription,
) -> Result<u8, Cow<'static, str>> {
    quote(state, cards, investigator, description).map(|quote| quote.total)
}

fn quote(
    state: &GameState,
    cards: &CardRegistry,
    investigator: InvestigatorId,
    description: &ActionDescription,
) -> Result<Quote, Cow<'static, str>> {
    let name = &description.kind;
    let Some(inv) = state.investigators.get(&investigator) else {
        return Err(format!("{name}: investigator {investigator:?} is not in state").into());
    };
    if inv.status != Status::Active {
        return Err(format!(
            "{name}: {investigator:?} is not Active (status {:?})",
            inv.status
        )
        .into());
    }
    if state.phase != Phase::Investigation {
        return Err(format!(
            "{name} is only valid during the Investigation phase (was {:?})",
            state.phase
        )
        .into());
    }
    if state.active_investigator != Some(investigator) {
        return Err(format!(
            "{name}: {investigator:?} is not the active investigator ({:?})",
            state.active_investigator
        )
        .into());
    }
    // The `ExtraActionCost` surcharge, plus the `first_each_round` sources to
    // mark spent once the action is paid; no card data for a code means no
    // surcharge. [`check`] and [`take`] price every action here, an
    // activation included, whose bold designator names the class via
    // [`ActionDesignator::action_class`]. Nothing outside the step
    // prices a surcharge: the play and activation validators ask [`check`].
    // One pricing is the point: a surcharge only one path applies is the bug
    // that made shooting a weapon cheaper than punching.
    let (extra, surcharge_sources) = match description.action_class() {
        Some(class) => evaluator::pending_action_surcharge(state, cards, investigator, class),
        None => (0, Vec::new()),
    };
    let total = description.action_cost.saturating_add(extra);
    let mut qualifying: Vec<AdditionalAction> = additional_actions(state, cards, investigator)
        .into_iter()
        .filter(|action| action.admits(state, cards, description))
        .collect();
    // An action costing more than one spends one additional action per action
    // point, then standard actions for the rest. The vendored rules say nothing
    // on it directly. The FAQ's Discipline (Balance of Body) answer treats a
    // multi-action cost as points drawn from separate pools: *"if so, you must
    // still pay the additional action cost of that ability, and those
    // additional actions would have to come from your standard pool of actions
    // or from a source other than Discipline (Balance of Body)."*
    let from_additional = qualifying.len().min(usize::from(total));
    // FAQ 1.10's choice, where the qualifying entries carry more than one
    // restriction and the action leaves at least one of them unspent (one that
    // spends them all has nothing to choose). Unanswered, the quote names the
    // options for [`take`] to ask. Answered, the chosen entry is spent first
    // and the rest keep the sort's order.
    let options = additional_action_options(&qualifying);
    let offered = if options.len() > 1 && from_additional < qualifying.len() {
        debug_assert!(
            total <= 1,
            "{name}: an action costing {total} leaves a FAQ 1.10 additional-action choice, \
             which is a choice of a set of grants; only a one-pick choice is built \
             (see additional_action_options)"
        );
        options.len()
    } else {
        0
    };
    let mut choice = Vec::new();
    match description.chosen_additional_action {
        None if offered > 0 => {
            choice = options.iter().map(|&i| qualifying[i].origin).collect();
        }
        None => {}
        Some(chosen) => {
            let Some(&index) = options.get(chosen).filter(|_| offered > 0) else {
                return Err(format!(
                    "{name}: additional action option {chosen} out of range (0..{offered})"
                )
                .into());
            };
            let chosen = qualifying.remove(index);
            qualifying.insert(0, chosen);
        }
    }
    let from_standard = total - u8::try_from(from_additional).unwrap_or(total);
    if inv.actions_remaining < from_standard {
        return Err(format!("{name} requires {total} action point(s)").into());
    }
    Ok(Quote {
        total,
        additional: qualifying[..from_additional]
            .iter()
            .map(|action| action.origin)
            .collect(),
        from_standard,
        surcharge_sources,
        choice,
    })
}

/// Take the action `description` describes for `investigator`: pay its actions,
/// mark the surcharge sources it consumed, run `before_attacks`, then either
/// park the action behind its attacks of opportunity or perform it now.
///
/// The actions are paid from the additional actions that admit the action
/// first, then from the standard ones (FAQ 1.10, see the module doc). Where the
/// action qualifies as additional actions with different restrictions, nothing
/// is paid yet: an
/// [`AdditionalActionChoice`](crate::state::Continuation::AdditionalActionChoice)
/// frame asks the player which to use, and the answer takes the action again
/// ([`resume_additional_action_choice`]).
///
/// `before_attacks` pays the caller's own costs and returns the rest of the
/// action as an [`ActionResume`]. A basic action has no other cost, so its hook
/// just returns its resume. A play's pays the resource cost and commences the
/// play; an activation's pays the ability's other costs and announces it. If
/// the action provokes, the resume is parked on an
/// [`ActionResolution`](crate::state::Continuation::ActionResolution) frame and
/// the attacks are driven; the action is performed when the frame resumes.
/// Otherwise it is performed immediately, by the same [`perform`] the frame
/// would reach.
///
/// Callers validate their targets with [`check`] and their own checks first:
/// this mutates once [`check`] passes. A hook that refuses after the actions
/// are paid returns `Rejected`, and the snapshot rollback on `Rejected` undoes
/// the payment.
pub(crate) fn take(
    cx: &mut Cx,
    investigator: InvestigatorId,
    description: &ActionDescription,
    before_attacks: impl FnOnce(&mut Cx) -> Result<ActionResume, Cow<'static, str>>,
) -> EngineOutcome {
    let Quote {
        additional,
        from_standard,
        surcharge_sources,
        choice,
        ..
    } = match quote(cx.state, cx.cards, investigator, description) {
        Ok(quote) => quote,
        Err(reason) => return EngineOutcome::Rejected { reason },
    };
    if !choice.is_empty() {
        return ask_additional_action_choice(cx, investigator, description, &choice);
    }
    use_additional_actions(cx.state, investigator, &additional);
    let inv = cx
        .state
        .investigators
        .get_mut(&investigator)
        .expect("quote checked the investigator exists");
    // The surcharge is spent, so its `first_each_round` sources are done for the
    // round: Frozen in Fear 01164's *"The first time you perform one of the
    // following actions (move, fight, or evade) each round"*.
    inv.action_surcharge_spent_this_round
        .extend(surcharge_sources);
    // The event announces the standard count, so an action paid wholly from
    // additional actions changes nothing it reports.
    if from_standard > 0 {
        let new_count = inv.actions_remaining - from_standard;
        inv.actions_remaining = new_count;
        cx.events.push(Event::ActionsRemainingChanged {
            investigator,
            new_count,
        });
    }

    // TODO(#649): the hook runs ahead of the attacks, and for a play it
    // commences the play: `CardPlayed` is emitted and the card leaves hand. So
    // the engine's order is pay action → pay resources → commence play →
    // attacks of opportunity. The rules' order commences after the attacks:
    // `glossary/Limbo.md`, *"An event card enters limbo during step 3 of the
    // Initiation Sequence, after costs are paid and attacks of opportunity are
    // made."* Reordering needs a binding for the hand card that survives the
    // attacks, which the resume carries only once commence has minted it.
    let resume = match before_attacks(cx) {
        Ok(resume) => resume,
        Err(reason) => return EngineOutcome::Rejected { reason },
    };
    if description.provokes() {
        cx.state.continuations.push(ActionResolutionFrame {
            investigator,
            resume,
        });
        combat::drive_aoo(cx, investigator)
    } else {
        perform(cx, investigator, resume)
    }
}

/// Pause the action `description` describes, before anything is paid, for
/// `investigator` to choose which of the additional actions `offered` names pays
/// for it. Option `i` is `offered[i]`, anchored to the card granting it; an
/// effect's grant has no card, so it is offered un-anchored.
fn ask_additional_action_choice(
    cx: &mut Cx,
    investigator: InvestigatorId,
    description: &ActionDescription,
    offered: &[AdditionalActionOrigin],
) -> EngineOutcome {
    // Only an activation reaches here today: the one restriction,
    // `ActivatedAbilityOn`, admits nothing else (see `AdditionalAction::admits`),
    // and an action only unrestricted entries admit has one option. A later
    // restriction that admits another kind needs a frame that can resume it, so
    // until then that kind is refused rather than paused.
    let ActionKind::Activate { source, address } = &description.kind else {
        return EngineOutcome::Rejected {
            reason: format!(
                "take: choosing between additional actions is supported only for an \
                 activation, the one kind a restricted additional action admits today; \
                 got {}",
                description.kind
            )
            .into(),
        };
    };
    cx.state.continuations.push(AdditionalActionChoiceFrame {
        investigator,
        source: *source,
        address: address.clone(),
    });
    let options = offered
        .iter()
        .enumerate()
        .map(|(i, origin)| {
            let id = OptionId(u32::try_from(i).expect("offered option count fits in u32"));
            match origin {
                AdditionalActionOrigin::InPlay(instance) => {
                    let label = board::find_instance(cx.state, *instance).map_or_else(
                        || "Additional action".to_owned(),
                        |(card, _)| choice::card_name(cx.cards, &card.code),
                    );
                    ChoiceOption::new(id, label).at(OptionTarget::CardInstance(*instance))
                }
                AdditionalActionOrigin::Effect { .. } => {
                    ChoiceOption::new(id, "Additional action gained this turn")
                }
            }
        })
        .collect();
    EngineOutcome::AwaitingInput {
        request: InputRequest::pick_single(
            format!(
                "Investigator {investigator:?}: choose which additional action to use for \
                 this action."
            ),
            options,
        ),
        resume_token: ResumeToken(0),
    }
}

/// Resume the FAQ 1.10 choice: take the paused activation again, paid for with
/// the chosen additional action. The state is as it was when the choice was
/// put (the prompt refuses every other action), so the activation re-validates
/// and re-quotes to the same options, and the quote checks the pick. An invalid
/// pick rejects, and the `apply` boundary's rollback restores the frame.
pub(in crate::engine::dispatch) fn resume_additional_action_choice(
    cx: &mut Cx,
    response: &InputResponse,
) -> EngineOutcome {
    let InputResponse::PickSingle(OptionId(chosen)) = response else {
        return EngineOutcome::Rejected {
            reason: format!(
                "ResolveInput: the additional-action choice expects PickSingle, got {response:?}"
            )
            .into(),
        };
    };
    let AdditionalActionChoiceFrame {
        investigator,
        source,
        address,
    } = cx
        .state
        .continuations
        .pop_expect::<AdditionalActionChoiceFrame>();
    let chosen = usize::try_from(*chosen).unwrap_or(usize::MAX);
    abilities::activate_ability_paid_with(cx, investigator, source, &address, chosen)
}

/// `investigator` loses `count` actions, saturating at none left.
/// `glossary/Action.md` (FAQ 1.10): *"If an effect causes an investigator to
/// lose one or more actions, that investigator has that many fewer standard
/// actions to take that turn (the investigator’s three standard actions are the
/// ones that are "lost" first). If an investigator only has additional actions
/// remaining, those are then lost, in an order of the investigator’s
/// choosing."*
///
/// So the standard actions go first, then the additional ones in the order
/// [`additional_actions`] sorts them. That order stands in for the
/// investigator's choice: unlike spending, losing does not ask.
///
/// TODO(#1100): this is a gap, not a reading. Daisy Walker 01002's ruling
/// (<https://arkhamdb.com/card/01002>) has the additional actions lost *"of
/// your choice"*, and the sort order picks for the investigator instead. Next
/// to Leo De Luca 01048, it decides whether her Tome-only action or his
/// unrestricted one survives.
///
/// Announces the new standard count only when it changed.
///
/// Reached through `NativeCx::lose_actions` (Blinding Light 01066's *"lose 1
/// action this turn"*). An investigator not in state loses nothing.
pub(crate) fn lose_actions(cx: &mut Cx, investigator: InvestigatorId, count: u8) {
    let Some(standard) = cx
        .state
        .investigators
        .get(&investigator)
        .map(|inv| inv.actions_remaining)
    else {
        return;
    };
    let from_standard = count.min(standard);
    let from_additional = usize::from(count - from_standard);
    if from_additional > 0 {
        let lost: Vec<AdditionalActionOrigin> =
            additional_actions(cx.state, cx.cards, investigator)
                .into_iter()
                .take(from_additional)
                .map(|action| action.origin)
                .collect();
        use_additional_actions(cx.state, investigator, &lost);
    }
    if from_standard == 0 {
        return;
    }
    let inv = cx
        .state
        .investigators
        .get_mut(&investigator)
        .expect("checked above");
    let new_count = standard - from_standard;
    inv.actions_remaining = new_count;
    cx.events.push(Event::ActionsRemainingChanged {
        investigator,
        new_count,
    });
}

/// Perform the action `resume` names for `investigator`: the action's effect,
/// once its costs are paid and any attacks of opportunity are made.
///
/// Reached two ways: directly from [`take`] for an action that provokes
/// nothing, and from `resume_action_resolution` once a parked action's attacks
/// are done and the actor is still Active.
pub(in crate::engine::dispatch) fn perform(
    cx: &mut Cx,
    investigator: InvestigatorId,
    resume: ActionResume,
) -> EngineOutcome {
    match resume {
        ActionResume::Move { destination } => r#move::perform(cx, investigator, destination),
        // A basic investigation carries no modification. A designated one
        // (Flashlight 01087) reaches the same perform with its shroud modifier.
        ActionResume::Investigate => investigate::perform(cx, investigator, None, None, None),
        ActionResume::Resource => resource::perform(cx, investigator),
        ActionResume::Engage { enemy } => engage::perform(cx, investigator, enemy),
        ActionResume::Draw => draw::perform(cx, investigator),
        // A basic attack carries no modification. A designated Fight (every
        // corpus weapon) reaches the same primary with its combat bonus and its
        // bonus damage.
        ActionResume::Fight { enemy } => {
            fight::perform(cx, investigator, enemy, FightModification::default(), None)
        }
        // Likewise a basic evasion; a designated Evade (Blinding Light 01066)
        // reaches the same primary with its skill override and clauses.
        ActionResume::Evade { enemy } => {
            evade::perform(cx, investigator, enemy, EvadeModification::default(), None)
        }
        ActionResume::ActivateAbility {
            source,
            designator,
            effect,
        } => abilities::resume_activate_ability(
            cx,
            investigator,
            source,
            designator.as_ref(),
            &effect,
        ),
        ActionResume::PlayCard { card } => {
            let Some(card) = card else {
                unreachable!(
                    "perform: the play frame for {investigator:?} lost its card while they \
                     are still Active — elimination is the only thing that empties an \
                     ActionResolution frame (see Continuation::take_play_in_progress), and it \
                     flips status first"
                );
            };
            cards::resume_play_card(cx, investigator, card)
        }
    }
}
