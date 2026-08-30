//! C2: 01104 reference-card symbol-token effects, end-to-end through the
//! real card registry (Ghoul metadata) + the installed scenario module.
//! Own process so the global registries can be installed once.

use game_core::engine::{EngineOutcome, OptionId};
use game_core::event::Event;
use game_core::scenario::ScenarioId;
use game_core::state::{
    Act, CardCode, CardInPlay, CardInstanceId, ChaosBag, ChaosToken, InvestigatorId, LocationId,
    Phase, SkillKind, TokenResolution,
};
use game_core::test_support::{
    dispatch_turn_action_unchecked, drive_skill_test, perform_skill_test_no_commits, terminal_code,
    test_enemy, test_investigator, test_location, GameStateBuilder, ScriptedResolver,
};
use game_core::{assert_event, assert_event_count, TurnAction};
use scenarios::REGISTRY;

#[ctor::ctor(unsafe)]
fn install_registries() {
    let _ = game_core::scenario_registry::install(REGISTRY);
    // The real registry plus `test_support`'s synthetic terminal card: the
    // victory-display fixtures below end their act deck in one, because a
    // terminal card reaches its resolution point by running an effect on its
    // reverse (ADR 0013) and so needs the registry to serve it.
    game_core::test_support::install_registry_with_terminal_cards(cards::REGISTRY);
}

fn gathering_state(token: ChaosToken, ghouls: u8) -> game_core::state::GameState {
    let inv = InvestigatorId(1);
    let loc = LocationId(1);
    let mut investigator = test_investigator(1);
    // Use Skids O'Toole (01003): a real corpus code known to cards::REGISTRY
    // (installed here) with capacity data, so max_health()/max_sanity() work.
    investigator.investigator_card.code = CardCode::new("01003");
    investigator.current_location = Some(loc);
    let mut state = GameStateBuilder::new()
        .with_investigator(investigator)
        .with_active_investigator(inv)
        .with_chaos_bag(ChaosBag::new([token]))
        .with_scenario_id(ScenarioId::new(scenarios::the_gathering::ID))
        .build();
    state.locations.insert(loc, test_location(1, "Study"));
    for i in 0..ghouls {
        let mut e = test_enemy(u32::from(i) + 1, "Ghoul");
        e.traits = vec!["Ghoul".to_string()]; // traits drives ghoul_count; test_enemy's name arg is display-only.
        e.current_location = Some(loc);
        state.enemies.insert(e.id, e);
    }
    state
}

fn perform(state: game_core::state::GameState, difficulty: i8) -> game_core::engine::ApplyResult {
    // perform_skill_test_no_commits drives past the card-commit window (the bare
    // helper stops there with AwaitingInput) so the symbol path resolves end-to-end.
    let r =
        perform_skill_test_no_commits(state, InvestigatorId(1), SkillKind::Willpower, difficulty);
    assert_eq!(r.outcome, EngineOutcome::Done);
    r
}

#[test]
fn skull_subtracts_ghoul_count_at_location() {
    // 0 ghouls: Skull → Modifier(0)
    let r0 = perform(gathering_state(ChaosToken::Skull, 0), 0);
    assert!(
        r0.events.iter().any(|e| matches!(
            e,
            Event::ChaosTokenRevealed {
                token: ChaosToken::Skull,
                resolution: TokenResolution::Modifier(0),
            }
        )),
        "expected ChaosTokenRevealed Skull Modifier(0), events: {:?}",
        r0.events
    );
    // 2 ghouls: Skull → Modifier(-2)
    let r2 = perform(gathering_state(ChaosToken::Skull, 2), 0);
    assert!(
        r2.events.iter().any(|e| matches!(
            e,
            Event::ChaosTokenRevealed {
                token: ChaosToken::Skull,
                resolution: TokenResolution::Modifier(-2),
            }
        )),
        "expected ChaosTokenRevealed Skull Modifier(-2), events: {:?}",
        r2.events
    );
}

#[test]
fn cultist_is_minus_one_and_horror_only_on_failure() {
    // Fail: difficulty 99 >> skill 3 + (-1) = 2
    let fail = perform(gathering_state(ChaosToken::Cultist, 0), 99);
    assert!(
        fail.events.iter().any(|e| matches!(
            e,
            Event::ChaosTokenRevealed {
                token: ChaosToken::Cultist,
                resolution: TokenResolution::Modifier(-1),
            }
        )),
        "expected ChaosTokenRevealed Cultist Modifier(-1) on failure, events: {:?}",
        fail.events
    );
    assert!(
        fail.events
            .iter()
            .any(|e| matches!(e, Event::HorrorTaken { amount: 1, .. })),
        "expected HorrorTaken(1) on cultist failure, events: {:?}",
        fail.events
    );
    // Win: difficulty 0 ≤ skill 3 + (-1) = 2
    let win = perform(gathering_state(ChaosToken::Cultist, 0), 0);
    assert!(
        !win.events
            .iter()
            .any(|e| matches!(e, Event::HorrorTaken { .. })),
        "expected NO HorrorTaken on cultist success, events: {:?}",
        win.events
    );
}

#[test]
fn tablet_is_minus_two_and_damage_iff_ghoul_present() {
    // Ghoul present: Tablet → Modifier(-2) + DamageTaken(1)
    let with_ghoul = perform(gathering_state(ChaosToken::Tablet, 1), 0);
    assert!(
        with_ghoul.events.iter().any(|e| matches!(
            e,
            Event::ChaosTokenRevealed {
                token: ChaosToken::Tablet,
                resolution: TokenResolution::Modifier(-2),
            }
        )),
        "expected ChaosTokenRevealed Tablet Modifier(-2) with ghoul, events: {:?}",
        with_ghoul.events
    );
    assert!(
        with_ghoul
            .events
            .iter()
            .any(|e| matches!(e, Event::DamageTaken { amount: 1, .. })),
        "expected DamageTaken(1) on tablet with ghoul, events: {:?}",
        with_ghoul.events
    );
    // No ghoul: Tablet → Modifier(-2), NO DamageTaken
    let no_ghoul = perform(gathering_state(ChaosToken::Tablet, 0), 0);
    assert!(
        !no_ghoul
            .events
            .iter()
            .any(|e| matches!(e, Event::DamageTaken { .. })),
        "expected NO DamageTaken on tablet without ghoul, events: {:?}",
        no_ghoul.events
    );
}

#[test]
fn tablet_immediate_damage_precedes_the_determination() {
    // RR ST.4 (apply chaos symbol effect) precedes ST.6 (determine
    // success/failure): Tablet's immediate Damage(1) must land in the event log
    // BEFORE SkillTestSucceeded/Failed. (Difficulty 0; willpower 3 + (-2) = 1
    // succeeds.)
    let r = perform(gathering_state(ChaosToken::Tablet, 1), 0);
    let damage = r
        .events
        .iter()
        .position(|e| matches!(e, Event::DamageTaken { amount: 1, .. }))
        .expect("Tablet+ghoul deals immediate damage");
    let determined = r
        .events
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
        r.events
    );

    // No-redraw: the token is drawn once at Resolving; pushing the immediate
    // Effect::Deal and resuming at DetermineOutcome must not re-draw it.
    let reveals = r
        .events
        .iter()
        .filter(|e| matches!(e, Event::ChaosTokenRevealed { .. }))
        .count();
    assert_eq!(
        reveals, 1,
        "exactly one ChaosTokenRevealed (no re-draw across the ST.4 push/resume); events: {:?}",
        r.events
    );
}

#[test]
fn cultist_on_fail_horror_follows_the_determination() {
    // RR: a chaos symbol's result-conditional effect ("if this test is failed")
    // resolves at ST.7, AFTER the ST.6 determination (and after the outcome
    // timing point). Cultist's on_fail Horror(1) must follow SkillTestFailed in
    // the log. (Difficulty 99 >> willpower 3 + (-1) = 2 → fail.)
    let r = perform(gathering_state(ChaosToken::Cultist, 0), 99);
    let failed = r
        .events
        .iter()
        .position(|e| matches!(e, Event::SkillTestFailed { .. }))
        .expect("the test fails");
    let horror = r
        .events
        .iter()
        .position(|e| matches!(e, Event::HorrorTaken { amount: 1, .. }))
        .expect("Cultist on_fail deals horror");
    assert!(
        failed < horror,
        "ST.7 symbol on_fail horror must follow the ST.6 determination; events: {:?}",
        r.events
    );
}

#[test]
fn tablet_immediate_damage_suspends_on_soak_without_redrawing() {
    // Tablet + ghoul deals immediate Damage(1) at ST.4. With Guard Dog (01021,
    // health 3) controlled, that non-attack damage is distributed interactively
    // (one PickSingle prompt) — the symbol effect suspends mid-ST.4. Resuming
    // must finish the test from DetermineOutcome WITHOUT re-drawing the chaos
    // token. (Symbol damage is effect-source, so no Guard Dog reaction window.)
    let mut state = gathering_state(ChaosToken::Tablet, 1);
    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .expect("investigator present")
        .cards_in_play
        .push(CardInPlay::enter_play(
            CardCode::new("01021"),
            CardInstanceId(1),
        ));

    let mut resolver = ScriptedResolver::new();
    resolver.commit_cards(&[]); // ST.2 commit window: commit nothing.
    resolver.pick_single(OptionId(1)); // soak the 1 damage onto Guard Dog (option 1).
    let r = drive_skill_test(state, InvestigatorId(1), SkillKind::Willpower, 0, resolver);

    assert_eq!(r.outcome, EngineOutcome::Done);
    let inv = &r.state.investigators[&InvestigatorId(1)];
    assert_eq!(inv.damage(), 0, "damage soaked, investigator took none");
    let dog = inv
        .cards_in_play
        .iter()
        .find(|c| c.instance_id == CardInstanceId(1));
    assert_eq!(
        dog.map(|c| c.accumulated_damage),
        Some(1),
        "1 damage soaked onto Guard Dog",
    );
    let reveals = r
        .events
        .iter()
        .filter(|e| matches!(e, Event::ChaosTokenRevealed { .. }))
        .count();
    assert_eq!(
        reveals, 1,
        "exactly one ChaosTokenRevealed across the ST.4 soak suspend; events: {:?}",
        r.events
    );
}

// ---------------------------------------------------------------------------
// ST.5 reads the board the ST.4 symbol effects left behind (#674)
// ---------------------------------------------------------------------------

const BEAT_COP: &str = "01018"; // Ally, 2 health, "You get +1 [combat]."
const COP_INST: CardInstanceId = CardInstanceId(7);

/// The Gathering board for the Beat Cop trace: one Ghoul co-located with the
/// investigator, a chaos bag holding only `[tablet]` (−2, and 1 damage while a
/// Ghoul is at your location), and Beat Cop 01018 in play carrying
/// `cop_damage` damage already.
fn beat_cop_board(cop_damage: u8) -> game_core::state::GameState {
    let mut state = gathering_state(ChaosToken::Tablet, 1);
    // Pushed after `build()`, so it names its own owner: a player card leaving
    // play goes to its owner's discard pile (#772).
    let mut cop =
        CardInPlay::enter_play(CardCode::new(BEAT_COP), COP_INST).owned_by(Some(InvestigatorId(1)));
    cop.accumulated_damage = cop_damage;
    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .expect("investigator present")
        .cards_in_play
        .push(cop);
    state
}

/// Drive a difficulty-2 Combat test on `state`, committing nothing and soaking
/// the `[tablet]`'s ST.4 damage onto the ally (option 1 — option 0 is the
/// investigator).
fn drive_combat_test_soaking_onto_the_ally(
    state: game_core::state::GameState,
) -> game_core::engine::ApplyResult {
    let mut resolver = ScriptedResolver::new();
    // Beat Cop's own [fast] ability makes both RR p.26 player windows live
    // (ST.1→ST.2 and ST.2→ST.3); pass on each.
    resolver.skip();
    resolver.commit_cards(&[]);
    resolver.skip();
    resolver.pick_single(OptionId(1));
    let r = drive_skill_test(state, InvestigatorId(1), SkillKind::Combat, 2, resolver);
    assert_eq!(r.outcome, EngineOutcome::Done);
    r
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
    let r = drive_combat_test_soaking_onto_the_ally(beat_cop_board(1));

    let inv = &r.state.investigators[&InvestigatorId(1)];
    assert_eq!(inv.damage(), 0, "the damage was soaked, not taken");
    assert!(
        !inv.cards_in_play.iter().any(|c| c.instance_id == COP_INST),
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
        r.events,
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
    let r = drive_combat_test_soaking_onto_the_ally(beat_cop_board(0));

    let inv = &r.state.investigators[&InvestigatorId(1)];
    let cop = inv
        .cards_in_play
        .iter()
        .find(|c| c.instance_id == COP_INST)
        .expect("Beat Cop survives its 1st damage");
    assert_eq!(cop.accumulated_damage, 1, "1 damage soaked onto Beat Cop");
    // combat 3 + 1 − 2 = 2 against difficulty 2.
    assert_event!(
        r.events,
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
    let r = drive_combat_test_soaking_onto_the_ally(beat_cop_board(1));
    assert_event_count!(r.events, 1, Event::ChaosTokenRevealed { .. });
}

// ---------------------------------------------------------------------------
// Victory display tests (C2 — location VPs at scenario end)
// ---------------------------------------------------------------------------

/// A terminal-act Gathering state with `attic` revealed/cleared or not,
/// so a single `AdvanceAct` latches Won and triggers the victory scan.
fn resolvable_state_with_attic(revealed: bool, clues: u8) -> game_core::state::GameState {
    let inv = InvestigatorId(1);
    let mut investigator = test_investigator(1);
    investigator.clues = 1;
    let mut state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator(investigator)
        .with_active_investigator(inv)
        .with_turn_order([inv])
        .with_scenario_id(ScenarioId::new(scenarios::the_gathering::ID))
        .build();
    let mut attic = test_location(1, "Attic");
    attic.code = CardCode("01113".into());
    attic.revealed = revealed;
    attic.clues = clues;
    state.locations.insert(attic.id, attic);
    state.act_deck = vec![Act {
        // Terminal because it is the only act; advancing it fires the reverse
        // that reaches R1, which is what triggers the victory-display scan.
        code: terminal_code(1),
        clue_threshold: 1,
    }];
    state
}

fn advance_to_resolution(state: game_core::state::GameState) -> game_core::engine::ApplyResult {
    let r = dispatch_turn_action_unchecked(
        state,
        &TurnAction::AdvanceAct {
            investigator: InvestigatorId(1),
        },
    );
    assert_eq!(r.outcome, EngineOutcome::Done);
    r
}

#[test]
fn cleared_revealed_victory_location_enters_victory_display() {
    let r = advance_to_resolution(resolvable_state_with_attic(true, 0));
    assert!(
        r.state.victory_display.contains(&CardCode("01113".into())),
        "Attic (01113) should be in victory_display; got: {:?}",
        r.state.victory_display
    );
    assert!(
        r.events.iter().any(|e| matches!(
            e,
            Event::EnteredVictoryDisplay { code, victory: 1 } if code.as_str() == "01113"
        )),
        "expected EnteredVictoryDisplay for 01113 with victory=1, events: {:?}",
        r.events
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
    let inv = InvestigatorId(1);
    let mut investigator = test_investigator(1);
    investigator.clues = 1;
    let mut state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator(investigator)
        .with_active_investigator(inv)
        .with_turn_order([inv])
        .with_scenario_id(ScenarioId::new(scenarios::the_gathering::ID))
        .build();
    for (lid, code, name) in [(1u32, "01113", "Attic"), (2u32, "01114", "Cellar")] {
        let mut loc = test_location(lid, name);
        loc.code = CardCode(code.into());
        loc.revealed = true;
        loc.clues = 0;
        state.locations.insert(loc.id, loc);
    }
    state.act_deck = vec![Act {
        // Terminal because it is the only act; advancing it fires the reverse
        // that reaches R1, which is what triggers the victory-display scan.
        code: terminal_code(1),
        clue_threshold: 1,
    }];
    let r = advance_to_resolution(state);
    assert!(
        r.state.victory_display.contains(&CardCode("01113".into())),
        "Attic (01113) should be in victory_display; got: {:?}",
        r.state.victory_display
    );
    assert!(
        r.state.victory_display.contains(&CardCode("01114".into())),
        "Cellar (01114) should be in victory_display; got: {:?}",
        r.state.victory_display
    );
    assert_eq!(
        r.events
            .iter()
            .filter(|e| matches!(e, Event::EnteredVictoryDisplay { .. }))
            .count(),
        2,
        "expected exactly 2 EnteredVictoryDisplay events, events: {:?}",
        r.events
    );
}
