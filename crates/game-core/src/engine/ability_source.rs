//! Reachability: which [`AbilitySource`]s a given investigator may use an
//! ability from (#707).
//!
//! The Rules Reference answers this once for `[free]`, `[reaction]` and
//! `[action]` abilities alike — `glossary/Triggered_Abilities.md`'s four
//! bullets, quoted on [`AbilitySource`]. This module is the engine's single
//! answer to them, so the validator and the turn-menu enumerator cannot drift
//! apart: [`reachable_sources`] *is* the predicate, and [`resolve`] is a lookup
//! in it rather than a second reading of the rules.
//!
//! This slice implements the **control** bullet only — *"A card in play and
//! under his or her control. This includes his or her investigator card."* The
//! co-location bullet (#708) and the act/agenda bullet (#709) attach here.
//!
//! Reachability says only *which sources are addressable*. It never widens what
//! is **legal**: everything `Appendix_I_Initiation_Sequence.md` requires still
//! runs afterwards in `check_activate_ability` — *"determine if the card can be
//! played, or if the ability can be initiated, at this time. (This includes
//! verifying that the resolution of the effect has the potential to change the
//! game state.)"*, and that the cost can be paid.

use std::borrow::Cow;

use crate::state::{AbilitySource, CardInPlay, GameState, InvestigatorId};

/// Every ability source `investigator` can reach, paired with the card instance
/// that carries the abilities, in a stable order.
///
/// The order is [`Investigator::controlled_card_instances`]': the investigator
/// card, then cards in play, then the threat area. It is what the turn menu is
/// listed in, so it must stay deterministic.
///
/// Yields nothing for an investigator who is not in `state`.
///
/// [`Investigator::controlled_card_instances`]: crate::state::Investigator::controlled_card_instances
pub(crate) fn reachable_sources(
    state: &GameState,
    investigator: InvestigatorId,
) -> impl Iterator<Item = (AbilitySource, &CardInPlay)> {
    // The control bullet, in full: `controlled_card_instances` is already the
    // definition of "a card in play and under his or her control, including his
    // or her investigator card" — the forced and reaction scans walk it, and
    // this is what makes the activation path agree with them (#707).
    state
        .investigators
        .get(&investigator)
        .into_iter()
        .flat_map(|inv| {
            inv.controlled_card_instances()
                .map(|card| (AbilitySource::InPlay(card.instance_id), card))
        })
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
) -> Vec<(AbilitySource, crate::state::CardCode)> {
    reachable_sources(state, investigator)
        .map(|(source, card)| (source, card.code.clone()))
        .collect()
}

/// The card instance behind `source`, or the rejection reason if `investigator`
/// cannot reach it.
///
/// Defined as a lookup in [`reachable_sources`] rather than as its own scan, so
/// "can this investigator reach this source" and "which sources does this
/// investigator have" are the same sentence read in two directions.
pub(crate) fn resolve(
    state: &GameState,
    investigator: InvestigatorId,
    source: AbilitySource,
) -> Result<&CardInPlay, Cow<'static, str>> {
    reachable_sources(state, investigator)
        .find(|(candidate, _)| *candidate == source)
        .map(|(_, card)| card)
        .ok_or_else(|| unreachable_reason(investigator, source))
}

/// The mutable peer of [`resolve`], for cost payment: the same instance,
/// addressed by identity at the moment it is paid against (#706).
///
/// Reachability is re-checked, not assumed: a cost earlier in the same
/// activation can remove the source from play, and the answer then is
/// legitimately "gone" rather than a stale position (see
/// `pay_activation_costs`).
///
/// **Reachability is decided by [`resolve`], never re-derived here.** This
/// function only re-finds mutably what the predicate already said is reachable,
/// which is why the second walk is over *every* investigator's collections: a
/// source reachable under a bullet the acting investigator does not control it
/// under — #708's co-located threat areas — must still be payable against.
/// Deciding reachability twice is how the validator and the cost path would come
/// to disagree.
pub(crate) fn resolve_mut(
    state: &mut GameState,
    investigator: InvestigatorId,
    source: AbilitySource,
) -> Option<&mut CardInPlay> {
    let instance = resolve(state, investigator, source).ok()?.instance_id;
    state
        .investigators
        .values_mut()
        .find_map(|inv| inv.controlled_card_instance_mut(instance))
}

/// Rejection reason for a source `investigator` cannot reach. Reasons reach the
/// client, so it reads as a sentence.
fn unreachable_reason(investigator: InvestigatorId, source: AbilitySource) -> Cow<'static, str> {
    format!(
        "ActivateAbility: {investigator:?} cannot reach {source:?} — it is not a card in play \
         under their control (RR \"Triggered Abilities\")",
    )
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{CardCode, CardInstanceId};
    use crate::test_support::{test_investigator, GameStateBuilder};

    /// One investigator holding an asset in play and a treachery in their
    /// threat area, plus a second investigator with an asset of their own.
    fn two_investigators() -> GameState {
        let mut one = test_investigator(1);
        one.cards_in_play.push(CardInPlay::enter_play(
            CardCode::new("01020"),
            CardInstanceId(1),
        ));
        one.threat_area.push(CardInPlay::enter_play(
            CardCode::new("01098"),
            CardInstanceId(2),
        ));
        one.investigator_card.instance_id = CardInstanceId(0);
        let mut two = test_investigator(2);
        two.cards_in_play.push(CardInPlay::enter_play(
            CardCode::new("01020"),
            CardInstanceId(3),
        ));

        GameStateBuilder::new()
            .with_investigator(one)
            .with_investigator(two)
            .build()
    }

    #[test]
    fn control_bullet_reaches_investigator_card_cards_in_play_and_own_threat_area() {
        let state = two_investigators();
        let sources: Vec<_> = reachable_sources(&state, InvestigatorId(1))
            .map(|(source, _)| source)
            .collect();
        assert_eq!(
            sources,
            vec![
                AbilitySource::InPlay(CardInstanceId(0)),
                AbilitySource::InPlay(CardInstanceId(1)),
                AbilitySource::InPlay(CardInstanceId(2)),
            ],
            "the control bullet covers the investigator card, cards in play and the \
             investigator's own threat area, in that order",
        );
    }

    #[test]
    fn another_investigators_card_is_not_reachable() {
        let state = two_investigators();
        let err = resolve(
            &state,
            InvestigatorId(1),
            AbilitySource::InPlay(CardInstanceId(3)),
        )
        .expect_err("a card another investigator controls is out of reach");
        assert!(
            err.contains("cannot reach"),
            "the reason should say the source is unreachable, got: {err}",
        );
    }

    #[test]
    fn resolve_returns_the_instance_behind_a_reachable_source() {
        let state = two_investigators();
        let card = resolve(
            &state,
            InvestigatorId(1),
            AbilitySource::InPlay(CardInstanceId(2)),
        )
        .expect("own threat-area card is reachable");
        assert_eq!(card.instance_id, CardInstanceId(2));
        assert_eq!(card.code.as_str(), "01098");
    }

    #[test]
    fn an_investigator_absent_from_state_reaches_nothing() {
        let state = two_investigators();
        assert_eq!(reachable_sources(&state, InvestigatorId(9)).count(), 0);
    }

    #[test]
    fn resolve_mut_addresses_the_same_instance() {
        let mut state = two_investigators();
        let card = resolve_mut(
            &mut state,
            InvestigatorId(1),
            AbilitySource::InPlay(CardInstanceId(2)),
        )
        .expect("own threat-area card is reachable");
        card.exhausted = true;
        assert!(state.investigators[&InvestigatorId(1)].threat_area[0].exhausted);
        assert!(resolve_mut(
            &mut state,
            InvestigatorId(1),
            AbilitySource::InPlay(CardInstanceId(3)),
        )
        .is_none());
    }
}
