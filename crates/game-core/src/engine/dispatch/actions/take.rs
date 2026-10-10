//! **Taking an action** (see `GLOSSARY.md`): deciding whether an investigator
//! may take it, paying its actions, and deciding whether it provokes attacks of
//! opportunity. Every action is taken through [`take`]: the basic actions, a
//! non-fast play, and an action-cost ability. A fast play or fast ability spends
//! no action, so it takes none and never reaches the step. The turn menu asks
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
//! Additional actions (#778, #779) are not modelled. When they are, payment in
//! [`take`] is where a pool plugs in, and [`ActionDescription`] is what it tests
//! an action against.

use std::borrow::Cow;
use std::fmt;

use card_dsl::dsl::{ActionClass, ActionDesignator};

use crate::card_registry;
use crate::engine::dispatch::actions::{
    draw, engage, evade, fight, investigate, move_action, resource,
};
use crate::engine::dispatch::{abilities, cards, combat};
use crate::engine::outcome::EngineOutcome;
use crate::engine::{evaluator, Cx};
use crate::event::Event;
use crate::state::{
    AbilitySource, ActionResolutionFrame, ActionResume, CardCode, CardInstanceId, GameState,
    InvestigatorId, Phase, Status,
};

/// What kind of action is being taken: one of the seven basic actions
/// (`glossary/Action.md`, FAQ 1.30: *"The following are basic actions:
/// **Draw**, **Resource**, **Move**, **Investigate**, **Fight**, **Engage**,
/// and **Evade**."*), a **Play** of the card `code`, or an **Activate** of an
/// ability on `source`. The last two carry their source so a rule that applies
/// to some actions by what they are taken from can be tested against the
/// description (Daisy Walker 01002, #779).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ActionKind {
    Draw,
    Resource,
    Move,
    Investigate,
    Fight,
    Engage,
    Evade,
    Play { code: CardCode },
    Activate { source: AbilitySource },
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
            Self::Activate { source } => write!(f, "Activate {source:?}"),
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
}

impl ActionDescription {
    /// A basic action, which costs one action and prints no designator.
    pub(crate) fn basic(kind: ActionKind) -> Self {
        Self {
            kind,
            action_cost: 1,
            designator: None,
        }
    }

    /// A non-fast play of the card `code`, which costs one action. An event
    /// carries no designator until #778 gives it one; Backstab 01051's
    /// **Fight** then makes its play exempt with no change here.
    pub(crate) fn play(code: CardCode) -> Self {
        Self {
            kind: ActionKind::Play { code },
            action_cost: 1,
            designator: None,
        }
    }

    /// An activation of an ability on `source` printing `action_cost` actions
    /// (at least one; a fast ability is not taken) and `designator`.
    pub(crate) fn activate(
        source: AbilitySource,
        action_cost: u8,
        designator: Option<ActionDesignator>,
    ) -> Self {
        Self {
            kind: ActionKind::Activate { source },
            action_cost,
            designator,
        }
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
                    | ActionDesignator::Evade
                    | ActionDesignator::Parley
                    | ActionDesignator::Resign
            )
        );
        self.action_cost > 0 && !exempt_kind && !exempt_designator
    }
}

/// What taking an action costs: the total actions, and the surcharge sources to
/// mark spent once they are paid.
struct Quote {
    total: u8,
    surcharge_sources: Vec<CardInstanceId>,
}

/// Whether `investigator` may take the action `description` describes, and if so
/// how many actions it costs: its printed cost plus any surcharge (Frozen in
/// Fear 01164). Pure, so the turn menu can price an action without taking it.
///
/// Checks, in order: the investigator exists, is Active, and it is the
/// Investigation phase and their turn; then that they can afford the total. An
/// unknown investigator is refused, never a panic, since the id can arrive in a
/// client message.
pub(crate) fn check(
    state: &GameState,
    investigator: InvestigatorId,
    description: &ActionDescription,
) -> Result<u8, Cow<'static, str>> {
    quote(state, investigator, description).map(|quote| quote.total)
}

fn quote(
    state: &GameState,
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
    let (extra, surcharge_sources) = match description.action_class() {
        Some(class) => action_surcharge(state, investigator, class),
        None => (0, Vec::new()),
    };
    let total = description.action_cost.saturating_add(extra);
    if inv.actions_remaining < total {
        return Err(format!("{name} requires {total} action point(s)").into());
    }
    Ok(Quote {
        total,
        surcharge_sources,
    })
}

/// Take the action `description` describes for `investigator`: pay its actions,
/// mark the surcharge sources it consumed, run `before_attacks`, then either
/// park the action behind its attacks of opportunity or perform it now.
///
/// `before_attacks` pays the caller's own costs and returns the rest of the
/// action as an [`ActionResume`]. A basic action has no other cost, so its hook
/// just returns its resume. A play's pays the resource cost and commences the
/// play; an activation's pays the ability's other costs and announces it. If the action provokes, the resume is parked on an
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
        total,
        surcharge_sources,
    } = match quote(cx.state, investigator, description) {
        Ok(quote) => quote,
        Err(reason) => return EngineOutcome::Rejected { reason },
    };
    let inv = cx
        .state
        .investigators
        .get_mut(&investigator)
        .expect("quote checked the investigator exists");
    let new_count = inv.actions_remaining - total;
    inv.actions_remaining = new_count;
    // The surcharge is spent, so its `first_each_round` sources are done for the
    // round: Frozen in Fear 01164's *"The first time you perform one of the
    // following actions (move, fight, or evade) each round"*.
    inv.action_surcharge_spent_this_round
        .extend(surcharge_sources);
    cx.events.push(Event::ActionsRemainingChanged {
        investigator,
        new_count,
    });

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
        ActionResume::Move { destination } => {
            move_action::move_primary_effect(cx, investigator, destination)
        }
        ActionResume::Investigate => investigate::investigate_primary_effect(cx, investigator),
        ActionResume::Resource => resource::resource_primary_effect(cx, investigator),
        ActionResume::Engage { enemy } => engage::engage_primary_effect(cx, investigator, enemy),
        ActionResume::Draw => draw::draw_primary_effect(cx, investigator),
        // A basic attack carries no modification. A designated Fight (every
        // corpus weapon) reaches the same primary with its combat bonus and its
        // bonus damage.
        ActionResume::Fight { enemy } => {
            fight::perform_fight(cx, investigator, enemy, None, 0, None)
        }
        ActionResume::Evade { enemy } => evade::perform_evade(cx, investigator, enemy),
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

/// The `ExtraActionCost` surcharge on `action_class` for `investigator`, plus
/// the `first_each_round` sources to mark spent once the action is paid.
///
/// The registry-optional wrapper around
/// [`pending_action_surcharge`](crate::engine::evaluator::pending_action_surcharge):
/// no registry, or no card data for a code, means no surcharge. Pure, so a
/// caller can price an action before deciding to pay for it.
///
/// [`check`] and [`take`] read it for every action they price, an activation
/// included, whose bold designator names the class via
/// [`ActionDesignator::action_class`] (#754). Nothing outside the step prices a
/// surcharge: the play and activation validators ask [`check`]. One pricing is
/// the point: a surcharge only one path applies is the bug that made shooting a
/// weapon cheaper than punching.
fn action_surcharge(
    state: &GameState,
    investigator: InvestigatorId,
    action_class: ActionClass,
) -> (u8, Vec<CardInstanceId>) {
    match card_registry::current() {
        Some(reg) => evaluator::pending_action_surcharge(state, reg, investigator, action_class),
        None => (0, Vec::new()),
    }
}
