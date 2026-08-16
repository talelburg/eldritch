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

#[ctor::ctor(unsafe)]
fn install() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
}

#[test]
fn emergency_cache_play_gains_three_resources() {
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
    let paused = game_core::test_support::perform_skill_test(state, INV, SkillKind::Willpower, 1);
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
    // willpower 3 + Numeric(0) vs difficulty 1 → success → draw 1.
    let r = perform_and_commit_guts(guts_board(3, ChaosToken::Numeric(0)));
    assert_eq!(r.outcome, EngineOutcome::Done);
    assert_event!(r.events, Event::CardsDrawn { count: 1, .. });
}

#[test]
fn guts_draws_nothing_on_a_failed_test() {
    // AutoFail → failure → no draw.
    let r = perform_and_commit_guts(guts_board(3, ChaosToken::AutoFail));
    assert_eq!(r.outcome, EngineOutcome::Done);
    assert_no_event!(r.events, Event::CardsDrawn { .. });
}

/// A card-effect draw obeys the empty-deck rule like the Draw action does
/// (#636). Rules Reference, glossary "Drawing Cards": "If an investigator
/// with an empty investigator deck needs to draw a card, that investigator
/// shuffles his or her discard pile back into his or her deck, then draws
/// the card, and upon completion of the entire draw takes one horror."
#[test]
fn guts_on_an_empty_deck_reshuffles_the_discard_and_takes_one_horror() {
    let mut state = guts_board(3, ChaosToken::Numeric(0));
    {
        let inv = state
            .investigators
            .get_mut(&INV)
            .expect("test investigator");
        // Horror lands on the investigator card, so it needs a real code.
        inv.investigator_card.code = CardCode::new("01003"); // Skids O'Toole: 8/6
        inv.deck.clear();
        inv.discard = vec![CardCode::new("spare-1"), CardCode::new("spare-2")];
    }

    let r = perform_and_commit_guts(state);

    assert_eq!(r.outcome, EngineOutcome::Done);
    assert_event!(r.events, Event::DeckShuffled { .. });
    assert_event!(r.events, Event::CardsDrawn { count: 1, .. });
    assert_event!(r.events, Event::HorrorTaken { amount: 1, .. });
    let inv = &r.state.investigators[&INV];
    assert_eq!(
        inv.discard,
        vec![CardCode::new(GUTS)],
        "the two discarded cards shuffled back into the deck; only the \
         just-resolved Guts remains in the discard",
    );
    assert_eq!(inv.deck.len(), 1, "two cards returned, one of them drawn");
    assert_eq!(inv.horror(), 1, "deck-out costs 1 horror, once");
}
