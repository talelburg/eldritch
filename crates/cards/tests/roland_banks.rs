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

use card_dsl::dsl::{UsageLimit, UsagePeriod};
use cards::REGISTRY;
use game_core::engine::enumerate::TurnAction;
use game_core::event::Event;
use game_core::state::{
    AbilityUsageRecord, CardCode, CardInPlay, ChaosBag, ChaosToken, EnemyId, GameState,
    GameStateBuilder, InvestigatorId, LocationId, Skills, TokenModifiers,
};
use game_core::test_support::{self, seat, SeatLookup as _, TestSession};
use game_core::{assert_event, assert_no_event};

/// `ArkhamDB` code for original-Core Roland Banks.
const ROLAND: &str = "01001";

const INV: InvestigatorId = InvestigatorId(1);
const ENEMY: EnemyId = EnemyId(100);
const HERE: LocationId = LocationId(10);

const FIGHT: TurnAction = TurnAction::Fight {
    investigator: INV,
    enemy: ENEMY,
};

#[ctor::ctor(unsafe)]
fn install_real_registry() {
    test_support::install_registry_with_test_cards(REGISTRY);
}

/// Build a Fight-ready scenario with Roland engaged with an enemy at
/// 1 HP and fight 1, so a successful Combat test defeats it. Roland's
/// investigator card is placed in `cards_in_play` (the seat's investigator
/// card stays the test investigator's), so his `[reaction]` is found by the
/// in-play scan; `roland_banks_seated.rs` covers the investigator-card scan.
fn roland_at_location_with_enemy(location_clues: u8, round: u32) -> GameState {
    let mut enemy = test_support::test_enemy(ENEMY.0, "Mock Ghoul");
    enemy.fight = 1;
    enemy.max_health = 1;

    let mut loc = test_support::test_location(HERE.0, "Study");
    loc.clues = location_clues;

    GameStateBuilder::new()
        .with_round(round)
        .with_seat(
            seat(1)
                .at(HERE)
                // Roland's printed combat.
                .skills(Skills {
                    willpower: 3,
                    intellect: 3,
                    combat: 4,
                    agility: 3,
                })
                .in_play([ROLAND]),
        )
        // Co-located: Fight is location-gated (#401).
        .with_enemy_engaged(enemy, INV)
        .with_location(loc)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .open_turn(INV)
        .build()
}

/// Roland's investigator card, in play.
fn roland_card(state: &GameState) -> &CardInPlay {
    let id = state.in_play(INV, ROLAND);
    state.investigators[&INV]
        .cards_in_play
        .iter()
        .find(|c| c.instance_id == id)
        .expect("Roland's investigator card stayed in cards_in_play")
}

/// `state` with Roland's reaction already fired in round 0.
fn fired_in_round_0(mut state: GameState) -> GameState {
    let id = state.in_play(INV, ROLAND);
    state
        .investigators
        .get_mut(&INV)
        .unwrap()
        .cards_in_play
        .iter_mut()
        .find(|c| c.instance_id == id)
        .unwrap()
        .bump_ability_usage(0, 0);
    state
}

/// Fight the enemy from `state`, committing nothing. The session rests at
/// Roland's reaction window if it opened, or back at the turn menu.
fn fight(state: GameState) -> TestSession {
    TestSession::new(state).take(&FIGHT).commit(&[])
}

/// [`fight`], then fire Roland's reaction from its window.
fn fight_and_react(state: GameState) -> TestSession {
    let session = fight(state);
    let roland = session.state().card(INV, ROLAND);
    session.pick(roland)
}

#[test]
fn reaction_fires_after_roland_defeats_enemy_and_discovers_clue() {
    let session = fight_and_react(roland_at_location_with_enemy(2, 0));

    session.assert_at_turn_menu(INV);
    assert_event!(
        session.events(),
        Event::EnemyDefeated { enemy: e, by: Some(by) } if *e == ENEMY && *by == INV
    );
    assert_event!(
        session.events(),
        Event::CluePlaced { investigator, count: 1 } if *investigator == INV
    );

    // 1 of 2 clues stayed at the location; Roland is carrying 1.
    let state = session.state();
    assert_eq!(state.locations[&HERE].clues, 1);
    assert_eq!(state.investigators[&INV].clues, 1);

    // Counter bumped on Roland's investigator card: 1 fire this round.
    assert_eq!(
        roland_card(state).ability_usage.get(&0),
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
    let session = fight(fired_in_round_0(roland_at_location_with_enemy(2, 0)));

    // No reaction window opened: the commit runs straight back to the menu.
    session.assert_at_turn_menu(INV);
    // Defeat still happened.
    assert_event!(
        session.events(),
        Event::EnemyDefeated { enemy: e, by: Some(by) } if *e == ENEMY && *by == INV
    );
    // But Roland's reaction does NOT fire — no triggers were pending after the
    // limit check filtered his ability out, so the window never offered the
    // reaction and resolution went straight through with no clue discovered.
    assert_no_event!(session.events(), Event::CluePlaced { .. });
    // Clue counts unchanged at the location and on Roland.
    assert_eq!(session.state().locations[&HERE].clues, 2);
    assert_eq!(session.state().investigators[&INV].clues, 0);
}

#[test]
fn lazy_round_reset_re_enables_reaction_in_a_later_round() {
    // Counter sits at "fired in round 0" but the state is now in
    // round 1. The lazy reset (`is_usage_exhausted` returns false when
    // the stored round differs from current) lets the reaction fire
    // again. No explicit round-end hook is invoked — the round counter
    // just advanced.
    let session = fight_and_react(fired_in_round_0(roland_at_location_with_enemy(2, 1)));

    session.assert_at_turn_menu(INV);
    assert_event!(
        session.events(),
        Event::CluePlaced { investigator, count: 1 } if *investigator == INV
    );

    // Counter now records "fired once in round 1" — the round-0 entry
    // was overwritten in place by the lazy reset.
    assert_eq!(
        roland_card(session.state()).ability_usage.get(&0),
        Some(&AbilityUsageRecord::new(1, 1)),
    );
}

#[test]
fn skipping_the_reaction_window_does_not_bump_the_counter() {
    // Roland's reaction is optional ("[reaction]" not "Forced —"). If
    // the controller chooses Skip, no firing happened, so the counter
    // must stay at 0 — Roland can still react to a later defeat this
    // round.
    let at_window = fight(roland_at_location_with_enemy(2, 0));
    // The window opened, offering Roland's reaction.
    let roland = at_window.state().card(INV, ROLAND);
    assert_eq!(at_window.offered_at(&roland), 1);
    let session = at_window.skip();

    session.assert_at_turn_menu(INV);
    // It closed without firing → no clue moved.
    assert_no_event!(session.events(), Event::CluePlaced { .. });
    assert_eq!(session.state().locations[&HERE].clues, 2);
    assert_eq!(session.state().investigators[&INV].clues, 0);

    // Counter is untouched.
    let card = roland_card(session.state());
    assert!(
        card.ability_usage.is_empty(),
        "Skip must not record a use; ability_usage = {:?}",
        card.ability_usage,
    );
}

#[test]
fn reaction_not_offered_when_location_has_no_clues() {
    // #495 / RR p.2: "Discover 1 clue at your location" can't change the game
    // state at a 0-clue location, so the reaction must not initiate — the engine
    // must not even open the (skippable) reaction window. If the window opened,
    // the commit would rest on a skippable reaction prompt instead of the
    // open-turn menu.
    let session = fight(roland_at_location_with_enemy(0, 0));

    // The defeat still happened.
    assert_event!(
        session.events(),
        Event::EnemyDefeated { enemy: e, by: Some(by) } if *e == ENEMY && *by == INV
    );
    // No reaction window was opened: the session rests at the open-turn menu,
    // not a reaction prompt.
    session.assert_at_turn_menu(INV);
    // Nothing discovered; counter untouched (the reaction never fired).
    assert_no_event!(session.events(), Event::CluePlaced { .. });
    assert_eq!(session.state().locations[&HERE].clues, 0);
    assert_eq!(session.state().investigators[&INV].clues, 0);
    let card = roland_card(session.state());
    assert!(
        card.ability_usage.is_empty(),
        "a suppressed reaction must not record a use; ability_usage = {:?}",
        card.ability_usage,
    );
}

/// Belt-and-suspenders sanity check: the real registry returns
/// Roland's abilities with the once-per-round usage limit set.
/// Card-level unit tests verify this against `super::abilities()`;
/// this verifies the dispatch through `cards::REGISTRY` (which
/// downstream code goes through) sees the same shape.
#[test]
fn registry_returns_reaction_with_once_per_round_limit() {
    let abilities = (REGISTRY.abilities_for)(&CardCode::new(ROLAND)).expect("Roland is registered");
    assert_eq!(abilities.len(), 2);
    assert_eq!(
        abilities[0].usage_limit,
        Some(UsageLimit {
            count: 1,
            period: UsagePeriod::Round,
        }),
    );
}
