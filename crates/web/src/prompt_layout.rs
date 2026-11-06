//! Where the live prompt goes, for this client's account.
//!
//! An investigator of yours gets a **strip** with controls, the tab of one
//! another account decides for shows a **spinner**, and a line may go above
//! the board. [`layout`] is the one place that settles it, pure over the
//! [`ClientState`] and the open tab, so a reassignment moves the placement on
//! the next read. It also lays out the tabs over the dock: one per
//! investigator, in seating order, with its marks, and which is open.

use game_core::engine::{self, EngineOutcome, InputRequest, OptionTarget, Responder};
use game_core::state::investigator::InvestigatorId;
use leptos::prelude::*;
use protocol::AccountId;

use crate::decision;
use crate::store::{self, ClientState};

/// Every placement the live prompt gets on this client.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PromptLayout {
    /// Each investigator the prompt asks, with where their part of it goes.
    pub investigators: Vec<(InvestigatorId, Placement)>,
    /// The one line above the board, if the prompt gets one.
    pub above_board: Option<AboveBoard>,
    /// One tab per investigator, in seating order, with its marks.
    pub tabs: Vec<Tab>,
    /// The investigator whose tab is open, and so is docked. `None` with no
    /// game.
    pub open: Option<InvestigatorId>,
}

/// One investigator's tab over the dock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tab {
    /// Whose tab it is.
    pub investigator: InvestigatorId,
    /// This account holds them, so the tab is tinted.
    pub held: bool,
    /// They take the turn now: ▶.
    pub active: bool,
    /// They lead now (`engine::the_lead`): ★.
    pub lead: bool,
    /// Where the live prompt puts their part, when it asks them: a
    /// [`Spinner`](Placement::Spinner) when another account answers for them.
    pub asked: Option<Placement>,
}

/// Which tab this client has open: UI state, kept outside the
/// [`ClientState`] and passed to [`layout`].
///
/// The board settles it when it first has a game, and a click sets it. Until a
/// click, a viewer's follows the active investigator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct OpenTab {
    /// The investigator whose tab is open, once one is settled.
    pub investigator: Option<InvestigatorId>,
    /// Whether this account picked it with a click.
    pub clicked: bool,
}

impl OpenTab {
    /// The tab of `investigator`, picked with a click.
    #[must_use]
    pub fn clicked(investigator: InvestigatorId) -> Self {
        Self {
            investigator: Some(investigator),
            clicked: true,
        }
    }
}

/// The open tab, as the board provides it to whatever opens a tab: a tab
/// itself, and an investigator's token on the map.
#[derive(Debug, Clone, Copy)]
pub struct OpenTabSignal(pub RwSignal<OpenTab>);

impl OpenTabSignal {
    /// Open `investigator`'s tab, as a click does.
    pub fn click(self, investigator: InvestigatorId) {
        self.0.set(OpenTab::clicked(investigator));
    }
}

/// The open tab from context, or none settled when mounted without the
/// board's contexts.
#[must_use]
pub fn use_open_tab() -> OpenTab {
    use_context::<OpenTabSignal>()
        .map(|open| open.0.get())
        .unwrap_or_default()
}

/// Where one investigator's part of the prompt goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    /// A strip with their controls, in the prompt bar: they are yours.
    Strip,
    /// A spinner on their tab: another account answers for them.
    Spinner,
}

/// What the prompt puts above the board.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AboveBoard {
    /// The lead investigator's strip, for the account holding them, when the
    /// lead decides for the table.
    LeadStrip(InvestigatorId),
    /// The lead-waiting line: who leads and which account holds them, for
    /// everyone else while the lead decides: "Waiting for the lead
    /// investigator, Roland Banks (Tal)".
    LeadWaitingLine(String),
    /// The waiting strip: who the game waits on, and on what, when the prompt
    /// asks none of your investigators: "Waiting for Sam: Daisy Walker: …". It
    /// has no controls.
    WaitingStrip(String),
}

impl PromptLayout {
    /// The investigators who get a strip, in the order the prompt asks them.
    #[must_use]
    pub fn strips(&self) -> Vec<InvestigatorId> {
        self.placed(Placement::Strip)
    }

    /// The investigators whose tab shows a spinner.
    #[must_use]
    pub fn spinners(&self) -> Vec<InvestigatorId> {
        self.placed(Placement::Spinner)
    }

    fn placed(&self, placement: Placement) -> Vec<InvestigatorId> {
        self.investigators
            .iter()
            .filter(|(_, placed)| *placed == placement)
            .map(|(id, _)| *id)
            .collect()
    }
}

/// What the live prompt puts above the board, as text with no controls: the
/// waiting strip, or the lead-waiting line.
#[component]
pub fn WaitingAboveBoard() -> impl IntoView {
    let store = store::use_store();
    let above_board = Memo::new(move |_| store.with(|s| layout(s, OpenTab::default()).above_board));
    move || match above_board.get() {
        Some(AboveBoard::WaitingStrip(text)) => {
            Some(view! { <div class="waiting-strip" role="status">{text}</div> })
        }
        Some(AboveBoard::LeadWaitingLine(text)) => {
            Some(view! { <div class="lead-waiting" role="status">{text}</div> })
        }
        // The lead's strip has controls, so the prompt banner's `LeadStrip`
        // renders it.
        Some(AboveBoard::LeadStrip(_)) | None => None,
    }
}

/// Where the live prompt goes for this client's account, and the tabs over the
/// dock with `open` as what this client has settled so far. Pure.
///
/// An investigator the prompt asks gets a [`Strip`](Placement::Strip) if this account holds them
/// and a [`Spinner`](Placement::Spinner) if not, and above the board goes the
/// lead's strip, the lead-waiting line, or the waiting strip, which
/// shows only when the prompt asks none of your investigators.
///
/// The open tab is the one clicked. Until a click, a viewer's follows the
/// active investigator, staying put while no one takes a turn. A player's is
/// the one settled, and before that, at load: the first investigator of
/// theirs the prompt asks, in seating order, else their active investigator,
/// else their first in seating order.
#[must_use]
pub fn layout(state: &ClientState, open: OpenTab) -> PromptLayout {
    let mut placed = placements(state);
    placed.tabs = tabs(state, &placed);
    placed.open = open_tab(state, &placed, open);
    placed
}

/// The tabs: every investigator in seating order, with their marks.
fn tabs(state: &ClientState, placed: &PromptLayout) -> Vec<Tab> {
    let Some(game) = &state.game else {
        return Vec::new();
    };
    let lead = engine::the_lead(game);
    let mut seated: Vec<InvestigatorId> = game.investigators.keys().copied().collect();
    seated.sort_by_key(|id| seat_rank(state, *id));
    seated
        .into_iter()
        .map(|investigator| Tab {
            investigator,
            held: state.holds(investigator),
            active: game.active_investigator == Some(investigator),
            lead: lead == Some(investigator),
            asked: placed
                .investigators
                .iter()
                .find(|(asked, _)| *asked == investigator)
                .map(|(_, placement)| *placement),
        })
        .collect()
}

/// The open tab: see [`layout`].
fn open_tab(state: &ClientState, placed: &PromptLayout, open: OpenTab) -> Option<InvestigatorId> {
    let tabs = &placed.tabs;
    let has_tab = |id: &InvestigatorId| tabs.iter().any(|tab| tab.investigator == *id);
    let settled = open.investigator.filter(has_tab);
    if open.clicked && settled.is_some() {
        return settled;
    }
    let active = state
        .game
        .as_ref()
        .and_then(|game| game.active_investigator)
        .filter(has_tab);
    let first = tabs.first().map(|tab| tab.investigator);
    let yours: Vec<InvestigatorId> = tabs
        .iter()
        .filter(|tab| tab.held)
        .map(|tab| tab.investigator)
        .collect();
    if yours.is_empty() {
        return active.or(settled).or(first);
    }
    let strips = placed.strips();
    settled
        .or_else(|| yours.iter().copied().find(|id| strips.contains(id)))
        .or_else(|| active.filter(|id| yours.contains(id)))
        .or_else(|| yours.first().copied())
}

/// Where the live prompt's parts go, with no tabs yet.
fn placements(state: &ClientState) -> PromptLayout {
    let Some(EngineOutcome::AwaitingInput { request, .. }) = &state.outcome else {
        return PromptLayout::default();
    };
    let asked = match request.responder {
        Responder::Investigator(id) => vec![id],
        Responder::Several => request.holders(),
        // A table-wide decision: no tab is marked.
        Responder::Lead(lead) => return lead_layout(state, lead),
        // `Responder` is non-exhaustive; a kind this client doesn't know is
        // placed nowhere.
        _ => Vec::new(),
    };
    let investigators = asked
        .into_iter()
        .map(|id| {
            let placement = if state.holds(id) {
                Placement::Strip
            } else {
                Placement::Spinner
            };
            (id, placement)
        })
        .collect::<Vec<_>>();
    let none_yours = investigators
        .iter()
        .all(|(_, placement)| *placement == Placement::Spinner);
    let above_board = (none_yours && !investigators.is_empty())
        .then(|| AboveBoard::WaitingStrip(waiting_text(state, request, &investigators)));
    PromptLayout {
        investigators,
        above_board,
        ..PromptLayout::default()
    }
}

/// The placement of a prompt the lead answers, for `lead`, the investigator
/// leading now: their strip for the account holding them, and the lead-waiting
/// line for everyone else.
fn lead_layout(state: &ClientState, lead: InvestigatorId) -> PromptLayout {
    let above_board = if state.holds(lead) {
        AboveBoard::LeadStrip(lead)
    } else {
        AboveBoard::LeadWaitingLine(format!(
            "Waiting for the lead investigator, {} ({})",
            state.investigator_name(lead),
            state.holder_name(state.assigned.holder(lead)),
        ))
    };
    PromptLayout {
        investigators: Vec::new(),
        above_board: Some(above_board),
        ..PromptLayout::default()
    }
}

/// The waiting strip's text: one clause per account holding an investigator
/// the prompt asks, in seating order, then the prompt's text unless it is
/// suppressed.
fn waiting_text(
    state: &ClientState,
    request: &InputRequest,
    asked: &[(InvestigatorId, Placement)],
) -> String {
    let mut asked: Vec<InvestigatorId> = asked.iter().map(|(id, _)| *id).collect();
    asked.sort_by_key(|id| seat_rank(state, *id));
    let mut clauses: Vec<(Option<AccountId>, Vec<String>)> = Vec::new();
    for id in asked {
        let holder = state.assigned.holder(id);
        let name = state.investigator_name(id);
        match clauses.iter_mut().find(|(account, _)| *account == holder) {
            Some((_, names)) => names.push(name),
            None => clauses.push((holder, vec![name])),
        }
    }
    let clauses: Vec<String> = clauses
        .into_iter()
        .map(|(holder, names)| format!("{}: {}", state.holder_name(holder), names.join(", ")))
        .collect();
    let mut text = format!("Waiting for {}", clauses.join(" · "));
    if !suppresses_text(state, request) {
        text.push_str(": ");
        text.push_str(&request.prompt);
    }
    text
}

/// Where `id` sits: their place in the seating, then their id, so a game
/// with no seating falls back to investigator order.
fn seat_rank(state: &ClientState, id: InvestigatorId) -> (usize, InvestigatorId) {
    let place = state
        .game
        .as_ref()
        .and_then(|game| game.seating.iter().position(|seated| *seated == id))
        .unwrap_or(usize::MAX);
    (place, id)
}

/// Whether the prompt's text stays off a strip: on the open turn, which the
/// turn control on the board already offers (ADR 0011), and while a decision
/// modal is up, which shows the text itself.
pub(crate) fn suppresses_text(state: &ClientState, request: &InputRequest) -> bool {
    matches!(request.target, Some(OptionTarget::TurnControl(_))) || decision::modal_is_live(state)
}

#[cfg(test)]
mod tests {
    use game_core::engine::{ChoiceOption, InputRequest, OptionId};
    use game_core::state::{GameState, GameStateBuilder};
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

    /// The tab `state` opens when `open` is what this client settled so far.
    fn opens(state: &ClientState, open: OpenTab) -> Option<InvestigatorId> {
        layout(state, open).open
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
    fn a_player_opens_on_their_active_investigator_else_their_first_in_seating() {
        // Daisy's horror is Sam's to answer, so no prompt is Tal's.
        let agnes_turn = with_active(seen_by(TAL, daisys_horror()), AGNES);
        assert_eq!(opens(&agnes_turn, OpenTab::default()), Some(AGNES));

        let daisys_turn = with_active(seen_by(TAL, daisys_horror()), DAISY);
        assert_eq!(opens(&daisys_turn, OpenTab::default()), Some(ROLAND));

        // Once settled, a player's tab stays put as the turn moves on.
        let rolands_turn = with_active(seen_by(TAL, daisys_horror()), ROLAND);
        let settled = OpenTab {
            investigator: Some(AGNES),
            clicked: false,
        };
        assert_eq!(opens(&rolands_turn, settled), Some(AGNES));
    }

    #[test]
    fn a_player_with_a_prompt_theirs_at_load_opens_the_first_investigator_it_asks() {
        let agness_discard = InputRequest::pick_single("Choose a card to discard", vec![])
            .answered_by(Responder::Investigator(AGNES));
        let tal = with_active(seen_by(TAL, agness_discard), ROLAND);
        assert_eq!(opens(&tal, OpenTab::default()), Some(AGNES));

        // Roland, Agnes and Daisy are asked: Roland sits first of Tal's.
        let tal = with_active(seen_by(TAL, three_reactions()), AGNES);
        assert_eq!(opens(&tal, OpenTab::default()), Some(ROLAND));
    }

    #[test]
    fn a_viewer_follows_the_active_investigator_until_they_click_a_tab() {
        let daisys_turn = with_active(seen_by(ALEX, daisys_horror()), DAISY);
        assert_eq!(opens(&daisys_turn, OpenTab::default()), Some(DAISY));

        let followed = OpenTab {
            investigator: Some(DAISY),
            clicked: false,
        };
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

        let followed = OpenTab {
            investigator: Some(DAISY),
            clicked: false,
        };
        assert_eq!(opens(&no_turn, followed), Some(DAISY));
    }

    #[test]
    fn a_player_clicking_a_tab_opens_it() {
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

    /// The waiting strip text `you` sees for `request`.
    fn waiting_for(you: AccountId, request: InputRequest) -> String {
        match layout_of(&seen_by(you, request)).above_board {
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
            waiting_for(ALEX, daisy_and_wendy()),
            "Waiting for Sam: Daisy Walker · Jo: Wendy Adams: You may trigger"
        );
        assert_eq!(
            waiting_for(ALEX, three_reactions()),
            "Waiting for Tal: Roland Banks, Agnes Baker · Sam: Daisy Walker: You may trigger"
        );
    }

    #[test]
    fn the_open_turn_waits_without_its_prompt_text() {
        let open_turn = InputRequest::pick_single("Choose an action", vec![])
            .at(OptionTarget::TurnControl(DAISY))
            .answered_by(Responder::Investigator(DAISY));
        assert_eq!(waiting_for(TAL, open_turn), "Waiting for Sam: Daisy Walker");
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
        let discard = InputRequest::pick_single("Choose a card to discard", vec![])
            .answered_by(Responder::Investigator(AGNES));
        let mut tal = seen_by(TAL, discard.clone());
        let mut sam = seen_by(SAM, discard);
        assert_eq!(layout_of(&tal).strips(), vec![AGNES]);
        assert_eq!(layout_of(&sam).spinners(), vec![AGNES]);

        let gave_agnes_to_sam = ServerMessage::Assigned {
            by: TAL,
            investigator: AGNES,
            assigned: Assignment::from_iter([
                (ROLAND, TAL),
                (AGNES, SAM),
                (DAISY, SAM),
                (WENDY, JO),
            ]),
        };
        store::reduce(&mut tal, gave_agnes_to_sam.clone());
        store::reduce(&mut sam, gave_agnes_to_sam);

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
}
