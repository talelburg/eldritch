//! Registry-backed tests for the legal-action enumerator (#393 slice 2a-ii):
//! the card actions (`PlayCard`, `ActivateAbility`, added in 2a-ii-3) plus the
//! whole-enumeration sweep covering every action category (2a-ii-4). These need
//! real card metadata/abilities, so they install `cards::REGISTRY` and live here
//! rather than in `game-core`'s registry-less unit tests.

use std::sync::Once;

use game_core::state::{
    CardCode, CardInPlay, CardInstanceId, ChaosBag, ChaosToken, Continuation, InvestigationResume,
    InvestigatorId, Phase,
};
use game_core::test_support::{test_investigator, test_location, GameStateBuilder};
use game_core::{
    legal_actions, Action, EngineOutcome, InputResponse, LocationId, OptionId, PlayerAction,
    TurnAction,
};

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
    // Real investigator code so max_health()/max_sanity() reads from the
    // installed cards registry (#448 cp2a). Skids O'Toole (01003, 8/6).
    inv.investigator_card.code = CardCode::new("01003");
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
    assert!(legal_actions(&state).contains(&TurnAction::PlayCard {
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
        legal_actions(&state).contains(&TurnAction::ActivateAbility {
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
    // OptionId round-trip: each enumerated action dispatches via
    // `ResolveInput(PickSingle(OptionId))` at the open turn (#447). None reject.
    let actions = legal_actions(&state);
    for (i, action) in actions.iter().enumerate() {
        let result = game_core::apply(
            state.clone(),
            Action::Player(PlayerAction::ResolveInput {
                response: InputResponse::PickSingle(OptionId(
                    u32::try_from(i).expect("action index fits u32"),
                )),
            }),
        );
        assert!(
            !matches!(result.outcome, EngineOutcome::Rejected { .. }),
            "enumerated action {action:?} (OptionId {i}) was rejected: {:?}",
            result.outcome,
        );
    }
}

#[test]
fn full_enumeration_covers_every_action_category_and_all_apply() {
    use game_core::state::{Act, EnemyId};

    let inst = CardInstanceId(0);
    let mut state = open_turn_state(&[HOLY_ROSARY], vec![flashlight_in_play(inst)]);
    // A connected destination (Move), an engaged enemy (Fight/Evade), a
    // co-located unengaged enemy (Engage), and an advanceable act (AdvanceAct).
    let mut other = test_location(11, "Hall");
    other.revealed = true;
    state
        .locations
        .get_mut(&LOC)
        .unwrap()
        .connections
        .push(other.id);
    let other_id = other.id;
    state.locations.insert(other_id, other);

    let mut foe = game_core::test_support::test_enemy(7, "Ghoul");
    foe.engaged_with = Some(INV);
    foe.current_location = Some(LOC);
    state.enemies.insert(EnemyId(7), foe);
    let mut rat = game_core::test_support::test_enemy(8, "Rat");
    rat.current_location = Some(LOC);
    state.enemies.insert(EnemyId(8), rat);

    state.investigators.get_mut(&INV).unwrap().clues = 2;
    state.act_deck = vec![
        Act {
            code: CardCode("_act1".into()),
            clue_threshold: 2,
            resolution: None,
        },
        Act {
            code: CardCode("_act2".into()),
            clue_threshold: 99,
            resolution: None,
        },
    ];

    let actions = legal_actions(&state);

    // Every category is represented.
    let has = |p: fn(&TurnAction) -> bool| actions.iter().any(p);
    assert!(actions.contains(&TurnAction::EndTurn), "EndTurn");
    assert!(has(|a| matches!(a, TurnAction::Move { .. })), "Move");
    assert!(
        has(|a| matches!(a, TurnAction::Investigate { .. })),
        "Investigate"
    );
    assert!(
        has(|a| matches!(a, TurnAction::Resource { .. })),
        "Resource"
    );
    assert!(has(|a| matches!(a, TurnAction::Draw { .. })), "Draw");
    assert!(has(|a| matches!(a, TurnAction::Fight { .. })), "Fight");
    assert!(has(|a| matches!(a, TurnAction::Evade { .. })), "Evade");
    assert!(has(|a| matches!(a, TurnAction::Engage { .. })), "Engage");
    assert!(
        has(|a| matches!(a, TurnAction::PlayCard { .. })),
        "PlayCard"
    );
    assert!(
        has(|a| matches!(a, TurnAction::ActivateAbility { .. })),
        "ActivateAbility"
    );
    assert!(
        has(|a| matches!(a, TurnAction::AdvanceAct { .. })),
        "AdvanceAct"
    );

    // And all of them apply without Rejected — via the OptionId round-trip
    // (`ResolveInput(PickSingle(OptionId))` at the open turn, #447).
    for (i, action) in actions.iter().enumerate() {
        let result = game_core::apply(
            state.clone(),
            Action::Player(PlayerAction::ResolveInput {
                response: InputResponse::PickSingle(OptionId(
                    u32::try_from(i).expect("action index fits u32"),
                )),
            }),
        );
        assert!(
            !matches!(result.outcome, EngineOutcome::Rejected { .. }),
            "enumerated action {action:?} (OptionId {i}) was rejected: {:?}",
            result.outcome,
        );
    }
}
