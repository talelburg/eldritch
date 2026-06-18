//! End-to-end tests for Evidence! 01022 (Axis C reaction-event-play, #304)
//! against the real `cards::REGISTRY`.
//!
//! Card text (verbatim, `data/arkhamdb-snapshot/pack/core/core.json`):
//!
//! > Fast. Play after you defeat an enemy.
//! > Discover 1 clue at your location.
//!
//! Lives at `crates/cards/tests/` so it can install [`cards::REGISTRY`] in its
//! own integration-test process.

use std::sync::Once;

use game_core::engine::EngineOutcome;
use game_core::event::Event;
use game_core::state::{
    CardCode, ChaosBag, ChaosToken, EnemyId, InvestigatorId, LocationId, Phase, TokenModifiers,
    WindowKind,
};
use game_core::test_support::{
    drive, test_enemy, test_investigator, test_location, GameStateBuilder, ScriptedResolver,
};
use game_core::{assert_event, assert_no_event, Action, PlayerAction};

/// `ArkhamDB` code for original-Core Evidence!.
const EVIDENCE: &str = "01022";

fn install_real_registry() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let _ = game_core::card_registry::install(cards::REGISTRY);
    });
}

/// Solo investigator (NOT Roland — no in-play reaction) engaged with a 1-HP
/// enemy at a location with `location_clues` clues, holding Evidence! in hand.
/// A successful Combat test defeats the enemy and opens the after-defeat
/// window.
fn investigator_with_evidence_and_enemy(
    location_clues: u8,
) -> (InvestigatorId, EnemyId, LocationId, game_core::GameState) {
    install_real_registry();
    let inv_id = InvestigatorId(1);
    let enemy_id = EnemyId(100);
    let loc_id = LocationId(10);

    let mut inv = test_investigator(1);
    inv.current_location = Some(loc_id);
    inv.skills.combat = 4;
    inv.hand.push(CardCode::new(EVIDENCE));

    let mut enemy = test_enemy(100, "Mock Ghoul");
    enemy.fight = 1;
    enemy.max_health = 1;
    enemy.damage = 0;
    enemy.engaged_with = Some(inv_id);

    let mut loc = test_location(10, "Study");
    loc.clues = location_clues;

    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_round(0)
        .with_active_investigator(inv_id)
        .with_turn_order([inv_id])
        .with_investigator(inv)
        .with_enemy(enemy)
        .with_location(loc)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .build();
    (inv_id, enemy_id, loc_id, state)
}

fn fight_action(inv: InvestigatorId, enemy: EnemyId) -> Action {
    Action::Player(PlayerAction::Fight {
        investigator: inv,
        enemy,
    })
}

#[test]
fn after_defeat_window_opens_and_offers_evidence_with_no_in_play_reaction() {
    let (inv_id, enemy_id, loc_id, state) = investigator_with_evidence_and_enemy(2);

    // Commit nothing to the Fight test, then SKIP the reaction window (the
    // option is offered here; playing it is exercised in the next test).
    let mut resolver = ScriptedResolver::new();
    resolver.commit_cards(&[]).skip();
    let result = drive(state, fight_action(inv_id, enemy_id), resolver);

    assert_eq!(result.outcome, EngineOutcome::Done);
    // The window opens even though no in-play card reacts — the hand match
    // alone opens it.
    assert_event!(
        result.events,
        Event::WindowOpened {
            kind: WindowKind::AfterEnemyDefeated { enemy: e, .. },
        } if *e == enemy_id
    );
    // Skipped → no clue discovered, Evidence! still in hand.
    assert_no_event!(result.events, Event::CluePlaced { .. });
    assert_eq!(result.state.locations[&loc_id].clues, 2);
    assert!(result.state.investigators[&inv_id]
        .hand
        .iter()
        .any(|c| c.as_str() == EVIDENCE));
}
