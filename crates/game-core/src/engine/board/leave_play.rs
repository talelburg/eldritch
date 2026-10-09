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
//!   exit takes, only an enemy has attachments.
//!
//! The third consequence, lasting-effect expiry, is not applied here.
//!
//! # One event per card
//!
//! Every card that leaves emits exactly one event: [`Event::CardDiscarded`]
//! naming the pile it landed in, or [`Event::CardRemovedFromGame`]. The leaving
//! card's event comes first, then one per attachment in attachment order. Each
//! names the [`Zone`] the card left.
//!
//! # Adding an exit
//!
//! A verb is added only when a card prints a new way out of play. A new exit is
//! a sibling verb over the same router, so the owner rule and the cascade stay
//! in one place: the victory display is one more `Exit` arm, and a location
//! leaving play takes its attachments and the cards put into play at it through
//! `discard_attachments`. There is deliberately no public destination enum: a
//! caller always knows which exit it means.

use crate::engine::Cx;
use crate::event::Event;
use crate::state::{CardCode, CardInPlay, CardInstanceId, DiscardPile, EnemyId, Owner, Zone};

use super::{take_instance, Placement};

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
/// [module docs](self)). Its attachments are discarded with it, each by its own
/// owner, and its tokens go away.
///
/// Returns the [`Placement`] the card left, from which a caller derives the
/// investigator whose area held it or the location it was at. `None`, with
/// nothing changed and no event, when `card` is not in play — or names an
/// investigator card, which never leaves play.
pub fn discard_from_play(cx: &mut Cx, card: impl Into<LeavingCard>) -> Option<Placement> {
    leave_play(cx, card.into(), Exit::Discard)
}

/// **Remove `card` from the game**, wherever it sits: an investigator's card to
/// their own removed-from-game pile, any other card to the game's (see the
/// [module docs](self)). Its attachments are *discarded*, each by its own owner
/// — Leaves Play discards them whichever way the card itself went — and its
/// tokens go away.
///
/// Returns the [`Placement`] the card left; `None`, with nothing changed and no
/// event, as for [`discard_from_play`].
pub fn remove_from_game(cx: &mut Cx, card: impl Into<LeavingCard>) -> Option<Placement> {
    leave_play(cx, card.into(), Exit::RemoveFromGame)
}

/// Which exit a card takes. Private: each variant has its own public verb.
#[derive(Debug, Clone, Copy)]
enum Exit {
    /// [`discard_from_play`].
    Discard,
    /// [`remove_from_game`].
    RemoveFromGame,
}

/// The router: take `card` off the board, file it through `exit` by its owner,
/// then discard its attachments.
fn leave_play(cx: &mut Cx, card: LeavingCard, exit: Exit) -> Option<Placement> {
    let (code, owner, placement, attachments) = match card {
        LeavingCard::Instance(id) => {
            let (card, placement) = take_instance(cx.state, id)?;
            (card.code, card.owner, placement, Vec::new())
        }
        LeavingCard::Enemy(id) => {
            let enemy = cx.state.enemies.remove(&id)?;
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
    Some(placement)
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
fn file(cx: &mut Cx, code: CardCode, owner: Owner, from: Zone, exit: Exit) {
    let state = &mut *cx.state;
    let owning_investigator = match owner {
        Owner::Investigator(id) => state.investigators.get_mut(&id).map(|inv| (id, inv)),
        Owner::EncounterDeck | Owner::Scenario => None,
    };
    let event = match (exit, owner, owning_investigator) {
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
    use crate::engine::board::find_instance;
    use crate::event::Event;
    use crate::state::{
        CardCode, CardInPlay, DiscardPile, GameState, GameStateBuilder, InvestigatorId, LocationId,
        Owner, UseKind, Zone,
    };
    use crate::test_support;

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
            .with_investigator_at(test_support::test_investigator(1), STUDY)
            .with_investigator_at(test_support::test_investigator(2), STUDY)
            .with_location(test_support::test_location(1, "Study"))
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
        super::super::instance_zone_mut(&mut state, zone)
            .unwrap_or_else(|| panic!("no card leaves play from {zone:?}"))
            .push(card);
        (state, LeavingCard::Instance(LEAVING))
    }

    /// The zone a discard or removal event names for a card leaving `placement`.
    fn zone_of(placement: Placement) -> Zone {
        match placement {
            Placement::PlayArea(_) => Zone::InPlay,
            Placement::ThreatArea(_) => Zone::ThreatArea,
            Placement::LocationAttachment(_) => Zone::LocationAttachment,
            Placement::AtLocation(_) => Zone::AtLocation,
            Placement::EnemyAttachment(_) => Zone::EnemyAttachment,
            Placement::Enemy(_) => Zone::Enemy,
            other => unreachable!("no card leaves play from {other:?}"),
        }
    }

    fn run(
        state: &mut GameState,
        exit: fn(&mut Cx, LeavingCard) -> Option<Placement>,
        card: LeavingCard,
    ) -> (Option<Placement>, Vec<Event>) {
        let mut events = Vec::new();
        let left = exit(
            &mut Cx {
                state,
                events: &mut events,
            },
            card,
        );
        (left, events)
    }

    fn discard(cx: &mut Cx, card: LeavingCard) -> Option<Placement> {
        discard_from_play(cx, card)
    }

    fn remove(cx: &mut Cx, card: LeavingCard) -> Option<Placement> {
        remove_from_game(cx, card)
    }

    /// Where every out-of-play pile stands: the owner's discard, the
    /// controller's discard, the encounter discard, the owner's removed-from-game
    /// pile, and the scenario's.
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
            LeavingCard::Instance(id) => find_instance(state, id).is_some(),
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
                assert_eq!(left, Some(zone), "{case}: reports where it left");
                assert!(!still_on_board(&state, card), "{case}: off the board");
                let code = CardCode::new(LEAVING_CODE);
                let from = zone_of(zone);
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
    /// out-of-play area instead"* — and any other card to the scenario's.
    #[test]
    fn remove_from_game_files_each_owner_from_each_zone() {
        for owner in OWNERS {
            for zone in ZONES {
                let (mut state, card) = board_with(owner, zone);
                let (left, events) = run(&mut state, remove, card);
                let case = format!("{owner:?} leaving {zone:?}");
                assert_eq!(left, Some(zone), "{case}: reports where it left");
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
                        from: zone_of(zone),
                    }],
                    "{case}: one event",
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
            discard as fn(&mut Cx, LeavingCard) -> Option<Placement>,
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
            assert_eq!(left, Some(Placement::Enemy(GHOUL)));
            assert!(!state.enemies.contains_key(&GHOUL));
            for id in [71, 72, 73] {
                assert!(find_instance(&state, CardInstanceId(id)).is_none());
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
            let (on_board, _) =
                crate::engine::board::find_instance_mut(&mut state, LEAVING).unwrap();
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

        run(&mut state, discard, card);

        assert!(find_instance(&state, LEAVING).is_none());
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
                discard as fn(&mut Cx, LeavingCard) -> Option<Placement>,
                remove,
            ] {
                let mut state = empty_board();
                let before = piles(&state);
                let (left, events) = run(&mut state, exit, card);
                assert_eq!(left, None, "{card:?}");
                assert!(events.is_empty(), "{card:?}");
                assert_eq!(piles(&state), before, "{card:?}");
            }
        }
    }
}
