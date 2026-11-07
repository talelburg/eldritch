use game_core::engine::{ChoiceOption, InputRequest, OptionId};
use game_core::scenario::ScenarioEnding;
use game_core::state::{DeckIndex, GameState, GameStateBuilder};
use game_core::test_support::{self, seat};
use protocol::lobby::Member;
use protocol::{Assignment, ServerMessage};

use super::*;

// The table: Tal holds Roland Banks, the lead, and Agnes Baker. Sam holds
// Daisy Walker, Jo holds Wendy Adams, and Alex is a viewer. The ids differ
// from the seating order, so a test can tell the two apart.
const ROLAND: InvestigatorId = InvestigatorId(1);
const WENDY: InvestigatorId = InvestigatorId(2);
const DAISY: InvestigatorId = InvestigatorId(3);
const AGNES: InvestigatorId = InvestigatorId(4);

const TAL: AccountId = AccountId(1);
const SAM: AccountId = AccountId(2);
const ALEX: AccountId = AccountId(3);
const JO: AccountId = AccountId(4);

/// The four investigators, seated Roland, Daisy, Agnes, Wendy.
fn table() -> GameState {
    GameStateBuilder::new()
        .with_registries(cards::TEST_REGISTRIES)
        .with_seat(seat(ROLAND.0).as_card("01001"))
        .with_seat(seat(WENDY.0).as_card("01005"))
        .with_seat(seat(DAISY.0).as_card("01002"))
        .with_seat(seat(AGNES.0).as_card("01004"))
        .with_seating([ROLAND, DAISY, AGNES, WENDY])
        .build()
}

/// Tal, Sam, Alex and Jo, in the order they joined.
fn members() -> Vec<Member> {
    [(TAL, "Tal"), (SAM, "Sam"), (ALEX, "Alex"), (JO, "Jo")]
        .into_iter()
        .map(|(account, name)| Member {
            account,
            display_name: name.to_owned(),
        })
        .collect()
}

/// Who holds whom at [`table`].
fn assigned() -> Assignment {
    Assignment::from_iter([(ROLAND, TAL), (AGNES, TAL), (DAISY, SAM), (WENDY, JO)])
}

/// The client of `you` at [`table`], with `request` live.
fn seen_by(you: AccountId, request: InputRequest) -> ClientState {
    ClientState {
        game: Some(table()),
        outcome: Some(test_support::awaiting_request(request)),
        you: Some(you),
        assigned: assigned(),
        members: members(),
        ..ClientState::default()
    }
}

/// Daisy's horror to assign, which Sam answers.
fn daisys_horror() -> InputRequest {
    InputRequest::confirm("Assign the horror").answered_by(Responder::Investigator(DAISY))
}

/// Tal giving Agnes to Sam.
fn gave_agnes_to_sam() -> ServerMessage {
    ServerMessage::Assigned {
        by: TAL,
        investigator: AGNES,
        assigned: Assignment::from_iter([(ROLAND, TAL), (AGNES, SAM), (DAISY, SAM), (WENDY, JO)]),
    }
}

/// `investigator`'s tab, settled on without a click and with no prompt seen.
fn settled_on(investigator: InvestigatorId) -> OpenTab {
    OpenTab {
        tab: TabPick::Settled(investigator),
        prompt: None,
    }
}

/// The layout of `state` before any tab is open.
fn layout_of(state: &ClientState) -> PromptLayout {
    layout(state, OpenTab::default())
}

/// `state` with `active` taking their turn.
fn with_active(mut state: ClientState, active: InvestigatorId) -> ClientState {
    if let Some(game) = &mut state.game {
        game.active_investigator = Some(active);
    }
    state
}

/// The investigator of each tab, in order.
fn tab_order(state: &ClientState) -> Vec<InvestigatorId> {
    layout_of(state)
        .tabs
        .iter()
        .map(|tab| tab.investigator)
        .collect()
}

/// The tab `state` opens when `open_tab` is what this client settled so far.
fn opens(state: &ClientState, open_tab: OpenTab) -> Option<InvestigatorId> {
    layout(state, open_tab).open_investigator
}

#[test]
fn the_tabs_come_in_seating_order_and_in_investigator_order_with_no_seating() {
    let mut state = seen_by(ALEX, daisys_horror());
    assert_eq!(tab_order(&state), vec![ROLAND, DAISY, AGNES, WENDY]);

    if let Some(game) = &mut state.game {
        game.seating.clear();
    }
    assert_eq!(tab_order(&state), vec![ROLAND, WENDY, DAISY, AGNES]);
}

#[test]
fn each_tab_marks_whether_you_hold_them_whose_turn_it_is_and_who_leads() {
    let tal = with_active(seen_by(TAL, daisys_horror()), AGNES);
    let marks: Vec<_> = layout_of(&tal)
        .tabs
        .iter()
        .map(|tab| (tab.investigator, tab.held, tab.active, tab.lead))
        .collect();
    assert_eq!(
        marks,
        vec![
            (ROLAND, true, false, true),
            (DAISY, false, false, false),
            (AGNES, true, true, false),
            (WENDY, false, false, false),
        ]
    );

    let alex = with_active(seen_by(ALEX, daisys_horror()), AGNES);
    assert!(
        layout_of(&alex).tabs.iter().all(|tab| !tab.held),
        "a viewer holds no tab"
    );
}

/// Once the scenario has ended nobody takes a turn and nobody leads, so no tab
/// carries ▶ or ★.
#[test]
fn no_tab_is_marked_once_the_scenario_has_ended() {
    let mut tal = with_active(
        ClientState {
            outcome: Some(EngineOutcome::Done),
            ..seen_by(TAL, daisys_horror())
        },
        ROLAND,
    );
    if let Some(game) = &mut tal.game {
        game.ending = Some(ScenarioEnding::NoResolution);
    }
    let marked: Vec<_> = layout_of(&tal)
        .tabs
        .iter()
        .filter(|tab| tab.active || tab.lead)
        .map(|tab| tab.investigator)
        .collect();
    assert_eq!(marked, vec![]);
}

#[test]
fn tal_sees_a_spinner_on_daisys_tab_while_sam_decides() {
    let spinning = |you| -> Vec<InvestigatorId> {
        layout_of(&seen_by(you, daisys_horror()))
            .tabs
            .iter()
            .filter(|tab| tab.asked == Some(Placement::Spinner))
            .map(|tab| tab.investigator)
            .collect()
    };
    assert_eq!(spinning(TAL), vec![DAISY]);
    assert_eq!(spinning(SAM), vec![], "Sam answers, so no spinner");
}

#[test]
fn an_account_opens_on_its_active_investigator_else_its_first_in_seating() {
    // Daisy's horror is Sam's to answer, so no prompt is Tal's.
    let agnes_turn = with_active(seen_by(TAL, daisys_horror()), AGNES);
    assert_eq!(opens(&agnes_turn, OpenTab::default()), Some(AGNES));

    let daisys_turn = with_active(seen_by(TAL, daisys_horror()), DAISY);
    assert_eq!(opens(&daisys_turn, OpenTab::default()), Some(ROLAND));

    // Once settled, an account's tab stays put as the turn moves on.
    let rolands_turn = with_active(seen_by(TAL, daisys_horror()), ROLAND);
    assert_eq!(opens(&rolands_turn, settled_on(AGNES)), Some(AGNES));
}

#[test]
fn an_account_with_a_prompt_its_own_at_load_opens_the_first_investigator_it_asks() {
    let tal = with_active(seen_by(TAL, agness_discard()), ROLAND);
    assert_eq!(opens(&tal, OpenTab::default()), Some(AGNES));

    // Roland, Agnes and Daisy are asked: Roland sits first of Tal's.
    let tal = with_active(seen_by(TAL, three_reactions()), AGNES);
    assert_eq!(opens(&tal, OpenTab::default()), Some(ROLAND));
}

#[test]
fn a_viewer_follows_the_active_investigator_until_they_click_a_tab() {
    let daisys_turn = with_active(seen_by(ALEX, daisys_horror()), DAISY);
    assert_eq!(opens(&daisys_turn, OpenTab::default()), Some(DAISY));

    let followed = settled_on(DAISY);
    let agness_turn = with_active(seen_by(ALEX, daisys_horror()), AGNES);
    assert_eq!(opens(&agness_turn, followed), Some(AGNES));

    let clicked = OpenTab::clicked(WENDY);
    assert_eq!(opens(&agness_turn, clicked), Some(WENDY));
    let rolands_turn = with_active(seen_by(ALEX, daisys_horror()), ROLAND);
    assert_eq!(opens(&rolands_turn, clicked), Some(WENDY));
}

#[test]
fn a_viewer_with_no_one_taking_a_turn_stays_put_or_opens_the_first_seat() {
    let no_turn = seen_by(ALEX, daisys_horror());
    assert_eq!(opens(&no_turn, OpenTab::default()), Some(ROLAND));

    assert_eq!(opens(&no_turn, settled_on(DAISY)), Some(DAISY));
}

#[test]
fn an_account_clicking_a_tab_opens_it() {
    let tal = with_active(seen_by(TAL, daisys_horror()), AGNES);
    assert_eq!(opens(&tal, OpenTab::clicked(DAISY)), Some(DAISY));
}

#[test]
fn sam_gets_a_strip_for_daisys_prompt() {
    let placed = layout_of(&seen_by(SAM, daisys_horror()));
    assert_eq!(placed.strips(), vec![DAISY]);
    assert_eq!(placed.spinners(), vec![]);
    assert_eq!(placed.above_board, None);
}

#[test]
fn tal_sees_a_spinner_on_daisy_and_what_sam_is_deciding() {
    let placed = layout_of(&seen_by(TAL, daisys_horror()));
    assert_eq!(placed.strips(), vec![]);
    assert_eq!(placed.spinners(), vec![DAISY]);
    assert_eq!(
        placed.above_board,
        Some(AboveBoard::WaitingStrip(
            "Waiting for Sam: Daisy Walker: Assign the horror".to_owned()
        ))
    );
}

#[test]
fn a_viewer_sees_spinners_and_the_waiting_strip_but_never_a_strip() {
    let window = InputRequest::pick_single(
        "You may trigger",
        vec![
            ChoiceOption::new(OptionId(0), "Roland's reaction").offered_to(ROLAND),
            ChoiceOption::new(OptionId(1), "Daisy's reaction").offered_to(DAISY),
        ],
    )
    .skippable()
    .answered_by(Responder::Several);

    let alone = layout_of(&seen_by(ALEX, daisys_horror()));
    assert_eq!(alone.strips(), vec![]);
    assert_eq!(alone.spinners(), vec![DAISY]);
    assert_eq!(
        alone.above_board,
        Some(AboveBoard::WaitingStrip(
            "Waiting for Sam: Daisy Walker: Assign the horror".to_owned()
        ))
    );

    let several = layout_of(&seen_by(ALEX, window));
    assert_eq!(several.strips(), vec![]);
    assert_eq!(several.spinners(), vec![ROLAND, DAISY]);
}

/// Swarm of Rats can move two ways, and the lead chooses.
fn rats_move() -> InputRequest {
    InputRequest::pick_single(
        "The lead investigator chooses where Swarm of Rats moves.",
        vec![],
    )
    .answered_by(Responder::Lead(ROLAND))
}

#[test]
fn tal_gets_the_lead_strip_and_roland_no_mark() {
    let placed = layout_of(&seen_by(TAL, rats_move()));
    assert_eq!(placed.investigators, vec![]);
    assert_eq!(placed.above_board, Some(AboveBoard::LeadStrip(ROLAND)));
}

#[test]
fn everyone_else_waits_for_the_lead_by_name_and_account() {
    let waiting = Some(AboveBoard::LeadWaitingLine(
        "Waiting for the lead investigator, Roland Banks (Tal)".to_owned(),
    ));
    for you in [SAM, ALEX] {
        let placed = layout_of(&seen_by(you, rats_move()));
        assert_eq!(placed.investigators, vec![]);
        assert_eq!(placed.above_board, waiting);
    }
}

/// A window offering Roland, Agnes and Daisy a reaction each.
fn three_reactions() -> InputRequest {
    InputRequest::pick_single(
        "You may trigger",
        vec![
            ChoiceOption::new(OptionId(0), "Roland's reaction").offered_to(ROLAND),
            ChoiceOption::new(OptionId(1), "Agnes's reaction").offered_to(AGNES),
            ChoiceOption::new(OptionId(2), "Daisy's reaction").offered_to(DAISY),
        ],
    )
    .skippable()
    .answered_by(Responder::Several)
}

#[test]
fn a_window_several_answer_splits_into_strips_and_spinners_per_account() {
    let tal = layout_of(&seen_by(TAL, three_reactions()));
    assert_eq!(tal.strips(), vec![ROLAND, AGNES]);
    assert_eq!(tal.spinners(), vec![DAISY]);
    assert_eq!(tal.above_board, None);

    let sam = layout_of(&seen_by(SAM, three_reactions()));
    assert_eq!(sam.strips(), vec![DAISY]);
    assert_eq!(sam.spinners(), vec![ROLAND, AGNES]);
    assert_eq!(sam.above_board, None);
}

/// The waiting strip text `you` see for `request` on `open`'s tab, which
/// the request doesn't ask.
fn waiting_for(you: AccountId, request: InputRequest, open: InvestigatorId) -> String {
    let state = seen_by(you, request);
    match layout(&state, clicked_after(&state, open)).above_board {
        Some(AboveBoard::WaitingStrip(text)) => text,
        other => panic!("expected a waiting strip, got {other:?}"),
    }
}

/// A window offering Wendy and then Daisy a reaction. Wendy's id is the
/// lower, but Daisy sits first.
fn daisy_and_wendy() -> InputRequest {
    InputRequest::pick_single(
        "You may trigger",
        vec![
            ChoiceOption::new(OptionId(0), "Wendy's reaction").offered_to(WENDY),
            ChoiceOption::new(OptionId(1), "Daisy's reaction").offered_to(DAISY),
        ],
    )
    .skippable()
    .answered_by(Responder::Several)
}

#[test]
fn the_waiting_text_gives_one_clause_per_account_in_seating_order() {
    assert_eq!(
        waiting_for(ALEX, daisy_and_wendy(), ROLAND),
        "Waiting for Sam: Daisy Walker · Jo: Wendy Adams: You may trigger"
    );
    assert_eq!(
        waiting_for(ALEX, three_reactions(), WENDY),
        "Waiting for Tal: Roland Banks, Agnes Baker · Sam: Daisy Walker: You may trigger"
    );
}

#[test]
fn the_open_turn_waits_without_its_prompt_text() {
    let open_turn = InputRequest::pick_single("Choose an action", vec![])
        .at(OptionTarget::TurnControl(DAISY))
        .answered_by(Responder::Investigator(DAISY));
    assert_eq!(
        waiting_for(TAL, open_turn, ROLAND),
        "Waiting for Sam: Daisy Walker"
    );
}

/// A search of Roland's top 2, which Tal answers in the card modal.
fn rolands_search() -> InputRequest {
    let card = |id, index| {
        ChoiceOption::new(OptionId(id), "a card").at(OptionTarget::DeckCard {
            investigator: ROLAND,
            index: DeckIndex(index),
        })
    };
    InputRequest::pick_single(
        "Search: choose a card to take",
        vec![card(0, 0), card(1, 1)],
    )
    .at(OptionTarget::SearchedTopOfDeck {
        investigator: ROLAND,
        count: 2,
    })
    .answered_by(Responder::Investigator(ROLAND))
}

/// The card modal shows the prompt itself, so Tal's strip leaves it out, as it
/// does for a decision.
#[test]
fn the_card_modal_keeps_the_prompt_text_off_the_answerers_strip() {
    let search = rolands_search();
    assert!(suppresses_text(&seen_by(TAL, search.clone()), &search));
}

/// A search is answered in the card modal by the account holding the
/// searcher: Tal, for Roland's.
#[test]
fn the_account_answering_a_search_gets_the_card_modal() {
    let placed = layout_of(&seen_by(TAL, rolands_search()));
    assert_eq!(placed.card_choice, Some(CardChoice::Modal));
    assert_eq!(placed.choice_row_on(ROLAND), None);
}

/// `glossary/Search.md`: *"that player is permitted to look at all of the
/// cards in the searched area without revealing those cards to the other
/// players."* Every other account, viewers included, gets the searched area
/// as a row on the searcher's tab, and no modal.
#[test]
fn every_other_account_gets_the_searched_area_as_a_row_on_the_searchers_tab() {
    let area = CardArea::SearchedTopOfDeck {
        investigator: ROLAND,
        count: 2,
    };
    for you in [SAM, ALEX] {
        let placed = layout_of(&seen_by(you, rolands_search()));
        assert_eq!(
            placed.card_choice,
            Some(CardChoice::Row { tab: ROLAND, area }),
            "{you:?}",
        );
        assert_eq!(placed.choice_row_on(ROLAND), Some(area), "{you:?}");
        assert_eq!(placed.choice_row_on(DAISY), None, "{you:?}");
    }
}

/// A prompt that is no card choice places neither the modal nor a row.
#[test]
fn a_prompt_choosing_no_card_places_no_card_choice() {
    for you in [TAL, SAM, ALEX] {
        assert_eq!(layout_of(&seen_by(you, daisys_horror())).card_choice, None);
    }
}

#[test]
fn an_investigator_no_account_holds_is_waited_for_as_someone() {
    let mut state = seen_by(ALEX, daisys_horror());
    state.assigned = Assignment::from_iter([(ROLAND, TAL), (AGNES, TAL), (WENDY, JO)]);
    let placed = layout_of(&state);
    assert_eq!(placed.spinners(), vec![DAISY]);
    assert_eq!(
        placed.above_board,
        Some(AboveBoard::WaitingStrip(
            "Waiting for someone: Daisy Walker: Assign the horror".to_owned()
        ))
    );
}

#[test]
fn with_no_seating_the_clauses_follow_investigator_order() {
    let mut state = seen_by(ALEX, daisy_and_wendy());
    if let Some(game) = &mut state.game {
        game.seating.clear();
    }
    assert_eq!(
        layout_of(&state).above_board,
        Some(AboveBoard::WaitingStrip(
            "Waiting for Jo: Wendy Adams · Sam: Daisy Walker: You may trigger".to_owned()
        ))
    );
}

#[test]
fn with_no_prompt_live_nothing_is_placed() {
    for outcome in [None, Some(EngineOutcome::Done)] {
        let state = ClientState {
            outcome,
            ..seen_by(TAL, daisys_horror())
        };
        let placed = layout_of(&state);
        assert_eq!(placed.investigators, vec![]);
        assert_eq!(placed.above_board, None);
    }
}

#[test]
fn a_reassignment_moves_agness_strip_from_tal_to_sam() {
    let mut tal = seen_by(TAL, agness_discard());
    let mut sam = seen_by(SAM, agness_discard());
    assert_eq!(layout_of(&tal).strips(), vec![AGNES]);
    assert_eq!(layout_of(&sam).spinners(), vec![AGNES]);

    store::reduce(&mut tal, gave_agnes_to_sam());
    store::reduce(&mut sam, gave_agnes_to_sam());

    let tal = layout_of(&tal);
    assert_eq!(tal.spinners(), vec![AGNES]);
    assert_eq!(
        tal.above_board,
        Some(AboveBoard::WaitingStrip(
            "Waiting for Sam: Agnes Baker: Choose a card to discard".to_owned()
        ))
    );
    let sam = layout_of(&sam);
    assert_eq!(sam.strips(), vec![AGNES]);
    assert_eq!(sam.above_board, None);
}

/// Agnes's hand-size discard, which Tal answers.
fn agness_discard() -> InputRequest {
    InputRequest::pick_single("Choose a card to discard", vec![])
        .answered_by(Responder::Investigator(AGNES))
}

/// `investigator`'s tab, open with the live prompt of `state` already
/// seen, as after a click once it arrived.
fn clicked_after(state: &ClientState, investigator: InvestigatorId) -> OpenTab {
    OpenTab {
        prompt: Some(state.prompt),
        ..OpenTab::clicked(investigator)
    }
}

/// `investigator`'s tab, open from before the live prompt of `state`
/// arrived: the prompt is new.
fn open_before(state: &ClientState, investigator: InvestigatorId) -> OpenTab {
    OpenTab {
        prompt: Some(LogPosition(state.prompt.0 - 1)),
        ..settled_on(investigator)
    }
}

/// The tabs `placed` marks "needs you".
fn needing_you(placed: &PromptLayout) -> Vec<InvestigatorId> {
    placed
        .tabs
        .iter()
        .filter(|tab| placed.needs_you(tab.investigator))
        .map(|tab| tab.investigator)
        .collect()
}

#[test]
fn agness_tab_needs_tal_while_he_reads_rolands() {
    let tal = seen_by(TAL, agness_discard());
    let placed = layout(&tal, clicked_after(&tal, ROLAND));
    assert_eq!(placed.open_investigator, Some(ROLAND));
    assert_eq!(needing_you(&placed), vec![AGNES]);
    assert_eq!(placed.dock_strip, None, "Roland isn't asked");

    let placed = layout(&tal, clicked_after(&tal, AGNES));
    assert_eq!(needing_you(&placed), vec![], "her strip is in the dock");
    assert_eq!(placed.dock_strip, Some(AGNES));
}

/// A window offering Roland and Agnes a reaction each.
fn roland_and_agnes() -> InputRequest {
    InputRequest::pick_single(
        "You may trigger",
        vec![
            ChoiceOption::new(OptionId(0), "Agnes's reaction").offered_to(AGNES),
            ChoiceOption::new(OptionId(1), "Roland's reaction").offered_to(ROLAND),
        ],
    )
    .skippable()
    .answered_by(Responder::Several)
}

#[test]
fn a_window_asking_roland_and_agnes_keeps_rolands_tab_and_leaves_daisys() {
    let tal = seen_by(TAL, roland_and_agnes());

    let from_roland = layout(&tal, open_before(&tal, ROLAND));
    assert_eq!(from_roland.open_investigator, Some(ROLAND));
    assert_eq!(from_roland.dock_strip, Some(ROLAND));
    assert_eq!(needing_you(&from_roland), vec![AGNES]);

    let from_daisy = layout(&tal, open_before(&tal, DAISY));
    assert_eq!(
        from_daisy.open_investigator,
        Some(ROLAND),
        "Roland sits first of Tal's"
    );
    assert_eq!(from_daisy.dock_strip, Some(ROLAND));
    assert_eq!(needing_you(&from_daisy), vec![AGNES]);
}

#[test]
fn a_new_prompt_for_agnes_opens_her_tab_even_from_one_clicked() {
    let tal = seen_by(TAL, agness_discard());
    let clicked_before = OpenTab {
        prompt: Some(LogPosition(tal.prompt.0 - 1)),
        ..OpenTab::clicked(DAISY)
    };
    assert_eq!(opens(&tal, clicked_before), Some(AGNES));
}

#[test]
fn a_tab_clicked_while_the_prompt_waits_stays_open() {
    let tal = seen_by(TAL, agness_discard());
    let placed = layout(&tal, clicked_after(&tal, DAISY));
    assert_eq!(placed.open_investigator, Some(DAISY));
    assert_eq!(needing_you(&placed), vec![AGNES]);
}

#[test]
fn a_new_prompt_not_yours_leaves_the_tab_where_it_is() {
    let tal = with_active(seen_by(TAL, daisys_horror()), ROLAND);
    assert_eq!(opens(&tal, open_before(&tal, AGNES)), Some(AGNES));
}

#[test]
fn tal_on_daisys_tab_sees_no_waiting_strip_and_on_any_other_tab_sees_it() {
    let tal = seen_by(TAL, daisys_horror());
    assert_eq!(
        layout(&tal, clicked_after(&tal, DAISY)).above_board,
        None,
        "the spinner on the open tab already says it"
    );
    let waiting = Some(AboveBoard::WaitingStrip(
        "Waiting for Sam: Daisy Walker: Assign the horror".to_owned(),
    ));
    for open in [ROLAND, AGNES, WENDY] {
        assert_eq!(layout(&tal, clicked_after(&tal, open)).above_board, waiting);
    }
}

#[test]
fn a_lead_decision_stays_above_the_tabs_whichever_is_open() {
    let tal = seen_by(TAL, rats_move());
    for open in [ROLAND, DAISY] {
        let placed = layout(&tal, open_before(&tal, open));
        assert_eq!(
            placed.open_investigator,
            Some(open),
            "a lead decision switches nothing"
        );
        assert_eq!(placed.above_board, Some(AboveBoard::LeadStrip(ROLAND)));
        assert_eq!(needing_you(&placed), vec![]);
        assert_eq!(placed.dock_strip, None);
    }
}

#[test]
fn a_reassignment_of_agnes_to_sam_moves_needs_you_and_the_strip() {
    let mut tal = seen_by(TAL, agness_discard());
    let mut sam = seen_by(SAM, agness_discard());
    store::reduce(&mut tal, gave_agnes_to_sam());
    store::reduce(&mut sam, gave_agnes_to_sam());

    let tal = layout(&tal, clicked_after(&tal, ROLAND));
    assert_eq!(needing_you(&tal), vec![]);

    let sams_open_tab = clicked_after(&sam, DAISY);
    let on_daisy = layout(&sam, sams_open_tab);
    assert_eq!(
        on_daisy.open_investigator,
        Some(DAISY),
        "a reassignment switches nothing"
    );
    assert_eq!(needing_you(&on_daisy), vec![AGNES]);
    let on_agnes = layout(&sam, clicked_after(&sam, AGNES));
    assert_eq!(on_agnes.dock_strip, Some(AGNES));
}
