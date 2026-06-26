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
//! does not auto-discard it. The before-timing clue-discovery interrupt
//! and the game-end forced trauma ride the C5a seam (`WouldDiscoverClues`
//! / `GameEnd`), backed by the two native effects below — ports of the
//! synthetic Cover-Up fixture C5a proved (`scenarios::test_fixtures::synth_cards`).

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
/// Eligibility tag: the discover-replacement reaction may be offered only while
/// Cover Up still holds clues to discard (RR p.2 potential gate). Replaces the
/// former hardcoded `card.clues == 0` stand-in in `scan_pending_triggers`.
const HAS_CLUES_TAG: &str = "01007:has_clues";

#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![
        revelation(put_into_threat_area_with_clues(CODE, 3)),
        reaction_on_event(
            EventPattern::WouldDiscoverClues,
            EventTiming::When,
            // "Discard that many clues from Cover Up instead": run the discard,
            // then cancel the discovery — cancel = degenerate replacement
            // (Axis D #336). The before-discover window's continuation skips
            // the deferred discovery when `pending_cancellation` is set.
            Effect::Seq(vec![native(DISCARD_TAG), Effect::Cancel]),
        )
        .with_eligibility(HAS_CLUES_TAG),
        forced_on_event(
            EventPattern::GameEnd,
            EventTiming::After,
            native(TRAUMA_TAG),
        ),
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
                pattern: EventPattern::WouldDiscoverClues,
                timing: EventTiming::When,
                ..
            }
        ));
        // The reaction discards from self, then cancels the discovery.
        assert!(matches!(
            &abilities[1].effect,
            Effect::Seq(steps) if steps.len() == 2 && matches!(steps[1], Effect::Cancel)
        ));
        assert!(matches!(
            abilities[2].trigger,
            Trigger::OnEvent {
                pattern: EventPattern::GameEnd,
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

        // The WouldDiscoverClues reaction now carries the eligibility tag.
        let abilities = super::abilities();
        assert_eq!(
            abilities[1].eligibility.as_deref(),
            Some("01007:has_clues"),
            "the discover-replacement reaction declares the potential gate"
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
