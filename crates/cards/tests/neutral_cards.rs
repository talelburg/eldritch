//! C6c (#243) integration: the neutral cards' new effect shapes end-to-end
//! against the real `cards::REGISTRY`.
//!
//! - Emergency Cache 01088: `on_play(gain_resources(You, 3))`.
//! - Guts 01089: `on_skill_test_resolution(Success, draw_cards(You, 1))` —
//!   draws on a successful committed-to test, not on a failed one.
//!
//! The other three draw-skills (Perception/Overpower/Manual Dexterity) are
//! structurally identical to Guts; their impl unit tests cover them.
//!
//! Own process → installs `cards::REGISTRY`.

use std::sync::Once;

use game_core::engine::TurnAction;
use game_core::engine::{EngineOutcome, OptionId};
use game_core::event::Event;
use game_core::state::{
    CardCode, ChaosBag, ChaosToken, InvestigatorId, Phase, SkillKind, TokenModifiers,
};
use game_core::test_support::{take_turn_action, test_investigator, GameStateBuilder};
use game_core::{assert_event, assert_no_event, Action, GameState, InputResponse, PlayerAction};

const EMERGENCY_CACHE: &str = "01088";
const GUTS: &str = "01089";
const INV: InvestigatorId = InvestigatorId(1);

fn install() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let _ = game_core::card_registry::install(cards::REGISTRY);
    });
}

#[test]
fn emergency_cache_play_gains_three_resources() {
    install();
    let mut inv = test_investigator(1);
    inv.hand = vec![CardCode::new(EMERGENCY_CACHE)];
    let before = inv.resources;
    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_active_investigator(INV)
        .with_turn_order([INV])
        .with_investigator_turn(INV)
        .with_investigator(inv)
        .build();

    let r = take_turn_action(
        state,
        &TurnAction::PlayCard {
            investigator: INV,
            hand_index: 0,
        },
    );

    assert_event!(r.events, Event::ResourcesGained { amount: 3, .. });
    assert_eq!(r.state.investigators[&INV].resources, before + 3);
}

/// Guts holding `GUTS` + spare deck cards, willpower `wp`, with a chaos bag
/// of `token` (`Numeric(0)` → success vs difficulty 1; `AutoFail` → failure).
fn guts_board(wp: i8, token: ChaosToken) -> GameState {
    let mut inv = test_investigator(1);
    inv.skills.willpower = wp;
    inv.hand = vec![CardCode::new(GUTS)];
    inv.deck = vec![CardCode::new("spare-1"), CardCode::new("spare-2")];
    GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_active_investigator(INV)
        .with_turn_order([INV])
        .with_investigator(inv)
        .with_chaos_bag(ChaosBag::new([token]))
        .with_token_modifiers(TokenModifiers::default())
        .build()
}

fn perform_and_commit_guts(state: GameState) -> game_core::engine::ApplyResult {
    let paused = game_core::engine::apply(
        state,
        Action::Player(PlayerAction::PerformSkillTest {
            investigator: INV,
            skill: SkillKind::Willpower,
            difficulty: 1,
        }),
    );
    game_core::engine::apply(
        paused.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickMultiple {
                selected: vec![OptionId(0)],
            },
        }),
    )
}

#[test]
fn guts_draws_a_card_on_a_successful_test() {
    install();
    // willpower 3 + Numeric(0) vs difficulty 1 → success → draw 1.
    let r = perform_and_commit_guts(guts_board(3, ChaosToken::Numeric(0)));
    assert_eq!(r.outcome, EngineOutcome::Done);
    assert_event!(r.events, Event::CardsDrawn { count: 1, .. });
}

#[test]
fn guts_draws_nothing_on_a_failed_test() {
    install();
    // AutoFail → failure → no draw.
    let r = perform_and_commit_guts(guts_board(3, ChaosToken::AutoFail));
    assert_eq!(r.outcome, EngineOutcome::Done);
    assert_no_event!(r.events, Event::CardsDrawn { .. });
}
