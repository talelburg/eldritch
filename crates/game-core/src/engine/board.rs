//! The board as a set of zones a card instance can sit in, and the one way to
//! find an instance in them.
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

use crate::state::{CardInPlay, CardInstanceId, EnemyId, GameState, InvestigatorId, LocationId};

/// Where a card instance sits on the board: which zone, and whose or which
/// location's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    /// The investigator's own investigator card.
    InvestigatorCard(InvestigatorId),
    /// In the investigator's play area (`Investigator::cards_in_play`).
    PlayArea(InvestigatorId),
    /// In the investigator's threat area.
    ThreatArea(InvestigatorId),
    /// Attached to a location (Obscuring Fog 01168).
    LocationAttachment(LocationId),
    /// Put into play **at** a location and controlled by nobody (Lita Chantler
    /// 01117 in the Parlor).
    AtLocation(LocationId),
    /// Attached to an enemy.
    EnemyAttachment(EnemyId),
}

impl Placement {
    /// The investigator whose area holds the card — their investigator card,
    /// play area or threat area — or `None` for a card at a location or on an
    /// enemy.
    ///
    /// This is the investigator the engine treats as the card's controller, and
    /// the "you" of a card in a threat area: `glossary/You_Your.md`, *"the
    /// investigator who has the card in his/her threat area"*.
    #[must_use]
    pub fn investigator(self) -> Option<InvestigatorId> {
        match self {
            Self::InvestigatorCard(id) | Self::PlayArea(id) | Self::ThreatArea(id) => Some(id),
            Self::LocationAttachment(_) | Self::AtLocation(_) | Self::EnemyAttachment(_) => None,
        }
    }

    /// The location the card is at: the location it is attached to or put
    /// into play at, the location of the investigator whose area holds it, or
    /// the location of the enemy it is attached to. `None` when that
    /// investigator or enemy is at no location.
    #[must_use]
    pub fn location(self, state: &GameState) -> Option<LocationId> {
        match self {
            Self::InvestigatorCard(id) | Self::PlayArea(id) | Self::ThreatArea(id) => {
                state.investigators.get(&id)?.current_location
            }
            Self::LocationAttachment(id) | Self::AtLocation(id) => Some(id),
            Self::EnemyAttachment(id) => state.enemies.get(&id)?.current_location,
        }
    }
}

/// The card instance `instance_id` names, wherever it sits on the board, with
/// its [`Placement`]. `None` once it has left play.
///
/// Searches every investigator's investigator card, play area and threat area
/// (eliminated investigators included), every location's attachments and the
/// cards put into play at it, and every enemy's attachments.
#[must_use]
pub fn find_instance(
    state: &GameState,
    instance_id: CardInstanceId,
) -> Option<(&CardInPlay, Placement)> {
    let is_it = |card: &&CardInPlay| card.instance_id == instance_id;
    for (&id, inv) in &state.investigators {
        if inv.investigator_card.instance_id == instance_id {
            return Some((&inv.investigator_card, Placement::InvestigatorCard(id)));
        }
        if let Some(card) = inv.cards_in_play.iter().find(is_it) {
            return Some((card, Placement::PlayArea(id)));
        }
        if let Some(card) = inv.threat_area.iter().find(is_it) {
            return Some((card, Placement::ThreatArea(id)));
        }
    }
    for (&id, location) in &state.locations {
        if let Some(card) = location.attachments.iter().find(is_it) {
            return Some((card, Placement::LocationAttachment(id)));
        }
        if let Some(card) = location.cards_at_location.iter().find(is_it) {
            return Some((card, Placement::AtLocation(id)));
        }
    }
    state.enemies.iter().find_map(|(&id, enemy)| {
        enemy
            .attachments
            .iter()
            .find(is_it)
            .map(|card| (card, Placement::EnemyAttachment(id)))
    })
}

/// The mutable twin of [`find_instance`]: the same zones, searched in the same
/// order.
///
/// Addressing by identity rather than by position is the #706 contract: a cost
/// that removes a card mid-payment invalidates any position cached earlier, and
/// this returns `None` in exactly that case.
#[must_use]
pub fn find_instance_mut(
    state: &mut GameState,
    instance_id: CardInstanceId,
) -> Option<(&mut CardInPlay, Placement)> {
    let is_it = |card: &&mut CardInPlay| card.instance_id == instance_id;
    for (&id, inv) in &mut state.investigators {
        if inv.investigator_card.instance_id == instance_id {
            return Some((&mut inv.investigator_card, Placement::InvestigatorCard(id)));
        }
        if let Some(card) = inv.cards_in_play.iter_mut().find(is_it) {
            return Some((card, Placement::PlayArea(id)));
        }
        if let Some(card) = inv.threat_area.iter_mut().find(is_it) {
            return Some((card, Placement::ThreatArea(id)));
        }
    }
    for (&id, location) in &mut state.locations {
        if let Some(card) = location.attachments.iter_mut().find(is_it) {
            return Some((card, Placement::LocationAttachment(id)));
        }
        if let Some(card) = location.cards_at_location.iter_mut().find(is_it) {
            return Some((card, Placement::AtLocation(id)));
        }
    }
    state.enemies.iter_mut().find_map(|(&id, enemy)| {
        enemy
            .attachments
            .iter_mut()
            .find(is_it)
            .map(|card| (card, Placement::EnemyAttachment(id)))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{CardCode, GameStateBuilder, Owner, Status};
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
