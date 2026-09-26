use card_dsl::card_data::CardMetadata;
use card_dsl::dsl::{self, Ability, ControlStatus, GrantTarget, Quantity};

use super::*;
use crate::state::{
    Continuation, GameStateBuilder, InFlightSkillTest, Lifetime, RecordedModifier, SkillTestId,
};
use crate::test_support;

mod determination;
mod difficulty;
mod fold;
mod non_investigator_targets;
mod recorded_rows;
mod scope;
mod sweep;

/// Mock registry over a small hardcoded set of codes. Keeps these
/// tests isolated from the global `OnceLock` and from the cards
/// crate — a query takes its registry by argument, so nothing is
/// installed. Named to match `tests/modified_value.rs`'s mocks,
/// which cover the same sweep from the integration side.
fn mock_metadata_for(_: &CardCode) -> Option<&'static CardMetadata> {
    None
}

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
        "elder-sign-clues-here" => Some(vec![dsl::elder_sign(IntExpr::Count(
            Quantity::CluesAtControllerLocation,
        ))]),
        // Lita Chantler 01117's exact shape (#773): the card prints no
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
        metadata_for: mock_metadata_for,
        abilities_for: mock_abilities_for,
        ..CardRegistry::EMPTY
    }
}

fn state_with_cards_in_play(codes: &[&str]) -> (GameState, InvestigatorId) {
    let id = InvestigatorId(1);
    let mut inv = test_support::test_investigator(1);
    inv.cards_in_play = codes
        .iter()
        .enumerate()
        .map(|(i, c)| {
            CardInPlay::enter_play(
                CardCode::new(*c),
                #[allow(clippy::cast_possible_truncation)]
                CardInstanceId(i as u32),
            )
        })
        .collect();
    let state = GameStateBuilder::new().with_investigator(inv).build();
    (state, id)
}

/// `modified_value` for an investigator's skill, in a plain test.
fn skill(state: &GameState, id: InvestigatorId, skill: SkillKind) -> i32 {
    modified_value(
        state,
        Some(&mock_registry()),
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
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .with_location(test_support::test_location(3, "Study"))
        .with_enemy(test_support::test_enemy(7, "Ghoul"))
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

/// Investigator 1's modified `skill`, read under the investigation
/// [`state_with_test`] puts in flight.
fn skill_of(state: &GameState, skill: SkillKind) -> i32 {
    modified_value(
        state,
        Some(&mock_registry()),
        ModifierTarget::Investigator(InvestigatorId(1)),
        ModifiedQuantity::Skill(skill),
        ReadContext::DuringTest(SkillTestKind::Investigate),
    )
    .total()
}

fn difficulty_of(state: &GameState) -> i32 {
    modified_value(
        state,
        Some(&mock_registry()),
        ModifierTarget::Test,
        ModifiedQuantity::Difficulty,
        ReadContext::DuringTest(SkillTestKind::Investigate),
    )
    .total()
}
