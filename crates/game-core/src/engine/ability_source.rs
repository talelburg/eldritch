//! Reachability: which [`AbilitySource`]s a given investigator may use an
//! ability from (#707, #708, #709).
//!
//! The Rules Reference answers this once for `[free]`, `[reaction]` and
//! `[action]` abilities alike — `glossary/Triggered_Abilities.md`'s four
//! bullets, quoted on [`AbilitySource`]. This module is the engine's single
//! answer to them, so the validator and the turn-menu enumerator cannot drift
//! apart: [`reachable_sources`] *is* the predicate, and [`resolve`] is a lookup
//! in it rather than a second reading of the rules.
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
//! wherever they stand, which is why [`reachable_sources`] appends them after
//! the co-location pass has had its chance to bail out.
//!
//! Reachability says only *which sources are addressable*. It never widens what
//! is **legal**: everything `Appendix_I_Initiation_Sequence.md` requires still
//! runs afterwards in `check_activate_ability` — *"determine if the card can be
//! played, or if the ability can be initiated, at this time. (This includes
//! verifying that the resolution of the effect has the potential to change the
//! game state.)"*, and that the cost can be paid.

use std::borrow::Cow;
use std::collections::BTreeMap;

use crate::state::{
    AbilitySource, Act, Agenda, CardCode, CardInPlay, CardInstanceId, Enemy, GameState,
    Investigator, InvestigatorId, Location, UseKind,
};

/// What a reachable [`AbilitySource`] points at: the record carrying the
/// abilities, whichever kind of thing it is.
///
/// The activation path needs four things from a source — its card code, whether
/// it is exhausted, its remaining uses, and its card instance (if it has one) —
/// and only the first is available uniformly. A location is a
/// [`Location`](crate::state::Location) keyed by `LocationId`, an enemy is an
/// [`Enemy`](crate::state::Enemy) keyed by `EnemyId`, and neither carries the
/// per-instance state a [`CardInPlay`] does. Answering all four here is what
/// keeps every caller from re-deriving "does this kind of source have an
/// instance behind it".
#[derive(Debug, Clone, Copy)]
pub(crate) enum SourceCard<'a> {
    /// A card instance in play — an investigator card, a card in play, a
    /// threat-area card, or an attachment on a location or an enemy.
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

impl SourceCard<'_> {
    /// The printed code the ability is looked up by in the card registry.
    pub(crate) fn code(&self) -> &CardCode {
        match self {
            SourceCard::Instance(card) => &card.code,
            SourceCard::Location(location) => &location.code,
            SourceCard::Enemy(enemy) => &enemy.code,
            SourceCard::Act(act) => &act.code,
            SourceCard::Agenda(agenda) => &agenda.code,
        }
    }

    /// The card instance behind this source, if it has one. `None` for a
    /// location (locations do not exhaust and carry no uses); for an enemy — an
    /// enemy readies and exhausts through its own `exhausted` field, which is
    /// not the card-instance state an `Exhaust` cost pays against; and for the
    /// act and the agenda, which are `Act` / `Agenda` records in the scenario
    /// decks and carry no per-instance state at all.
    pub(crate) fn instance(&self) -> Option<&CardInPlay> {
        match self {
            SourceCard::Instance(card) => Some(card),
            SourceCard::Location(_)
            | SourceCard::Enemy(_)
            | SourceCard::Act(_)
            | SourceCard::Agenda(_) => None,
        }
    }

    /// Whether an `Exhaust` cost is already spent on this source. Only a card
    /// instance can carry one; see [`instance`](Self::instance).
    pub(crate) fn exhausted(&self) -> bool {
        self.instance().is_some_and(|card| card.exhausted)
    }

    /// Remaining uses by kind — empty for a source with no card instance.
    pub(crate) fn uses(&self) -> BTreeMap<UseKind, u8> {
        self.instance()
            .map(|card| card.uses.clone())
            .unwrap_or_default()
    }
}

/// Every ability source `investigator` can reach, paired with the record that
/// carries the abilities, in a stable order.
///
/// The order is the three bullets in the order the Rules Reference prints them:
/// the control bullet first (`Investigator::controlled_card_instances`': the
/// investigator card, then cards in play, then the threat area), then the
/// co-location bullet (the location itself, its attachments, each enemy at it
/// with its attachments, then the threat areas of the *other* investigators
/// there), then the current act and the current agenda. It is what the turn menu
/// is listed in, so it must stay deterministic — `investigators`, `locations`
/// and `enemies` are all `BTreeMap`s, so iteration is by id.
///
/// The acting investigator's own threat area is yielded once, under the control
/// bullet: the co-location pass skips them, since a card cannot be in two
/// collections.
///
/// Empty for an investigator who is not in `state`. An investigator who is not
/// at a location (one in the setup phase, or one who has left the board) skips
/// the co-location bullet and **keeps** the act and the agenda: that bullet is
/// gated on nothing.
///
/// [`Investigator::controlled_card_instances`]: crate::state::Investigator::controlled_card_instances
pub(crate) fn reachable_sources(
    state: &GameState,
    investigator: InvestigatorId,
) -> Vec<(AbilitySource, SourceCard<'_>)> {
    let Some(inv) = state.investigators.get(&investigator) else {
        return Vec::new();
    };

    // The control bullet, in full: `controlled_card_instances` is already the
    // definition of "a card in play and under his or her control, including his
    // or her investigator card" — the forced and reaction scans walk it, and
    // this is what makes the activation path agree with them (#707).
    let mut sources: Vec<_> = inv.controlled_card_instances().map(as_instance).collect();
    sources.extend(colocated_sources(state, inv));
    // *"The current act or current agenda card."* (#709) — appended after the
    // co-location pass rather than inside it, because this bullet has no gate:
    // an investigator between locations still reaches both. A deck that is empty
    // or whose cursor has run off the end (a fixture with no acts, a scenario
    // past its last agenda) simply yields nothing.
    if let Some(act) = state.act_deck.get(state.act_index) {
        sources.push((AbilitySource::Act, SourceCard::Act(act)));
    }
    if let Some(agenda) = state.agenda_deck.get(state.agenda_index) {
        sources.push((AbilitySource::Agenda, SourceCard::Agenda(agenda)));
    }
    sources
}

/// The co-location bullet's sources for `inv` (#708) — empty for an
/// investigator who is not standing at a location on the map.
///
/// Split out of [`reachable_sources`] so that bailing out of *this* bullet
/// cannot skip the act and agenda bullet that follows it: the two are
/// independent, and an early `return` in one function body made them look
/// sequential.
fn colocated_sources<'a>(
    state: &'a GameState,
    inv: &Investigator,
) -> Vec<(AbilitySource, SourceCard<'a>)> {
    let mut sources = Vec::new();
    // Everything below is gated on standing in the same place, never on
    // controlling it.
    let Some(location_id) = inv.current_location else {
        return sources;
    };
    let Some(location) = state.locations.get(&location_id) else {
        return sources;
    };

    // "the location itself"
    sources.push((
        AbilitySource::Location(location_id),
        SourceCard::Location(location),
    ));
    // "encounter cards placed at that location" — attachments on the location
    // (Obscuring Fog 01168), and the enemies standing on it, which are exactly
    // the encounter cards the Parley abilities are printed on (Herman Collins
    // 01138, Mob Enforcer 01101). An enemy's own attachments ride with it.
    //
    // The bullet says *scenario* card, and attachments are taken unfiltered:
    // `Effect::AttachSelfToLocation` has one caller in the corpus and it is an
    // encounter card, so no player card can sit in either collection today. The
    // day one can — an attaching player asset — this is where the encounter /
    // player distinction goes, and it wants the card's own metadata rather than
    // the collection it landed in.
    sources.extend(location.attachments.iter().map(as_instance));
    for enemy in state
        .enemies
        .values()
        .filter(|enemy| enemy.current_location == Some(location_id))
    {
        sources.push((AbilitySource::Enemy(enemy.id), SourceCard::Enemy(enemy)));
        sources.extend(enemy.attachments.iter().map(as_instance));
    }
    // "all encounter cards in the threat area of any investigator at that
    // location" — *any*, so this is other people's threat areas too (Haunted
    // 01098's ruling). The acting investigator's own came with the control
    // bullet above.
    for other in state
        .investigators
        .values()
        .filter(|other| other.id != inv.id && other.current_location == Some(location_id))
    {
        sources.extend(other.threat_area.iter().map(as_instance));
    }

    sources
}

/// One in-play card instance, as a reachable source. A free function rather
/// than a closure so the borrow it returns lives as long as the state it came
/// from.
fn as_instance(card: &CardInPlay) -> (AbilitySource, SourceCard<'_>) {
    (
        AbilitySource::InPlay(card.instance_id),
        SourceCard::Instance(card),
    )
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
    instance_in_play_mut(state, instance)
}

/// The in-play instance `instance_id` names, wherever on the board it sits:
/// any investigator's controlled collections, a location's attachments, or an
/// enemy's.
///
/// The write-side mirror of the collections [`reachable_sources`] reads, kept as
/// one walk so a source that became reachable through somebody else's
/// collection is still payable against.
fn instance_in_play_mut(
    state: &mut GameState,
    instance_id: CardInstanceId,
) -> Option<&mut CardInPlay> {
    if let Some(card) = state
        .investigators
        .values_mut()
        .find_map(|inv| inv.controlled_card_instance_mut(instance_id))
    {
        return Some(card);
    }
    state
        .locations
        .values_mut()
        .flat_map(|location| location.attachments.iter_mut())
        .chain(
            state
                .enemies
                .values_mut()
                .flat_map(|enemy| enemy.attachments.iter_mut()),
        )
        .find(|card| card.instance_id == instance_id)
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
    use crate::state::{Act, Agenda, CardCode, CardInstanceId, EnemyId, LocationId};
    use crate::test_support::{test_enemy, test_investigator, test_location, GameStateBuilder};

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
        let mut mine = test_investigator(1);
        mine.investigator_card.instance_id = CardInstanceId(10);
        mine.cards_in_play.push(card("01020", 11));
        mine.threat_area.push(card("01098", 12));

        let mut neighbour = test_investigator(2);
        neighbour.investigator_card.instance_id = CardInstanceId(20);
        neighbour.threat_area.push(card("01099", 21));

        let mut elsewhere = test_investigator(3);
        elsewhere.investigator_card.instance_id = CardInstanceId(30);
        elsewhere.threat_area.push(card("01100", 31));

        let mut study = test_location(1, "Study");
        study.attachments.push(card("01168", 40));
        let mut hallway = test_location(2, "Hallway");
        hallway.attachments.push(card("01168", 41));

        let mut here = test_enemy(1, "Ghoul");
        here.current_location = Some(STUDY);
        here.attachments.push(card("02256", 50));
        let mut there = test_enemy(2, "Acolyte");
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
        ] {
            assert!(
                !sources.contains(&unexpected),
                "{why} must stay out of reach; sources were {sources:?}",
            );
        }
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
            resolution: None,
        }
    }

    fn agenda(code: &str) -> Agenda {
        Agenda {
            code: CardCode::new(code),
            doom_threshold: 3,
            resolution: None,
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
