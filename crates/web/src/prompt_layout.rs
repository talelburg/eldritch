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

use crate::store::{self, ClientState};
use crate::{card_modal, decision};

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
    pub open_investigator: Option<InvestigatorId>,
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
    /// Whose tab is open, and how it came to be.
    pub tab: TabPick,
    /// The [`ClientState::prompt`] this tab was last settled on, so a prompt
    /// at any other position is new. `None` before the board has a game.
    pub prompt: Option<LogPosition>,
}

/// How the open tab was picked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TabPick {
    /// No tab is settled yet.
    #[default]
    Unsettled,
    /// [`layout`] settled on this investigator's tab, which a viewer's moves
    /// off as the turn does.
    Settled(InvestigatorId),
    /// This account clicked this investigator's tab.
    Clicked(InvestigatorId),
}

impl OpenTab {
    /// The tab of `investigator`, picked with a click, with no prompt seen.
    #[must_use]
    pub fn clicked(investigator: InvestigatorId) -> Self {
        Self {
            tab: TabPick::Clicked(investigator),
            prompt: None,
        }
    }

    /// The investigator whose tab is open, once one is settled.
    #[must_use]
    pub fn investigator(self) -> Option<InvestigatorId> {
        match self.tab {
            TabPick::Unsettled => None,
            TabPick::Settled(id) | TabPick::Clicked(id) => Some(id),
        }
    }

    /// Whether this account picked the open tab with a click.
    #[must_use]
    pub fn is_clicked(self) -> bool {
        matches!(self.tab, TabPick::Clicked(_))
    }

    /// What this client settles on next for `state`: [`layout`]'s open tab,
    /// with the prompt it was settled on. A click stands until a new prompt
    /// moves the tab.
    #[must_use]
    pub fn settle(self, state: &ClientState) -> Self {
        let tab = match layout(state, self).open_investigator {
            None => TabPick::Unsettled,
            Some(id) if self.tab == TabPick::Clicked(id) => TabPick::Clicked(id),
            Some(id) => TabPick::Settled(id),
        };
        Self {
            tab,
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
        self.0.update(|open_tab| {
            *open_tab = OpenTab {
                prompt: open_tab.prompt,
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
        .map(|signal| signal.0.get())
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

    /// Whether `investigator`'s tab says "needs you": the prompt asks them,
    /// this account holds them, and their tab isn't the open one.
    #[must_use]
    pub fn needs_you(&self, investigator: InvestigatorId) -> bool {
        self.open_investigator != Some(investigator) && self.strips().contains(&investigator)
    }

    /// Whether this account holds `investigator`, whose tab is then tinted
    /// and whose hand shows.
    #[must_use]
    pub fn holds(&self, investigator: InvestigatorId) -> bool {
        self.tabs
            .iter()
            .any(|tab| tab.investigator == investigator && tab.held)
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
    let open_tab_signal = use_context::<OpenTabSignal>();
    let above_board = Memo::new(move |_| {
        let open_tab = open_tab_signal
            .map(|signal| signal.0.get())
            .unwrap_or_default();
        store.with(|s| layout(s, open_tab).above_board)
    });
    move || match above_board.get() {
        Some(AboveBoard::WaitingStrip(text)) => {
            Some(view! { <div class="waiting-strip" role="status">{text}</div> })
        }
        Some(AboveBoard::LeadWaitingLine(text)) => {
            Some(view! { <div class="lead-waiting" role="status">{text}</div> })
        }
        // The lead's strip has controls, so `prompt_strip::LeadStrip`
        // renders it.
        Some(AboveBoard::LeadStrip(_)) | None => None,
    }
}

/// Where the live prompt goes for this client's account, and the tabs over the
/// dock with `open_tab` as what this client has settled so far. Pure.
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
/// staying put while no one takes a turn. An account holding investigators
/// keeps the one settled, and before that, at load, opens its active
/// investigator, else its first in seating order. A prompt is new when it sits
/// at a position other than [`OpenTab::prompt`].
#[must_use]
pub fn layout(state: &ClientState, open_tab: OpenTab) -> PromptLayout {
    let mut placed = placements(state);
    placed.tabs = tabs(state, &placed);
    let opened = next_open(state, &placed, open_tab);
    placed.open_investigator = opened;
    let asks_open = |tab: &Tab| Some(tab.investigator) == opened && tab.asked.is_some();
    // The open tab's spinner already says what the waiting strip would.
    if placed.tabs.iter().any(asks_open)
        && matches!(placed.above_board, Some(AboveBoard::WaitingStrip(_)))
    {
        placed.above_board = None;
    }
    placed.dock_strip = opened.filter(|id| placed.strips().contains(id));
    placed
}

/// The tabs: every investigator in seating order, with their marks.
fn tabs(state: &ClientState, placed: &PromptLayout) -> Vec<Tab> {
    let Some(game) = &state.game else {
        return Vec::new();
    };
    // Once the scenario has ended nobody takes a turn or leads, though the
    // state still names whoever did last.
    let playing = game.ending.is_none();
    let lead = engine::the_lead(game).filter(|_| playing);
    let active = game.active_investigator.filter(|_| playing);
    let mut seated: Vec<InvestigatorId> = game.investigators.keys().copied().collect();
    seated.sort_by_key(|id| seat_rank(state, *id));
    seated
        .into_iter()
        .map(|investigator| Tab {
            investigator,
            held: state.holds(investigator),
            active: active == Some(investigator),
            lead: lead == Some(investigator),
            asked: placed
                .investigators
                .iter()
                .find(|(asked, _)| *asked == investigator)
                .map(|(_, placement)| *placement),
        })
        .collect()
}

/// The investigator whose tab opens next, from `open_tab`: see [`layout`].
fn next_open(
    state: &ClientState,
    placed: &PromptLayout,
    open_tab: OpenTab,
) -> Option<InvestigatorId> {
    let tabs = &placed.tabs;
    let has_tab = |id: &InvestigatorId| tabs.iter().any(|tab| tab.investigator == *id);
    let settled = open_tab.investigator().filter(has_tab);
    // A new prompt for one of yours opens the first it asks, in seating
    // order, unless the open tab is one it asks.
    let new_prompt = open_tab.prompt != Some(state.prompt);
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
    if open_tab.is_clicked() && settled.is_some() {
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
/// modal or the card modal is up, which shows the text itself.
pub(crate) fn suppresses_text(state: &ClientState, request: &InputRequest) -> bool {
    matches!(request.target, Some(OptionTarget::TurnControl(_)))
        || decision::modal_is_live(state)
        || card_modal::modal_is_live(state)
}

#[cfg(test)]
mod tests;
