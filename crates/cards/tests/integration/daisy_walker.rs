//! Daisy Walker 01002: the additional action only a Tome's `[action]` ability
//! can use, and the elder sign's one draw per Tome.
//!
//! Card text (`data/arkhamdb-snapshot/pack/core/core.json`, 01002):
//! > You may take an additional action during your turn, which can only be
//! > used on \[\[Tome\]\] \[action\] abilities.
//! > \[elder_sign\] effect: +0. If you succeed, draw 1 card for each
//! > \[\[Tome\]\] you control.
//!
//! Her ruling (<https://arkhamdb.com/card/01002>): *"If you are instructed to
//! lose 1 or more actions, you have that many fewer actions to take during
//! your turn. This is referring to your normal three “full” actions. So if you
//! are instructed to lose 1 or more actions, those must be the ones that are
//! “lost” first. If you have no more of those actions to lose, then you start
//! losing “additional” actions, of your choice. So, for example, if you are
//! playing Daisy and an effect instructs you to “lose 2 actions”, you would
//! have 1 normal action and Daisy’s special additional action left."*

use cards::TEST_REGISTRIES;
use game_core::action::{Action, InputResponse, PlayerAction};
use game_core::engine::enumerate::{self, PlaySource, TurnAction};
use game_core::engine::{self, EngineOutcome, OptionId, OptionTarget};
use game_core::event::Event;
use game_core::state::{
    AbilityAddress, AbilitySource, ChaosBag, ChaosToken, GameState, GameStateBuilder,
    InvestigatorId, LocationId, SkillKind, UseKind,
};
use game_core::test_support::{
    self, seat, uses, AnyKind, BoardLookup as _, CardSetup, Printed, Seat, TestSession,
};
use game_core::{assert_event, assert_no_event};

const DAISY: &str = "01002";
/// *"Item. Tome."* `[action] Exhaust Old Book of Lore: …`
const OLD_BOOK_OF_LORE: &str = "01031";
/// *"Item. Tome."* `[action] Choose an investigator at your location and test
/// [intellect] (2). …`
const MEDICAL_TEXTS: &str = "01035";
/// *"Item. Tool."* `[action] Spend 1 supply: <b>Investigate.</b> …`: an
/// `[action]` ability on a card that is not a Tome.
const FLASHLIGHT: &str = "01087";
/// *"You may take an additional action during your turn."*
const LEO_DE_LUCA: &str = "01048";

const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(10);

const RESOURCE: TurnAction = TurnAction::Resource { investigator: INV };

/// Daisy at `HERE`, with a deck to search and draw from.
fn daisy() -> Seat<Printed> {
    seat(1)
        .as_card(DAISY)
        .at(HERE)
        .deck(["90001", "90002", "90003", "90004"])
}

/// `investigator`'s open turn, alone at `HERE`. A `Numeric(0)` chaos bag lets
/// a skill-testing ability start its test.
fn board(investigator: impl Into<Seat<AnyKind>>) -> TestSession {
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_location(test_support::location(HERE))
        .with_seat(investigator)
        .open_turn(INV)
        .session()
}

/// Activate the first printed ability of `code`, in play under Daisy, and
/// check the engine took it.
fn activate(session: TestSession, code: &str) -> TestSession {
    let action = TurnAction::ActivateAbility {
        investigator: INV,
        source: AbilitySource::InPlay(session.state().in_play(INV, code)),
        address: AbilityAddress::Printed(0),
    };
    let session = session.take(&action);
    assert_eq!(session.rejection(), None, "activating {code}");
    session
}

/// Take the first card of Old Book of Lore's searched top 3, which ends its
/// ability. Sent raw: a deck search's options are cards in a deck, with no
/// board anchor a session step can name (see `old_book_of_lore.rs`).
fn take_first_searched(session: TestSession) -> TestSession {
    session.apply(Action::Player(PlayerAction::ResolveInput {
        response: InputResponse::PickSingle(OptionId(0)),
    }))
}

fn standard_actions(state: &GameState) -> u8 {
    state.investigators[&INV].actions_remaining
}

/// The actions she can still take this turn: standard plus additional.
fn actions(state: &GameState) -> u8 {
    engine::actions_available(state, TEST_REGISTRIES.cards, INV)
}

/// Run `NativeCx::lose_actions` for her, as Blinding Light 01066's *"lose 1
/// action this turn"* does.
fn lose(session: TestSession, count: u8) -> TestSession {
    session.edit_state(|state| {
        let (outcome, _) = test_support::run_native(
            state,
            TEST_REGISTRIES.cards,
            |cx, _| {
                cx.lose_actions(INV, count);
                EngineOutcome::Done
            },
            &engine::EvalContext::for_controller(INV),
        );
        assert!(matches!(outcome, EngineOutcome::Done));
    })
}

/// The turn menu's count includes her additional action, Tome or not: three
/// standard and hers.
#[test]
fn her_turn_counts_four_actions() {
    let session = board(daisy());
    assert_eq!(actions(session.state()), 4);
}

/// Old Book of Lore's `[action]` is a Tome `[action]` ability: her additional
/// action pays for it, and her three standard actions are untouched.
#[test]
fn old_book_of_lore_is_paid_by_her_additional_action() {
    let session = board(daisy().in_play([OLD_BOOK_OF_LORE]));
    let session = activate(session, OLD_BOOK_OF_LORE);
    assert_eq!(standard_actions(session.state()), 3);
    assert_no_event!(session.events(), Event::ActionsRemainingChanged { .. });
    assert_eq!(actions(session.state()), 3);
}

/// Medical Texts' `[action]` is a Tome `[action]` ability too.
#[test]
fn medical_texts_is_paid_by_her_additional_action() {
    let session = board(daisy().in_play([MEDICAL_TEXTS]));
    let session = activate(session, MEDICAL_TEXTS);
    assert_eq!(standard_actions(session.state()), 3);
    assert_no_event!(session.events(), Event::ActionsRemainingChanged { .. });
}

/// *"You may take an additional action"*: one. The second Tome activation of
/// the turn is paid from her standard actions.
#[test]
fn a_second_tome_activation_costs_a_standard_action() {
    let session = board(daisy().in_play([OLD_BOOK_OF_LORE, MEDICAL_TEXTS]));
    let session = take_first_searched(activate(session, OLD_BOOK_OF_LORE));
    session.assert_at_turn_menu(INV);
    assert_eq!(standard_actions(session.state()), 3);

    let session = activate(session, MEDICAL_TEXTS);
    assert_eq!(standard_actions(session.state()), 2);
}

/// Playing a Tome from hand is a play action, not a Tome `[action]` ability:
/// it costs a standard action, and her additional action is still there.
#[test]
fn playing_a_tome_costs_a_standard_action() {
    let session =
        board(daisy().hand([OLD_BOOK_OF_LORE]).resources(3)).take(&TurnAction::PlayCard {
            investigator: INV,
            source: PlaySource::Hand(0),
        });
    session.state().in_play(INV, OLD_BOOK_OF_LORE);
    assert_eq!(standard_actions(session.state()), 2);
    assert_eq!(actions(session.state()), 3);
}

/// A basic action costs a standard action.
#[test]
fn a_basic_action_costs_a_standard_action() {
    let session = board(daisy()).take(&RESOURCE);
    assert_eq!(standard_actions(session.state()), 2);
    assert_eq!(actions(session.state()), 3);
}

/// Flashlight's `[action]` is not on a Tome: it costs a standard action.
#[test]
fn a_non_tome_action_ability_costs_a_standard_action() {
    let session = board(daisy().in_play([(FLASHLIGHT, uses(UseKind::Supplies, 3))]));
    let session = activate(session, FLASHLIGHT);
    assert_eq!(standard_actions(session.state()), 2);
}

/// With no Tome in play, her turn is three actions: once the standard ones
/// are spent the menu offers no further action, though the count still shows
/// her unusable one.
#[test]
fn without_a_tome_she_has_three_actions_to_take() {
    let session = board(daisy())
        .take(&RESOURCE)
        .take(&RESOURCE)
        .take(&RESOURCE);
    session.assert_at_turn_menu(INV);
    assert_eq!(standard_actions(session.state()), 0);
    assert_eq!(actions(session.state()), 1);
    assert!(
        !enumerate::legal_actions(session.state(), TEST_REGISTRIES.cards).contains(&RESOURCE),
        "her additional action cannot pay for a basic action"
    );
}

/// The ruling's own example: *"if you are playing Daisy and an effect
/// instructs you to “lose 2 actions”, you would have 1 normal action and
/// Daisy’s special additional action left."* The Tome activation after it is
/// still paid by her additional action.
#[test]
fn losing_two_actions_leaves_one_standard_and_hers() {
    let session = lose(board(daisy().in_play([OLD_BOOK_OF_LORE])), 2);
    assert_eq!(standard_actions(session.state()), 1);
    assert_eq!(actions(session.state()), 2);

    let session = activate(session, OLD_BOOK_OF_LORE);
    assert_eq!(standard_actions(session.state()), 1, "hers paid for it");
}

/// Daisy's investigator card and Leo De Luca, the anchors of the two
/// additional actions a Tome activation qualifies as.
fn anchors(state: &GameState) -> (OptionTarget, OptionTarget) {
    (
        OptionTarget::CardInstance(state.investigators[&INV].investigator_card.instance_id),
        OptionTarget::CardInstance(state.in_play(INV, LEO_DE_LUCA)),
    )
}

/// With Leo De Luca also in play, a Tome activation qualifies as both his
/// unrestricted additional action and her restricted one. FAQ 1.10
/// (`glossary/Action.md`): *"If an action qualifies as more than one of an
/// investigator’s additional actions, he or she may choose which additional
/// action is used."* She is asked, before anything is paid.
#[test]
fn a_tome_activation_with_leo_in_play_asks_which_additional_action_to_use() {
    let session = activate(
        board(daisy().in_play([LEO_DE_LUCA, OLD_BOOK_OF_LORE])),
        OLD_BOOK_OF_LORE,
    );
    let (hers, leos) = anchors(session.state());
    assert_eq!(session.offered_at(&hers), 1);
    assert_eq!(session.offered_at(&leos), 1);
    assert_eq!(session.prompt().options.len(), 2);
    assert_no_event!(session.events(), Event::AbilityActivated { .. });
    assert_eq!(actions(session.state()), 5, "nothing is paid yet");
}

/// Choosing hers pays for the Tome, and Leo's is left for an action hers
/// could not pay for: a basic action costs no standard action.
#[test]
fn choosing_hers_keeps_leos_for_a_non_tome_action() {
    let session = activate(
        board(daisy().in_play([LEO_DE_LUCA, OLD_BOOK_OF_LORE])),
        OLD_BOOK_OF_LORE,
    );
    let (hers, _) = anchors(session.state());
    let session = take_first_searched(session.pick(hers));
    session.assert_at_turn_menu(INV);
    assert_eq!(standard_actions(session.state()), 3);
    assert_eq!(actions(session.state()), 4);

    let session = session.take(&RESOURCE);
    assert_eq!(standard_actions(session.state()), 3, "Leo's paid for it");
    assert_eq!(actions(session.state()), 3);
}

/// Choosing Leo's pays for the Tome, and hers is left for a second Tome
/// activation this turn.
#[test]
fn choosing_leos_keeps_hers_for_a_second_tome_activation() {
    let session = activate(
        board(daisy().in_play([LEO_DE_LUCA, OLD_BOOK_OF_LORE, MEDICAL_TEXTS])),
        OLD_BOOK_OF_LORE,
    );
    let (_, leos) = anchors(session.state());
    let session = take_first_searched(session.pick(leos));
    session.assert_at_turn_menu(INV);
    assert_eq!(standard_actions(session.state()), 3);
    assert_eq!(actions(session.state()), 4);

    let session = activate(session, MEDICAL_TEXTS);
    assert_eq!(standard_actions(session.state()), 3, "hers paid for it");
}

/// A basic action qualifies only as Leo's: it uses his without asking, and
/// hers is left for a Tome.
#[test]
fn a_non_tome_action_with_leo_in_play_uses_his_without_asking() {
    let session = board(daisy().in_play([LEO_DE_LUCA, OLD_BOOK_OF_LORE])).take(&RESOURCE);
    session.assert_at_turn_menu(INV);
    assert_eq!(standard_actions(session.state()), 3);
    assert_eq!(actions(session.state()), 4);

    let session = take_first_searched(activate(session, OLD_BOOK_OF_LORE));
    session.assert_at_turn_menu(INV);
    assert_eq!(
        standard_actions(session.state()),
        3,
        "hers paid for the Tome"
    );
}

/// Daisy's Willpower 3 test against `difficulty`, with only the elder sign in
/// the bag, committing nothing. Returns once the test has resolved.
fn willpower_test_on_the_elder_sign(
    daisy: impl Into<Seat<AnyKind>>,
    difficulty: i8,
) -> TestSession {
    let session = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_chaos_bag(ChaosBag::new([ChaosToken::ElderSign]))
        .with_location(test_support::location(HERE))
        .with_seat(daisy)
        .open_turn(INV)
        .session()
        .test_skill(INV, SkillKind::Willpower, difficulty)
        .commit(&[]);
    session.assert_at_turn_menu(INV);
    session
}

fn hand_size(state: &GameState) -> usize {
    state.investigators[&INV].hand.len()
}

/// *"+0. If you succeed, draw 1 card for each \[\[Tome\]\] you control."* Two
/// Tomes in play: Willpower 3 + 0 against 3 succeeds, and she draws two.
#[test]
fn the_elder_sign_draws_one_card_per_tome_in_play() {
    let session =
        willpower_test_on_the_elder_sign(daisy().in_play([OLD_BOOK_OF_LORE, MEDICAL_TEXTS]), 3);
    assert_event!(
        session.events(),
        Event::SkillTestSucceeded { margin: 0, .. }
    );
    assert_event!(
        session.events(),
        Event::CardsDrawn { investigator, count: 2 } if *investigator == INV
    );
    assert_eq!(hand_size(session.state()), 2);
}

/// A non-Tome in play is not counted: one Tome, one draw.
#[test]
fn the_elder_sign_counts_only_tomes() {
    let session = willpower_test_on_the_elder_sign(
        daisy().in_play([
            (OLD_BOOK_OF_LORE, CardSetup::default()),
            (FLASHLIGHT, uses(UseKind::Supplies, 3)),
        ]),
        3,
    );
    assert_event!(
        session.events(),
        Event::CardsDrawn { investigator, count: 1 } if *investigator == INV
    );
    assert_eq!(hand_size(session.state()), 1);
}

/// *"If you succeed"*: a failed test draws nothing, Tomes or not.
#[test]
fn the_elder_sign_draws_nothing_when_the_test_fails() {
    let session =
        willpower_test_on_the_elder_sign(daisy().in_play([OLD_BOOK_OF_LORE, MEDICAL_TEXTS]), 4);
    assert_event!(session.events(), Event::SkillTestFailed { by: 1, .. });
    assert_no_event!(session.events(), Event::CardsDrawn { .. });
    assert_eq!(hand_size(session.state()), 0);
}

/// With no Tome, *"1 card for each"* is zero cards: the success draws nothing.
#[test]
fn the_elder_sign_draws_nothing_without_a_tome() {
    let session = willpower_test_on_the_elder_sign(daisy(), 3);
    assert_event!(
        session.events(),
        Event::SkillTestSucceeded { margin: 0, .. }
    );
    assert_no_event!(session.events(), Event::CardsDrawn { .. });
    assert_eq!(hand_size(session.state()), 0);
}
