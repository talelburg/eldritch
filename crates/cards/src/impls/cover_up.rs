//! Cover Up (Roland Banks signature weakness, 01007).
//!
//! ```text
//! Revelation - Put Cover Up into play in your threat area, with 3 clues
//!   on it.
//! [reaction] When you would discover 1 or more clues at your location:
//!   Discard that many clues from Cover Up instead.
//! Forced - When the game ends, if there are any clues on Cover Up:
//!   You suffer 1 mental trauma.
//! ```
//!
//! Persistent treachery: the Revelation self-places into the threat area
//! with 3 clues (`Effect::PutIntoThreatArea`), so `resolve_encounter_card`
//! does not auto-discard it. Both triggered abilities are backed by the native
//! effects below — ports of the synthetic Cover-Up fixture C5a proved
//! (`scenarios::test_fixtures::synth_cards`).
//!
//! **Cell: the `when` cell of the `DiscoverClues` condition** for the reaction
//! (#703). The printed word is *"When you **would** discover…"*, and the
//! replacement has to land before the clues move or there is nothing left to
//! replace.
//!
//! **Cell: the `when` cell of the `GameEnd` condition** for the Forced trauma
//! (#720). The printed word is *"When the game ends"*, and `glossary/When.md`
//! puts that *"immediately after the specified timing point or triggering
//! condition initiates, but before its impact upon the game state resolves."*
//! The game's ending is a **bare milestone** — its resolve step is a documented
//! no-op, because the victory-display scan and `apply_resolution` run after the
//! whole sequence, at the apply boundary — so the `when` cell here is the first
//! cell of a sequence that changes nothing as it resolves, and the trauma lands
//! with the board exactly as the ending found it. The same declaration is
//! scanned at Rules Reference p.10 Elimination step 0, which is why an
//! eliminated Roland suffers it too: *"If Roland is eliminated (by being
//! defeated or taking a **resign** action) while Cover Up is in play, Cover Up's
//! **Forced** effect triggers"* (<https://arkhamdb.com/card/01007>).
//!
//! **The Forced's *"if there are any clues"* is an initiation condition, not
//! part of its effect** (#786). RR p.2: *"If a forced ability does not have the
//! potential to change the game state, the ability does not initiate."* So it
//! carries `HAS_CLUES_TAG` — the same gate the reaction declares — and a
//! clueless Cover Up is collected by neither scan. Without the tag the opaque
//! native effect is uninspectable, the forced scan collected it anyway, and
//! interactive play prompted the player to resolve an ability that did nothing.

use card_dsl::dsl::{
    forced_on_event, native, put_into_threat_area_with_clues, reaction_on_event, revelation,
    Ability, Effect, EventPattern, EventTiming,
};
use game_core::card_registry::{EligibilityFn, NativeEffectFn};
use game_core::event::TraumaKind;
use game_core::state::GameState;
use game_core::{Cx, EngineOutcome, EvalContext, Event};

/// `ArkhamDB` code for Cover Up.
pub const CODE: &str = "01007";

/// Native tag: discard the replaced clue count from Cover Up.
const DISCARD_TAG: &str = "01007:discard_clues";
/// Native tag: suffer 1 mental trauma at game end if clues remain.
const TRAUMA_TAG: &str = "01007:trauma";
/// Eligibility tag: both of Cover Up's clue-conditional abilities gate on it —
/// the discover-replacement reaction is offered only while Cover Up still holds
/// clues to discard, and the game-end Forced only initiates while it does
/// (RR p.2 potential gate, #786). Replaces the former hardcoded
/// `card.clues == 0` stand-in in `scan_pending_triggers`.
const HAS_CLUES_TAG: &str = "01007:has_clues";

#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![
        revelation(put_into_threat_area_with_clues(CODE, 3)),
        reaction_on_event(
            EventPattern::DiscoverClues,
            EventTiming::When,
            // "Discard that many clues from Cover Up instead": run the discard,
            // then cancel the discovery — cancel = degenerate replacement
            // (Axis D #336). The `when` cell resolves before the clues move, and
            // the coordinator abandons the sequence when `pending_cancellation`
            // is set (#703, #714), so the discovery never happens — and neither
            // does any other ability referencing it. `glossary/Instead.md`: this
            // "would" replacement changes the nature of the triggering
            // condition, so *"No further abilities referencing the original
            // triggering condition may be used."*
            Effect::Seq(vec![native(DISCARD_TAG), Effect::Cancel]),
        )
        .with_eligibility(HAS_CLUES_TAG),
        // "if there are any clues on Cover Up" is an initiation condition, not
        // part of the effect: RR p.2 ("Forced Abilities") — *"If a forced
        // ability does not have the potential to change the game state, the
        // ability does not initiate."* With no clues the trauma cannot happen,
        // so the same tag the reaction gates on keeps the forced scan from
        // collecting it — and from prompting to resolve it (#786).
        forced_on_event(EventPattern::GameEnd, EventTiming::When, native(TRAUMA_TAG))
            .with_eligibility(HAS_CLUES_TAG),
    ]
}

#[must_use]
pub fn native_effect_for(tag: &str) -> Option<NativeEffectFn> {
    match tag {
        DISCARD_TAG => Some(discard_clues),
        TRAUMA_TAG => Some(trauma),
        _ => None,
    }
}

/// True while the Cover Up instance (the firing source) still holds clues to
/// discard. Read-only mirror of [`discard_clues`]'s instance lookup.
fn has_clues(state: &GameState, ctx: &EvalContext) -> bool {
    let Some(source) = ctx.source else {
        return false;
    };
    state.investigators.get(&ctx.controller).is_some_and(|inv| {
        inv.threat_area
            .iter()
            .chain(inv.cards_in_play.iter())
            .any(|c| c.instance_id == source && c.clues > 0)
    })
}

/// Resolve Cover Up's eligibility tag.
pub(crate) fn native_eligibility_for(tag: &str) -> Option<EligibilityFn> {
    match tag {
        HAS_CLUES_TAG => Some(has_clues as EligibilityFn),
        _ => None,
    }
}

/// "Discard that many clues from Cover Up instead" — discard the replaced
/// count (threaded via `clue_discovery_count`) from the firing instance.
fn discard_clues(cx: &mut Cx, ctx: &EvalContext) -> EngineOutcome {
    debug_assert!(
        ctx.clue_discovery_count().is_some(),
        "cover_up discard: clue_discovery_count not threaded"
    );
    let count = ctx.clue_discovery_count().unwrap_or(0);
    let Some(source) = ctx.source else {
        return EngineOutcome::Rejected {
            reason: "cover_up discard: no source instance".into(),
        };
    };
    if let Some(inv) = cx.state.investigators.get_mut(&ctx.controller) {
        for card in inv
            .threat_area
            .iter_mut()
            .chain(inv.cards_in_play.iter_mut())
        {
            if card.instance_id == source {
                let take = count.min(card.clues);
                card.clues -= take;
                break;
            }
        }
    }
    EngineOutcome::Done
}

/// "When the game ends, if there are any clues on Cover Up: You suffer 1
/// mental trauma."
///
/// The clue check is duplicated deliberately. [`HAS_CLUES_TAG`] keeps a clueless
/// instance from *initiating* (#786), which is the rules-correct home for the
/// condition; this inner check guards the window the gate cannot, since the
/// forced scan evaluates eligibility once at collect time. Two simultaneous
/// game-end hits resolve in a chosen order (#213), so a sibling could strip the
/// clues between this one's collection and its resolution.
fn trauma(cx: &mut Cx, ctx: &EvalContext) -> EngineOutcome {
    let Some(source) = ctx.source else {
        return EngineOutcome::Rejected {
            reason: "cover_up trauma: no source instance".into(),
        };
    };
    let has_clues = cx
        .state
        .investigators
        .get(&ctx.controller)
        .is_some_and(|inv| {
            inv.controlled_card_instances()
                .any(|c| c.instance_id == source && c.clues > 0)
        });
    if has_clues {
        cx.events.push(Event::TraumaSuffered {
            investigator: ctx.controller,
            kind: TraumaKind::Mental,
            amount: 1,
        });
    }
    EngineOutcome::Done
}

#[cfg(test)]
mod tests {
    use super::*;
    use card_dsl::dsl::{Effect, Trigger};

    #[test]
    fn revelation_places_with_three_clues_plus_interrupt_and_gameend() {
        let abilities = abilities();
        assert_eq!(abilities.len(), 3);
        assert_eq!(abilities[0].trigger, Trigger::Revelation);
        assert!(matches!(
            &abilities[0].effect,
            Effect::PutIntoThreatArea { code, clues: 3 } if code == CODE
        ));
        assert!(matches!(
            abilities[1].trigger,
            Trigger::OnEvent {
                pattern: EventPattern::DiscoverClues,
                timing: EventTiming::When,
                ..
            }
        ));
        // The reaction discards from self, then cancels the discovery.
        assert!(matches!(
            &abilities[1].effect,
            Effect::Seq(steps) if steps.len() == 2 && matches!(steps[1], Effect::Cancel)
        ));
        // *"**Forced** - When the game ends…"* — the `when` cell of the
        // `GameEnd` condition, a bare milestone since #720.
        assert!(matches!(
            abilities[2].trigger,
            Trigger::OnEvent {
                pattern: EventPattern::GameEnd,
                timing: EventTiming::When,
                ..
            }
        ));
    }

    #[test]
    fn native_tags_resolve() {
        assert!(native_effect_for(DISCARD_TAG).is_some());
        assert!(native_effect_for(TRAUMA_TAG).is_some());
        assert!(native_effect_for("nope").is_none());
    }

    #[test]
    fn has_clues_predicate_gates_on_source_instance_clues() {
        use game_core::state::{CardInPlay, CardInstanceId, GameStateBuilder, InvestigatorId};

        // Both clue-conditional abilities carry the eligibility tag.
        let abilities = super::abilities();
        assert_eq!(
            abilities[1].eligibility.as_deref(),
            Some("01007:has_clues"),
            "the discover-replacement reaction declares the potential gate"
        );
        assert_eq!(
            abilities[2].eligibility.as_deref(),
            Some("01007:has_clues"),
            "the game-end Forced declares it too, so a clueless Cover Up never \
             initiates (#786)"
        );

        // Predicate: true while the source instance holds clues, false at 0.
        let pred = super::native_eligibility_for("01007:has_clues").expect("registered");
        let mut inv = game_core::test_support::test_investigator(1);
        let mut card =
            CardInPlay::enter_play(game_core::state::CardCode::new("01007"), CardInstanceId(0));
        card.clues = 3;
        inv.threat_area.push(card);
        let mut state = GameStateBuilder::new().with_investigator(inv).build();
        let ctx = EvalContext::for_controller_with_source(InvestigatorId(1), CardInstanceId(0));
        assert!(pred(&state, &ctx), "3 clues → eligible");

        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .threat_area[0]
            .clues = 0;
        assert!(!pred(&state, &ctx), "0 clues → ineligible");
    }
}
