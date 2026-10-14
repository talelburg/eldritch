//! "They're Getting Out!" (agenda 01107) forced abilities through the real
//! card registry: the enemy-phase-end move + round-end doom fire via the
//! forced-trigger path and the `Effect::Native` bridge end-to-end. Also
//! covers round-end ordering — the act's "when the round ends" window
//! resolves before this agenda's "at the end of the round" doom (RR `when`
//! before `at`).
//!
//! And the **reverse's branch** (#809), through the same real registry: which of
//! the two printed bullets fires depends on the act cursor, and only the first
//! carries a `(→R#)`. Driven the way play reaches it: the table ends its turns
//! one doom short of the threshold, and the next Mythos phase's doom advances
//! the agenda, firing the `AgendaAdvanced` timing point its reverse hangs on.
//! The scenario's own act and agenda progression is
//! `scenarios/tests/the_gathering_resolutions.rs`'s job; this is the seam that
//! carries the *multi-investigator* claims the solo end-to-end cannot: the fan-out
//! order, and the investigator it must leave alone.

use cards::TEST_REGISTRIES;
use game_core::engine::enumerate::TurnAction;
use game_core::engine::OptionTarget;
use game_core::event::{Event, TraumaKind};
use game_core::scenario::{ResolutionId, ScenarioEnding};
use game_core::state::{
    Act, Agenda, CardCode, EliminationCause, Enemy, EnemyId, GameState, GameStateBuilder,
    InvestigatorId, Location, LocationId, Phase, Status,
};
use game_core::test_support::{self, seat, Seat, TestSession};
use game_core::{assert_event, assert_no_event};

const INV: InvestigatorId = InvestigatorId(1);
const HALLWAY: LocationId = LocationId(2);
const ATTIC: LocationId = LocationId(3);
const PARLOR: LocationId = LocationId(5);
const GHOUL: EnemyId = EnemyId(1);
const SECOND_GHOUL: EnemyId = EnemyId(2);

/// Ghoul Minion 01160 (Humanoid. Monster. Ghoul.), not Elite.
fn ghoul(id: EnemyId, at: LocationId) -> Enemy {
    let mut e = test_support::test_enemy(id.0, "Ghoul");
    e.code = CardCode::new("01160");
    e.current_location = Some(at);
    e
}

/// The agenda board — 01107 current, a Hallway and a Parlor, `investigator`
/// seated alone — on top of `builder`, which names the moment.
fn board_from(builder: GameStateBuilder, investigator: Seat) -> GameState {
    let loc = |id: LocationId, code: &str, name| Location::new(id, CardCode::new(code), name, 1, 0);
    let mut state = builder
        .with_seat(investigator)
        .with_turn_order([INV])
        .with_location(loc(HALLWAY, "01112", "Hallway"))
        .with_location(loc(PARLOR, "01115", "Parlor"))
        .build();
    state.connect(HALLWAY, PARLOR);
    state.agenda_deck = vec![Agenda {
        code: CardCode::new("01107"),
        doom_threshold: 10,
    }];
    state.agenda_index = 0;
    state
}

/// Regression (#569): the move must fire through the *real* step-3.4 site, not
/// only when its timing point is fired on its own. `enemy_phase_end` queues
/// the agenda's forced ability as a frame and returns `Done`; before the fix it
/// read that `Done` as "nothing happened" and pushed the Upkeep anchor on top of
/// the queued frame, orphaning it at the bottom of the stack for the rest of the
/// scenario — agenda 3's Ghoul movement never happened in real play.
///
/// Also the plain claim: the Enemy phase's end moves a Ghoul toward the Parlor.
#[test]
fn enemy_phase_end_moves_ghoul_before_the_upkeep_transition() {
    // The step-3.4 site runs once the Enemy phase's attacks are done. One Ghoul
    // in the Hallway, one step from the Parlor. The cascade runs on into
    // Upkeep: give the investigator a card to draw.
    let session = TestSession::new(
        board_from(
            GameStateBuilder::new()
                .with_registries(TEST_REGISTRIES)
                .ending_enemy_phase()
                .with_enemy(ghoul(GHOUL, HALLWAY)),
            seat(1).at(HALLWAY).deck(["01006"]),
        ),
        TEST_REGISTRIES,
    );
    let (state, events) = (session.state(), session.events());

    assert_eq!(
        state.enemies[&GHOUL].current_location,
        Some(PARLOR),
        "Ghoul stepped Hallway -> Parlor at the end of the enemy phase"
    );
    // Ordering is the defect: the queued ability resolves BEFORE the phase
    // transition's tail work, not after it (and not never).
    let moved = events
        .iter()
        .position(|e| matches!(e, Event::EnemyMoved { to, .. } if *to == PARLOR))
        .expect("the Ghoul move must fire at the enemy phase end");
    let upkeep_started = events
        .iter()
        .position(|e| {
            matches!(
                e,
                Event::PhaseStarted {
                    phase: Phase::Upkeep
                }
            )
        })
        .expect("the Enemy -> Upkeep transition must still run");
    assert!(
        moved < upkeep_started,
        "the queued forced ability must resolve before the Upkeep phase begins; \
         events = {events:?}"
    );
}

/// #633: a Ghoul the agenda walks into the investigator's location engages on
/// arrival, and — being engaged and ready — attacks in the *next* Enemy phase.
///
/// `glossary/Enemy_Engagement.md`: *"Any time a ready unengaged enemy is at the
/// same location as an investigator, it engages that investigator, and is
/// placed in that investigator's threat area"*, listing *"It moves into the
/// same location as an investigator"*. The Ghoul here is not a Hunter, so
/// nothing else in the framework would ever pick it up: before the fix it stood
/// in the Hallway beside the investigator forever.
#[test]
fn ghoul_moved_into_the_investigator_engages_then_attacks_next_enemy_phase() {
    let mut walker = ghoul(GHOUL, ATTIC); // Attic, one step from the Hallway
    walker.attack_damage = 1;
    walker.attack_horror = 0;
    assert!(!walker.hunter, "the divergence needs a non-Hunter Ghoul");
    // Step 3.4 runs once the Enemy phase's attacks are done. The Attic (01113)
    // is a third room so a Ghoul can step Attic -> Hallway on its way to the
    // Parlor.
    let mut state = board_from(
        GameStateBuilder::new()
            .with_registries(TEST_REGISTRIES)
            .ending_enemy_phase()
            .with_location(Location::new(ATTIC, CardCode::new("01113"), "Attic", 1, 0))
            .with_enemy(walker),
        seat(1).at(HALLWAY).deck(["01088"]),
    );
    state.connect(HALLWAY, ATTIC);
    // The next Mythos phase draws an encounter card. Obscuring Fog 01168 —
    // *"Revelation - Attach to your location. Limit 1 per location."* — leaves
    // the attack this test is about alone.
    state.encounter_deck.push_back(CardCode::new("01168"));

    let session = TestSession::new(state, TEST_REGISTRIES);
    let (state, events) = (session.state(), session.events());

    assert_eq!(
        state.enemies[&GHOUL].current_location,
        Some(HALLWAY),
        "the Ghoul stepped Attic -> Hallway"
    );
    assert_eq!(
        state.enemies[&GHOUL].engaged_with,
        Some(INV),
        "and engaged the investigator it arrived on top of"
    );
    assert!(
        events.iter().any(|e| matches!(e,
            Event::EnemyEngaged { enemy, investigator }
                if *enemy == GHOUL && *investigator == INV)),
        "engage-on-arrival is announced: {events:?}"
    );

    // The follow-up Enemy phase: hand the (still engaged) Ghoul a round in
    // which to attack. Play runs on from the phase end through Upkeep and the
    // next Mythos phase, whose step-1.4 encounter draw is the one prompt on the
    // way to the investigator's next turn; ending that turn, the cascade runs
    // Investigation -> Enemy and resolves step 3.3.
    let damage_before = state.investigators[&INV].damage();
    let session = session.confirm();
    // A ready enemy attacks; the Upkeep readying in between would have done
    // this anyway, but assert the precondition rather than assume it.
    assert!(!session.state().enemies[&GHOUL].exhausted);
    session.assert_at_turn_menu(INV);

    let before = session.events().len();
    let result = session.take(&TurnAction::EndTurn);
    let events = &result.events()[before..];

    assert!(
        events.iter().any(|e| matches!(
            e,
            Event::DamageTaken { investigator, amount: 1 } if *investigator == INV
        )),
        "the now-engaged Ghoul attacks in the next Enemy phase: {events:?}",
    );
    assert!(
        result.state().investigators[&INV].damage() > damage_before,
        "the attack landed"
    );
}

#[test]
fn round_end_places_doom_per_ghoul_in_hallway_or_parlor() {
    // The round ends once the Upkeep phase does.
    let session = TestSession::new(
        board_from(
            GameStateBuilder::new()
                .with_registries(TEST_REGISTRIES)
                .ending_upkeep_phase()
                .with_enemy(ghoul(GHOUL, HALLWAY))
                .with_enemy(ghoul(SECOND_GHOUL, PARLOR)),
            seat(1),
        ),
        TEST_REGISTRIES,
    );
    assert_event!(
        session.events(),
        Event::PhaseEnded {
            phase: Phase::Upkeep
        }
    );
    // 3, not 2: the round end runs on into the next round's Mythos, whose step
    // 1.2 places a doom of its own.
    assert_eq!(
        session.state().agenda_doom,
        3,
        "1 doom per Ghoul in Hallway/Parlor (plus Mythos step 1.2's)"
    );
}

#[test]
fn round_end_act_when_window_opens_before_agenda_at_doom() {
    // Act 01109 ("The Barrier") carries the "when the round ends" clue-spend
    // window; agenda 01107 carries the "at the end of the round" doom. Per the
    // RR "At" entry, `when` resolves before `at`, so the act window must open
    // BEFORE any doom is placed.
    //
    // Affordable act window: investigator in the Hallway (01112) with >= 3
    // clues. Two Ghouls in Hallway/Parlor -> agenda 01107 would place 2 doom.
    let mut state = board_from(
        GameStateBuilder::new()
            .with_registries(TEST_REGISTRIES)
            .ending_upkeep_phase()
            .with_enemy(ghoul(GHOUL, HALLWAY))
            .with_enemy(ghoul(SECOND_GHOUL, PARLOR)),
        seat(1).at(HALLWAY).clues(3),
    );
    state.act_deck = vec![Act {
        code: CardCode::new("01109"),
        clue_threshold: 3,
    }];
    state.act_index = 0;

    let window = TestSession::new(state, TEST_REGISTRIES);

    // The act's `when the round ends` window opens first...
    let offered: Vec<_> = window
        .prompt()
        .options
        .iter()
        .map(|o| o.target.clone())
        .collect();
    assert_eq!(offered, vec![Some(OptionTarget::Act)]);
    // ...and the agenda's `at the end of the round` doom is NOT placed yet.
    assert_eq!(
        window.state().agenda_doom,
        0,
        "`when` resolves before `at`: doom must wait for the act window"
    );

    // Declining the `when` window then runs the `at` doom (2: one per Ghoul in
    // Hallway/Parlor). The exact total isn't pinned because the Skip cascades
    // through step_phase into the next Mythos phase, whose step 1.2 places a
    // further doom on the agenda — so `>= 2` (the `at` doom landed) is the
    // assertion that isolates this test's concern from the downstream cascade.
    let declined = window.skip();
    assert!(
        declined.state().agenda_doom >= 2,
        "the `at` doom lands after the act window resolves"
    );
}

// ---------------------------------------------------------------------------
// The reverse's branch (#809)
// ---------------------------------------------------------------------------

/// A table of `n` seated investigators in turn order, the first's turn open,
/// with The Gathering's three-card act deck and 01107 as the sole (and
/// therefore terminal) agenda, one doom short of its threshold — so the next
/// Mythos phase's step 1.2 doom tips it at 1.3 and its reverse fires.
fn table_at_act(n: u32, act_index: usize) -> GameState {
    // A card each for Upkeep 4.4 to draw.
    let mut state = (1..=n)
        .map(|i| seat(i).deck(["01088"]))
        .fold(
            GameStateBuilder::new().with_registries(TEST_REGISTRIES),
            GameStateBuilder::with_seat,
        )
        .with_turn_order((1..=n).map(InvestigatorId))
        .open_turn(INV)
        .build();
    state.act_deck = ["01108", "01109", "01110"]
        .iter()
        .map(|code| Act {
            code: CardCode::new(*code),
            clue_threshold: 2,
        })
        .collect();
    state.act_index = act_index;
    state.agenda_deck = vec![Agenda {
        code: CardCode::new("01107"),
        doom_threshold: 10,
    }];
    state.agenda_index = 0;
    state.agenda_doom = 9;
    state
}

/// Play `state`'s round out: every investigator still in it ends their turn,
/// and the round runs on into the next Mythos phase, where 01107 advances and
/// its reverse — the `AgendaAdvanced` forced — resolves.
fn advance_01107(state: GameState) -> TestSession {
    let mut session = TestSession::new(state, TEST_REGISTRIES);
    while session.state().ending.is_none() {
        session = session.take(&TurnAction::EndTurn);
    }
    assert_event!(session.events(), Event::AgendaAdvanced { .. });
    session
}

fn defeat_order(events: &[Event]) -> Vec<InvestigatorId> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::InvestigatorEliminated { investigator, .. } => Some(*investigator),
            _ => None,
        })
        .collect()
}

/// *"If the investigators are at Act 1 or 2, they are trapped inside the house
/// as the ghouls tear them apart. **(→R3)**"* — both halves of that bullet reach
/// the printed resolution point, and nobody is defeated.
#[test]
fn agenda_01107_reverse_at_act_1_or_2_reaches_resolution_3() {
    for act_index in [0, 1] {
        let session = advance_01107(table_at_act(2, act_index));
        let events = session.events();

        assert_eq!(
            session.state().ending,
            Some(ScenarioEnding::Resolution(ResolutionId::new(3))),
            "act cursor {act_index} is the card's Act {}",
            act_index + 1,
        );
        assert!(
            defeat_order(events).is_empty(),
            "nobody is defeated on this branch: {events:?}",
        );
    }
}

/// *"If the investigators are at Act 3, they barely escape with their lives,
/// allowing the ghouls to run rampant. Each investigator that has not resigned
/// is defeated and suffers 1 physical trauma."*
///
/// Three seats with the middle one resigned. The fan-out runs in **turn order**
/// — observable here, where the solo end-to-end cannot see it — skips the
/// resigned investigator entirely, and ends the scenario at **no resolution
/// point**: the card prints no `(→R#)` on this bullet, and the ending arrives
/// via Rules Reference p.10 Elimination step 6, *"If there are no remaining
/// players, the scenario ends."*
#[test]
fn agenda_01107_reverse_at_act_3_defeats_the_unresigned_in_turn_order() {
    let (a, b, c) = (InvestigatorId(1), InvestigatorId(2), InvestigatorId(3));
    let mut state = table_at_act(3, 2);
    state.investigators.get_mut(&b).expect("seated").status = Status::Resigned;
    let session = advance_01107(state);
    let (state, events) = (session.state(), session.events());

    assert_eq!(
        defeat_order(events),
        vec![a, c],
        "turn order, and the resigned investigator is left alone",
    );
    for id in [a, c] {
        assert_eq!(
            state.investigators[&id].status,
            Status::Defeated,
            "defeated by a card ability — neither killed nor driven insane",
        );
        assert_event!(
            events,
            Event::InvestigatorEliminated { investigator, cause }
                if *investigator == id && *cause == EliminationCause::CardAbility
        );
        assert_event!(
            events,
            Event::TraumaSuffered { investigator, kind, amount }
                if *investigator == id && *kind == TraumaKind::Physical && *amount == 1
        );
    }
    assert_eq!(
        state.investigators[&b].status,
        Status::Resigned,
        "*that has not resigned*",
    );
    // No trauma for an investigator this card never defeated.
    assert_no_event!(
        events,
        Event::TraumaSuffered { investigator, .. } if *investigator == b
    );
    assert_event!(events, Event::AllInvestigatorsEliminated);
    assert_eq!(
        state.ending,
        Some(ScenarioEnding::NoResolution),
        "the act-3 bullet prints no (→R#)",
    );
}
