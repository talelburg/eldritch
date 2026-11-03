//! Integration: other investigators at the performer's location commit to
//! the performer's skill test.
//!
//! Appendix II, ST.2: *"Each other investigator at the same location as the
//! investigator performing the skill test may commit one card with an
//! appropriate skill icon to this test."* Each such investigator holding a
//! candidate (a **helper**) is asked in player order, and the performer last.
//!
//! The helper's card is Unexpected Courage 01093: two wild icons and no
//! ability, so its icons are all it contributes. Roland (seat 1, intellect 3)
//! tests Intellect against 5 on a single `Numeric(0)` token, so the test
//! succeeds exactly when two icons are committed.
//!
//! Runs against `cards::TEST_REGISTRIES`.

use cards::TEST_REGISTRIES;
use game_core::action::InputResponse;
use game_core::assert_event;
use game_core::engine::{OptionId, OptionTarget, Responder};
use game_core::event::Event;
use game_core::state::{
    CardCode, ChaosBag, ChaosToken, GameStateBuilder, InvestigatorId, LocationId, SkillKind, Status,
};
use game_core::test_support::{location, seat, Seat, TestSession};

const UNEXPECTED_COURAGE: &str = "01093";
/// Overpower 01091: two combat icons, no wild, so no appropriate icon for an
/// Intellect test.
const OVERPOWER: &str = "01091";
/// Working a Hunch 01037: an event with two intellect icons, so a second
/// committer's card with icons and no commit ability. Unexpected Courage is
/// "Max 1 committed per skill test" across every player, so no test commits
/// two copies of it.
const WORKING_A_HUNCH: &str = "01037";

/// The investigator performing the test.
const ROLAND: InvestigatorId = InvestigatorId(1);
/// A helper at Roland's location.
const DAISY: InvestigatorId = InvestigatorId(2);
const HERE: LocationId = LocationId(20);
/// A location away from Roland.
const THERE: LocationId = LocationId(21);

/// Roland (intellect 3) at `HERE` on his open turn, with `daisy` seated after
/// him, on a single-`Numeric(0)` bag. `daisy` is placed by the caller.
fn board(daisy: Seat) -> GameStateBuilder {
    board_with_roland_holding(&[], daisy)
}

/// [`board`], with Roland holding `hand`.
fn board_with_roland_holding(hand: &[&str], daisy: Seat) -> GameStateBuilder {
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(
            seat(1)
                .at(HERE)
                .skill(SkillKind::Intellect, 3)
                .hand(hand.iter().copied()),
        )
        .with_seat(daisy)
        .with_location(location(HERE))
        .with_seating([ROLAND, DAISY])
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .open_turn(ROLAND)
}

/// [`board`] with Daisy at `HERE` holding `hand`, started into Roland's
/// Intellect-vs-5 test.
fn rolands_test_with_daisy_holding(hand: &[&str]) -> TestSession {
    board(seat(2).at(HERE).hand(hand.iter().copied()))
        .session()
        .test_skill(ROLAND, SkillKind::Intellect, 5)
}

/// The issue's original criterion: Daisy's wild icons count toward Roland's
/// skill value. Intellect 3 + 2 wild = 5 meets difficulty 5.
#[test]
fn a_helpers_icons_count_toward_the_performers_skill_value() {
    let session = rolands_test_with_daisy_holding(&[UNEXPECTED_COURAGE])
        .commit(&[UNEXPECTED_COURAGE])
        .commit(&[]);
    session.assert_at_turn_menu(ROLAND);
    assert_event!(
        session.events(),
        Event::SkillTestSucceeded { investigator, skill: SkillKind::Intellect, margin: 0 }
            if *investigator == ROLAND
    );
}

/// Daisy's hand card at `hand_index`, as an option anchor.
fn daisys_card(hand_index: u8) -> OptionTarget {
    OptionTarget::HandCard {
        investigator: DAISY,
        hand_index,
    }
}

/// The helper's prompt is Daisy's, not Roland's: Roland can't answer it.
#[test]
fn a_helpers_prompt_is_addressed_to_the_helper() {
    let session = rolands_test_with_daisy_holding(&[UNEXPECTED_COURAGE]);
    assert_eq!(session.prompt().responder, Responder::Investigator(DAISY));
    session.assert_rejects(|s| {
        s.answer_as(
            ROLAND,
            InputResponse::PickMultiple {
                selected: Vec::new(),
            },
        )
    });
}

/// ST.2: a helper may commit a card *"with an appropriate skill icon"*.
/// Overpower's combat icons don't fit an Intellect test.
#[test]
fn a_helper_is_offered_only_cards_with_an_appropriate_icon() {
    let session = rolands_test_with_daisy_holding(&[OVERPOWER, UNEXPECTED_COURAGE]);
    assert_eq!(session.offered_at(&daisys_card(0)), 0, "Overpower");
    assert_eq!(session.offered_at(&daisys_card(1)), 1, "Unexpected Courage");
}

/// ST.2: a helper *"may commit one card"*. A pick of two is rejected and the
/// prompt stays up. The two cards have different titles, so no commit cap is
/// what turns the pick away.
#[test]
fn a_helpers_pick_of_two_cards_is_rejected() {
    let session = rolands_test_with_daisy_holding(&[UNEXPECTED_COURAGE, WORKING_A_HUNCH]);
    let both: Vec<OptionId> = session.prompt().options.iter().map(|o| o.id).collect();
    assert_eq!(both.len(), 2, "both cards are offered");
    session.assert_rejects(|s| s.answer_as(DAISY, InputResponse::PickMultiple { selected: both }));
}

/// A helper may commit nothing; Roland is asked next, and without the help
/// intellect 3 fails against 5.
#[test]
fn a_helper_may_commit_nothing() {
    let session = rolands_test_with_daisy_holding(&[UNEXPECTED_COURAGE]).commit(&[]);
    assert_eq!(session.prompt().responder, Responder::Investigator(ROLAND));
    let session = session.commit(&[]);
    assert_event!(
        session.events(),
        Event::SkillTestFailed { investigator, by: 2, .. } if *investigator == ROLAND
    );
    assert_eq!(
        session.state().investigators[&DAISY].hand.len(),
        1,
        "Daisy keeps her card"
    );
}

/// Roland's test started on `board(daisy)`: the first prompt is Roland's own
/// commit window, so Daisy is not asked.
fn assert_only_roland_is_asked(daisy: Seat) {
    let session = board(daisy)
        .with_location(location(THERE))
        .session()
        .test_skill(ROLAND, SkillKind::Intellect, 5);
    assert_eq!(session.prompt().responder, Responder::Investigator(ROLAND));
    assert_eq!(session.offered_at(&daisys_card(0)), 0);
}

/// A co-located investigator with no card they could commit is not asked.
#[test]
fn a_helper_with_no_candidate_is_not_asked() {
    assert_only_roland_is_asked(seat(2).at(HERE).hand([OVERPOWER]));
}

/// ST.2 asks only investigators *"at the same location"*.
#[test]
fn an_investigator_at_another_location_is_not_asked() {
    assert_only_roland_is_asked(seat(2).at(THERE).hand([UNEXPECTED_COURAGE]));
}

/// An eliminated investigator has left the scenario, and is not asked.
#[test]
fn an_eliminated_investigator_is_not_asked() {
    assert_only_roland_is_asked(
        seat(2)
            .at(HERE)
            .hand([UNEXPECTED_COURAGE])
            .status(Status::Defeated),
    );
}

/// A third investigator, at Roland's location.
const AGNES: InvestigatorId = InvestigatorId(3);

/// Roland, Daisy and Agnes at `HERE`, seated in that order with Agnes the
/// lead, started into Roland's Intellect-vs-5 test. Agnes holds Unexpected
/// Courage and Daisy Working a Hunch. Player order from the lead is Agnes,
/// Roland, Daisy, so the helpers are Agnes then Daisy.
fn rolands_test_with_two_helpers() -> TestSession {
    board(seat(2).at(HERE).hand([WORKING_A_HUNCH]))
        .with_seat(seat(3).at(HERE).hand([UNEXPECTED_COURAGE]))
        .with_seating([ROLAND, DAISY, AGNES])
        .with_lead(AGNES)
        .session()
        .test_skill(ROLAND, SkillKind::Intellect, 5)
}

/// Helpers are asked in player order (`glossary/In_Player_Order.md`: *"the
/// lead investigator performs his or her part of the sequence first,
/// followed by the other players in clockwise order"*), then the performer.
#[test]
fn two_helpers_are_asked_in_player_order_then_the_performer() {
    let session = rolands_test_with_two_helpers();
    assert_eq!(session.prompt().responder, Responder::Investigator(AGNES));
    let session = session.commit(&[UNEXPECTED_COURAGE]);
    assert_eq!(session.prompt().responder, Responder::Investigator(DAISY));
    let session = session.commit(&[WORKING_A_HUNCH]);
    assert_eq!(session.prompt().responder, Responder::Investigator(ROLAND));
    let session = session.commit(&[]);
    // Intellect 3 + 2 wild + 2 intellect = 7 against 5.
    assert_event!(
        session.events(),
        Event::SkillTestSucceeded { investigator, margin: 2, .. } if *investigator == ROLAND
    );
}

/// `investigator`'s name, as the prompts print it.
fn name(session: &TestSession, investigator: InvestigatorId) -> String {
    session.state().investigators[&investigator].name.clone()
}

/// The helper's prompt names the performer's test and its difficulty, and
/// every later prompt names the commits made so far, card and committer.
#[test]
fn each_prompt_names_the_commits_so_far() {
    let session = rolands_test_with_two_helpers();
    let agnes_prompt = session.prompt().prompt.clone();
    assert!(
        agnes_prompt.contains(&format!(
            "{}'s Intellect test (difficulty 5)",
            name(&session, ROLAND)
        )),
        "{agnes_prompt}"
    );
    assert!(!agnes_prompt.contains("Committed so far"), "{agnes_prompt}");

    let agnes_commit = format!("Unexpected Courage ({})", name(&session, AGNES));
    let session = session.commit(&[UNEXPECTED_COURAGE]);
    let daisy_prompt = session.prompt().prompt.clone();
    assert!(
        daisy_prompt.contains(&format!("Committed so far: {agnes_commit}.")),
        "{daisy_prompt}"
    );

    let daisy_commit = format!("Working a Hunch ({})", name(&session, DAISY));
    let session = session.commit(&[WORKING_A_HUNCH]);
    let roland_prompt = session.prompt().prompt.clone();
    assert!(
        roland_prompt.contains(&format!(
            "Committed so far: {agnes_commit}, {daisy_commit}."
        )),
        "{roland_prompt}"
    );
}

/// ST.8: *"Discard all cards that were committed to this skill test"*, each
/// to its owner's discard pile.
#[test]
fn each_committed_card_goes_to_its_owners_discard_pile() {
    let session = board_with_roland_holding(
        &[WORKING_A_HUNCH],
        seat(2).at(HERE).hand([UNEXPECTED_COURAGE]),
    )
    .session()
    .test_skill(ROLAND, SkillKind::Intellect, 5)
    .commit(&[UNEXPECTED_COURAGE])
    .commit(&[WORKING_A_HUNCH]);
    session.assert_at_turn_menu(ROLAND);
    let state = session.state();
    for (investigator, card) in [(ROLAND, WORKING_A_HUNCH), (DAISY, UNEXPECTED_COURAGE)] {
        assert_eq!(
            state.investigators[&investigator].discard,
            vec![CardCode::new(card)],
            "{investigator:?}'s discard pile",
        );
        assert!(state.investigators[&investigator].hand.is_empty());
    }
}

/// Opportunist 01053: *"Commit only to a skill test you are performing."* One
/// wild icon, so only the restriction keeps it off Roland's Intellect test.
const OPPORTUNIST: &str = "01053";

/// Opportunist isn't offered to a helper: the test is Roland's, not Daisy's.
#[test]
fn a_helper_is_not_offered_opportunist() {
    let session = rolands_test_with_daisy_holding(&[OPPORTUNIST, UNEXPECTED_COURAGE]);
    assert_eq!(session.prompt().responder, Responder::Investigator(DAISY));
    assert_eq!(session.offered_at(&daisys_card(0)), 0, "Opportunist");
    assert_eq!(session.offered_at(&daisys_card(1)), 1, "Unexpected Courage");
}

/// Roland's hand card at `hand_index`, as an option anchor.
fn rolands_card(hand_index: u8) -> OptionTarget {
    OptionTarget::HandCard {
        investigator: ROLAND,
        hand_index,
    }
}

/// The performer is still offered their own Opportunist, with a helper beside
/// them holding one too.
#[test]
fn the_performer_is_offered_their_own_opportunist() {
    let session = board_with_roland_holding(&[OPPORTUNIST], seat(2).at(HERE).hand([OPPORTUNIST]))
        .session()
        .test_skill(ROLAND, SkillKind::Intellect, 5);
    assert_eq!(session.prompt().responder, Responder::Investigator(ROLAND));
    assert_eq!(session.offered_at(&rolands_card(0)), 1);
}

/// Guts 01089: *"Max 1 committed per skill test."* Two willpower icons. Its
/// ruling (<https://arkhamdb.com/card/01089>): *"\"Max 1 per skill test\"
/// across all players."*
const GUTS: &str = "01089";

/// Once Daisy commits Guts to Roland's Willpower test the cap is reached
/// across all players, so Roland's prompt doesn't offer his own copy.
#[test]
fn after_a_helper_commits_guts_the_performer_is_not_offered_guts() {
    let session = board_with_roland_holding(&[GUTS], seat(2).at(HERE).hand([GUTS]))
        .session()
        .test_skill(ROLAND, SkillKind::Willpower, 1)
        .commit(&[GUTS]);
    assert_eq!(session.prompt().responder, Responder::Investigator(ROLAND));
    assert_eq!(session.offered_at(&rolands_card(0)), 0, "Roland's Guts");
}
