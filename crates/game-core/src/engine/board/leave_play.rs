//! The exits a card takes out of play, and the one router behind them.
//!
//! # Filed by its owner
//!
//! **Where a card goes when it leaves play is a question about its owner, not
//! its controller.** `glossary/Discard_Piles.md`: *"Any time a card is
//! discarded, it is placed faceup on top of its owner's discard pile. Encounter
//! cards are owned by the encounter deck."* `glossary/Ownership_and_Control.md`
//! says the same of every out-of-play area: *"If a card would enter an
//! out-of-play area that does not belong to the card's owner, the card is
//! physically placed in its owner's equivalent out-of-play area instead."*
//!
//! So each exit asks the card's [`Owner`] where it goes:
//!
//! | owner | [`discard_from_play`] | [`remove_from_game`] |
//! |---|---|---|
//! | an investigator | their discard | their removed-from-game pile |
//! | the encounter deck | the encounter discard | the game's removed-from-game pile |
//! | the scenario | the game's removed-from-game pile | the game's removed-from-game pile |
//!
//! A scenario-owned card has no discard pile, so discarding it removes it from
//! the game. Lita Chantler 01117 is the corpus case, and her ruling states it
//! (<https://arkhamdb.com/card/01117>): *"If Lita leaves play while a player
//! controls her temporarily during "The Gathering" scenario (i.e. while she is
//! technically not a part of that player's deck), remove her from the game (do
//! not place her into any discard pile)."* An investigator owner who is no
//! longer in the game has no pile either, and their card is removed to the
//! game's pile.
//!
//! [`place_in_victory_display`] is the one exit the owner does not route: the
//! victory display is *"an out-of-play game area shared by all players"*
//! (`glossary/Victory_Display_Victory_Points.md`), so every card that takes it
//! lands in the one display.
//!
//! # Leaves Play's consequences
//!
//! `glossary/Leaves_Play.md`: *"If a card leaves play, the following
//! consequences occur simultaneously with the card leaving play: All tokens on
//! the card are returned to the token pool. All attachments on the card are
//! discarded."* The exits apply both in the same step:
//!
//! - **Tokens** go away with the card. The engine has no token pool: the clues,
//!   damage, horror and uses on a [`CardInPlay`] cease to exist with it, and its
//!   pile holds only the card's code.
//! - **Attachments** are discarded through the same router, each by its own
//!   owner, whichever exit the card they were attached to took. Of the cards an
//!   exit takes, only an enemy and a location have attachments, and a location
//!   takes the cards put into play at it the same way.
//!
//! The third consequence, lasting-effect expiry, is not applied here.
//!
//! # Cards that cannot leave play
//!
//! A card can forbid its own exit: The Necronomicon 01009's *"It cannot leave
//! play while it has 1 or more horror on it."*, declared as a constant
//! `Restriction::CannotLeavePlay`. `glossary/Cannot.md` admits no exception:
//! *"The word "cannot" is absolute, and cannot be countermanded by other
//! abilities."* So every exit checks it before taking the card, and a card it
//! forbids stays where it is, untouched, with no event, and the exit answers
//! [`NotLeft::Refused`]. Each caller decides what a refusal means for it: a
//! cost that discards its source cannot be paid, an effect that discards one
//! rejects, a slot make-room never offers the card.
//!
//! Tokens do not dissolve the prohibition first. Leaves Play returns them
//! *"simultaneously with the card leaving play"*, so while the card is still
//! in play its horror is still on it.
//!
//! **Elimination is the one override.** `glossary/Elimination.md` step 0:
//! *"For the purpose of resolving weakness cards, the game has ended for the
//! eliminated investigator. Trigger any “when the game ends” abilities on each
//! weakness the eliminated investigator owns that is in play. Then, remove
//! those weaknesses from the game."* The game is over for that investigator,
//! so nothing they hold stays in play. Elimination's drains take
//! [`remove_from_game_on_elimination`] and [`discard_from_play_on_elimination`],
//! which skip the check.
//!
//! # One event per card
//!
//! Every card that leaves emits exactly one event: [`Event::CardDiscarded`]
//! naming the pile it landed in, [`Event::CardRemovedFromGame`],
//! [`Event::EnteredVictoryDisplay`], or [`Event::CardPlacedOnBottomOfDeck`] for
//! a discarded event its owner's cards redirect (Wendy's Amulet 01014). The
//! leaving card's event comes first, then one per attachment in attachment
//! order (for a location, then one per card put into play at it). A discard or
//! a removal names the [`Zone`] the card left.
//!
//! # Adding an exit
//!
//! A verb is added only when a card prints a new way out of play. A new exit is
//! a sibling verb over the same router, so the owner rule and the cascade stay
//! in one place: the victory display is one more `Exit` arm, and
//! [`remove_location_from_game`] files the location through the same router and
//! takes its attachments and the cards put into play at it through
//! `discard_attachments`. There is deliberately no public destination enum: a
//! caller always knows which exit it means.

use card_dsl::card_data::{CardMetadata, CardType};

use crate::engine::board::{self, Placement};
use crate::engine::{abilities_in_effect, Cx};
use crate::event::Event;
use crate::state::{
    CardCode, CardInPlay, CardInstanceId, DiscardPile, EnemyId, InvestigatorId, LocationId, Owner,
    Zone,
};

/// A card that can leave play: a card instance, wherever it sits on the board,
/// or an enemy.
///
/// An enemy is not a [`CardInPlay`]; it is keyed by its [`EnemyId`], so it is
/// addressed separately. Both convert into this with `.into()`, so a caller
/// passes whichever id it holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeavingCard {
    /// A card instance: in an investigator's play area or threat area, attached
    /// to a location, put into play at a location, or attached to an enemy.
    Instance(CardInstanceId),
    /// An enemy itself.
    Enemy(EnemyId),
}

impl From<CardInstanceId> for LeavingCard {
    fn from(id: CardInstanceId) -> Self {
        Self::Instance(id)
    }
}

impl From<EnemyId> for LeavingCard {
    fn from(id: EnemyId) -> Self {
        Self::Enemy(id)
    }
}

/// **Discard `card` from play**, wherever it sits, and file it by its owner:
/// an investigator's card to their discard, an encounter card to the encounter
/// discard, and a scenario-owned card out of the game (see the
/// `leave_play` module docs). Its attachments are discarded with it, each by its own
/// owner, and its tokens go away.
///
/// Returns the [`Placement`] the card left, from which a caller derives the
/// investigator whose area held it or the location it was at.
///
/// # Errors
///
/// With nothing changed and no event: [`NotLeft::NotInPlay`] when `card` is not
/// in play, or names an investigator card, which never leaves play;
/// [`NotLeft::Refused`] when the card cannot leave play right now (see the
/// `leave_play` module docs).
pub(crate) fn discard_from_play(
    cx: &mut Cx,
    card: impl Into<LeavingCard>,
) -> Result<Placement, NotLeft> {
    leave_play(cx, card.into(), Exit::Discard, Prohibition::Honoured)
}

/// **Remove `card` from the game**, wherever it sits: an investigator's card to
/// their own removed-from-game pile, any other card to the game's (see the
/// `leave_play` module docs). Its attachments are *discarded*, each by its own owner
/// — Leaves Play discards them whichever way the card itself went — and its
/// tokens go away.
///
/// Returns the [`Placement`] the card left.
///
/// # Errors
///
/// [`NotLeft`], with nothing changed and no event, as for
/// [`discard_from_play`].
pub(crate) fn remove_from_game(
    cx: &mut Cx,
    card: impl Into<LeavingCard>,
) -> Result<Placement, NotLeft> {
    leave_play(cx, card.into(), Exit::RemoveFromGame, Prohibition::Honoured)
}

/// [`remove_from_game`] for Elimination step 1, which removes the cards an
/// eliminated investigator controls in play even where they cannot leave play
/// (see the `leave_play` module docs).
///
/// # Errors
///
/// [`NotLeft::NotInPlay`] alone, as for [`discard_from_play`].
pub(crate) fn remove_from_game_on_elimination(
    cx: &mut Cx,
    card: impl Into<LeavingCard>,
) -> Result<Placement, NotLeft> {
    leave_play(
        cx,
        card.into(),
        Exit::RemoveFromGame,
        Prohibition::Overridden,
    )
}

/// [`discard_from_play`] for Elimination step 4, which empties the eliminated
/// investigator's threat area of the cards step 1 left there, even where they
/// cannot leave play (see the `leave_play` module docs).
///
/// # Errors
///
/// [`NotLeft::NotInPlay`] alone, as for [`discard_from_play`].
pub(crate) fn discard_from_play_on_elimination(
    cx: &mut Cx,
    card: impl Into<LeavingCard>,
) -> Result<Placement, NotLeft> {
    leave_play(cx, card.into(), Exit::Discard, Prohibition::Overridden)
}

/// **Place `card` in the victory display**, wherever it sits, worth `victory`
/// points. The victory display is *"an out-of-play game area shared by all
/// players"* (`glossary/Victory_Display_Victory_Points.md`), so the card goes
/// there whoever owns it, and emits [`Event::EnteredVictoryDisplay`]. Its
/// attachments are *discarded*, each by its own owner, and its tokens go away,
/// as for every exit.
///
/// The defeat of a Victory enemy takes this exit: *"As a victory point enemy is
/// defeated, place the card in the victory display instead of in the discard
/// pile."*
///
/// Returns the [`Placement`] the card left.
///
/// # Errors
///
/// [`NotLeft`], with nothing changed and no event, as for
/// [`discard_from_play`].
pub(crate) fn place_in_victory_display(
    cx: &mut Cx,
    card: impl Into<LeavingCard>,
    victory: u8,
) -> Result<Placement, NotLeft> {
    leave_play(
        cx,
        card.into(),
        Exit::VictoryDisplay { victory },
        Prohibition::Honoured,
    )
}

/// **Remove `location` from the game**: its card to the game's removed-from-game
/// pile, then its attachments and the cards put into play at it each
/// *discarded* by its own owner, as for [`remove_from_game`] (see the
/// `leave_play` module docs). Trapped 01108's *"Remove the Study from the game."* is
/// the corpus case.
///
/// A location is not a card instance and records no owner, so it has its own
/// verb rather than a [`LeavingCard`] arm. Its removal lands in the game's pile
/// whichever non-investigator owner it has.
///
/// The location only: an investigator or enemy standing there is the caller's
/// to move first, as Trapped places each investigator in the Hallway before
/// removing the Study.
///
/// Returns whether `location` was in play; `false`, with nothing changed and no
/// event, when it was not.
#[must_use = "false means the location was not in play and nothing left"]
pub(crate) fn remove_location_from_game(cx: &mut Cx, location: LocationId) -> bool {
    let Some(location) = cx.state.locations.remove(&location) else {
        return false;
    };
    file(
        cx,
        location.code,
        Owner::Scenario,
        Zone::Location,
        Exit::RemoveFromGame,
    );
    discard_attachments(cx, location.attachments, Zone::LocationAttachment);
    discard_attachments(cx, location.cards_at_location, Zone::AtLocation);
    true
}

/// Discard `code`, a card outside any zone of play, to its `owner`'s discard
/// pile, and emit its [`Event::CardDiscarded`] naming `from`. Filed through the
/// same router as the leave-play exits.
///
/// The card never entered play, so it takes no leave-play cascade: it has no
/// tokens and no attachments. `glossary/Discard_Piles.md`: *"Any time a card is
/// discarded, it is placed faceup on top of its owner's discard pile. Encounter
/// cards are owned by the encounter deck."* The cases that reach it:
///
/// - A resolved one-shot, `from` the deck it was drawn from
///   ([`Zone::EncounterDeck`] or [`Zone::Deck`]), since a card resolving in
///   limbo is in no zone of its own. An encounter-cardtype weakness drawn from
///   its bearer's deck resolves as an encounter card and goes to its
///   owner's pile.
/// - An encounter card that cannot enter play (Obscuring Fog 01168's limit-1
///   second copy, reported `from` [`Zone::LocationAttachment`]), or one taken
///   off the deck without being drawn (Rise of the Ghouls 01106's misses, `from`
///   [`Zone::EncounterDeck`]), both owned by the encounter deck. A native
///   reaches these through
///   [`NativeCx::discard_encounter_card`](crate::engine::NativeCx::discard_encounter_card).
pub(crate) fn discard_drawn_card(cx: &mut Cx, code: CardCode, owner: Owner, from: Zone) {
    file(cx, code, owner, from, Exit::Discard);
}

/// Why a card an exit was asked to take did not leave play. Nothing changed
/// and no event was emitted either way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotLeft {
    /// The card is not in play, or is an investigator card, which never leaves
    /// play.
    NotInPlay,
    /// The card is in play but cannot leave it right now: a constant
    /// `Restriction::CannotLeavePlay` on it holds (see the `leave_play` module
    /// docs).
    Refused,
}

/// Whether an exit honours a card's `Restriction::CannotLeavePlay`. Private:
/// only elimination's verbs override it.
#[derive(Debug, Clone, Copy)]
enum Prohibition {
    /// The exit refuses a card that cannot leave play.
    Honoured,
    /// Elimination: the card leaves regardless.
    Overridden,
}

/// Which exit a card takes. Private: each variant has its own public verb.
#[derive(Debug, Clone, Copy)]
enum Exit {
    /// [`discard_from_play`].
    Discard,
    /// [`remove_from_game`].
    RemoveFromGame,
    /// [`place_in_victory_display`], worth `victory` points.
    VictoryDisplay { victory: u8 },
}

/// The router: refuse a card that cannot leave play (unless `prohibition` is
/// overridden), else take `card` off the board, file it through `exit` by its
/// owner, then discard its attachments.
///
/// Only a card instance is checked: an enemy is no [`CardInPlay`], and no enemy
/// in the corpus prints that it cannot leave play.
fn leave_play(
    cx: &mut Cx,
    card: LeavingCard,
    exit: Exit,
    prohibition: Prohibition,
) -> Result<Placement, NotLeft> {
    let (code, owner, placement, attachments) = match card {
        LeavingCard::Instance(id) => {
            if matches!(prohibition, Prohibition::Honoured)
                && abilities_in_effect::cannot_leave_play(cx.state, cx.cards, id)
            {
                return Err(NotLeft::Refused);
            }
            let (card, placement) = board::take_instance(cx.state, id).ok_or(NotLeft::NotInPlay)?;
            (card.code, card.owner, placement, Vec::new())
        }
        LeavingCard::Enemy(id) => {
            let enemy = cx.state.enemies.remove(&id).ok_or(NotLeft::NotInPlay)?;
            (
                enemy.code,
                enemy.owner,
                Placement::Enemy(id),
                enemy.attachments,
            )
        }
    };
    file(cx, code, owner, zone_left(placement), exit);
    discard_attachments(cx, attachments, Zone::EnemyAttachment);
    Ok(placement)
}

/// Discard `attachments`, already taken off the card they were attached to,
/// each by its own owner. The Leaves Play cascade, shared by every exit.
fn discard_attachments(cx: &mut Cx, attachments: Vec<CardInPlay>, from: Zone) {
    for attachment in attachments {
        file(cx, attachment.code, attachment.owner, from, Exit::Discard);
    }
}

/// File `code`, just taken out of `from`, through `exit` by its `owner`, and
/// emit its one event.
///
/// An event an investigator owns is discarded to the bottom of their deck
/// instead while their cards redirect it
/// ([`events_go_to_bottom_of_deck`](abilities_in_effect::events_go_to_bottom_of_deck)):
/// Wendy's Amulet 01014's *"discard an event from play: Place it on the bottom
/// of your deck instead of in your discard pile"*. An eliminated investigator's
/// sweep never reaches it: step 1 has already taken the Amulet out of play.
fn file(cx: &mut Cx, code: CardCode, owner: Owner, from: Zone, exit: Exit) {
    if let (Exit::Discard, Owner::Investigator(id)) = (&exit, owner) {
        if (cx.cards.metadata_for)(&code).map(CardMetadata::card_type) == Some(CardType::Event)
            && redirect_event_to_bottom_of_deck(cx, id, &code)
        {
            return;
        }
    }
    let state = &mut *cx.state;
    let owning_investigator = match owner {
        Owner::Investigator(id) => state.investigators.get_mut(&id).map(|inv| (id, inv)),
        Owner::EncounterDeck | Owner::Scenario => None,
    };
    let event = match (exit, owner, owning_investigator) {
        // Shared by all players, so whoever owns the card.
        (Exit::VictoryDisplay { victory }, _, _) => {
            state.victory_display.push(code.clone());
            Event::EnteredVictoryDisplay { code, victory }
        }
        (Exit::Discard, Owner::Investigator(_), Some((id, inv))) => {
            inv.discard.push(code.clone());
            Event::CardDiscarded {
                code,
                from,
                to: DiscardPile::Investigator(id),
            }
        }
        (Exit::Discard, Owner::EncounterDeck, _) => {
            state.encounter_discard.push(code.clone());
            Event::CardDiscarded {
                code,
                from,
                to: DiscardPile::Encounter,
            }
        }
        (Exit::RemoveFromGame, Owner::Investigator(_), Some((_, inv))) => {
            inv.removed_from_game.push(code.clone());
            Event::CardRemovedFromGame { code, from }
        }
        // A scenario-owned card, an owner no longer in the game, or a removal
        // of a card no investigator owns: the game's removed-from-game pile.
        _ => {
            state.removed_from_game.push(code.clone());
            Event::CardRemovedFromGame { code, from }
        }
    };
    cx.events.push(event);
}

/// Place the event `code`, which `investigator` owns and which would go to
/// their discard pile, on the bottom of their deck instead, if their cards
/// redirect it
/// ([`events_go_to_bottom_of_deck`](abilities_in_effect::events_go_to_bottom_of_deck),
/// Wendy's Amulet 01014), emitting [`Event::CardPlacedOnBottomOfDeck`]. Returns
/// whether it did; on `false` nothing has moved and the caller discards the
/// card as usual. The caller has already established that `code` is an event.
///
/// Shared by the two ways an event reaches the discard pile: a played event
/// placed in step 4 of its play (`cards::place_played_event`), and an event
/// discarded from play (Barricade 01038, through [`file`]). For the played
/// event the Official FAQ puts the redirect inside step 4: *"An event card is
/// placed on top of its owner’s discard pile during step 4 of the
/// play/initiation sequence, so any effect that modifies where the event goes
/// (such as Parallel Agnes Baker or Crystallizer of Dreams) must occur during
/// that step. An effect that triggers “after you play” an event but does not
/// modify where the event goes (such as Heirloom of Hyberborea’s ability) would
/// naturally trigger just after step 4."* The Amulet is the corpus's only such
/// effect, so the redirect is one constant read here rather than a step-4
/// replacement window. TODO(#366): general replacement machinery, once a
/// second card changes where an event goes. Pushing onto the deck's bottom also
/// makes an empty deck the event itself, as the Amulet's ruling has it
/// (<https://arkhamdb.com/card/01014>): *"If your deck is empty, and Wendy’s
/// Amulet would have you place an event “on the bottom of your deck,” that
/// event becomes your new deck."*
pub(crate) fn redirect_event_to_bottom_of_deck(
    cx: &mut Cx,
    investigator: InvestigatorId,
    code: &CardCode,
) -> bool {
    if !abilities_in_effect::events_go_to_bottom_of_deck(cx.state, cx.cards, investigator) {
        return false;
    }
    let Some(inv) = cx.state.investigators.get_mut(&investigator) else {
        return false;
    };
    inv.deck.push(code.clone());
    cx.events.push(Event::CardPlacedOnBottomOfDeck {
        investigator,
        code: code.clone(),
    });
    true
}

/// The [`Zone`] an event names for a card that left `placement`.
fn zone_left(placement: Placement) -> Zone {
    match placement {
        Placement::PlayArea(_) => Zone::InPlay,
        Placement::ThreatArea(_) => Zone::ThreatArea,
        Placement::LocationAttachment(_) => Zone::LocationAttachment,
        Placement::AtLocation(_) => Zone::AtLocation,
        Placement::EnemyAttachment(_) => Zone::EnemyAttachment,
        Placement::Enemy(_) => Zone::Enemy,
        // `take_instance` answers only with a zone a card instance can be taken
        // out of, and `leave_play` names an enemy's placement itself.
        Placement::InvestigatorCard(_)
        | Placement::Location(_)
        | Placement::Act
        | Placement::Agenda => unreachable!("no card leaves play from {placement:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{GameState, GameStateBuilder, InvestigatorId, UseKind};
    use crate::test_support::{self, seat, TEST_REGISTRIES};

    /// The investigator whose areas the leaving cards sit in.
    const CONTROLLER: InvestigatorId = InvestigatorId(1);
    /// A second investigator, who owns the investigator-owned cards — so a card
    /// that lands in their pile was filed by its owner, not by whoever held it.
    const OWNER: InvestigatorId = InvestigatorId(2);
    const STUDY: LocationId = LocationId(1);
    const GHOUL: EnemyId = EnemyId(1);
    const LEAVING: CardInstanceId = CardInstanceId(70);
    const LEAVING_CODE: &str = "LEAVING";

    const OWNERS: [Owner; 3] = [
        Owner::Investigator(OWNER),
        Owner::EncounterDeck,
        Owner::Scenario,
    ];

    /// Every zone a card can leave play from.
    const ZONES: [Placement; 6] = [
        Placement::PlayArea(CONTROLLER),
        Placement::ThreatArea(CONTROLLER),
        Placement::LocationAttachment(STUDY),
        Placement::AtLocation(STUDY),
        Placement::EnemyAttachment(GHOUL),
        Placement::Enemy(GHOUL),
    ];

    fn instance(code: &str, id: u32, owner: Owner) -> CardInPlay {
        CardInPlay::enter_play(CardCode::new(code), CardInstanceId(id), owner)
    }

    /// Both investigators at the Study, with a ghoul engaged there, and nothing
    /// in any zone.
    fn empty_board() -> GameState {
        let mut ghoul = test_support::test_enemy(1, "Ghoul");
        ghoul.current_location = Some(STUDY);
        GameStateBuilder::new()
            .with_registries(TEST_REGISTRIES)
            .with_seat(seat(CONTROLLER.0).at(STUDY))
            .with_seat(seat(OWNER.0).at(STUDY))
            .with_location(test_support::test_location(STUDY.0, "Study"))
            .with_enemy(ghoul)
            .build()
    }

    /// The board with a card owned by `owner` in `zone`, and the address it
    /// leaves play by. In the `Enemy` row the leaving card is the ghoul itself.
    fn board_with(owner: Owner, zone: Placement) -> (GameState, LeavingCard) {
        let mut state = empty_board();
        let card = instance(LEAVING_CODE, LEAVING.0, owner);
        if let Placement::Enemy(id) = zone {
            let ghoul = state.enemies.get_mut(&id).unwrap();
            ghoul.code = CardCode::new(LEAVING_CODE);
            ghoul.owner = owner;
            return (state, LeavingCard::Enemy(id));
        }
        board::instance_zone_mut(&mut state, zone)
            .unwrap_or_else(|| panic!("no card leaves play from {zone:?}"))
            .push(card);
        (state, LeavingCard::Instance(LEAVING))
    }

    fn run(
        state: &mut GameState,
        exit: fn(&mut Cx, LeavingCard) -> Result<Placement, NotLeft>,
        card: LeavingCard,
    ) -> (Result<Placement, NotLeft>, Vec<Event>) {
        let mut events = Vec::new();
        let left = exit(&mut Cx::for_test(state, &mut events), card);
        (left, events)
    }

    fn discard(cx: &mut Cx, card: LeavingCard) -> Result<Placement, NotLeft> {
        discard_from_play(cx, card)
    }

    fn remove(cx: &mut Cx, card: LeavingCard) -> Result<Placement, NotLeft> {
        remove_from_game(cx, card)
    }

    /// Where every out-of-play pile stands: the owner's discard, the
    /// controller's discard, the encounter discard, the owner's removed-from-game
    /// pile, and the game's.
    fn piles(state: &GameState) -> [Vec<CardCode>; 5] {
        [
            state.investigators[&OWNER].discard.clone(),
            state.investigators[&CONTROLLER].discard.clone(),
            state.encounter_discard.clone(),
            state.investigators[&OWNER].removed_from_game.clone(),
            state.removed_from_game.clone(),
        ]
    }

    fn only(code: &str, slot: usize) -> [Vec<CardCode>; 5] {
        let mut piles: [Vec<CardCode>; 5] = Default::default();
        piles[slot] = vec![CardCode::new(code)];
        piles
    }

    fn still_on_board(state: &GameState, card: LeavingCard) -> bool {
        match card {
            LeavingCard::Instance(id) => board::find_instance(state, id).is_some(),
            LeavingCard::Enemy(id) => state.enemies.contains_key(&id),
        }
    }

    /// **Discard from play, owner × zone.** `glossary/Discard_Piles.md`: *"Any
    /// time a card is discarded, it is placed faceup on top of its owner's
    /// discard pile. Encounter cards are owned by the encounter deck."* A
    /// scenario-owned card has no pile and is removed from the game — Lita
    /// Chantler 01117's ruling (<https://arkhamdb.com/card/01117>).
    #[test]
    fn discard_from_play_files_each_owner_from_each_zone() {
        for owner in OWNERS {
            for zone in ZONES {
                let (mut state, card) = board_with(owner, zone);
                let (left, events) = run(&mut state, discard, card);
                let case = format!("{owner:?} leaving {zone:?}");
                assert_eq!(left, Ok(zone), "{case}: reports where it left");
                assert!(!still_on_board(&state, card), "{case}: off the board");
                let code = CardCode::new(LEAVING_CODE);
                let from = zone_left(zone);
                let (landed, event) = match owner {
                    Owner::Investigator(id) => (
                        only(LEAVING_CODE, 0),
                        Event::CardDiscarded {
                            code,
                            from,
                            to: DiscardPile::Investigator(id),
                        },
                    ),
                    Owner::EncounterDeck => (
                        only(LEAVING_CODE, 2),
                        Event::CardDiscarded {
                            code,
                            from,
                            to: DiscardPile::Encounter,
                        },
                    ),
                    Owner::Scenario => (
                        only(LEAVING_CODE, 4),
                        Event::CardRemovedFromGame { code, from },
                    ),
                };
                assert_eq!(piles(&state), landed, "{case}: destination");
                assert_eq!(events, vec![event], "{case}: one event");
            }
        }
    }

    /// **Remove from game, owner × zone.** An investigator's card goes to their
    /// own removed-from-game pile — `glossary/Ownership_and_Control.md`, *"If a
    /// card would enter an out-of-play area that does not belong to the card's
    /// owner, the card is physically placed in its owner's equivalent
    /// out-of-play area instead"* — and any other card to the game's.
    #[test]
    fn remove_from_game_files_each_owner_from_each_zone() {
        for owner in OWNERS {
            for zone in ZONES {
                let (mut state, card) = board_with(owner, zone);
                let (left, events) = run(&mut state, remove, card);
                let case = format!("{owner:?} leaving {zone:?}");
                assert_eq!(left, Ok(zone), "{case}: reports where it left");
                assert!(!still_on_board(&state, card), "{case}: off the board");
                let landed = match owner {
                    Owner::Investigator(_) => only(LEAVING_CODE, 3),
                    Owner::EncounterDeck | Owner::Scenario => only(LEAVING_CODE, 4),
                };
                assert_eq!(piles(&state), landed, "{case}: destination");
                assert_eq!(
                    events,
                    vec![Event::CardRemovedFromGame {
                        code: CardCode::new(LEAVING_CODE),
                        from: zone_left(zone),
                    }],
                    "{case}: one event",
                );
            }
        }
    }

    /// **Place in the victory display, owner × zone.** The display is *"an
    /// out-of-play game area shared by all players"*
    /// (`glossary/Victory_Display_Victory_Points.md`), so every owner's card
    /// lands in it, and in no pile of its owner's. An enemy's attachments are
    /// still discarded, each by its own owner (`glossary/Leaves_Play.md`).
    #[test]
    fn place_in_victory_display_takes_each_owner_from_each_zone() {
        for owner in OWNERS {
            for zone in ZONES {
                let (mut state, card) = board_with(owner, zone);
                state
                    .enemies
                    .get_mut(&GHOUL)
                    .unwrap()
                    .attachments
                    .push(instance("MINE", 71, Owner::Investigator(OWNER)));
                let (left, events) = run(
                    &mut state,
                    |cx, card| place_in_victory_display(cx, card, 2),
                    card,
                );
                let case = format!("{owner:?} leaving {zone:?}");
                assert_eq!(left, Ok(zone), "{case}: reports where it left");
                assert!(!still_on_board(&state, card), "{case}: off the board");
                assert_eq!(
                    state.victory_display,
                    vec![CardCode::new(LEAVING_CODE)],
                    "{case}: in the display"
                );
                assert_eq!(
                    events[0],
                    Event::EnteredVictoryDisplay {
                        code: CardCode::new(LEAVING_CODE),
                        victory: 2,
                    },
                    "{case}: its one event comes first"
                );
                let cascaded = matches!(zone, Placement::Enemy(_));
                let expected_piles = if cascaded {
                    only("MINE", 0)
                } else {
                    Default::default()
                };
                assert_eq!(
                    piles(&state),
                    expected_piles,
                    "{case}: no pile but the cascade's"
                );
                assert_eq!(
                    events.len(),
                    if cascaded { 2 } else { 1 },
                    "{case}: {events:?}"
                );
            }
        }
    }

    /// **The attachment cascade.** `glossary/Leaves_Play.md`: *"All
    /// attachments on the card are discarded."* — each by its own owner, and
    /// discarded whichever exit the card it was attached to took.
    #[test]
    fn an_enemys_attachments_are_discarded_each_by_its_own_owner() {
        for exit in [
            discard as fn(&mut Cx, LeavingCard) -> Result<Placement, NotLeft>,
            remove,
        ] {
            let mut state = empty_board();
            let ghoul = state.enemies.get_mut(&GHOUL).unwrap();
            ghoul.code = CardCode::new("GHOUL");
            ghoul.attachments = vec![
                instance("MINE", 71, Owner::Investigator(OWNER)),
                instance("ENCOUNTER", 72, Owner::EncounterDeck),
                instance("STORY", 73, Owner::Scenario),
            ];
            let (left, events) = run(&mut state, exit, GHOUL.into());
            assert_eq!(left, Ok(Placement::Enemy(GHOUL)));
            assert!(!state.enemies.contains_key(&GHOUL));
            for id in [71, 72, 73] {
                assert!(board::find_instance(&state, CardInstanceId(id)).is_none());
            }
            let piles = piles(&state);
            assert_eq!(piles[0], vec![CardCode::new("MINE")]);
            assert_eq!(piles[2].last(), Some(&CardCode::new("ENCOUNTER")));
            assert_eq!(piles[4].last(), Some(&CardCode::new("STORY")));
            let from = Zone::EnemyAttachment;
            assert_eq!(
                events[1..],
                [
                    Event::CardDiscarded {
                        code: CardCode::new("MINE"),
                        from,
                        to: DiscardPile::Investigator(OWNER),
                    },
                    Event::CardDiscarded {
                        code: CardCode::new("ENCOUNTER"),
                        from,
                        to: DiscardPile::Encounter,
                    },
                    Event::CardRemovedFromGame {
                        code: CardCode::new("STORY"),
                        from,
                    },
                ],
                "the enemy's own event, then one per attachment, in order",
            );
            assert!(
                matches!(&events[0], Event::CardDiscarded { code, from: Zone::Enemy, .. } | Event::CardRemovedFromGame { code, from: Zone::Enemy } if code.as_str() == "GHOUL"),
                "the enemy's own event comes first, got {:?}",
                events[0],
            );
        }
    }

    /// **Tokens go away with the card** (user story 11). `glossary/Leaves_Play.md`:
    /// *"All tokens on the card are returned to the token pool."* The engine has
    /// no token pool, so the clues, damage, horror and uses on the card cease to
    /// exist: none lands on the location or the investigator, and the pile holds
    /// the bare card.
    #[test]
    fn tokens_on_a_card_go_away_with_it() {
        let (mut state, card) = board_with(
            Owner::Investigator(OWNER),
            Placement::ThreatArea(CONTROLLER),
        );
        {
            let (on_board, _) = board::find_instance_mut(&mut state, LEAVING).unwrap();
            on_board.clues = 2;
            on_board.accumulated_damage = 1;
            on_board.accumulated_horror = 1;
            on_board.uses.insert(UseKind::Ammo, 3);
        }
        let location_clues = state.locations[&STUDY].clues;
        let investigator_clues = [
            state.investigators[&CONTROLLER].clues,
            state.investigators[&OWNER].clues,
        ];

        let (left, _) = run(&mut state, discard, card);

        assert_eq!(left, Ok(Placement::ThreatArea(CONTROLLER)));
        assert!(board::find_instance(&state, LEAVING).is_none());
        assert_eq!(
            state.locations[&STUDY].clues, location_clues,
            "no clue returned to the location"
        );
        assert_eq!(
            [
                state.investigators[&CONTROLLER].clues,
                state.investigators[&OWNER].clues
            ],
            investigator_clues,
            "no clue given to an investigator",
        );
        assert_eq!(
            state.investigators[&OWNER].discard,
            vec![CardCode::new(LEAVING_CODE)]
        );
    }

    /// A card that is not on the board leaves nothing: no event, no pile
    /// changed. Nor does an investigator card, which is not in a zone a card
    /// leaves play from.
    #[test]
    fn a_card_not_in_play_leaves_nothing() {
        for card in [
            LeavingCard::Instance(CardInstanceId(99)),
            LeavingCard::Instance(
                empty_board().investigators[&CONTROLLER]
                    .investigator_card
                    .instance_id,
            ),
            LeavingCard::Enemy(EnemyId(99)),
        ] {
            for exit in [
                discard as fn(&mut Cx, LeavingCard) -> Result<Placement, NotLeft>,
                remove,
            ] {
                let mut state = empty_board();
                let before = piles(&state);
                let (left, events) = run(&mut state, exit, card);
                assert_eq!(left, Err(NotLeft::NotInPlay), "{card:?}");
                assert!(events.is_empty(), "{card:?}");
                assert_eq!(piles(&state), before, "{card:?}");
            }
        }
    }

    /// **A location removed from the game takes its cards with it.** The
    /// location goes to the game's removed-from-game pile; its attachments, and
    /// then the cards put into play at it, are discarded each by its own owner
    /// (`glossary/Leaves_Play.md`: *"All attachments on the card are
    /// discarded."*).
    #[test]
    fn a_removed_location_discards_its_attachments_and_cards_at_it_by_owner() {
        let mut state = empty_board();
        let study = state.locations.get_mut(&STUDY).unwrap();
        study.code = CardCode::new("STUDY");
        study.attachments = vec![
            instance("MINE", 71, Owner::Investigator(OWNER)),
            instance("ENCOUNTER", 72, Owner::EncounterDeck),
        ];
        study.cards_at_location = vec![instance("STORY", 73, Owner::Scenario)];
        let mut events = Vec::new();
        let removed = remove_location_from_game(&mut Cx::for_test(&mut state, &mut events), STUDY);

        assert!(removed);
        assert!(!state.locations.contains_key(&STUDY));
        let piles = piles(&state);
        assert_eq!(piles[0], vec![CardCode::new("MINE")]);
        assert_eq!(piles[2], vec![CardCode::new("ENCOUNTER")]);
        assert_eq!(
            piles[4],
            vec![CardCode::new("STUDY"), CardCode::new("STORY")],
            "the location and the scenario's card are removed from the game",
        );
        assert_eq!(
            events,
            vec![
                Event::CardRemovedFromGame {
                    code: CardCode::new("STUDY"),
                    from: Zone::Location,
                },
                Event::CardDiscarded {
                    code: CardCode::new("MINE"),
                    from: Zone::LocationAttachment,
                    to: DiscardPile::Investigator(OWNER),
                },
                Event::CardDiscarded {
                    code: CardCode::new("ENCOUNTER"),
                    from: Zone::LocationAttachment,
                    to: DiscardPile::Encounter,
                },
                Event::CardRemovedFromGame {
                    code: CardCode::new("STORY"),
                    from: Zone::AtLocation,
                },
            ],
            "the location's own event, its attachments, then the cards at it",
        );
    }

    #[test]
    fn removing_a_location_not_in_play_leaves_nothing() {
        let mut state = empty_board();
        let before = state.clone();
        let mut events = Vec::new();
        let removed =
            remove_location_from_game(&mut Cx::for_test(&mut state, &mut events), LocationId(99));
        assert!(!removed);
        assert!(events.is_empty());
        assert_eq!(state, before);
    }
}
