//! The legal-action enumerator (slice 2a-ii, #393): the legal open-turn
//! actions for the active investigator. Read-only; routing is via `ResolveInput`
//! (2b) — this module shares the handlers' legality predicates so the
//! enumeration matches handler-acceptance by construction.

use crate::card_registry;
use crate::engine::dispatch::actions::take::{self, ActionDescription, ActionKind};
use crate::engine::dispatch::{act_agenda, movement, reaction_windows};
use crate::engine::outcome::OptionTarget;
use crate::engine::{abilities_in_effect, ability_source};
use crate::state::{
    AbilityAddress, AbilitySource, EnemyId, GameState, InvestigatorId, InvestigatorTurnFrame,
    LocationId,
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
    /// Play a card from the investigator's hand.
    PlayCard {
        /// Investigator playing the card.
        investigator: InvestigatorId,
        /// Zero-based position in the investigator's hand.
        hand_index: u8,
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

impl TurnAction {
    /// Plain human-readable menu label. Rich/structured rendering is #205.
    #[must_use]
    pub fn label(&self, state: &GameState) -> String {
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
                hand_index,
            } => {
                let code = state
                    .investigators
                    .get(investigator)
                    .and_then(|inv| inv.hand.get(*hand_index as usize))
                    .map_or_else(|| format!("card {hand_index}"), ToString::to_string);
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
                hand_index,
            } => OptionTarget::HandCard {
                investigator: *investigator,
                hand_index: *hand_index,
            },
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
pub fn legal_actions(state: &GameState) -> Vec<TurnAction> {
    let Some(investigator) = active_investigator(state) else {
        return Vec::new();
    };
    let mut actions = Vec::new();
    push_basic_actions(state, investigator, &mut actions);
    push_combat_engage_actions(state, investigator, &mut actions);
    push_card_actions(state, investigator, &mut actions);
    push_act_actions(state, investigator, &mut actions);
    actions
}

/// Append the `AdvanceAct` action if legal (slice 2a-ii-4, #393) — delegated to
/// `check_advance_act`, registry-free (act decks are scenario state, not card
/// data).
fn push_act_actions(state: &GameState, investigator: InvestigatorId, out: &mut Vec<TurnAction>) {
    if act_agenda::check_advance_act(state, investigator).is_ok() {
        out.push(TurnAction::AdvanceAct { investigator });
    }
}

/// Append the card actions legal for `investigator` — `PlayCard` and (Task 2)
/// `ActivateAbility` (slice 2a-ii-3, #393). Both need card data, so they yield
/// nothing without a registry (matching the handlers, which reject on `None`).
/// Fidelity is by delegation: the enumerator calls the same `check_play_card` /
/// `check_activate_ability` the handlers call.
fn push_card_actions(state: &GameState, investigator: InvestigatorId, out: &mut Vec<TurnAction>) {
    if card_registry::current().is_none() {
        return;
    }
    let Some(inv) = state.investigators.get(&investigator) else {
        return;
    };

    // PlayCard: one option per hand card the handler would accept —
    // `check_play_card` is the whole predicate, constant play-bans included
    // (Dissonant Voices 01165).
    let hand_len = inv.hand.len();
    for idx in 0..hand_len {
        let hand_index = u8::try_from(idx).unwrap_or(u8::MAX);
        if reaction_windows::check_play_card(state, investigator, hand_index).is_ok() {
            out.push(TurnAction::PlayCard {
                investigator,
                hand_index,
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
    for (source, code) in ability_source::reachable_source_codes(state, investigator) {
        let abilities = abilities_in_effect::for_source(state, source, &code).unwrap_or_default();
        for (address, _) in abilities {
            if reaction_windows::check_activate_ability(state, investigator, source, &address)
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

/// Append the combat / engage actions legal for `investigator`, mirroring the
/// `fight`/`evade`/`engage` handlers (slice 2a-ii-2, #393). The three target
/// distinct, overlapping enemy sets:
/// - **Fight**: any enemy at the investigator's location, engaged or not (RR
///   p.12, #401 — co-location, like Engage).
/// - **Evade**: only an enemy engaged with the investigator (RR p.11).
/// - **Engage**: a co-located enemy not already engaged with the investigator
///   (including one engaged with another investigator; RR p.11).
fn push_combat_engage_actions(
    state: &GameState,
    investigator: InvestigatorId,
    out: &mut Vec<TurnAction>,
) {
    // Whether each action may be taken at all — phase, turn, Status, and its
    // cost, surcharge included — is the step's question, asked per kind since
    // Frozen in Fear prices Fight and Evade but not Engage.
    let fight_takeable = may_take(state, investigator, ActionKind::Fight);
    let evade_takeable = may_take(state, investigator, ActionKind::Evade);
    let engage_takeable = may_take(state, investigator, ActionKind::Engage);
    let Some(inv) = state.investigators.get(&investigator) else {
        return;
    };
    let inv_location = inv.current_location;

    // One pass over the enemies; the three actions' conditions are independent
    // and can overlap (a co-located engaged enemy is both a Fight and an Evade
    // target; a co-located unengaged enemy is both a Fight and an Engage target).
    // The `inv_location.is_some()` guard avoids a `None == None` co-location match
    // when both are locationless (mirrors the fight/engage handlers' guard).
    for (&enemy_id, enemy) in &state.enemies {
        let co_located = inv_location.is_some() && enemy.current_location == inv_location;
        let engaged_with_me = enemy.engaged_with == Some(investigator);

        // Fight: any co-located enemy, non-negative difficulty, affordable.
        if co_located && fight_takeable && enemy.fight >= 0 {
            out.push(TurnAction::Fight {
                investigator,
                enemy: enemy_id,
            });
        }
        // Evade: only an enemy engaged with the investigator.
        if engaged_with_me && evade_takeable && enemy.evade >= 0 {
            out.push(TurnAction::Evade {
                investigator,
                enemy: enemy_id,
            });
        }
        // Engage: a co-located enemy not already engaged with the investigator.
        if co_located && !engaged_with_me && engage_takeable {
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
fn push_basic_actions(state: &GameState, investigator: InvestigatorId, out: &mut Vec<TurnAction>) {
    // EndTurn: always legal at the open turn (no action point required).
    out.push(TurnAction::EndTurn);

    // Whether each may be taken at all — phase, turn, Status, and its cost,
    // surcharge included — is the step's question. Frozen in Fear prices Move
    // but none of the other three.
    if may_take(state, investigator, ActionKind::Resource) {
        out.push(TurnAction::Resource { investigator });
    }
    if may_take(state, investigator, ActionKind::Draw) {
        out.push(TurnAction::Draw { investigator });
    }
    let Some(inv) = state.investigators.get(&investigator) else {
        return;
    };
    if may_take(state, investigator, ActionKind::Investigate) {
        if let Some(loc_id) = inv.current_location {
            if state.locations.get(&loc_id).is_some_and(|l| l.revealed) {
                out.push(TurnAction::Investigate { investigator });
            }
        }
    }

    // Move: one option per connected destination in state.
    if !may_take(state, investigator, ActionKind::Move) {
        return;
    }
    let Some(from) = inv.current_location else {
        return;
    };
    let Some(from_loc) = state.locations.get(&from) else {
        return;
    };
    for &dest in &from_loc.connections {
        // The barrier filter is applied to the *step*, never to the graph
        // (#651/#774): a blocked destination is simply not offered, and the
        // connection itself stays on the map for everything that measures
        // distance across it.
        if dest != from
            && state.locations.contains_key(&dest)
            && movement::investigator_can_enter_location(state, dest)
        {
            out.push(TurnAction::Move {
                investigator,
                destination: dest,
            });
        }
    }
}

/// Whether `investigator` may take the basic action `kind` now, cost included —
/// the step's own [`take::check`], so the menu offers exactly
/// what taking the action accepts.
fn may_take(state: &GameState, investigator: InvestigatorId, kind: ActionKind) -> bool {
    take::check(state, investigator, &ActionDescription::basic(kind)).is_ok()
}

#[cfg(test)]
mod tests;
