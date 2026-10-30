use card_dsl::dsl::{self, Ability, BlankTarget, ControlStatus, GrantTarget, IntExpr, Quantity};

use super::*;
use crate::state::{
    Continuation, EnemyId, GameStateBuilder, InFlightSkillTest, Lifetime, LocationId, Owner,
    RecordedModifier, SkillTestId,
};
use crate::test_support::{
    self, enemy, location, seat, SeatLookup as _, StagedLocation, TEST_REGISTRIES,
};

mod blank;
mod determination;
mod difficulty;
mod fold;
mod non_investigator_targets;
mod recorded_rows;
mod scope;
mod sweep;

/// Mock registry over a small hardcoded set of codes, keeping these tests
/// isolated from the cards crate — a query takes its registry by argument.
/// Named to match `tests/modified_value.rs`'s mocks, which cover the same
/// sweep from the integration side. Its metadata is the unit-test registry's,
/// so a `TEST_INV` seat's printed capacity reads.
fn mock_abilities_for(code: &CardCode) -> Option<Vec<Ability>> {
    match code.as_str() {
        "willpower-plus-1" => Some(vec![dsl::constant(dsl::modify(
            Stat::Willpower,
            1,
            ModifierScope::WhileInPlay,
        ))]),
        "willpower-minus-1" => Some(vec![dsl::constant(dsl::modify(
            Stat::Willpower,
            -1,
            ModifierScope::WhileInPlay,
        ))]),
        "intellect-plus-2" => Some(vec![dsl::constant(dsl::modify(
            Stat::Intellect,
            2,
            ModifierScope::WhileInPlay,
        ))]),
        "inv-willpower-plus-2" => Some(vec![dsl::constant(dsl::modify(
            Stat::Willpower,
            2,
            ModifierScope::WhileInPlay,
        ))]),
        "intellect-plus-1-while-investigating" => Some(vec![dsl::constant(dsl::modify(
            Stat::Intellect,
            1,
            ModifierScope::WhileInPlayDuring(SkillTestKind::Investigate),
        ))]),
        "willpower-plus-1-this-test-only" => Some(vec![dsl::constant(dsl::modify(
            Stat::Willpower,
            1,
            ModifierScope::ThisSkillTest,
        ))]),
        "non-constant-willpower" => Some(vec![dsl::on_play(dsl::modify(
            Stat::Willpower,
            5,
            ModifierScope::WhileInPlay,
        ))]),
        "max-health-plus-1" => Some(vec![dsl::constant(dsl::modify(
            Stat::MaxHealth,
            1,
            ModifierScope::WhileInPlay,
        ))]),
        // Obscuring Fog 01168's shape: "Attached location gets +2
        // shroud."
        "shroud-plus-2" => Some(vec![dsl::constant(dsl::modify_for(
            ModifierAudience::AttachedCard,
            Stat::Shroud,
            2,
            ModifierScope::WhileInPlay,
        ))]),
        // Lita Chantler 01117's buff, printed: "Each investigator at your
        // location gets +1 [combat]."
        "combat-plus-1-here" => Some(vec![dsl::constant(dsl::modify_for(
            ModifierAudience::EachInvestigatorAtSourceLocation,
            Stat::Combat,
            1,
            ModifierScope::WhileInPlay,
        ))]),
        // Grants a card it does not sit on — `grant-recipient` — a modifier
        // for that card's controller.
        "grants-combat-to-recipient" => Some(vec![dsl::constant(dsl::grant(
            GrantTarget::Card("grant-recipient".to_owned()),
            None,
            vec![dsl::constant(dsl::modify(
                Stat::Combat,
                1,
                ModifierScope::WhileInPlay,
            ))],
        ))]),
        "elder-sign-clues-here" => Some(vec![dsl::elder_sign(IntExpr::Count(
            Quantity::CluesAtControllerLocation,
        ))]),
        // Stubborn Detective 01103's shape: "While Stubborn Detective is at
        // your location, treat your investigator as if his or her printed
        // text box were blank (except for [[Traits]])."
        "blanks-investigators-here" => Some(vec![dsl::constant(Effect::BlankText {
            target: BlankTarget::InvestigatorsAtThisLocation,
        })]),
        // Grants `elder-sign-clues-here` a second elder sign, `+5`, so a
        // granted elder sign can be told from the printed one.
        "grants-elder-sign" => Some(vec![dsl::constant(dsl::grant(
            GrantTarget::Card("elder-sign-clues-here".to_owned()),
            None,
            vec![dsl::elder_sign(IntExpr::Lit(5))],
        ))]),
        // Lita Chantler 01117's exact shape: the card prints no
        // modifier of its own and **grants itself** one, gated on being
        // controlled by a player. The audience is location-scoped so the
        // same card can be asked from both placements — controlled, and
        // sitting at a location under nobody's control.
        "self-granting-combat" => Some(vec![dsl::constant(dsl::grant(
            GrantTarget::SelfCard,
            Some(dsl::control_status(
                "self-granting-combat",
                ControlStatus::ByAPlayer,
            )),
            vec![dsl::constant(dsl::modify_for(
                ModifierAudience::EachInvestigatorAtSourceLocation,
                Stat::Combat,
                1,
                ModifierScope::WhileInPlay,
            ))],
        ))]),
        _ => None,
    }
}

fn mock_registry() -> CardRegistry {
    CardRegistry {
        abilities_for: mock_abilities_for,
        ..test_support::TEST_REGISTRY
    }
}

/// Investigator 1 with `codes` in play, in order.
fn state_with_cards_in_play(codes: &[&str]) -> (GameState, InvestigatorId) {
    let id = InvestigatorId(1);
    let state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(id.0).in_play(codes.iter().copied()))
        .build();
    (state, id)
}

/// `modified_value` for an investigator's skill, in a plain test.
fn skill(state: &GameState, id: InvestigatorId, skill: SkillKind) -> i32 {
    modified_value(
        state,
        &mock_registry(),
        ModifierTarget::Investigator(id),
        ModifiedQuantity::Skill(skill),
        ReadContext::DuringTest(SkillTestKind::Plain),
    )
    .total()
}

/// The id of the test the recorded-row tests put in flight.
const IN_FLIGHT: SkillTestId = SkillTestId(7);

/// A row scoped to [`IN_FLIGHT`].
fn recorded(investigator: InvestigatorId, stat: Stat, delta: i8) -> RecordedModifier {
    recorded_for(investigator, stat, delta, IN_FLIGHT)
}

/// A row scoped to the named test.
fn recorded_for(
    investigator: InvestigatorId,
    stat: Stat,
    delta: i8,
    test: SkillTestId,
) -> RecordedModifier {
    RecordedModifier::new(
        investigator,
        stat,
        IntExpr::Lit(delta),
        Lifetime::SkillTest(test),
        None,
    )
}

/// A board carrying `test` in flight, one investigator, one location
/// (printed shroud 2) and one enemy (printed fight 2, evade 2).
fn state_with_test(basis: DifficultyBasis) -> GameState {
    state_with_test_at(basis, location(3))
}

/// [`state_with_test`], with its one location staged as `at`.
fn state_with_test_at(basis: DifficultyBasis, at: StagedLocation) -> GameState {
    let mut state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(1))
        .with_location(at)
        .with_enemy(enemy(7))
        .build();
    state
        .continuations
        .push(Continuation::SkillTest(InFlightSkillTest {
            difficulty_basis: basis,
            ..test_support::test_skill_test(
                IN_FLIGHT,
                InvestigatorId(1),
                SkillKind::Intellect,
                SkillTestKind::Investigate,
                0,
            )
        }));
    state
}

fn difficulty_of(state: &GameState) -> i32 {
    modified_value(
        state,
        &mock_registry(),
        ModifierTarget::Test,
        ModifiedQuantity::Difficulty,
        ReadContext::DuringTest(SkillTestKind::Investigate),
    )
    .total()
}
