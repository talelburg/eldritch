//! #664: an [`Effect::ChooseOne`] offers only its **live** modes, and one whose
//! modes are *all* dead is **skipped**, not rejected.
//!
//! The skip half needs a `ChooseOne` reached where no initiation gate has
//! already proved a branch live — the shape is Medical Texts 01035's: a skill
//! test whose `on_success` heals. The activation gate passes (a `SkillTest` is
//! never provably inert), the test resolves, and only then does the heal find
//! nobody to heal. Rejecting there would unwind the whole action through
//! `apply_via`'s snapshot restore — chaos draw included — so the sub-effect is
//! skipped instead, the convention `ground_investigator_choice` established for
//! the same reason (#639).
//!
//! Lives at `crates/game-core/tests/` for its own process and its own
//! `OnceLock<CardRegistry>`, like the sibling mock-registry integration tests.
//! No corpus card carries a modal `on_success` yet, so a mock is the only way to
//! reach the shape.

use game_core::card_data::CardMetadata;
use game_core::card_registry::CardRegistry;
use game_core::dsl::{
    activated, choose_one, heal_damage, heal_horror, skill_test, Ability, InvestigatorTarget,
};
use game_core::engine::EngineOutcome;
use game_core::event::Event;
use game_core::state::{
    AbilitySource, CardCode, CardInPlay, CardInstanceId, ChaosBag, ChaosToken, InvestigatorId,
    LocationId, Phase, SkillKind, TokenModifiers,
};
use game_core::test_support::{test_investigator, test_location, GameStateBuilder, TestSession};
use game_core::TurnAction;
use game_core::{assert_event, assert_no_event};

/// Mock: `[action] Test intellect(2). If you succeed, heal 1 damage or horror
/// from you.` — Medical Texts 01035's shape with the heal made modal, so the
/// post-test `ChooseOne` can be reached with every mode dead.
const MODAL_HEAL_ON_SUCCESS: &str = "MOCK-664";

const INV: InvestigatorId = InvestigatorId(1);
const LOC: LocationId = LocationId(10);
const INST: CardInstanceId = CardInstanceId(0);

fn mock_metadata_for(_: &CardCode) -> Option<&'static CardMetadata> {
    None
}

fn mock_abilities_for(code: &CardCode) -> Option<Vec<Ability>> {
    match code.as_str() {
        MODAL_HEAL_ON_SUCCESS => Some(vec![activated(
            1,
            vec![],
            skill_test(
                SkillKind::Intellect,
                2,
                Some(choose_one([
                    heal_damage(InvestigatorTarget::You, 1),
                    heal_horror(InvestigatorTarget::You, 1),
                ])),
                None,
            ),
        )]),
        _ => None,
    }
}

#[ctor::ctor(unsafe)]
fn install_mock_registry() {
    let _ = game_core::card_registry::install(CardRegistry {
        metadata_for: mock_metadata_for,
        abilities_for: mock_abilities_for,
        ..CardRegistry::EMPTY
    });
}

/// Board: the mock asset in play, the investigator at `LOC` carrying `damage`
/// damage and `horror` horror, and an all-`Numeric(0)` chaos bag so the
/// intellect(2) test's outcome is decided by the investigator's stats alone.
fn board(damage: u8, horror: u8) -> game_core::GameState {
    let mut inv = test_investigator(1);
    inv.investigator_card.accumulated_damage = damage;
    inv.investigator_card.accumulated_horror = horror;
    inv.cards_in_play.push(CardInPlay::enter_play(
        CardCode::new(MODAL_HEAL_ON_SUCCESS),
        INST,
    ));

    GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator_at(inv, LOC)
        .with_location(test_location(10, "Study"))
        .with_active_investigator(INV)
        .with_turn_order([INV])
        .with_investigator_turn(INV)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .build()
}

fn activate(state: game_core::GameState) -> game_core::ApplyResult {
    TestSession::new(state)
        .take(&TurnAction::ActivateAbility {
            investigator: INV,
            source: AbilitySource::InPlay(INST),
            ability_index: 0,
        })
        .resolve_choices(|r| {
            r.commit_cards(&[]);
        })
        .run()
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
