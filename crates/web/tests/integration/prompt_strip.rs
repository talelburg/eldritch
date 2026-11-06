//! Headless tests for `PromptStrip`, one investigator's prompt strip, mounted
//! on its own: the dock renders it while their tab is open.
//!
//! The strips are the **floor**: the strip of an investigator of yours the
//! live prompt asks, headed with their name, names the prompt and homes
//! whatever has no board surface — un-anchored options, a `PickMultiple`
//! commit's Confirm, a skippable window's Pass, and a fallback Confirm. Its one
//! silence is the open-turn menu, recognised by its `TurnControl` anchor rather
//! than by its text.
#![cfg(target_arch = "wasm32")]

use std::collections::BTreeSet;

use futures::channel::mpsc::{self, UnboundedReceiver};
use game_core::action::{InputResponse, PlayerAction};
use game_core::engine::{
    ChoiceOption, EngineOutcome, InputRequest, OptionId, OptionTarget, PickCount, Responder,
};
use game_core::state::investigator::InvestigatorId;
use game_core::state::{CommittedIndex, GameState, GameStateBuilder};
use game_core::test_support::{self, seat, BoardLookup as _};
use leptos::prelude::*;
use leptos::task;
use protocol::{Assignment, ClientMessage};
use wasm_bindgen::JsCast as _;
use wasm_bindgen_test::*;
use web::interaction::MultiSelect;
use web::prompt_strip::PromptStrip;
use web::store::ClientState;
use web::transport::OutboundTx;
use web_sys::{Element, HtmlElement};

use crate::common;

/// Investigator 1, whom the `test_support` prompts ask.
const SEAT_ONE: InvestigatorId = InvestigatorId(1);

/// Mount [`SEAT_ONE`]'s `PromptStrip` with a store carrying `outcome`, a
/// `MultiSelect` whose `selected` starts as `preselected`, and a capturing
/// channel.
async fn mount(outcome: EngineOutcome, preselected: &[u32]) -> UnboundedReceiver<ClientMessage> {
    mount_with(None, outcome, preselected, None, SEAT_ONE).await
}

/// [`mount`], for the strip of `investigator`, with the store also holding
/// `game`, so the strip can name investigators, and signed in as an account
/// holding only `held` when it is `Some`, or every investigator when it is
/// `None`.
async fn mount_with(
    game: Option<GameState>,
    outcome: EngineOutcome,
    preselected: &[u32],
    held: Option<&[InvestigatorId]>,
    investigator: InvestigatorId,
) -> UnboundedReceiver<ClientMessage> {
    let store = RwSignal::new(ClientState::default());
    store.update(|s| {
        s.game = game;
        s.outcome = Some(outcome);
        common::you_hold_every_investigator(s);
        if let Some(held) = held {
            s.assigned = Assignment::all_to(common::YOU, held.iter().copied());
        }
    });
    let selected = RwSignal::new(preselected.iter().copied().collect::<BTreeSet<u32>>());
    let (tx, rx) = mpsc::unbounded::<ClientMessage>();
    let tx_for_mount: OutboundTx = tx;
    mount_to_body(move || {
        provide_context(store);
        provide_context::<OutboundTx>(tx_for_mount.clone());
        let active = Signal::derive(move || store.with(web::interaction::is_multi_select));
        provide_context(MultiSelect { active, selected });
        view! { <div class="strip-root"><PromptStrip investigator=investigator/></div> }
    });
    task::tick().await;
    rx
}

/// The last-mounted `.strip-root` wrapper — scopes queries to this test's mount so
/// DOM accumulation across tests can't shadow an "absence" assertion.
fn last_root() -> Element {
    let roots = document().query_selector_all(".strip-root").expect("query");
    roots
        .item(roots.length() - 1)
        .and_then(|n| n.dyn_into::<Element>().ok())
        .expect("a .strip-root")
}

fn last_strip() -> Element {
    last_root()
        .query_selector(".prompt-strip")
        .expect("query")
        .and_then(|n| n.dyn_into::<Element>().ok())
        .expect("a .prompt-strip")
}

fn click(sel: &str) {
    last_strip()
        .query_selector(sel)
        .expect("query")
        .and_then(|n| n.dyn_into::<HtmlElement>().ok())
        .expect("element")
        .click();
}

#[wasm_bindgen_test]
async fn confirm_submits_the_selected_options() {
    let mut rx = mount(
        test_support::awaiting_commit_input("Commit", &[0, 1, 2]),
        &[0, 2],
    )
    .await;
    click(".confirm");
    task::tick().await;
    let msg = rx.try_recv().expect("a frame after tick");
    match msg {
        ClientMessage::Submit {
            action: PlayerAction::ResolveInput { response, .. },
            ..
        } => assert_eq!(
            response,
            InputResponse::PickMultiple {
                selected: vec![OptionId(0), OptionId(2)]
            }
        ),
        other => panic!("expected PickMultiple, got {other:?}"),
    }
}

#[wasm_bindgen_test]
async fn confirm_with_no_selection_submits_empty() {
    let mut rx = mount(test_support::awaiting_commit_input("Commit", &[0]), &[]).await;
    click(".confirm");
    task::tick().await;
    let msg = rx.try_recv().expect("a frame after tick");
    match msg {
        ClientMessage::Submit {
            action: PlayerAction::ResolveInput { response, .. },
            ..
        } => assert_eq!(response, InputResponse::PickMultiple { selected: vec![] }),
        other => panic!("expected PickMultiple, got {other:?}"),
    }
}

#[wasm_bindgen_test]
async fn skippable_prompt_shows_pass_that_submits_skip() {
    let mut rx = mount(
        test_support::awaiting_skippable_commit_input("Commit", &[0]),
        &[],
    )
    .await;
    click(".pass");
    task::tick().await;
    let msg = rx.try_recv().expect("a frame after tick");
    match msg {
        ClientMessage::Submit {
            action: PlayerAction::ResolveInput { response, .. },
            ..
        } => assert_eq!(response, InputResponse::Skip),
        other => panic!("expected Skip, got {other:?}"),
    }
}

/// A hand-size discard that must discard exactly one card, offering two.
fn discard_exactly_one() -> EngineOutcome {
    test_support::awaiting_request(
        InputRequest::pick_multiple(
            "Choose 1 card to discard",
            vec![
                ChoiceOption::new(OptionId(0), "Flashlight"),
                ChoiceOption::new(OptionId(1), "Knife"),
            ],
            PickCount::exactly(1),
        )
        .answered_by(Responder::Investigator(InvestigatorId(1))),
    )
}

fn confirm_disabled() -> bool {
    last_strip()
        .query_selector(".confirm")
        .expect("query")
        .expect("a Confirm")
        .has_attribute("disabled")
}

/// The engine rejects a discard of the wrong size, so Confirm holds until the
/// selection is one the count admits.
#[wasm_bindgen_test]
async fn confirm_holds_until_the_selection_is_the_count_asked() {
    let mut rx = mount(discard_exactly_one(), &[]).await;
    assert!(confirm_disabled(), "no card chosen of the one asked");
    click(".confirm");
    task::tick().await;
    assert!(rx.try_recv().is_err(), "a disabled Confirm submits nothing");

    let _rx = mount(discard_exactly_one(), &[0, 1]).await;
    assert!(confirm_disabled(), "two cards chosen of the one asked");

    let mut rx = mount(discard_exactly_one(), &[1]).await;
    assert!(!confirm_disabled(), "the one card asked is chosen");
    click(".confirm");
    task::tick().await;
    match rx.try_recv().expect("a frame after tick") {
        ClientMessage::Submit {
            action: PlayerAction::ResolveInput { response, .. },
            ..
        } => assert_eq!(
            response,
            InputResponse::PickMultiple {
                selected: vec![OptionId(1)]
            }
        ),
        other => panic!("expected PickMultiple, got {other:?}"),
    }
}

#[wasm_bindgen_test]
async fn renders_the_prompt_text() {
    let _rx = mount(
        test_support::awaiting_commit_input("Redraw your opening hand", &[0]),
        &[],
    )
    .await;
    assert!(
        last_strip()
            .text_content()
            .unwrap_or_default()
            .contains("Redraw your opening hand"),
        "strip shows the prompt text"
    );
}

#[wasm_bindgen_test]
async fn an_unanchored_confirm_gets_text_and_a_confirm_button() {
    // The strip is the floor, so a prompt the
    // client does not specifically home still tells the player the engine is
    // waiting — and an un-anchored `Confirm` the result modal is not carrying gets
    // a Confirm here rather than being unreachable.
    let mut rx = mount(
        test_support::awaiting_confirm_input("Something happened"),
        &[],
    )
    .await;
    assert!(last_strip()
        .text_content()
        .unwrap_or_default()
        .contains("Something happened"));
    click(".confirm");
    task::tick().await;
    match rx.try_recv().expect("a frame after tick") {
        ClientMessage::Submit {
            action: PlayerAction::ResolveInput { response, .. },
            ..
        } => assert_eq!(response, InputResponse::Confirm),
        other => panic!("expected Confirm, got {other:?}"),
    }
}

#[wasm_bindgen_test]
async fn an_anchored_confirm_gets_text_but_no_strip_confirm() {
    // The Mythos draw's button lives on the encounter deck. The strip still
    // names the prompt — the floor — but must not render a second Confirm, which
    // is the duplication the bar retirement exists to end.
    let mut outcome = test_support::awaiting_confirm_input("Draw");
    if let EngineOutcome::AwaitingInput { request, .. } = &mut outcome {
        request.target = Some(OptionTarget::EncounterDeck);
    }
    let _rx = mount(outcome, &[]).await;
    let strip = last_strip();
    assert!(strip.text_content().unwrap_or_default().contains("Draw"));
    assert!(
        strip.query_selector(".confirm").expect("query").is_none(),
        "the encounter deck carries this prompt's only Confirm"
    );
}

#[wasm_bindgen_test]
async fn skippable_window_shows_prompt_and_pass_submits_skip() {
    // A skippable PickSingle (reaction/Fast window) → strip with prompt + Pass.
    let outcome = test_support::awaiting_skippable_pick_single_input("You may trigger");
    let mut rx = mount(outcome, &[]).await;
    assert!(
        last_strip()
            .text_content()
            .unwrap_or_default()
            .contains("You may trigger"),
        "window prompt shows in the strip"
    );
    click(".pass");
    task::tick().await;
    let msg = rx.try_recv().expect("a frame after tick");
    match msg {
        ClientMessage::Submit {
            action: PlayerAction::ResolveInput { response, .. },
            ..
        } => assert_eq!(response, InputResponse::Skip),
        other => panic!("expected Skip, got {other:?}"),
    }
}

#[wasm_bindgen_test]
async fn the_open_turn_menu_is_suppressed_by_its_anchor() {
    // The one prompt the strip stays silent about. It is identified by the
    // `TurnControl` anchor the engine attaches, never by matching "Choose an
    // action" — the string coupling ADR 0011 exists to prevent. Reserving the one
    // persistent surface means not spending it on every ordinary turn.
    let mut outcome = test_support::awaiting_pick_single_with(
        "Choose an action",
        vec![ChoiceOption::new(OptionId(0), "End turn")
            .at(OptionTarget::TurnControl(InvestigatorId(1)))],
    );
    if let EngineOutcome::AwaitingInput { request, .. } = &mut outcome {
        request.target = Some(OptionTarget::TurnControl(InvestigatorId(1)));
    }
    let _rx = mount(outcome, &[]).await;
    assert!(
        last_root()
            .query_selector(".prompt-strip")
            .expect("query")
            .is_none(),
        "an ordinary turn — every option on a board surface — renders no strip at all"
    );
}

#[wasm_bindgen_test]
async fn the_open_turn_menu_suppresses_its_text_but_not_its_controls() {
    // Only the text. An open-turn action whose own anchor is `None` — Investigate
    // with no current location, EndTurn off-frame, both pinned by engine tests —
    // has no board home, so suppressing the whole strip would make it
    // unreachable rather than merely misplaced (ADR 0011's floor).
    let mut outcome = test_support::awaiting_pick_single_with(
        "Choose an action",
        vec![
            ChoiceOption::new(OptionId(0), "End turn")
                .at(OptionTarget::TurnControl(InvestigatorId(1))),
            ChoiceOption::new(OptionId(1), "Investigate"),
        ],
    );
    if let EngineOutcome::AwaitingInput { request, .. } = &mut outcome {
        request.target = Some(OptionTarget::TurnControl(InvestigatorId(1)));
    }
    let mut rx = mount(outcome, &[]).await;
    let strip = last_strip();
    assert!(
        strip.query_selector(".prompt").expect("query").is_none(),
        "no 'Choose an action' noise on an ordinary turn"
    );
    let btns = strip.query_selector_all(".strip-option").expect("query");
    assert_eq!(
        btns.length(),
        1,
        "the un-anchored option keeps its only home; the anchored one is on its surface"
    );
    click(".strip-option");
    task::tick().await;
    match rx.try_recv().expect("a frame after tick") {
        ClientMessage::Submit {
            action: PlayerAction::ResolveInput { response, .. },
            ..
        } => assert_eq!(response, InputResponse::PickSingle(OptionId(1))),
        other => panic!("expected PickSingle, got {other:?}"),
    }
}

#[wasm_bindgen_test]
async fn an_unanchored_pick_single_still_reaches_the_strip() {
    // Not skippable, not multi, no board home: the strip
    // is the only thing between this option and being unreachable. The
    // fixture's two options are un-anchored.
    let outcome = test_support::awaiting_pick_single_input("Choose one");
    let mut rx = mount(outcome, &[]).await;
    let strip = last_strip();
    assert!(strip
        .text_content()
        .unwrap_or_default()
        .contains("Choose one"));
    assert_eq!(
        strip
            .query_selector_all(".strip-option")
            .expect("query")
            .length(),
        2
    );
    click(".strip-option");
    task::tick().await;
    match rx.try_recv().expect("a frame after tick") {
        ClientMessage::Submit {
            action: PlayerAction::ResolveInput { response, .. },
            ..
        } => assert_eq!(response, InputResponse::PickSingle(OptionId(0))),
        other => panic!("expected PickSingle, got {other:?}"),
    }
}

/// Regression for #549: a skippable window's options render as strip buttons
/// (not only Pass), so a Board/Global option such as the round-end advance is
/// reachable.
#[wasm_bindgen_test]
async fn skippable_window_renders_options_that_submit_pick_single() {
    let mut rx = mount(
        test_support::awaiting_skippable_pick_single_input("You may advance"),
        &[],
    )
    .await;
    let btn = last_strip()
        .query_selector(".strip-option")
        .expect("query")
        .and_then(|n| n.dyn_into::<HtmlElement>().ok())
        .expect("an option button");
    assert_eq!(btn.text_content().unwrap_or_default(), "Resolve");
    btn.click();
    task::tick().await;
    let msg = rx.try_recv().expect("a frame after tick");
    match msg {
        ClientMessage::Submit {
            action: PlayerAction::ResolveInput { response, .. },
            ..
        } => assert_eq!(response, InputResponse::PickSingle(OptionId(0))),
        other => panic!("expected PickSingle, got {other:?}"),
    }
}

#[wasm_bindgen_test]
async fn strip_renders_only_unanchored_options() {
    // With the round-end advance anchored to the act card, the strip does not
    // duplicate anchored options — it renders only un-anchored ones.
    let outcome = test_support::awaiting_skippable_pick_single_with(
        "You may advance",
        vec![
            ChoiceOption::new(OptionId(0), "Advance act").at(OptionTarget::Act),
            ChoiceOption::new(OptionId(1), "Some global"),
        ],
    );
    let mut rx = mount(outcome, &[]).await;
    let strip = last_strip();
    let btns = strip.query_selector_all(".strip-option").expect("query");
    assert_eq!(
        btns.length(),
        1,
        "only the un-anchored option renders as a button"
    );
    let btn = btns
        .item(0)
        .and_then(|n| n.dyn_into::<HtmlElement>().ok())
        .expect("the one strip button");
    assert_eq!(btn.text_content().unwrap_or_default(), "Some global");
    btn.click();
    task::tick().await;
    let msg = rx.try_recv().expect("a frame after tick");
    match msg {
        ClientMessage::Submit {
            action: PlayerAction::ResolveInput { response, .. },
            ..
        } => assert_eq!(response, InputResponse::PickSingle(OptionId(1))),
        other => panic!("expected PickSingle, got {other:?}"),
    }
}

/// The ST.7 result-order prompt anchors a committed card's result to its place
/// in the committed list, which no board surface renders yet, so the strip
/// homes it, labelled with the card's name, rather than leave it unreachable
/// (ADR 0011's committed-card stopgap).
#[wasm_bindgen_test]
async fn a_committed_cards_option_renders_on_the_strip() {
    let outcome = test_support::awaiting_pick_single_with(
        "Choose which result of the skill test to apply next.",
        vec![
            // Anchored to a board surface, so not the strip's.
            ChoiceOption::new(OptionId(0), "Result of the act").at(OptionTarget::Act),
            ChoiceOption::new(OptionId(1), "Result of Overpower").at(OptionTarget::CommittedCard {
                index: CommittedIndex(0),
            }),
        ],
    );
    let mut rx = mount(outcome, &[]).await;
    let btns = last_strip()
        .query_selector_all(".strip-option")
        .expect("query");
    assert_eq!(btns.length(), 1, "only the committed card's option");
    let btn = btns
        .item(0)
        .and_then(|n| n.dyn_into::<HtmlElement>().ok())
        .expect("the one strip button");
    assert_eq!(
        btn.text_content().unwrap_or_default(),
        "Result of Overpower"
    );
    btn.click();
    task::tick().await;
    match rx.try_recv().expect("a frame after tick") {
        ClientMessage::Submit {
            action: PlayerAction::ResolveInput { response, .. },
            ..
        } => assert_eq!(response, InputResponse::PickSingle(OptionId(1))),
        other => panic!("expected PickSingle, got {other:?}"),
    }
}

/// A window several answer, offering investigator 2 and investigator 3 one
/// un-anchored reaction each.
fn offered_to_two_and_three() -> EngineOutcome {
    test_support::awaiting_request(
        InputRequest::pick_single(
            "You may trigger",
            vec![
                ChoiceOption::new(OptionId(0), "Two's reaction").offered_to(InvestigatorId(2)),
                ChoiceOption::new(OptionId(1), "Three's reaction").offered_to(InvestigatorId(3)),
            ],
        )
        .skippable()
        .answered_by(Responder::Several),
    )
}

/// The investigator a submitted frame names as answering.
fn answering(rx: &mut UnboundedReceiver<ClientMessage>) -> (InvestigatorId, InputResponse) {
    match rx.try_recv().expect("a frame after tick") {
        ClientMessage::Submit {
            action:
                PlayerAction::ResolveInput {
                    investigator,
                    response,
                },
            ..
        } => (investigator, response),
        other => panic!("expected ResolveInput, got {other:?}"),
    }
}

/// In a window several answer, a pick names the investigator its option is
/// offered to.
#[wasm_bindgen_test]
async fn a_pick_in_a_window_several_answer_names_its_options_investigator() {
    let mut rx = mount_with(None, offered_to_two_and_three(), &[], None, THREE).await;
    click(".strip-option");
    task::tick().await;
    assert_eq!(
        answering(&mut rx),
        (THREE, InputResponse::PickSingle(OptionId(1)))
    );
}

/// Investigator 2, asked first in [`offered_to_two_and_three`].
const TWO: InvestigatorId = InvestigatorId(2);
/// Investigator 3, asked second in [`offered_to_two_and_three`].
const THREE: InvestigatorId = InvestigatorId(3);

/// The game [`offered_to_two_and_three`] is asked in: investigators 2 and 3
/// seated.
fn two_and_three_seated() -> GameState {
    GameStateBuilder::new()
        .with_seat(seat(2))
        .with_seat(seat(3))
        .with_seating([TWO, THREE])
        .build()
}

/// The `.pass` buttons the last strip renders, in order.
fn pass_buttons() -> Vec<HtmlElement> {
    let btns = last_strip().query_selector_all(".pass").expect("query");
    (0..btns.length())
        .filter_map(|i| btns.item(i))
        .filter_map(|n| n.dyn_into::<HtmlElement>().ok())
        .collect()
}

/// In a window several answer, each investigator holding an option passes
/// separately (ADR 0019): each gets a strip headed with their name, with the
/// prompt, their own option and a plain Pass that passes for them.
#[wasm_bindgen_test]
async fn a_window_two_answer_shows_a_strip_with_a_plain_pass_for_each() {
    let game = two_and_three_seated();
    for (investigator, option) in [(TWO, "Two's reaction"), (THREE, "Three's reaction")] {
        let name = game.name_of(investigator);
        let mut rx = mount_with(
            Some(game.clone()),
            offered_to_two_and_three(),
            &[],
            None,
            investigator,
        )
        .await;
        let strip = last_strip();
        assert_eq!(common::texts_in(&strip, ".strip-name"), [name]);
        assert_eq!(common::texts_in(&strip, ".strip-option"), [option]);
        assert_eq!(common::texts_in(&strip, ".prompt"), ["You may trigger"]);
        assert_eq!(common::texts_in(&strip, ".pass"), ["Pass"]);
        click(".pass");
        task::tick().await;
        assert_eq!(answering(&mut rx), (investigator, InputResponse::Skip));
    }
}

/// An option naming no investigator goes on the strip of the first of this
/// account's investigators holding an option, the one the store answers it as.
#[wasm_bindgen_test]
async fn an_option_naming_no_investigator_goes_on_the_first_strip() {
    let outcome = test_support::awaiting_request(
        InputRequest::pick_single(
            "You may trigger",
            vec![
                ChoiceOption::new(OptionId(0), "Two's reaction").offered_to(TWO),
                ChoiceOption::new(OptionId(1), "Three's reaction").offered_to(THREE),
                ChoiceOption::new(OptionId(2), "No one's option"),
            ],
        )
        .skippable()
        .answered_by(Responder::Several),
    );
    let game = two_and_three_seated();
    let _rx = mount_with(Some(game.clone()), outcome.clone(), &[], None, TWO).await;
    assert_eq!(
        common::texts_in(&last_strip(), ".strip-option"),
        ["Two's reaction", "No one's option"]
    );
    let _rx = mount_with(Some(game), outcome, &[], None, THREE).await;
    assert_eq!(
        common::texts_in(&last_strip(), ".strip-option"),
        ["Three's reaction"]
    );
}

/// A strip is headed with the name of the investigator it answers for, so a
/// player holding two knows whose hand Agnes Baker's hand-size discard comes
/// from.
#[wasm_bindgen_test]
async fn a_strip_is_headed_with_its_investigators_name() {
    let agnes = InvestigatorId(1);
    let game = GameStateBuilder::new()
        .with_registries(cards::TEST_REGISTRIES)
        .with_seat(seat(agnes.0).as_card("01004"))
        .build();
    let discard = test_support::awaiting_request(
        InputRequest::pick_multiple(
            "Choose 1 card to discard",
            vec![ChoiceOption::new(OptionId(0), "Flashlight")],
            PickCount::exactly(1),
        )
        .answered_by(Responder::Investigator(agnes)),
    );
    let _rx = mount_with(Some(game), discard, &[], None, agnes).await;
    assert_eq!(
        common::texts_in(&last_strip(), ".strip-name"),
        ["Agnes Baker"]
    );
}

/// When only one investigator in a window several answer still holds an
/// option, the strip shows a single plain Pass, which passes for them.
#[wasm_bindgen_test]
async fn a_window_one_still_answers_shows_one_plain_pass() {
    let outcome = test_support::awaiting_request(
        InputRequest::pick_single(
            "You may trigger",
            vec![ChoiceOption::new(OptionId(1), "Three's reaction").offered_to(THREE)],
        )
        .skippable()
        .answered_by(Responder::Several),
    );
    let mut rx = mount_with(Some(two_and_three_seated()), outcome, &[], None, THREE).await;
    let labels: Vec<String> = pass_buttons()
        .iter()
        .map(|b| b.text_content().unwrap_or_default())
        .collect();
    assert_eq!(labels, ["Pass"]);
    click(".pass");
    task::tick().await;
    assert_eq!(answering(&mut rx), (THREE, InputResponse::Skip));
}

/// A viewer's account holds nobody in the window investigators 2 and 3
/// answer: neither strip offers it an option or a Pass, and neither says
/// anything.
#[wasm_bindgen_test]
async fn a_viewer_is_offered_no_option_and_no_pass() {
    for investigator in [TWO, THREE] {
        let _rx = mount_with(
            Some(two_and_three_seated()),
            offered_to_two_and_three(),
            &[],
            Some(&[]),
            investigator,
        )
        .await;
        assert!(
            last_root()
                .query_selector(".prompt-strip")
                .expect("query")
                .is_none(),
            "a viewer's strip renders nothing: {}",
            last_root().inner_html()
        );
    }
}

/// An account holding investigator 3, but not 2, in a window both answer gets
/// only 3's option and a single plain Pass, which passes for 3.
#[wasm_bindgen_test]
async fn an_account_holding_one_answerer_gets_its_option_and_a_plain_pass_for_them() {
    let mut rx = mount_with(
        Some(two_and_three_seated()),
        offered_to_two_and_three(),
        &[],
        Some(&[THREE]),
        THREE,
    )
    .await;
    assert_eq!(
        common::texts_in(&last_strip(), ".strip-option"),
        ["Three's reaction"]
    );
    let labels: Vec<String> = pass_buttons()
        .iter()
        .map(|b| b.text_content().unwrap_or_default())
        .collect();
    assert_eq!(labels, ["Pass"]);
    click(".pass");
    task::tick().await;
    assert_eq!(answering(&mut rx), (THREE, InputResponse::Skip));
}
