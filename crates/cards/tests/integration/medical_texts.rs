//! Medical Texts 01035's `[action] Choose an
//! investigator at your location and test [intellect] (2). If you succeed,
//! heal 1 damage from that investigator. If you fail, deal 1 damage to that
//! investigator.` end-to-end against the real `cards::REGISTRY`.
//!
//! Exercises the first `Effect::SkillTest` initiated from an *activated*
//! ability: the test suspends at the commit window (answered with
//! `commit(&[])`) and resumes through `advance`, then the
//! outcome branch heals or deals 1 damage to the chosen co-located
//! investigator. In solo the choice auto-binds to the controller (one
//! candidate at the location), so no choice suspend occurs.
//!
//! Runs against `cards::TEST_REGISTRIES`.

use card_dsl::dsl::HarmKind;
use cards::TEST_REGISTRIES;
use game_core::assert_event;
use game_core::engine::enumerate::TurnAction;
use game_core::engine::EngineOutcome;
use game_core::event::Event;
use game_core::state::{
    AbilityAddress, AbilitySource, ChaosBag, ChaosToken, GameStateBuilder, InvestigatorId,
    LocationId, SkillKind,
};
use game_core::test_support::{location, seat, BoardLookup as _, TestSession};

const MEDICAL_TEXTS: &str = "01035";
const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(10);

/// Board: Medical Texts in play; the active investigator with `intellect`
/// intellect and `damage` damage, alone at `HERE`. A `Numeric(0)` chaos bag
/// makes the intellect(2) test deterministic — intellect 3 succeeds, 1 fails,
/// both off the difficulty boundary.
fn board(intellect: i8, damage: u8) -> TestSession {
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(location(HERE))
        .with_seat(
            seat(1)
                .at(HERE)
                .skill(SkillKind::Intellect, intellect)
                // Harm accumulates on the investigator card.
                .harm(damage, 0)
                .in_play([MEDICAL_TEXTS]),
        )
        .open_turn(INV)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .session()
}

/// Activate Medical Texts and commit nothing to its test.
fn activate(session: TestSession) -> TestSession {
    let book = session.state().in_play(INV, MEDICAL_TEXTS);
    session
        .take(&TurnAction::ActivateAbility {
            investigator: INV,
            source: AbilitySource::InPlay(book),
            address: AbilityAddress::Printed(0),
        })
        .commit(&[])
}

#[test]
fn success_heals_one_damage_from_the_chosen_investigator() {
    // intellect 3 + 0 = 3 >= difficulty 2 → success → heal 1 damage.
    let session = activate(board(3, 2));
    session.assert_at_turn_menu(INV);
    assert_event!(session.events(), Event::SkillTestSucceeded { .. });
    assert_event!(
        session.events(),
        Event::Healed {
            kind: HarmKind::Damage,
            amount: 1,
            ..
        }
    );
    assert_eq!(
        session.state().investigators[&INV].damage(),
        1,
        "1 damage healed"
    );
}

/// The heal's target eligibility bites at *resolution*, not initiation —
/// Medical Texts' effect is a skill test, which always has the potential to
/// change the game state, so the ability initiates however healthy everyone is.
/// Succeeding with nobody damaged leaves the `on_success` heal with no eligible
/// target; it is **skipped**, not rejected, so the test result stands rather
/// than the whole action unwinding.
#[test]
fn success_with_nothing_to_heal_still_resolves_the_test() {
    let session = activate(board(3, 0));
    session.assert_at_turn_menu(INV);
    assert_event!(session.events(), Event::SkillTestSucceeded { .. });
    assert_eq!(
        session.state().investigators[&INV].damage(),
        0,
        "nothing to heal, nothing healed — and no damage dealt either",
    );
    let r = session.finish();
    assert!(
        !matches!(r.outcome, EngineOutcome::Rejected { .. }),
        "a heal with no eligible target must not unwind the action: {:?}",
        r.outcome,
    );
}

#[test]
fn failure_deals_one_damage_to_the_chosen_investigator() {
    // intellect 1 + 0 = 1 < difficulty 2 → failure → deal 1 damage.
    let session = activate(board(1, 0));
    session.assert_at_turn_menu(INV);
    assert_event!(session.events(), Event::SkillTestFailed { .. });
    assert_event!(
        session.events(),
        Event::DamagePlacedOnInvestigator {
            investigator: INV,
            amount: 1,
        }
    );
    assert_eq!(
        session.state().investigators[&INV].damage(),
        1,
        "1 damage dealt"
    );
}
