//! Regression for #664: an [`Effect::ChooseOne`] offers only its **live**
//! modes, and one whose
//! modes are *all* dead is **skipped**, not rejected.
//!
//! The skip half needs a `ChooseOne` reached where no initiation gate has
//! already proved a branch live — the shape is Medical Texts 01035's: a skill
//! test whose `on_success` heals. The activation gate passes (a `SkillTest` is
//! never provably inert), the test resolves, and only then does the heal find
//! nobody to heal. Rejecting there would unwind the whole action through
//! `apply_via`'s snapshot restore — chaos draw included — so the sub-effect is
//! skipped instead, the convention `ground_investigator_choice` follows for
//! the same reason.
//!
//! Lives at `crates/game-core/tests/` for its own process and its own
//! `MockRegistry`, like the sibling mock-registry integration tests.
//! No corpus card carries a modal `on_success` yet, so a mock is the only way to
//! reach the shape.

use std::sync::LazyLock;

use card_dsl::dsl::{self, Ability, InvestigatorTarget};
use game_core::engine::enumerate::TurnAction;
use game_core::engine::{ApplyResult, EngineOutcome, OptionTarget, Registries};
use game_core::event::Event;
use game_core::scenario::ScenarioRegistry;
use game_core::state::{
    AbilityAddress, AbilitySource, ChaosBag, ChaosToken, GameState, GameStateBuilder,
    InvestigatorId, LocationId, SkillKind, TokenModifiers,
};
use game_core::test_support::{self, seat, MockRegistry, SeatLookup as _, TestSession};
use game_core::{assert_event, assert_no_event};

/// Mock: `[action] Test intellect(2). If you succeed, heal 1 damage or horror
/// from you.` — Medical Texts 01035's shape with the heal made modal, so the
/// post-test `ChooseOne` can be reached with every mode dead.
const MODAL_HEAL_ON_SUCCESS: &str = "MOCK-664";

const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(10);

fn modal_heal_abilities() -> Vec<Ability> {
    vec![dsl::activated(
        1,
        vec![],
        dsl::skill_test(
            SkillKind::Intellect,
            2,
            Some(dsl::choose_one([
                (
                    "Heal 1 damage",
                    dsl::heal_damage(InvestigatorTarget::You, 1),
                ),
                (
                    "Heal 1 horror",
                    dsl::heal_horror(InvestigatorTarget::You, 1),
                ),
            ])),
            None,
        ),
    )]
}

fn registries() -> Registries {
    static REGISTRIES: LazyLock<Registries> = LazyLock::new(|| Registries {
        cards: MockRegistry::new()
            .with_abilities(MODAL_HEAL_ON_SUCCESS, modal_heal_abilities)
            .build(),
        scenarios: &ScenarioRegistry::EMPTY,
    });
    *REGISTRIES
}

/// Board: the mock asset in play, the investigator at `HERE` carrying `damage`
/// damage and `horror` horror, and an all-`Numeric(0)` chaos bag so the
/// intellect(2) test's outcome is decided by the investigator's stats alone.
fn board(damage: u8, horror: u8) -> GameState {
    GameStateBuilder::new()
        .with_registries(registries())
        .with_seat(
            seat(1)
                .at(HERE)
                .harm(damage, horror)
                .in_play([MODAL_HEAL_ON_SUCCESS]),
        )
        .with_location(test_support::test_location(HERE.0, "Study"))
        .open_turn(INV)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .build()
}

/// The mock asset's activation.
fn activation(state: &GameState) -> TurnAction {
    TurnAction::ActivateAbility {
        investigator: INV,
        source: AbilitySource::InPlay(state.in_play(INV, MODAL_HEAL_ON_SUCCESS)),
        address: AbilityAddress::Printed(0),
    }
}

/// Activate the mock asset and commit nothing to its test.
fn activate(state: GameState) -> ApplyResult {
    let action = activation(&state);
    TestSession::new(state, registries())
        .take(&action)
        .commit(&[])
        .finish()
}

/// The control: with damage to heal the modal `on_success` has a live mode, so
/// the test's success runs it.
#[test]
fn a_live_mode_under_a_skill_test_still_resolves() {
    let r = activate(board(2, 0));
    assert_event!(
        r.events,
        Event::SkillTestSucceeded {
            skill: SkillKind::Intellect,
            ..
        }
    );
    assert_eq!(r.state.investigators[&INV].damage(), 1, "1 damage healed");
}

/// The regression: unharmed, both modes are dead. The ability still initiates
/// (a `SkillTest` is never provably inert), the test still resolves — and the
/// modal heal is *skipped*, leaving the resolved test standing rather than
/// rejecting and unwinding it.
#[test]
fn every_mode_dead_under_a_skill_test_skips_rather_than_rejecting() {
    let r = activate(board(0, 0));
    assert!(
        !matches!(r.outcome, EngineOutcome::Rejected { .. }),
        "a dead modal heal must not unwind the resolved test: {:?}",
        r.outcome,
    );
    assert_event!(
        r.events,
        Event::SkillTestSucceeded {
            skill: SkillKind::Intellect,
            ..
        }
    );
    assert_no_event!(r.events, Event::Healed { .. });
    assert_eq!(
        r.state.investigators[&INV].actions_remaining, 2,
        "the action was spent — the test resolved and stands",
    );
}

/// **The anchor survives the chaos-token suspension.**
///
/// The eval context that runs the `on_success` is not the one that started the
/// test — it is rebuilt at `apply_result_effect_step` from the source parked on
/// `InFlightSkillTest`. Park the `AbilitySource::instance()` projection there
/// instead of the whole descriptor and this still passes for an in-play card
/// while silently un-anchoring every board source (an act's `on_fail`), which
/// is the half-fix this test rules out. ADR 0011: *"the client never
/// decides where a control belongs — it reads the anchor the engine attached"*.
#[test]
fn a_modal_on_success_keeps_the_activating_cards_anchor_across_the_test() {
    // Both modes live, so the ChooseOne suspends instead of auto-resolving.
    let state = board(2, 2);
    let action = activation(&state);
    let card = state.in_play(INV, MODAL_HEAL_ON_SUCCESS);
    let session = TestSession::new(state, registries())
        .take(&action)
        .commit(&[]);
    // The session rests at the `on_success` ChooseOne.
    let request = session.prompt();
    assert_eq!(
        request
            .options
            .iter()
            .map(|o| o.label.as_str())
            .collect::<Vec<_>>(),
        ["Heal 1 damage", "Heal 1 horror"],
    );
    for option in &request.options {
        assert_eq!(
            option.target,
            Some(OptionTarget::CardInstance(card)),
            "still anchored to the activated card after the test resolved",
        );
    }
}
