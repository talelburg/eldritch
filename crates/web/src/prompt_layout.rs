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
use protocol::{AccountId, LogPosition};

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
    /// The investigator whose strip goes in the dock: the open tab's, when the
    /// prompt asks them and this account holds them.
    pub dock_strip: Option<InvestigatorId>,
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
    /// "Needs you": the prompt asks them, this account holds them, and their
    /// tab isn't the open one.
    pub needs_you: bool,
}

/// Which tab this client has open: UI state, kept outside the
/// [`ClientState`] and passed to [`layout`].
///
/// The board settles it when it first has a game, and a click sets it. Until a
/// click, a viewer's follows the active investigator. A new prompt for one of
/// this account's investigators opens the first it asks, unless the open tab
/// is already one it asks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct OpenTab {
    /// The investigator whose tab is open, once one is settled.
    pub investigator: Option<InvestigatorId>,
    /// Whether this account picked it with a click.
    pub clicked: bool,
    /// The [`ClientState::prompt`] this tab was last settled on, so a prompt
    /// at any other position is new. `None` before the board has a game.
    pub prompt: Option<LogPosition>,
}

impl OpenTab {
    /// The tab of `investigator`, picked with a click, with no prompt seen.
    #[must_use]
    pub fn clicked(investigator: InvestigatorId) -> Self {
        Self {
            investigator: Some(investigator),
            clicked: true,
            prompt: None,
        }
    }

    /// What this client settles on next for `state`: [`layout`]'s open tab,
    /// with the prompt it was settled on. A click stands until a new prompt
    /// moves the tab.
    #[must_use]
    pub fn settle(self, state: &ClientState) -> Self {
        let investigator = layout(state, self).open;
        Self {
            investigator,
            clicked: self.clicked && investigator == self.investigator,
            prompt: state.game.as_ref().map(|_| state.prompt),
        }
    }
}

/// The open tab, as the board provides it to whatever opens a tab: a tab
/// itself, and an investigator's token on the map.
#[derive(Debug, Clone, Copy)]
pub struct OpenTabSignal(pub RwSignal<OpenTab>);

impl OpenTabSignal {
    /// Open `investigator`'s tab, as a click does. The prompt it was settled
    /// on stays, so the live prompt doesn't count as new.
    pub fn click(self, investigator: InvestigatorId) {
        self.0.update(|open| {
            *open = OpenTab {
                prompt: open.prompt,
                ..OpenTab::clicked(investigator)
            };
        });
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
    /// A strip with their controls, in the dock while their tab is open: they
    /// are yours.
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
    let open_tab = use_context::<OpenTabSignal>();
    let above_board = Memo::new(move |_| {
        let open = open_tab.map(|open| open.0.get()).unwrap_or_default();
        store.with(|s| layout(s, open).above_board)
    });
    move || match above_board.get() {
        Some(AboveBoard::WaitingStrip(text)) => {
            Some(view! { <div class="waiting-strip" role="status">{text}</div> })
        }
        Some(AboveBoard::LeadWaitingLine(text)) => {
            Some(view! { <div class="lead-waiting" role="status">{text}</div> })
        }
        // The lead's strip has controls, so `prompt_banner::LeadStrip`
        // renders it.
        Some(AboveBoard::LeadStrip(_)) | None => None,
    }
}

/// Where the live prompt goes for this client's account, and the tabs over the
/// dock with `open` as what this client has settled so far. Pure.
///
/// An investigator the prompt asks gets a [`Strip`](Placement::Strip) if this account holds them
/// and a [`Spinner`](Placement::Spinner) if not. The strip goes in the dock
/// when their tab is open, and their tab says "needs you" when it isn't. Above
/// the tabs goes the lead's strip, the lead-waiting line, or the waiting
/// strip, which shows only when the prompt asks none of your investigators
/// and the open tab isn't one it asks.
///
/// The open tab is the first of yours a new prompt asks, in seating order,
/// unless the open tab is already one of yours it asks. Otherwise it is the
/// one clicked. Until a click, a viewer's follows the active investigator,
/// staying put while no one takes a turn. A player's is the one settled, and
/// before that, at load, their active investigator, else their first in
/// seating order. A prompt is new when it sits at a position other than
/// [`OpenTab::prompt`].
#[must_use]
pub fn layout(state: &ClientState, open: OpenTab) -> PromptLayout {
    let mut placed = placements(state);
    placed.tabs = tabs(state, &placed);
    let opened = open_tab(state, &placed, open);
    placed.open = opened;
    let asks_open = |tab: &Tab| Some(tab.investigator) == opened && tab.asked.is_some();
    // The open tab's spinner already says what the waiting strip would.
    if placed.tabs.iter().any(asks_open)
        && matches!(placed.above_board, Some(AboveBoard::WaitingStrip(_)))
    {
        placed.above_board = None;
    }
    for tab in &mut placed.tabs {
        let yours = tab.asked == Some(Placement::Strip);
        if yours && Some(tab.investigator) == opened {
            placed.dock_strip = opened;
        }
        tab.needs_you = yours && Some(tab.investigator) != opened;
    }
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
            needs_you: false,
        })
        .collect()
}

/// The open tab: see [`layout`].
fn open_tab(state: &ClientState, placed: &PromptLayout, open: OpenTab) -> Option<InvestigatorId> {
    let tabs = &placed.tabs;
    let has_tab = |id: &InvestigatorId| tabs.iter().any(|tab| tab.investigator == *id);
    let settled = open.investigator.filter(has_tab);
    // A new prompt for one of yours opens the first it asks, in seating
    // order, unless the open tab is one it asks.
    let new_prompt = open.prompt != Some(state.prompt);
    let first_yours_asked = tabs
        .iter()
        .find(|tab| tab.asked == Some(Placement::Strip))
        .map(|tab| tab.investigator);
    // A tab another account answers for isn't one to answer from, so it
    // doesn't hold you.
    let open_is_asked = tabs
        .iter()
        .any(|tab| Some(tab.investigator) == settled && tab.asked == Some(Placement::Strip));
    if new_prompt && first_yours_asked.is_some() && !open_is_asked {
        return first_yours_asked;
    }
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
    settled
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
            ..OpenTab::default()
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
            ..OpenTab::default()
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
            ..OpenTab::default()
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
            investigator: Some(investigator),
            clicked: false,
            prompt: Some(LogPosition(state.prompt.0 - 1)),
        }
    }

    /// The tabs `placed` marks "needs you".
    fn needing_you(placed: &PromptLayout) -> Vec<InvestigatorId> {
        placed
            .tabs
            .iter()
            .filter(|tab| tab.needs_you)
            .map(|tab| tab.investigator)
            .collect()
    }

    #[test]
    fn agness_tab_needs_tal_while_he_reads_rolands() {
        let tal = seen_by(TAL, agness_discard());
        let placed = layout(&tal, clicked_after(&tal, ROLAND));
        assert_eq!(placed.open, Some(ROLAND));
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
        assert_eq!(from_roland.open, Some(ROLAND));
        assert_eq!(from_roland.dock_strip, Some(ROLAND));
        assert_eq!(needing_you(&from_roland), vec![AGNES]);

        let from_daisy = layout(&tal, open_before(&tal, DAISY));
        assert_eq!(from_daisy.open, Some(ROLAND), "Roland sits first of Tal's");
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
        assert_eq!(placed.open, Some(DAISY));
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
            assert_eq!(placed.open, Some(open), "a lead decision switches nothing");
            assert_eq!(placed.above_board, Some(AboveBoard::LeadStrip(ROLAND)));
            assert_eq!(needing_you(&placed), vec![]);
            assert_eq!(placed.dock_strip, None);
        }
    }

    #[test]
    fn a_reassignment_of_agnes_to_sam_moves_needs_you_and_the_strip() {
        let mut tal = seen_by(TAL, agness_discard());
        let mut sam = seen_by(SAM, agness_discard());
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

        let tal = layout(&tal, clicked_after(&tal, ROLAND));
        assert_eq!(needing_you(&tal), vec![]);

        let sams_open_tab = clicked_after(&sam, DAISY);
        let on_daisy = layout(&sam, sams_open_tab);
        assert_eq!(
            on_daisy.open,
            Some(DAISY),
            "a reassignment switches nothing"
        );
        assert_eq!(needing_you(&on_daisy), vec![AGNES]);
        let on_agnes = layout(&sam, clicked_after(&sam, AGNES));
        assert_eq!(on_agnes.dock_strip, Some(AGNES));
    }
}
