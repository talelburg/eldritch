use super::*;
use crate::test_support::TEST_REGISTRIES;

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
/// on [`HERE`], each seeded `(id, damage, horror, deck_len)`. #639 fixtures.
fn state_with_harm(seeds: &[(u32, u8, u8, usize)]) -> GameState {
    seeds
        .iter()
        .map(|&(id, damage, horror, deck_len)| {
            seat(id)
                .at(HERE)
                .harm(damage, horror)
                .deck((0..deck_len).map(|i| format!("9000{i}")))
        })
        .fold(
            GameStateBuilder::new()
                .with_registries(TEST_REGISTRIES)
                .with_location(test_support::test_location(HERE.0, "Study")),
            GameStateBuilder::with_seat,
        )
        .build()
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

/// Investigator 1 on [`HERE`] with an enemy there, engaged with them or not.
fn state_with_enemy(engaged: bool) -> GameState {
    let mut enemy = test_support::test_enemy(100, "Ghoul");
    enemy.current_location = Some(HERE);
    let builder = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(test_support::test_location(HERE.0, "Study"))
        .with_seat(seat(1).at(HERE));
    if engaged {
        builder.with_enemy_engaged(enemy, InvestigatorId(1))
    } else {
        builder.with_enemy(enemy)
    }
    .build()
}

#[test]
fn a_fan_out_over_an_empty_set_cannot_change_state() {
    // Elusive 01050's disengage half: with no enemy engaged with you there is
    // nothing to walk, so the fan-out is inert; with one, its body is live.
    let disengage_each = dsl::for_each(EnemySet::EngagedWithYou, dsl::disengage(EnemyTarget::Each));
    assert!(effect_can_change_state(
        &state_with_enemy(true),
        ctx(1),
        &disengage_each
    ));
    assert!(!effect_can_change_state(
        &state_with_enemy(false),
        ctx(1),
        &disengage_each
    ));
}

#[test]
fn disengaging_the_bound_enemy_changes_state_only_while_it_is_engaged() {
    let disengage = dsl::disengage(EnemyTarget::Each);
    let mut bound = ctx(1);
    bound.set_each(EachBinding::Enemy(EnemyId(100)));
    assert!(effect_can_change_state(
        &state_with_enemy(true),
        bound,
        &disengage
    ));
    assert!(!effect_can_change_state(
        &state_with_enemy(false),
        bound,
        &disengage
    ));
}
