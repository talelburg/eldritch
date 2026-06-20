//! Registry-backed tests for the legal-action enumerator's card actions
//! (`PlayCard`, `ActivateAbility`) — slice 2a-ii-3 (#393). These need real card
//! metadata/abilities, so they install `cards::REGISTRY` and live here rather
//! than in `game-core`'s registry-less unit tests.

use std::sync::Once;

use game_core::state::{
    CardCode, CardInPlay, CardInstanceId, ChaosBag, ChaosToken, Continuation, InvestigationResume,
    InvestigatorId, Phase,
};
use game_core::test_support::{test_investigator, test_location, GameStateBuilder};
use game_core::{legal_actions, Action, EngineOutcome, LocationId, PlayerAction};

const HOLY_ROSARY: &str = "01059"; // Mystic asset, cost 2, constant +1 willpower.
const FLASHLIGHT: &str = "01087"; // Asset with an activated ability (uses: Supplies).
const INV: InvestigatorId = InvestigatorId(1);
const LOC: LocationId = LocationId(10);

fn install_real_registry() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let _ = game_core::card_registry::install(cards::REGISTRY);
    });
}

/// A single-investigator open-turn state (`InvestigatorTurn` frame on top of the
/// `InvestigationPhase` anchor) with `hand` in hand and `in_play` in play, 3
/// actions, 9 resources, on a revealed location, non-empty chaos bag.
fn open_turn_state(hand: &[&str], in_play: Vec<CardInPlay>) -> game_core::GameState {
    install_real_registry();
    let mut inv = test_investigator(1);
    inv.current_location = Some(LOC);
    inv.actions_remaining = 3;
    inv.resources = 9;
    inv.hand = hand.iter().map(|c| CardCode::new(*c)).collect();
    inv.cards_in_play = in_play;
    GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator(inv)
        .with_location(test_location(LOC.0, "Study"))
        .with_active_investigator(INV)
        .with_turn_order([INV])
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_phase_anchor(Continuation::InvestigationPhase {
            resume: InvestigationResume::TurnBegins,
        })
        .with_investigator_turn(INV)
        .build()
}

#[test]
fn play_card_offered_for_a_playable_hand_card() {
    let state = open_turn_state(&[HOLY_ROSARY], Vec::new());
    assert!(legal_actions(&state).contains(&PlayerAction::PlayCard {
        investigator: INV,
        hand_index: 0,
    }));
}

/// Flashlight in play with 3 Supplies uses, ready — its `ability_index: 0`
/// activated ability is usable.
fn flashlight_in_play(instance: CardInstanceId) -> CardInPlay {
    use game_core::state::UseKind;
    let mut torch = CardInPlay::enter_play(CardCode::new(FLASHLIGHT), instance);
    torch.uses.insert(UseKind::Supplies, 3);
    torch
}

#[test]
fn activate_offered_for_an_in_play_activated_ability() {
    let inst = CardInstanceId(0);
    let state = open_turn_state(&[], vec![flashlight_in_play(inst)]);
    assert!(
        legal_actions(&state).contains(&PlayerAction::ActivateAbility {
            investigator: INV,
            instance_id: inst,
            ability_index: 0,
        })
    );
}

#[test]
fn every_enumerated_action_applies_without_rejection_with_registry() {
    // Cross-check, registry edition: with real card data the enumeration
    // includes PlayCard (Holy Rosary) and ActivateAbility (Flashlight) alongside
    // the basic actions; each applies without Rejected (Done or AwaitingInput
    // are both acceptance).
    let state = open_turn_state(&[HOLY_ROSARY], vec![flashlight_in_play(CardInstanceId(0))]);
    for action in legal_actions(&state) {
        let result = game_core::apply(state.clone(), Action::Player(action.clone()));
        assert!(
            !matches!(result.outcome, EngineOutcome::Rejected { .. }),
            "enumerated action {action:?} was rejected: {:?}",
            result.outcome,
        );
    }
}
