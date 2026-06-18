//! They're Getting Out! (The Gathering Agenda 3, 01107).
//!
//! ```text
//! Forced - At the end of the enemy phase: Each unengaged [[Ghoul]]
//!   enemy moves 1 location towards the Parlor.
//! Forced - At the end of the round: Place 1 doom on this agenda for
//!   each [[Ghoul]] enemy in the Hallway or Parlor.
//! ```
//!
//! Both are board-dependent, single-use scenario logic, so they live
//! card-locally as `Effect::Native` handlers (#276) rather than shared
//! `Effect` variants. The enemy-phase-end move keys off the existing
//! `EventPattern::PhaseEnded { Enemy }`; the round-end doom off the new
//! `EventPattern::RoundEnded`.
//!
//! Map note: on The Gathering's star map (Hallway hub ↔ Attic/Cellar/
//! Parlor), every location has a unique shortest first step toward the
//! Parlor, so the lowest-`LocationId` tie-break below is unreachable in
//! this scenario (RR p.12: the controlling player chooses on a tie —
//! deferred until a map with ties lands). Engagement-on-arrival is not
//! modeled for the forced move (the card text is positional only).

use card_dsl::dsl::{forced_on_event, native, Ability, EventPattern, EventTiming, Phase};
use game_core::card_registry::NativeEffectFn;
use game_core::state::{EnemyId, LocationId};
use game_core::{
    enemy_can_enter_location, location_id_by_code, shortest_first_steps_with, Cx, EngineOutcome,
    EvalContext, Event,
};

/// `ArkhamDB` code for Agenda 3, "They're Getting Out!".
pub const CODE: &str = "01107";

const MOVE_GHOULS: &str = "01107:move-ghouls";
const ROUND_END_DOOM: &str = "01107:round-end-doom";

/// The Parlor and Hallway printed codes (the doom-counting locations;
/// the Parlor is also the movement target).
const PARLOR: &str = "01115";
const HALLWAY: &str = "01112";

#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![
        forced_on_event(
            EventPattern::PhaseEnded {
                phase: Phase::Enemy,
            },
            EventTiming::After,
            native(MOVE_GHOULS),
        ),
        forced_on_event(
            EventPattern::RoundEnded,
            EventTiming::After,
            native(ROUND_END_DOOM),
        ),
    ]
}

/// Resolve this agenda's native-effect tags. Wired into the crate
/// registry's `native_effect_for`.
pub(crate) fn native_effect_for(tag: &str) -> Option<NativeEffectFn> {
    match tag {
        MOVE_GHOULS => Some(move_ghouls_toward_parlor as NativeEffectFn),
        ROUND_END_DOOM => Some(place_round_end_doom as NativeEffectFn),
        _ => None,
    }
}

fn is_ghoul(traits: &[String]) -> bool {
    traits.iter().any(|t| t.as_str() == "Ghoul")
}

/// Each unengaged Ghoul moves one location toward the Parlor (01115).
fn move_ghouls_toward_parlor(cx: &mut Cx, _ctx: &EvalContext) -> EngineOutcome {
    let Some(parlor) = location_id_by_code(cx.state, PARLOR) else {
        return EngineOutcome::Rejected {
            reason: "01107 move-ghouls: Parlor (01115) not in play".into(),
        };
    };
    // Scan first (shared borrows), then mutate. Deterministic lowest-
    // LocationId tie-break among shortest first steps.
    let mut movers: Vec<(EnemyId, LocationId)> = Vec::new();
    for (id, e) in &cx.state.enemies {
        if e.engaged_with.is_some() || !is_ghoul(&e.traits) {
            continue;
        }
        let Some(from) = e.current_location else {
            continue;
        };
        // A non-Elite Ghoul cannot move into a barricaded location (Barricade
        // 01038); the block is graph-level, mirroring Hunter movement.
        let mut steps = shortest_first_steps_with(cx.state, from, parlor, |loc| {
            enemy_can_enter_location(cx.state, e, loc)
        });
        steps.sort_unstable();
        if let Some(&to) = steps.first() {
            movers.push((*id, to));
        }
    }
    for (id, to) in movers {
        if let Some(e) = cx.state.enemies.get_mut(&id) {
            e.current_location = Some(to);
        }
        cx.events.push(Event::EnemyMoved { enemy: id, to });
    }
    EngineOutcome::Done
}

/// Place 1 doom on the agenda per Ghoul in the Hallway (01112) or Parlor
/// (01115). Not filtered by engagement (per card text). No threshold
/// check — RR p.24 checks doom in Mythos step 1.3.
fn place_round_end_doom(cx: &mut Cx, _ctx: &EvalContext) -> EngineOutcome {
    let counted: Vec<LocationId> = [HALLWAY, PARLOR]
        .iter()
        .filter_map(|c| location_id_by_code(cx.state, c))
        .collect();
    let count = cx
        .state
        .enemies
        .values()
        .filter(|e| is_ghoul(&e.traits))
        .filter(|e| e.current_location.is_some_and(|l| counted.contains(&l)))
        .count();
    let count = u8::try_from(count).unwrap_or(u8::MAX);
    cx.state.agenda_doom = cx.state.agenda_doom.saturating_add(count);
    EngineOutcome::Done
}

#[cfg(test)]
mod tests {
    use super::*;
    use card_dsl::dsl::{Effect, Trigger};
    use game_core::state::{Agenda, CardCode, Enemy, InvestigatorId, Location};
    use game_core::test_support::{test_enemy, GameStateBuilder};

    fn ghoul(id: u32, at: LocationId) -> Enemy {
        let mut e = test_enemy(id, "Ghoul");
        e.traits = vec!["Humanoid".into(), "Monster".into(), "Ghoul".into()];
        e.current_location = Some(at);
        e
    }

    // Hallway(2) hub connects to Attic(3), Cellar(4), Parlor(5).
    fn star_board() -> game_core::state::GameState {
        let loc =
            |id, code: &str, name| Location::new(LocationId(id), CardCode::new(code), name, 1, 0);
        let mut state = GameStateBuilder::new()
            .with_location(loc(2, "01112", "Hallway"))
            .with_location(loc(3, "01113", "Attic"))
            .with_location(loc(4, "01114", "Cellar"))
            .with_location(loc(5, "01115", "Parlor"))
            .build();
        for spoke in [LocationId(3), LocationId(4), LocationId(5)] {
            state.connect(LocationId(2), spoke);
        }
        state
    }

    fn cx_apply(state: &mut game_core::state::GameState, f: NativeEffectFn) -> Vec<Event> {
        let mut events = Vec::new();
        let mut cx = Cx {
            state,
            events: &mut events,
        };
        let out = f(&mut cx, &EvalContext::for_controller(InvestigatorId(1)));
        assert_eq!(out, EngineOutcome::Done);
        events
    }

    fn with_agenda(state: &mut game_core::state::GameState) {
        state.agenda_deck = vec![Agenda {
            code: CardCode::new("01107"),
            doom_threshold: 10,
            resolution: None,
        }];
    }

    #[test]
    fn abilities_are_two_forced_native_effects() {
        let abilities = abilities();
        assert_eq!(abilities.len(), 2);
        assert_eq!(
            abilities[0].trigger,
            Trigger::OnEvent {
                pattern: EventPattern::PhaseEnded {
                    phase: Phase::Enemy
                },
                timing: EventTiming::After,
                kind: card_dsl::dsl::TriggerKind::Forced,
            }
        );
        assert!(matches!(&abilities[0].effect, Effect::Native { tag } if tag == MOVE_GHOULS));
        assert_eq!(
            abilities[1].trigger,
            Trigger::OnEvent {
                pattern: EventPattern::RoundEnded,
                timing: EventTiming::After,
                kind: card_dsl::dsl::TriggerKind::Forced,
            }
        );
        assert!(matches!(&abilities[1].effect, Effect::Native { tag } if tag == ROUND_END_DOOM));
    }

    #[test]
    fn native_effect_for_resolves_both_tags() {
        assert!(native_effect_for(MOVE_GHOULS).is_some());
        assert!(native_effect_for(ROUND_END_DOOM).is_some());
        assert!(native_effect_for("01107:other").is_none());
    }

    #[test]
    fn unengaged_ghoul_in_attic_steps_to_hallway() {
        let mut state = star_board();
        state.enemies.insert(EnemyId(1), ghoul(1, LocationId(3))); // Attic
        let events = cx_apply(&mut state, move_ghouls_toward_parlor);
        assert_eq!(
            state.enemies[&EnemyId(1)].current_location,
            Some(LocationId(2))
        );
        assert!(events.iter().any(|e| matches!(e,
            Event::EnemyMoved { enemy, to } if *enemy == EnemyId(1) && *to == LocationId(2))));
    }

    #[test]
    fn ghoul_in_hallway_steps_to_parlor() {
        let mut state = star_board();
        state.enemies.insert(EnemyId(1), ghoul(1, LocationId(2))); // Hallway
        cx_apply(&mut state, move_ghouls_toward_parlor);
        assert_eq!(
            state.enemies[&EnemyId(1)].current_location,
            Some(LocationId(5))
        );
    }

    #[test]
    fn non_elite_ghoul_does_not_move_into_a_barricaded_parlor() {
        // A Barricade (01038) on the Parlor blocks the non-Elite Ghoul's
        // forced move — same graph-level block as Hunter movement. Needs the
        // real registry so the attachment's `EnemyMovementBlocked` restriction
        // is read.
        let _ = game_core::card_registry::install(crate::REGISTRY);
        let mut state = star_board();
        state.enemies.insert(EnemyId(1), ghoul(1, LocationId(2))); // Hallway
        state
            .locations
            .get_mut(&LocationId(5))
            .unwrap()
            .attachments
            .push(game_core::state::CardInPlay::enter_play(
                CardCode::new("01038"),
                game_core::state::CardInstanceId(900),
            ));
        cx_apply(&mut state, move_ghouls_toward_parlor);
        assert_eq!(
            state.enemies[&EnemyId(1)].current_location,
            Some(LocationId(2)),
            "Ghoul stayed in the Hallway — the only step toward the Parlor is blocked",
        );
    }

    #[test]
    fn engaged_ghoul_and_ghoul_at_parlor_do_not_move() {
        let mut state = star_board();
        let mut engaged = ghoul(1, LocationId(3));
        engaged.engaged_with = Some(InvestigatorId(1));
        state.enemies.insert(EnemyId(1), engaged);
        state.enemies.insert(EnemyId(2), ghoul(2, LocationId(5))); // already at Parlor
        cx_apply(&mut state, move_ghouls_toward_parlor);
        assert_eq!(
            state.enemies[&EnemyId(1)].current_location,
            Some(LocationId(3))
        );
        assert_eq!(
            state.enemies[&EnemyId(2)].current_location,
            Some(LocationId(5))
        );
    }

    #[test]
    fn non_ghoul_does_not_move() {
        let mut state = star_board();
        let mut e = test_enemy(1, "Rat");
        e.traits = vec!["Creature".into()];
        e.current_location = Some(LocationId(3));
        state.enemies.insert(EnemyId(1), e);
        cx_apply(&mut state, move_ghouls_toward_parlor);
        assert_eq!(
            state.enemies[&EnemyId(1)].current_location,
            Some(LocationId(3))
        );
    }

    #[test]
    fn doom_counts_ghouls_in_hallway_and_parlor_only() {
        let mut state = star_board();
        with_agenda(&mut state);
        state.enemies.insert(EnemyId(1), ghoul(1, LocationId(2))); // Hallway — counts
        state.enemies.insert(EnemyId(2), ghoul(2, LocationId(5))); // Parlor — counts
        state.enemies.insert(EnemyId(3), ghoul(3, LocationId(3))); // Attic — no
        let mut non_ghoul = test_enemy(4, "Rat");
        non_ghoul.traits = vec!["Creature".into()];
        non_ghoul.current_location = Some(LocationId(2));
        state.enemies.insert(EnemyId(4), non_ghoul); // Hallway non-Ghoul — no
        cx_apply(&mut state, place_round_end_doom);
        assert_eq!(state.agenda_doom, 2);
    }

    #[test]
    fn engaged_ghoul_in_hallway_still_counts_for_doom() {
        let mut state = star_board();
        with_agenda(&mut state);
        let mut engaged = ghoul(1, LocationId(2)); // Hallway, engaged
        engaged.engaged_with = Some(InvestigatorId(1));
        state.enemies.insert(EnemyId(1), engaged);
        cx_apply(&mut state, place_round_end_doom);
        assert_eq!(state.agenda_doom, 1);
    }
}
