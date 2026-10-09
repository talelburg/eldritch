//! Reachability: which [`AbilitySource`]s a given investigator may use an
//! ability from (#707, #708, #709).
//!
//! The Rules Reference answers this once for `[free]`, `[reaction]` and
//! `[action]` abilities alike — `glossary/Triggered_Abilities.md`'s four
//! bullets, quoted on [`AbilitySource`]. This module is the engine's single
//! answer to them, so its three consumers cannot drift apart:
//! [`reachable_sources`] *is* the predicate, and [`resolve`] is a lookup in it
//! rather than a second reading of the rules.
//!
//! The three are the activation validator (`check_activate_ability`), the
//! turn-menu enumerator (`enumerate::push_card_actions`) and the player-window
//! enumerator (`reaction_windows::enumerate_fast_plays`). The last is why the
//! bullets being written once matters: a zero-action ability — the `[free]`
//! icon, *"a free triggered ability that does not cost an action and may be
//! used during any player window"* — on a location, an enemy, a co-located
//! threat area, the act or the agenda reaches a player window on exactly the
//! terms an action-costed one reaches the turn menu, because both ask this
//! function (#710).
//!
//! Three bullets are implemented. The **control** bullet — *"A card in play and
//! under his or her control. This includes his or her investigator card."* —
//! and the **co-location** bullet, verbatim:
//!
//! > A scenario card that is in play and at the same location as the
//! > investigator. This includes the location itself, encounter cards placed at
//! > that location, and all encounter cards in the threat area of any
//! > investigator at that location.
//!
//! The co-location bullet is **not** controller-scoped: *"any investigator at
//! that location"* means another investigator's threat area is reachable, which
//! Haunted 01098's ruling states directly (<https://arkhamdb.com/card/01098>):
//!
//! > Any investigator at the same location as the investigator with Haunted in
//! > their threat area may trigger the \[action\]\[action\] to discard Haunted, as
//! > per the FAQ \[V1.0, section 2.1\].
//!
//! The third bullet lands here too (#709) — *"The current act or current agenda
//! card."* It is the one bullet with **no gate at all**: not control, not
//! co-location. Disrupting the Ritual 01148's ruling says so about the printed
//! card (<https://arkhamdb.com/card/01148>), verbatim:
//!
//! > Your investigator doesn't need to be at the Ritual Site in order to
//! > activate the ability of this act card.
//!
//! An investigator therefore reaches the current act and the current agenda from
//! wherever they stand, which is why [`reachable_sources`] gates them on
//! nothing while every co-location arm is gated on standing somewhere.
//!
//! Reachability says only *which sources are addressable*. It never widens what
//! is **legal**: everything `Appendix_I_Initiation_Sequence.md` requires still
//! runs afterwards in `check_activate_ability` — *"determine if the card can be
//! played, or if the ability can be initiated, at this time. (This includes
//! verifying that the resolution of the effect has the potential to change the
//! game state.)"*, and that the cost can be paid.

use std::borrow::Cow;

use card_dsl::card_data::CardType;

use crate::card_registry;
use crate::engine::board::{self, BoardCard, Placement, SourceCard};
use crate::state::{AbilitySource, CardCode, CardInPlay, GameState, InvestigatorId, LocationId};

/// Every ability source `investigator` can reach, paired with the record that
/// carries the abilities.
///
/// It is the [board walk](board::walk) filtered by the three bullets, so it is
/// in the walk's order (ADR 0018) — every investigator's cards first, the
/// active investigator's leading, then each location with its cards, each enemy
/// with its attachments, the act and the agenda. The order is what the turn menu
/// and the player window list options in, so it must stay deterministic, which
/// the walk is. Options are routed per board card, so the order *across* cards
/// is presentation only; the order of one card's abilities is the card's own.
///
/// - **Control:** the investigator's own investigator card, play area and
///   threat area.
/// - **Co-location**, for an investigator standing at a location on the map:
///   the location itself, its attachments, the cards put into play at it, each
///   enemy at it with its attachments, and the **encounter** cards in the
///   threat areas of the other investigators there.
/// - **The current act and agenda**, from anywhere.
///
/// The acting investigator's own threat area is yielded once, under the control
/// bullet.
///
/// Empty for an investigator who is not in `state`. An investigator who is not
/// at a location (one in the setup phase, or one who has left the board) skips
/// the co-location bullet and **keeps** the act and the agenda: that bullet is
/// gated on nothing.
///
/// Not filtered by elimination: an investigator who has left the board is at no
/// location, so co-location already reaches nothing of theirs, and their own
/// control bullet answers an activation they cannot take anyway.
pub(crate) fn reachable_sources(
    state: &GameState,
    investigator: InvestigatorId,
) -> Vec<(AbilitySource, SourceCard<'_>)> {
    let Some(inv) = state.investigators.get(&investigator) else {
        return Vec::new();
    };
    // Co-location is gated on standing at a location that is on the map.
    let here = inv
        .current_location
        .filter(|id| state.locations.contains_key(id));
    board::walk(state)
        .into_iter()
        .filter(|card| reaches(state, investigator, here, card))
        .map(|card| (card.source, card.card))
        .collect()
}

/// Whether `investigator`, standing at `here`, reaches the walked `card` under
/// one of the three bullets.
fn reaches(
    state: &GameState,
    investigator: InvestigatorId,
    here: Option<LocationId>,
    card: &BoardCard<'_>,
) -> bool {
    match card.placement {
        // The control bullet: *"A card in play and under his or her control.
        // This includes his or her investigator card."*
        //
        // It is a slightly *wider* set than "a card in play and under his or
        // her control", and deliberately so for this use: the threat area also
        // holds **encounter** cards, which the scenario owns and controls.
        // Frozen in Fear 01164 is not one of your cards — per the official FAQ,
        // *"In general, 'your cards' are the cards you currently control. If you
        // own a card but do not control it, it is not 'yours' for the purposes
        // of abilities."* (`data/official-faq/Frequently_Asked_Questions.md`.)
        // Reaching such a card's ability from this seat is still right, because
        // a threat-area encounter card's Forced abilities are the acting
        // investigator's to resolve.
        Placement::InvestigatorCard(id) | Placement::PlayArea(id) | Placement::ThreatArea(id)
            if id == investigator =>
        {
            true
        }
        // "all encounter cards in the threat area of any investigator at that
        // location" — *any*, so this is other people's threat areas too
        // (Haunted 01098's ruling), but only their **encounter** cards (#975):
        // a player card there is its bearer's alone.
        Placement::ThreatArea(_) => {
            here.is_some() && card.placement.location(state) == here && is_encounter_card(card)
        }
        // Co-location is not control: a co-located investigator's own
        // investigator card and assets are theirs alone.
        Placement::InvestigatorCard(_) | Placement::PlayArea(_) => false,
        // *"The current act or current agenda card."* (#709) — the one bullet
        // with no gate at all, so an investigator between locations still
        // reaches both.
        Placement::Act | Placement::Agenda => true,
        // "the location itself", and "encounter cards placed at that location":
        // its attachments (Obscuring Fog 01168), the cards put into play at it
        // (Lita Chantler 01117, whom nobody controls, so this is the only way
        // she is reached at all, #771), and the enemies standing on it with
        // their attachments — the Parley abilities are printed on enemies
        // (Herman Collins 01138, Mob Enforcer 01101).
        //
        // The bullet says *scenario* card, and attachments are taken
        // unfiltered: `Effect::AttachSelfToLocation` has one caller in the
        // corpus and it is an encounter card, so no player card can sit in
        // either collection today. The day one can — an attaching player asset
        // — this is where the encounter / player distinction goes, the way the
        // threat-area arm above already draws it.
        Placement::Location(_)
        | Placement::LocationAttachment(_)
        | Placement::AtLocation(_)
        | Placement::Enemy(_)
        | Placement::EnemyAttachment(_) => here.is_some() && card.placement.location(state) == here,
    }
}

/// Whether the card has an encounter cardtype, read through the registry. A
/// card the registry has no metadata for counts as one, matching the other
/// metadata fallbacks: it is excluded only when it is known to be a player card.
fn is_encounter_card(card: &BoardCard<'_>) -> bool {
    let Some(meta) = card_registry::current().and_then(|reg| (reg.metadata_for)(card.code()))
    else {
        return true;
    };
    !matches!(
        meta.card_type(),
        CardType::Investigator | CardType::Asset | CardType::Event | CardType::Skill
    )
}

/// The card `source` names right now, wherever it sits — **existence, not
/// reachability** (#735). `None` once the source has left the board.
///
/// The deliberate contrast with [`reachable_sources`], which sits directly
/// above: that predicate answers *"may **this investigator** use an ability
/// from this source"*, and is gated on control and co-location. This one
/// answers only *"what is the source now"*, because its caller is the forced /
/// reaction path (`reaction_windows::candidate_source_present`, the
/// [`LapseReason::SourceGone`](crate::engine::LapseReason::SourceGone) probe)
/// and **a forced ability is not restricted to sources its controller could
/// legally use**: Silver Twilight Acolyte 01102's *"Forced - After Silver
/// Twilight Acolyte attacks: Place 1 doom on the current agenda."* resolves
/// whether or not the investigator it just attacked may "use" that enemy.
/// Routing the probe through reachability would report a still-present enemy as
/// gone the moment the defender moved.
///
/// It returns the **card** rather than a bare `bool` so that a caller holding a
/// candidate can ask the sharper question in one step — *is the source still
/// there **and** still the card this candidate was minted from* — which is what
/// [`Act`](AbilitySource::Act) and [`Agenda`](AbilitySource::Agenda) need:
/// those two name *the current* one, so an advanced act is not "gone" but is no
/// longer the card whose ability was scanned. Comparing
/// [`SourceCard::code`] against the candidate's code answers that without any
/// caller re-deriving which board card the source is.
///
/// [`InPlay`](AbilitySource::InPlay) is answered board-wide by
/// [`board::find_instance`] — any investigator's controlled collections, a
/// location's attachments or the cards put into play at it, or an enemy's
/// attachments — matching
/// the collections [`reachable_sources`] reads, so a co-located threat-area card
/// (#708) is not reported gone just because its controller is not the
/// candidate's.
pub(crate) fn source_card(state: &GameState, source: AbilitySource) -> Option<SourceCard<'_>> {
    match source {
        AbilitySource::InPlay(instance_id) => {
            board::find_instance(state, instance_id).map(|(card, _)| SourceCard::Instance(card))
        }
        AbilitySource::Location(location_id) => {
            state.locations.get(&location_id).map(SourceCard::Location)
        }
        AbilitySource::Enemy(enemy_id) => state.enemies.get(&enemy_id).map(SourceCard::Enemy),
        AbilitySource::Act => state.act_deck.get(state.act_index).map(SourceCard::Act),
        AbilitySource::Agenda => state
            .agenda_deck
            .get(state.agenda_index)
            .map(SourceCard::Agenda),
    }
}

/// Every reachable source paired with its card code, materialized so the caller
/// can consult the validator (which borrows `state` again) while iterating.
///
/// The enumerators' shared shape: the turn menu and the fast window ask the same
/// question of the same predicate and differ only in what they do with the
/// answer.
pub(crate) fn reachable_source_codes(
    state: &GameState,
    investigator: InvestigatorId,
) -> Vec<(AbilitySource, CardCode)> {
    reachable_sources(state, investigator)
        .into_iter()
        .map(|(source, card)| (source, card.code().clone()))
        .collect()
}

/// The record behind `source`, or the rejection reason if `investigator` cannot
/// reach it.
///
/// Defined as a lookup in [`reachable_sources`] rather than as its own scan, so
/// "can this investigator reach this source" and "which sources does this
/// investigator have" are the same sentence read in two directions.
pub(crate) fn resolve(
    state: &GameState,
    investigator: InvestigatorId,
    source: AbilitySource,
) -> Result<SourceCard<'_>, Cow<'static, str>> {
    reachable_sources(state, investigator)
        .into_iter()
        .find(|(candidate, _)| *candidate == source)
        .map(|(_, card)| card)
        .ok_or_else(|| unreachable_reason(investigator, source))
}

/// The mutable peer of [`resolve`], for cost payment: the same instance,
/// addressed by identity at the moment it is paid against (#706). `None` for a
/// source with no card instance behind it — a location, an enemy, the act or the
/// agenda — which is why `check_activate_ability` refuses a source-referencing
/// cost on one before any payment starts.
///
/// Reachability is re-checked, not assumed: a cost earlier in the same
/// activation can remove the source from play, and the answer then is
/// legitimately "gone" rather than a stale position (see
/// `pay_activation_costs`).
///
/// **Reachability is decided by [`resolve`], never re-derived here.** This
/// function only re-finds mutably what the predicate already said is reachable,
/// which is why the second walk is over the whole board: a source reachable
/// under a bullet the acting investigator does not control it under — #708's
/// co-located threat areas — must still be payable against. Deciding
/// reachability twice is how the validator and the cost path would come to
/// disagree.
pub(crate) fn resolve_mut(
    state: &mut GameState,
    investigator: InvestigatorId,
    source: AbilitySource,
) -> Option<&mut CardInPlay> {
    let instance = resolve(state, investigator, source)
        .ok()?
        .instance()?
        .instance_id;
    board::find_instance_mut(state, instance).map(|(card, _)| card)
}

/// Rejection reason for a source `investigator` cannot reach. Reasons reach the
/// client, so it reads as a sentence.
fn unreachable_reason(investigator: InvestigatorId, source: AbilitySource) -> Cow<'static, str> {
    format!(
        "ActivateAbility: {investigator:?} cannot reach {source:?} — it is neither a card in \
         play under their control, nor a scenario card at their location, nor the current act \
         or agenda (RR \"Triggered Abilities\")",
    )
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{Act, Agenda, CardInstanceId, EnemyId, GameStateBuilder};
    use crate::test_support;

    const STUDY: LocationId = LocationId(1);
    const HALLWAY: LocationId = LocationId(2);

    fn card(code: &str, instance: u32) -> CardInPlay {
        CardInPlay::enter_play(CardCode::new(code), CardInstanceId(instance))
    }

    /// Two investigators in the Study and one in the Hallway, each with a
    /// threat-area card; a ghoul in the Study and another in the Hallway; an
    /// attachment on each location.
    ///
    /// Investigator 1 is the one doing the reaching.
    fn board() -> GameState {
        let mut mine = test_support::test_investigator(1);
        mine.investigator_card.instance_id = CardInstanceId(10);
        mine.cards_in_play.push(card("01020", 11));
        mine.threat_area.push(card("01098", 12));

        let mut neighbour = test_support::test_investigator(2);
        neighbour.investigator_card.instance_id = CardInstanceId(20);
        neighbour.threat_area.push(card("01099", 21));

        let mut elsewhere = test_support::test_investigator(3);
        elsewhere.investigator_card.instance_id = CardInstanceId(30);
        elsewhere.threat_area.push(card("01100", 31));

        let mut study = test_support::test_location(1, "Study");
        study.attachments.push(card("01168", 40));
        study.cards_at_location.push(card("01117", 60));
        let mut hallway = test_support::test_location(2, "Hallway");
        hallway.attachments.push(card("01168", 41));
        hallway.cards_at_location.push(card("01117", 61));

        let mut here = test_support::test_enemy(1, "Ghoul");
        here.current_location = Some(STUDY);
        here.attachments.push(card("02256", 50));
        let mut there = test_support::test_enemy(2, "Acolyte");
        there.current_location = Some(HALLWAY);

        GameStateBuilder::new()
            .with_investigator_at(mine, STUDY)
            .with_investigator_at(neighbour, STUDY)
            .with_investigator_at(elsewhere, HALLWAY)
            .with_location(study)
            .with_location(hallway)
            .with_enemy(here)
            .with_enemy(there)
            .build()
    }

    fn sources_for(state: &GameState, investigator: InvestigatorId) -> Vec<AbilitySource> {
        reachable_sources(state, investigator)
            .into_iter()
            .map(|(source, _)| source)
            .collect()
    }

    #[test]
    fn control_bullet_reaches_investigator_card_cards_in_play_and_own_threat_area() {
        let state = board();
        let sources = sources_for(&state, InvestigatorId(1));
        assert_eq!(
            &sources[..3],
            &[
                AbilitySource::InPlay(CardInstanceId(10)),
                AbilitySource::InPlay(CardInstanceId(11)),
                AbilitySource::InPlay(CardInstanceId(12)),
            ],
            "the control bullet comes first, and covers the investigator card, cards in play \
             and the investigator's own threat area, in that order",
        );
    }

    /// *"This includes the location itself, encounter cards placed at that
    /// location, and all encounter cards in the threat area of any investigator
    /// at that location."*
    #[test]
    fn colocation_bullet_reaches_the_location_its_encounter_cards_and_colocated_threat_areas() {
        let state = board();
        let sources = sources_for(&state, InvestigatorId(1));
        for (expected, why) in [
            (AbilitySource::Location(STUDY), "the location itself"),
            (
                AbilitySource::InPlay(CardInstanceId(40)),
                "an encounter card attached to the location",
            ),
            (
                AbilitySource::Enemy(EnemyId(1)),
                "an enemy placed at the location",
            ),
            (
                AbilitySource::InPlay(CardInstanceId(50)),
                "an encounter card attached to that enemy",
            ),
            (
                AbilitySource::InPlay(CardInstanceId(21)),
                "a co-located investigator's threat area (Haunted 01098's ruling)",
            ),
        ] {
            assert!(
                sources.contains(&expected),
                "{why} should be reachable; sources were {sources:?}",
            );
        }
    }

    #[test]
    fn nothing_at_another_location_is_reachable() {
        let state = board();
        let sources = sources_for(&state, InvestigatorId(1));
        for (unexpected, why) in [
            (AbilitySource::Location(HALLWAY), "another location"),
            (
                AbilitySource::InPlay(CardInstanceId(41)),
                "an encounter card attached to another location",
            ),
            (AbilitySource::Enemy(EnemyId(2)), "an enemy elsewhere"),
            (
                AbilitySource::InPlay(CardInstanceId(31)),
                "the threat area of an investigator at another location",
            ),
            (
                AbilitySource::InPlay(CardInstanceId(61)),
                "a card put into play at another location",
            ),
        ] {
            assert!(
                !sources.contains(&unexpected),
                "{why} must stay out of reach; sources were {sources:?}",
            );
        }
    }

    /// A card put into play **at** the location is reached by the same
    /// co-location bullet as the location's attachments, and by nothing else:
    /// nobody controls it, so the control bullet never yields it (#771).
    #[test]
    fn a_card_put_into_play_at_the_location_is_reachable() {
        let state = board();
        let sources = sources_for(&state, InvestigatorId(1));
        assert!(
            sources.contains(&AbilitySource::InPlay(CardInstanceId(60))),
            "a card at the acting investigator's location should be reachable; sources were \
             {sources:?}",
        );
        for inv in state.investigators.values() {
            assert!(
                !inv.controlled_card_instances()
                    .any(|c| c.instance_id == CardInstanceId(60)),
                "an uncontrolled card at a location is in nobody's controlled collections",
            );
        }
    }

    /// The presence probe (`source_card`) is board-wide, so a forced or
    /// reaction candidate minted from an uncontrolled card at a location is
    /// not reported gone (#735's contract, extended to the new zone).
    #[test]
    fn source_card_finds_a_card_put_into_play_at_a_location() {
        let state = board();
        let found = source_card(&state, AbilitySource::InPlay(CardInstanceId(60)))
            .expect("a card at a location is on the board");
        assert_eq!(found.code().as_str(), "01117");
    }

    /// Co-location is not control: a co-located investigator's *assets* are
    /// theirs alone. Only the threat area is shared by the bullet, because only
    /// the threat area holds scenario cards.
    #[test]
    fn a_colocated_investigators_own_cards_in_play_are_still_not_reachable() {
        let mut state = board();
        state
            .investigators
            .get_mut(&InvestigatorId(2))
            .expect("neighbour is on the board")
            .cards_in_play
            .push(card("01020", 22));
        let sources = sources_for(&state, InvestigatorId(1));
        assert!(
            !sources.contains(&AbilitySource::InPlay(CardInstanceId(22))),
            "sources were {sources:?}",
        );
    }

    /// *"all **encounter** cards in the threat area of any investigator at
    /// that location"* (#975): co-location reaches another investigator's
    /// threat-area treachery, not a player card sitting beside it. The bearer
    /// reaches both, through control.
    #[test]
    fn colocation_reaches_only_encounter_cards_in_another_investigators_threat_area() {
        let mut a = test_support::test_investigator(1);
        a.investigator_card.instance_id = CardInstanceId(10);
        a.threat_area.push(card(test_support::TEST_ASSET, 11));
        a.threat_area.push(card(test_support::TEST_TREACHERY, 12));
        let mut b = test_support::test_investigator(2);
        b.investigator_card.instance_id = CardInstanceId(20);
        let state = GameStateBuilder::new()
            .with_investigator_at(a, STUDY)
            .with_investigator_at(b, STUDY)
            .with_location(test_support::test_location(1, "Study"))
            .build();

        let player_card = AbilitySource::InPlay(CardInstanceId(11));
        let treachery = AbilitySource::InPlay(CardInstanceId(12));
        let a_reaches = sources_for(&state, InvestigatorId(1));
        assert!(
            a_reaches.contains(&player_card) && a_reaches.contains(&treachery),
            "the bearer reaches both through control; sources were {a_reaches:?}",
        );
        let b_reaches = sources_for(&state, InvestigatorId(2));
        assert!(
            b_reaches.contains(&treachery),
            "a co-located investigator reaches the encounter card; sources were {b_reaches:?}",
        );
        assert!(
            !b_reaches.contains(&player_card),
            "a co-located investigator must not reach a player card in another's threat area; \
             sources were {b_reaches:?}",
        );
    }

    #[test]
    fn own_threat_area_is_offered_exactly_once() {
        let state = board();
        let sources = sources_for(&state, InvestigatorId(1));
        assert_eq!(
            sources
                .iter()
                .filter(|s| **s == AbilitySource::InPlay(CardInstanceId(12)))
                .count(),
            1,
            "the control bullet already yielded it; the co-location pass must skip the acting \
             investigator; sources were {sources:?}",
        );
    }

    #[test]
    fn another_investigators_card_is_not_reachable() {
        let state = board();
        let err = resolve(
            &state,
            InvestigatorId(1),
            AbilitySource::InPlay(CardInstanceId(31)),
        )
        .expect_err("a threat-area card at another location is out of reach");
        assert!(
            err.contains("cannot reach"),
            "the reason should say the source is unreachable, got: {err}",
        );
    }

    #[test]
    fn resolve_returns_the_record_behind_a_reachable_source() {
        let state = board();
        let ward = resolve(
            &state,
            InvestigatorId(1),
            AbilitySource::InPlay(CardInstanceId(12)),
        )
        .expect("own threat-area card is reachable");
        assert_eq!(ward.code().as_str(), "01098");
        assert_eq!(
            ward.instance().map(|c| c.instance_id),
            Some(CardInstanceId(12)),
        );

        let study = resolve(&state, InvestigatorId(1), AbilitySource::Location(STUDY))
            .expect("the location an investigator stands at is reachable");
        assert_eq!(study.code(), &state.locations[&STUDY].code);
        assert!(
            study.instance().is_none() && !study.exhausted() && study.uses().is_empty(),
            "a location carries no per-instance state",
        );

        let ghoul = resolve(&state, InvestigatorId(1), AbilitySource::Enemy(EnemyId(1)))
            .expect("a co-located enemy is reachable");
        assert_eq!(ghoul.code(), &state.enemies[&EnemyId(1)].code);
        assert!(ghoul.instance().is_none());
    }

    #[test]
    fn an_investigator_absent_from_state_reaches_nothing() {
        let state = board();
        assert!(reachable_sources(&state, InvestigatorId(9)).is_empty());
    }

    /// An investigator who is not on the board (setup, or eliminated) still
    /// reaches their own cards — the control bullet does not depend on standing
    /// anywhere — and, on this board, nothing else: `board()` loads no act or
    /// agenda deck, so the third bullet has nothing to offer either. The act and
    /// agenda half of that is
    /// [`an_investigator_at_no_location_still_reaches_the_act_and_agenda`].
    #[test]
    fn an_investigator_at_no_location_reaches_only_the_control_bullet() {
        let mut state = board();
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .expect("on the board")
            .current_location = None;
        assert_eq!(
            sources_for(&state, InvestigatorId(1)),
            vec![
                AbilitySource::InPlay(CardInstanceId(10)),
                AbilitySource::InPlay(CardInstanceId(11)),
                AbilitySource::InPlay(CardInstanceId(12)),
            ],
        );
    }

    /// The write side has to reach every collection the read side does,
    /// including ones the acting investigator does not own.
    #[test]
    fn resolve_mut_addresses_instances_anywhere_the_predicate_reached() {
        let mut state = board();
        for instance in [
            CardInstanceId(12), // own threat area
            CardInstanceId(21), // a co-located investigator's threat area
            CardInstanceId(40), // a location attachment
            CardInstanceId(50), // an enemy attachment
        ] {
            let card = resolve_mut(
                &mut state,
                InvestigatorId(1),
                AbilitySource::InPlay(instance),
            )
            .unwrap_or_else(|| panic!("{instance:?} is reachable, so it must be addressable"));
            card.exhausted = true;
        }
        assert!(state.investigators[&InvestigatorId(2)].threat_area[0].exhausted);
        assert!(state.locations[&STUDY].attachments[0].exhausted);
        assert!(state.enemies[&EnemyId(1)].attachments[0].exhausted);
    }

    /// A source with no card instance has nothing to mutate, which is why a
    /// source-referencing cost on one is refused before payment starts.
    #[test]
    fn resolve_mut_is_none_for_an_unreachable_source_and_for_one_without_an_instance() {
        let mut state = board();
        assert!(resolve_mut(
            &mut state,
            InvestigatorId(1),
            AbilitySource::InPlay(CardInstanceId(31)),
        )
        .is_none());
        assert!(resolve_mut(
            &mut state,
            InvestigatorId(1),
            AbilitySource::Location(STUDY)
        )
        .is_none());
        assert!(resolve_mut(
            &mut state,
            InvestigatorId(1),
            AbilitySource::Enemy(EnemyId(1))
        )
        .is_none());
    }
    // --- The act / agenda bullet (#709) ------------------------------------
    //
    // *"The current act or current agenda card."* — the one bullet gated on
    // nothing at all.

    /// The Gathering's act 1, Trapped.
    const ACT_ONE: &str = "01108";
    /// Its act 2, The Barrier — the act that supersedes [`ACT_ONE`].
    const ACT_TWO: &str = "01109";
    /// The Gathering's agenda 1, What's Going On?!
    const AGENDA: &str = "01105";

    fn act(code: &str, clue_threshold: u8) -> Act {
        Act {
            code: CardCode::new(code),
            clue_threshold,
        }
    }

    fn agenda(code: &str) -> Agenda {
        Agenda {
            code: CardCode::new(code),
            doom_threshold: 3,
        }
    }

    /// [`board`] with a two-act deck and a one-agenda deck loaded, cursors at
    /// the front.
    fn board_with_scenario_decks() -> GameState {
        let mut state = board();
        // Printed clue thresholds, from the snapshot: Trapped 2, The Barrier 3.
        state.act_deck = vec![act(ACT_ONE, 2), act(ACT_TWO, 3)];
        state.act_index = 0;
        state.agenda_deck = vec![agenda(AGENDA)];
        state.agenda_index = 0;
        state
    }

    /// Not location-gated: the investigator in the Study and the one in the
    /// Hallway reach the same two board cards.
    #[test]
    fn the_act_and_agenda_are_reachable_from_every_location() {
        let state = board_with_scenario_decks();
        for investigator in [InvestigatorId(1), InvestigatorId(3)] {
            let sources = sources_for(&state, investigator);
            assert!(
                sources.contains(&AbilitySource::Act) && sources.contains(&AbilitySource::Agenda),
                "{investigator:?} should reach both board cards wherever they stand; \
                 sources were {sources:?}",
            );
        }
    }

    /// The co-location bullet bails out for an investigator who is nowhere on
    /// the map; the act/agenda bullet must not bail out with it.
    #[test]
    fn an_investigator_at_no_location_still_reaches_the_act_and_agenda() {
        let mut state = board_with_scenario_decks();
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .expect("on the board")
            .current_location = None;
        let sources = sources_for(&state, InvestigatorId(1));
        assert!(
            sources.contains(&AbilitySource::Act) && sources.contains(&AbilitySource::Agenda),
            "the third bullet is gated on nothing; sources were {sources:?}",
        );
        assert!(
            !sources.contains(&AbilitySource::Location(STUDY)),
            "the co-location bullet still bails out; sources were {sources:?}",
        );
    }

    /// The descriptor names *the current* act, not an act by position, so
    /// advancing the cursor silently re-points it — the superseded act's
    /// abilities are simply not addressable any more.
    #[test]
    fn the_source_follows_the_cursor_to_the_current_act() {
        let mut state = board_with_scenario_decks();
        assert_eq!(
            resolve(&state, InvestigatorId(1), AbilitySource::Act)
                .expect("act one is current")
                .code()
                .as_str(),
            ACT_ONE,
        );

        state.act_index = 1;
        assert_eq!(
            resolve(&state, InvestigatorId(1), AbilitySource::Act)
                .expect("act two is now current")
                .code()
                .as_str(),
            ACT_TWO,
            "the superseded act is no longer what the source names",
        );
    }

    /// A fixture with no acts, or a scenario whose cursor has run off the end
    /// of its deck, has no board source to offer.
    #[test]
    fn an_absent_act_or_agenda_is_not_reachable() {
        for (state, why) in [
            (board(), "no act or agenda deck is loaded"),
            (
                {
                    let mut state = board_with_scenario_decks();
                    state.act_index = 2;
                    state.agenda_index = 1;
                    state
                },
                "both cursors have run off the end of their decks",
            ),
        ] {
            let sources = sources_for(&state, InvestigatorId(1));
            assert!(
                !sources.contains(&AbilitySource::Act) && !sources.contains(&AbilitySource::Agenda),
                "{why}, so neither board card is reachable; sources were {sources:?}",
            );
            assert!(
                resolve(&state, InvestigatorId(1), AbilitySource::Act).is_err(),
                "{why}, so resolving the act must fail",
            );
            assert!(
                resolve(&state, InvestigatorId(1), AbilitySource::Agenda).is_err(),
                "{why}, so resolving the agenda must fail",
            );
        }
    }

    /// Neither board card carries per-instance state, so neither can be the
    /// target of an exhaust, uses or discard-self cost.
    #[test]
    fn neither_board_card_carries_per_instance_state() {
        let mut state = board_with_scenario_decks();
        for source in [AbilitySource::Act, AbilitySource::Agenda] {
            let card = resolve(&state, InvestigatorId(1), source).expect("reachable");
            assert!(
                card.instance().is_none() && !card.exhausted() && card.uses().is_empty(),
                "{source:?} should carry no per-instance state",
            );
            assert!(resolve_mut(&mut state, InvestigatorId(1), source).is_none());
        }
    }
}
