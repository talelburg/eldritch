//! Taking control of a card that is already in play (#772).
//!
//! `glossary/Ownership_and_Control.md` splits the two properties a card carries
//! at once:
//!
//! > A card's owner is the player whose deck (or game area) held the card at
//! > the start of the game.
//!
//! > - Cards by default enter play under their owner's control. Some abilities
//! >   may cause cards to change control during a game.
//!
//! **Control is which collection the instance sits in; ownership is a fact
//! about where it came from, and taking control does not touch it.** Lita
//! Chantler 01117's rulings say exactly that (<https://arkhamdb.com/card/01117>):
//! *"When you 'take control' of a card, it enters your play area (not your
//! hand)."*, and *"You take control of Lita only **temporarily**, until the end
//! of the scenario. Taking control of her doesn't make her a part of your
//! deck."*
//!
//! So this module **moves the instance** rather than minting a fresh one: the
//! same [`CardInPlay`] leaves the zone it was in and lands in the taker's
//! `cards_in_play`, carrying its accumulated damage and horror, its uses pool
//! and its per-ability usage counters with it. What it does *not* carry is a
//! new owner, which is what later routes it out of the game rather than into a
//! discard pile (`cards::discard_card_from_play`).

use crate::event::Event;
use crate::state::{AssetEntry, CardInPlay, InvestigatorId};

use super::Cx;
use crate::engine::EngineOutcome;

/// Resolve [`Effect::TakeControl`](crate::dsl::Effect::TakeControl): move the
/// in-play card printed with `code` into `investigator`'s play area.
///
/// Validate-first. Rejects, mutating nothing, when the investigator is not in
/// state or when no in-play card carries `code`. Taking control of a card the
/// investigator already controls is a no-op rather than a rejection — the
/// effect's whole content is *"be the controller"*, and they already are.
///
/// Entry raises the slot make-room prompt where it is needed, which is why the
/// outcome can be an `AwaitingInput`: `glossary/Slots.md` puts playing and
/// gaining control under one sentence — *"If playing **or gaining control** of
/// an asset would put an investigator above his or her slot limit for that type
/// of asset, the investigator must choose and discard other assets under his or
/// her control simultaneously with the new asset entering the slot."*
pub(crate) fn take_control(cx: &mut Cx, investigator: InvestigatorId, code: &str) -> EngineOutcome {
    if !cx.state.investigators.contains_key(&investigator) {
        return EngineOutcome::Rejected {
            reason: format!("TakeControl: investigator {investigator:?} is not in state").into(),
        };
    }
    if cx
        .state
        .investigators
        .get(&investigator)
        .is_some_and(|inv| inv.cards_in_play.iter().any(|c| c.code.as_str() == code))
    {
        return EngineOutcome::Done;
    }
    let Some(card) = lift_in_play_card(cx, code) else {
        return EngineOutcome::Rejected {
            reason: format!("TakeControl: no card {code} is in play to take control of").into(),
        };
    };
    cx.events.push(Event::ControlTaken {
        investigator,
        code: card.code.clone(),
        instance_id: card.instance_id,
    });
    super::slots::enter_asset_making_room(cx, investigator, card, AssetEntry::ControlTaken)
}

/// Take the in-play instance printed with `code` out of the zone that holds it,
/// leaving it in **no zone** — the caller places it (ADR 0002's shape, on a much
/// shorter leash: the hand-off is immediate).
///
/// Two zones can yield one, and both are control changes the corpus can reach:
/// a card put into play **at a location** under nobody's control (Lita Chantler
/// 01117 in the Parlor, #825), and a card **another investigator controls** —
/// nothing in Core takes control off a teammate, but the verb means the same
/// thing when something does, and leaving that arm out would make it a silent
/// rejection rather than a considered one.
///
/// Attachments are deliberately not searched: `glossary/Attach_To.md` binds an
/// attachment's lifetime to its host, and no card prints *"take control of"* an
/// attached card.
fn lift_in_play_card(cx: &mut Cx, code: &str) -> Option<CardInPlay> {
    for location in cx.state.locations.values_mut() {
        if let Some(pos) = location
            .cards_at_location
            .iter()
            .position(|c| c.code.as_str() == code)
        {
            return Some(location.cards_at_location.remove(pos));
        }
    }
    for inv in cx.state.investigators.values_mut() {
        if let Some(pos) = inv
            .cards_in_play
            .iter()
            .position(|c| c.code.as_str() == code)
        {
            return Some(inv.cards_in_play.remove(pos));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{CardCode, CardInstanceId, GameStateBuilder, LocationId};
    use crate::test_support::{test_investigator, test_location};

    const INV: InvestigatorId = InvestigatorId(1);
    const HERE: LocationId = LocationId(1);
    const CODE: &str = "TAKECTRL";
    const INST: CardInstanceId = CardInstanceId(9);

    /// A board with one card in play *at* a location under nobody's control.
    fn board_with_uncontrolled_card() -> crate::state::GameState {
        let mut state = GameStateBuilder::new()
            .with_investigator_at(test_investigator(1), HERE)
            .with_location(test_location(1, "Study"))
            .build();
        state
            .locations
            .get_mut(&HERE)
            .expect("HERE is on the board")
            .cards_at_location
            .push(CardInPlay::enter_play(CardCode::new(CODE), INST));
        state
    }

    fn run(state: &mut crate::state::GameState, code: &str) -> (EngineOutcome, Vec<crate::Event>) {
        let mut events = Vec::new();
        let outcome = take_control(
            &mut Cx {
                state,
                events: &mut events,
            },
            INV,
            code,
        );
        (outcome, events)
    }

    /// The instance **moves**: it leaves the location's zone and lands in the
    /// taker's play area, keeping its id (and everything hanging off it).
    #[test]
    fn the_same_instance_moves_into_the_takers_play_area() {
        let mut state = board_with_uncontrolled_card();
        let (outcome, events) = run(&mut state, CODE);

        assert_eq!(outcome, EngineOutcome::Done);
        assert!(state.locations[&HERE].cards_at_location.is_empty());
        let in_play = &state.investigators[&INV].cards_in_play;
        assert_eq!(in_play.len(), 1);
        assert_eq!(in_play[0].instance_id, INST, "the same instance moved");
        assert!(events
            .iter()
            .any(|e| matches!(e, Event::ControlTaken { instance_id, .. } if *instance_id == INST)));
    }

    /// **Ownership is untouched.** *"Taking control of her doesn't make her a
    /// part of your deck."* — so the card stays scenario-owned, which is what
    /// later routes it out of the game rather than into a discard pile.
    #[test]
    fn taking_control_does_not_change_the_owner() {
        let mut state = board_with_uncontrolled_card();
        let (outcome, _) = run(&mut state, CODE);

        assert_eq!(outcome, EngineOutcome::Done);
        assert_eq!(
            state.investigators[&INV].cards_in_play[0].owner, None,
            "the card is still scenario-owned",
        );
    }

    /// Taking control of a card the investigator already controls is a no-op,
    /// not a rejection: the effect's whole content is *"be the controller"*.
    #[test]
    fn taking_control_of_a_card_you_already_control_is_a_no_op() {
        let mut state = board_with_uncontrolled_card();
        let (first, _) = run(&mut state, CODE);
        assert_eq!(first, EngineOutcome::Done);
        let (second, events) = run(&mut state, CODE);

        assert_eq!(second, EngineOutcome::Done);
        assert!(events.is_empty(), "nothing happened, got {events:?}");
        assert_eq!(state.investigators[&INV].cards_in_play.len(), 1);
    }

    /// A card that is not in play cannot be taken control of — the effect
    /// rejects rather than minting one by code.
    #[test]
    fn taking_control_of_a_card_that_is_not_in_play_rejects() {
        let mut state = board_with_uncontrolled_card();
        let (outcome, events) = run(&mut state, "NOTHERE");

        assert!(matches!(outcome, EngineOutcome::Rejected { .. }));
        assert!(events.is_empty(), "a rejection mutates nothing");
        assert_eq!(state.locations[&HERE].cards_at_location.len(), 1);
    }
}
