//! The board as a set of zones a card sits in: the one walk over them, and the
//! one way to find an instance in them.
//!
//! # The walk
//!
//! `walk` visits every card on the board, each with its [`Placement`], in
//! ADR 0018's order:
//!
//! 1. each investigator — the active one first, then the rest of `turn_order`,
//!    then anyone else by id — their investigator card, the cards in their play
//!    area, then their threat area;
//! 2. each location by [`LocationId`] — the location, its attachments, then the
//!    cards put into play at it;
//! 3. each enemy by its id, then its attachments;
//! 4. the current act, then the current agenda.
//!
//! The trigger scan, the modifier sweep, the grant sweep, reachability and the
//! instance lookup all read it, and none keeps a zone list of its own, so a new
//! kind of source (Dunwich's) is added here once and every one of them sees it
//! (`docs/adr/0018-one-trigger-scan-walks-the-whole-board.md`).
//!
//! **The walk is unfiltered; each caller filters it.** An eliminated
//! investigator's cards are on it. The modifier sweep and the grant sweep read
//! `walk_active`, which skips them; the trigger scan skips them with
//! [`Placement::in_eliminated_area`] itself, keeping one investigator's at
//! elimination step 0; and the instance lookup does not skip them. Fast events
//! in hand are not on it at all — they are not on the board — and the trigger
//! scan adds them itself.
//!
//! # Finding an instance
//!
//! [`find_instance`] and [`find_instance_mut`] answer *"where is the card with
//! this instance id"* for the engine, for natives in `cards`, and for the web
//! client. Before them, each caller searched its own subset of zones: the web
//! client looked only among cards an investigator controls, so a prompt anchored
//! on Lita Chantler 01117 lost its source, and Cover Up 01007's natives looked
//! only in the triggering investigator's areas, so a co-located investigator
//! could not use it (#974).
//!
//! The answer comes with a [`Placement`] — which zone, and whose or which
//! location's — so a caller derives the investigator whose area holds the card,
//! or the location it is at, from where the card actually is rather than from a
//! parameter that can disagree with it.
//!
//! **The lookup is unfiltered.** It finds instances in an eliminated
//! investigator's areas too: Cover Up's game-end trauma resolves at elimination
//! step 0, after its holder has left `Active`, and must still find its card.
//!
//! # Leaving play
//!
//! [`discard_from_play`] and [`remove_from_game`] are the exits a card takes
//! out of play, wherever it sits. Both file the card by its **owner** through
//! one router and apply the Leaves Play consequences in the same step; the
//! private `leave_play` module's docs give the owner table and the rules
//! behind it.
//!
//! [`place_in_victory_display`] is the victory display's exit, which a
//! defeated Victory enemy takes.

mod leave_play;

pub use leave_play::{
    discard_from_play, place_in_victory_display, remove_from_game, remove_location_from_game,
    LeavingCard,
};

use std::collections::BTreeMap;

use crate::state::{
    AbilitySource, Act, Agenda, CardCode, CardInPlay, CardInstanceId, Enemy, EnemyId, GameState,
    InvestigatorId, Location, LocationId, Status, UseKind,
};

/// Where a card sits on the board: which zone, and whose or which location's.
///
/// [`find_instance`] only ever answers with the six placements a card
/// *instance* can have. `walk` also yields the four board cards that are not
/// instances — a location, an enemy, the act and the agenda.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    /// The investigator's own investigator card.
    InvestigatorCard(InvestigatorId),
    /// In the investigator's play area (`Investigator::cards_in_play`).
    PlayArea(InvestigatorId),
    /// In the investigator's threat area.
    ThreatArea(InvestigatorId),
    /// A location's own card.
    Location(LocationId),
    /// Attached to a location (Obscuring Fog 01168).
    LocationAttachment(LocationId),
    /// Put into play **at** a location and controlled by nobody (Lita Chantler
    /// 01117 in the Parlor). Its location is that location, but it is not an
    /// attachment.
    AtLocation(LocationId),
    /// An enemy's own card.
    Enemy(EnemyId),
    /// Attached to an enemy.
    EnemyAttachment(EnemyId),
    /// The current act. It is at no location.
    Act,
    /// The current agenda. It is at no location.
    Agenda,
}

impl Placement {
    /// The investigator whose area holds the card — their investigator card,
    /// play area or threat area — or `None` for any other placement.
    ///
    /// This is the investigator the engine treats as the card's controller, and
    /// the "you" of a card in a threat area: `glossary/You_Your.md`, *"the
    /// investigator who has the card in his/her threat area"*.
    #[must_use]
    pub fn investigator(self) -> Option<InvestigatorId> {
        match self {
            Self::InvestigatorCard(id) | Self::PlayArea(id) | Self::ThreatArea(id) => Some(id),
            Self::Location(_)
            | Self::LocationAttachment(_)
            | Self::AtLocation(_)
            | Self::Enemy(_)
            | Self::EnemyAttachment(_)
            | Self::Act
            | Self::Agenda => None,
        }
    }

    /// The location the card is at: the location itself, the location it is
    /// attached to or put into play at, the location of the investigator whose
    /// area holds it, or the location of the enemy it is or is attached to.
    /// `None` when that investigator or enemy is at no location, and for the act
    /// and the agenda, which are nowhere.
    #[must_use]
    pub fn location(self, state: &GameState) -> Option<LocationId> {
        match self {
            Self::InvestigatorCard(id) | Self::PlayArea(id) | Self::ThreatArea(id) => {
                state.investigators.get(&id)?.current_location
            }
            Self::Location(id) | Self::LocationAttachment(id) | Self::AtLocation(id) => Some(id),
            Self::Enemy(id) | Self::EnemyAttachment(id) => state.enemies.get(&id)?.current_location,
            Self::Act | Self::Agenda => None,
        }
    }

    /// Whether the card sits in the area of an investigator who has been
    /// eliminated — any status but `Active`. Rules Reference p.10 removes an
    /// eliminated investigator's cards from play, all but the investigator card,
    /// and this is the filter that keeps that card, and anything still on the
    /// board in the step-0 window, from acting (#567).
    #[must_use]
    pub fn in_eliminated_area(self, state: &GameState) -> bool {
        self.investigator()
            .and_then(|id| state.investigators.get(&id))
            .is_some_and(|inv| inv.status != Status::Active)
    }
}

/// The record behind a card on the board, whichever kind of thing it is.
///
/// Callers need four things from one — its card code, whether it is exhausted,
/// its remaining uses, and its card instance (if it has one) — and only the
/// first is available uniformly. A location is a [`Location`] keyed by
/// `LocationId`, an enemy is an [`Enemy`] keyed by `EnemyId`, and neither
/// carries the per-instance state a [`CardInPlay`] does. Answering all four
/// here is what keeps every caller from re-deriving "does this kind of card
/// have an instance behind it".
#[derive(Debug, Clone, Copy)]
pub(crate) enum SourceCard<'a> {
    /// A card instance in play — an investigator card, a card in play, a
    /// threat-area card, a card attached to or put into play at a location, or
    /// an attachment on an enemy.
    Instance(&'a CardInPlay),
    /// A location card itself.
    Location(&'a Location),
    /// An enemy in play.
    Enemy(&'a Enemy),
    /// The current act card.
    Act(&'a Act),
    /// The current agenda card.
    Agenda(&'a Agenda),
}

impl<'a> SourceCard<'a> {
    /// The printed code the card's abilities are looked up by in the card
    /// registry.
    pub(crate) fn code(&self) -> &'a CardCode {
        match *self {
            SourceCard::Instance(card) => &card.code,
            SourceCard::Location(location) => &location.code,
            SourceCard::Enemy(enemy) => &enemy.code,
            SourceCard::Act(act) => &act.code,
            SourceCard::Agenda(agenda) => &agenda.code,
        }
    }

    /// The card instance behind this card, if it has one. `None` for a
    /// location (locations do not exhaust and carry no uses); for an enemy — an
    /// enemy readies and exhausts through its own `exhausted` field, which is
    /// not the card-instance state an `Exhaust` cost pays against; and for the
    /// act and the agenda, which are `Act` / `Agenda` records in the scenario
    /// decks and carry no per-instance state at all.
    pub(crate) fn instance(&self) -> Option<&'a CardInPlay> {
        match *self {
            SourceCard::Instance(card) => Some(card),
            SourceCard::Location(_)
            | SourceCard::Enemy(_)
            | SourceCard::Act(_)
            | SourceCard::Agenda(_) => None,
        }
    }

    /// Whether an `Exhaust` cost is already spent on this card. Only a card
    /// instance can carry one; see [`instance`](Self::instance).
    pub(crate) fn exhausted(&self) -> bool {
        self.instance().is_some_and(|card| card.exhausted)
    }

    /// Remaining uses by kind — empty for a card with no card instance.
    pub(crate) fn uses(&self) -> BTreeMap<UseKind, u8> {
        self.instance()
            .map(|card| card.uses.clone())
            .unwrap_or_default()
    }
}

/// One card the [`walk`] visits: the record behind it, and where it sits.
#[derive(Debug, Clone, Copy)]
pub(crate) struct BoardCard<'a> {
    /// The record behind the card.
    pub(crate) card: SourceCard<'a>,
    /// Where the card sits.
    pub(crate) placement: Placement,
    /// The [`AbilitySource`] that names the card's abilities.
    pub(crate) source: AbilitySource,
}

impl<'a> BoardCard<'a> {
    /// The card's code.
    pub(crate) fn code(&self) -> &'a CardCode {
        self.card.code()
    }
}

/// Every card on the board, each with its [`Placement`], in ADR 0018's order —
/// see the [module docs](self). Unfiltered: an eliminated investigator's cards
/// are on it, and each caller decides whether to skip them.
pub(crate) fn walk(state: &GameState) -> Vec<BoardCard<'_>> {
    fn instances(
        cards: &[CardInPlay],
        placement: Placement,
    ) -> impl Iterator<Item = BoardCard<'_>> {
        cards.iter().map(move |card| BoardCard {
            card: SourceCard::Instance(card),
            placement,
            source: AbilitySource::InPlay(card.instance_id),
        })
    }
    let mut walked = Vec::new();
    for id in investigator_order(state) {
        let Some(inv) = state.investigators.get(&id) else {
            continue;
        };
        walked.push(BoardCard {
            card: SourceCard::Instance(&inv.investigator_card),
            placement: Placement::InvestigatorCard(id),
            source: AbilitySource::InPlay(inv.investigator_card.instance_id),
        });
        walked.extend(instances(&inv.cards_in_play, Placement::PlayArea(id)));
        walked.extend(instances(&inv.threat_area, Placement::ThreatArea(id)));
    }
    for (&id, location) in &state.locations {
        walked.push(BoardCard {
            card: SourceCard::Location(location),
            placement: Placement::Location(id),
            source: AbilitySource::Location(id),
        });
        walked.extend(instances(
            &location.attachments,
            Placement::LocationAttachment(id),
        ));
        walked.extend(instances(
            &location.cards_at_location,
            Placement::AtLocation(id),
        ));
    }
    for (&id, enemy) in &state.enemies {
        walked.push(BoardCard {
            card: SourceCard::Enemy(enemy),
            placement: Placement::Enemy(id),
            source: AbilitySource::Enemy(id),
        });
        walked.extend(instances(
            &enemy.attachments,
            Placement::EnemyAttachment(id),
        ));
    }
    if let Some(act) = state.act_deck.get(state.act_index) {
        walked.push(BoardCard {
            card: SourceCard::Act(act),
            placement: Placement::Act,
            source: AbilitySource::Act,
        });
    }
    if let Some(agenda) = state.agenda_deck.get(state.agenda_index) {
        walked.push(BoardCard {
            card: SourceCard::Agenda(agenda),
            placement: Placement::Agenda,
            source: AbilitySource::Agenda,
        });
    }
    walked
}

/// The [`walk`] less the cards in an eliminated investigator's area
/// ([`Placement::in_eliminated_area`]): the cards still acting on the game. The
/// modifier sweep and the grant sweep read it. The trigger scan does not,
/// because it keeps the cards of the investigator whose game-end weaknesses
/// resolve at elimination step 0.
pub(crate) fn walk_active(state: &GameState) -> Vec<BoardCard<'_>> {
    let mut walked = walk(state);
    walked.retain(|card| !card.placement.in_eliminated_area(state));
    walked
}

/// The order the [`walk`] visits investigators in: the active investigator,
/// then the rest of `turn_order`, then every other investigator by id — so an
/// investigator a fixture never seated in `turn_order` is still walked, last.
pub(crate) fn investigator_order(state: &GameState) -> Vec<InvestigatorId> {
    let mut order: Vec<InvestigatorId> = state.active_investigator.into_iter().collect();
    for id in state.turn_order.iter().chain(state.investigators.keys()) {
        if !order.contains(id) {
            order.push(*id);
        }
    }
    order
}

/// The card instance `instance_id` names, wherever it sits on the board, with
/// its [`Placement`]. `None` once it has left play.
///
/// The first instance on the board walk with that id, unfiltered: every
/// investigator's investigator card, play area and threat area (eliminated
/// investigators included), every location's attachments and the cards put
/// into play at it, and every enemy's attachments.
#[must_use]
pub fn find_instance(
    state: &GameState,
    instance_id: CardInstanceId,
) -> Option<(&CardInPlay, Placement)> {
    walk(state).into_iter().find_map(|walked| {
        walked
            .card
            .instance()
            .filter(|card| card.instance_id == instance_id)
            .map(|card| (card, walked.placement))
    })
}

/// The mutable twin of [`find_instance`]: the same instance, found by the same
/// walk, then borrowed mutably out of the zone its placement names.
///
/// Addressing by identity rather than by position is the #706 contract: a cost
/// that removes a card mid-payment invalidates any position cached earlier, and
/// this returns `None` in exactly that case.
#[must_use]
pub fn find_instance_mut(
    state: &mut GameState,
    instance_id: CardInstanceId,
) -> Option<(&mut CardInPlay, Placement)> {
    let (_, placement) = find_instance(state, instance_id)?;
    if let Placement::InvestigatorCard(id) = placement {
        let card = &mut state.investigators.get_mut(&id)?.investigator_card;
        return Some((card, placement));
    }
    instance_zone_mut(state, placement)?
        .iter_mut()
        .find(|card| card.instance_id == instance_id)
        .map(|card| (card, placement))
}

/// Take the card instance `instance_id` off the board, with the [`Placement`]
/// it left. `None` when it is not on the board, and for an investigator card,
/// which is not in a zone a card can be taken out of.
///
/// The removal half of the leave-play exits in [`leave_play`]; a card taken
/// out here and not filed somewhere is a card lost, so nothing else calls it.
fn take_instance(
    state: &mut GameState,
    instance_id: CardInstanceId,
) -> Option<(CardInPlay, Placement)> {
    let (_, placement) = find_instance(state, instance_id)?;
    let zone = instance_zone_mut(state, placement)?;
    let index = zone
        .iter()
        .position(|card| card.instance_id == instance_id)?;
    Some((zone.remove(index), placement))
}

/// The zone of card instances a [`Placement`] names, or `None` for a placement
/// that is not one: an investigator card, a location, an enemy, the act and the
/// agenda.
fn instance_zone_mut(state: &mut GameState, placement: Placement) -> Option<&mut Vec<CardInPlay>> {
    match placement {
        Placement::PlayArea(id) => Some(&mut state.investigators.get_mut(&id)?.cards_in_play),
        Placement::ThreatArea(id) => Some(&mut state.investigators.get_mut(&id)?.threat_area),
        Placement::LocationAttachment(id) => Some(&mut state.locations.get_mut(&id)?.attachments),
        Placement::AtLocation(id) => Some(&mut state.locations.get_mut(&id)?.cards_at_location),
        Placement::EnemyAttachment(id) => Some(&mut state.enemies.get_mut(&id)?.attachments),
        Placement::InvestigatorCard(_)
        | Placement::Location(_)
        | Placement::Enemy(_)
        | Placement::Act
        | Placement::Agenda => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{GameStateBuilder, Owner};
    use crate::test_support;

    const STUDY: LocationId = LocationId(1);
    const HALLWAY: LocationId = LocationId(2);
    const ROLAND: InvestigatorId = InvestigatorId(1);
    const GHOUL: EnemyId = EnemyId(1);

    fn card(code: &str, instance: u32, owner: Owner) -> CardInPlay {
        CardInPlay::enter_play(CardCode::new(code), CardInstanceId(instance), owner)
    }

    /// One card in every zone: investigator 1's investigator card (10), play
    /// area (11) and threat area (12); the Study's attachment (40) and the card
    /// put into play there (60); and an attachment (50) on a ghoul in the
    /// Hallway.
    fn board() -> GameState {
        let mut roland = test_support::test_investigator(1);
        roland.investigator_card.instance_id = CardInstanceId(10);
        roland
            .cards_in_play
            .push(card("01020", 11, Owner::Investigator(ROLAND)));
        roland
            .threat_area
            .push(card("01007", 12, Owner::Investigator(ROLAND)));

        let mut study = test_support::test_location(1, "Study");
        study
            .attachments
            .push(card("01168", 40, Owner::EncounterDeck));
        study
            .cards_at_location
            .push(card("01117", 60, Owner::Scenario));

        let mut ghoul = test_support::test_enemy(1, "Ghoul");
        ghoul.current_location = Some(HALLWAY);
        ghoul
            .attachments
            .push(card("02256", 50, Owner::EncounterDeck));

        GameStateBuilder::new()
            .with_investigator_at(roland, STUDY)
            .with_location(study)
            .with_location(test_support::test_location(2, "Hallway"))
            .with_enemy(ghoul)
            .build()
    }

    /// Every zone's instance, with the code it carries and the placement it
    /// must report.
    const EVERY_ZONE: [(u32, &str, Placement); 6] = [
        (10, "TEST_INV", Placement::InvestigatorCard(ROLAND)),
        (11, "01020", Placement::PlayArea(ROLAND)),
        (12, "01007", Placement::ThreatArea(ROLAND)),
        (40, "01168", Placement::LocationAttachment(STUDY)),
        (60, "01117", Placement::AtLocation(STUDY)),
        (50, "02256", Placement::EnemyAttachment(GHOUL)),
    ];

    #[test]
    fn find_instance_finds_a_card_in_every_zone_and_reports_its_placement() {
        let state = board();
        for (instance, code, placement) in EVERY_ZONE {
            let (found, at) = find_instance(&state, CardInstanceId(instance))
                .unwrap_or_else(|| panic!("instance {instance} is on the board"));
            assert_eq!(found.code.as_str(), code, "instance {instance}");
            assert_eq!(at, placement, "instance {instance}");
        }
    }

    #[test]
    fn find_instance_mut_reaches_a_card_in_every_zone_and_reports_its_placement() {
        let mut state = board();
        for (instance, _, placement) in EVERY_ZONE {
            let (found, at) = find_instance_mut(&mut state, CardInstanceId(instance))
                .unwrap_or_else(|| panic!("instance {instance} is on the board"));
            found.clues = 1;
            assert_eq!(at, placement, "instance {instance}");
        }
        for (instance, _, _) in EVERY_ZONE {
            assert_eq!(
                find_instance(&state, CardInstanceId(instance)).map(|(card, _)| card.clues),
                Some(1),
                "the write landed on instance {instance}",
            );
        }
    }

    /// ADR 0018's order: the active investigator's cards first, then the rest
    /// of `turn_order`; each location with its attachments and the cards put
    /// into play at it; each enemy with its attachments; the act; the agenda.
    #[test]
    fn the_walk_visits_every_card_in_adr_0018_order() {
        const SECOND: InvestigatorId = InvestigatorId(2);
        let mut state = board();
        let mut second = test_support::test_investigator(2);
        second.investigator_card.instance_id = CardInstanceId(20);
        state.investigators.insert(SECOND, second);
        state.turn_order = vec![ROLAND, SECOND];
        state.active_investigator = Some(SECOND);
        state.act_deck = vec![Act {
            code: CardCode::new("01108"),
            clue_threshold: 2,
        }];
        state.agenda_deck = vec![Agenda {
            code: CardCode::new("01105"),
            doom_threshold: 3,
        }];

        let walked: Vec<_> = walk(&state)
            .iter()
            .map(|card| (card.placement, card.source))
            .collect();
        let instance = |id| AbilitySource::InPlay(CardInstanceId(id));
        assert_eq!(
            walked,
            vec![
                (Placement::InvestigatorCard(SECOND), instance(20)),
                (Placement::InvestigatorCard(ROLAND), instance(10)),
                (Placement::PlayArea(ROLAND), instance(11)),
                (Placement::ThreatArea(ROLAND), instance(12)),
                (Placement::Location(STUDY), AbilitySource::Location(STUDY)),
                (Placement::LocationAttachment(STUDY), instance(40)),
                (Placement::AtLocation(STUDY), instance(60)),
                (
                    Placement::Location(HALLWAY),
                    AbilitySource::Location(HALLWAY)
                ),
                (Placement::Enemy(GHOUL), AbilitySource::Enemy(GHOUL)),
                (Placement::EnemyAttachment(GHOUL), instance(50)),
                (Placement::Act, AbilitySource::Act),
                (Placement::Agenda, AbilitySource::Agenda),
            ],
        );
    }

    /// Eliminated investigators are on the walk; each caller filters them.
    #[test]
    fn a_card_in_an_eliminated_investigators_area_is_flagged_and_no_other_is() {
        let mut state = board();
        let flagged = |state: &GameState| -> Vec<bool> {
            walk(state)
                .iter()
                .map(|card| card.placement.in_eliminated_area(state))
                .collect()
        };
        assert!(flagged(&state).iter().all(|f| !f));
        state
            .investigators
            .get_mut(&ROLAND)
            .expect("Roland is on the board")
            .status = Status::Resigned;
        assert_eq!(
            flagged(&state),
            vec![true, true, true, false, false, false, false, false, false],
            "Roland's investigator card, play area and threat area; nothing else",
        );
    }

    #[test]
    fn find_instance_is_none_for_a_card_not_on_the_board() {
        let mut state = board();
        assert!(find_instance(&state, CardInstanceId(99)).is_none());
        assert!(find_instance_mut(&mut state, CardInstanceId(99)).is_none());
    }

    /// Unfiltered by status: Cover Up's trauma resolves at elimination step 0,
    /// after its holder has left `Active`.
    #[test]
    fn find_instance_finds_a_card_in_an_eliminated_investigators_threat_area() {
        let mut state = board();
        state
            .investigators
            .get_mut(&ROLAND)
            .expect("Roland is on the board")
            .status = Status::Defeated;
        let (_, at) = find_instance(&state, CardInstanceId(12)).expect("still on the board");
        assert_eq!(at, Placement::ThreatArea(ROLAND));
    }

    #[test]
    fn a_placement_names_the_investigator_whose_area_holds_the_card() {
        let state = board();
        let investigators: Vec<_> = EVERY_ZONE
            .iter()
            .map(|(instance, _, _)| {
                find_instance(&state, CardInstanceId(*instance))
                    .and_then(|(_, at)| at.investigator())
            })
            .collect();
        assert_eq!(
            investigators,
            vec![Some(ROLAND), Some(ROLAND), Some(ROLAND), None, None, None],
        );
    }

    #[test]
    fn a_placement_names_the_location_the_card_is_at() {
        let state = board();
        let locations: Vec<_> = EVERY_ZONE
            .iter()
            .map(|(instance, _, _)| {
                find_instance(&state, CardInstanceId(*instance))
                    .and_then(|(_, at)| at.location(&state))
            })
            .collect();
        assert_eq!(
            locations,
            vec![
                Some(STUDY),
                Some(STUDY),
                Some(STUDY),
                Some(STUDY),
                Some(STUDY),
                Some(HALLWAY),
            ],
        );
    }
}
