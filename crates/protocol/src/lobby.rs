//! A **lobby**: a game with a scenario, a creator and a link, and nothing
//! seated yet (ADR 0024). Every signed-in account that opens its link becomes
//! a member.
//!
//! [`Lobby`] holds the lobby's rules and does no I/O, as
//! [`LiveGame`](crate::live::LiveGame) does for a started game: the server's
//! room calls it, stores what it changed, and sends every member the
//! [`LobbyView`] it renders.

use game_core::scenario::ScenarioId;
use game_core::state::CardCode;
use serde::{Deserialize, Serialize};

use crate::AccountId;

/// A game in its lobby: who created it and who has joined it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lobby {
    scenario_id: ScenarioId,
    creator: AccountId,
    /// In the order they joined, the creator first.
    members: Vec<Member>,
}

impl Lobby {
    /// A lobby of `scenario_id` as it is stored: created by `creator`, with
    /// `members` in the order they joined.
    #[must_use]
    pub fn new(scenario_id: ScenarioId, creator: AccountId, members: Vec<Member>) -> Self {
        Self {
            scenario_id,
            creator,
            members,
        }
    }

    /// Add `member` at the end of the members, unless their account is one
    /// already. Returns whether it was added.
    pub fn join(&mut self, member: Member) -> bool {
        if self.is_member(member.account) {
            return false;
        }
        self.members.push(member);
        true
    }

    /// Whether `account` is a member.
    #[must_use]
    pub fn is_member(&self, account: AccountId) -> bool {
        self.members.iter().any(|member| member.account == account)
    }

    /// What every member is sent of the lobby.
    #[must_use]
    pub fn view(&self) -> LobbyView {
        LobbyView {
            scenario_id: self.scenario_id.clone(),
            creator: self.creator,
            members: self.members.clone(),
            picks: Vec::new(),
            lead: None,
        }
    }
}

/// An account that has opened a game's link, with the name it is shown by.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Member {
    /// The member's account.
    pub account: AccountId,
    /// The name the host set for it, shown at the table.
    pub display_name: String,
}

/// One investigator a member has picked to play.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pick {
    /// The investigator's card code.
    pub investigator: CardCode,
    /// The member who picked them, and will hold them once the game starts.
    pub account: AccountId,
}

/// The body of [`ServerMessage::Lobby`](crate::ServerMessage::Lobby): the
/// whole lobby as every member sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LobbyView {
    /// The scenario the game will play.
    pub scenario_id: ScenarioId,
    /// The account that created the game, which controls the lobby.
    pub creator: AccountId,
    /// Every member, in the order they joined.
    pub members: Vec<Member>,
    /// The picks, in seating order: the first sits in position 1.
    pub picks: Vec<Pick>,
    /// The lead investigator's card code, once anyone is picked.
    pub lead: Option<CardCode>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(account: i64, name: &str) -> Member {
        Member {
            account: AccountId(account),
            display_name: name.to_string(),
        }
    }

    fn tals_lobby() -> Lobby {
        Lobby::new(
            ScenarioId::new("the-gathering"),
            AccountId(1),
            vec![member(1, "Tal")],
        )
    }

    #[test]
    fn a_new_member_joins_at_the_end() {
        let mut lobby = tals_lobby();
        assert!(lobby.join(member(2, "Sam")));
        assert_eq!(lobby.view().members, [member(1, "Tal"), member(2, "Sam")]);
    }

    #[test]
    fn a_member_joining_again_changes_nothing() {
        let mut lobby = tals_lobby();
        assert!(!lobby.join(member(1, "Tal")));
        assert_eq!(lobby.view().members, [member(1, "Tal")]);
    }

    #[test]
    fn a_new_lobby_has_no_picks_and_no_lead() {
        let view = tals_lobby().view();
        assert_eq!(view.creator, AccountId(1));
        assert!(view.picks.is_empty());
        assert_eq!(view.lead, None);
    }
}
