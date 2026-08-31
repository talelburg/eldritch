//! C7b — the Slice-1 "done" gate: drive solo Roland through The Gathering
//! to a genuine engine-latched Won and Lost resolution, against the real
//! `scenarios` + `cards` registries.
//!
//! Hybrid fidelity (see the C7b design spec): drive the cheap, deterministic
//! real progression and seed only the expensive preconditions, so the
//! resolution itself is always engine-latched. Test-determinism stand-ins
//! (a controlled chaos bag, a minimal roster deck, seeded health/act state)
//! are called out at their use sites.

use game_core::action::RosterEntry;
use game_core::engine::{apply, seat_and_open, EngineOutcome};
use game_core::event::{Event, TraumaKind};
use game_core::scenario::{ResolutionId, ScenarioEnding};
use game_core::state::{
    CardCode, ChaosBag, ChaosToken, EliminationCause, GameState, InvestigatorId, Status,
};
use game_core::test_support::take_turn_action;
use game_core::{assert_event, Action, InputResponse, PlayerAction, TurnAction};

const ROLAND: &str = "01001";
/// The Parlor, which enters play only via act 2 (01109)'s reverse.
const PARLOR: &str = "01115";
const INV: InvestigatorId = InvestigatorId(1);

#[ctor::ctor(unsafe)]
fn install() {
    let _ = game_core::scenario_registry::install(scenarios::REGISTRY);
    let _ = game_core::card_registry::install(cards::REGISTRY);
}

/// The Gathering set up + solo Roland seated and past the mulligan, ready
/// to act in the Investigation phase. Determinism stand-in: the random
/// Standard bag (which contains `AutoFail`) is replaced with a single-token
/// `Numeric(0)` bag so skill tests resolve predictably.
fn seated_roland() -> GameState {
    let mut state = scenarios::the_gathering::setup();
    // Stand-in: deterministic chaos bag (production serves Standard).
    state.chaos_bag = ChaosBag::new([ChaosToken::Numeric(0)]);

    // Stand-in: a minimal deck (the resolution paths don't read deck
    // contents). Eight copies of a real neutral event so the opening hand
    // of 5 draws cleanly.
    let roster = vec![RosterEntry {
        investigator: CardCode::new(ROLAND),
        deck: vec![CardCode::new("01088"); 8],
    }];
    // seat_and_open opens the mulligan prompt (AwaitingInput); each
    // investigator then submits a single mulligan (ResolveInput) before the
    // turn's actions begin.
    let started = seat_and_open(state, &roster);
    assert!(
        matches!(started.outcome, EngineOutcome::AwaitingInput { .. }),
        "seat_and_open opens the mulligan prompt, got {:?}",
        started.outcome
    );
    let after_mulligan = apply(
        started.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickMultiple { selected: vec![] },
        }),
    );
    assert!(matches!(
        after_mulligan.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
    after_mulligan.state
}

#[test]
fn solo_roland_is_seated_in_the_study_ready_to_act() {
    let state = seated_roland();
    assert_eq!(state.round, 1);
    assert!(
        state.investigators.contains_key(&INV),
        "Roland seated as investigator 1"
    );
    assert!(state.ending.is_none(), "no resolution latched at setup");
}

/// Ended with *no resolution reached* via the real all-investigators-defeated
/// latch: Roland is seeded one hit from death with an engaged Ghoul Minion,
/// then a real Enemy-phase attack defeats him and `check_all_eliminated` latches
/// [`ScenarioEnding::NoResolution`] — Rules Reference `Elimination` step 6,
/// which the campaign guide answers under "If no resolution was reached (each
/// investigator resigned or was defeated)". Not a loss: in campaign play the
/// players proceed "regardless of the outcome".
#[test]
fn enemy_attack_defeats_roland_and_latches_no_resolution() {
    use game_core::state::EnemyId;
    use game_core::test_support::test_enemy;

    let mut state = seated_roland();

    // Seed: Roland one hit from death. After cp2a, accumulated_damage is the
    // source of truth; max_health() reads from cards::REGISTRY (9 for Roland).
    {
        let roland = state.investigators.get_mut(&INV).expect("Roland seated");
        roland.investigator_card.accumulated_damage = roland.max_health() - 1;
    }
    let loc = state.investigators[&INV]
        .current_location
        .expect("Roland is at a location");

    // Seed: a Ghoul Minion engaged with Roland (the `test_enemy` fixture
    // defaults to attack_damage 1 ≥ his 1 remaining health → lethal).
    let enemy_id = EnemyId(900);
    let mut minion = test_enemy(900, "Ghoul Minion");
    minion.code = CardCode::new("01160");
    minion.current_location = Some(loc);
    minion.engaged_with = Some(INV);
    state.enemies.insert(enemy_id, minion);

    // Drive: end Roland's turn → tick into the Enemy phase → the engaged
    // enemy attacks → Roland defeated → no remaining players → NoResolution.
    let result = take_turn_action(state, &TurnAction::EndTurn);

    assert_event!(result.events, Event::AllInvestigatorsEliminated);
    assert_event!(result.events, Event::ScenarioResolved { .. });
    assert_eq!(
        result.state.ending,
        Some(ScenarioEnding::NoResolution),
        "no investigator remains, so no resolution point was reached",
    );
}

/// Drive acts 1 and 2 for real, leaving Roland in round 2's Investigation phase
/// with the terminal act 3 (01110) current and the real Ghoul Priest on the
/// board. Extracted so the test below reads as the terminal-act claim it makes.
fn advance_to_the_terminal_act(state: GameState) -> GameState {
    // --- Act 1 (real): spend clues to advance → the reverse builds the board
    // and relocates Roland to the Hallway (the act-2 contributor location).
    let advanced = take_turn_action(state, &TurnAction::AdvanceAct { investigator: INV });
    assert_eq!(advanced.state.act_index, 1, "act 1 advanced to act 2");

    // --- Act 2 (real): end the round → the C3d round-end clue-spend window
    // opens (Roland holds 3 clues in the Hallway) → Confirm spends them →
    // act 2 advances and its reverse spawns the real Ghoul Priest (01116).
    let round_end = take_turn_action(advanced.state, &TurnAction::EndTurn);
    assert!(
        matches!(round_end.outcome, EngineOutcome::AwaitingInput { .. }),
        "EndTurn should open the act-2 round-end window, got {:?}",
        round_end.outcome,
    );
    assert!(matches!(
        round_end.state.continuations.last(),
        Some(game_core::state::Continuation::TimingPointWindow {
            event: game_core::engine::TimingEvent::RoundEnded,
            mode: game_core::state::TimingMode::Reaction,
            ..
        })
    ));
    let after_confirm = apply(
        round_end.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(game_core::engine::OptionId(0)),
        }),
    );
    assert!(
        !matches!(after_confirm.outcome, EngineOutcome::Rejected { .. }),
        "round-end Confirm rejected: {:?}",
        after_confirm.outcome,
    );
    assert_eq!(
        after_confirm.state.act_index, 2,
        "act 2 advanced to the terminal act 3 (01110)"
    );

    // Round 2 begins in the Mythos phase; draw the seeded Ancient Evils
    // (1 doom) to advance into Investigation, where Roland can take the Fight.
    let mythos = apply(
        after_confirm.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::Confirm,
        }),
    );
    assert!(matches!(
        mythos.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
    mythos.state
}

/// Won via the real progression + defeat→advance→win latch. Drives act 1
/// (`AdvanceAct`) and act 2 (the C3d round-end clue-spend window) for real —
/// act 2's reverse spawns the **real** Ghoul Priest — then fights that
/// spawned Priest to trigger `what_have_you_done`'s forced advance on the terminal
/// act → `ScenarioEnding::Resolution(R1)`.
///
/// Two seeds, both off the resolution path: clues (acquiring them via
/// Attic/Cellar investigation is unit-tested elsewhere — the focus here is
/// the act-advancement chain), and the spawned Priest's health (solo Roland
/// has no weapon and 5 sanity, so he cannot out-damage a 5-health Retaliate
/// Hunter dealing 2 horror/attack without going insane first — the kill is
/// the one necessary shortcut). The encounter deck is emptied so round-2's
/// Mythos draw doesn't inject random interference.
#[test]
fn act_progression_and_ghoul_priest_defeat_reaches_r1() {
    drive_the_ghoul_priest_defeat_and_pick(0, 1);
}

/// The second printed bullet — *"This hell-pit is my home! No way are we
/// burning it! (→R2)"* — down the same real path. Before #775 no sequence of
/// play could reach it: the act's reverse latched R1 unconditionally.
///
/// The banner the player ends on is a pure function of this `ResolutionId`
/// (`crates/web/tests/board.rs::resolution_banner_names_the_resolution_point`),
/// so pinning the id here is what pins the two endings apart on screen.
#[test]
fn picking_the_second_bullet_reaches_r2_down_the_same_path() {
    drive_the_ghoul_priest_defeat_and_pick(1, 2);
}

/// Drive solo Roland to the Ghoul Priest's defeat, answer act 01110's printed
/// choice with `pick`, and assert the scenario ends at Resolution `expected`.
fn drive_the_ghoul_priest_defeat_and_pick(pick: u32, expected: u8) {
    let mut state = seated_roland();
    {
        // Seed: clues for both thresholds (act 1 = 2, act 2 = 3).
        let roland = state.investigators.get_mut(&INV).expect("Roland seated");
        roland.clues = 5;
    }
    // Seed: round-2's Mythos draws exactly one benign card — Ancient Evils
    // (01166), whose Revelation only places 1 doom (threshold 3, so it can't
    // advance to a loss), keeping the Mythos deterministic and harmless.
    state.encounter_deck.clear();
    state.encounter_deck.push_back(CardCode::new("01166"));

    let mut state = advance_to_the_terminal_act(state);

    // --- Seed only the spawned Priest's health + engagement (see doc above).
    let priest_id = {
        let priest = state
            .enemies
            .values_mut()
            .find(|e| e.code.as_str() == "01116")
            .expect("act 2's reverse spawned the real Ghoul Priest");
        priest.damage = priest.max_health - 1; // one hit from death
        priest.engaged_with = Some(INV);
        priest.id
    };
    // Ensure Roland is mid-Investigation with an action for the Fight.
    state
        .investigators
        .get_mut(&INV)
        .expect("Roland seated")
        .actions_remaining = 3;

    // --- Drive the defeating Fight: combat 4 + Numeric(0) ≥ fight 4 → success
    // → deal 1 → defeated → act 3 advances → its reverse reaches R1.
    let paused = take_turn_action(
        state,
        &TurnAction::Fight {
            investigator: INV,
            enemy: priest_id,
        },
    );
    assert!(
        matches!(paused.outcome, EngineOutcome::AwaitingInput { .. }),
        "Fight should pause at the commit window, got {:?}",
        paused.outcome,
    );
    let result = apply(
        paused.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickMultiple { selected: vec![] },
        }),
    );

    assert_event!(result.events, Event::EnemyDefeated { .. });
    // The terminal act flips like every other one before the ending lands, and
    // holds its cursor because there is no next act (ADR 0013).
    assert_event!(result.events, Event::ActAdvanced { from } if *from == 2);
    assert_eq!(result.state.act_index, 2, "terminal: cursor does not bump");

    // The flipped act asks the lead investigator which printed point to reach
    // (#775) — the scenario has not ended yet.
    let EngineOutcome::AwaitingInput { request, .. } = &result.outcome else {
        panic!("expected 01110's R1/R2 choice, got {:?}", result.outcome);
    };
    assert_eq!(
        request
            .options
            .iter()
            .map(|o| o.label.as_str())
            .collect::<Vec<_>>(),
        [
            "It was never much of a home. Burn it down! (→R1)",
            "This hell-pit is my home! No way are we burning it! (→R2)",
        ],
    );
    assert!(
        request
            .options
            .iter()
            .all(|o| o.target == Some(game_core::engine::OptionTarget::Act)),
        "the choice renders on the act card it is printed on (#555): {request:?}",
    );
    assert!(
        result.state.ending.is_none(),
        "nothing latches before the pick"
    );

    let result = apply(
        result.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(game_core::engine::OptionId(pick)),
        }),
    );
    assert_event!(result.events, Event::ScenarioResolved { .. });
    assert_eq!(
        result.state.ending,
        Some(ScenarioEnding::Resolution(ResolutionId::new(expected))),
        "act 01110's reverse reaches the campaign guide's (→R{expected})",
    );

    // #566: advancing act 3 reaches a resolution point, so the scenario ends
    // *during* the defeat that triggered it — and Roland Banks' "After you
    // defeat an enemy: Discover 1 clue at your location" window must never open.
    // Act 01110's ruling is explicit that its Forced objective "will trigger as
    // soon as you defeat the Ghoul Priest, before any 'After you defeat an
    // enemy' reactions can be used"; the queued reaction window sits *beneath*
    // that objective's effect frame, so only cancelling it on the way back down
    // honours the ruling.
    assert_eq!(
        result.outcome,
        EngineOutcome::Done,
        "the ended scenario must not surface Roland's after-defeat window",
    );
    assert!(
        result.state.continuations.is_empty(),
        "no stranded frames after the ending — in particular no open \
         after-defeat reaction window: {:?}",
        result.state.continuations,
    );
}

/// The open prompt's text. Panics with the outcome if nothing is awaiting input.
fn prompt_of(r: &game_core::engine::ApplyResult) -> &str {
    match &r.outcome {
        EngineOutcome::AwaitingInput { request, .. } => &request.prompt,
        other => panic!("expected a prompt, got {other:?}"),
    }
}

/// Answer whatever single-option prompt is open with `OptionId(0)`.
fn pick_single(state: GameState) -> game_core::engine::ApplyResult {
    apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(game_core::engine::OptionId(0)),
        }),
    )
}

/// Agenda 3 turns up on a doom clock that does not read the act deck, so a group
/// still on act 1 or 2 meets 01107's enemy-phase-end Forced with no Parlor on
/// the board. That is a no-op — `move_ghouls_toward_parlor`'s own doc carries the
/// rules argument — and what this pins is the part only a full turn can show:
/// because a rejection rolls the whole apply back (#161), the old `Rejected`
/// landed on the *player's* action, and the scenario was unplayable from the
/// moment agenda 3 turned up (#811). The no-op itself is pinned at the unit.
#[test]
fn the_terminal_agendas_ghoul_move_is_a_no_op_when_the_parlor_is_not_in_play() {
    let mut state = seated_roland();
    // Agenda 3 current while the act deck is still on act 1 — the reachable
    // board state, and the one the Parlor is absent on.
    state.agenda_index = 2;
    assert_eq!(state.act_index, 0, "still act 1: act 2's reverse never ran");
    assert!(
        !state.locations.values().any(|l| l.code.as_str() == PARLOR),
        "the Parlor is not in play",
    );
    state.encounter_deck.clear();

    let result = take_turn_action(state, &TurnAction::EndTurn);

    assert!(
        !matches!(result.outcome, EngineOutcome::Rejected { .. }),
        "the missing Parlor is a no-op, not a rejected player action: {:?}",
        result.outcome,
    );
    assert!(
        !result
            .events
            .iter()
            .any(|e| matches!(e, Event::EnemyMoved { .. })),
        "no Ghoul moved — there was nowhere to move: {:?}",
        result.events,
    );
}

/// Doomed out on the **terminal agenda** (01107): the agenda advances like any
/// other — `AgendaAdvanced` is emitted and its reverse fires — and the reverse
/// is what reaches the printed `(→R3)` (ADR 0013).
///
/// `back_text` verbatim (`data/arkhamdb-snapshot/pack/core/core_encounter.json`):
///
/// > - If the investigators are at Act 1 or 2, they are trapped inside the house
/// >   as the ghouls tear them apart. **(→R3)**
///
/// Roland is at act 1 here, so R3 is the branch the card prints for him; the
/// act-3 branch is the two tests below.
///
/// Before #808 this path never emitted `AgendaAdvanced` at all: the doom-threshold
/// check latched the field *instead of* advancing, so the board went quiet and the
/// player never saw which agenda ended their game.
///
/// Here rather than in `crates/cards/tests/` — which is where the rest of 01107's
/// integration coverage lives — because reaching agenda 3 needs
/// `scenarios::the_gathering::setup()`, and `scenarios` depends on `cards`, not the
/// other way round. This target installs `cards::REGISTRY` too, so it is the same
/// real-corpus seam.
#[test]
fn dooming_out_the_terminal_agenda_advances_it_and_its_reverse_reaches_r3() {
    let mut state = seated_roland();
    // Seed the terminal agenda as current, one doom short of its threshold, so
    // Mythos step 1.2's single doom tips it at 1.3.
    state.agenda_index = 2;
    assert_eq!(
        state.agenda_deck[2].code.as_str(),
        "01107",
        "agenda 3 is the terminal agenda"
    );
    state.agenda_doom = state.agenda_deck[2].doom_threshold - 1;
    // No encounter draws to interfere; the ending cancels 1.4 anyway.
    state.encounter_deck.clear();

    let result = take_turn_action(state, &TurnAction::EndTurn);

    assert_event!(result.events, Event::AgendaAdvanced { from } if *from == 2);
    assert_event!(result.events, Event::ScenarioResolved { .. });
    assert_eq!(
        result.state.ending,
        Some(ScenarioEnding::Resolution(ResolutionId::new(3))),
        "the reverse reached the campaign guide's Resolution 3",
    );
    assert_eq!(
        result.state.agenda_index, 2,
        "a terminal agenda does not bump the cursor — there is no next agenda",
    );
    assert!(
        result.state.continuations.is_empty(),
        "no stranded frames after the ending: {:?}",
        result.state.continuations,
    );
}

/// Act **2** is the other half of the card's first bullet — *"If the
/// investigators are at Act 1 or 2"* — so it reaches the same printed `(→R3)`.
/// Seeded rather than driven: the act-progression path is
/// [`act_progression_and_ghoul_priest_defeat_reaches_r1`]'s subject, and what
/// this pins is the branch predicate's boundary, one index below the act-3 arm.
#[test]
fn dooming_out_the_terminal_agenda_at_act_2_also_reaches_r3() {
    let mut state = seated_roland();
    state.act_index = 1; // the card's Act 2 (zero-based cursor)
    state.agenda_index = 2;
    state.agenda_doom = state.agenda_deck[2].doom_threshold - 1;
    state.encounter_deck.clear();

    let result = take_turn_action(state, &TurnAction::EndTurn);

    assert_eq!(
        result.state.ending,
        Some(ScenarioEnding::Resolution(ResolutionId::new(3))),
        "act 2 is still the (→R3) bullet",
    );
}

/// Dooming out at **act 3** reaches *no resolution point at all*.
///
/// `back_text` verbatim (`data/arkhamdb-snapshot/pack/core/core_encounter.json`):
///
/// > - If the investigators are at Act 3, they barely escape with their lives,
/// >   allowing the ghouls to run rampant. Each investigator that has not
/// >   resigned is defeated and suffers 1 physical trauma.
///
/// No `(→R#)` is printed on this bullet, and none is latched. Solo Roland is
/// defeated by the card ability, which drains Rules Reference p.10 Elimination
/// step 6 — *"If there are no remaining players, the scenario ends. Refer to 'no
/// resolution was reached' entry for that scenario in the campaign guide."* —
/// and *that* is what ends the scenario. The ending is
/// [`ScenarioEnding::NoResolution`], the campaign guide's own untitled entry,
/// rather than the Resolution 3 the act-1/2 branch reaches (#809).
#[test]
fn dooming_out_the_terminal_agenda_at_act_3_defeats_the_table_and_reaches_no_resolution() {
    let mut state = seated_roland();
    state.act_index = 2; // the card's Act 3 (zero-based cursor)
    assert_eq!(
        state.act_deck[2].code.as_str(),
        "01110",
        "act 3 is the terminal act"
    );
    state.agenda_index = 2;
    state.agenda_doom = state.agenda_deck[2].doom_threshold - 1;
    state.encounter_deck.clear();

    let result = take_turn_action(state, &TurnAction::EndTurn);

    assert_event!(result.events, Event::AgendaAdvanced { from } if *from == 2);
    assert_event!(
        result.events,
        Event::InvestigatorEliminated { investigator, cause }
            if *investigator == INV && *cause == EliminationCause::CardAbility
    );
    assert_eq!(
        result.state.investigators[&INV].status,
        Status::Defeated,
        "defeated by a card ability — neither killed nor driven insane",
    );
    assert_event!(
        result.events,
        Event::TraumaSuffered { investigator, kind, amount }
            if *investigator == INV && *kind == TraumaKind::Physical && *amount == 1
    );
    assert_event!(result.events, Event::AllInvestigatorsEliminated);
    assert_eq!(
        result.state.ending,
        Some(ScenarioEnding::NoResolution),
        "the act-3 bullet prints no (→R#), so no resolution point is reached",
    );
    assert!(
        !result
            .events
            .iter()
            .any(|e| matches!(e, Event::ScenarioResolved { ending }
                if matches!(ending, ScenarioEnding::Resolution(_)))),
        "no numbered resolution is ever latched: {:?}",
        result.events,
    );
    assert!(
        result.state.continuations.is_empty(),
        "no stranded frames after the ending: {:?}",
        result.state.continuations,
    );
}

/// The player gets to read the terminal agenda's reverse before the result panel
/// replaces the board: the advance-flip acknowledge (#558) surfaces on the
/// terminal agenda exactly as it does on 01105 and 01106, and the ending is not
/// latched until it is answered.
#[test]
fn the_terminal_agendas_advance_flip_acknowledge_precedes_the_ending() {
    let mut state = seated_roland();
    state.agenda_index = 2;
    state.agenda_doom = state.agenda_deck[2].doom_threshold - 1;
    state.encounter_deck.clear();
    state.interactive_acknowledge = true;

    // Ending the round fires 01107's two Forced *fronts* first — the
    // enemy-phase-end Ghoul move and the round-end doom — each raising its own
    // #466 acknowledge before the Mythos doom tips the threshold.
    let mut r = take_turn_action(state, &TurnAction::EndTurn);
    for _ in 0..2 {
        assert_eq!(
            prompt_of(&r),
            "Forced — They're Getting Out!",
            "01107's own fronts resolve before the agenda advances",
        );
        r = pick_single(r.state);
    }

    // Then the flip: a single on-card option anchored to the agenda, with the
    // ending still unlatched — the player reads the reverse before the result
    // panel replaces the board (#558).
    assert_eq!(prompt_of(&r), "Agenda 3 advanced — acknowledge.");
    let EngineOutcome::AwaitingInput { request, .. } = &r.outcome else {
        unreachable!("prompt_of would have panicked")
    };
    assert_eq!(request.options.len(), 1, "one option: {request:?}");
    assert_eq!(
        request.options[0].target,
        Some(game_core::engine::OptionTarget::Agenda),
        "it anchors to the agenda card the player is being asked to read",
    );
    assert!(r.state.ending.is_none(), "nothing latched before the flip");

    // Answering the flip fires the reverse, which is a Forced like 01105's and
    // acknowledges the same way — and only running it latches the ending.
    r = pick_single(r.state);
    assert_eq!(
        prompt_of(&r),
        "Forced — They're Getting Out!",
        "the reverse"
    );
    assert!(r.state.ending.is_none(), "nor before the reverse runs");

    let done = pick_single(r.state);
    assert_eq!(
        done.outcome,
        EngineOutcome::Done,
        "the reverse's acknowledge is the last of them",
    );
    assert_eq!(
        done.state.ending,
        Some(ScenarioEnding::Resolution(ResolutionId::new(3))),
    );
}
