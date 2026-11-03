//! Reactive client store: `ClientState` + the pure `ServerMessage` reducer.

use game_core::action::{InputResponse, PlayerAction};
use game_core::engine::{EngineOutcome, InputRequest, OptionTarget, Responder};
use game_core::event::Event;
use game_core::state::investigator::InvestigatorId;
use game_core::state::{ChaosToken, Continuation, GameState, TokenResolution};
use leptos::prelude::*;
use protocol::logged_answer::LoggedResponse;
use protocol::{
    AccountId, Applied, Assignment, LogPosition, Refusal, ServerMessage, SignInMethod, SignedIn,
};

use crate::socket_ending::SocketEnding;

/// Connection lifecycle, set by the transport (not by `reduce`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ConnStatus {
    #[default]
    Connecting,
    Connected,
    Reconnecting,
    /// The server closed the socket for a restart. Held through the reconnect
    /// attempts that follow, however they fail, until the next `Hello`.
    Restarting,
    Failed,
    /// No saved game and no roster chosen yet — render the picker.
    AwaitingRoster,
    /// The server's `Hello` named another build, or a server frame failed to
    /// deserialize: the client and server binaries disagree on the wire
    /// format. Terminal: nothing more is sent, and the player reloads.
    VersionMismatch,
    /// Not signed in: render the sign-in screen, offering these methods. The
    /// saved game id is kept, so signing in returns to the same game.
    SignedOut(Vec<SignInMethod>),
    /// The server can't rebuild this game on its current build: an
    /// unresumable game. Terminal: the transport stops retrying and keeps the
    /// saved id, which a rollback to an earlier build can resume.
    Unresumable,
}

impl ConnStatus {
    /// Whether the board and its in-game modals refuse input: at every status
    /// but `Connected`, so a status added later is locked unless it says
    /// otherwise. Between a socket opening and its `Hello` the status isn't
    /// `Connected` yet, so a board still showing pre-drop state stays locked.
    #[must_use]
    pub fn locks_board(&self) -> bool {
        *self != ConnStatus::Connected
    }

    /// Whether the transport is between attempts to get a lost connection
    /// back: the statuses a connect attempt keeps rather than overwriting with
    /// `Connecting`.
    #[must_use]
    pub fn is_retrying(&self) -> bool {
        matches!(self, ConnStatus::Reconnecting | ConnStatus::Restarting)
    }

    /// The banner that says why the board is locked, at the statuses that have
    /// one. The picker and the terminal screens cover the board and explain
    /// themselves, and the first connect has nothing to explain yet.
    #[must_use]
    pub fn lock_banner(&self) -> Option<&'static str> {
        match self {
            ConnStatus::Reconnecting => Some("Reconnecting…"),
            ConnStatus::Restarting => Some("Server restarting…"),
            _ => None,
        }
    }

    /// The status to wait at before the next connect attempt, after a socket
    /// ended `ending` at this status. A restart says so until the next
    /// `Hello`: the refused connects while the server is down are plain
    /// disconnects, and mustn't turn it into `Reconnecting`.
    #[must_use]
    pub fn retrying_after(&self, ending: SocketEnding) -> ConnStatus {
        if ending == SocketEnding::Restarting || *self == ConnStatus::Restarting {
            ConnStatus::Restarting
        } else {
            ConnStatus::Reconnecting
        }
    }
}

/// What the status bar says for a [`Refusal::Failed`]: the answer didn't take
/// effect, and trying again may work.
const SAVE_ERROR: &str = "Couldn't save, try again.";

/// What the status bar says for a refusal that means this client has a bug
/// ([`Refusal::Malformed`], [`Refusal::NotYours`]). The detail goes to the
/// console.
const CLIENT_BUG: &str = "Something went wrong. Reload if it keeps happening.";

/// One applied submit's worth of events, for the event-log view.
#[derive(Debug, Clone, PartialEq)]
pub struct LogBatch {
    /// Who answered and what, e.g. "Agnes Baker: Fight: Ghoul Priest", or
    /// "Setup" for the batch a `Hello` carries.
    pub header: String,
    /// The events emitted by that submit, in order.
    pub events: Vec<Event>,
}

/// The chaos token a reveal drew, with how it resolved — the pair
/// `Event::ChaosTokenRevealed` carries, latched for the result panel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RevealedToken {
    /// The token drawn from the bag.
    pub token: ChaosToken,
    /// How that token resolved for this test.
    pub resolution: TokenResolution,
}

/// Everything the UI renders. `game`/`outcome`/`last_rejection` come
/// from `reduce`; `status` is driven by the transport, and by [`receive`] as a
/// frame arrives.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ClientState {
    pub game: Option<GameState>,
    pub outcome: Option<EngineOutcome>,
    /// The log position of the prompt `outcome` holds, which every submit
    /// sends back. From `Hello`, then each `Applied`; the start of the log
    /// before the first `Hello`, while the board is still locked.
    pub prompt: LogPosition,
    pub status: ConnStatus,
    /// The account this client is signed in as, once the server has said.
    /// Set by the transport, not by `reduce`.
    pub account: Option<SignedIn>,
    /// The account this connection answers as, from `Hello`. `None` before
    /// the first `Hello`, when the client answers for nobody.
    pub you: Option<AccountId>,
    /// Which account holds each investigator, from `Hello`. The client offers
    /// a prompt, and a Pass, only for the investigators [`you`](Self::you)
    /// hold.
    pub assigned: Assignment,
    pub last_rejection: Option<String>,
    /// Difficulty of the most recently *started* skill test, captured from
    /// `Event::SkillTestStarted` (which arrives in an earlier batch than the
    /// resolution). The result panel pairs it with the latched
    /// `SkillTestSucceeded`/`Failed` margin to show total-vs-difficulty.
    /// Cleared by `Hello`.
    pub last_skill_test_difficulty: Option<i8>,
    /// The chaos token drawn by the most recent reveal, captured from
    /// `Event::ChaosTokenRevealed` with how it resolved. Latched for the same
    /// reason as the difficulty: a symbol token whose ST.4 effect suspends for
    /// input puts the reveal in an earlier batch than the outcome.
    /// Cleared by a new test's `SkillTestStarted` and by `Hello`.
    pub last_revealed_token: Option<RevealedToken>,
    /// The most recent skill test's resolution — the `SkillTestSucceeded` or
    /// `SkillTestFailed` event itself — held from the batch that determined it
    /// until the batch that tears the test down. Latched for the same reason as
    /// the token, and for a sharper case: a reaction on `SkillTestResolved`
    /// suspends in the *same* batch as the outcome, so by the time the acknowledge
    /// pause arrives the live batch is empty and the result panel had nothing to
    /// render. Lita Chantler 01117 fires from that condition's `when` cell, before
    /// its resolve step logs the result, and Dr. Milan 01033 from its
    /// `after` cell, after it; both cells are walked before `AcknowledgeOutcome`,
    /// so either splits the batch. A prompt in the `when` cell therefore has no
    /// result latched yet: the banner shows
    /// [`provisional_verdict`](crate::skill_test_result::provisional_verdict)
    /// there instead.
    /// Cleared by `SkillTestEnded`, by a new test's `SkillTestStarted`, and by
    /// `Hello`.
    pub last_skill_test_result: Option<Event>,
    /// Full accumulated event history, grouped per applied submit, oldest
    /// first. Cleared by `Hello`. The event-log panel renders this.
    pub log: Vec<LogBatch>,
}

impl ClientState {
    /// Whether this connection's account holds `investigator`, so answers
    /// for them.
    #[must_use]
    pub fn holds(&self, investigator: InvestigatorId) -> bool {
        self.you
            .is_some_and(|you| self.assigned.holds(you, investigator))
    }

    /// The action answering the live prompt with `response`, named for the
    /// investigator who answers it: the responder's, or in a window several
    /// answer, the picked option's. A pass there, or a pick of an option
    /// offered to no one in particular, names the first investigator holding
    /// an option whom this account holds.
    ///
    /// `None` when no prompt is live, or when the investigator answering is
    /// not one this account holds: the client answers only for its own
    /// investigators.
    #[must_use]
    pub fn answer(&self, response: InputResponse) -> Option<PlayerAction> {
        let Some(EngineOutcome::AwaitingInput { request, .. }) = &self.outcome else {
            return None;
        };
        let investigator = match request.responder {
            Responder::Several => match request.picked_investigator(&response) {
                Some(investigator) => investigator,
                None => request
                    .holders()
                    .into_iter()
                    .find(|holder| self.holds(*holder))?,
            },
            _ => request.answerer(&response)?,
        };
        self.holds(investigator)
            .then_some(PlayerAction::ResolveInput {
                investigator,
                response,
            })
    }

    /// The live prompt as this account may answer it: its options cut to
    /// those [`answer`](Self::answer) would send. `None` when no prompt is
    /// live or this account answers none of it: a viewer, or an account
    /// none of whose investigators the prompt asks. Every surface that offers
    /// an answer reads the prompt through here.
    #[must_use]
    pub fn answerable_request(&self) -> Option<InputRequest> {
        let Some(EngineOutcome::AwaitingInput { request, .. }) = &self.outcome else {
            return None;
        };
        self.answer(InputResponse::Confirm)?;
        let mut own = request.clone();
        own.options
            .retain(|option| self.answer(InputResponse::PickSingle(option.id)).is_some());
        Some(own)
    }

    /// An investigator's name, or their seat's id if no game holding them is
    /// loaded.
    #[must_use]
    pub fn investigator_name(&self, id: InvestigatorId) -> String {
        self.game
            .as_ref()
            .and_then(|game| game.investigators.get(&id))
            .map_or_else(|| format!("Investigator {}", id.0), |inv| inv.name.clone())
    }
}

#[cfg(test)]
impl ClientState {
    /// The account a unit test that never sends a `Hello` answers as.
    pub(crate) const TEST_ACCOUNT: AccountId = AccountId(1);

    /// This state, signed in as [`Self::TEST_ACCOUNT`] and holding every
    /// investigator its game seats or its prompt names: what a `Hello` would
    /// have seeded, for a unit test that sets the prompt directly.
    #[must_use]
    pub(crate) fn you_hold_every_investigator(mut self) -> Self {
        let mut investigators: Vec<InvestigatorId> = self
            .game
            .iter()
            .flat_map(|game| game.investigators.keys().copied())
            .collect();
        if let Some(EngineOutcome::AwaitingInput { request, .. }) = &self.outcome {
            if let Responder::Investigator(investigator) | Responder::Lead(investigator) =
                request.responder
            {
                investigators.push(investigator);
            }
            investigators.extend(request.holders());
        }
        self.you = Some(Self::TEST_ACCOUNT);
        self.assigned = Assignment::all_to(Self::TEST_ACCOUNT, investigators);
        self
    }
}

/// What [`receive`] made of one server frame, for the transport to act on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Received {
    /// A `Hello` from this client's build: the board is `Connected`.
    Hello,
    /// Any other frame, reduced in.
    Message,
    /// A `Hello` from another build, or a frame that didn't parse: the status
    /// is `VersionMismatch`, nothing was reduced, and the transport stops.
    VersionMismatch,
}

/// The one field of a `{"hello": {...}}` frame read before the rest: every
/// other field is skipped unparsed, so it reads a `Hello` of any shape.
#[derive(serde::Deserialize)]
struct HelloBuild {
    hello: BuildOnly,
}

#[derive(serde::Deserialize)]
struct BuildOnly {
    build: String,
}

/// Take one text frame from the server: the transport's whole decode.
///
/// A `Hello` is checked for its build before anything else is parsed, since
/// the rest of a `Hello` from another build may not deserialize here; a
/// mismatch sets [`ConnStatus::VersionMismatch`] and reduces nothing. Any
/// other frame, or a `Hello` from this build, is parsed in full and
/// [`reduce`]d, and a `Hello` also sets the status `Connected`, in the same
/// update as its state. A frame that doesn't parse is a wire-format skew too.
pub fn receive(state: &mut ClientState, frame: &str) -> Received {
    if let Ok(HelloBuild { hello }) = serde_json::from_str::<HelloBuild>(frame) {
        if hello.build != protocol::build_id() {
            state.status = ConnStatus::VersionMismatch;
            return Received::VersionMismatch;
        }
    }
    let Ok(msg) = serde_json::from_str::<ServerMessage>(frame) else {
        state.status = ConnStatus::VersionMismatch;
        return Received::VersionMismatch;
    };
    let is_hello = matches!(msg, ServerMessage::Hello { .. });
    reduce(state, msg);
    if is_hello {
        state.status = ConnStatus::Connected;
        Received::Hello
    } else {
        Received::Message
    }
}

/// Fold one server message into the client state. Data only — never
/// touches `status`. Mirrors the server: a `Rejected` leaves
/// `game`/`outcome` unchanged (the rejection was sender-only).
pub fn reduce(state: &mut ClientState, msg: ServerMessage) {
    match msg {
        ServerMessage::Hello {
            build: _,
            you,
            assigned,
            prompt,
            state: s,
            outcome,
            events,
        } => {
            state.you = Some(you);
            state.assigned = assigned;
            state.game = Some(*s);
            state.outcome = Some(outcome);
            state.prompt = prompt;
            state.last_rejection = None;
            state.last_skill_test_difficulty = None;
            state.last_revealed_token = None;
            state.last_skill_test_result = None;
            state.log = Vec::new();
            if !events.is_empty() {
                state.log.push(LogBatch {
                    header: "Setup".to_string(),
                    events,
                });
            }
        }
        // An answer to a position before the current one is already in this
        // state: a socket attaching can be sent an `Applied` its `Hello`
        // already includes.
        ServerMessage::Applied(Applied { answered, .. }) if answered < state.prompt => {}
        ServerMessage::Applied(Applied {
            by,
            response,
            prompt,
            state: s,
            events,
            outcome,
            ..
        }) => {
            // The prompt still held here is the one this answers: an earlier
            // position was dropped above, and the server answers in order.
            let answered = state.outcome.replace(outcome);
            let picking = MultiPick::answered_on(state.game.as_ref());
            state.game = Some(*s);
            state.prompt = prompt;
            let header = batch_header(state, answered.as_ref(), picking, by, &response);
            // Walk the batch in order, latching what the result panel renders
            // from. Order matters within a batch as much as across batches:
            // `SkillTestStarted` clears the *previous* test's token and result,
            // and a batch may legitimately carry the end of one test and the
            // start of the next.
            //
            // Why latch at all: a resolution can span several apply batches, so
            // the batch holding the outcome need not be the one holding the
            // reveal, nor the one the acknowledge pause arrives on. Two known
            // splitters — an ST.4 effect that suspends for input, The
            // Gathering's Tablet dealing damage with a soak target in play,
            // and a reaction on `SkillTestResolved`, which fires in the
            // same apply as the outcome and pushes the acknowledge into a batch
            // of its own.
            for event in &events {
                match event {
                    // The difficulty announced at ST.1. Exact in current scope:
                    // the difficulty is a live query rather than a
                    // stored number, so this is the difficulty *at ST.1* and the
                    // margin is computed against the ST.6 re-read — but no
                    // in-corpus card changes an investigated location's shroud or
                    // an attacked enemy's fight or evade mid-test, so the two
                    // agree. The alternative is reading the modified difficulty
                    // off the still-live in-flight frame; that would be immune to
                    // (a) a reconnect mid-pause (`Hello` clears this cache) and
                    // (b) the first card that does move it mid-test. Revisit when
                    // either lands.
                    Event::SkillTestStarted { difficulty, .. } => {
                        state.last_skill_test_difficulty = Some(*difficulty);
                        // A new test invalidates the previous test's token and
                        // result. The difficulty needs no such clear — every test
                        // re-announces its own — but a test whose determination is
                        // already known draws no token at all (ST.3 and ST.4 do
                        // not happen), so without this the panel would name the
                        // *previous* test's token instead of the em dash.
                        state.last_revealed_token = None;
                        state.last_skill_test_result = None;
                    }
                    Event::ChaosTokenRevealed { token, resolution } => {
                        state.last_revealed_token = Some(RevealedToken {
                            token: *token,
                            resolution: *resolution,
                        });
                    }
                    Event::SkillTestSucceeded { .. } | Event::SkillTestFailed { .. } => {
                        state.last_skill_test_result = Some(event.clone());
                    }
                    // ST.8 teardown: the test is over, so its result stops being
                    // the one the panel would render. Without this the modal's
                    // other guard — an un-anchored `Confirm` (ADR 0011) — would be
                    // all that stands between a stale result and the next
                    // acknowledge-shaped pause.
                    Event::SkillTestEnded { .. } => {
                        state.last_skill_test_result = None;
                    }
                    _ => {}
                }
            }
            state.log.push(LogBatch { header, events });
        }
        ServerMessage::Rejected(refusal) => {
            match refusal {
                // The newer state is already here, or on its way.
                Refusal::StalePrompt => {}
                Refusal::Engine(reason) => state.last_rejection = Some(reason),
                Refusal::Failed(reason) => {
                    leptos::logging::warn!("the server couldn't save an answer: {reason}");
                    state.last_rejection = Some(SAVE_ERROR.into());
                }
                Refusal::Malformed => {
                    leptos::logging::error!("the server couldn't parse a frame this client sent");
                    state.last_rejection = Some(CLIENT_BUG.into());
                }
                Refusal::NotYours => {
                    leptos::logging::error!(
                        "this client answered for an investigator its account doesn't hold"
                    );
                    state.last_rejection = Some(CLIENT_BUG.into());
                }
            }
        }
        // Proof of life only, which the transport counts before reducing.
        ServerMessage::Pong => {}
    }
}

/// What a multi-pick answer did, which its wire form doesn't say: a mulligan, a
/// hand-size discard and a commit all arrive as the same
/// [`LoggedResponse::PickMultiple`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MultiPick {
    /// Cards committed to a skill test.
    Commit,
    /// Cards set aside and redrawn in a setup mulligan.
    Mulligan,
    /// Cards discarded down to the Upkeep hand size.
    HandSizeDiscard,
}

impl MultiPick {
    /// What a multi-pick answered on `game` (the game before the answer
    /// applied) does, read off its top frame. Only these three prompts take a
    /// multi-pick, so a top frame that is neither a mulligan nor a hand-size
    /// discard is a commit.
    fn answered_on(game: Option<&GameState>) -> Self {
        match game.and_then(|game| game.continuations.top()) {
            Some(Continuation::Mulligan(_)) => Self::Mulligan,
            Some(Continuation::HandSizeDiscard(_)) => Self::HandSizeDiscard,
            _ => Self::Commit,
        }
    }
}

/// The event-log header for an answer `by` gave with `response`, to the prompt
/// `answered` (the outcome the client held before the answer applied), named
/// from `state`'s game, the one the answer produced: who answered, then what
/// they answered.
///
/// A pick names its option, and a multi-pick names each of its options under
/// what `picking` says it did. A `Confirm` carries nothing, so it is named by
/// the prompt it confirmed: the Mythos draw is the `Confirm` anchored to the
/// encounter deck (ADR 0011).
fn batch_header(
    state: &ClientState,
    answered: Option<&EngineOutcome>,
    picking: MultiPick,
    by: InvestigatorId,
    response: &LoggedResponse,
) -> String {
    let what = match response {
        LoggedResponse::PickSingle { option } => option.label.clone(),
        LoggedResponse::PickMultiple { options } => {
            let names: Vec<&str> = options.iter().map(|o| o.label.as_str()).collect();
            match (picking, names.is_empty()) {
                (MultiPick::Commit, true) => "Commit no cards".into(),
                (MultiPick::Mulligan, true) => "Keep opening hand".into(),
                (MultiPick::Commit, false) => format!("Commit {}", names.join(", ")),
                (MultiPick::Mulligan, false) => format!("Mulligan {}", names.join(", ")),
                // Never empty: the engine asks for exactly the excess, which is
                // at least one card.
                (MultiPick::HandSizeDiscard, _) => format!("Discard {}", names.join(", ")),
            }
        }
        LoggedResponse::Skip => "Pass".into(),
        LoggedResponse::Confirm => match answered {
            Some(EngineOutcome::AwaitingInput { request, .. })
                if request.target == Some(OptionTarget::EncounterDeck) =>
            {
                "Draw encounter card".into()
            }
            _ => "Confirm".into(),
        },
    };
    format!("{}: {what}", state.investigator_name(by))
}

/// The single reactive store handed through Leptos context.
pub type StoreSignal = RwSignal<ClientState>;

/// Provide a fresh store signal into context and return it.
pub fn provide_store() -> StoreSignal {
    let signal = RwSignal::new(ClientState::default());
    provide_context(signal);
    signal
}

/// Read the store signal from context.
///
/// # Panics
///
/// Panics if no store signal is in context — a programmer error (every view
/// lives under [`provide_store`]).
pub fn use_store() -> StoreSignal {
    use_context::<StoreSignal>().expect("store signal provided at App root")
}

#[cfg(test)]
mod tests {
    use card_dsl::card_data::SkillKind;
    use game_core::engine::{ChoiceOption, OptionId};
    use game_core::event::FailureReason;
    use game_core::state::GameStateBuilder;
    use game_core::test_support;
    use protocol::logged_answer::LoggedOption;

    use super::*;

    /// An `Applied` frame answering the prompt at log position `answered`,
    /// carrying `events` and nothing else of interest: seat 1 confirmed.
    fn applied_at(answered: i64, events: Vec<Event>) -> ServerMessage {
        ServerMessage::Applied(Applied {
            answered: LogPosition(answered),
            by: InvestigatorId(1),
            response: LoggedResponse::Confirm,
            prompt: LogPosition(answered + 1),
            state: Box::new(sample_state()),
            events,
            outcome: EngineOutcome::Done,
        })
    }

    /// An `Applied` frame for the prompt at log position 0, answered by `by`
    /// with `response`, on the [`two_seats`] board.
    fn applied_by(by: u32, response: LoggedResponse) -> ServerMessage {
        ServerMessage::Applied(Applied {
            answered: LogPosition(0),
            by: InvestigatorId(by),
            response,
            prompt: LogPosition(1),
            state: Box::new(two_seats()),
            events: Vec::new(),
            outcome: EngineOutcome::Done,
        })
    }

    /// Two stock seats, named "Test Investigator 1" and "Test Investigator 2".
    fn two_seats() -> GameState {
        GameStateBuilder::new()
            .with_seat(test_support::seat(1))
            .with_seat(test_support::seat(2))
            .build()
    }

    /// A picked option labelled `label`, anchored nowhere.
    fn option(id: u32, label: &str) -> LoggedOption {
        LoggedOption {
            id: OptionId(id),
            label: label.into(),
            target: None,
            investigator: None,
        }
    }

    /// The account holding seat 1.
    const ACCOUNT_1: AccountId = AccountId(7);
    /// The account holding seat 2.
    const ACCOUNT_2: AccountId = AccountId(8);

    /// The client's state with `outcome` as the prompt it holds.
    fn holding(outcome: EngineOutcome) -> ClientState {
        ClientState {
            game: Some(two_seats()),
            outcome: Some(outcome),
            ..Default::default()
        }
    }

    /// [`holding`] `outcome`, signed in as `you`, with seat 1 held by
    /// [`ACCOUNT_1`] and seat 2 by [`ACCOUNT_2`].
    fn signed_in_holding(you: AccountId, outcome: EngineOutcome) -> ClientState {
        ClientState {
            you: Some(you),
            assigned: Assignment::from_iter([
                (InvestigatorId(1), ACCOUNT_1),
                (InvestigatorId(2), ACCOUNT_2),
            ]),
            ..holding(outcome)
        }
    }

    /// A window several answer offering each seat one reaction, and a third
    /// option offered to no one in particular.
    fn offered_to_both_seats() -> EngineOutcome {
        test_support::awaiting_request(
            InputRequest::pick_single(
                "You may trigger",
                vec![
                    ChoiceOption::new(OptionId(0), "Seat 1's reaction")
                        .offered_to(InvestigatorId(1)),
                    ChoiceOption::new(OptionId(1), "Seat 2's reaction")
                        .offered_to(InvestigatorId(2)),
                    ChoiceOption::new(OptionId(2), "Anyone's"),
                ],
            )
            .skippable()
            .answered_by(Responder::Several),
        )
    }

    fn answering_as(action: Option<PlayerAction>) -> Option<InvestigatorId> {
        match action? {
            PlayerAction::ResolveInput { investigator, .. } => Some(investigator),
            other => panic!("expected ResolveInput, got {other:?}"),
        }
    }

    #[test]
    fn hello_names_this_account_and_who_holds_each_investigator() {
        let mut s = ClientState::default();
        reduce(&mut s, hello_at(0, Vec::new()));
        assert_eq!(s.you, Some(ACCOUNT_1));
        assert_eq!(s.assigned.holder(InvestigatorId(1)), Some(ACCOUNT_1));
        assert!(s.holds(InvestigatorId(1)));
    }

    #[test]
    fn a_not_yours_refusal_shows_a_generic_error() {
        let mut s = ClientState::default();
        reduce(&mut s, ServerMessage::Rejected(Refusal::NotYours));
        assert_eq!(
            s.last_rejection.as_deref(),
            Some("Something went wrong. Reload if it keeps happening.")
        );
    }

    /// Seat 1's skill test asks seat 1: the account holding it answers, and
    /// the other account is offered nothing.
    #[test]
    fn only_the_account_holding_the_responder_answers_its_prompt() {
        let outcome = test_support::awaiting_confirm_input("Acknowledge");

        let holds_seat_1 = signed_in_holding(ACCOUNT_1, outcome.clone());
        assert_eq!(
            answering_as(holds_seat_1.answer(InputResponse::Confirm)),
            Some(InvestigatorId(1))
        );
        assert!(holds_seat_1.answerable_request().is_some());

        let holds_seat_2 = signed_in_holding(ACCOUNT_2, outcome);
        assert_eq!(holds_seat_2.answer(InputResponse::Confirm), None);
        assert_eq!(holds_seat_2.answerable_request(), None);
    }

    #[test]
    fn a_client_before_its_hello_answers_for_nobody() {
        let s = holding(test_support::awaiting_confirm_input("Acknowledge"));
        assert_eq!(s.answer(InputResponse::Confirm), None);
        assert_eq!(s.answerable_request(), None);
    }

    /// In a window both seats answer, the account holding seat 2 is offered
    /// seat 2's reaction and the option offered to no one, and its pass is
    /// seat 2's.
    #[test]
    fn in_a_window_several_answer_an_account_is_offered_only_its_own_options() {
        let holds_seat_2 = signed_in_holding(ACCOUNT_2, offered_to_both_seats());

        let offered: Vec<OptionId> = holds_seat_2
            .answerable_request()
            .expect("the account holds an answerer")
            .options
            .iter()
            .map(|option| option.id)
            .collect();
        assert_eq!(offered, [OptionId(1), OptionId(2)]);
        assert_eq!(
            answering_as(holds_seat_2.answer(InputResponse::Skip)),
            Some(InvestigatorId(2))
        );
        assert_eq!(
            answering_as(holds_seat_2.answer(InputResponse::PickSingle(OptionId(2)))),
            Some(InvestigatorId(2))
        );
        assert_eq!(
            holds_seat_2.answer(InputResponse::PickSingle(OptionId(0))),
            None
        );
    }

    /// The header of the one batch an `Applied` logged.
    fn only_header(s: &ClientState) -> &str {
        assert_eq!(s.log.len(), 1, "one batch logged");
        &s.log[0].header
    }

    /// A `Hello` naming the prompt at log position `prompt`, carrying setup's
    /// `events`.
    fn hello_at(prompt: i64, events: Vec<Event>) -> ServerMessage {
        ServerMessage::Hello {
            build: protocol::build_id().to_string(),
            you: ACCOUNT_1,
            assigned: Assignment::all_to(ACCOUNT_1, [InvestigatorId(1)]),
            prompt: LogPosition(prompt),
            state: Box::new(sample_state()),
            outcome: EngineOutcome::Done,
            events,
        }
    }

    fn started(difficulty: i8) -> Event {
        Event::SkillTestStarted {
            investigator: InvestigatorId(1),
            skill: SkillKind::Willpower,
            difficulty,
        }
    }

    fn succeeded(margin: i8) -> Event {
        Event::SkillTestSucceeded {
            investigator: InvestigatorId(1),
            skill: SkillKind::Willpower,
            margin,
        }
    }

    fn sample_state() -> GameState {
        GameStateBuilder::new()
            .with_investigator(test_support::test_investigator(1))
            .build()
    }

    #[test]
    fn hello_sets_game_and_clears_rejection() {
        let mut s = ClientState {
            last_rejection: Some("stale".into()),
            ..Default::default()
        };
        reduce(&mut s, hello_at(0, Vec::new()));
        assert!(s.game.is_some());
        assert_eq!(s.outcome, Some(EngineOutcome::Done));
        assert_eq!(s.last_rejection, None);
    }

    /// The JSON frame of a `Hello` from build `build`, carrying setup's
    /// `events`.
    fn hello_frame(build: &str, events: Vec<Event>) -> String {
        serde_json::to_string(&ServerMessage::Hello {
            build: build.to_string(),
            you: ACCOUNT_1,
            assigned: Assignment::all_to(ACCOUNT_1, [InvestigatorId(1)]),
            prompt: LogPosition(0),
            state: Box::new(sample_state()),
            outcome: EngineOutcome::Done,
            events,
        })
        .expect("a Hello serializes")
    }

    /// A build id that is not this client's.
    fn another_build() -> String {
        format!("not-{}", protocol::build_id())
    }

    #[test]
    fn a_hello_from_this_build_connects_with_its_state() {
        let mut s = ClientState::default();
        let received = receive(
            &mut s,
            &hello_frame(protocol::build_id(), vec![Event::ScenarioStarted]),
        );
        assert_eq!(received, Received::Hello);
        assert_eq!(s.status, ConnStatus::Connected);
        assert!(s.game.is_some(), "the Hello's state is reduced in");
        assert_eq!(s.log.len(), 1, "setup's events are logged");
    }

    #[test]
    fn a_hello_from_another_build_is_a_version_mismatch_that_reduces_nothing() {
        let mut s = ClientState::default();
        let received = receive(
            &mut s,
            &hello_frame(&another_build(), vec![Event::ScenarioStarted]),
        );
        assert_eq!(received, Received::VersionMismatch);
        assert_eq!(s.status, ConnStatus::VersionMismatch);
        assert_eq!(s.game, None, "a mismatched Hello's state is not taken");
        assert!(
            s.log.is_empty(),
            "a mismatched Hello's events are not logged"
        );
    }

    #[test]
    fn a_hello_from_another_build_is_a_version_mismatch_even_when_the_rest_does_not_parse() {
        // Another build's `state` may be any shape; only `build` is read.
        let frame = format!(
            r#"{{"hello":{{"build":"{}","prompt":"seven","state":[1,2],"novel":{{}}}}}}"#,
            another_build()
        );
        let mut s = ClientState::default();
        assert_eq!(receive(&mut s, &frame), Received::VersionMismatch);
        assert_eq!(s.status, ConnStatus::VersionMismatch);
    }

    #[test]
    fn a_frame_that_does_not_parse_is_a_version_mismatch() {
        let mut s = ClientState::default();
        assert_eq!(
            receive(&mut s, r#"{"novel":{}}"#),
            Received::VersionMismatch
        );
        assert_eq!(s.status, ConnStatus::VersionMismatch);
    }

    #[test]
    fn an_applied_frame_is_reduced_without_touching_the_status() {
        let mut s = ClientState {
            status: ConnStatus::Connected,
            ..ClientState::default()
        };
        let frame = serde_json::to_string(&applied_at(0, vec![Event::ScenarioStarted]))
            .expect("an Applied serializes");
        assert_eq!(receive(&mut s, &frame), Received::Message);
        assert_eq!(s.status, ConnStatus::Connected);
        assert_eq!(s.log.len(), 1, "the Applied's batch is logged");
    }

    #[test]
    fn applied_updates_game_and_outcome() {
        // Seed a pending rejection to prove Applied leaves it untouched
        // (only Hello clears it).
        let mut s = ClientState {
            last_rejection: Some("stale".into()),
            ..Default::default()
        };
        reduce(&mut s, applied_at(0, Vec::new()));
        assert!(s.game.is_some());
        assert_eq!(s.outcome, Some(EngineOutcome::Done));
        assert_eq!(s.last_rejection.as_deref(), Some("stale"));
    }

    #[test]
    fn applied_logs_the_batch_and_captures_difficulty() {
        let mut s = ClientState::default();
        reduce(
            &mut s,
            applied_at(
                0,
                vec![Event::SkillTestStarted {
                    investigator: InvestigatorId(1),
                    skill: SkillKind::Willpower,
                    difficulty: 3,
                }],
            ),
        );
        assert_eq!(s.last_skill_test_difficulty, Some(3));
        assert_eq!(s.log.len(), 1, "the batch is logged");
    }

    /// Regression for #787: the reveal and the outcome can land in different
    /// batches when an ST.4 effect suspends for input, and the latch carries the
    /// token over.
    #[test]
    fn applied_latches_the_revealed_token_across_batches() {
        let mut s = ClientState::default();
        reduce(
            &mut s,
            applied_at(
                0,
                vec![Event::ChaosTokenRevealed {
                    token: ChaosToken::Tablet,
                    resolution: TokenResolution::Modifier(-2),
                }],
            ),
        );
        assert_eq!(
            s.last_revealed_token,
            Some(RevealedToken {
                token: ChaosToken::Tablet,
                resolution: TokenResolution::Modifier(-2),
            })
        );
        // The next batch carries the outcome and no reveal; the latch survives.
        reduce(
            &mut s,
            applied_at(
                1,
                vec![Event::SkillTestFailed {
                    investigator: InvestigatorId(1),
                    skill: SkillKind::Combat,
                    reason: FailureReason::Total,
                    by: 1,
                }],
            ),
        );
        assert_eq!(
            s.last_revealed_token,
            Some(RevealedToken {
                token: ChaosToken::Tablet,
                resolution: TokenResolution::Modifier(-2),
            }),
            "a batch without a reveal leaves the latch alone"
        );
    }

    /// A test whose determination is already known draws no token at all, so the
    /// previous test's token must not survive into it.
    #[test]
    fn a_new_test_clears_the_previous_tests_token() {
        let mut s = ClientState {
            last_revealed_token: Some(RevealedToken {
                token: ChaosToken::Tablet,
                resolution: TokenResolution::Modifier(-2),
            }),
            ..Default::default()
        };
        reduce(
            &mut s,
            applied_at(
                0,
                vec![Event::SkillTestStarted {
                    investigator: InvestigatorId(1),
                    skill: SkillKind::Willpower,
                    difficulty: 3,
                }],
            ),
        );
        assert_eq!(
            s.last_revealed_token, None,
            "a started test clears the token the previous one drew"
        );
    }

    /// A batch carrying the start *and* the reveal — the common, non-suspending
    /// case — still ends with the token latched.
    #[test]
    fn a_start_and_reveal_in_one_batch_latches_the_token() {
        let mut s = ClientState::default();
        reduce(
            &mut s,
            applied_at(
                0,
                vec![
                    Event::SkillTestStarted {
                        investigator: InvestigatorId(1),
                        skill: SkillKind::Willpower,
                        difficulty: 3,
                    },
                    Event::ChaosTokenRevealed {
                        token: ChaosToken::Numeric(1),
                        resolution: TokenResolution::Modifier(1),
                    },
                ],
            ),
        );
        assert_eq!(
            s.last_revealed_token,
            Some(RevealedToken {
                token: ChaosToken::Numeric(1),
                resolution: TokenResolution::Modifier(1),
            })
        );
    }

    #[test]
    fn hello_clears_the_retained_latches() {
        let mut s = ClientState {
            last_skill_test_difficulty: Some(3),
            last_revealed_token: Some(RevealedToken {
                token: ChaosToken::Tablet,
                resolution: TokenResolution::Modifier(-2),
            }),
            last_skill_test_result: Some(succeeded(1)),
            ..Default::default()
        };
        reduce(&mut s, hello_at(0, Vec::new()));
        assert_eq!(
            s.last_skill_test_difficulty, None,
            "Hello clears the retained difficulty"
        );
        assert_eq!(
            s.last_revealed_token, None,
            "Hello clears the retained token, so a reconnect mid-pause shows no stale draw"
        );
        assert_eq!(
            s.last_skill_test_result, None,
            "and the retained result, for the same reason"
        );
    }

    /// Regression for #853: the resolution outlives the batch that determined it, because a
    /// reaction on `SkillTestResolved` puts the acknowledge pause in a batch of
    /// its own — an *empty* one, on the board that found this.
    #[test]
    fn applied_latches_the_resolution_across_batches() {
        let mut s = ClientState::default();
        reduce(&mut s, applied_at(0, vec![started(3), succeeded(0)]));
        assert_eq!(s.last_skill_test_result, Some(succeeded(0)));
        reduce(&mut s, applied_at(1, Vec::new()));
        assert_eq!(
            s.last_skill_test_result,
            Some(succeeded(0)),
            "the acknowledge pause's empty batch leaves the latch alone"
        );
    }

    /// ST.8 teardown releases it: past the end of its own test the result is no
    /// longer the one the panel would render, and the modal's other guard — an
    /// un-anchored `Confirm` (ADR 0011) — should not be all that stands between
    /// a stale result and the next acknowledge-shaped pause.
    #[test]
    fn the_end_of_a_test_clears_its_result() {
        let mut s = ClientState::default();
        reduce(&mut s, applied_at(0, vec![started(3), succeeded(0)]));
        reduce(
            &mut s,
            applied_at(
                1,
                vec![Event::SkillTestEnded {
                    investigator: InvestigatorId(1),
                }],
            ),
        );
        assert_eq!(s.last_skill_test_result, None);
    }

    /// A new test clears the previous one's result, as it clears its token: the
    /// panel must never pair this test's difficulty with the last one's outcome.
    #[test]
    fn a_new_test_clears_the_previous_tests_result() {
        let mut s = ClientState {
            last_skill_test_result: Some(succeeded(4)),
            ..Default::default()
        };
        reduce(&mut s, applied_at(0, vec![started(2)]));
        assert_eq!(s.last_skill_test_result, None);
    }

    /// The batch walk is ordered, which is what lets one batch carry the end of
    /// one test and the start of the next: the teardown clears, and then the new
    /// test's difficulty lands rather than being wiped by an earlier-in-batch
    /// event processed later.
    #[test]
    fn one_batch_can_end_a_test_and_start_the_next() {
        let mut s = ClientState::default();
        reduce(&mut s, applied_at(0, vec![started(3), succeeded(0)]));
        reduce(
            &mut s,
            applied_at(
                1,
                vec![
                    Event::SkillTestEnded {
                        investigator: InvestigatorId(1),
                    },
                    started(5),
                ],
            ),
        );
        assert_eq!(
            s.last_skill_test_result, None,
            "the first test's result is gone"
        );
        assert_eq!(
            s.last_skill_test_difficulty,
            Some(5),
            "and the second test's difficulty is the live one"
        );
    }

    #[test]
    fn an_engine_refusal_shows_its_reason_without_touching_game() {
        let mut s = ClientState {
            game: Some(sample_state()),
            outcome: Some(EngineOutcome::Done),
            ..Default::default()
        };
        let before = s.game.clone();
        reduce(
            &mut s,
            ServerMessage::Rejected(Refusal::Engine("not your turn".into())),
        );
        assert_eq!(s.last_rejection.as_deref(), Some("not your turn"));
        assert_eq!(s.game, before);
        assert_eq!(s.outcome, Some(EngineOutcome::Done));
    }

    #[test]
    fn a_stale_prompt_refusal_pushes_nothing() {
        let mut s = ClientState::default();
        reduce(&mut s, hello_at(4, vec![Event::ScenarioStarted]));
        let before = s.clone();

        reduce(&mut s, ServerMessage::Rejected(Refusal::StalePrompt));

        assert_eq!(s, before, "the newer state is already on its way");
    }

    #[test]
    fn a_failed_refusal_shows_a_save_error() {
        let mut s = ClientState::default();
        reduce(
            &mut s,
            ServerMessage::Rejected(Refusal::Failed("database is locked".into())),
        );
        assert_eq!(
            s.last_rejection.as_deref(),
            Some("Couldn't save, try again.")
        );
    }

    #[test]
    fn a_malformed_refusal_shows_a_generic_error() {
        let mut s = ClientState::default();
        reduce(&mut s, ServerMessage::Rejected(Refusal::Malformed));
        assert_eq!(
            s.last_rejection.as_deref(),
            Some("Something went wrong. Reload if it keeps happening.")
        );
    }

    #[test]
    fn hello_names_the_prompt_every_submit_answers() {
        let mut s = ClientState::default();
        reduce(&mut s, hello_at(7, Vec::new()));
        assert_eq!(s.prompt, LogPosition(7));
    }

    #[test]
    fn an_applied_moves_the_prompt_on() {
        let mut s = ClientState::default();
        reduce(&mut s, hello_at(7, Vec::new()));
        reduce(&mut s, applied_at(7, Vec::new()));
        assert_eq!(s.prompt, LogPosition(8));
    }

    /// The attach race: a socket can be sent an `Applied` its `Hello` already
    /// includes.
    #[test]
    fn an_applied_the_hello_already_included_is_not_logged_twice() {
        let mut s = ClientState::default();
        reduce(&mut s, hello_at(3, vec![Event::ScenarioStarted]));
        let before = s.clone();

        reduce(&mut s, applied_at(2, vec![Event::ScenarioStarted]));

        assert_eq!(s, before, "the Applied for position 2 is dropped");
    }

    /// The account holding seat 2 picks "Fight: Ghoul Priest". Another
    /// account's tab, whose own last click was something else, heads the batch
    /// with seat 2's pick.
    #[test]
    fn another_investigators_pick_is_logged_under_their_name_and_label() {
        let mut s = holding(test_support::awaiting_pick_single_input("Choose"));
        reduce(
            &mut s,
            applied_by(
                2,
                LoggedResponse::PickSingle {
                    option: option(3, "Fight: Ghoul Priest"),
                },
            ),
        );
        assert_eq!(only_header(&s), "Test Investigator 2: Fight: Ghoul Priest");
    }

    /// The board answered from has no mulligan or hand-size discard on top, so
    /// the multi-pick is a commit.
    #[test]
    fn a_commit_is_logged_with_the_cards_it_committed() {
        let mut s = holding(test_support::awaiting_commit_input("Commit", &[0, 1]));
        reduce(
            &mut s,
            applied_by(
                1,
                LoggedResponse::PickMultiple {
                    options: vec![option(0, "Guts"), option(1, "Overpower")],
                },
            ),
        );
        assert_eq!(
            only_header(&s),
            "Test Investigator 1: Commit Guts, Overpower"
        );
    }

    /// The [`two_seats`] board with seat 1's setup mulligan on top.
    fn mulliganing() -> GameState {
        GameStateBuilder::new()
            .with_seat(test_support::seat(1))
            .with_seat(test_support::seat(2))
            .with_mulligan_remaining([InvestigatorId(1), InvestigatorId(2)])
            .build()
    }

    /// The client's state on `game`, holding a multi-pick prompt for seat 1.
    fn picking_over(game: GameState) -> ClientState {
        ClientState {
            game: Some(game),
            outcome: Some(test_support::awaiting_commit_input("Pick", &[0, 1])),
            ..Default::default()
        }
    }

    #[test]
    fn a_mulligan_is_logged_with_the_cards_it_redrew() {
        let mut s = picking_over(mulliganing());
        reduce(
            &mut s,
            applied_by(
                1,
                LoggedResponse::PickMultiple {
                    options: vec![option(0, "Guts"), option(1, "Overpower")],
                },
            ),
        );
        assert_eq!(
            only_header(&s),
            "Test Investigator 1: Mulligan Guts, Overpower"
        );
    }

    #[test]
    fn a_hand_size_discard_is_logged_with_the_cards_it_discarded() {
        let game = GameStateBuilder::new()
            .with_seat(test_support::seat(1))
            .with_seat(test_support::seat(2))
            .with_hand_size_discard_pending([InvestigatorId(1)])
            .build();
        let mut s = picking_over(game);
        reduce(
            &mut s,
            applied_by(
                1,
                LoggedResponse::PickMultiple {
                    options: vec![option(0, "Guts")],
                },
            ),
        );
        assert_eq!(only_header(&s), "Test Investigator 1: Discard Guts");
    }

    #[test]
    fn an_empty_commit_is_logged_as_committing_nothing() {
        let mut s = holding(test_support::awaiting_commit_input("Commit", &[0, 1]));
        reduce(
            &mut s,
            applied_by(1, LoggedResponse::PickMultiple { options: vec![] }),
        );
        assert_eq!(only_header(&s), "Test Investigator 1: Commit no cards");
    }

    #[test]
    fn an_empty_mulligan_is_logged_as_keeping_the_opening_hand() {
        let mut s = picking_over(mulliganing());
        reduce(
            &mut s,
            applied_by(1, LoggedResponse::PickMultiple { options: vec![] }),
        );
        assert_eq!(only_header(&s), "Test Investigator 1: Keep opening hand");
    }

    #[test]
    fn a_pass_is_logged() {
        let mut s = holding(test_support::awaiting_skippable_pick_single_input("React?"));
        reduce(&mut s, applied_by(2, LoggedResponse::Skip));
        assert_eq!(only_header(&s), "Test Investigator 2: Pass");
    }

    #[test]
    fn a_confirm_is_logged() {
        let mut s = holding(test_support::awaiting_confirm_input("Acknowledge"));
        reduce(&mut s, applied_by(1, LoggedResponse::Confirm));
        assert_eq!(only_header(&s), "Test Investigator 1: Confirm");
    }

    /// The Mythos draw is an option-less `Confirm`: only the prompt the client
    /// held, anchored to the encounter deck, says what was confirmed.
    #[test]
    fn a_confirm_of_the_encounter_draw_is_logged_as_the_draw() {
        let mut outcome = test_support::awaiting_confirm_input("Mythos step 1.4");
        if let EngineOutcome::AwaitingInput { request, .. } = &mut outcome {
            request.target = Some(OptionTarget::EncounterDeck);
        }
        let mut s = holding(outcome);
        reduce(&mut s, applied_by(1, LoggedResponse::Confirm));
        assert_eq!(only_header(&s), "Test Investigator 1: Draw encounter card");
    }

    #[test]
    fn applied_logs_its_events_under_the_header() {
        let mut s = ClientState::default();
        reduce(&mut s, applied_at(0, vec![Event::ScenarioStarted]));
        assert_eq!(s.log.len(), 1);
        assert_eq!(s.log[0].events, vec![Event::ScenarioStarted]);
    }

    #[test]
    fn consecutive_applied_accumulate_in_order() {
        let mut s = ClientState::default();
        reduce(&mut s, applied_at(0, vec![Event::ScenarioStarted]));
        reduce(&mut s, applied_at(1, Vec::new()));
        let events: Vec<usize> = s.log.iter().map(|b| b.events.len()).collect();
        assert_eq!(events, vec![1, 0]);
    }

    #[test]
    fn rejected_pushes_no_batch() {
        let mut s = ClientState::default();
        reduce(
            &mut s,
            ServerMessage::Rejected(Refusal::Engine("nope".into())),
        );
        assert!(s.log.is_empty(), "rejection pushes no batch");
    }

    #[test]
    fn hello_clears_log() {
        let mut s = ClientState::default();
        s.log.push(LogBatch {
            header: "old".into(),
            events: Vec::new(),
        });
        reduce(&mut s, hello_at(0, Vec::new()));
        assert!(s.log.is_empty());
    }

    #[test]
    fn hello_with_events_pushes_setup_batch() {
        let mut s = ClientState::default();
        reduce(&mut s, hello_at(0, vec![Event::ScenarioStarted]));
        assert_eq!(s.log.len(), 1, "one Setup batch expected");
        assert_eq!(s.log[0].header, "Setup");
        assert_eq!(s.log[0].events, vec![Event::ScenarioStarted]);
    }

    #[test]
    fn hello_with_empty_events_leaves_log_empty() {
        let mut s = ClientState::default();
        reduce(&mut s, hello_at(0, Vec::new()));
        assert!(s.log.is_empty(), "no setup batch when events is empty");
    }

    #[test]
    fn hello_with_events_clears_prior_batches_first() {
        let mut s = ClientState::default();
        // Seed a prior action batch.
        s.log.push(LogBatch {
            header: "prior action".into(),
            events: vec![Event::ScenarioStarted],
        });
        // Hello with setup events: prior batch gone, Setup batch present.
        reduce(&mut s, hello_at(0, vec![Event::ScenarioStarted]));
        assert_eq!(s.log.len(), 1, "only the Setup batch should remain");
        assert_eq!(s.log[0].header, "Setup");
    }

    #[test]
    fn a_restart_close_waits_as_restarting() {
        for current in [ConnStatus::Connected, ConnStatus::Reconnecting] {
            assert_eq!(
                current.retrying_after(SocketEnding::Restarting),
                ConnStatus::Restarting,
                "from {current:?}"
            );
        }
    }

    #[test]
    fn a_drop_while_restarting_stays_restarting() {
        // While the server is down its connects are refused, which the browser
        // reports as 1006: a disconnect, which mustn't flicker the banner to
        // "Reconnecting…".
        assert_eq!(
            ConnStatus::Restarting.retrying_after(SocketEnding::Disconnected),
            ConnStatus::Restarting
        );
    }

    #[test]
    fn a_drop_otherwise_waits_as_reconnecting() {
        for current in [
            ConnStatus::Connecting,
            ConnStatus::Connected,
            ConnStatus::Reconnecting,
            ConnStatus::AwaitingRoster,
        ] {
            assert_eq!(
                current.retrying_after(SocketEnding::Disconnected),
                ConnStatus::Reconnecting,
                "from {current:?}"
            );
        }
    }
}
