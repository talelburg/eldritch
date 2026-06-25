//! C2: 01104 reference-card symbol-token effects, end-to-end through the
//! real card registry (Ghoul metadata) + the installed scenario module.
//! Own process so the global registries can be installed once.

use std::sync::Once;

use game_core::action::{Action, PlayerAction};
use game_core::engine::{apply, EngineOutcome, OptionId};
use game_core::event::Event;
use game_core::scenario::{Resolution, ScenarioId};
use game_core::state::{
    Act, CardCode, CardInPlay, CardInstanceId, ChaosBag, ChaosToken, InvestigatorId, LocationId,
    Phase, SkillKind, TokenResolution,
};
use game_core::test_support::{
    apply_no_commits, drive, test_enemy, test_investigator, test_location, GameStateBuilder,
    ScriptedResolver,
};
use scenarios::REGISTRY;

static INSTALL: Once = Once::new();

fn install_registries() {
    INSTALL.call_once(|| {
        let _ = game_core::scenario_registry::install(REGISTRY);
        let _ = game_core::card_registry::install(cards::REGISTRY);
    });
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
    // apply_no_commits drives past the card-commit window (raw apply stops there with AwaitingInput) so the symbol path resolves end-to-end.
    let r = apply_no_commits(
        state,
        Action::Player(PlayerAction::PerformSkillTest {
            investigator: InvestigatorId(1),
            skill: SkillKind::Willpower,
            difficulty,
        }),
    );
    assert_eq!(r.outcome, EngineOutcome::Done);
    r
}

#[test]
fn skull_subtracts_ghoul_count_at_location() {
    install_registries();
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
    install_registries();
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
    install_registries();
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
    install_registries();
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
    install_registries();
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
    install_registries();
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
    let r = drive(
        state,
        Action::Player(PlayerAction::PerformSkillTest {
            investigator: InvestigatorId(1),
            skill: SkillKind::Willpower,
            difficulty: 0,
        }),
        resolver,
    );

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
        code: CardCode("_test_act".into()),
        clue_threshold: 1,
        resolution: Some(Resolution::Won { id: "R1".into() }),
    }];
    state
}

fn advance_to_resolution(state: game_core::state::GameState) -> game_core::engine::ApplyResult {
    let r = apply(
        state,
        Action::Player(PlayerAction::AdvanceAct {
            investigator: InvestigatorId(1),
        }),
    );
    assert_eq!(r.outcome, EngineOutcome::Done);
    r
}

#[test]
fn cleared_revealed_victory_location_enters_victory_display() {
    install_registries();
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
    install_registries();
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
    install_registries();
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
        code: CardCode("_test_act".into()),
        clue_threshold: 1,
        resolution: Some(Resolution::Won { id: "R1".into() }),
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
