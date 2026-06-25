//! The legal-action enumerator (slice 2a-ii, #393): the legal open-turn
//! actions for the active investigator. Read-only; routing is via `ResolveInput`
//! (2b) — this module shares the handlers' legality predicates so the
//! enumeration matches handler-acceptance by construction.

use crate::state::{CardInstanceId, Continuation, EnemyId, GameState, InvestigatorId, LocationId};

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
    /// Activate a `Trigger::Activated` ability on a specific in-play card instance.
    ActivateAbility {
        /// Investigator activating the ability.
        investigator: InvestigatorId,
        /// Which copy of the in-play card is the source.
        instance_id: CardInstanceId,
        /// Zero-based index into the card's abilities vec.
        ability_index: u8,
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
                    .map_or_else(
                        || format!("card {hand_index}"),
                        std::string::ToString::to_string,
                    );
                format!("Play {code}")
            }
            TurnAction::ActivateAbility { ability_index, .. } => {
                format!("Activate ability {ability_index}")
            }
            TurnAction::AdvanceAct { .. } => "Advance act".into(),
        }
    }
}

/// The legal [`TurnAction`]s the active investigator may take at the open
/// turn, in stable order (position = the `OptionId` accepted by
/// `ResolveInput(PickSingle(OptionId))`). Empty unless an
/// [`InvestigatorTurn`](Continuation::InvestigatorTurn) frame is on top — the
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
    let Some(Continuation::InvestigatorTurn { investigator, .. }) = state.continuations.last()
    else {
        return Vec::new();
    };
    let investigator = *investigator;
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
    if crate::engine::dispatch::act_agenda::check_advance_act(state, investigator).is_ok() {
        out.push(TurnAction::AdvanceAct { investigator });
    }
}

/// Append the card actions legal for `investigator` — `PlayCard` and (Task 2)
/// `ActivateAbility` (slice 2a-ii-3, #393). Both need card data, so they yield
/// nothing without a registry (matching the handlers, which reject on `None`).
/// Fidelity is by delegation: the enumerator calls the same `check_play_card` /
/// `check_activate_ability` the handlers call, plus the `PlayCard` handler's
/// inline `play_is_prohibited` guard.
fn push_card_actions(state: &GameState, investigator: InvestigatorId, out: &mut Vec<TurnAction>) {
    let Some(reg) = crate::card_registry::current() else {
        return;
    };
    let Some(inv) = state.investigators.get(&investigator) else {
        return;
    };

    // PlayCard: one option per hand card the handler would accept — playable
    // (`check_play_card`) and not forbidden by a constant restriction
    // (`play_is_prohibited`, e.g. Dissonant Voices 01165).
    let hand_len = inv.hand.len();
    for idx in 0..hand_len {
        let hand_index = u8::try_from(idx).unwrap_or(u8::MAX);
        if let Ok(check) = crate::engine::dispatch::reaction_windows::check_play_card(
            state,
            investigator,
            hand_index,
        ) {
            if !crate::engine::evaluator::play_is_prohibited(
                state,
                reg,
                investigator,
                check.card_type,
            ) {
                out.push(TurnAction::PlayCard {
                    investigator,
                    hand_index,
                });
            }
        }
    }

    // ActivateAbility: one option per activatable ability on each in-play card.
    // `ability_index` indexes the card's full ability list; `check_activate_ability`
    // filters to the activated, payable, window-eligible ones (so non-`Activated`
    // indices are simply not offered).
    for card in &inv.cards_in_play {
        let ability_count = (reg.abilities_for)(&card.code).map_or(0, |a| a.len());
        for idx in 0..ability_count {
            let ability_index = u8::try_from(idx).unwrap_or(u8::MAX);
            if crate::engine::dispatch::reaction_windows::check_activate_ability(
                state,
                investigator,
                card.instance_id,
                ability_index,
            )
            .is_ok()
            {
                out.push(TurnAction::ActivateAbility {
                    investigator,
                    instance_id: card.instance_id,
                    ability_index,
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
    use crate::engine::dispatch::actions::{action_cost, validate_basic_action};

    // The shared basic-action prologue gates Fight/Evade/Engage alike; if it
    // fails (wrong phase / not active / no action), none are legal.
    let Ok(inv) = validate_basic_action(state, "enumerate", investigator) else {
        return;
    };
    let actions_remaining = inv.actions_remaining;
    let fight_affordable =
        action_cost(state, investigator, crate::dsl::ActionClass::Fight) <= actions_remaining;
    let evade_affordable =
        action_cost(state, investigator, crate::dsl::ActionClass::Evade) <= actions_remaining;
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
        if co_located && fight_affordable && enemy.fight >= 0 {
            out.push(TurnAction::Fight {
                investigator,
                enemy: enemy_id,
            });
        }
        // Evade: only an enemy engaged with the investigator.
        if engaged_with_me && evade_affordable && enemy.evade >= 0 {
            out.push(TurnAction::Evade {
                investigator,
                enemy: enemy_id,
            });
        }
        // Engage: a co-located enemy not already engaged with the investigator.
        if co_located && !engaged_with_me {
            out.push(TurnAction::Engage {
                investigator,
                enemy: enemy_id,
            });
        }
    }
}

/// Append the basic actions legal for `investigator`. `EndTurn` is always legal
/// at the open turn (the handler only needs an active investigator, guaranteed
/// here). Later tasks add Resource/Draw/Investigate/Move.
fn push_basic_actions(state: &GameState, investigator: InvestigatorId, out: &mut Vec<TurnAction>) {
    use crate::engine::dispatch::actions::{action_cost, validate_basic_action};

    // EndTurn: always legal at the open turn (no action point required).
    out.push(TurnAction::EndTurn);

    // Resource / Draw / Investigate share the basic-action prologue (phase +
    // active + Status::Active + actions_remaining >= 1). Investigate adds a
    // revealed-current-location gate.
    if let Ok(inv) = validate_basic_action(state, "enumerate", investigator) {
        out.push(TurnAction::Resource { investigator });
        out.push(TurnAction::Draw { investigator });
        if let Some(loc_id) = inv.current_location {
            if state.locations.get(&loc_id).is_some_and(|l| l.revealed) {
                out.push(TurnAction::Investigate { investigator });
            }
        }
    }

    // Move uses its own prefix (the action-point check folds into the cost):
    // phase Investigation + active + Status::Active + a current location +
    // affordable, with one option per connected destination in state.
    let Some(inv) = state.investigators.get(&investigator) else {
        return;
    };
    if state.phase != crate::state::Phase::Investigation
        || state.active_investigator != Some(investigator)
        || inv.status != crate::state::Status::Active
    {
        return;
    }
    let Some(from) = inv.current_location else {
        return;
    };
    if action_cost(state, investigator, crate::dsl::ActionClass::Move) > inv.actions_remaining {
        return;
    }
    let Some(from_loc) = state.locations.get(&from) else {
        return;
    };
    for &dest in &from_loc.connections {
        if dest != from && state.locations.contains_key(&dest) {
            out.push(TurnAction::Move {
                investigator,
                destination: dest,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::engine::enumerate::{legal_actions, TurnAction};
    use crate::state::{Continuation, InvestigationResume, InvestigatorId, Phase};
    use crate::test_support::{test_investigator, GameStateBuilder};

    /// Build a single-investigator open-turn state (`InvestigatorTurn` frame on
    /// top of the `InvestigationPhase` anchor), the shape `legal_actions` enumerates.
    fn open_turn_state() -> crate::state::GameState {
        GameStateBuilder::default()
            .with_investigator(test_investigator(1))
            .with_phase(Phase::Investigation)
            .with_active_investigator(InvestigatorId(1))
            .with_turn_order([InvestigatorId(1)])
            // A realistic board has a non-empty chaos bag — skill-test-initiating
            // actions (Investigate) reject on an empty bag (a malformed-state
            // guard the enumerator does not replicate; real bags are never empty).
            .with_chaos_bag(crate::state::ChaosBag::new([
                crate::state::ChaosToken::Numeric(0),
            ]))
            .with_phase_anchor(Continuation::InvestigationPhase {
                resume: InvestigationResume::TurnBegins,
            })
            .with_investigator_turn(InvestigatorId(1))
            .build()
    }

    /// An open-turn state with an advanceable act (threshold `t`) and the
    /// investigator holding `clues`.
    fn open_turn_with_act(threshold: u8, clues: u8) -> crate::state::GameState {
        use crate::state::{Act, CardCode};
        let mut state = open_turn_state();
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .clues = clues;
        state.act_deck = vec![Act {
            code: CardCode("_test_act".into()),
            clue_threshold: threshold,
            resolution: None,
        }];
        state
    }

    #[test]
    fn advance_act_offered_when_clues_meet_threshold() {
        let state = open_turn_with_act(2, 2);
        assert!(legal_actions(&state).contains(&TurnAction::AdvanceAct {
            investigator: InvestigatorId(1),
        }));
    }

    #[test]
    fn advance_act_absent_when_clues_insufficient() {
        let state = open_turn_with_act(2, 1);
        assert!(!legal_actions(&state).contains(&TurnAction::AdvanceAct {
            investigator: InvestigatorId(1),
        }));
    }

    #[test]
    fn advance_act_absent_with_no_act_deck() {
        // open_turn_state has an empty act_deck → AdvanceAct not offered.
        let state = open_turn_state();
        assert!(!legal_actions(&state).contains(&TurnAction::AdvanceAct {
            investigator: InvestigatorId(1),
        }));
    }

    /// An enemy engaged with investigator 1 at `loc`, ready.
    fn engaged_enemy(id: u32, loc: crate::state::LocationId) -> crate::state::Enemy {
        let mut e = crate::test_support::test_enemy(id, "Ghoul");
        e.engaged_with = Some(InvestigatorId(1));
        e.current_location = Some(loc);
        e
    }

    #[test]
    fn end_turn_is_always_offered_at_the_open_turn() {
        let state = open_turn_state();
        assert!(legal_actions(&state).contains(&TurnAction::EndTurn));
    }

    #[test]
    fn fight_and_evade_offered_for_each_engaged_enemy() {
        let mut state = open_turn_state();
        let loc = crate::test_support::test_location(10, "Study");
        let loc_id = loc.id;
        state.locations.insert(loc_id, loc);
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .current_location = Some(loc_id);
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .actions_remaining = 3;
        let e = engaged_enemy(7, loc_id);
        state.enemies.insert(e.id, e);

        let actions = legal_actions(&state);
        assert!(actions.contains(&TurnAction::Fight {
            investigator: InvestigatorId(1),
            enemy: crate::state::EnemyId(7),
        }));
        assert!(actions.contains(&TurnAction::Evade {
            investigator: InvestigatorId(1),
            enemy: crate::state::EnemyId(7),
        }));
    }

    #[test]
    fn fight_but_not_evade_for_an_unengaged_co_located_enemy() {
        // #401: Fight targets any co-located enemy (RR p.12); Evade is
        // engagement-only (RR p.11). An unengaged enemy at the investigator's
        // location is a Fight target but not an Evade target.
        let mut state = open_turn_state();
        let loc = crate::test_support::test_location(10, "Study");
        let loc_id = loc.id;
        state.locations.insert(loc_id, loc);
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .current_location = Some(loc_id);
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .actions_remaining = 3;
        let mut e = crate::test_support::test_enemy(7, "Ghoul");
        e.current_location = Some(loc_id); // co-located, but engaged with nobody
        state.enemies.insert(e.id, e);

        let actions = legal_actions(&state);
        assert!(actions.contains(&TurnAction::Fight {
            investigator: InvestigatorId(1),
            enemy: crate::state::EnemyId(7),
        }));
        assert!(!actions.contains(&TurnAction::Evade {
            investigator: InvestigatorId(1),
            enemy: crate::state::EnemyId(7),
        }));
    }

    #[test]
    fn no_combat_for_an_enemy_at_a_different_location() {
        // An enemy elsewhere (and unengaged) is neither a Fight nor an Evade
        // target (#401: Fight needs co-location).
        let mut state = open_turn_state();
        let here = crate::test_support::test_location(10, "Study");
        let there = crate::test_support::test_location(11, "Attic");
        let (here_id, there_id) = (here.id, there.id);
        state.locations.insert(here_id, here);
        state.locations.insert(there_id, there);
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .current_location = Some(here_id);
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .actions_remaining = 3;
        let mut e = crate::test_support::test_enemy(7, "Ghoul");
        e.current_location = Some(there_id);
        state.enemies.insert(e.id, e);
        assert!(!legal_actions(&state)
            .iter()
            .any(|a| matches!(a, TurnAction::Fight { .. } | TurnAction::Evade { .. })));
    }

    #[test]
    fn negative_fight_value_offers_evade_only() {
        let mut state = open_turn_state();
        let loc = crate::test_support::test_location(10, "Study");
        let loc_id = loc.id;
        state.locations.insert(loc_id, loc);
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .current_location = Some(loc_id);
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .actions_remaining = 3;
        let mut e = engaged_enemy(7, loc_id);
        e.fight = -1; // malformed-but-handled: handler rejects Fight, allows Evade
        state.enemies.insert(e.id, e);

        let actions = legal_actions(&state);
        assert!(!actions.contains(&TurnAction::Fight {
            investigator: InvestigatorId(1),
            enemy: crate::state::EnemyId(7),
        }));
        assert!(actions.contains(&TurnAction::Evade {
            investigator: InvestigatorId(1),
            enemy: crate::state::EnemyId(7),
        }));
    }

    #[test]
    fn no_actions_when_not_the_open_turn() {
        // No InvestigatorTurn frame on top (empty stack) → nothing to offer.
        let state = GameStateBuilder::default()
            .with_investigator(test_investigator(1))
            .with_phase(Phase::Investigation)
            .with_active_investigator(InvestigatorId(1))
            .build();
        assert!(legal_actions(&state).is_empty());
    }

    #[test]
    fn basic_actions_offered_with_a_revealed_location_and_an_action() {
        let mut state = open_turn_state();
        // Place the investigator on a revealed location so Investigate is legal.
        let loc = crate::test_support::test_location(10, "Study");
        let loc_id = loc.id;
        state.locations.insert(loc_id, loc);
        state.locations.get_mut(&loc_id).unwrap().revealed = true;
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .current_location = Some(loc_id);
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .actions_remaining = 3;

        let actions = legal_actions(&state);
        assert!(actions.contains(&TurnAction::Resource {
            investigator: InvestigatorId(1)
        }));
        assert!(actions.contains(&TurnAction::Draw {
            investigator: InvestigatorId(1)
        }));
        assert!(actions.contains(&TurnAction::Investigate {
            investigator: InvestigatorId(1)
        }));
    }

    #[test]
    fn no_action_points_offers_only_end_turn() {
        let mut state = open_turn_state();
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .actions_remaining = 0;
        // With 0 actions, only EndTurn (which needs no action point) is legal.
        assert_eq!(legal_actions(&state), vec![TurnAction::EndTurn]);
    }

    #[test]
    fn investigate_absent_on_an_unrevealed_location() {
        let mut state = open_turn_state();
        let mut loc = crate::test_support::test_location(10, "Study");
        loc.revealed = false;
        let loc_id = loc.id;
        state.locations.insert(loc_id, loc);
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .current_location = Some(loc_id);
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .actions_remaining = 3;
        assert!(!legal_actions(&state).contains(&TurnAction::Investigate {
            investigator: InvestigatorId(1)
        }));
    }

    #[test]
    fn move_offers_one_option_per_connected_destination() {
        let mut state = open_turn_state();
        let mut a = crate::test_support::test_location(10, "A");
        let b = crate::test_support::test_location(11, "B");
        a.connections = vec![b.id];
        let (a_id, b_id) = (a.id, b.id);
        state.locations.insert(a_id, a);
        state.locations.insert(b_id, b);
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .current_location = Some(a_id);
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .actions_remaining = 3;

        let actions = legal_actions(&state);
        assert!(actions.contains(&TurnAction::Move {
            investigator: InvestigatorId(1),
            destination: b_id,
        }));
        // No self-move.
        assert!(!actions.contains(&TurnAction::Move {
            investigator: InvestigatorId(1),
            destination: a_id,
        }));
    }

    #[test]
    fn move_absent_when_unaffordable() {
        let mut state = open_turn_state();
        let mut a = crate::test_support::test_location(10, "A");
        let b = crate::test_support::test_location(11, "B");
        a.connections = vec![b.id];
        let (a_id, b_id) = (a.id, b.id);
        state.locations.insert(a_id, a);
        state.locations.insert(b_id, b);
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .current_location = Some(a_id);
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .actions_remaining = 0;
        assert!(!legal_actions(&state)
            .iter()
            .any(|a| matches!(a, TurnAction::Move { .. })));
    }

    #[test]
    fn engage_offered_for_co_located_enemy_engaged_with_another() {
        let mut state = open_turn_state();
        // Two investigators so an enemy can be engaged with the *other* one.
        state
            .investigators
            .insert(InvestigatorId(2), test_investigator(2));
        let loc = crate::test_support::test_location(10, "Study");
        let loc_id = loc.id;
        state.locations.insert(loc_id, loc);
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .current_location = Some(loc_id);
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .actions_remaining = 3;
        // Enemy at my location, engaged with investigator 2 → I may engage it.
        let mut e = crate::test_support::test_enemy(7, "Ghoul");
        e.current_location = Some(loc_id);
        e.engaged_with = Some(InvestigatorId(2));
        state.enemies.insert(e.id, e);

        assert!(legal_actions(&state).contains(&TurnAction::Engage {
            investigator: InvestigatorId(1),
            enemy: crate::state::EnemyId(7),
        }));
    }

    #[test]
    fn no_engage_for_an_enemy_already_engaged_with_me_or_elsewhere() {
        let mut state = open_turn_state();
        let loc = crate::test_support::test_location(10, "Study");
        let other = crate::test_support::test_location(11, "Hall");
        let (loc_id, other_id) = (loc.id, other.id);
        state.locations.insert(loc_id, loc);
        state.locations.insert(other_id, other);
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .current_location = Some(loc_id);
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .actions_remaining = 3;
        // Already engaged with me → not engageable.
        let mut mine = engaged_enemy(7, loc_id);
        mine.current_location = Some(loc_id);
        state.enemies.insert(mine.id, mine);
        // At a different location → not engageable.
        let mut away = crate::test_support::test_enemy(8, "Rat");
        away.current_location = Some(other_id);
        state.enemies.insert(away.id, away);

        let engages: Vec<_> = legal_actions(&state)
            .into_iter()
            .filter(|a| matches!(a, TurnAction::Engage { .. }))
            .collect();
        assert!(engages.is_empty(), "no Engage offered, got {engages:?}");
    }

    #[test]
    fn every_enumerated_action_is_accepted_by_its_handler() {
        // The cross-check that makes "defer routing" safe: each enumerated
        // action applies without Rejected (Done or AwaitingInput both mean
        // "accepted"). Uses the OptionId round-trip (the truest cross-check:
        // dispatch goes through `ResolveInput(PickSingle(OptionId))`, not the
        // typed arms). Apply to a fresh clone per action. The board has a
        // connected, revealed destination so a Move is enumerated and checked too.
        //
        // install_test_registry: EndTurn (and other actions) reads max_health /
        // max_sanity on the investigator card; the test registry provides those.
        crate::test_support::install_test_registry();
        let mut state = open_turn_state();
        let mut a = crate::test_support::test_location(10, "A");
        let b = crate::test_support::test_location(11, "B");
        a.connections = vec![b.id];
        let (a_id, _b_id) = (a.id, b.id);
        state.locations.insert(a_id, a);
        state.locations.insert(b.id, b);
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .current_location = Some(a_id);
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .actions_remaining = 3;
        // An enemy engaged with the active investigator → Fight + Evade enumerated.
        let mut foe = crate::test_support::test_enemy(7, "Ghoul");
        foe.engaged_with = Some(InvestigatorId(1));
        foe.current_location = Some(a_id);
        state.enemies.insert(foe.id, foe);
        // A co-located unengaged enemy → Engage enumerated (its AoO comes from
        // the engaged foe above; that is enemy_attack, never a Rejected).
        let mut engageable = crate::test_support::test_enemy(8, "Rat");
        engageable.current_location = Some(a_id);
        state.enemies.insert(engageable.id, engageable);
        // An advanceable act (threshold met) → AdvanceAct enumerated; a second
        // act so advancing is a clean transition, not a terminal resolution.
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .clues = 2;
        state.act_deck = vec![
            crate::state::Act {
                code: crate::state::CardCode("_act1".into()),
                clue_threshold: 2,
                resolution: None,
            },
            crate::state::Act {
                code: crate::state::CardCode("_act2".into()),
                clue_threshold: 99,
                resolution: None,
            },
        ];

        let actions = legal_actions(&state);
        for (i, action) in actions.iter().enumerate() {
            let result = crate::apply(
                state.clone(),
                crate::Action::Player(crate::action::PlayerAction::ResolveInput {
                    response: crate::action::InputResponse::PickSingle(crate::engine::OptionId(
                        u32::try_from(i).expect("action index fits u32"),
                    )),
                }),
            );
            assert!(
                !matches!(result.outcome, crate::EngineOutcome::Rejected { .. }),
                "enumerated {action:?} (OptionId {i}) rejected: {:?}",
                result.outcome,
            );
        }
    }

    #[test]
    fn resolve_input_optionid_dispatches_enumerated_turn_action() {
        // EndTurn is always OptionId of its position in legal_actions; submitting it
        // via ResolveInput must dispatch (not reject) even while the open turn still
        // idles Done (pre-flip).
        //
        // install_test_registry: EndTurn reads max_health / max_sanity.
        crate::test_support::install_test_registry();
        let state = open_turn_state();
        let actions = legal_actions(&state);
        let idx = actions
            .iter()
            .position(|a| *a == TurnAction::EndTurn)
            .expect("EndTurn offered");
        let result = crate::apply(
            state,
            crate::Action::Player(crate::action::PlayerAction::ResolveInput {
                response: crate::action::InputResponse::PickSingle(crate::engine::OptionId(
                    u32::try_from(idx).expect("action index fits u32"),
                )),
            }),
        );
        assert!(
            !matches!(result.outcome, crate::EngineOutcome::Rejected { .. }),
            "open-turn OptionId dispatch rejected: {:?}",
            result.outcome
        );
    }
}
