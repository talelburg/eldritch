//! An effect resolves in full before the `at` and `after` cells of the
//! conditions it raised: `glossary/Effects.md`, *"All aspects of an effect have
//! timing priority over all "after..." triggering conditions that might arise
//! as a consequence of that effect."*
//!
//! The probe cards are test-local (`_eb_*`), served by [`PROBES`]: each fires
//! one cell of one condition, so the event log shows where that cell opened.

use std::sync::OnceLock;

use card_dsl::card_data::{CardKind, Class, SkillIcons, WeaknessKind};
use card_dsl::dsl::{EventPattern, EventTiming};

use super::*;
use crate::assert_event_sequence;
use crate::test_support::StagedEnemy;

/// `[reaction]` after its controller defeats an enemy: gain 1 resource.
const AFTER_DEFEAT_REACTION: &str = "_eb_after_defeat_reaction";
/// Forced, after its controller defeats an enemy: gain 1 resource.
const AFTER_DEFEAT_FORCED: &str = "_eb_after_defeat_forced";
/// Forced, after its controller discovers clues: gain 3 resources.
const AFTER_DISCOVERY_FORCED: &str = "_eb_after_discovery_forced";
/// An asset weakness whose Revelation deals 1 damage to an enemy at your
/// location. Drawn mid-effect, its Revelation is an effect of its own, nested
/// in the one that drew it.
const DEFEATING_WEAKNESS: &str = "_eb_defeating_weakness";

const ENEMY: EnemyId = EnemyId(100);

/// The registry the probes resolve against, with `TEST_INV` composed in for the
/// capacity reads that dealing damage makes.
const PROBES: CardRegistry = CardRegistry {
    metadata_for: probe_metadata_for,
    abilities_for: probe_abilities_for,
    ..CardRegistry::EMPTY
};

fn probe_metadata_for(code: &CardCode) -> Option<&'static CardMetadata> {
    static WEAKNESS: OnceLock<CardMetadata> = OnceLock::new();
    if code.as_str() != DEFEATING_WEAKNESS {
        return test_support::metadata_for_test_inv(code);
    }
    Some(WEAKNESS.get_or_init(|| CardMetadata {
        code: DEFEATING_WEAKNESS.to_owned(),
        name: "Defeating Weakness".to_owned(),
        traits: vec![],
        text: None,
        back_name: None,
        back_text: None,
        pack_code: "_test".to_owned(),
        weakness: Some(WeaknessKind::Other),
        kind: CardKind::Asset {
            class: Class::Neutral,
            cost: None,
            xp: None,
            slots: vec![],
            health: None,
            sanity: None,
            skill_icons: SkillIcons::default(),
            is_fast: false,
            deck_limit: 1,
            uses: None,
            play_only_during_turn: false,
        },
    }))
}

fn probe_abilities_for(code: &CardCode) -> Option<Vec<Ability>> {
    let you_defeat_an_enemy = EventPattern::EnemyDefeated {
        by_controller: true,
        code: None,
    };
    match code.as_str() {
        AFTER_DEFEAT_REACTION => Some(vec![dsl::reaction_on_event(
            you_defeat_an_enemy,
            EventTiming::After,
            dsl::gain_resources(InvestigatorTarget::You, 1),
        )]),
        AFTER_DEFEAT_FORCED => Some(vec![dsl::forced_on_event(
            you_defeat_an_enemy,
            EventTiming::After,
            dsl::gain_resources(InvestigatorTarget::You, 1),
        )]),
        AFTER_DISCOVERY_FORCED => Some(vec![dsl::forced_on_event(
            EventPattern::DiscoverClues,
            EventTiming::After,
            dsl::gain_resources(InvestigatorTarget::You, 3),
        )]),
        DEFEATING_WEAKNESS => Some(vec![dsl::revelation(defeat_the_enemy_here())]),
        _ => None,
    }
}

/// "Deal 1 damage to an enemy at your location", which defeats a 1-health one.
fn defeat_the_enemy_here() -> Effect {
    dsl::deal_damage_to_enemy(EnemyTarget::chosen_at_your_location(), 1)
}

/// A 1-health enemy standing at [`HERE`].
fn frail_enemy(id: EnemyId) -> StagedEnemy {
    enemy(id).health(1).at(HERE)
}

/// Investigator 1 at [`HERE`], which holds 2 clues, with `in_play` in play and
/// `deck` as their deck, beside one frail enemy.
fn board(in_play: &[&str], deck: &[&str]) -> GameState {
    let here = location(HERE).clues(2);
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(here)
        .with_seat(
            seat(1)
                .at(HERE)
                .in_play(in_play.iter().copied())
                .deck(deck.iter().copied()),
        )
        .with_enemy(frail_enemy(ENEMY))
        .build()
}

/// Run `effect` for investigator 1 against the probe registry.
fn run_with_probes(
    state: &mut GameState,
    events: &mut Vec<Event>,
    effect: &Effect,
) -> EngineOutcome {
    let mut cx = Cx::for_test(state, events);
    cx.cards = &PROBES;
    run(&mut cx, effect, ctx(1))
}

fn resources(state: &GameState) -> u8 {
    state.investigators[&InvestigatorId(1)].resources
}

/// The `Effects.md` example's shape: the effect defeats an enemy and then gains
/// resources, and the after-defeat reaction waits for the gain.
#[test]
fn an_effect_resolves_in_full_before_an_after_reaction_to_its_defeat_is_offered() {
    let mut state = board(&[AFTER_DEFEAT_REACTION], &[]);
    let before = resources(&state);
    let mut events = Vec::new();

    let outcome = run_with_probes(
        &mut state,
        &mut events,
        &dsl::seq([
            defeat_the_enemy_here(),
            dsl::gain_resources(InvestigatorTarget::You, 2),
        ]),
    );

    assert_eq!(offered_count(&outcome), 1, "the reaction is offered");
    assert_event!(events, Event::EnemyDefeated { enemy, .. } if *enemy == ENEMY);
    assert_eq!(
        resources(&state),
        before + 2,
        "both aspects resolved before the reaction is offered"
    );
}

/// A `Seq` suspended on a choice is still the same effect: the defeat its first
/// step made waits through the choice and the step after it.
#[test]
fn a_seq_suspended_on_a_choice_resumes_with_the_earlier_defeat_still_held() {
    let mut state = board(&[AFTER_DEFEAT_REACTION], &[]);
    let before = resources(&state);
    let mut events = Vec::new();

    let outcome = run_with_probes(
        &mut state,
        &mut events,
        &dsl::seq([
            defeat_the_enemy_here(),
            dsl::choose_one([
                (
                    "Gain 1 resource",
                    dsl::gain_resources(InvestigatorTarget::You, 1),
                ),
                (
                    "Gain 2 resources",
                    dsl::gain_resources(InvestigatorTarget::You, 2),
                ),
            ]),
            dsl::discover_clue(LocationTarget::YourLocation, 1),
        ]),
    );
    assert_eq!(offered_count(&outcome), 2, "the choice comes first");
    assert_suspended_leaf(&state);
    let EngineOutcome::AwaitingInput { request, .. } = &outcome else {
        unreachable!("offered_count saw AwaitingInput");
    };
    assert_eq!(
        request
            .options
            .iter()
            .map(|o| o.label.as_str())
            .collect::<Vec<_>>(),
        ["Gain 1 resource", "Gain 2 resources"],
        "the prompt is the choice alone: the reaction to the defeat is not offered"
    );
    assert_event!(events, Event::EnemyDefeated { enemy, .. } if *enemy == ENEMY);
    assert_eq!(resources(&state), before, "no branch has resolved yet");

    let mut cx = Cx::for_test(&mut state, &mut events);
    cx.cards = &PROBES;
    let outcome = resume_pick_with_cx(&mut cx, 1);

    assert_eq!(
        offered_count(&outcome),
        1,
        "then the reaction to the defeat"
    );
    assert_eq!(resources(&state), before + 2, "the picked branch resolved");
    assert_eq!(
        state.investigators[&InvestigatorId(1)].clues,
        1,
        "the step after the choice resolved"
    );
}

/// Two conditions one effect raises open their cells in the order they reached
/// them: the defeat's after cell, then the discovery's, both once the effect is
/// done.
#[test]
fn two_conditions_one_effect_raises_open_in_the_order_they_occurred() {
    let mut state = board(&[AFTER_DEFEAT_FORCED, AFTER_DISCOVERY_FORCED], &[]);
    let mut events = Vec::new();

    let outcome = run_with_probes(
        &mut state,
        &mut events,
        &dsl::seq([
            defeat_the_enemy_here(),
            dsl::discover_clue(LocationTarget::YourLocation, 1),
        ]),
    );

    assert_eq!(outcome, EngineOutcome::Done);
    assert_event_sequence!(
        events,
        Event::EnemyDefeated { enemy, .. } if *enemy == ENEMY,
        Event::CluePlaced { count: 1, .. },
        Event::ResourcesGained { amount: 1, .. },
        Event::ResourcesGained { amount: 3, .. },
    );
}

/// The `Then.md` shape across a nested effect: the first step draws a card
/// whose Revelation defeats an enemy, and the outer effect's next step lands
/// before the after cell of that defeat.
#[test]
fn a_nested_effects_after_condition_waits_for_the_outer_effect_to_finish() {
    let mut state = board(&[AFTER_DEFEAT_FORCED], &[DEFEATING_WEAKNESS]);
    let mut events = Vec::new();

    let outcome = run_with_probes(
        &mut state,
        &mut events,
        &dsl::seq([
            dsl::draw_cards(InvestigatorTarget::You, 1u8),
            dsl::gain_resources(InvestigatorTarget::You, 2),
        ]),
    );

    assert_eq!(outcome, EngineOutcome::Done);
    assert_event_sequence!(
        events,
        Event::CardRevealed { code, .. } if code.as_str() == DEFEATING_WEAKNESS,
        Event::EnemyDefeated { enemy, .. } if *enemy == ENEMY,
        Event::ResourcesGained { amount: 2, .. },
        Event::ResourcesGained { amount: 1, .. },
    );
}
