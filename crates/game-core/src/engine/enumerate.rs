//! The legal-action enumerator (slice 2a-ii, #393): the legal open-turn
//! actions for the active investigator. Read-only; routing is via `ResolveInput`
//! (2b) — this module shares the handlers' legality predicates so the
//! enumeration matches handler-acceptance by construction.

use std::fmt;

use crate::card_registry::CardRegistry;
use crate::engine::dispatch::actions::take::{self, ActionDescription, ActionKind};
use crate::engine::dispatch::actions::{engage, evade, fight, investigate, r#move};
use crate::engine::dispatch::{act_agenda, legality};
use crate::engine::outcome::OptionTarget;
use crate::engine::{abilities_in_effect, ability_source, board};
use crate::state::{
    AbilityAddress, AbilitySource, CardCode, CardInstanceId, EnemyId, GameState, Investigator,
    InvestigatorId, InvestigatorTurnFrame, LocationId,
};

/// The enumerated open-turn actions for the active investigator.
///
/// Each variant mirrors an identically-named [`crate::action::PlayerAction`]
/// gameplay arm, with the same field names and types. No `serde` — these are
/// internal only and never cross the wire; the wire surface stays
/// `PlayerAction::ResolveInput(PickSingle(OptionId))`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnAction {
    /// Active investigator ends their turn.
    EndTurn,
    /// Move the active investigator to a connected location.
    Move {
        /// Investigator performing the move.
        investigator: InvestigatorId,
        /// Destination location.
        destination: LocationId,
    },
    /// Investigate at the investigator's current location.
    Investigate {
        /// Investigator performing the action.
        investigator: InvestigatorId,
    },
    /// Gain 1 resource (the basic "Resource" action).
    Resource {
        /// Investigator taking the action.
        investigator: InvestigatorId,
    },
    /// Draw a card from the player deck.
    Draw {
        /// Investigator drawing.
        investigator: InvestigatorId,
    },
    /// Engage an enemy with a combat skill test.
    Fight {
        /// Investigator performing the Fight action.
        investigator: InvestigatorId,
        /// The enemy to fight.
        enemy: EnemyId,
    },
    /// Evade an engaged enemy with an agility skill test.
    Evade {
        /// Investigator performing the Evade action.
        investigator: InvestigatorId,
        /// The enemy to evade.
        enemy: EnemyId,
    },
    /// Engage a co-located enemy not already engaged with the investigator.
    Engage {
        /// Investigator performing the action.
        investigator: InvestigatorId,
        /// The enemy to engage.
        enemy: EnemyId,
    },
    /// Play a card from one of the investigator's zones.
    PlayCard {
        /// Investigator playing the card.
        investigator: InvestigatorId,
        /// The zone the card is played from, and where in it.
        source: PlaySource,
    },
    /// Activate a `Trigger::Activated` ability on an ability source the
    /// investigator can reach (#707).
    ActivateAbility {
        /// Investigator activating the ability.
        investigator: InvestigatorId,
        /// The ability source — what carries the ability, addressed
        /// independently of which collection holds it. Reachability is
        /// `engine::ability_source`'s answer, not the descriptor's.
        source: AbilitySource,
        /// Which ability on that source — named by where it is printed
        /// (#772), so the menu entry and the handler agree about the same
        /// ability even when a grant lands or lapses in between.
        address: AbilityAddress,
    },
    /// Spend clues to advance the current act.
    AdvanceAct {
        /// The investigator initiating the spend.
        investigator: InvestigatorId,
    },
}

/// Where a [`TurnAction::PlayCard`] takes its card from: the zone the card
/// leaves when it "commences being played" (RR Appendix I step 3), and its
/// position there.
///
/// This names a **zone**, not an [`AbilitySource`]: a card being played is
/// not a source of triggered abilities (see **Ability source** in
/// `GLOSSARY.md`). Whether the source may be played from at all is
/// `legality::play_source_available`'s answer, which the turn menu, the Fast
/// window and the reaction scan all ask.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaySource {
    /// Zero-based position in the investigator's hand.
    Hand(u8),
    /// The topmost card of the investigator's discard pile, under the play
    /// permission on the card instance `permission`: Wendy's Amulet 01014,
    /// *"You may play the topmost event in your discard pile as if it were in
    /// your hand."* The permission is named so the option can anchor on the
    /// card that grants it — the client renders no discard pile (#1101).
    TopOfDiscardPile {
        /// The in-play card whose permission the play is made under.
        permission: CardInstanceId,
    },
}

impl PlaySource {
    /// The card at this source in `investigator`'s zones, or `None` when the
    /// position is out of bounds or the discard pile is empty. Positional
    /// only: whether the permission still holds is not asked here.
    #[must_use]
    pub fn card(self, investigator: &Investigator) -> Option<&CardCode> {
        match self {
            PlaySource::Hand(index) => investigator.hand.get(usize::from(index)),
            PlaySource::TopOfDiscardPile { .. } => investigator.discard.last(),
        }
    }
}

impl fmt::Display for PlaySource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PlaySource::Hand(index) => write!(f, "hand index {index}"),
            PlaySource::TopOfDiscardPile { permission } => {
                write!(f, "the top of the discard pile (under {permission:?})")
            }
        }
    }
}

impl TurnAction {
    /// Plain human-readable menu label. Rich/structured rendering is #205.
    ///
    /// A play from the discard pile names the event and the card whose
    /// permission it is played under — *"Play Emergency Cache (Wendy's
    /// Amulet)"* — because it is anchored on that card, not on the event
    /// (#1112); `cards` supplies both names.
    #[must_use]
    pub fn label(&self, state: &GameState, cards: &CardRegistry) -> String {
        let loc_name = |id: LocationId| {
            state
                .locations
                .get(&id)
                .map_or_else(|| format!("loc {}", id.0), |l| l.name.clone())
        };
        let enemy_name = |id: EnemyId| {
            state
                .enemies
                .get(&id)
                .map_or_else(|| format!("enemy {}", id.0), |e| e.name.clone())
        };
        match self {
            TurnAction::EndTurn => "End turn".into(),
            TurnAction::Move { destination, .. } => format!("Move to {}", loc_name(*destination)),
            TurnAction::Investigate { .. } => "Investigate".into(),
            TurnAction::Resource { .. } => "Gain resource".into(),
            TurnAction::Draw { .. } => "Draw".into(),
            TurnAction::Fight { enemy, .. } => format!("Fight {}", enemy_name(*enemy)),
            TurnAction::Evade { enemy, .. } => format!("Evade {}", enemy_name(*enemy)),
            TurnAction::Engage { enemy, .. } => format!("Engage {}", enemy_name(*enemy)),
            TurnAction::PlayCard {
                investigator,
                source: source @ PlaySource::TopOfDiscardPile { permission },
            } => {
                let event = state
                    .investigators
                    .get(investigator)
                    .and_then(|inv| source.card(inv));
                play_under_permission_label(state, cards, event, *permission)
            }
            TurnAction::PlayCard {
                investigator,
                source,
            } => {
                let code = state
                    .investigators
                    .get(investigator)
                    .and_then(|inv| source.card(inv))
                    .map_or_else(|| format!("card at {source}"), ToString::to_string);
                format!("Play {code}")
            }
            // Structured / rich rendering is #205; until then a granted
            // ability says so rather than printing an index that belongs to a
            // different card.
            TurnAction::ActivateAbility { address, .. } => match address {
                AbilityAddress::Printed(index) => format!("Activate ability {index}"),
                AbilityAddress::Granted { granter, .. } => {
                    format!("Activate ability granted by {granter}")
                }
            },
            TurnAction::AdvanceAct { .. } => "Advance act".into(),
        }
    }

    /// The board surface this action anchors to, for host rendering (#535).
    /// Mirrors [`label`](Self::label): it takes `state` because some actions'
    /// anchors are implicit — Investigate acts at the investigator's current
    /// location, which is not a field on the variant.
    #[must_use]
    pub fn target(&self, state: &GameState) -> Option<OptionTarget> {
        Some(match self {
            // The three open-turn "global" actions are not un-anchored — they are
            // anchored to surfaces the board renders per investigator (ADR 0011).
            TurnAction::EndTurn => OptionTarget::TurnControl(active_investigator(state)?),
            TurnAction::Resource { investigator } => OptionTarget::ResourcePool(*investigator),
            TurnAction::Draw { investigator } => OptionTarget::PlayerDeck(*investigator),
            TurnAction::Move { destination, .. } => OptionTarget::Location(*destination),
            TurnAction::Investigate { investigator } => state
                .investigators
                .get(investigator)
                .and_then(|inv| inv.current_location)
                .map(OptionTarget::Location)?,
            TurnAction::Fight { enemy, .. }
            | TurnAction::Evade { enemy, .. }
            | TurnAction::Engage { enemy, .. } => OptionTarget::Enemy(*enemy),
            TurnAction::PlayCard {
                investigator,
                source: PlaySource::Hand(hand_index),
            } => OptionTarget::HandCard {
                investigator: *investigator,
                hand_index: *hand_index,
            },
            // On the permission's card: the client renders no discard pile
            // (#1101), and ADR 0011 needs a surface it does render.
            TurnAction::PlayCard {
                source: PlaySource::TopOfDiscardPile { permission },
                ..
            } => OptionTarget::CardInstance(*permission),
            // One map from a source to an anchor, shared with the forced /
            // reaction candidates' `candidate_anchor` (#735): the two carried a
            // copy each and the copies had drifted. The exhaustive `match` that
            // used to live here — a new `AbilitySource` kind should stop the
            // build rather than quietly anchor itself somewhere wrong — lives in
            // the `From` impl now.
            TurnAction::ActivateAbility { source, .. } => (*source).into(),
            TurnAction::AdvanceAct { .. } => OptionTarget::Act,
        })
    }
}

/// The label of a play made under the permission on the in-play card
/// `permission`, naming the card played and the permission's card — *"Play
/// Emergency Cache (Wendy's Amulet)"*. Shared by the turn menu and the
/// reaction window, the two places such a play is offered. A code the registry
/// does not know is printed as the code.
pub(crate) fn play_under_permission_label(
    state: &GameState,
    cards: &CardRegistry,
    card: Option<&CardCode>,
    permission: CardInstanceId,
) -> String {
    let name = |code: Option<&CardCode>| {
        code.map_or_else(
            || "?".to_owned(),
            |code| {
                (cards.metadata_for)(code)
                    .map_or_else(|| code.to_string(), |meta| meta.name.clone())
            },
        )
    };
    let granter = board::find_instance(state, permission).map(|(granter, _)| &granter.code);
    format!("Play {} ({})", name(card), name(granter))
}

/// The investigator whose turn is open, or `None` when no
/// [`InvestigatorTurn`](crate::state::Continuation::InvestigatorTurn) frame is on top —
/// [`TurnAction::EndTurn`] carries no investigator field, so its `TurnControl`
/// anchor has to come from the frame.
fn active_investigator(state: &GameState) -> Option<InvestigatorId> {
    state
        .continuations
        .top_of::<InvestigatorTurnFrame>()
        .map(|turn| turn.investigator)
}

/// The legal [`TurnAction`]s the active investigator may take at the open
/// turn, in stable order (position = the `OptionId` accepted by
/// `ResolveInput(PickSingle(OptionId))`). Empty unless an
/// [`InvestigatorTurn`](crate::state::Continuation::InvestigatorTurn) frame is on top — the
/// only point gameplay actions are taken (slice 2a-ii, #393).
///
/// Covers the full open-turn surface: `EndTurn`, `Resource`, `Draw`,
/// `Investigate`, `Move` (basic); `Fight`, `Evade`, `Engage` (combat/engage);
/// `PlayCard`, `ActivateAbility` (cards, registry-gated); `AdvanceAct`.
/// Read-only and side-effect-free; each action is included iff the same legality
/// predicate the handler uses accepts it, so the enumeration matches
/// handler-acceptance by construction (routing via `OptionId` is 2b).
#[must_use]
pub fn legal_actions(state: &GameState, cards: &CardRegistry) -> Vec<TurnAction> {
    let Some(investigator) = active_investigator(state) else {
        return Vec::new();
    };
    let mut actions = Vec::new();
    push_basic_actions(state, cards, investigator, &mut actions);
    push_combat_engage_actions(state, cards, investigator, &mut actions);
    push_card_actions(state, cards, investigator, &mut actions);
    push_act_actions(state, cards, investigator, &mut actions);
    actions
}

/// Append the `AdvanceAct` action if legal (slice 2a-ii-4, #393) — delegated to
/// `check_advance_act`, registry-free (act decks are scenario state, not card
/// data).
fn push_act_actions(
    state: &GameState,
    cards: &CardRegistry,
    investigator: InvestigatorId,
    out: &mut Vec<TurnAction>,
) {
    if act_agenda::check_advance_act(state, cards, investigator).is_ok() {
        out.push(TurnAction::AdvanceAct { investigator });
    }
}

/// Append the card actions legal for `investigator` — `PlayCard` and (Task 2)
/// `ActivateAbility` (slice 2a-ii-3, #393).
/// Fidelity is by delegation: the enumerator calls the same `check_play_card` /
/// `check_activate_ability` the handlers call.
fn push_card_actions(
    state: &GameState,
    cards: &CardRegistry,
    investigator: InvestigatorId,
    out: &mut Vec<TurnAction>,
) {
    // PlayCard: one option per play source the handler would accept — each
    // hand card, and the top of the discard pile under a permission (Wendy's
    // Amulet 01014). `check_play_card` is the whole predicate, constant
    // play-bans included (Dissonant Voices 01165).
    for source in legality::play_sources(state, cards, investigator) {
        if legality::check_play_card(state, cards, investigator, source).is_ok() {
            out.push(TurnAction::PlayCard {
                investigator,
                source,
            });
        }
    }

    // ActivateAbility: one option per activatable ability on each ability source
    // the investigator can reach (#707) — the same predicate the validator
    // consults, so the menu and handler-acceptance cannot disagree about which
    // sources exist. The list is the card's abilities *in effect*: printed plus
    // granted (#772), each carrying the address that names it;
    // `check_activate_ability` filters to the activated, payable,
    // window-eligible ones (so a non-`Activated` ability is simply not offered).
    for (source, code) in ability_source::reachable_source_codes(state, cards, investigator) {
        let abilities =
            abilities_in_effect::for_source(state, cards, source, &code).unwrap_or_default();
        for (address, _) in abilities {
            if legality::check_activate_ability(state, cards, investigator, source, &address)
                .is_ok()
            {
                out.push(TurnAction::ActivateAbility {
                    investigator,
                    source,
                    address,
                });
            }
        }
    }
}

/// Append the combat / engage actions legal for `investigator` (slice 2a-ii-2,
/// #393): one per enemy in each action's own `candidates` — the same lists the
/// `fight`/`evade`/`engage` handlers validate against — less an enemy whose
/// fight or evade value is malformed.
fn push_combat_engage_actions(
    state: &GameState,
    cards: &CardRegistry,
    investigator: InvestigatorId,
    out: &mut Vec<TurnAction>,
) {
    // Whether each action may be taken at all — phase, turn, Status, and its
    // cost, surcharge included — is the step's question, asked per kind since
    // Frozen in Fear prices Fight and Evade but not Engage. An action that
    // can't be taken has no targets.
    let targets = |kind: ActionKind, candidates: fn(&GameState, InvestigatorId) -> Vec<EnemyId>| {
        if may_take(state, cards, investigator, kind) {
            candidates(state, investigator)
        } else {
            Vec::new()
        }
    };
    let fight_targets = targets(ActionKind::Fight, fight::candidates);
    let evade_targets = targets(ActionKind::Evade, evade::candidates);
    let engage_targets = targets(ActionKind::Engage, engage::candidates);

    // One pass over the enemies, so the menu keeps the per-enemy interleave
    // (Fight, Evade, Engage for each enemy in id order) the web client renders.
    // The three sets can overlap: a co-located engaged enemy is both a Fight
    // and an Evade target; a co-located unengaged enemy is both a Fight and an
    // Engage target.
    for (&enemy_id, enemy) in &state.enemies {
        if fight_targets.contains(&enemy_id) && !fight::has_malformed_value(enemy) {
            out.push(TurnAction::Fight {
                investigator,
                enemy: enemy_id,
            });
        }
        if evade_targets.contains(&enemy_id) && !evade::has_malformed_value(enemy) {
            out.push(TurnAction::Evade {
                investigator,
                enemy: enemy_id,
            });
        }
        if engage_targets.contains(&enemy_id) {
            out.push(TurnAction::Engage {
                investigator,
                enemy: enemy_id,
            });
        }
    }
}

/// Append the basic actions legal for `investigator`. `EndTurn` is always legal
/// at the open turn (the handler only needs an active investigator, guaranteed
/// here). Resource, Draw, Investigate and Move follow, each offered only if the
/// investigator may take it.
fn push_basic_actions(
    state: &GameState,
    cards: &CardRegistry,
    investigator: InvestigatorId,
    out: &mut Vec<TurnAction>,
) {
    // EndTurn: always legal at the open turn (no action point required).
    out.push(TurnAction::EndTurn);

    // Whether each may be taken at all — phase, turn, Status, and its cost,
    // surcharge included — is the step's question. Frozen in Fear prices Move
    // but none of the other three.
    if may_take(state, cards, investigator, ActionKind::Resource) {
        out.push(TurnAction::Resource { investigator });
    }
    if may_take(state, cards, investigator, ActionKind::Draw) {
        out.push(TurnAction::Draw { investigator });
    }
    if may_take(state, cards, investigator, ActionKind::Investigate)
        && investigate::candidates(state, investigator).is_some()
    {
        out.push(TurnAction::Investigate { investigator });
    }

    // Move: one option per destination, in connection order.
    if may_take(state, cards, investigator, ActionKind::Move) {
        for destination in r#move::candidates(state, cards, investigator) {
            out.push(TurnAction::Move {
                investigator,
                destination,
            });
        }
    }
}

/// Whether `investigator` may take the basic action `kind` now, cost included —
/// the step's own [`take::check`], so the menu offers exactly
/// what taking the action accepts.
fn may_take(
    state: &GameState,
    cards: &CardRegistry,
    investigator: InvestigatorId,
    kind: ActionKind,
) -> bool {
    take::check(state, cards, investigator, &ActionDescription::basic(kind)).is_ok()
}

#[cfg(test)]
mod tests;
