//! End-to-end test for Roland Banks (01001)'s `[reaction]` ability
//! against the real `cards::REGISTRY`.
//!
//! Card text (from `data/arkhamdb-snapshot/pack/core/core.json`):
//!
//! > [reaction] After you defeat an enemy: Discover 1 clue at your
//! > location. (Limit once per round.)
//!
//! Closes the Phase-3 acceptance for #55's reaction half. The
//! `[elder_sign]` half stays on its `+0` placeholder; #118 picks it
//! up alongside the dynamic skill-test modifier DSL primitive.
//!
//! Lives at `crates/cards/tests/` so it can install [`cards::REGISTRY`]
//! in its own integration-test process without colliding with the
//! mock-registry tests in `crates/game-core/tests/reaction_windows.rs`.

use std::sync::Once;

use card_dsl::dsl::{UsageLimit, UsagePeriod};
use game_core::engine::EngineOutcome;
use game_core::event::Event;
use game_core::state::{
    AbilityUsageRecord, CardCode, CardInPlay, CardInstanceId, ChaosBag, ChaosToken, EnemyId,
    InvestigatorId, LocationId, Phase, TokenModifiers, WindowKind,
};
use game_core::test_support::{
    apply_no_commits, drive, test_enemy, test_investigator, test_location, GameStateBuilder,
    ScriptedResolver,
};
use game_core::{assert_event, assert_no_event, Action, PlayerAction};

/// `ArkhamDB` code for original-Core Roland Banks.
const ROLAND: &str = "01001";

/// `instance_id` we assign to Roland's investigator card in
/// `cards_in_play`. Arbitrary; tests just need it stable to compare
/// against `PendingTrigger.instance_id`.
const ROLAND_INSTANCE: u32 = 1;

fn install_real_registry() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let _ = game_core::card_registry::install(cards::REGISTRY);
    });
}

/// Build a Fight-ready scenario with Roland engaged with an enemy at
/// 1 HP and Combat 1 so a successful Combat test defeats. Roland's
/// investigator card is placed in `cards_in_play` so the existing
/// reaction-window scan finds his `[reaction]` ability via the real
/// registry.
fn roland_at_location_with_enemy(
    location_clues: u8,
    round: u32,
) -> (InvestigatorId, EnemyId, LocationId, game_core::GameState) {
    install_real_registry();
    let inv_id = InvestigatorId(1);
    let enemy_id = EnemyId(100);
    let loc_id = LocationId(10);

    let mut inv = test_investigator(1);
    inv.current_location = Some(loc_id);
    inv.skills.combat = 4; // Roland's printed combat.
    inv.cards_in_play.push(CardInPlay::enter_play(
        CardCode::new(ROLAND),
        CardInstanceId(ROLAND_INSTANCE),
    ));

    let mut enemy = test_enemy(100, "Mock Ghoul");
    enemy.fight = 1;
    enemy.max_health = 1;
    enemy.damage = 0;
    enemy.engaged_with = Some(inv_id);
    enemy.current_location = Some(loc_id); // co-located: Fight is location-gated (#401)

    let mut loc = test_location(10, "Study");
    loc.clues = location_clues;

    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_round(round)
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
fn reaction_fires_after_roland_defeats_enemy_and_discovers_clue() {
    let (inv_id, enemy_id, loc_id, state) = roland_at_location_with_enemy(2, 0);

    // Empty commit, then PickSingle(OptionId(0)) for the reaction window.
    let mut resolver = ScriptedResolver::new();
    resolver
        .commit_cards(&[])
        .pick_single(game_core::engine::OptionId(0));
    let result = drive(state, fight_action(inv_id, enemy_id), resolver);

    assert_eq!(result.outcome, EngineOutcome::Done);
    assert_event!(
        result.events,
        Event::EnemyDefeated { enemy: e, by: Some(by) } if *e == enemy_id && *by == inv_id
    );
    assert_event!(
        result.events,
        Event::WindowOpened {
            kind: WindowKind::AfterEnemyDefeated { enemy: e, .. },
        } if *e == enemy_id
    );
    assert_event!(
        result.events,
        Event::CluePlaced { investigator, count: 1 } if *investigator == inv_id
    );
    assert_event!(
        result.events,
        Event::WindowClosed {
            kind: WindowKind::AfterEnemyDefeated { enemy: e, .. },
        } if *e == enemy_id
    );

    // 1 of 2 clues stayed at the location; Roland is carrying 1.
    assert_eq!(result.state.locations[&loc_id].clues, 1);
    assert_eq!(result.state.investigators[&inv_id].clues, 1);

    // Counter bumped on Roland's investigator card: 1 fire this round.
    let inv = &result.state.investigators[&inv_id];
    let roland_card = inv
        .cards_in_play
        .iter()
        .find(|c| c.code.as_str() == ROLAND)
        .expect("Roland's investigator card stayed in cards_in_play");
    assert_eq!(
        roland_card.ability_usage.get(&0),
        Some(&AbilityUsageRecord::new(0, 1)),
        "Roland's reaction (ability index 0) should have recorded one fire \
         in round 0",
    );
}

#[test]
fn once_per_round_limit_blocks_second_reaction_in_same_round() {
    // Pre-populate the counter as if Roland already fired his reaction
    // earlier this round. The scan must see the limit as exhausted and
    // NOT queue the trigger — so no reaction window opens after a
    // second defeat in the same round.
    let (inv_id, enemy_id, loc_id, mut state) = roland_at_location_with_enemy(2, 0);
    {
        let inv = state.investigators.get_mut(&inv_id).unwrap();
        let roland_card = inv
            .cards_in_play
            .iter_mut()
            .find(|c| c.code.as_str() == ROLAND)
            .unwrap();
        roland_card.bump_ability_usage(0, 0);
    }

    let result = apply_no_commits(state, fight_action(inv_id, enemy_id));

    assert_eq!(result.outcome, EngineOutcome::Done);
    // Defeat still happened.
    assert_event!(
        result.events,
        Event::EnemyDefeated { enemy: e, by: Some(by) } if *e == enemy_id && *by == inv_id
    );
    // But Roland's reaction window does NOT open — no triggers were pending
    // after the limit check filtered his ability out. (The Fight's skill test
    // still opens its ST.1/ST.2 framework player windows, #374, which auto-skip;
    // scope the assertion to the AfterEnemyDefeated reaction window.)
    assert_no_event!(
        result.events,
        Event::WindowOpened {
            kind: WindowKind::AfterEnemyDefeated { .. }
        }
    );
    assert_no_event!(
        result.events,
        Event::WindowClosed {
            kind: WindowKind::AfterEnemyDefeated { .. }
        }
    );
    assert_no_event!(result.events, Event::CluePlaced { .. });
    // Clue counts unchanged at the location and on Roland.
    assert_eq!(result.state.locations[&loc_id].clues, 2);
    assert_eq!(result.state.investigators[&inv_id].clues, 0);
}

#[test]
fn lazy_round_reset_re_enables_reaction_in_a_later_round() {
    // Counter sits at "fired in round 0" but the state is now in
    // round 1. The lazy reset (`is_usage_exhausted` returns false when
    // the stored round differs from current) lets the reaction fire
    // again. No explicit round-end hook is invoked — the round counter
    // just advanced.
    let (inv_id, enemy_id, _loc_id, mut state) = roland_at_location_with_enemy(2, 1);
    {
        let inv = state.investigators.get_mut(&inv_id).unwrap();
        let roland_card = inv
            .cards_in_play
            .iter_mut()
            .find(|c| c.code.as_str() == ROLAND)
            .unwrap();
        // Pretend Roland fired in the previous round.
        roland_card.bump_ability_usage(0, 0);
    }

    let mut resolver = ScriptedResolver::new();
    resolver
        .commit_cards(&[])
        .pick_single(game_core::engine::OptionId(0));
    let result = drive(state, fight_action(inv_id, enemy_id), resolver);

    assert_eq!(result.outcome, EngineOutcome::Done);
    assert_event!(
        result.events,
        Event::WindowOpened {
            kind: WindowKind::AfterEnemyDefeated { enemy: e, .. },
        } if *e == enemy_id
    );
    assert_event!(
        result.events,
        Event::CluePlaced { investigator, count: 1 } if *investigator == inv_id
    );

    // Counter now records "fired once in round 1" — the round-0 entry
    // was overwritten in place by the lazy reset.
    let inv = &result.state.investigators[&inv_id];
    let roland_card = inv
        .cards_in_play
        .iter()
        .find(|c| c.code.as_str() == ROLAND)
        .unwrap();
    assert_eq!(
        roland_card.ability_usage.get(&0),
        Some(&AbilityUsageRecord::new(1, 1)),
    );
}

#[test]
fn skipping_the_reaction_window_does_not_bump_the_counter() {
    // Roland's reaction is optional ("[reaction]" not "Forced —"). If
    // the controller chooses Skip, no firing happened, so the counter
    // must stay at 0 — Roland can still react to a later defeat this
    // round.
    let (inv_id, enemy_id, loc_id, state) = roland_at_location_with_enemy(2, 0);

    let mut resolver = ScriptedResolver::new();
    resolver.commit_cards(&[]).skip();
    let result = drive(state, fight_action(inv_id, enemy_id), resolver);

    assert_eq!(result.outcome, EngineOutcome::Done);
    assert_event!(
        result.events,
        Event::WindowOpened {
            kind: WindowKind::AfterEnemyDefeated { enemy: e, .. },
        } if *e == enemy_id
    );
    // Window closed without firing → no clue moved.
    assert_no_event!(result.events, Event::CluePlaced { .. });
    assert_eq!(result.state.locations[&loc_id].clues, 2);
    assert_eq!(result.state.investigators[&inv_id].clues, 0);

    // Counter is untouched.
    let inv = &result.state.investigators[&inv_id];
    let roland_card = inv
        .cards_in_play
        .iter()
        .find(|c| c.code.as_str() == ROLAND)
        .unwrap();
    assert!(
        roland_card.ability_usage.is_empty(),
        "Skip must not record a use; ability_usage = {:?}",
        roland_card.ability_usage,
    );
}

/// Belt-and-suspenders sanity check: the real registry returns
/// Roland's abilities with the once-per-round usage limit set.
/// Card-level unit tests verify this against `super::abilities()`;
/// this verifies the dispatch through `cards::REGISTRY` (which
/// downstream code goes through) sees the same shape.
#[test]
fn registry_returns_reaction_with_once_per_round_limit() {
    install_real_registry();
    let abilities =
        (cards::REGISTRY.abilities_for)(&CardCode::new(ROLAND)).expect("Roland is registered");
    assert_eq!(abilities.len(), 1);
    assert_eq!(
        abilities[0].usage_limit,
        Some(UsageLimit {
            count: 1,
            period: UsagePeriod::Round,
        }),
    );
}
