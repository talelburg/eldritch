//! The 01104 reference card's symbol-token effects, end-to-end through the
//! real card registry (Ghoul metadata) + the scenario module, both named by
//! `scenarios::TEST_REGISTRIES`.

use game_core::engine::enumerate::TurnAction;
use game_core::engine::{ApplyResult, EngineOutcome, OptionTarget};
use game_core::event::Event;
use game_core::scenario::ScenarioId;
use game_core::state::{
    CardCode, CardInstanceId, ChaosBag, ChaosToken, GameState, GameStateBuilder, InvestigatorId,
    LocationId, SkillKind, TokenResolution,
};
use game_core::test_support::{
    self, act, enemy, location, seat, BoardLookup as _, Printed, StagedLocation, TestSession,
};
use game_core::{assert_event, assert_event_count, assert_no_event};
use scenarios::the_gathering;

const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(1);

/// `INV`'s open turn in a Gathering game at the Study `HERE`, with `ghouls`
/// Ghoul Minions there, `in_play` in the play area, and a chaos bag holding
/// only `token`.
fn gathering_board(token: ChaosToken, ghouls: u8, in_play: &[&str]) -> GameState {
    let mut builder = GameStateBuilder::new()
        .with_registries(scenarios::TEST_REGISTRIES)
        .with_seat(seat(1).at(HERE).in_play(in_play.iter().copied()))
        .with_location(location(HERE))
        .with_chaos_bag(ChaosBag::new([token]))
        .with_scenario_id(ScenarioId::new(the_gathering::ID));
    for i in 0..ghouls {
        // Ghoul Minion 01160 (Humanoid. Monster. Ghoul.): its printed traits
        // drive ghoul_count.
        builder = builder.with_enemy(enemy(u32::from(i) + 1).as_card("01160").at(HERE));
    }
    builder.open_turn(INV).build()
}

/// [`gathering_board`] with nothing in play.
fn gathering_state(token: ChaosToken, ghouls: u8) -> GameState {
    gathering_board(token, ghouls, &[])
}

/// A difficulty-`difficulty` Willpower test from the open turn, committing
/// nothing, run through to the turn menu so the symbol path resolves
/// end-to-end. Returns the session back at the turn menu.
fn perform(state: GameState, difficulty: i8) -> TestSession {
    let session = TestSession::new(state, scenarios::TEST_REGISTRIES)
        .test_skill(INV, SkillKind::Willpower, difficulty)
        .commit(&[]);
    session.assert_at_turn_menu(INV);
    session
}

#[test]
fn skull_subtracts_ghoul_count_at_location() {
    // 0 ghouls: Skull → Modifier(0)
    let r0 = perform(gathering_state(ChaosToken::Skull, 0), 0);
    assert_event!(
        r0.events(),
        Event::ChaosTokenRevealed {
            token: ChaosToken::Skull,
            resolution: TokenResolution::Modifier(0),
        }
    );
    // 2 ghouls: Skull → Modifier(-2)
    let r2 = perform(gathering_state(ChaosToken::Skull, 2), 0);
    assert_event!(
        r2.events(),
        Event::ChaosTokenRevealed {
            token: ChaosToken::Skull,
            resolution: TokenResolution::Modifier(-2),
        }
    );
}

#[test]
fn cultist_is_minus_one_and_horror_only_on_failure() {
    // Fail: difficulty 99 >> skill 3 + (-1) = 2
    let fail = perform(gathering_state(ChaosToken::Cultist, 0), 99);
    assert_event!(
        fail.events(),
        Event::ChaosTokenRevealed {
            token: ChaosToken::Cultist,
            resolution: TokenResolution::Modifier(-1),
        }
    );
    assert_event!(
        fail.events(),
        Event::HorrorPlacedOnInvestigator { amount: 1, .. }
    );
    // Win: difficulty 0 ≤ skill 3 + (-1) = 2
    let win = perform(gathering_state(ChaosToken::Cultist, 0), 0);
    assert_no_event!(win.events(), Event::HorrorPlacedOnInvestigator { .. });
}

#[test]
fn tablet_is_minus_two_and_damage_iff_ghoul_present() {
    // Ghoul present: Tablet → Modifier(-2) + DamagePlacedOnInvestigator(1)
    let with_ghoul = perform(gathering_state(ChaosToken::Tablet, 1), 0);
    assert_event!(
        with_ghoul.events(),
        Event::ChaosTokenRevealed {
            token: ChaosToken::Tablet,
            resolution: TokenResolution::Modifier(-2),
        }
    );
    assert_event!(
        with_ghoul.events(),
        Event::DamagePlacedOnInvestigator { amount: 1, .. }
    );
    // No ghoul: Tablet → Modifier(-2), NO DamagePlacedOnInvestigator
    let no_ghoul = perform(gathering_state(ChaosToken::Tablet, 0), 0);
    assert_no_event!(no_ghoul.events(), Event::DamagePlacedOnInvestigator { .. });
}

#[test]
fn tablet_immediate_damage_precedes_the_determination() {
    // RR ST.4 (apply chaos symbol effect) precedes ST.6 (determine
    // success/failure): Tablet's immediate Damage(1) must land in the event log
    // BEFORE SkillTestSucceeded/Failed. (Difficulty 0; willpower 3 + (-2) = 1
    // succeeds.)
    let session = perform(gathering_state(ChaosToken::Tablet, 1), 0);
    let damage = session
        .events()
        .iter()
        .position(|e| matches!(e, Event::DamagePlacedOnInvestigator { amount: 1, .. }))
        .expect("Tablet+ghoul deals immediate damage");
    let determined = session
        .events()
        .iter()
        .position(|e| {
            matches!(
                e,
                Event::SkillTestSucceeded { .. } | Event::SkillTestFailed { .. }
            )
        })
        .expect("the test resolves");
    assert!(
        damage < determined,
        "ST.4 immediate damage must precede the ST.6 determination; events: {:?}",
        session.events()
    );

    // No-redraw: the token is drawn once at Resolving; pushing the immediate
    // Effect::Deal and resuming at DetermineOutcome must not re-draw it.
    let reveals = session
        .events()
        .iter()
        .filter(|e| matches!(e, Event::ChaosTokenRevealed { .. }))
        .count();
    assert_eq!(
        reveals,
        1,
        "exactly one ChaosTokenRevealed (no re-draw across the ST.4 push/resume); events: {:?}",
        session.events()
    );
}

#[test]
fn cultist_on_fail_horror_follows_the_determination() {
    // RR: a chaos symbol's result-conditional effect (01104's Cultist: "If you
    // fail, take 1 horror.") resolves at ST.7, AFTER the ST.6 determination
    // (and after the outcome timing point). Cultist's on_fail Horror(1) must
    // follow SkillTestFailed in the log. (Difficulty 99 >> willpower 3 + (-1)
    // = 2 → fail.)
    let session = perform(gathering_state(ChaosToken::Cultist, 0), 99);
    let failed = session
        .events()
        .iter()
        .position(|e| matches!(e, Event::SkillTestFailed { .. }))
        .expect("the test fails");
    let horror = session
        .events()
        .iter()
        .position(|e| matches!(e, Event::HorrorPlacedOnInvestigator { amount: 1, .. }))
        .expect("Cultist on_fail deals horror");
    assert!(
        failed < horror,
        "ST.7 symbol on_fail horror must follow the ST.6 determination; events: {:?}",
        session.events()
    );
}

const GUARD_DOG: &str = "01021"; // Ally, 3 health.

#[test]
fn tablet_immediate_damage_suspends_on_soak_without_redrawing() {
    // Tablet + ghoul deals immediate Damage(1) at ST.4. With Guard Dog (01021,
    // health 3) controlled, that non-attack damage is distributed interactively
    // (one PickSingle prompt) — the symbol effect suspends mid-ST.4. Resuming
    // must finish the test from DetermineOutcome WITHOUT re-drawing the chaos
    // token. (Symbol damage is effect-source, so no Guard Dog reaction window.)
    let state = gathering_board(ChaosToken::Tablet, 1, &[GUARD_DOG]);
    let dog = state.in_play(INV, GUARD_DOG);

    let session = TestSession::new(state, scenarios::TEST_REGISTRIES)
        .test_skill(INV, SkillKind::Willpower, 0)
        .commit(&[]) // ST.2 commit window: commit nothing.
        .pick(OptionTarget::CardInstance(dog)); // soak the 1 damage onto Guard Dog.
    session.assert_at_turn_menu(INV);

    let inv = &session.state().investigators[&INV];
    assert_eq!(inv.damage(), 0, "damage soaked, investigator took none");
    let dog = inv.cards_in_play.iter().find(|c| c.instance_id == dog);
    assert_eq!(
        dog.map(|c| c.accumulated_damage),
        Some(1),
        "1 damage soaked onto Guard Dog",
    );
    assert_event_count!(session.events(), 1, Event::ChaosTokenRevealed { .. });
}

// ---------------------------------------------------------------------------
// ST.5 reads the board the ST.4 symbol effects left behind
// ---------------------------------------------------------------------------

const BEAT_COP: &str = "01018"; // Ally, 2 health, "You get +1 [combat]."

/// The Gathering board for the Beat Cop trace: one Ghoul co-located with the
/// investigator, a chaos bag holding only `[tablet]` (−2, and 1 damage while a
/// Ghoul is at your location), and Beat Cop 01018 in play carrying
/// `cop_damage` damage already.
fn beat_cop_board(cop_damage: u8) -> GameState {
    let mut state = gathering_board(ChaosToken::Tablet, 1, &[BEAT_COP]);
    // No seat entry carries damage on an ally: set it on the built state.
    let cop = state.in_play(INV, BEAT_COP);
    state
        .investigators
        .get_mut(&INV)
        .expect("investigator present")
        .cards_in_play
        .iter_mut()
        .find(|c| c.instance_id == cop)
        .expect("Beat Cop in play")
        .accumulated_damage = cop_damage;
    state
}

/// Drive a difficulty-2 Combat test on `state`, committing nothing and soaking
/// the `[tablet]`'s ST.4 damage onto Beat Cop. Returns the session back at the
/// turn menu, with Beat Cop's instance id (read before the test: a discarded
/// ally is gone after it).
fn drive_combat_test_soaking_onto_the_ally(state: GameState) -> (TestSession, CardInstanceId) {
    let cop = state.in_play(INV, BEAT_COP);
    // Beat Cop's own [fast] ability makes both RR p.26 player windows live
    // (ST.1→ST.2 and ST.2→ST.3); pass on each.
    let session = TestSession::new(state, scenarios::TEST_REGISTRIES)
        .test_skill(INV, SkillKind::Combat, 2)
        .skip()
        .commit(&[])
        .skip()
        .pick(OptionTarget::CardInstance(cop));
    session.assert_at_turn_menu(INV);
    (session, cop)
}

/// `Appendix_II_Timing_and_Gameplay.md` orders ST.4 (*"Apply chaos symbol
/// effect(s)"*) before ST.5, and ST.5 sums *"all active card abilities that are
/// modifying the investigator's skill value"* — active **at ST.5**. Beat Cop at
/// 1 remaining health soaks the `[tablet]`'s ST.4 damage, is discarded, and so
/// contributes nothing at ST.5: combat 3 + (−2) = 1 against difficulty 2, a
/// failure by 1. Summing at ST.3 instead credits the dead ally's `+1` and turns
/// this into a pass.
#[test]
fn an_ally_killed_by_the_symbol_damage_does_not_contribute_at_st5() {
    let (session, cop) = drive_combat_test_soaking_onto_the_ally(beat_cop_board(1));

    let inv = &session.state().investigators[&INV];
    assert_eq!(inv.damage(), 0, "the damage was soaked, not taken");
    assert!(
        !inv.cards_in_play.iter().any(|c| c.instance_id == cop),
        "Beat Cop took its 2nd damage at ST.4 and left play; in play: {:?}",
        inv.cards_in_play,
    );
    assert!(
        inv.discard.contains(&CardCode::new(BEAT_COP)),
        "the discarded Beat Cop is in the discard pile; discard: {:?}",
        inv.discard,
    );
    // combat 3 − 2 = 1 against difficulty 2.
    assert_event!(
        session.events(),
        Event::SkillTestFailed {
            skill: SkillKind::Combat,
            by: 1,
            ..
        }
    );
}

/// The mirror of the trace above: the same board with Beat Cop undamaged. It
/// survives the ST.4 soak, is still in play at ST.5, and its `+1 [combat]`
/// carries the test — so the fix is the discard, not a blanket loss of the
/// ally's contribution.
#[test]
fn an_ally_that_survives_the_symbol_damage_still_contributes_at_st5() {
    let (session, cop) = drive_combat_test_soaking_onto_the_ally(beat_cop_board(0));

    let inv = &session.state().investigators[&INV];
    let cop = inv
        .cards_in_play
        .iter()
        .find(|c| c.instance_id == cop)
        .expect("Beat Cop survives its 1st damage");
    assert_eq!(cop.accumulated_damage, 1, "1 damage soaked onto Beat Cop");
    // combat 3 + 1 − 2 = 2 against difficulty 2.
    assert_event!(
        session.events(),
        Event::SkillTestSucceeded {
            skill: SkillKind::Combat,
            margin: 0,
            ..
        }
    );
}

/// The ST.4 soak window suspends *between* the token reveal (ST.3) and the ST.5
/// sum. Resuming must finish the test without re-drawing the token — the
/// sibling of `tablet_immediate_damage_suspends_on_soak_without_redrawing`, now
/// that the determination is computed on the far side of the suspension.
#[test]
fn the_soak_suspension_between_st4_and_st5_does_not_redraw_the_token() {
    let (session, _) = drive_combat_test_soaking_onto_the_ally(beat_cop_board(1));
    assert_event_count!(session.events(), 1, Event::ChaosTokenRevealed { .. });
}

// ---------------------------------------------------------------------------
// Victory display tests (location VPs at scenario end)
// ---------------------------------------------------------------------------

/// `INV`'s open turn in a Gathering game holding the 1 clue a terminal act
/// needs, with `locations` in play, so a single `AdvanceAct` latches Won and
/// triggers the victory scan.
fn resolvable_state(locations: impl IntoIterator<Item = StagedLocation<Printed>>) -> GameState {
    locations
        .into_iter()
        .fold(
            GameStateBuilder::new()
                .with_registries(scenarios::TEST_REGISTRIES)
                .with_seat(seat(1).clues(1)),
            GameStateBuilder::with_location,
        )
        .with_scenario_id(ScenarioId::new(the_gathering::ID))
        // Terminal because it is the only act; advancing it fires the reverse
        // that reaches R1, which is what triggers the victory-display scan.
        .with_act_deck([act(test_support::terminal_code(1), 1)])
        .open_turn(INV)
        .build()
}

/// A terminal-act Gathering state with the Attic 01113 revealed/cleared or
/// not, so a single `AdvanceAct` latches Won and triggers the victory scan.
fn resolvable_state_with_attic(revealed: bool, clues: u8) -> GameState {
    let attic = location(1).as_card("01113").clues(clues);
    resolvable_state([if revealed { attic } else { attic.unrevealed() }])
}

/// Take `AdvanceAct` from the turn menu; the terminal act's reverse ends the
/// game.
fn advance_to_resolution(state: GameState) -> ApplyResult {
    let r = TestSession::new(state, scenarios::TEST_REGISTRIES)
        .take(&TurnAction::AdvanceAct { investigator: INV })
        .finish();
    assert_eq!(r.outcome, EngineOutcome::Done, "the game is over");
    r
}

#[test]
fn cleared_revealed_victory_location_enters_victory_display() {
    let r = advance_to_resolution(resolvable_state_with_attic(true, 0));
    assert!(
        r.state.victory_display.contains(&CardCode::new("01113")),
        "Attic (01113) should be in victory_display; got: {:?}",
        r.state.victory_display
    );
    assert_event!(
        r.events,
        Event::EnteredVictoryDisplay { code, victory: 1 } if code.as_str() == "01113"
    );
}

#[test]
fn unrevealed_or_clued_victory_location_is_not_placed() {
    let clued = advance_to_resolution(resolvable_state_with_attic(true, 2));
    assert!(
        clued.state.victory_display.is_empty(),
        "clued Attic should not enter victory display; got: {:?}",
        clued.state.victory_display
    );
    let unrevealed = advance_to_resolution(resolvable_state_with_attic(false, 0));
    assert!(
        unrevealed.state.victory_display.is_empty(),
        "unrevealed Attic should not enter victory display; got: {:?}",
        unrevealed.state.victory_display
    );
}

#[test]
fn two_cleared_victory_locations_both_enter_display() {
    let r = advance_to_resolution(resolvable_state([
        location(1).as_card("01113"),
        location(2).as_card("01114"),
    ]));
    assert!(
        r.state.victory_display.contains(&CardCode::new("01113")),
        "Attic (01113) should be in victory_display; got: {:?}",
        r.state.victory_display
    );
    assert!(
        r.state.victory_display.contains(&CardCode::new("01114")),
        "Cellar (01114) should be in victory_display; got: {:?}",
        r.state.victory_display
    );
    assert_event_count!(r.events, 2, Event::EnteredVictoryDisplay { .. });
}
