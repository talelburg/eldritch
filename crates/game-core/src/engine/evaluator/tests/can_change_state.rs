use super::*;

#[test]
fn discover_clue_can_change_state_only_with_clues_present() {
    // #495 / RR p.2: discovering a clue at your location changes state iff
    // there is a clue to discover.
    let discover = dsl::discover_clue(LocationTarget::YourLocation, 1);

    let with_clues = state_with_clues_at_location(2);
    assert!(effect_can_change_state(&with_clues, ctx(1), &discover));

    let no_clues = state_with_clues_at_location(0);
    assert!(!effect_can_change_state(&no_clues, ctx(1), &discover));
}

#[test]
fn discover_zero_count_cannot_change_state() {
    let discover = dsl::discover_clue(LocationTarget::YourLocation, 0);
    let with_clues = state_with_clues_at_location(2);
    assert!(!effect_can_change_state(&with_clues, ctx(1), &discover));
}

#[test]
fn seq_can_change_state_iff_any_step_can() {
    let no_clues = state_with_clues_at_location(0);
    // Both steps inert (discover at 0-clue location) → Seq inert.
    let inert = dsl::seq(vec![
        dsl::discover_clue(LocationTarget::YourLocation, 1),
        dsl::discover_clue(LocationTarget::YourLocation, 1),
    ]);
    assert!(!effect_can_change_state(&no_clues, ctx(1), &inert));
    // One step meaningful (gain resources, conservatively state-changing).
    let mixed = dsl::seq(vec![
        dsl::discover_clue(LocationTarget::YourLocation, 1),
        dsl::gain_resources(InvestigatorTarget::You, 1),
    ]);
    assert!(effect_can_change_state(&no_clues, ctx(1), &mixed));
}

#[test]
fn choose_one_can_change_state_iff_any_branch_can() {
    let no_clues = state_with_clues_at_location(0);
    let choice = dsl::choose_one([
        (
            "Discover 1 clue",
            dsl::discover_clue(LocationTarget::YourLocation, 1),
        ),
        (
            "Gain 1 resource",
            dsl::gain_resources(InvestigatorTarget::You, 1),
        ),
    ]);
    assert!(effect_can_change_state(&no_clues, ctx(1), &choice));
}

#[test]
fn unknown_effects_are_conservatively_state_changing() {
    // Default arm: anything we can't prove inert is assumed to change state,
    // so a meaningful ability is never wrongly suppressed.
    let no_clues = state_with_clues_at_location(0);
    assert!(effect_can_change_state(
        &no_clues,
        ctx(1),
        &dsl::gain_resources(InvestigatorTarget::You, 1)
    ));
}

/// Investigator 1 (and, when `others` is non-empty, further investigators)
/// on location 10, each seeded `(damage, horror, deck_len)`. #639 fixtures.
fn state_with_harm(seeds: &[(u32, u8, u8, usize)]) -> GameState {
    let mut builder =
        GameStateBuilder::new().with_location(test_support::test_location(10, "Study"));
    for &(id, damage, horror, deck_len) in seeds {
        let mut inv = test_support::test_investigator(id);
        inv.current_location = Some(LocationId(10));
        inv.investigator_card.accumulated_damage = damage;
        inv.investigator_card.accumulated_horror = horror;
        inv.deck = (0..deck_len)
            .map(|i| CardCode::new(format!("9000{i}")))
            .collect();
        builder = builder.with_investigator(inv);
    }
    builder.build()
}

#[test]
fn heal_can_change_state_only_when_the_target_carries_that_harm() {
    // #639 / RR "Ability": First Aid's heal on an unharmed investigator has
    // no potential to change the game state.
    let heal_damage = dsl::heal(HarmKind::Damage, InvestigatorTarget::You, 1);
    assert!(effect_can_change_state(
        &state_with_harm(&[(1, 2, 0, 0)]),
        ctx(1),
        &heal_damage,
    ));
    assert!(!effect_can_change_state(
        &state_with_harm(&[(1, 0, 0, 0)]),
        ctx(1),
        &heal_damage,
    ));
    // The kinds are independent: horror on the card doesn't make a damage
    // heal meaningful.
    assert!(!effect_can_change_state(
        &state_with_harm(&[(1, 0, 3, 0)]),
        ctx(1),
        &heal_damage,
    ));
}

#[test]
fn heal_of_zero_cannot_change_state() {
    assert!(!effect_can_change_state(
        &state_with_harm(&[(1, 2, 2, 0)]),
        ctx(1),
        &dsl::heal(HarmKind::Damage, InvestigatorTarget::You, 0),
    ));
}

#[test]
fn a_chosen_heal_target_scans_every_candidate_in_scope() {
    // Ungrounded `Chosen` at initiation time: the gate asks whether *any*
    // co-located investigator is an eligible target (RR "Target").
    let heal_damage = dsl::heal(
        HarmKind::Damage,
        InvestigatorTarget::chosen_at_your_location(),
        1,
    );
    assert!(
        effect_can_change_state(
            &state_with_harm(&[(1, 0, 0, 0), (2, 2, 0, 0)]),
            ctx(1),
            &heal_damage
        ),
        "a co-located damaged investigator keeps the heal live",
    );
    assert!(
        !effect_can_change_state(
            &state_with_harm(&[(1, 0, 0, 0), (2, 0, 0, 0)]),
            ctx(1),
            &heal_damage
        ),
        "nobody at the location has damage ⇒ no eligible target ⇒ inert",
    );
}

#[test]
fn search_deck_is_inert_only_against_an_empty_deck() {
    // #639 / Old Book of Lore 01031: an empty deck has nothing to find and
    // nothing to shuffle. A non-empty one is never proven inert (the
    // mandatory shuffle reorders it even on a fruitless search).
    let search = dsl::search_deck(
        InvestigatorTarget::chosen_at_your_location(),
        SearchScope::Top(3),
        None,
    );
    assert!(effect_can_change_state(
        &state_with_harm(&[(1, 0, 0, 4)]),
        ctx(1),
        &search,
    ));
    assert!(!effect_can_change_state(
        &state_with_harm(&[(1, 0, 0, 0)]),
        ctx(1),
        &search,
    ));
}
