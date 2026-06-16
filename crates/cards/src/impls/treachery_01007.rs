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
    Ability, EventPattern, EventTiming,
};
use game_core::card_registry::NativeEffectFn;
use game_core::event::TraumaKind;
use game_core::{Cx, EngineOutcome, EvalContext, Event};

/// `ArkhamDB` code for Cover Up.
pub const CODE: &str = "01007";

/// Native tag: discard the replaced clue count from Cover Up.
const DISCARD_TAG: &str = "01007:discard_clues";
/// Native tag: suffer 1 mental trauma at game end if clues remain.
const TRAUMA_TAG: &str = "01007:trauma";

#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![
        revelation(put_into_threat_area_with_clues(CODE, 3)),
        reaction_on_event(
            EventPattern::WouldDiscoverClues,
            EventTiming::Before,
            native(DISCARD_TAG),
        ),
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

/// "Discard that many clues from Cover Up instead" — discard the replaced
/// count (threaded via `clue_discovery_count`) from the firing instance.
fn discard_clues(cx: &mut Cx, ctx: &EvalContext) -> EngineOutcome {
    debug_assert!(
        ctx.clue_discovery_count.is_some(),
        "cover_up discard: clue_discovery_count not threaded"
    );
    let count = ctx.clue_discovery_count.unwrap_or(0);
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
                timing: EventTiming::Before,
                ..
            }
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
}
