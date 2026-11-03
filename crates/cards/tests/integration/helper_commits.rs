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
//! The last section commits cards with abilities, to pin whose result each
//! one is: a result addition is its committer's, a result modifier the
//! performer's.
//!
//! Runs against `cards::TEST_REGISTRIES`.

use cards::TEST_REGISTRIES;
use game_core::action::InputResponse;
use game_core::assert_event;
use game_core::engine::enumerate::TurnAction;
use game_core::engine::{OptionId, OptionTarget, PromptNature, Responder};
use game_core::event::Event;
use game_core::state::{
    CardCode, ChaosBag, ChaosToken, CommittedIndex, EnemyId, GameStateBuilder, InvestigatorId,
    LocationId, SkillKind, Status,
};
use game_core::test_support::{enemy, location, seat, BoardLookup as _, Seat, TestSession};

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
    board_with_roland(
        seat(1)
            .at(HERE)
            .skill(SkillKind::Intellect, 3)
            .hand(hand.iter().copied()),
        daisy,
    )
}

/// [`board`], with Roland's seat `roland` (placed by the caller).
fn board_with_roland(roland: Seat, daisy: Seat) -> GameStateBuilder {
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(roland)
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

/// Daisy's committed wild icons count toward Roland's skill value.
/// Intellect 3 + 2 wild = 5 meets difficulty 5.
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

/// The card at `hand_index` in `holder`'s hand, as an option anchor.
fn hand_card(holder: InvestigatorId, hand_index: u8) -> OptionTarget {
    OptionTarget::HandCard {
        investigator: holder,
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
    assert_eq!(session.offered_at(&hand_card(DAISY, 0)), 0, "Overpower");
    assert_eq!(
        session.offered_at(&hand_card(DAISY, 1)),
        1,
        "Unexpected Courage"
    );
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
    assert_eq!(session.offered_at(&hand_card(DAISY, 0)), 0);
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
/// lead, Daisy holding `daisy` and Agnes `agnes`. Player order from the lead
/// is Agnes, Roland, Daisy, so the helpers are Agnes then Daisy.
fn board_with_two_helpers(daisy: &[&str], agnes: &[&str]) -> GameStateBuilder {
    board(seat(2).at(HERE).hand(daisy.iter().copied()))
        .with_seat(seat(3).at(HERE).hand(agnes.iter().copied()))
        .with_seating([ROLAND, DAISY, AGNES])
        .with_lead(AGNES)
}

/// [`board_with_two_helpers`] with Agnes holding Unexpected Courage and Daisy
/// Working a Hunch, started into Roland's Intellect-vs-5 test.
fn rolands_test_with_two_helpers() -> TestSession {
    board_with_two_helpers(&[WORKING_A_HUNCH], &[UNEXPECTED_COURAGE])
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

/// The helper's prompt names the performer's test and its difficulty, and
/// every later prompt names the commits made so far, card and committer.
#[test]
fn each_prompt_names_the_commits_so_far() {
    let session = rolands_test_with_two_helpers();
    let agnes_prompt = session.prompt().prompt.clone();
    assert!(
        agnes_prompt.contains(&format!(
            "{}'s Intellect test (difficulty 5)",
            session.state().name_of(ROLAND)
        )),
        "{agnes_prompt}"
    );
    assert!(!agnes_prompt.contains("Committed so far"), "{agnes_prompt}");

    let agnes_commit = format!("Unexpected Courage ({})", session.state().name_of(AGNES));
    let session = session.commit(&[UNEXPECTED_COURAGE]);
    let daisy_prompt = session.prompt().prompt.clone();
    assert!(
        daisy_prompt.contains(&format!("Committed so far: {agnes_commit}.")),
        "{daisy_prompt}"
    );

    let daisy_commit = format!("Working a Hunch ({})", session.state().name_of(DAISY));
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
    assert_eq!(session.offered_at(&hand_card(DAISY, 0)), 0, "Opportunist");
    assert_eq!(
        session.offered_at(&hand_card(DAISY, 1)),
        1,
        "Unexpected Courage"
    );
}

/// The performer is still offered their own Opportunist, with a helper beside
/// them holding one too.
#[test]
fn the_performer_is_offered_their_own_opportunist() {
    let session = board_with_roland_holding(&[OPPORTUNIST], seat(2).at(HERE).hand([OPPORTUNIST]))
        .session()
        .test_skill(ROLAND, SkillKind::Intellect, 5);
    assert_eq!(session.prompt().responder, Responder::Investigator(ROLAND));
    assert_eq!(session.offered_at(&hand_card(ROLAND, 0)), 1);
}

/// Guts 01089: *"Max 1 committed per skill test. If this test is successful,
/// draw 1 card."* Two willpower icons. Its
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
    assert_eq!(
        session.offered_at(&hand_card(ROLAND, 0)),
        0,
        "Roland's Guts"
    );
}

/// A helper whose only candidate an earlier helper's commit filtered out is
/// skipped, not asked. Agnes and Daisy each hold only Guts; once Agnes commits
/// hers the cap is reached, so the next prompt is Roland's.
#[test]
fn a_helper_left_with_no_candidate_by_an_earlier_commit_is_not_asked() {
    let session = board_with_two_helpers(&[GUTS], &[GUTS])
        .session()
        .test_skill(ROLAND, SkillKind::Willpower, 1);
    assert_eq!(session.prompt().responder, Responder::Investigator(AGNES));
    let session = session.commit(&[GUTS]);
    assert_eq!(session.prompt().responder, Responder::Investigator(ROLAND));
}

// ---- whose result a helper's card is -------------------------------------
//
// Official FAQ (`data/official-faq/Frequently_Asked_Questions.md`):
// *"Generally speaking, the player who committed the skill card gets the
// effects of any ability on that skill card. … However, if a skill card changes
// or adds to the results of the skill test itself, the investigator performing
// the test receives the benefit of that ability."* See **Committer /
// Performer** in `GLOSSARY.md`.

/// An enemy engaged with Roland.
const GHOUL: EnemyId = EnemyId(100);

/// [`board`] with `daisy` seated, the Ghoul (fight 2, health 10, evade 2)
/// engaged with Roland, Roland's stock combat 3 and agility 3.
fn board_with_the_ghoul(daisy: Seat) -> GameStateBuilder {
    board(daisy).with_enemy(
        enemy(GHOUL)
            .fight(2)
            .evade(2)
            .health(10)
            .engaged_with(ROLAND),
    )
}

/// Roland fights the Ghoul; Daisy commits `daisy_commits`, Roland nothing.
fn roland_fights(board: GameStateBuilder, daisy_commits: &[&str]) -> TestSession {
    let session = board
        .session()
        .take(&TurnAction::Fight {
            investigator: ROLAND,
            enemy: GHOUL,
        })
        .commit(daisy_commits)
        .commit(&[]);
    session.assert_at_turn_menu(ROLAND);
    session
}

/// Overpower 01091's ruling (<https://arkhamdb.com/card/01091>): *"If you
/// commit this card to another player's skill test, and the test is
/// successful, you draw a card (not the other player)."* Combat 3 + 2 combat
/// icons against fight 2 succeeds.
#[test]
fn a_helpers_overpower_draws_the_helper_a_card() {
    let session = roland_fights(
        board_with_the_ghoul(seat(2).at(HERE).hand([OVERPOWER]).deck(["spare-1"])),
        &[OVERPOWER],
    );
    let state = session.state();
    assert_eq!(
        state.investigators[&DAISY].hand,
        vec![CardCode::new("spare-1")]
    );
    assert!(
        state.investigators[&ROLAND].hand.is_empty(),
        "Roland draws none"
    );
    assert_eq!(
        state.investigators[&ROLAND].horror(),
        0,
        "no empty-deck draw"
    );
}

/// Fearless 01067: *"If this skill test is successful, heal 1 horror."*
const FEARLESS: &str = "01067";

/// Fearless 01067's ruling (<https://arkhamdb.com/card/01067>): *"If you
/// commit this card to another player's skill test, and the test is
/// successful, you heal 1 horror (not the other player)."* Both carry 1
/// horror; willpower 3 + 1 against 1 succeeds.
#[test]
fn a_helpers_fearless_heals_the_helpers_horror() {
    let session = board_with_roland(
        seat(1).at(HERE).harm(0, 1),
        seat(2).at(HERE).hand([FEARLESS]).harm(0, 1),
    )
    .session()
    .test_skill(ROLAND, SkillKind::Willpower, 1)
    .commit(&[FEARLESS])
    .commit(&[]);
    session.assert_at_turn_menu(ROLAND);
    let state = session.state();
    assert_eq!(state.investigators[&DAISY].horror(), 0, "Daisy heals");
    assert_eq!(state.investigators[&ROLAND].horror(), 1, "Roland does not");
}

/// Deduction 01039: *"If this skill test is successful while investigating a
/// location, discover 1 additional clue at that location."*
const DEDUCTION: &str = "01039";

/// A result modifier is the performer's. Deduction 01039's ruling
/// (<https://arkhamdb.com/card/01039>): *"If you commit Deduction to another
/// player's investigation attempt, that other player would discover 1
/// additional clue, not you"*. Intellect 3 + 1 against shroud 4 succeeds.
#[test]
fn a_helpers_deduction_discovers_the_additional_clue_for_the_performer() {
    let session = board(seat(2).at(HERE).hand([DEDUCTION]))
        .with_location(location(HERE).shroud(4).clues(2))
        .session()
        .take(&TurnAction::Investigate {
            investigator: ROLAND,
        })
        .commit(&[DEDUCTION])
        .commit(&[]);
    session.assert_at_turn_menu(ROLAND);
    let state = session.state();
    assert_eq!(state.investigators[&ROLAND].clues, 2);
    assert_eq!(state.investigators[&DAISY].clues, 0);
    assert_eq!(state.locations[&HERE].clues, 0);
}

/// Vicious Blow 01025: *"If this skill test is successful during an attack,
/// that attack deals +1 damage."*
const VICIOUS_BLOW: &str = "01025";

/// A result modifier is the performer's: Daisy's Vicious Blow gives Roland's
/// attack +1 damage, 1 + 1. Combat 3 + 1 against fight 2 succeeds.
#[test]
fn a_helpers_vicious_blow_adds_damage_to_the_performers_attack() {
    let session = roland_fights(
        board_with_the_ghoul(seat(2).at(HERE).hand([VICIOUS_BLOW])),
        &[VICIOUS_BLOW],
    );
    assert_eq!(session.state().enemies[&GHOUL].damage, 2);
}

/// Survival Instinct 01081: *"If this skill test is successful during an
/// evasion attempt, the evading investigator may immediately disengage from
/// each other enemy engaged with him or her, and may move to a connecting
/// location."*
const SURVIVAL_INSTINCT: &str = "01081";
/// A second enemy engaged with Roland, the "other enemy".
const RAT: EnemyId = EnemyId(101);

/// Survival Instinct's text names the evader, so Daisy's copy offers Roland
/// the disengage and the move, each a Decision (0 = do it), and Roland is the
/// one who moves. Agility 3 + 1 against evade 2 succeeds.
#[test]
fn a_helpers_survival_instinct_offers_the_evader_the_disengage_and_the_move() {
    let session = board_with_the_ghoul(seat(2).at(HERE).hand([SURVIVAL_INSTINCT]))
        .with_location(location(HERE).connections([THERE]))
        .with_location(location(THERE))
        .with_enemy(enemy(RAT).evade(2).engaged_with(ROLAND))
        .session()
        .take(&TurnAction::Evade {
            investigator: ROLAND,
            enemy: GHOUL,
        })
        .commit(&[SURVIVAL_INSTINCT])
        .commit(&[]);
    assert_eq!(session.prompt().responder, Responder::Investigator(ROLAND));
    assert_eq!(session.prompt().nature, PromptNature::Decision);
    let session = session.pick_nth(0);
    assert_eq!(session.prompt().responder, Responder::Investigator(ROLAND));
    assert_eq!(session.prompt().nature, PromptNature::Decision);
    let session = session.pick_nth(0);

    session.assert_at_turn_menu(ROLAND);
    let state = session.state();
    assert_eq!(state.investigators[&ROLAND].current_location, Some(THERE));
    assert_eq!(state.investigators[&DAISY].current_location, Some(HERE));
    assert_ne!(
        state.enemies[&RAT].engaged_with,
        Some(ROLAND),
        "Roland disengaged from the rat and left it behind"
    );
}

/// ST.7: *"the investigator performing the test applies those results in the
/// order of his or her choice"*, a helper's addition included. Daisy's
/// Fearless is committed first, Roland's Guts second; willpower 3 + 2 against
/// 1 succeeds, and both apply whichever Roland picks first.
#[test]
fn the_performer_orders_a_helpers_addition_beside_their_own() {
    let session = board_with_roland(
        seat(1).at(HERE).hand([GUTS]).deck(["spare-1"]),
        seat(2).at(HERE).hand([FEARLESS]).harm(0, 1),
    )
    .session()
    .test_skill(ROLAND, SkillKind::Willpower, 1)
    .commit(&[FEARLESS])
    .commit(&[GUTS]);
    assert_eq!(session.prompt().responder, Responder::Investigator(ROLAND));
    let session = session.pick(OptionTarget::CommittedCard {
        index: CommittedIndex(0),
    });

    session.assert_at_turn_menu(ROLAND);
    let state = session.state();
    assert_eq!(state.investigators[&DAISY].horror(), 0, "Daisy's Fearless");
    assert_eq!(
        state.investigators[&ROLAND].hand,
        vec![CardCode::new("spare-1")],
        "Roland's Guts"
    );
}
