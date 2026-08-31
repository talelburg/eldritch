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
//! **Cell: the `at` cell, for both fronts.** Each prints *"At"* — *"At the end of
//! the enemy phase"* and *"At the end of the round"* — and `glossary/At.md`
//! puts such abilities *"in between any "when..." abilities and any
//! "after..." abilities with the same triggering condition."* The ruling on
//! the round-end half says it in this scenario's own terms: *""At the end of
//! the round" effects trigger after "When the round ends" effects (e.g. The
//! Barrier)"* (<https://arkhamdb.com/card/01107>) — act 01109's *when* half
//! resolves first, then this doom placement, then any `after` ability.
//!
//! **The reverse**, `back_text` verbatim
//! (`data/arkhamdb-snapshot/pack/core/core_encounter.json`):
//!
//! > - If the investigators are at Act 1 or 2, they are trapped inside the
//! >   house as the ghouls tear them apart. **(→R3)**
//! > - If the investigators are at Act 3, they barely escape with their lives,
//! >   allowing the ghouls to run rampant. Each investigator that has not
//! >   resigned is defeated and suffers 1 physical trauma.
//!
//! **The reverse branches on the act deck**, and only the first branch carries a
//! resolution point. At act 1 or 2 it reaches the printed `(→R3)` by *running an
//! effect* (ADR 0013); at act 3 it prints **no** `(→R#)` at all — it defeats the
//! table instead, and the scenario ends where the campaign guide's untitled
//! *"If no resolution was reached (each investigator resigned or was defeated)"*
//! entry says it does. 01107 is terminal in the ordinary way: last card in the
//! agenda deck, so dooming out flips it and its reverse is what ends the
//! scenario, on either branch.
//!
//! The branch is an `Effect::If` over a card-local
//! [`Condition::Native`](card_dsl::dsl::Condition::Native) (#276/#592) reading
//! `act_index`. One consumer, and a plain scenario-state read rather than the
//! *compound or target-referencing* predicate `TODO(#609)` reserves promotion
//! for, so it stays card-local. **The card says "Act 3"; the engine holds a
//! zero-based act cursor**, so the predicate is `act_index == 2`.
//!
//! The act-3 arm is likewise a card-local native effect — a loop over
//! `turn_order` calling [`defeat_investigator`], not `Effect::ForEach`, whose
//! evaluator arm is a stub and whose general design is still open (#363). The
//! loop and the body it fans out to each have one consumer, so both fail the
//! DSL-primitive threshold independently. **Turn order rather than map order**
//! because the order is observable on a defeat body: Elimination step 5
//! reassigns the lead when the lead is eliminated, and step 6's all-defeated
//! check fires on whoever falls last. **One caveat on that order**: an
//! investigator holding an in-play weakness with a *"when the game ends"* ability
//! (Cover Up 01007) routes Elimination onto a `Continuation::Elimination` frame
//! (#638), so their steps 1–6 run after this synchronous loop rather than inside
//! it. The ending is the same either way — the frame drains before anything reads
//! it — but the teardown interleaves. Pre-existing to this card, and the shape
//! `Effect::ForEach` will have to answer for whenever #363 lands.
//!
//! **The ending is reached by the rules' route, not latched.** Each defeat runs
//! Rules Reference p.10 Elimination, and step 6 — *"If there are no remaining
//! players, the scenario ends. Refer to "no resolution was reached" entry for
//! that scenario in the campaign guide."* — is what produces
//! [`ScenarioEnding::NoResolution`](game_core::ScenarioEnding::NoResolution).
//! The card names no ending on this branch, because the card prints none.
//!
//! *"That has not resigned"* needs no filter to be *correct*:
//! [`defeat_investigator`] no-ops on any investigator who is not `Active`, and a
//! resigned one is not. The loop reads the status anyway, so that the trauma
//! announcement fires only for someone this card actually defeated.
//!
//! **A defeat by a card ability is neither killed nor driven insane.**
//! `glossary/Defeat.md`: *"An investigator might also be defeated by a card
//! ability."* The same entry makes killed and insane consequences of **trauma**
//! — *"Taking trauma may cause an investigator to be killed or driven insane"* —
//! and these investigators take one physical trauma, the first step on that
//! track rather than its end. So the defeat carries
//! [`game_core::EliminationCause::CardAbility`] and leaves
//! [`game_core::Status::Defeated`], both distinct from the damage
//! and horror values.
//!
//! **The physical trauma is announced, not recorded.** `Event::TraumaSuffered`
//! is emitted per defeated investigator, following Cover Up 01007's
//! mental-trauma precedent; nothing persists it until the phase-9 campaign log
//! (#766).
//!
//! **Cell: the `after` cell of the `AgendaAdvanced` condition**, for the
//! reverse. The reverse prints no trigger word, because it is not a triggered
//! ability: it is step 2 of the advance procedure — *"Flip the advancing card
//! over and follow the instructions on the reverse ("b") side."*
//! (`glossary/Act_Deck_and_Agenda_Deck.md`). Declaring it in the `after` cell of
//! the flip is what puts it where the procedure puts it: after step 1's token
//! removal and the flip itself, and before step 3's *"the next card in the deck
//! becomes the current act/agenda"* — which for a terminal agenda never comes,
//! since the `AdvanceReverse` frame holds the cursor and there is no next card.
//! The same cell 01105 and 01106 declare. With no printed word to read against,
//! nothing contests it; the card's rulings
//! (<https://arkhamdb.com/card/01107>) reach only the two Forced fronts.
//!
//! Map note: on The Gathering's star map (Hallway hub ↔ Attic/Cellar/
//! Parlor), every location has a unique shortest first step toward the
//! Parlor, so the lowest-`LocationId` tie-break below is unreachable in
//! this scenario (RR p.12: the controlling player chooses on a tie —
//! deferred until a map with ties lands). The move goes through
//! [`relocate_enemy`], so a Ghoul arriving at an investigator's location
//! engages on arrival per the general engagement rule (#633) — the card
//! text is positional only, but the framework rule applies regardless.

use card_dsl::dsl::{
    forced_on_event, if_else, native, native_condition, reach_resolution, Ability, EventPattern,
    EventTiming, Phase,
};
use game_core::card_registry::{NativeConditionFn, NativeEffectFn};
use game_core::event::TraumaKind;
use game_core::state::{EnemyId, GameState, InvestigatorId, LocationId, Status};
use game_core::{
    defeat_investigator, enemy_can_enter_location, location_id_by_code, relocate_enemy,
    shortest_first_steps, Cx, EngineOutcome, EvalContext, Event,
};

/// `ArkhamDB` code for Agenda 3, "They're Getting Out!".
pub const CODE: &str = "01107";

const MOVE_GHOULS: &str = "01107:move-ghouls";
const ROUND_END_DOOM: &str = "01107:round-end-doom";
const AT_ACT_THREE: &str = "01107:at-act-three";
const GHOULS_RUN_RAMPANT: &str = "01107:ghouls-run-rampant";

/// Zero-based cursor for the card's *"Act 3"*. `act_index` counts from 0, so
/// act 3 is index 2.
const ACT_THREE_INDEX: usize = 2;

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
            EventTiming::At,
            native(MOVE_GHOULS),
        ),
        forced_on_event(
            EventPattern::RoundEnded,
            EventTiming::At,
            native(ROUND_END_DOOM),
        ),
        forced_on_event(
            EventPattern::AgendaAdvanced,
            EventTiming::After,
            if_else(
                native_condition(AT_ACT_THREE),
                native(GHOULS_RUN_RAMPANT),
                reach_resolution(3),
            ),
        ),
    ]
}

/// Resolve this agenda's native-effect tags. Wired into the crate
/// registry's `native_effect_for`.
pub(crate) fn native_effect_for(tag: &str) -> Option<NativeEffectFn> {
    match tag {
        MOVE_GHOULS => Some(move_ghouls_toward_parlor as NativeEffectFn),
        ROUND_END_DOOM => Some(place_round_end_doom as NativeEffectFn),
        GHOULS_RUN_RAMPANT => Some(the_ghouls_run_rampant as NativeEffectFn),
        _ => None,
    }
}

/// Resolve this agenda's native-condition tags. Wired into the crate
/// registry's `native_condition_for`.
pub(crate) fn native_condition_for(tag: &str) -> Option<NativeConditionFn> {
    match tag {
        AT_ACT_THREE => Some(at_act_three as NativeConditionFn),
        _ => None,
    }
}

/// *"If the investigators are at Act 3"* — the reverse's branch predicate.
///
/// The card counts acts from 1 and the engine holds a zero-based cursor, so
/// this is [`ACT_THREE_INDEX`]. Everything else — The Gathering's acts 1 and 2 —
/// takes the printed `(→R3)` arm, which is the other half of the card's own
/// dichotomy.
fn at_act_three(state: &GameState, _ctx: &EvalContext) -> bool {
    state.act_index == ACT_THREE_INDEX
}

/// *"If the investigators are at Act 3, they barely escape with their lives,
/// allowing the ghouls to run rampant. Each investigator that has not resigned
/// is defeated and suffers 1 physical trauma."*
///
/// **No resolution point.** This branch prints no `(→R#)`, and this function
/// latches nothing: defeating the last active investigator drains Rules
/// Reference p.10 Elimination step 6 — *"If there are no remaining players, the
/// scenario ends"* — which is what produces `ScenarioEnding::NoResolution`.
///
/// **Turn order**, because the order is observable here: Elimination step 5
/// reassigns the lead when the lead falls, and step 6's check fires on whoever
/// falls last.
///
/// **The `Active` gate is *"that has not resigned"*.** [`defeat_investigator`]
/// no-ops on a non-`Active` investigator anyway; reading the status first is
/// what keeps the trauma announcement from firing for someone who was never
/// defeated by this card.
fn the_ghouls_run_rampant(cx: &mut Cx, _ctx: &EvalContext) -> EngineOutcome {
    let order: Vec<InvestigatorId> = cx.state.turn_order.clone();
    for investigator in order {
        if cx.state.investigators.get(&investigator).map(|i| i.status) != Some(Status::Active) {
            continue;
        }
        defeat_investigator(cx, investigator);
        // Announced, not recorded: nothing persists trauma until the phase-9
        // campaign log (#766). Cover Up 01007's mental trauma is the precedent.
        cx.events.push(Event::TraumaSuffered {
            investigator,
            kind: TraumaKind::Physical,
            amount: 1,
        });
    }
    EngineOutcome::Done
}

fn is_ghoul(traits: &[String]) -> bool {
    traits.iter().any(|t| t.as_str() == "Ghoul")
}

/// Each unengaged Ghoul moves one location toward the Parlor (01115).
///
/// **No Parlor in play is a no-op, not a rejection.** The Parlor enters play
/// only via act 2 (01109)'s reverse, while this agenda becomes current at
/// agenda 2's doom threshold — which does not read the act deck — so a group
/// still at act 1 or 2 meets this Forced with no destination on the board,
/// every enemy phase. `glossary/Ability.md`, Forced Abilities: *"If a forced
/// ability does not have the potential to change the game state, the ability
/// does not initiate."* Rejecting here would report malformed data for a board
/// state the scenario reaches legitimately — and since a rejection rolls the
/// whole apply back (#161), it landed on the *player's* action and made the
/// scenario unplayable (#811). Same reading as the barricaded first step
/// below: no move available means no move, never an error.
fn move_ghouls_toward_parlor(cx: &mut Cx, _ctx: &EvalContext) -> EngineOutcome {
    let Some(parlor) = location_id_by_code(cx.state, PARLOR) else {
        return EngineOutcome::Done;
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
        // 01038), and the block bites at the *compelled step*, not on the
        // graph the path is measured over — as Hunter movement does since
        // #651. 01107 names a fixed destination rather than a "nearest"
        // target, so `glossary/Nearest.md` does not reach it, but
        // `glossary/Patrol.md` carries the same rule on a fixed-destination,
        // shortest-path mover, which is structurally what 01107 is: *"If an
        // enemy with patrol would be compelled to move to a location which is
        // blocked by a card ability, the enemy does not move."* So a
        // barricaded first step is a non-move, never a detour (#797).
        let mut steps: Vec<LocationId> = shortest_first_steps(cx.state, from, parlor)
            .into_iter()
            .filter(|&loc| enemy_can_enter_location(cx.state, e, loc))
            .collect();
        steps.sort_unstable();
        if let Some(&to) = steps.first() {
            movers.push((*id, to));
        }
    }
    for (id, to) in movers {
        // The shared relocation funnel (#633): the board write, the
        // `EnemyMoved` emit, and the engage-on-arrival check the general
        // engagement rule requires of *any* enemy movement — an exhausted
        // (evaded) Ghoul moved by this agenda still arrives unengaged.
        relocate_enemy(cx, id, to);
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
    use game_core::test_support::{test_enemy, test_investigator, GameStateBuilder};
    use game_core::Event;

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

    /// The investigators announced defeated, in the order the events landed.
    fn defeat_order(events: &[Event]) -> Vec<InvestigatorId> {
        events
            .iter()
            .filter_map(|e| match e {
                Event::InvestigatorEliminated { investigator, .. } => Some(*investigator),
                _ => None,
            })
            .collect()
    }

    fn with_agenda(state: &mut game_core::state::GameState) {
        state.agenda_deck = vec![Agenda {
            code: CardCode::new("01107"),
            doom_threshold: 10,
        }];
    }

    #[test]
    fn abilities_are_the_two_forced_natives_then_the_reverse() {
        let abilities = abilities();
        assert_eq!(abilities.len(), 3);
        assert_eq!(
            abilities[0].trigger,
            Trigger::OnEvent {
                pattern: EventPattern::PhaseEnded {
                    phase: Phase::Enemy
                },
                timing: EventTiming::At,
                kind: card_dsl::dsl::TriggerKind::Forced,
            }
        );
        assert!(matches!(&abilities[0].effect, Effect::Native { tag } if tag == MOVE_GHOULS));
        assert_eq!(
            abilities[1].trigger,
            Trigger::OnEvent {
                pattern: EventPattern::RoundEnded,
                timing: EventTiming::At,
                kind: card_dsl::dsl::TriggerKind::Forced,
            }
        );
        assert!(matches!(&abilities[1].effect, Effect::Native { tag } if tag == ROUND_END_DOOM));
    }

    /// The reverse branches on the `after` cell of the agenda's own advance
    /// (ADR 0013): at act 3 the ghouls run rampant, and **otherwise** — the
    /// card's act-1-or-2 half — it reaches the printed `(→R3)`.
    #[test]
    fn reverse_branches_on_the_act_deck_after_the_agenda_advances() {
        let abilities = abilities();
        assert_eq!(
            abilities[2].trigger,
            Trigger::OnEvent {
                pattern: EventPattern::AgendaAdvanced,
                timing: EventTiming::After,
                kind: card_dsl::dsl::TriggerKind::Forced,
            }
        );
        let Effect::If {
            condition,
            then,
            else_,
        } = &abilities[2].effect
        else {
            panic!("the reverse is a branch, got {:?}", abilities[2].effect);
        };
        assert!(
            matches!(condition, card_dsl::dsl::Condition::Native { tag } if tag == AT_ACT_THREE),
            "branches on the card-local act-3 predicate, got {condition:?}",
        );
        assert!(
            matches!(&**then, Effect::Native { tag } if tag == GHOULS_RUN_RAMPANT),
            "act 3: the ghouls run rampant, got {then:?}",
        );
        assert!(
            matches!(else_.as_deref(), Some(Effect::ReachResolution(3))),
            "act 1 or 2: the printed (→R3), got {else_:?}",
        );
    }

    /// *"If the investigators are at Act 3"* against a zero-based cursor.
    #[test]
    fn the_act_three_predicate_reads_the_zero_based_cursor() {
        let mut state = GameStateBuilder::new().build();
        let ctx = EvalContext::for_controller(InvestigatorId(1));
        for (act_index, expected) in [(0, false), (1, false), (ACT_THREE_INDEX, true)] {
            state.act_index = act_index;
            assert_eq!(
                at_act_three(&state, &ctx),
                expected,
                "act_index {act_index} (the card's Act {})",
                act_index + 1,
            );
        }
    }

    #[test]
    fn native_condition_for_resolves_the_act_three_tag() {
        assert!(native_condition_for(AT_ACT_THREE).is_some());
        assert!(native_condition_for("01107:other").is_none());
        assert!(
            crate::impls::native_condition_for(AT_ACT_THREE).is_some(),
            "and is reachable through the crate-level dispatch",
        );
    }

    /// The act-3 branch, end to end over the native body: both investigators are
    /// defeated in turn order, each announcing 1 physical trauma, and the
    /// scenario ends at **no resolution point** — Elimination step 6, reached by
    /// the defeats rather than latched by the card.
    #[test]
    fn the_act_three_branch_defeats_the_table_and_reaches_no_resolution() {
        let _ = game_core::card_registry::install(crate::REGISTRY);
        let (a, b) = (InvestigatorId(1), InvestigatorId(2));
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_investigator(test_investigator(2))
            .with_turn_order([a, b])
            .build();

        let events = cx_apply(&mut state, the_ghouls_run_rampant);

        for id in [a, b] {
            assert_eq!(
                state.investigators[&id].status,
                Status::Defeated,
                "defeated by a card ability — neither killed nor driven insane",
            );
        }
        assert_eq!(
            defeat_order(&events),
            vec![a, b],
            "defeated in turn order, not map order",
        );
        for id in [a, b] {
            assert!(
                events.iter().any(|e| matches!(e, Event::TraumaSuffered {
                    investigator, kind, amount
                } if *investigator == id
                    && *kind == TraumaKind::Physical
                    && *amount == 1)),
                "each defeated investigator suffers 1 physical trauma",
            );
        }
        assert!(events
            .iter()
            .any(|e| matches!(e, Event::AllInvestigatorsDefeated)));
        assert_eq!(
            state.ending,
            Some(game_core::ScenarioEnding::NoResolution),
            "no resolution point: the card prints none on this branch",
        );
    }

    /// *"Each investigator that has not resigned"* — the resigned one is left
    /// alone, and announces no trauma.
    #[test]
    fn an_investigator_who_has_resigned_is_left_alone() {
        let _ = game_core::card_registry::install(crate::REGISTRY);
        let (a, b) = (InvestigatorId(1), InvestigatorId(2));
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_investigator(test_investigator(2))
            .with_turn_order([a, b])
            .build();
        state.investigators.get_mut(&a).expect("seated").status = Status::Resigned;

        let events = cx_apply(&mut state, the_ghouls_run_rampant);

        assert_eq!(state.investigators[&a].status, Status::Resigned);
        assert_eq!(state.investigators[&b].status, Status::Defeated);
        assert_eq!(defeat_order(&events), vec![b]);
        assert!(
            !events.iter().any(|e| matches!(
                e,
                Event::TraumaSuffered { investigator, .. } if *investigator == a
            )),
            "no trauma announced for an investigator this card never defeated",
        );
    }

    #[test]
    fn native_effect_for_resolves_every_tag() {
        assert!(native_effect_for(MOVE_GHOULS).is_some());
        assert!(native_effect_for(ROUND_END_DOOM).is_some());
        assert!(native_effect_for(GHOULS_RUN_RAMPANT).is_some());
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
        // compelled step, so it does not move. Needs the real registry so the
        // attachment's `EnemyMovementBlocked` restriction is read.
        let _ = game_core::card_registry::install(crate::REGISTRY);
        let mut state = star_board();
        state.enemies.insert(EnemyId(1), ghoul(1, LocationId(2))); // Hallway
        barricade(&mut state, LocationId(5));
        cx_apply(&mut state, move_ghouls_toward_parlor);
        assert_eq!(
            state.enemies[&EnemyId(1)].current_location,
            Some(LocationId(2)),
            "Ghoul stayed in the Hallway — the only step toward the Parlor is blocked",
        );
    }

    /// A detour board: Attic -> Hallway -> Parlor is the shortest route
    /// (2 steps); Attic -> Study -> Cellar -> Parlor is the long way (3).
    /// Synthetic topology on real Gathering codes — the printed map is the
    /// star in [`star_board`], which has no detour to test against.
    fn detour_board() -> game_core::state::GameState {
        let loc =
            |id, code: &str, name| Location::new(LocationId(id), CardCode::new(code), name, 1, 0);
        let mut state = GameStateBuilder::new()
            .with_location(loc(2, "01112", "Hallway"))
            .with_location(loc(3, "01113", "Attic"))
            .with_location(loc(4, "01114", "Cellar"))
            .with_location(loc(5, "01115", "Parlor"))
            .with_location(loc(6, "01111", "Study"))
            .build();
        state.connect(LocationId(3), LocationId(2));
        state.connect(LocationId(2), LocationId(5));
        state.connect(LocationId(3), LocationId(6));
        state.connect(LocationId(6), LocationId(4));
        state.connect(LocationId(4), LocationId(5));
        state
    }

    fn barricade(state: &mut game_core::state::GameState, at: LocationId) {
        state.locations.get_mut(&at).unwrap().attachments.push(
            game_core::state::CardInPlay::enter_play(
                CardCode::new("01038"),
                game_core::state::CardInstanceId(900),
            ),
        );
    }

    /// `glossary/Patrol.md`: *"If an enemy with patrol would be compelled to
    /// move to a location which is blocked by a card ability, the enemy does
    /// not move."* 01107 is structurally the same fixed-destination,
    /// shortest-path mover, so a barricaded compelled step is a non-move,
    /// not a detour (#797).
    #[test]
    fn ghoul_does_not_reroute_around_a_barricaded_shortest_step() {
        let _ = game_core::card_registry::install(crate::REGISTRY);
        let mut state = detour_board();
        state.enemies.insert(EnemyId(1), ghoul(1, LocationId(3))); // Attic
        barricade(&mut state, LocationId(2)); // Hallway — the only shortest step

        let events = cx_apply(&mut state, move_ghouls_toward_parlor);

        assert_eq!(
            state.enemies[&EnemyId(1)].current_location,
            Some(LocationId(3)),
            "the Ghoul stays put rather than taking the long way through the Study",
        );
        assert!(!events.iter().any(|e| matches!(e, Event::EnemyMoved { .. })));
    }

    #[test]
    fn ghoul_takes_the_shortest_step_when_nothing_is_barricaded() {
        let mut state = detour_board();
        state.enemies.insert(EnemyId(1), ghoul(1, LocationId(3))); // Attic
        cx_apply(&mut state, move_ghouls_toward_parlor);
        assert_eq!(
            state.enemies[&EnemyId(1)].current_location,
            Some(LocationId(2)),
        );
    }

    #[test]
    fn a_barricade_off_the_shortest_path_does_not_affect_the_move() {
        let _ = game_core::card_registry::install(crate::REGISTRY);
        let mut state = detour_board();
        state.enemies.insert(EnemyId(1), ghoul(1, LocationId(3))); // Attic
        barricade(&mut state, LocationId(6)); // Study — on the long route only
        cx_apply(&mut state, move_ghouls_toward_parlor);
        assert_eq!(
            state.enemies[&EnemyId(1)].current_location,
            Some(LocationId(2)),
        );
    }

    /// A Ghoul that steps into a lone investigator's location engages on
    /// arrival (#633). `Enemy_Engagement.md`: *"Any time a ready unengaged
    /// enemy is at the same location as an investigator, it engages that
    /// investigator"*, listed example *"It moves into the same location as
    /// an investigator"*.
    #[test]
    fn ghoul_moving_into_the_investigators_location_engages_on_arrival() {
        let mut state = star_board();
        let mut inv = test_investigator(1);
        inv.current_location = Some(LocationId(2)); // Hallway
        state.investigators.insert(InvestigatorId(1), inv);
        state.turn_order = vec![InvestigatorId(1)];
        state.enemies.insert(EnemyId(1), ghoul(1, LocationId(3))); // Attic

        let events = cx_apply(&mut state, move_ghouls_toward_parlor);

        assert_eq!(
            state.enemies[&EnemyId(1)].current_location,
            Some(LocationId(2)),
            "Ghoul stepped Attic -> Hallway"
        );
        assert_eq!(
            state.enemies[&EnemyId(1)].engaged_with,
            Some(InvestigatorId(1)),
            "and engaged the investigator standing there"
        );
        assert!(events.iter().any(|e| matches!(e,
            Event::EnemyEngaged { enemy, investigator }
                if *enemy == EnemyId(1) && *investigator == InvestigatorId(1))));
    }

    /// *"This agenda can move exhausted (evaded) enemies"*
    /// (<https://arkhamdb.com/card/01107>), but `Enemy_Engagement.md`:
    /// *"An exhausted unengaged enemy does not engage"* — so it arrives
    /// unengaged and engages only when it readies (#633).
    #[test]
    fn exhausted_ghoul_moved_into_the_investigators_location_arrives_unengaged() {
        let mut state = star_board();
        let mut inv = test_investigator(1);
        inv.current_location = Some(LocationId(2)); // Hallway
        state.investigators.insert(InvestigatorId(1), inv);
        state.turn_order = vec![InvestigatorId(1)];
        let mut evaded = ghoul(1, LocationId(3)); // Attic
        evaded.exhausted = true;
        state.enemies.insert(EnemyId(1), evaded);

        let events = cx_apply(&mut state, move_ghouls_toward_parlor);

        assert_eq!(
            state.enemies[&EnemyId(1)].current_location,
            Some(LocationId(2)),
            "the evaded Ghoul still moves"
        );
        assert_eq!(
            state.enemies[&EnemyId(1)].engaged_with,
            None,
            "but an exhausted unengaged enemy does not engage"
        );
        assert!(!events
            .iter()
            .any(|e| matches!(e, Event::EnemyEngaged { .. })));
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
