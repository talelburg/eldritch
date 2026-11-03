//! Medical Texts 01035's `[action] Choose an
//! investigator at your location and test [intellect] (2). If you succeed,
//! heal 1 damage from that investigator. If you fail, deal 1 damage to that
//! investigator.` end-to-end against the real `cards::REGISTRY`.
//!
//! No rulings: 01035 is listed in `data/arkhamdb-faq/no-rulings.txt`.
//!
//! The investigator is chosen first, among those at the controller's
//! location, and kept: the test then suspends at the commit window (answered
//! with `commit(&[])`) and resumes through `advance`, and the outcome branch
//! heals or deals 1 damage to that same investigator without asking again.
//! Solo, the choice binds the controller without a prompt (one candidate at
//! the location, outside interactive play). With two investigators at the
//! location, the choice is a selection anchored on each investigator card,
//! made before the commit window.
//!
//! Runs against `cards::TEST_REGISTRIES`.

use card_dsl::dsl::HarmKind;
use cards::TEST_REGISTRIES;
use game_core::assert_event;
use game_core::engine::enumerate::TurnAction;
use game_core::engine::{EngineOutcome, OptionTarget, PromptNature};
use game_core::event::Event;
use game_core::state::{
    AbilityAddress, AbilitySource, ChaosBag, ChaosToken, GameStateBuilder, InvestigatorId,
    LocationId, SkillKind,
};
use game_core::test_support::{location, seat, BoardLookup as _, Seat, TestSession};

const MEDICAL_TEXTS: &str = "01035";
const INV: InvestigatorId = InvestigatorId(1);
/// A second investigator, seated after [`INV`].
const AGNES: InvestigatorId = InvestigatorId(2);
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

/// Medical Texts' effect is a skill test whose failure deals damage, so the
/// ability initiates and the investigator is chosen however healthy everyone
/// is. Succeeding with nobody damaged heals the chosen investigator by 0: the
/// heal resolves with nothing to remove, rather than rejecting, so the test
/// result stands rather than the whole action unwinding.
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

/// Two seats: Medical Texts' controller (`INV`, with `intellect` intellect)
/// and `agnes`, placed by the caller, on `INV`'s turn at `HERE`. Agnes holds
/// no card unless the caller gives her one, so she has nothing to commit to
/// the controller's test and isn't asked to (ST.2's helper commits).
fn board_with(intellect: i8, agnes: Seat) -> TestSession {
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(location(HERE))
        .with_seat(
            seat(1)
                .at(HERE)
                .skill(SkillKind::Intellect, intellect)
                .in_play([MEDICAL_TEXTS]),
        )
        .with_seat(agnes)
        .with_seating([INV, AGNES])
        .open_turn(INV)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .session()
}

/// Activate Medical Texts and rest at its first prompt.
fn activate_only(session: TestSession) -> TestSession {
    let book = session.state().in_play(INV, MEDICAL_TEXTS);
    session.take(&TurnAction::ActivateAbility {
        investigator: INV,
        source: AbilitySource::InPlay(book),
        address: AbilityAddress::Printed(0),
    })
}

fn card_anchor(session: &TestSession, investigator: InvestigatorId) -> OptionTarget {
    session.state().investigators[&investigator].card_anchor()
}

/// *"Choose an investigator at your location and test"*: the target is
/// chosen before the test starts, so before anyone commits a card. Picking
/// Agnes and failing deals the damage to her, and the failure asks nothing
/// more.
#[test]
fn the_target_is_chosen_before_the_test_and_a_failure_damages_it() {
    let session = activate_only(board_with(1, seat(2).at(HERE)));
    assert_eq!(session.prompt().prompt, "Choose an investigator");
    assert_eq!(session.prompt().nature, PromptNature::Selection);
    assert!(
        !session
            .events()
            .iter()
            .any(|e| matches!(e, Event::SkillTestStarted { .. })),
        "no test has started while the target is being chosen",
    );

    let agnes = card_anchor(&session, AGNES);
    let session = session.pick(agnes).commit(&[]);

    session.assert_at_turn_menu(INV);
    assert_event!(session.events(), Event::SkillTestFailed { .. });
    assert_event!(
        session.events(),
        Event::DamagePlacedOnInvestigator {
            investigator: AGNES,
            amount: 1,
        }
    );
    assert_eq!(session.state().investigators[&AGNES].damage(), 1);
    assert_eq!(
        session.state().investigators[&INV].damage(),
        0,
        "the controller, not chosen, takes nothing",
    );
}

/// Picking Agnes and succeeding heals 1 damage from her.
#[test]
fn a_success_heals_the_chosen_investigator() {
    let session = activate_only(board_with(3, seat(2).at(HERE).harm(2, 0)));
    let agnes = card_anchor(&session, AGNES);
    let session = session.pick(agnes).commit(&[]);

    session.assert_at_turn_menu(INV);
    assert_event!(session.events(), Event::SkillTestSucceeded { .. });
    assert_eq!(
        session.state().investigators[&AGNES].damage(),
        1,
        "1 of Agnes's 2 damage healed",
    );
}

/// With nobody damaged, every investigator at the location is still offered:
/// a failure deals damage, so the effect could change either one's state
/// (Rules Reference, *Target*).
#[test]
fn with_nobody_damaged_every_investigator_here_is_offered() {
    let session = activate_only(board_with(3, seat(2).at(HERE)));
    assert_eq!(
        session.offered_targets(),
        [
            Some(card_anchor(&session, INV)),
            Some(card_anchor(&session, AGNES)),
        ],
    );
}
