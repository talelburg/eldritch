//! C5d (#239) integration: First Aid 01019's `[action] Spend 1 supply: Heal 1
//! damage or horror from an investigator at your location` end-to-end against
//! the real `cards::REGISTRY`. The damage-or-horror choice is a `ChooseOne`
//! that suspends; resume picks the branch. The `Uses (3 supplies)` pool and
//! the "if no supplies, discard it" depletion-discard come from corpus
//! metadata (#302). Runs against `cards::TEST_REGISTRIES`.

use card_dsl::dsl::HarmKind;
use cards::TEST_REGISTRIES;
use game_core::assert_event;
use game_core::engine::enumerate::{self, TurnAction};
use game_core::engine::{OptionTarget, PromptNature};
use game_core::event::Event;
use game_core::state::{
    AbilityAddress, AbilitySource, CardCode, CardInstanceId, GameState, GameStateBuilder,
    InvestigatorId, LocationId, UseKind,
};
use game_core::test_support::{self, seat, uses, Seat, SeatLookup as _, TestSession};

const FIRST_AID: &str = "01019";
const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(10);

/// Investigator 1 at the Study carrying `damage` damage and `horror` horror,
/// with First Aid in play holding `supplies` supplies.
fn healer(supplies: u8, damage: u8, horror: u8) -> Seat {
    seat(1)
        .at(HERE)
        .harm(damage, horror)
        .in_play([(FIRST_AID, uses(UseKind::Supplies, supplies))])
}

/// Board: First Aid in play with `supplies` supplies; the active investigator
/// at `HERE` carrying 2 damage and 2 horror to heal from.
fn board(supplies: u8) -> GameStateBuilder {
    board_with_harm(supplies, 2, 2)
}

/// [`board`] with the controller's harm dialled explicitly — an undamaged,
/// unhorrored investigator leaves First Aid's heal with nothing to do.
fn board_with_harm(supplies: u8, damage: u8, horror: u8) -> GameStateBuilder {
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(healer(supplies, damage, horror))
        .with_location(test_support::test_location(HERE.0, "Study"))
        .open_turn(INV)
}

/// The seated First Aid's instance id.
fn kit(state: &GameState) -> CardInstanceId {
    state.in_play(INV, FIRST_AID)
}

fn supplies(state: &GameState) -> Option<u8> {
    state.investigators[&INV]
        .cards_in_play
        .iter()
        .find(|c| c.code.as_str() == FIRST_AID)
        .map(|c| c.uses.get(&UseKind::Supplies).copied().unwrap_or(0))
}

/// First Aid's `[action]` ability.
fn activation(state: &GameState) -> TurnAction {
    TurnAction::ActivateAbility {
        investigator: INV,
        source: AbilitySource::InPlay(kit(state)),
        address: AbilityAddress::Printed(0),
    }
}

fn activate(session: TestSession) -> TestSession {
    let activation = activation(session.state());
    session.take(&activation)
}

#[test]
fn spends_a_supply_and_heals_one_damage_when_the_damage_branch_is_chosen() {
    // Activate → suspends on the damage-or-horror choice.
    let s = activate(board(3).session());
    assert_eq!(
        s.prompt().nature,
        PromptNature::Decision,
        "suspends on the damage-or-horror choice"
    );
    assert_eq!(supplies(s.state()), Some(2), "1 supply spent on activation");

    // Branch 0 = heal damage; the sole co-located investigator (the controller)
    // auto-binds, so this completes.
    let s = s.pick_nth(0);
    s.assert_at_turn_menu(INV);
    assert_eq!(s.state().investigators[&INV].damage(), 1, "1 damage healed");
    assert_eq!(
        s.state().investigators[&INV].horror(),
        2,
        "horror untouched"
    );
    assert_event!(
        s.events(),
        Event::Healed {
            kind: HarmKind::Damage,
            amount: 1,
            ..
        }
    );
}

#[test]
fn heals_one_horror_when_the_horror_branch_is_chosen() {
    let s = activate(board(3).session());
    assert_eq!(
        s.prompt().nature,
        PromptNature::Decision,
        "suspends on the damage-or-horror choice"
    );

    // Branch 1 = heal horror.
    let s = s.pick_nth(1);
    s.assert_at_turn_menu(INV);
    assert_eq!(s.state().investigators[&INV].horror(), 1, "1 horror healed");
    assert_eq!(
        s.state().investigators[&INV].damage(),
        2,
        "damage untouched"
    );
    assert_event!(
        s.events(),
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
    // depletion discard — "If First Aid has no supplies, discard it.", a
    // constant ability (#353) — fires during cost payment, through the one
    // uses-spending path: First Aid is gone before the heal's choice even
    // suspends.
    let ready = board(1).session();
    let kit_inst = kit(ready.state());
    let s = activate(ready);
    assert_eq!(
        s.prompt().nature,
        PromptNature::Decision,
        "suspends on the damage-or-horror choice"
    );
    assert!(
        s.state().investigators[&INV].cards_in_play.is_empty(),
        "First Aid discarded when its last supply was spent",
    );
    assert_eq!(
        s.state().investigators[&INV].discard,
        vec![CardCode::new(FIRST_AID)],
    );
    let spent = s
        .events()
        .iter()
        .position(|e| matches!(e, Event::UsesSpent { instance_id, .. } if *instance_id == kit_inst))
        .expect("UsesSpent emitted");
    let discarded = s
        .events()
        .iter()
        .position(
            |e| matches!(e, Event::CardDiscarded { code, .. } if *code == CardCode::new(FIRST_AID)),
        )
        .expect("CardDiscarded emitted");
    assert!(
        spent < discarded,
        "the spend is announced before the discard"
    );

    // The heal still resolves (the ability continues even though its source
    // left play).
    let s = s.pick_nth(0);
    s.assert_at_turn_menu(INV);
    assert_eq!(
        s.state().investigators[&INV].damage(),
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
    let before = board_with_harm(3, 0, 0).session();
    let actions_before = before.state().investigators[&INV].actions_remaining;
    let kit_inst = kit(before.state());

    // Bypass the turn menu — which already filters this out (see the sibling
    // test) — to prove the *handler* rejects a directly-submitted activation.
    let activation = activation(before.state());
    let r = before.submit_off_menu(&activation);
    // Nothing to heal ⇒ the ability cannot initiate.
    let _ = r.expect_rejected();
    assert_eq!(supplies(r.state()), Some(3), "no supply spent on a reject");
    assert_eq!(
        r.state().investigators[&INV].actions_remaining,
        actions_before,
        "no action spent on a reject",
    );
    assert!(
        r.state().investigators[&INV]
            .cards_in_play
            .iter()
            .any(|c| c.instance_id == kit_inst),
        "First Aid not discarded by a depletion that never happened",
    );
}

/// The turn menu agrees with the validator (#639): an activation that
/// `check_activate_ability` would reject is not offered.
#[test]
fn the_turn_menu_does_not_offer_first_aid_with_nothing_to_heal() {
    let harmed = board_with_harm(3, 2, 2).build();
    assert!(
        enumerate::legal_actions(&harmed, TEST_REGISTRIES.cards).contains(&activation(&harmed)),
        "sanity: a damaged investigator is offered the activation",
    );
    let unharmed = board_with_harm(3, 0, 0).build();
    assert!(
        !enumerate::legal_actions(&unharmed, TEST_REGISTRIES.cards)
            .contains(&activation(&unharmed)),
        "an unharmed investigator is not offered an activation that would reject",
    );
}

/// Board: investigator 1 (the healer, holding First Aid) and investigator 2,
/// both at `HERE`, carrying `healer_damage` / `patient_damage` damage and no
/// horror. Investigator 1 is the active one.
fn two_investigators(healer_damage: u8, patient_damage: u8) -> GameStateBuilder {
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(healer(3, healer_damage, 0))
        .with_seat(seat(2).at(HERE).harm(patient_damage, 0))
        .with_location(test_support::test_location(HERE.0, "Study"))
        .open_turn(INV)
}

/// RR "Target": *"A card is not an eligible target for an ability if the
/// resolution of that ability's effect could not change the target's state."*
/// With two co-located investigators and only one of them damaged, the heal's
/// `Chosen` target grounding offers just the damaged one — so it auto-binds
/// rather than prompting, and the undamaged investigator is untouched. The
/// control below (both damaged) shows the same board *does* prompt when both
/// are eligible, which is what makes the auto-bind evidence of the filter.
///
/// Neither board carries horror, so #664's mode filter drops First Aid's horror
/// branch and the damage-or-horror choice auto-resolves: the *first* suspension
/// on these boards is the target grounding, not the mode pick.
#[test]
fn only_a_damaged_investigator_is_offered_as_a_heal_target() {
    // Control: both damaged ⇒ 2 eligible targets ⇒ the pick suspends, and
    // neither is healed until it is answered.
    let s = activate(two_investigators(2, 2).session());
    let options = &s.prompt().options;
    assert_eq!(
        options.len(),
        2,
        "both co-located investigators offered as heal targets: {options:?}",
    );
    assert_eq!(
        s.state().investigators[&INV].damage(),
        2,
        "nobody healed until the target pick is answered",
    );

    // Only investigator 2 damaged ⇒ 1 eligible target ⇒ auto-binds, no prompt.
    let s = activate(two_investigators(0, 2).session());

    assert_eq!(
        s.state().investigators[&InvestigatorId(2)].damage(),
        1,
        "the damaged investigator — the only eligible target — was healed",
    );
    assert_eq!(
        s.state().investigators[&INV].damage(),
        0,
        "the undamaged investigator was never a candidate",
    );
    assert_event!(
        s.events(),
        Event::Healed {
            investigator: InvestigatorId(2),
            kind: HarmKind::Damage,
            amount: 1,
            ..
        }
    );
}

// -----------------------------------------------------------------------
// #664 — the same gate, applied per *mode* rather than per ability.
// -----------------------------------------------------------------------

/// RR "Ability": *"A triggered ability can only be initiated if its effect has
/// the potential to change the game state…"* — read per branch. A damaged,
/// unhorrored solo investigator leaves First Aid's horror mode with nothing to
/// remove, so only the damage mode is live; one live branch auto-resolves, so
/// the damage-or-horror prompt never appears and the supply cannot be burned on
/// a dead mode.
#[test]
fn a_dead_mode_is_not_offered() {
    let s = activate(board_with_harm(3, 2, 0).session());
    assert_eq!(supplies(s.state()), Some(2), "1 supply spent on activation");
    assert_eq!(
        s.state().investigators[&INV].damage(),
        1,
        "the sole live mode auto-resolved and healed 1 damage",
    );
    assert_event!(
        s.events(),
        Event::Healed {
            kind: HarmKind::Damage,
            amount: 1,
            ..
        }
    );
}

/// The offer itself, for the damaged-and-unhorrored board: with
/// `interactive_acknowledge` on, the lone live mode surfaces as a one-option
/// prompt (#466) — and answering it heals damage, so the option offered is the
/// damage mode and the horror mode is absent from the list.
#[test]
fn only_the_damage_mode_is_offered_with_no_horror_to_heal() {
    let mut state = board_with_harm(3, 2, 0).build();
    state.interactive_acknowledge = true;
    let s = activate(TestSession::new_with_registries(state, TEST_REGISTRIES));
    let options = &s.prompt().options;
    assert_eq!(
        options.len(),
        1,
        "only the damage mode is offered: {options:?}",
    );

    // Option 0 = the damage mode; the heal's target grounding then raises its
    // own one-option prompt (the sole co-located investigator).
    let me = s.state().investigators[&INV].card_anchor();
    let s = s.pick_nth(0).pick(me);
    assert_eq!(
        s.state().investigators[&INV].damage(),
        1,
        "the offered option resolved the damage branch",
    );
    assert_event!(
        s.events(),
        Event::Healed {
            kind: HarmKind::Damage,
            amount: 1,
            ..
        }
    );
}

/// The control for [`a_dead_mode_is_not_offered`]: with both harms present both
/// modes are live, so the damage-or-horror choice still prompts with two
/// options. Without this the filter could offer *nothing* and the sibling test
/// would not notice.
#[test]
fn both_modes_are_offered_when_both_harms_are_present() {
    let s = activate(board_with_harm(3, 2, 2).session());
    let options = &s.prompt().options;
    assert_eq!(
        options.len(),
        2,
        "damage and horror both offered: {options:?}",
    );
    assert_eq!(
        s.state().investigators[&INV].damage(),
        2,
        "no branch resolves until the mode is picked",
    );
}

/// `OptionId` indexes the **filtered** list, so a filtered-out mode shifts the
/// ones behind it: with only horror to heal, offered option 0 is the *horror*
/// branch (branch 1 of the printed `ChooseOne`), not the damage branch. Driven
/// with `interactive_acknowledge` on so the single live mode surfaces as a
/// one-option prompt (#466) rather than auto-binding.
#[test]
fn the_offered_index_names_the_live_mode_not_the_printed_one() {
    let mut state = board_with_harm(3, 0, 2).build();
    state.interactive_acknowledge = true;
    let s = activate(TestSession::new_with_registries(state, TEST_REGISTRIES));
    let options = &s.prompt().options;
    assert_eq!(
        options.len(),
        1,
        "only the horror mode is live: {options:?}",
    );

    // Option 0 = the horror mode; the heal's target grounding then raises its
    // own one-option prompt (the sole co-located investigator) under
    // `interactive_acknowledge`.
    let me = s.state().investigators[&INV].card_anchor();
    let s = s.pick_nth(0).pick(me);
    assert_eq!(
        s.state().investigators[&INV].horror(),
        1,
        "offered option 0 resolved the horror branch",
    );
    assert_event!(
        s.events(),
        Event::Healed {
            kind: HarmKind::Horror,
            amount: 1,
            ..
        }
    );
}

/// **The damage-or-horror choice anchors to the First Aid asset** (#834).
///
/// #775 gave a `ChooseOne` inside a *forced* or *reaction* ability its card's
/// anchor, but the activation path narrowed its `AbilitySource` to a bare
/// `CardInstanceId` on the way into the evaluator and left the anchor unset —
/// so this prompt rendered in the banner. Collapsing `EvalContext` onto the one
/// source field carries the activation's own source through, and ADR 0011 is
/// why that matters: *"the client never decides where a control belongs — it
/// reads the anchor the engine attached"*.
#[test]
fn the_damage_or_horror_choice_anchors_to_the_asset_it_is_printed_on() {
    let ready = board(3).session();
    let kit_inst = kit(ready.state());
    let s = activate(ready);
    assert_eq!(
        s.offered_at(&OptionTarget::CardInstance(kit_inst)),
        2,
        "both modes live: {:?}",
        s.prompt(),
    );
    for option in &s.prompt().options {
        assert_eq!(
            option.target,
            Some(OptionTarget::CardInstance(kit_inst)),
            "anchored to the First Aid asset the ability is printed on",
        );
    }
}

/// **The choice survives its own card's depletion-discard** (#845).
///
/// Spending the *last* supply discards First Aid during cost payment, so the
/// damage-or-horror prompt that follows is built with its source already in the
/// discard pile. Anchoring it to that card left it unrenderable: ADR 0011 —
/// *"an option the engine anchors to a surface the client does not render is
/// unreachable rather than merely misplaced"* — so the prompt was unanswerable
/// and the game could not advance. The anchor now degrades to `None`, taking
/// ADR 0011's other branch (*"leave it un-anchored and accept the banner"*).
///
/// The ability still resolves in full: RR Appendix I step 4, *"If the ability
/// being initiated is on an in-play card, the sequence does not stop from
/// completing if that card leaves play during the sequence."*
#[test]
fn the_choice_offered_after_the_last_supply_is_answerable() {
    let s = activate(board(1).session());
    assert!(
        s.state().investigators[&INV].cards_in_play.is_empty(),
        "First Aid left play during cost payment",
    );

    let options = s.prompt().options.clone();
    assert_eq!(options.len(), 2, "both modes still live: {options:?}");
    for option in &options {
        assert_eq!(
            option.target, None,
            "un-anchored, so the prompt banner renders it: {option:?}",
        );
    }

    // Un-anchored is not merely cosmetic here — the pick has to land.
    let s = s.pick_nth(1);
    assert_eq!(
        s.state().investigators[&INV].horror(),
        1,
        "the horror branch resolved from a source that had left play",
    );
}

/// The control for [`the_choice_offered_after_the_last_supply_is_answerable`]:
/// with supplies to spare First Aid is still in play when the prompt is built,
/// so the anchor is kept. Without this the liveness gate could return `None`
/// unconditionally and the sibling test would not notice.
#[test]
fn the_choice_keeps_its_anchor_while_first_aid_is_still_in_play() {
    let ready = board(2).session();
    let kit_inst = kit(ready.state());
    let s = activate(ready);
    assert_eq!(
        s.offered_at(&OptionTarget::CardInstance(kit_inst)),
        2,
        "both modes live: {:?}",
        s.prompt(),
    );
    for option in &s.prompt().options {
        assert_eq!(
            option.target,
            Some(OptionTarget::CardInstance(kit_inst)),
            "still in play, so still anchored to the asset",
        );
    }
}

/// **The damage-or-horror choice is a *decision*, and the heal's target pick is
/// a *selection*** (ADR 0015). One activation raises both, in that order, so the
/// two natures are pinned against the same fixture.
///
/// The nature is what the client maps to a presentation: a decision presents
/// itself the moment it arises, a selection keeps its context menu. Nothing in
/// the type catches an omitted classification — `Selection` is the silent
/// default — so this assertion is what stands in for the compiler (ADR 0011 made
/// the identical trade for un-anchored options).
#[test]
fn the_damage_or_horror_choice_is_a_decision_and_the_heal_target_is_a_selection() {
    // Two co-located investigators, both hurt, so the heal's target grounding is
    // a genuine multi-candidate selection rather than an auto-bind.
    let s = activate(board(3).with_seat(seat(2).at(HERE).harm(2, 2)).session());
    let request = s.prompt();
    assert_eq!(
        request.nature,
        PromptNature::Decision,
        "the two heal modes are alternatives printed on First Aid, not board \
         entities to disambiguate between: {request:?}",
    );

    // Branch 0 = heal damage; grounding its `InvestigatorTarget::Chosen` offers
    // the two co-located investigators — board entities, so a selection.
    let s = s.pick_nth(0);
    let request = s.prompt();
    assert_eq!(
        request.options.len(),
        2,
        "both investigators live: {request:?}"
    );
    assert_eq!(
        request.nature,
        PromptNature::Selection,
        "picking which investigator to heal is a selection, and `Selection` is \
         deliberate here rather than merely absent: {request:?}",
    );
}
