//! C2: 01104 reference-card symbol-token effects, end-to-end through the
//! real card registry (Ghoul metadata) + the installed scenario module.
//! Own process so the global registries can be installed once.

use std::sync::Once;

use game_core::action::{Action, PlayerAction};
use game_core::engine::EngineOutcome;
use game_core::event::Event;
use game_core::scenario::ScenarioId;
use game_core::state::{
    ChaosBag, ChaosToken, InvestigatorId, LocationId, SkillKind, TokenResolution,
};
use game_core::test_support::{
    apply_no_commits, test_enemy, test_investigator, test_location, GameStateBuilder,
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
