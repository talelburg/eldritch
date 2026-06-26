//! C5d (#239) integration: First Aid 01019's `[action] Spend 1 supply: Heal 1
//! damage or horror from an investigator at your location` end-to-end against
//! the real `cards::REGISTRY`. The damage-or-horror choice is a `ChooseOne`
//! that suspends; resume picks the branch. The `Uses (3 supplies)` pool and
//! the "if no supplies, discard it" depletion-discard come from corpus
//! metadata (#302). Own process → installs `cards::REGISTRY`.

use game_core::dsl::HarmKind;
use game_core::engine::EngineOutcome;
use game_core::engine::TurnAction;
use game_core::event::Event;
use game_core::state::{
    CardCode, CardInPlay, CardInstanceId, InvestigatorId, LocationId, Phase, UseKind,
};
use game_core::test_support::{
    take_turn_action, test_investigator, test_location, GameStateBuilder,
};
use game_core::{apply, assert_event, Action, InputResponse, OptionId, PlayerAction};

const FIRST_AID: &str = "01019";
const INV: InvestigatorId = InvestigatorId(1);
const LOC: LocationId = LocationId(10);
const KIT_INST: CardInstanceId = CardInstanceId(0);

#[ctor::ctor]
fn install() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
}

/// Board: First Aid in play with `supplies` supplies; the active investigator
/// at `LOC` carrying 2 damage and 2 horror to heal from.
fn board(supplies: u8) -> game_core::GameState {
    let mut inv = test_investigator(1);
    // Harm accumulates on the investigator card after #448 cp2a.
    inv.investigator_card.accumulated_damage = 2;
    inv.investigator_card.accumulated_horror = 2;
    let mut kit = CardInPlay::enter_play(CardCode::new(FIRST_AID), KIT_INST);
    kit.uses.insert(UseKind::Supplies, supplies);
    inv.cards_in_play.push(kit);

    GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator_at(inv, LOC)
        .with_location(test_location(10, "Study"))
        .with_active_investigator(INV)
        .with_turn_order([INV])
        .with_investigator_turn(INV)
        .build()
}

fn supplies(state: &game_core::GameState) -> Option<u8> {
    state.investigators[&INV]
        .cards_in_play
        .iter()
        .find(|c| c.instance_id == KIT_INST)
        .map(|c| c.uses.get(&UseKind::Supplies).copied().unwrap_or(0))
}

fn activate(state: game_core::GameState) -> game_core::engine::ApplyResult {
    take_turn_action(
        state,
        &TurnAction::ActivateAbility {
            investigator: INV,
            instance_id: KIT_INST,
            ability_index: 0,
        },
    )
}

fn pick(state: game_core::GameState, branch: u32) -> game_core::engine::ApplyResult {
    apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(OptionId(branch)),
        }),
    )
}

#[test]
fn spends_a_supply_and_heals_one_damage_when_the_damage_branch_is_chosen() {
    // Activate → suspends on the damage-or-horror choice.
    let r = activate(board(3));
    assert!(matches!(r.outcome, EngineOutcome::AwaitingInput { .. }));
    assert_eq!(supplies(&r.state), Some(2), "1 supply spent on activation");

    // Branch 0 = heal damage; the sole co-located investigator (the controller)
    // auto-binds, so this completes.
    let r = pick(r.state, 0);
    assert!(matches!(r.outcome, EngineOutcome::AwaitingInput { .. }));
    assert_eq!(r.state.investigators[&INV].damage(), 1, "1 damage healed");
    assert_eq!(r.state.investigators[&INV].horror(), 2, "horror untouched");
    assert_event!(
        r.events,
        Event::Healed {
            kind: HarmKind::Damage,
            amount: 1,
            ..
        }
    );
}

#[test]
fn heals_one_horror_when_the_horror_branch_is_chosen() {
    let r = activate(board(3));
    assert!(matches!(r.outcome, EngineOutcome::AwaitingInput { .. }));

    // Branch 1 = heal horror.
    let r = pick(r.state, 1);
    assert!(matches!(r.outcome, EngineOutcome::AwaitingInput { .. }));
    assert_eq!(r.state.investigators[&INV].horror(), 1, "1 horror healed");
    assert_eq!(r.state.investigators[&INV].damage(), 2, "damage untouched");
    assert_event!(
        r.events,
        Event::Healed {
            kind: HarmKind::Horror,
            amount: 1,
            ..
        }
    );
}

#[test]
fn spending_the_last_supply_discards_first_aid() {
    // 1 supply: the activation's SpendUses empties the pool, so the
    // depletion-discard (#302) fires during cost payment — First Aid is gone
    // before the heal's choice even suspends.
    let r = activate(board(1));
    assert!(matches!(r.outcome, EngineOutcome::AwaitingInput { .. }));
    assert!(
        r.state.investigators[&INV].cards_in_play.is_empty(),
        "First Aid discarded when its last supply was spent",
    );
    assert_eq!(
        r.state.investigators[&INV].discard,
        vec![CardCode::new(FIRST_AID)],
    );

    // The heal still resolves (the ability continues even though its source
    // left play).
    let r = pick(r.state, 0);
    assert!(matches!(r.outcome, EngineOutcome::AwaitingInput { .. }));
    assert_eq!(
        r.state.investigators[&INV].damage(),
        1,
        "heal still applied"
    );
}
