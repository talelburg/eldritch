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
//! card-locally as `Effect::Native` handlers rather than shared
//! `Effect` variants. The enemy-phase-end move keys off
//! `EventPattern::PhaseEnded { Enemy }`; the round-end doom off
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
//! [`Condition::Native`](card_dsl::dsl::Condition::Native) reading
//! `act_index`. One consumer, and a plain scenario-state read rather than a
//! *compound or target-referencing* predicate, the kind due for promotion to
//! declarative vocab, so it stays card-local. **The card says "Act 3"; the engine holds a
//! zero-based act cursor**, so the predicate is `act_index == 2`.
//!
//! The act-3 arm's fan-out rides `Effect::ForEach` over every investigator;
//! its body is a card-local native effect calling
//! [`defeat_investigator`](NativeCx::defeat_investigator) and announcing the
//! trauma for the one investigator it runs for. The defeat-and-trauma body has
//! one consumer, so it fails the DSL-primitive threshold and stays native (ADR
//! 0013). The walk runs in **turn order**, and the order is observable on a
//! defeat body: Elimination step 5 reassigns the lead when the lead is
//! eliminated, and step 6's no-remaining-players check fires on whoever falls
//! last. Each investigator's Elimination — including one a *"when the game
//! ends"* weakness such as Cover Up 01007 routes onto a
//! `Continuation::Elimination` frame — drains before the walk reaches
//! the next investigator.
//!
//! **The ending is reached by the rules' route, not latched.** Each defeat runs
//! Rules Reference p.10 Elimination, and step 6 — *"If there are no remaining
//! players, the scenario ends. Refer to "no resolution was reached" entry for
//! that scenario in the campaign guide."* — is what produces
//! [`ScenarioEnding::NoResolution`](game_core::scenario::ScenarioEnding::NoResolution).
//! The card names no ending on this branch, because the card prints none.
//!
//! *"That has not resigned"* needs no filter of the card's own: the walk only
//! reaches investigators still `Active`, and a resigned one is not.
//! [`defeat_investigator`](NativeCx::defeat_investigator) no-ops on a
//! non-`Active` investigator as well, and the body reads the status anyway, so
//! that the trauma announcement fires only for someone this card actually
//! defeated.
//!
//! **A defeat by a card ability is neither killed nor driven insane.**
//! `glossary/Defeat.md`: *"An investigator might also be defeated by a card
//! ability."* The same entry makes killed and insane consequences of **trauma**
//! — *"Taking trauma may cause an investigator to be killed or driven insane"* —
//! and these investigators take one physical trauma, the first step on that
//! track rather than its end. So the defeat carries
//! [`game_core::state::EliminationCause::CardAbility`] and leaves
//! [`game_core::state::Status::Defeated`], both distinct from the damage
//! and horror values.
//!
//! **The physical trauma is announced, not recorded.** `Event::TraumaSuffered`
//! is emitted per defeated investigator, following Cover Up 01007's
//! mental-trauma precedent. TODO(#766): record it in the campaign log.
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
//! Parlor, so the lowest-`LocationId` tie-break below is unreachable in this
//! scenario. The move goes through
//! [`relocate_enemy`](NativeCx::relocate_enemy), so a Ghoul arriving at an
//! investigator's location engages on arrival per the general engagement rule
//! — the card text is positional only, but the framework rule applies
//! regardless.
//!
//! # Module gap
//!
//! TODO(#1026): the walk is sequential, and this body's intermediate state is
//! visible. The rules resolve *"each investigator … is defeated"*
//! simultaneously (`glossary/Priority_of_Simultaneous_Resolution.md`: *"Once all
//! necessary choices have been made, the effect resolves simultaneously upon
//! all affected entities."*), while the walk defeats one investigator at a time,
//! so the lead transfer and the step-6 check above see a table that is part
//! defeated.
//!
//! A tie between first steps toward the Parlor is broken by the lowest
//! `LocationId`, where the rules have the controlling player choose (RR p.12).
//! A player's choice on a tie is not modelled; The Gathering's map has no tie.

use card_dsl::dsl::{self, Ability, EventPattern, EventTiming, InvestigatorTargetSet, Phase};
use game_core::engine::evaluator::{self, EvalContext};
use game_core::engine::{self, EngineOutcome, NativeCx};
use game_core::event::TraumaKind;
use game_core::state::{Enemy, EnemyId, GameState, LocationId, Status};

use crate::generated::traits::GHOUL;
use crate::impls::CardRecord;

/// `ArkhamDB` code for Agenda 3, "They're Getting Out!".
pub const CODE: &str = "01107";

/// This card's registration, listed in [`ALL`](super::ALL). Its act-three
/// branch is one of the two [`CardRecord::conditions`] registrants.
pub const CARD: CardRecord = CardRecord::new(CODE, abilities)
    .effects(&[
        (MOVE_GHOULS, move_ghouls_toward_parlor),
        (ROUND_END_DOOM, place_round_end_doom),
        (DEFEATED_WITH_TRAUMA, defeated_with_trauma),
    ])
    .conditions(&[(AT_ACT_THREE, at_act_three)]);

const MOVE_GHOULS: &str = "01107:move-ghouls";
const ROUND_END_DOOM: &str = "01107:round-end-doom";
const AT_ACT_THREE: &str = "01107:at-act-three";
const DEFEATED_WITH_TRAUMA: &str = "01107:defeated-with-trauma";

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
        dsl::forced_on_event(
            EventPattern::PhaseEnded {
                phase: Phase::Enemy,
            },
            EventTiming::At,
            dsl::native(MOVE_GHOULS),
        ),
        dsl::forced_on_event(
            EventPattern::RoundEnded,
            EventTiming::At,
            dsl::native(ROUND_END_DOOM),
        ),
        dsl::forced_on_event(
            EventPattern::AgendaAdvanced,
            EventTiming::After,
            dsl::if_else(
                dsl::native_condition(AT_ACT_THREE),
                dsl::for_each(
                    InvestigatorTargetSet::All,
                    dsl::native(DEFEATED_WITH_TRAUMA),
                ),
                dsl::reach_resolution(3),
            ),
        ),
    ]
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
/// is defeated and suffers 1 physical trauma."* — the body the reverse's
/// `ForEach` runs for each investigator, in turn order.
///
/// **No resolution point.** This branch prints no `(→R#)`, and this function
/// latches nothing: defeating the last active investigator drains Rules
/// Reference p.10 Elimination step 6 — *"If there are no remaining players, the
/// scenario ends"* — which is what produces `ScenarioEnding::NoResolution`.
///
/// **The `Active` gate is *"that has not resigned"*.** The walk already skips a
/// non-`Active` investigator, and [`defeat_investigator`] no-ops on one too;
/// reading the status here is what keeps the trauma announcement from firing
/// for someone who was never defeated by this card, whoever runs the body.
fn defeated_with_trauma(cx: &mut NativeCx, ctx: &EvalContext) -> EngineOutcome {
    let Some(investigator) = ctx.each_investigator() else {
        return EngineOutcome::Rejected {
            reason: "01107 defeated-with-trauma: run outside its ForEach over investigators".into(),
        };
    };
    if cx
        .state()
        .investigators
        .get(&investigator)
        .map(|i| i.status)
        != Some(Status::Active)
    {
        return EngineOutcome::Done;
    }
    cx.defeat_investigator(investigator);
    // Announced, not recorded: nothing persists trauma (TODO(#766): record it
    // in the campaign log). Cover Up 01007's mental trauma is the precedent.
    cx.suffer_trauma(investigator, TraumaKind::Physical, 1);
    EngineOutcome::Done
}

/// Whether `enemy`'s printed card has the `Ghoul` trait.
fn is_ghoul(enemy: &Enemy) -> bool {
    crate::has_trait(enemy.code.as_str(), GHOUL)
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
/// whole apply back, it would land on the *player's* action and make the
/// scenario unplayable. Same reading as the barricaded first step
/// below: no move available means no move, never an error.
fn move_ghouls_toward_parlor(cx: &mut NativeCx, _ctx: &EvalContext) -> EngineOutcome {
    let Some(parlor) = evaluator::location_id_by_code(cx.state(), PARLOR) else {
        return EngineOutcome::Done;
    };
    // Scan first (shared borrows), then mutate. Deterministic lowest-
    // LocationId tie-break among shortest first steps.
    let mut movers: Vec<(EnemyId, LocationId)> = Vec::new();
    for (id, e) in &cx.state().enemies {
        if e.engaged_with.is_some() || !is_ghoul(e) {
            continue;
        }
        let Some(from) = e.current_location else {
            continue;
        };
        // A non-Elite Ghoul cannot move into a barricaded location (Barricade
        // 01038), and the block bites at the *compelled step*, not on the
        // graph the path is measured over — as Hunter movement does.
        // 01107 names a fixed destination rather than a "nearest"
        // target, so `glossary/Nearest.md` does not reach it, but
        // `glossary/Patrol.md` carries the same rule on a fixed-destination,
        // shortest-path mover, which is structurally what 01107 is: *"If an
        // enemy with patrol would be compelled to move to a location which is
        // blocked by a card ability, the enemy does not move."* So a
        // barricaded first step is a non-move, never a detour.
        let mut steps: Vec<LocationId> = engine::shortest_first_steps(cx.state(), from, parlor)
            .into_iter()
            .filter(|&loc| engine::enemy_can_enter_location(cx.state(), cx.cards(), e, loc))
            .collect();
        steps.sort_unstable();
        if let Some(&to) = steps.first() {
            movers.push((*id, to));
        }
    }
    for (id, to) in movers {
        // The shared relocation funnel: the board write, the
        // `EnemyMoved` emit, and the engage-on-arrival check the general
        // engagement rule requires of *any* enemy movement — an exhausted
        // (evaded) Ghoul moved by this agenda still arrives unengaged.
        cx.relocate_enemy(id, to);
    }
    EngineOutcome::Done
}

/// Place 1 doom on the agenda per Ghoul in the Hallway (01112) or Parlor
/// (01115). Not filtered by engagement (per card text). No threshold
/// check — RR p.24 checks doom in Mythos step 1.3. Placed through the
/// engine's `place_doom_on_agenda`, so an empty agenda deck takes none.
fn place_round_end_doom(cx: &mut NativeCx, _ctx: &EvalContext) -> EngineOutcome {
    let counted: Vec<LocationId> = [HALLWAY, PARLOR]
        .iter()
        .filter_map(|c| evaluator::location_id_by_code(cx.state(), c))
        .collect();
    let count = cx
        .state()
        .enemies
        .values()
        .filter(|e| is_ghoul(e))
        .filter(|e| e.current_location.is_some_and(|l| counted.contains(&l)))
        .count();
    let count = u8::try_from(count).unwrap_or(u8::MAX);
    cx.place_doom_on_agenda(count);
    EngineOutcome::Done
}

#[cfg(test)]
mod tests {
    use card_dsl::dsl::{Condition, Effect, ForEachTargets, Trigger, TriggerKind};
    use game_core::card_registry::NativeEffectFn;
    use game_core::engine::evaluator::EachBinding;
    use game_core::event::Event;
    use game_core::state::{
        Agenda, CardCode, CardInPlay, Enemy, GameStateBuilder, InvestigatorId, Location, Owner,
    };
    use game_core::test_support;

    use super::*;
    use crate::REGISTRY;

    /// Ghoul Minion 01160 (Humanoid. Monster. Ghoul.), not Elite.
    fn ghoul(id: u32, at: LocationId) -> Enemy {
        let mut e = test_support::test_enemy(id, "Ghoul");
        e.code = CardCode::new("01160");
        e.current_location = Some(at);
        e
    }

    // Hallway(2) hub connects to Attic(3), Cellar(4), Parlor(5).
    fn star_board() -> GameState {
        star_board_from(GameStateBuilder::new().with_registries(crate::TEST_REGISTRIES))
    }

    /// The [`star_board`] with investigator 1 seated in the Hallway.
    fn star_board_with_investigator_in_hallway() -> GameState {
        star_board_from(
            GameStateBuilder::new()
                .with_registries(crate::TEST_REGISTRIES)
                .with_seat(test_support::seat(1).at(LocationId(2)))
                .with_turn_order([InvestigatorId(1)]),
        )
    }

    /// The [`star_board`]'s map on top of `builder`.
    fn star_board_from(builder: GameStateBuilder) -> GameState {
        let loc =
            |id, code: &str, name| Location::new(LocationId(id), CardCode::new(code), name, 1, 0);
        let mut state = builder
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

    /// Run `f` for investigator 1, assert it finished, and return its events.
    fn run_done(state: &mut GameState, f: NativeEffectFn) -> Vec<Event> {
        let (out, events) = test_support::run_native(
            state,
            &REGISTRY,
            f,
            &EvalContext::for_controller(InvestigatorId(1)),
        );
        assert_eq!(out, EngineOutcome::Done);
        events
    }

    /// Run the act-3 body for `investigator`, bound the way the reverse's
    /// `ForEach` binds it, assert it finished, and return its events.
    fn run_body_for(state: &mut GameState, investigator: InvestigatorId) -> Vec<Event> {
        let mut ctx = EvalContext::for_controller(InvestigatorId(1));
        ctx.set_each(EachBinding::Investigator(investigator));
        let (out, events) = test_support::run_native(state, &REGISTRY, defeated_with_trauma, &ctx);
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

    /// Investigators 1 and 2, seated in that turn order.
    fn two_investigators() -> GameState {
        GameStateBuilder::new()
            .with_registries(crate::TEST_REGISTRIES)
            .with_seat(test_support::seat(1))
            .with_seat(test_support::seat(2))
            .with_turn_order([InvestigatorId(1), InvestigatorId(2)])
            .build()
    }

    fn with_agenda(state: &mut GameState) {
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
                kind: TriggerKind::Forced,
            }
        );
        assert!(matches!(&abilities[0].effect, Effect::Native { tag } if tag == MOVE_GHOULS));
        assert_eq!(
            abilities[1].trigger,
            Trigger::OnEvent {
                pattern: EventPattern::RoundEnded,
                timing: EventTiming::At,
                kind: TriggerKind::Forced,
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
                kind: TriggerKind::Forced,
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
            matches!(condition, Condition::Native { tag } if tag == AT_ACT_THREE),
            "branches on the card-local act-3 predicate, got {condition:?}",
        );
        let Effect::ForEach { targets, body } = &**then else {
            panic!("act 3 fans out over the investigators, got {then:?}");
        };
        assert_eq!(
            *targets,
            ForEachTargets::Investigators(InvestigatorTargetSet::All),
            "act 3: each investigator",
        );
        assert!(
            matches!(&**body, Effect::Native { tag } if tag == DEFEATED_WITH_TRAUMA),
            "act 3: each is defeated and suffers trauma, got {body:?}",
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

    /// The act-3 body, for the investigator it runs for: defeated by a card
    /// ability, announcing 1 physical trauma, and nobody else touched. The walk
    /// over the table — turn order, the resigned seat skipped, and the
    /// no-resolution ending — is pinned end to end in
    /// `tests/theyre_getting_out.rs`.
    #[test]
    fn the_act_three_body_defeats_its_investigator_with_one_physical_trauma() {
        let (a, b) = (InvestigatorId(1), InvestigatorId(2));
        let mut state = two_investigators();

        let events = run_body_for(&mut state, b);

        assert_eq!(
            state.investigators[&b].status,
            Status::Defeated,
            "defeated by a card ability — neither killed nor driven insane",
        );
        assert_eq!(state.investigators[&a].status, Status::Active);
        assert_eq!(defeat_order(&events), vec![b]);
        assert!(
            events.iter().any(|e| matches!(e, Event::TraumaSuffered {
                investigator, kind, amount
            } if *investigator == b
                && *kind == TraumaKind::Physical
                && *amount == 1)),
            "the defeated investigator suffers 1 physical trauma",
        );
    }

    /// *"Each investigator that has not resigned"* — run for a resigned
    /// investigator, the body leaves them alone and announces no trauma.
    #[test]
    fn an_investigator_who_has_resigned_is_left_alone() {
        let a = InvestigatorId(1);
        let mut state = two_investigators();
        state.investigators.get_mut(&a).expect("seated").status = Status::Resigned;

        let events = run_body_for(&mut state, a);

        assert_eq!(state.investigators[&a].status, Status::Resigned);
        assert!(defeat_order(&events).is_empty());
        assert!(
            !events.iter().any(|e| matches!(
                e,
                Event::TraumaSuffered { investigator, .. } if *investigator == a
            )),
            "no trauma announced for an investigator this card never defeated",
        );
    }

    /// Regression for #811: no Parlor on the board is a **no-op**, not a
    /// rejection: `Done`, no events, nothing moved. The reachable case is
    /// agenda 3 turning up while the act deck is still on act 1 or 2 — see the
    /// function's own doc for why. `run_done` already asserts the `Done`.
    #[test]
    fn no_parlor_in_play_moves_nothing_and_does_not_reject() {
        let mut state = star_board();
        state.locations.remove(&LocationId(5)); // the Parlor
        state.enemies.insert(EnemyId(1), ghoul(1, LocationId(3))); // Attic
        let events = run_done(&mut state, move_ghouls_toward_parlor);
        assert!(events.is_empty(), "no events: {events:?}");
        assert_eq!(
            state.enemies[&EnemyId(1)].current_location,
            Some(LocationId(3)),
            "the Ghoul stays put — there is no destination to step toward",
        );
    }

    #[test]
    fn unengaged_ghoul_in_attic_steps_to_hallway() {
        let mut state = star_board();
        state.enemies.insert(EnemyId(1), ghoul(1, LocationId(3))); // Attic
        let events = run_done(&mut state, move_ghouls_toward_parlor);
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
        run_done(&mut state, move_ghouls_toward_parlor);
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
        let mut state = star_board();
        state.enemies.insert(EnemyId(1), ghoul(1, LocationId(2))); // Hallway
        barricade(&mut state, LocationId(5));
        run_done(&mut state, move_ghouls_toward_parlor);
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
    fn detour_board() -> GameState {
        let loc =
            |id, code: &str, name| Location::new(LocationId(id), CardCode::new(code), name, 1, 0);
        let mut state = GameStateBuilder::new()
            .with_registries(crate::TEST_REGISTRIES)
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

    fn barricade(state: &mut GameState, at: LocationId) {
        // A seat cannot place a location attachment; mint its id from the
        // state's counter rather than hand-pick one.
        let id = state.card_instance_ids.mint();
        state
            .locations
            .get_mut(&at)
            .unwrap()
            .attachments
            .push(CardInPlay::enter_play(
                CardCode::new("01038"),
                id,
                Owner::Investigator(InvestigatorId(1)),
            ));
    }

    /// Regression for #797: a barricaded compelled step is a non-move, not a
    /// detour. `glossary/Patrol.md`: *"If an enemy with patrol would be
    /// compelled to move to a location which is blocked by a card ability, the
    /// enemy does not move."* 01107 is structurally the same fixed-destination,
    /// shortest-path mover.
    #[test]
    fn ghoul_does_not_reroute_around_a_barricaded_shortest_step() {
        let mut state = detour_board();
        state.enemies.insert(EnemyId(1), ghoul(1, LocationId(3))); // Attic
        barricade(&mut state, LocationId(2)); // Hallway — the only shortest step

        let events = run_done(&mut state, move_ghouls_toward_parlor);

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
        run_done(&mut state, move_ghouls_toward_parlor);
        assert_eq!(
            state.enemies[&EnemyId(1)].current_location,
            Some(LocationId(2)),
        );
    }

    #[test]
    fn a_barricade_off_the_shortest_path_does_not_affect_the_move() {
        let mut state = detour_board();
        state.enemies.insert(EnemyId(1), ghoul(1, LocationId(3))); // Attic
        barricade(&mut state, LocationId(6)); // Study — on the long route only
        run_done(&mut state, move_ghouls_toward_parlor);
        assert_eq!(
            state.enemies[&EnemyId(1)].current_location,
            Some(LocationId(2)),
        );
    }

    /// Regression for #633: a Ghoul that steps into a lone investigator's
    /// location engages on arrival. `Enemy_Engagement.md`: *"Any time a ready unengaged
    /// enemy is at the same location as an investigator, it engages that
    /// investigator"*, listed example *"It moves into the same location as
    /// an investigator"*.
    #[test]
    fn ghoul_moving_into_the_investigators_location_engages_on_arrival() {
        let mut state = star_board_with_investigator_in_hallway();
        state.enemies.insert(EnemyId(1), ghoul(1, LocationId(3))); // Attic

        let events = run_done(&mut state, move_ghouls_toward_parlor);

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
    /// unengaged and engages only when it readies.
    #[test]
    fn exhausted_ghoul_moved_into_the_investigators_location_arrives_unengaged() {
        let mut state = star_board_with_investigator_in_hallway();
        let mut evaded = ghoul(1, LocationId(3)); // Attic
        evaded.exhausted = true;
        state.enemies.insert(EnemyId(1), evaded);

        let events = run_done(&mut state, move_ghouls_toward_parlor);

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
        run_done(&mut state, move_ghouls_toward_parlor);
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
        // Swarm of Rats 01159 (Creature.): traits, but not Ghoul.
        let mut e = test_support::test_enemy(1, "Rat");
        e.code = CardCode::new("01159");
        e.current_location = Some(LocationId(3));
        state.enemies.insert(EnemyId(1), e);
        run_done(&mut state, move_ghouls_toward_parlor);
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

        // Swarm of Rats 01159 (Creature.): traits, but not Ghoul.
        let mut non_ghoul = test_support::test_enemy(4, "Rat");
        non_ghoul.code = CardCode::new("01159");
        non_ghoul.current_location = Some(LocationId(2));
        state.enemies.insert(EnemyId(4), non_ghoul); // Hallway non-Ghoul — no
        run_done(&mut state, place_round_end_doom);
        assert_eq!(state.agenda_doom, 2);
    }

    #[test]
    fn engaged_ghoul_in_hallway_still_counts_for_doom() {
        let mut state = star_board();
        with_agenda(&mut state);
        let mut engaged = ghoul(1, LocationId(2)); // Hallway, engaged
        engaged.engaged_with = Some(InvestigatorId(1));
        state.enemies.insert(EnemyId(1), engaged);
        run_done(&mut state, place_round_end_doom);
        assert_eq!(state.agenda_doom, 1);
    }

    /// The empty-agenda-deck guard: with no agenda to place doom on, a Ghoul
    /// in the Hallway places none.
    #[test]
    fn round_end_doom_on_an_empty_agenda_deck_is_a_no_op() {
        let mut state = star_board();
        assert!(state.agenda_deck.is_empty());
        state.enemies.insert(EnemyId(1), ghoul(1, LocationId(2))); // Hallway
        run_done(&mut state, place_round_end_doom);
        assert_eq!(state.agenda_doom, 0);
    }
}
