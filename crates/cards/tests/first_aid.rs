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
    AbilitySource, CardCode, CardInPlay, CardInstanceId, InvestigatorId, LocationId, Phase, UseKind,
};
use game_core::test_support::{
    dispatch_turn_action_unchecked, take_turn_action, test_investigator, test_location,
    GameStateBuilder,
};
use game_core::{
    apply, assert_event, legal_actions, Action, InputResponse, OptionId, PlayerAction,
};

const FIRST_AID: &str = "01019";
const INV: InvestigatorId = InvestigatorId(1);
const LOC: LocationId = LocationId(10);
const KIT_INST: CardInstanceId = CardInstanceId(0);

#[ctor::ctor(unsafe)]
fn install() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
}

/// Board: First Aid in play with `supplies` supplies; the active investigator
/// at `LOC` carrying 2 damage and 2 horror to heal from.
fn board(supplies: u8) -> game_core::GameState {
    board_with_harm(supplies, 2, 2)
}

/// [`board`] with the controller's harm dialled explicitly — an undamaged,
/// unhorrored investigator leaves First Aid's heal with nothing to do.
fn board_with_harm(supplies: u8, damage: u8, horror: u8) -> game_core::GameState {
    let mut inv = test_investigator(1);
    // Harm accumulates on the investigator card after #448 cp2a.
    inv.investigator_card.accumulated_damage = damage;
    inv.investigator_card.accumulated_horror = horror;
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
            source: AbilitySource::InPlay(KIT_INST),
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

// -----------------------------------------------------------------------
// #639 — the RR initiation gate on the activation path.
// -----------------------------------------------------------------------

/// RR "Ability": *"A triggered ability can only be initiated if its effect has
/// the potential to change the game state…"* An undamaged, unhorrored solo
/// investigator gives First Aid's heal nothing to remove, so the activation is
/// rejected outright — no action spent, no supply consumed, First Aid still in
/// play with its full pool.
#[test]
fn an_undamaged_solo_investigator_cannot_activate_first_aid() {
    let before = board_with_harm(3, 0, 0);
    let actions_before = before.investigators[&INV].actions_remaining;

    // Bypass the turn menu — which already filters this out (see the sibling
    // test) — to prove the *handler* rejects a directly-submitted activation.
    let r = dispatch_turn_action_unchecked(
        before,
        &TurnAction::ActivateAbility {
            investigator: INV,
            source: AbilitySource::InPlay(KIT_INST),
            ability_index: 0,
        },
    );
    assert!(
        matches!(r.outcome, EngineOutcome::Rejected { .. }),
        "nothing to heal ⇒ the ability cannot initiate: {:?}",
        r.outcome,
    );
    assert_eq!(supplies(&r.state), Some(3), "no supply spent on a reject");
    assert_eq!(
        r.state.investigators[&INV].actions_remaining, actions_before,
        "no action spent on a reject",
    );
    assert!(
        r.state.investigators[&INV]
            .cards_in_play
            .iter()
            .any(|c| c.instance_id == KIT_INST),
        "First Aid not discarded by a depletion that never happened",
    );
}

/// The turn menu agrees with the validator (#639): an activation that
/// `check_activate_ability` would reject is not offered.
#[test]
fn the_turn_menu_does_not_offer_first_aid_with_nothing_to_heal() {
    let activation = TurnAction::ActivateAbility {
        investigator: INV,
        source: AbilitySource::InPlay(KIT_INST),
        ability_index: 0,
    };
    assert!(
        legal_actions(&board_with_harm(3, 2, 2)).contains(&activation),
        "sanity: a damaged investigator is offered the activation",
    );
    assert!(
        !legal_actions(&board_with_harm(3, 0, 0)).contains(&activation),
        "an unharmed investigator is not offered an activation that would reject",
    );
}

/// Board: investigator 1 (the healer, holding First Aid) and investigator 2,
/// both at `LOC`, carrying `healer_damage` / `patient_damage` damage and no
/// horror. Investigator 1 is the active one.
fn two_investigators(healer_damage: u8, patient_damage: u8) -> game_core::GameState {
    let mut healer = test_investigator(1);
    healer.investigator_card.accumulated_damage = healer_damage;
    let mut kit = CardInPlay::enter_play(CardCode::new(FIRST_AID), KIT_INST);
    kit.uses.insert(UseKind::Supplies, 3);
    healer.cards_in_play.push(kit);

    let mut patient = test_investigator(2);
    patient.investigator_card.accumulated_damage = patient_damage;

    GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator_at(healer, LOC)
        .with_investigator_at(patient, LOC)
        .with_location(test_location(10, "Study"))
        .with_active_investigator(INV)
        .with_turn_order([INV])
        .with_investigator_turn(INV)
        .build()
}

/// RR "Target": *"A card is not an eligible target for an ability if the
/// resolution of that ability's effect could not change the target's state."*
/// With two co-located investigators and only one of them damaged, the heal's
/// `Chosen` target grounding offers just the damaged one — so it auto-binds
/// rather than prompting, and the undamaged investigator is untouched. The
/// control below (both damaged) shows the same board *does* prompt when both
/// are eligible, which is what makes the auto-bind evidence of the filter.
#[test]
fn only_a_damaged_investigator_is_offered_as_a_heal_target() {
    // Control: both damaged ⇒ 2 eligible targets ⇒ the pick suspends, and
    // neither is healed until it is answered.
    let r = pick(activate(two_investigators(2, 2)).state, 0);
    let request = match &r.outcome {
        EngineOutcome::AwaitingInput { request, .. } => request,
        other => panic!("two eligible targets must prompt: {other:?}"),
    };
    assert_eq!(
        request.options.len(),
        2,
        "both co-located investigators offered as heal targets: {:?}",
        request.options,
    );

    // Only investigator 2 damaged ⇒ 1 eligible target ⇒ auto-binds, no prompt.
    let r = activate(two_investigators(0, 2));
    assert!(matches!(r.outcome, EngineOutcome::AwaitingInput { .. }));
    let r = pick(r.state, 0);

    assert_eq!(
        r.state.investigators[&InvestigatorId(2)].damage(),
        1,
        "the damaged investigator — the only eligible target — was healed",
    );
    assert_eq!(
        r.state.investigators[&INV].damage(),
        0,
        "the undamaged investigator was never a candidate",
    );
    assert_event!(
        r.events,
        Event::Healed {
            investigator: InvestigatorId(2),
            kind: HarmKind::Damage,
            amount: 1,
            ..
        }
    );
}
