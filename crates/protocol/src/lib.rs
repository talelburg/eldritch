//! The client/server contract: the JSON messages exchanged between a client
//! and the server, and the pure live game both sides drive.
//!
//! The server is authoritative; clients submit [`PlayerAction`]s and render the
//! state the server broadcasts. [`live::LiveGame`] decides each submission,
//! and [`logged_answer`] is the form the action log records it in. Both are
//! I/O-free, so the web crate's tests run the server's exact submit path.

use std::collections::BTreeMap;
use std::fmt;
use std::time::Duration;

use cards::decks::WeaknessChoice;
use game_core::action::PlayerAction;
use game_core::engine;
use game_core::engine::EngineOutcome;
use game_core::event::Event;
use game_core::scenario::ScenarioId;
use game_core::state::investigator::InvestigatorId;
use game_core::state::{CardCode, GameState};
use serde::{Deserialize, Serialize};

pub mod live;
pub mod lobby;
pub mod logged_answer;

use crate::live::{LiveGame, LiveSummary};
use crate::lobby::{Direction, LobbyRefusal, LobbyView, Member};
use crate::logged_answer::{LoggedResponse, ReplayFailure};

/// The id of the build this crate was compiled into: the `ELDRITCH_BUILD`
/// environment variable at compile time, or `"dev"` when it was unset.
///
/// `server` logs it at startup. `web` depends on `protocol` too, so a client
/// reading it here gets the id its bundle was compiled with, from the same
/// definition. TODO(#1226): the release build sets it to the commit sha.
#[must_use]
pub fn build_id() -> &'static str {
    option_env!("ELDRITCH_BUILD").unwrap_or("dev")
}

/// A message sent from a client to the server.
///
/// Externally tagged (`{"submit": {...}}`) rather than internally
/// tagged: the `Hello` baseline carries a [`GameState`] whose
/// integer-keyed maps round-trip through `serde_json` only when the
/// value is deserialized directly, not buffered through an
/// internally-tagged `Content`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientMessage {
    /// Submit a player action for validation and application.
    Submit {
        /// The log position of the prompt this answers, as the last
        /// [`ServerMessage::Hello`] or [`Applied`] named it. Any other
        /// position is refused as [`Refusal::StalePrompt`].
        prompt: LogPosition,
        /// The action to apply.
        action: PlayerAction,
    },
    /// Ask the server for a [`ServerMessage::Pong`], proving the connection
    /// is alive. Sent every [`PING_INTERVAL`], and at once when the tab comes
    /// back to the front.
    Ping,
    /// Take an investigator in the lobby: [`Lobby::take`](lobby::Lobby::take).
    Take {
        /// The investigator's card code.
        investigator: CardCode,
    },
    /// Give up one of the sender's picks in the lobby:
    /// [`Lobby::give_up`](lobby::Lobby::give_up).
    GiveUp {
        /// The picked investigator's card code.
        investigator: CardCode,
    },
    /// Set how one of the sender's picks meets its *"1 random basic
    /// weakness"*: [`Lobby::set_weakness`](lobby::Lobby::set_weakness).
    SetWeakness {
        /// The picked investigator's card code.
        investigator: CardCode,
        /// The fixed weakness, or a random one.
        choice: WeaknessChoice,
    },
    /// The creator moves an investigator one position along the lobby's
    /// seating: [`Lobby::move_in_seating`](lobby::Lobby::move_in_seating).
    MoveInSeating {
        /// The picked investigator's card code.
        investigator: CardCode,
        /// Which way they move.
        direction: Direction,
    },
    /// The creator makes a pick the lead investigator:
    /// [`Lobby::set_lead`](lobby::Lobby::set_lead).
    SetLead {
        /// The picked investigator's card code.
        investigator: CardCode,
    },
    /// Start the game in the lobby: [`Lobby::start`](lobby::Lobby::start).
    /// Every connected member is then sent a `Hello`.
    Start,
    /// Leave the lobby: [`Lobby::leave`](lobby::Lobby::leave). Every socket
    /// of the sender's to the game is then closed with
    /// [`CLOSE_NO_LONGER_A_MEMBER`].
    Leave,
    /// The creator removes a member from the lobby:
    /// [`Lobby::remove`](lobby::Lobby::remove). Every socket of the removed
    /// account's to the game is then closed with
    /// [`CLOSE_NO_LONGER_A_MEMBER`].
    Remove {
        /// The member removed.
        member: AccountId,
    },
}

/// A message sent from the server to a client. Externally tagged for
/// the same reason as [`ClientMessage`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServerMessage {
    /// The full render baseline, sent on connect/reconnect. Carries the
    /// current state and the outcome of the most recent apply (including
    /// any pending [`AwaitingInput`](EngineOutcome::AwaitingInput)).
    Hello {
        /// The server's [`build_id`]. A client compiled from another build
        /// reads this field alone, before parsing anything else, and shows the
        /// version-mismatch overlay instead: the rest of the frame may not
        /// deserialize for it. So the `"hello"` tag and this `"build"` key
        /// keep their names in every build.
        build: String,
        /// The account this connection is signed in as.
        you: AccountId,
        /// Which account holds each investigator: the client offers a prompt,
        /// and a Pass, only for the investigators `you` holds.
        assigned: Assignment,
        /// The log position of the prompt the game awaits: what a submit
        /// answering it sends back.
        prompt: LogPosition,
        /// Current derived game state. Boxed to keep the enum (and the
        /// per-game broadcast ring buffer) small — `GameState` dwarfs
        /// the other variants.
        state: Box<GameState>,
        /// Outcome of the most recent apply.
        outcome: EngineOutcome,
        /// Events emitted during scenario setup (`seat_and_open`), so
        /// the client's event log can show the opening draws, shuffles,
        /// and weakness set-aside. Carried for both freshly-created and
        /// reloaded games (the server persists them); empty only for games
        /// created before this was added.
        events: Vec<Event>,
    },
    /// The whole lobby: the first message on a socket to a game still in its
    /// lobby, and sent again to every connection after each change to it.
    Lobby {
        /// The server's [`build_id`], read first as `Hello`'s is: the
        /// `"lobby"` tag and this `"build"` key keep their names in every
        /// build.
        build: String,
        /// The account this connection is signed in as.
        you: AccountId,
        /// The lobby as every member sees it.
        view: LobbyView,
        /// Whether opening this connection is what made `you` a member. Set
        /// only on the first `Lobby` sent to that connection.
        joined: bool,
    },
    /// Sent only to the connection whose lobby command was refused, never
    /// broadcast to the room. The lobby is unchanged.
    LobbyRefused {
        /// Why it was refused.
        reason: LobbyRefusal,
    },
    /// Broadcast to every connection of a game after an accepted action.
    Applied(Applied),
    /// Sent only to the submitting client when its submit is refused.
    Rejected(Refusal),
    /// Sent only to the client whose [`ClientMessage::Ping`] it answers, never
    /// broadcast to the room.
    Pong,
}

/// A place in a game's action log: the position the next answer will occupy,
/// which names the prompt the game is waiting on.
///
/// Every prompt a client is shown carries one, and a submit sends it back, so
/// an answer meant for an earlier prompt is refused rather than resolving the
/// current one. Transparent over the log's `i64` row position, so it
/// serialises as a bare JSON number. The default is the first position, the
/// one a game with no answers awaits.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct LogPosition(pub i64);

impl LogPosition {
    /// The position after this one: where the answer after an answer at this
    /// position goes.
    #[must_use]
    pub fn next(self) -> Self {
        Self(self.0 + 1)
    }
}

impl fmt::Display for LogPosition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// The body of [`ServerMessage::Applied`]: one accepted answer, and the game
/// it left.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Applied {
    /// The log position the answer occupies.
    pub answered: LogPosition,
    /// The investigator who answered.
    pub by: InvestigatorId,
    /// What they answered, as the action log recorded it: each picked
    /// option's label, target and investigator, so every connection can say
    /// what was decided rather than what it last clicked.
    pub response: LoggedResponse,
    /// The log position of the prompt the game now awaits, the one after
    /// [`answered`](Self::answered).
    pub prompt: LogPosition,
    /// The authoritative game state after the action resolved.
    /// Boxed for the same reason as [`Hello`](ServerMessage::Hello)'s
    /// `state`: `GameState` dwarfs the other variants. The client
    /// renders this snapshot directly (events are for log/animation,
    /// not state reconstruction).
    pub state: Box<GameState>,
    /// Events emitted by the action's resolution.
    pub events: Vec<Event>,
    /// Outcome of the apply.
    pub outcome: EngineOutcome,
}

/// Why a submit was refused: the body of [`ServerMessage::Rejected`]. Each
/// variant tells a client how to react.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Refusal {
    /// It answered an earlier log position than the prompt the game awaits.
    /// The client stays quiet: it already has the newer state, or it is on
    /// its way.
    StalePrompt,
    /// It answered for an investigator the submitting account doesn't hold.
    /// A client offers only its own investigators' prompts, so this is a
    /// client bug.
    NotYours,
    /// The engine rejected the answer, for this reason, which the client
    /// shows the player.
    Engine(String),
    /// The frame didn't parse as a [`ClientMessage`]: a client bug.
    Malformed,
    /// It answered a game still in its lobby, which has no prompt to answer:
    /// a client bug.
    NotStarted,
    /// The engine accepted the answer but it couldn't be saved, for this
    /// reason: the log can't record it, or the store write failed. Nothing
    /// changed, and the player may try again.
    Failed(String),
}

/// Stable identifier for a persisted game.
///
/// Part of the client/server contract: returned by `POST /games` and used in
/// the WebSocket path `/ws/{game_id}`. Lives here (not in `game-core`) because
/// it is a host/transport concept, not a kernel domain id like `ScenarioId`.
/// Transparent over `String` — serializes as a bare JSON string, binds to a
/// `SQLite` TEXT column, and extracts from a URL path segment. Id *minting*
/// (`random_game_id`) lives server-side to keep `uuid` out of this wasm-safe
/// crate.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct GameId(String);

impl GameId {
    /// Wrap an existing id string.
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
    /// Borrow the underlying string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for GameId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// An account's stable id: its allowlist entry's primary key, never reused.
///
/// Part of the client/server contract: `GET /auth/me` reports it, and the dev
/// picker's form posts it. Transparent over `i64`, so it serialises as a bare
/// JSON number. Lives here rather than in `server` for the same reason as
/// [`GameId`]: the client reads it too.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AccountId(pub i64);

impl fmt::Display for AccountId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// Which account holds each investigator in a game: the account that answers
/// for them.
///
/// A live game is built with one, and refuses an answer from any account but
/// the investigator's holder, so an account holding no investigator is a
/// viewer. It is stored beside the game, not among its creation inputs
/// (ADR 0024). Serialises as a JSON object from investigator id to account id.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Assignment(BTreeMap<InvestigatorId, AccountId>);

impl Assignment {
    /// Every investigator in `investigators` held by `account`.
    pub fn all_to(
        account: AccountId,
        investigators: impl IntoIterator<Item = InvestigatorId>,
    ) -> Self {
        investigators
            .into_iter()
            .map(|investigator| (investigator, account))
            .collect()
    }

    /// Each seated investigator held by the account at its roster position in
    /// `accounts`. Seating gives the roster's first entry `InvestigatorId(1)`,
    /// its second `InvestigatorId(2)`, and so on, so `accounts` lists each
    /// roster entry's holder in roster order.
    #[must_use]
    pub fn in_seating_order(accounts: impl IntoIterator<Item = AccountId>) -> Self {
        (1..)
            .zip(accounts)
            .map(|(id, account)| (InvestigatorId(id), account))
            .collect()
    }

    /// Each investigator with the account holding it, in investigator order.
    pub fn iter(&self) -> impl Iterator<Item = (InvestigatorId, AccountId)> + '_ {
        self.0
            .iter()
            .map(|(investigator, account)| (*investigator, *account))
    }

    /// Whether no investigator has a holder.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The account holding `investigator`, if any does.
    #[must_use]
    pub fn holder(&self, investigator: InvestigatorId) -> Option<AccountId> {
        self.0.get(&investigator).copied()
    }

    /// Whether `account` holds `investigator`.
    #[must_use]
    pub fn holds(&self, account: AccountId, investigator: InvestigatorId) -> bool {
        self.holder(investigator) == Some(account)
    }
}

impl FromIterator<(InvestigatorId, AccountId)> for Assignment {
    fn from_iter<I: IntoIterator<Item = (InvestigatorId, AccountId)>>(iter: I) -> Self {
        Self(iter.into_iter().collect())
    }
}

/// Body of `POST /games`: the game is created as a lobby of the scenario,
/// with nothing seated until Start.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateGameRequest {
    /// The scenario module to set up. Serialises as its bare id string.
    pub scenario_id: ScenarioId,
}

/// Response to a successful `POST /games`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateGameResponse {
    /// The newly created game's id.
    pub game_id: GameId,
}

/// One game in the body of `GET /games`, the "Your games" listing: a game
/// the signed-in account is a member of. It is structured, with no prose:
/// the client words and groups it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameEntry {
    /// The game's id, which its link names.
    pub game_id: GameId,
    /// The scenario it plays.
    pub scenario_id: ScenarioId,
    /// The account that created it; `None` for a game stored before the
    /// creator was.
    pub creator: Option<AccountId>,
    /// Its investigators in seating order, each with its holder.
    pub investigators: Vec<EntryInvestigator>,
    /// The display name of every account the entry mentions: its creator and
    /// each investigator's holder, once each.
    pub names: Vec<Member>,
    /// When the game last moved, in unix millis.
    pub last_activity: i64,
    /// Where the game stands.
    pub state: GameEntryState,
}

impl GameEntry {
    /// The entry for the lobby `view` under `game_id`, created at
    /// `created_at` (unix millis): its picks are its investigators, and
    /// nothing has happened in it since it was created.
    #[must_use]
    pub fn of_lobby(game_id: GameId, view: &LobbyView, created_at: i64) -> Self {
        let investigators: Vec<EntryInvestigator> = view
            .picks
            .iter()
            .map(|pick| EntryInvestigator {
                investigator: pick.investigator.clone(),
                holder: Some(pick.account),
                lead: view.lead.as_ref() == Some(&pick.investigator),
            })
            .collect();
        let names = mentioned_names(Some(view.creator), &investigators, &[], &view.members);
        Self {
            game_id,
            scenario_id: view.scenario_id.clone(),
            creator: Some(view.creator),
            investigators,
            names,
            last_activity: created_at,
            state: GameEntryState::Lobby,
        }
    }

    /// The entry `account` is shown for the started game `listed`, loaded as
    /// `live`: its seated investigators in seating order, and its
    /// [`LiveSummary`] for `account`.
    #[must_use]
    pub fn of_live(listed: ListedGame, live: &LiveGame, account: AccountId) -> Self {
        let state = live.state();
        let lead = engine::the_lead(state).or(state.lead);
        let investigators: Vec<EntryInvestigator> = state
            .seating
            .iter()
            .filter_map(|id| {
                let investigator = state.investigators.get(id)?;
                Some(EntryInvestigator {
                    investigator: investigator.investigator_card.code.clone(),
                    // Start gives every seat a holder.
                    holder: live.assignment().holder(*id),
                    lead: lead == Some(*id),
                })
            })
            .collect();
        let summary = live.summary_for(account);
        let names = mentioned_names(
            listed.creator,
            &investigators,
            &summary.waiting_for(),
            &listed.members,
        );
        Self {
            game_id: listed.game_id,
            scenario_id: listed.scenario_id,
            creator: listed.creator,
            investigators,
            names,
            last_activity: listed.last_activity,
            state: GameEntryState::Live(summary),
        }
    }

    /// The entry for the started game `listed`, which this build can't
    /// replay, with its `investigators` as its stored rows give them and why
    /// it stopped.
    #[must_use]
    pub fn of_unresumable(
        listed: ListedGame,
        investigators: Vec<EntryInvestigator>,
        summary: UnresumableSummary,
    ) -> Self {
        let names = mentioned_names(listed.creator, &investigators, &[], &listed.members);
        Self {
            game_id: listed.game_id,
            scenario_id: listed.scenario_id,
            creator: listed.creator,
            investigators,
            names,
            last_activity: listed.last_activity,
            state: GameEntryState::Unresumable(summary),
        }
    }

    /// The display name of `account`, if the entry mentions it.
    #[must_use]
    pub fn name(&self, account: AccountId) -> Option<&str> {
        self.names
            .iter()
            .find(|member| member.account == account)
            .map(|member| member.display_name.as_str())
    }
}

/// A started game as "Your games" reads it from its stored rows, before
/// loading it: what its [`GameEntry`] carries whether it loads or is an
/// unresumable game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedGame {
    /// The game's id, which its link names.
    pub game_id: GameId,
    /// The scenario it plays.
    pub scenario_id: ScenarioId,
    /// The account that created it; `None` for a game stored before the
    /// creator was.
    pub creator: Option<AccountId>,
    /// Every account the game has, each with its display name.
    pub members: Vec<Member>,
    /// When the game last moved, in unix millis.
    pub last_activity: i64,
}

/// One investigator of a [`GameEntry`]: in a lobby, a pick.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntryInvestigator {
    /// The investigator's card code.
    pub investigator: CardCode,
    /// The account holding it: in a lobby, the member who picked it. `None`
    /// for an unresumable game stored with no assignment.
    pub holder: Option<AccountId>,
    /// Whether it is the lead investigator.
    pub lead: bool,
}

/// Where a [`GameEntry`]'s game stands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GameEntryState {
    /// In its lobby, waiting for its creator to press Start.
    Lobby,
    /// Started, and loaded by this build: who it waits on, summarised for the
    /// account listing it.
    Live(LiveSummary),
    /// Started, and this build can't replay it: an unresumable game.
    Unresumable(UnresumableSummary),
}

/// Where an unresumable game stopped and why, and the two builds involved: a
/// rollback to the build it started on resumes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnresumableSummary {
    /// Why it no longer loads.
    pub reason: UnresumableCause,
    /// How many answers its log holds.
    pub logged_answers: u32,
    /// The build the game was created on.
    pub created_build: String,
    /// The build the server runs now.
    pub server_build: String,
}

/// Why an unresumable game no longer loads: one variant for each of the
/// server's checks, in the order they run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnresumableCause {
    /// It was stored in a format version this build doesn't load.
    Version,
    /// It records no assignment, so nobody holds its investigators.
    NoAssignment,
    /// Setup no longer runs from its stored inputs, for this reason.
    Setup(String),
    /// Setup runs, but now decides something different from what it decided
    /// when the game was created.
    Fingerprint,
    /// A logged answer no longer replays.
    Replay {
        /// Zero-based position of that answer in the log.
        seq: u32,
        /// The check it fails.
        cause: ReplayCause,
    },
}

/// Which replay check a logged answer fails: the wire form of a
/// [`ReplayFailure`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReplayCause {
    /// The engine now asks a different prompt, or none.
    PromptChanged,
    /// The logged pick, labelled `label`, matches `matches` of the prompt's
    /// options where it matched one.
    OptionNotFound {
        /// The logged option's label.
        label: String,
        /// How many of the prompt's options it matches: none, or several.
        matches: usize,
    },
    /// The engine rejects the answer.
    Rejected,
    /// The answer drew from the RNG a different number of times.
    Draws,
}

impl From<&ReplayFailure> for ReplayCause {
    fn from(failure: &ReplayFailure) -> Self {
        match failure {
            ReplayFailure::PromptChanged { .. } => Self::PromptChanged,
            ReplayFailure::OptionNotFound { option, matches } => Self::OptionNotFound {
                label: option.label.clone(),
                matches: *matches,
            },
            ReplayFailure::Rejected(_) => Self::Rejected,
            ReplayFailure::Draws { .. } => Self::Draws,
        }
    }
}

/// The display name of every account an entry mentions, each once, from
/// `members`: its creator, each investigator's holder, then each account
/// in `also`.
fn mentioned_names(
    creator: Option<AccountId>,
    investigators: &[EntryInvestigator],
    also: &[AccountId],
    members: &[Member],
) -> Vec<Member> {
    let mut mentioned = Vec::new();
    let holders = investigators
        .iter()
        .filter_map(|investigator| investigator.holder);
    for account in creator
        .into_iter()
        .chain(holders)
        .chain(also.iter().copied())
    {
        if !mentioned.contains(&account) {
            mentioned.push(account);
        }
    }
    mentioned
        .into_iter()
        .filter_map(|account| members.iter().find(|member| member.account == account))
        .cloned()
        .collect()
}

/// The close code ending a game's WebSocket whose upgrade had no valid session.
///
/// It is from the private range: RFC 6455 §7.4.2, *"Status codes in the
/// range 4000-4999 are reserved for private use."* The server upgrades and
/// then closes, rather than refusing the upgrade, because a refused upgrade
/// reaches script only as close code 1006 (WHATWG `WebSockets`: *"the
/// WebSocket connection close code would be 1006"*), which a client can't
/// tell from a network drop.
pub const CLOSE_UNAUTHENTICATED: u16 = 4401;

/// The close code ending a game's WebSocket when the server has no game with
/// that id.
///
/// This is the only ending that means a dead link: a client leaves the game's
/// address on this code and on no other. Every other close, before or after
/// `Hello`, may be a server restarting or a network drop, and is retried.
pub const CLOSE_GAME_NOT_FOUND: u16 = 4404;

/// The close code ending a game's WebSocket when the game exists but is an
/// unresumable game: its stored rows can't be rebuilt by this server's build.
///
/// It fails the same way on every attempt until the build changes, so a
/// client stops retrying. The game's link still names it, and a rollback can
/// resume it.
pub const CLOSE_UNRESUMABLE: u16 = 4410;

/// The close code ending each of an account's WebSockets to a game it is no
/// longer a member of: it left the lobby, or the creator removed it.
///
/// It is not retried: a reconnect would make the account a member again,
/// since opening a game's link is what makes one. A client goes to "Your
/// games" instead, saying why.
pub const CLOSE_NO_LONGER_A_MEMBER: u16 = 4403;

/// The close code every open WebSocket gets when the server shuts down for a
/// restart: 1012, the standard "Service Restart" code.
///
/// The server is expected back, so a client stays on the game and retries
/// with backoff, saying the server is restarting until the next `Hello` or
/// `Lobby`.
pub const CLOSE_SERVER_RESTARTING: u16 = 1012;

// Liveness. Any frame proves a connection is alive, in both directions: every
// inbound frame resets the receiving end's silence timer, not only a `Pong` or
// a `Ping`, so a busy connection is never mistaken for a dead one.

/// How often a client sends [`ClientMessage::Ping`].
pub const PING_INTERVAL: Duration = Duration::from_secs(10);

/// How long a client waits with no frame from the server before it drops the
/// socket and reconnects: two missed [`PING_INTERVAL`]s.
pub const CLIENT_SILENCE_TIMEOUT: Duration = Duration::from_secs(20);

/// How long the server waits with no frame from a client before it closes the
/// socket with 1001 (Going Away): three missed [`PING_INTERVAL`]s.
pub const SERVER_SILENCE_TIMEOUT: Duration = Duration::from_secs(30);

/// Body of `GET /auth/me`'s `200`: the account the request's session signs
/// in as.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedIn {
    /// The account's stable id.
    pub account_id: AccountId,
    /// The name the host set for it, shown at the table.
    pub display_name: String,
}

/// Body of `GET /auth/me`'s `401`: the ways to sign in that this server
/// offers. Empty when it offers none.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignInRequired {
    /// Each available sign-in method.
    pub methods: Vec<SignInMethod>,
}

/// One way to sign in: the label a sign-in screen shows, and the URL it
/// sends the browser to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignInMethod {
    /// What the sign-in screen calls it.
    pub label: String,
    /// Where the browser goes to sign in this way, a same-origin path.
    pub url: String,
}

#[cfg(test)]
mod id_tests {
    use super::*;

    #[test]
    fn serializes_as_a_bare_string() {
        let id = GameId::new("abc");
        assert_eq!(serde_json::to_string(&id).unwrap(), "\"abc\"");
        let back: GameId = serde_json::from_str("\"abc\"").unwrap();
        assert_eq!(back, id);
    }

    #[test]
    fn an_account_id_serializes_as_a_bare_number() {
        let id = AccountId(7);
        assert_eq!(serde_json::to_string(&id).unwrap(), "7");
        let back: AccountId = serde_json::from_str("7").unwrap();
        assert_eq!(back, id);
    }
}

#[cfg(test)]
mod tests {
    use game_core::state::GameStateBuilder;
    use game_core::test_support;

    use super::*;

    #[test]
    fn hello_round_trips_through_json() {
        let state = GameStateBuilder::new()
            .with_seat(test_support::seat(1))
            .build();
        let events = vec![Event::ScenarioStarted];
        let assigned = Assignment::all_to(AccountId(7), [InvestigatorId(1)]);
        let msg = ServerMessage::Hello {
            build: "a1b2c3".to_string(),
            you: AccountId(8),
            assigned: assigned.clone(),
            prompt: LogPosition(0),
            state: Box::new(state.clone()),
            outcome: EngineOutcome::Done,
            events: events.clone(),
        };

        let json = serde_json::to_string(&msg).expect("serialize");
        let back: ServerMessage = serde_json::from_str(&json).expect("deserialize");

        match back {
            ServerMessage::Hello {
                build,
                you,
                assigned: a,
                state: s,
                events: e,
                ..
            } => {
                assert_eq!(build, "a1b2c3");
                assert_eq!(you, AccountId(8));
                assert_eq!(a, assigned, "the assignment round-trips through JSON");
                assert_eq!(*s, state);
                assert_eq!(e, events, "events round-trip through JSON");
            }
            other => panic!("expected Hello, got {other:?}"),
        }
    }

    #[test]
    fn applied_round_trips_through_json() {
        let state = GameStateBuilder::new()
            .with_seat(test_support::seat(1))
            .build();
        let msg = ServerMessage::Applied(Applied {
            answered: LogPosition(0),
            by: InvestigatorId(1),
            response: LoggedResponse::Confirm,
            prompt: LogPosition(1),
            state: Box::new(state.clone()),
            events: Vec::new(),
            outcome: EngineOutcome::Done,
        });

        let json = serde_json::to_string(&msg).expect("serialize");
        let back: ServerMessage = serde_json::from_str(&json).expect("deserialize");

        match back {
            ServerMessage::Applied(Applied {
                by,
                response,
                state: s,
                ..
            }) => {
                assert_eq!(*s, state);
                assert_eq!(by, InvestigatorId(1));
                assert_eq!(response, LoggedResponse::Confirm);
            }
            other => panic!("expected Applied, got {other:?}"),
        }
    }

    #[test]
    fn applied_round_trips_awaiting_input_option_target() {
        let state = GameStateBuilder::new()
            .with_seat(test_support::seat(1))
            .build();
        // The fixture's options are un-anchored; this proves the
        // ChoiceOption.target field survives the ServerMessage envelope
        // (game-core covers an anchored value directly).
        let outcome = test_support::awaiting_pick_single_input("Choose an action");
        let msg = ServerMessage::Applied(Applied {
            answered: LogPosition(0),
            by: InvestigatorId(1),
            response: LoggedResponse::Skip,
            prompt: LogPosition(1),
            state: Box::new(state),
            events: Vec::new(),
            outcome,
        });

        let json = serde_json::to_string(&msg).expect("serialize");
        let back: ServerMessage = serde_json::from_str(&json).expect("deserialize");

        let ServerMessage::Applied(Applied { outcome, .. }) = back else {
            panic!("expected Applied, got {back:?}");
        };
        let EngineOutcome::AwaitingInput { request, .. } = outcome else {
            panic!("expected AwaitingInput outcome");
        };
        assert!(
            request.options.iter().all(|o| o.target.is_none()),
            "option targets survive the envelope"
        );
    }

    #[test]
    fn a_lobby_request_is_the_scenario_alone() {
        let req = CreateGameRequest {
            scenario_id: ScenarioId::new("the-gathering"),
        };
        let json = serde_json::to_string(&req).expect("serialize");
        assert_eq!(
            json, r#"{"scenario_id":"the-gathering"}"#,
            "the scenario id is a bare string on the wire"
        );
        let back: CreateGameRequest = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.scenario_id.as_str(), "the-gathering");
    }

    #[test]
    fn a_lobby_round_trips_through_json_under_its_stable_tag() {
        let view = lobby::Lobby::new(
            ScenarioId::new("the-gathering"),
            AccountId(1),
            vec![Member {
                account: AccountId(1),
                display_name: "Tal".to_string(),
            }],
        )
        .view();
        let msg = ServerMessage::Lobby {
            build: "a1b2c3".to_string(),
            you: AccountId(2),
            view: view.clone(),
            joined: true,
        };

        let json = serde_json::to_string(&msg).expect("serialize");
        assert!(
            json.starts_with(r#"{"lobby":{"build":"a1b2c3""#),
            "the tag and the build key a client of any build reads: {json}"
        );
        let back: ServerMessage = serde_json::from_str(&json).expect("deserialize");
        let ServerMessage::Lobby {
            you,
            view: v,
            joined,
            ..
        } = back
        else {
            panic!("expected Lobby, got {back:?}");
        };
        assert_eq!((you, v, joined), (AccountId(2), view, true));
    }

    #[test]
    fn an_unstamped_build_reports_dev() {
        // A build stamped at compile time carries its own id instead, which
        // a test can't set.
        if option_env!("ELDRITCH_BUILD").is_none() {
            assert_eq!(build_id(), "dev");
        }
    }
}
