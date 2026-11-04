//! A **lobby**: a game with a scenario, a creator and a link, and nothing
//! seated yet (ADR 0024). Every signed-in account that opens its link becomes
//! a member.
//!
//! [`Lobby`] holds the lobby's rules and does no I/O, as
//! [`LiveGame`](crate::live::LiveGame) does for a started game: the server's
//! room calls it, stores what it changed, and sends every member the
//! [`LobbyView`] it renders.
//!
//! Each command a member sends is one function here, which applies it, or
//! refuses it with a [`LobbyRefusal`] and changes nothing.

use cards::decks::{WeaknessChoice, DEFAULT_DECKS};
use game_core::action::RosterEntry;
use game_core::scenario::ScenarioId;
use game_core::state::CardCode;
use serde::{Deserialize, Serialize};

use crate::{AccountId, Assignment};

/// The most investigators a lobby seats. Project scope, not a rule: Appendix
/// III caps nothing, and the phase-8 doc supports *"1–4 investigators"*.
pub const MAX_PICKS: usize = 4;

/// A game in its lobby: who created it, who has joined it, and the
/// investigators they have picked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lobby {
    scenario_id: ScenarioId,
    creator: AccountId,
    /// In the order they joined, the creator first.
    members: Vec<Member>,
    /// In seating order: the first sits in position 1.
    picks: Vec<Pick>,
    /// The lead investigator's card code: one of the picks', or `None`
    /// exactly when there are none.
    lead: Option<CardCode>,
}

impl Lobby {
    /// A lobby of `scenario_id` as it is stored: created by `creator`, with
    /// `members` in the order they joined, and nothing picked.
    #[must_use]
    pub fn new(scenario_id: ScenarioId, creator: AccountId, members: Vec<Member>) -> Self {
        Self {
            scenario_id,
            creator,
            members,
            picks: Vec::new(),
            lead: None,
        }
    }

    /// This lobby with `picks`, in seating order, and `lead`, as they are
    /// stored.
    #[must_use]
    pub fn with_picks(mut self, picks: Vec<Pick>, lead: Option<CardCode>) -> Self {
        self.picks = picks;
        self.lead = lead;
        self
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

    /// `account` takes `investigator`: the pick joins the end of the seating,
    /// and the first pick becomes the lead (Appendix III step 3: *"Choose one
    /// of those investigators to be the lead investigator for this game."*).
    ///
    /// # Errors
    ///
    /// [`LobbyRefusal::Taken`] if the investigator is already picked (Appendix
    /// III step 1: *"Each player chooses a different investigator"*), and
    /// [`LobbyRefusal::TableFull`] if [`MAX_PICKS`] are. Before either,
    /// [`LobbyRefusal::NotInPool`] if the investigator has no default deck:
    /// Start seats each pick with its default deck.
    pub fn take(&mut self, account: AccountId, investigator: CardCode) -> Result<(), LobbyRefusal> {
        if !DEFAULT_DECKS
            .iter()
            .any(|deck| deck.investigator == investigator.as_str())
        {
            return Err(LobbyRefusal::NotInPool { investigator });
        }
        if let Some(held) = self.pick_of(&investigator) {
            return Err(LobbyRefusal::Taken {
                investigator,
                by: held.account,
            });
        }
        if self.picks.len() >= MAX_PICKS {
            return Err(LobbyRefusal::TableFull);
        }
        if self.lead.is_none() {
            self.lead = Some(investigator.clone());
        }
        self.picks.push(Pick {
            investigator,
            account,
            weakness: WeaknessChoice::default(),
        });
        Ok(())
    }

    /// `account` gives up its pick of `investigator`. The seating closes the
    /// gap, and a lead given up passes to the new first position, or to nobody
    /// when no picks remain.
    ///
    /// # Errors
    ///
    /// [`LobbyRefusal::NotYourPick`] unless `account` holds the investigator.
    pub fn give_up(
        &mut self,
        account: AccountId,
        investigator: &CardCode,
    ) -> Result<(), LobbyRefusal> {
        let Some(position) = self
            .picks
            .iter()
            .position(|pick| pick.investigator == *investigator && pick.account == account)
        else {
            return Err(LobbyRefusal::NotYourPick {
                investigator: investigator.clone(),
            });
        };
        self.picks.remove(position);
        if self.lead.as_ref() == Some(investigator) {
            self.lead = self.picks.first().map(|pick| pick.investigator.clone());
        }
        Ok(())
    }

    /// `account` sets how its pick of `investigator` meets the card back's
    /// *"1 random basic weakness"*: the default deck's fixed weakness, or a
    /// random one drawn at Start.
    ///
    /// # Errors
    ///
    /// [`LobbyRefusal::NotYourPick`] unless `account` holds the investigator.
    pub fn set_weakness(
        &mut self,
        account: AccountId,
        investigator: &CardCode,
        choice: WeaknessChoice,
    ) -> Result<(), LobbyRefusal> {
        let Some(pick) = self
            .picks
            .iter_mut()
            .find(|pick| pick.investigator == *investigator && pick.account == account)
        else {
            return Err(LobbyRefusal::NotYourPick {
                investigator: investigator.clone(),
            });
        };
        pick.weakness = choice;
        Ok(())
    }

    /// The creator `account` moves `investigator` one position along the
    /// seating, swapping them with their neighbour that way. The lead stays
    /// with its investigator wherever they move.
    ///
    /// # Errors
    ///
    /// [`LobbyRefusal::NotCreator`] unless `account` created the lobby,
    /// [`LobbyRefusal::NotPicked`] unless the investigator is picked, and
    /// [`LobbyRefusal::NoNeighbour`] at the end of the seating `direction`
    /// points past.
    pub fn move_in_seating(
        &mut self,
        account: AccountId,
        investigator: &CardCode,
        direction: Direction,
    ) -> Result<(), LobbyRefusal> {
        self.check_creator(account)?;
        let position = self.position_of(investigator)?;
        let neighbour = match direction {
            Direction::Earlier => position.checked_sub(1),
            Direction::Later => Some(position + 1).filter(|&next| next < self.picks.len()),
        }
        .ok_or_else(|| LobbyRefusal::NoNeighbour {
            investigator: investigator.clone(),
        })?;
        self.picks.swap(position, neighbour);
        Ok(())
    }

    /// The creator `account` makes `investigator` the lead investigator: the
    /// players' choice, which the creator enters (Appendix III step 3:
    /// *"Choose one of those investigators to be the lead investigator for
    /// this game."*).
    ///
    /// # Errors
    ///
    /// [`LobbyRefusal::NotCreator`] unless `account` created the lobby, and
    /// [`LobbyRefusal::NotPicked`] unless the investigator is picked.
    pub fn set_lead(
        &mut self,
        account: AccountId,
        investigator: &CardCode,
    ) -> Result<(), LobbyRefusal> {
        self.check_creator(account)?;
        self.position_of(investigator)?;
        self.lead = Some(investigator.clone());
        Ok(())
    }

    /// Refuses a creator-only command from any other `account`.
    fn check_creator(&self, account: AccountId) -> Result<(), LobbyRefusal> {
        if account == self.creator {
            Ok(())
        } else {
            Err(LobbyRefusal::NotCreator)
        }
    }

    /// Where `investigator` sits in the seating, counting from 0.
    fn position_of(&self, investigator: &CardCode) -> Result<usize, LobbyRefusal> {
        self.picks
            .iter()
            .position(|pick| pick.investigator == *investigator)
            .ok_or_else(|| LobbyRefusal::NotPicked {
                investigator: investigator.clone(),
            })
    }

    /// `account` starts the game: what it is seated with. The picks, in
    /// seating order, become the roster, each with its default deck and its
    /// weakness choice; the lead is its pick's seat; and each pick's member
    /// holds the investigator seated from it. The lobby itself is unchanged: the room replaces it
    /// with the game once the game is stored.
    ///
    /// # Errors
    ///
    /// [`LobbyRefusal::NotCreator`] unless `account` created the game,
    /// [`LobbyRefusal::NoPicks`] while nobody is picked, and
    /// [`LobbyRefusal::NotInPool`] for a pick with no default deck.
    pub fn start(&self, account: AccountId) -> Result<Start, LobbyRefusal> {
        self.check_creator(account)?;
        if self.picks.is_empty() {
            return Err(LobbyRefusal::NoPicks);
        }
        let roster = self
            .picks
            .iter()
            .map(|pick| {
                DEFAULT_DECKS
                    .iter()
                    .find(|deck| deck.investigator == pick.investigator.as_str())
                    .map(|deck| deck.roster_entry(pick.weakness))
                    .ok_or_else(|| LobbyRefusal::NotInPool {
                        investigator: pick.investigator.clone(),
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let lead = self
            .picks
            .iter()
            .position(|pick| Some(&pick.investigator) == self.lead.as_ref())
            // With a pick, the lead is always one of the picks.
            .unwrap_or(0);
        Ok(Start {
            scenario_id: self.scenario_id.clone(),
            roster,
            lead,
            assignment: Assignment::in_seating_order(self.picks.iter().map(|pick| pick.account)),
        })
    }

    /// The pick of `investigator`, if anyone has taken them.
    fn pick_of(&self, investigator: &CardCode) -> Option<&Pick> {
        self.picks
            .iter()
            .find(|pick| pick.investigator == *investigator)
    }

    /// What every member is sent of the lobby.
    #[must_use]
    pub fn view(&self) -> LobbyView {
        LobbyView {
            scenario_id: self.scenario_id.clone(),
            creator: self.creator,
            members: self.members.clone(),
            picks: self.picks.clone(),
            lead: self.lead.clone(),
        }
    }
}

/// What [`Lobby::start`] seats the game with: the creation inputs the lobby
/// decides, and who holds each investigator.
#[derive(Debug, Clone, PartialEq)]
pub struct Start {
    /// The scenario the lobby was created for.
    pub scenario_id: ScenarioId,
    /// The picks, in seating order, each with its default deck.
    pub roster: Vec<RosterEntry>,
    /// The lead investigator, as an index into [`roster`](Self::roster).
    pub lead: usize,
    /// Each seated investigator held by the member who picked it: the roster
    /// index plus one, as seating numbers them.
    pub assignment: Assignment,
}

/// Why a lobby command was refused: the body of
/// [`ServerMessage::LobbyRefused`](crate::ServerMessage::LobbyRefused), sent
/// only to the account that sent it. Nothing changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LobbyRefusal {
    /// The investigator is already picked, by the account `by`: most likely a
    /// moment earlier, by a member whose Take arrived first.
    Taken {
        /// The investigator asked for.
        investigator: CardCode,
        /// The member who holds them.
        by: AccountId,
    },
    /// [`MAX_PICKS`] investigators are already picked.
    TableFull,
    /// The investigator has no default deck to play, so isn't in the pool.
    NotInPool {
        /// The investigator asked for.
        investigator: CardCode,
    },
    /// The command names a pick the sending account doesn't hold.
    NotYourPick {
        /// The investigator named.
        investigator: CardCode,
    },
    /// The command is the creator's alone: ordering the seating, choosing the
    /// lead, and starting the game.
    NotCreator,
    /// The command names an investigator nobody has picked.
    NotPicked {
        /// The investigator named.
        investigator: CardCode,
    },
    /// The investigator sits at the end of the seating the move points past,
    /// so has no neighbour to swap with.
    NoNeighbour {
        /// The investigator named.
        investigator: CardCode,
    },
    /// The game has started, and its roster is frozen.
    Started,
    /// Start needs at least one pick.
    NoPicks,
    /// The engine refused to seat the picks, for this reason: the game was
    /// not started, and the lobby is unchanged.
    Seating(String),
    /// The lobby accepted the command but it couldn't be saved, for this
    /// reason. Nothing changed, and the member may try again.
    Failed(String),
}

/// Which way [`Lobby::move_in_seating`] moves an investigator along the
/// seating.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    /// ◀: one position nearer position 1.
    Earlier,
    /// ▶: one position further from position 1.
    Later,
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
    /// How the pick's default deck meets its *"1 random basic weakness"*:
    /// [`WeaknessChoice::Fixed`] until its member sets it.
    pub weakness: WeaknessChoice,
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
    use game_core::state::InvestigatorId;

    use super::*;

    const ROLAND: &str = "01001";
    const DAISY: &str = "01002";
    const SKIDS: &str = "01003";
    const AGNES: &str = "01004";
    const WENDY: &str = "01005";

    fn member(account: i64, name: &str) -> Member {
        Member {
            account: AccountId(account),
            display_name: name.to_string(),
        }
    }

    fn code(code: &str) -> CardCode {
        CardCode::new(code)
    }

    fn pick(investigator: &str, account: i64) -> Pick {
        Pick {
            investigator: code(investigator),
            account: AccountId(account),
            weakness: WeaknessChoice::Fixed,
        }
    }

    fn tals_lobby() -> Lobby {
        Lobby::new(
            ScenarioId::new("the-gathering"),
            AccountId(1),
            vec![member(1, "Tal")],
        )
    }

    /// Tal's lobby, which Sam joined, with Sam's Daisy, then Tal's Roland and
    /// Agnes picked.
    fn three_picked() -> Lobby {
        let mut lobby = tals_lobby();
        lobby.join(member(2, "Sam"));
        lobby
            .take(AccountId(2), code(DAISY))
            .expect("Daisy is free");
        lobby
            .take(AccountId(1), code(ROLAND))
            .expect("Roland is free");
        lobby
            .take(AccountId(1), code(AGNES))
            .expect("Agnes is free");
        lobby
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

    #[test]
    fn the_first_pick_sits_first_and_leads() {
        let mut lobby = tals_lobby();
        lobby
            .take(AccountId(1), code(ROLAND))
            .expect("Roland is free");
        let view = lobby.view();
        assert_eq!(view.picks, [pick(ROLAND, 1)]);
        assert_eq!(view.lead, Some(code(ROLAND)));
    }

    #[test]
    fn later_picks_join_the_end_of_the_seating_and_the_lead_stays() {
        let view = three_picked().view();
        assert_eq!(
            view.picks,
            [pick(DAISY, 2), pick(ROLAND, 1), pick(AGNES, 1)]
        );
        assert_eq!(view.lead, Some(code(DAISY)));
    }

    #[test]
    fn taking_an_investigator_already_picked_is_refused() {
        let mut lobby = three_picked();
        let before = lobby.clone();
        assert_eq!(
            lobby.take(AccountId(2), code(ROLAND)),
            Err(LobbyRefusal::Taken {
                investigator: code(ROLAND),
                by: AccountId(1),
            })
        );
        assert_eq!(lobby, before);
    }

    #[test]
    fn a_fifth_take_is_refused() {
        let mut lobby = three_picked();
        lobby
            .take(AccountId(2), code(SKIDS))
            .expect("a fourth fits");
        let before = lobby.clone();
        assert_eq!(
            lobby.take(AccountId(2), code(WENDY)),
            Err(LobbyRefusal::TableFull)
        );
        assert_eq!(lobby, before);
    }

    #[test]
    fn giving_up_a_pick_closes_the_gap_and_keeps_the_lead() {
        let mut lobby = three_picked();
        lobby
            .give_up(AccountId(1), &code(ROLAND))
            .expect("Tal holds Roland");
        let view = lobby.view();
        assert_eq!(view.picks, [pick(DAISY, 2), pick(AGNES, 1)]);
        assert_eq!(view.lead, Some(code(DAISY)));
    }

    #[test]
    fn a_given_up_lead_falls_to_the_new_first_position() {
        let mut lobby = three_picked();
        lobby
            .give_up(AccountId(2), &code(DAISY))
            .expect("Sam holds Daisy");
        let view = lobby.view();
        assert_eq!(view.picks, [pick(ROLAND, 1), pick(AGNES, 1)]);
        assert_eq!(view.lead, Some(code(ROLAND)));
    }

    #[test]
    fn giving_up_the_last_pick_leaves_no_lead() {
        let mut lobby = tals_lobby();
        lobby
            .take(AccountId(1), code(WENDY))
            .expect("Wendy is free");
        lobby
            .give_up(AccountId(1), &code(WENDY))
            .expect("Tal holds Wendy");
        let view = lobby.view();
        assert!(view.picks.is_empty());
        assert_eq!(view.lead, None);
    }

    #[test]
    fn giving_up_another_members_pick_is_refused() {
        let mut lobby = three_picked();
        let before = lobby.clone();
        assert_eq!(
            lobby.give_up(AccountId(1), &code(DAISY)),
            Err(LobbyRefusal::NotYourPick {
                investigator: code(DAISY)
            })
        );
        assert_eq!(lobby, before);
    }

    #[test]
    fn giving_up_an_investigator_nobody_picked_is_refused() {
        let mut lobby = three_picked();
        assert_eq!(
            lobby.give_up(AccountId(1), &code(WENDY)),
            Err(LobbyRefusal::NotYourPick {
                investigator: code(WENDY)
            })
        );
    }

    /// The investigators a Start seats, and the fixed weakness each brings.
    fn seated(start: &Start) -> Vec<(&str, bool)> {
        start
            .roster
            .iter()
            .map(|entry| {
                let deck = DEFAULT_DECKS
                    .iter()
                    .find(|deck| deck.investigator == entry.investigator.as_str())
                    .expect("a default deck's investigator");
                (
                    entry.investigator.as_str(),
                    entry.random_basic_weaknesses == 0 && entry.deck.contains(&code(deck.weakness)),
                )
            })
            .collect()
    }

    #[test]
    fn start_seats_the_picks_in_seating_order_each_with_its_default_deck() {
        let start = three_picked().start(AccountId(1)).expect("Tal may start");
        assert_eq!(start.scenario_id, ScenarioId::new("the-gathering"));
        assert_eq!(
            seated(&start),
            [(DAISY, true), (ROLAND, true), (AGNES, true)],
            "each pick's default deck, with its fixed weakness"
        );
        assert_eq!(
            start.roster[0].deck.len(),
            33,
            "Daisy's 30 cards, her two signatures and her fixed weakness"
        );
    }

    #[test]
    fn start_seats_a_random_weakness_for_a_pick_that_chose_one() {
        let mut lobby = three_picked();
        lobby
            .set_weakness(AccountId(1), &code(ROLAND), WeaknessChoice::Random)
            .expect("Tal holds Roland");
        let start = lobby.start(AccountId(1)).expect("Tal may start");
        assert_eq!(
            seated(&start),
            [(DAISY, true), (ROLAND, false), (AGNES, true)],
            "Roland's fixed weakness is gone"
        );
        assert_eq!(start.roster[1].random_basic_weaknesses, 1);
    }

    #[test]
    fn start_assigns_each_seat_to_the_member_who_picked_it() {
        let start = three_picked().start(AccountId(1)).expect("Tal may start");
        assert_eq!(
            start.assignment,
            Assignment::from_iter([
                (InvestigatorId(1), AccountId(2)),
                (InvestigatorId(2), AccountId(1)),
                (InvestigatorId(3), AccountId(1)),
            ])
        );
    }

    #[test]
    fn start_leads_with_the_lead_pick_by_its_seat() {
        assert_eq!(three_picked().start(AccountId(1)).map(|s| s.lead), Ok(0));
        let stored = three_picked().view();
        let roland_leads = tals_lobby().with_picks(stored.picks, Some(code(ROLAND)));
        assert_eq!(roland_leads.start(AccountId(1)).map(|s| s.lead), Ok(1));
    }

    #[test]
    fn only_the_creator_may_start() {
        assert_eq!(
            three_picked().start(AccountId(2)).map(|s| s.lead),
            Err(LobbyRefusal::NotCreator)
        );
    }

    #[test]
    fn a_lobby_with_no_picks_cannot_start() {
        assert_eq!(
            tals_lobby().start(AccountId(1)).map(|s| s.lead),
            Err(LobbyRefusal::NoPicks)
        );
    }

    #[test]
    fn the_creator_moves_a_pick_earlier_and_the_lead_stays_with_its_investigator() {
        let mut lobby = three_picked();
        lobby
            .move_in_seating(AccountId(1), &code(AGNES), Direction::Earlier)
            .expect("Agnes has a neighbour before her");
        let view = lobby.view();
        assert_eq!(
            view.picks,
            [pick(DAISY, 2), pick(AGNES, 1), pick(ROLAND, 1)]
        );
        assert_eq!(view.lead, Some(code(DAISY)));
    }

    #[test]
    fn moving_the_lead_later_keeps_them_leading() {
        let mut lobby = three_picked();
        lobby
            .move_in_seating(AccountId(1), &code(DAISY), Direction::Later)
            .expect("Daisy has a neighbour after her");
        let view = lobby.view();
        assert_eq!(
            view.picks,
            [pick(ROLAND, 1), pick(DAISY, 2), pick(AGNES, 1)]
        );
        assert_eq!(view.lead, Some(code(DAISY)));
    }

    #[test]
    fn moving_the_first_position_earlier_is_refused() {
        let mut lobby = three_picked();
        let before = lobby.clone();
        assert_eq!(
            lobby.move_in_seating(AccountId(1), &code(DAISY), Direction::Earlier),
            Err(LobbyRefusal::NoNeighbour {
                investigator: code(DAISY)
            })
        );
        assert_eq!(lobby, before);
    }

    #[test]
    fn moving_the_last_position_later_is_refused() {
        let mut lobby = three_picked();
        let before = lobby.clone();
        assert_eq!(
            lobby.move_in_seating(AccountId(1), &code(AGNES), Direction::Later),
            Err(LobbyRefusal::NoNeighbour {
                investigator: code(AGNES)
            })
        );
        assert_eq!(lobby, before);
    }

    #[test]
    fn moving_an_investigator_nobody_picked_is_refused() {
        let mut lobby = three_picked();
        assert_eq!(
            lobby.move_in_seating(AccountId(1), &code(WENDY), Direction::Earlier),
            Err(LobbyRefusal::NotPicked {
                investigator: code(WENDY)
            })
        );
    }

    #[test]
    fn a_member_who_is_not_the_creator_cannot_move_the_seating() {
        let mut lobby = three_picked();
        let before = lobby.clone();
        assert_eq!(
            lobby.move_in_seating(AccountId(2), &code(DAISY), Direction::Later),
            Err(LobbyRefusal::NotCreator)
        );
        assert_eq!(lobby, before);
    }

    #[test]
    fn the_creator_stars_any_pick_as_lead() {
        let mut lobby = three_picked();
        lobby
            .set_lead(AccountId(1), &code(AGNES))
            .expect("Agnes is picked");
        let view = lobby.view();
        assert_eq!(view.lead, Some(code(AGNES)));
        assert_eq!(
            view.picks,
            [pick(DAISY, 2), pick(ROLAND, 1), pick(AGNES, 1)]
        );
    }

    #[test]
    fn a_lead_moved_by_the_creator_stays_with_its_investigator_across_moves() {
        let mut lobby = three_picked();
        lobby
            .set_lead(AccountId(1), &code(ROLAND))
            .expect("Roland is picked");
        lobby
            .move_in_seating(AccountId(1), &code(ROLAND), Direction::Later)
            .expect("Roland has a neighbour after him");
        lobby
            .move_in_seating(AccountId(1), &code(DAISY), Direction::Later)
            .expect("Daisy has a neighbour after her");
        let view = lobby.view();
        assert_eq!(
            view.picks,
            [pick(AGNES, 1), pick(DAISY, 2), pick(ROLAND, 1)]
        );
        assert_eq!(view.lead, Some(code(ROLAND)));
    }

    #[test]
    fn starring_an_investigator_nobody_picked_is_refused() {
        let mut lobby = three_picked();
        let before = lobby.clone();
        assert_eq!(
            lobby.set_lead(AccountId(1), &code(WENDY)),
            Err(LobbyRefusal::NotPicked {
                investigator: code(WENDY)
            })
        );
        assert_eq!(lobby, before);
    }

    #[test]
    fn a_member_who_is_not_the_creator_cannot_move_the_lead() {
        let mut lobby = three_picked();
        let before = lobby.clone();
        assert_eq!(
            lobby.set_lead(AccountId(2), &code(ROLAND)),
            Err(LobbyRefusal::NotCreator)
        );
        assert_eq!(lobby, before);
    }

    #[test]
    fn a_stored_lobby_keeps_its_picks_and_lead() {
        let stored = three_picked().view();
        let restored = tals_lobby()
            .with_picks(stored.picks.clone(), stored.lead.clone())
            .view();
        assert_eq!((restored.picks, restored.lead), (stored.picks, stored.lead));
    }

    #[test]
    fn taking_an_investigator_with_no_default_deck_is_refused() {
        let mut lobby = three_picked();
        let before = lobby.clone();
        assert_eq!(
            lobby.take(AccountId(2), code("01016")),
            Err(LobbyRefusal::NotInPool {
                investigator: code("01016")
            })
        );
        assert_eq!(lobby, before);
    }

    #[test]
    fn a_member_sets_their_picks_weakness_to_random() {
        let mut lobby = three_picked();
        lobby
            .set_weakness(AccountId(2), &code(DAISY), WeaknessChoice::Random)
            .expect("Sam holds Daisy");
        let view = lobby.view();
        assert_eq!(
            view.picks,
            [
                Pick {
                    weakness: WeaknessChoice::Random,
                    ..pick(DAISY, 2)
                },
                pick(ROLAND, 1),
                pick(AGNES, 1)
            ]
        );
    }

    #[test]
    fn setting_another_members_picks_weakness_is_refused() {
        let mut lobby = three_picked();
        let before = lobby.clone();
        assert_eq!(
            lobby.set_weakness(AccountId(1), &code(DAISY), WeaknessChoice::Random),
            Err(LobbyRefusal::NotYourPick {
                investigator: code(DAISY)
            })
        );
        assert_eq!(lobby, before);
    }

    #[test]
    fn setting_the_weakness_of_an_investigator_nobody_picked_is_refused() {
        let mut lobby = three_picked();
        assert_eq!(
            lobby.set_weakness(AccountId(1), &code(WENDY), WeaknessChoice::Random),
            Err(LobbyRefusal::NotYourPick {
                investigator: code(WENDY)
            })
        );
    }
}
