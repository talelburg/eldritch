//! Obscuring Fog (The Gathering treachery, 01168).
//!
//! ```text
//! Revelation - Attach to your location. Limit 1 per location.
//! Attached location gets +2 shroud.
//! Forced - After attached location is successfully investigated:
//!   Discard Obscuring Fog.
//! ```
//!
//! Persistent treachery: it has non-Revelation abilities (a constant
//! `+2` shroud modifier and a forced self-discard), so
//! `resolve_encounter_card` does not auto-discard it — the card owns its
//! own disposition. The Revelation native enforces the printed "Limit 1
//! per location": a second copy is discarded to the encounter discard
//! instead of attaching (a treachery that cannot enter play is
//! discarded). The `+2` shroud is read by `investigate` via
//! `effective_shroud`; the forced discard runs `Effect::DiscardSelf`,
//! which finds this attachment by the firing instance.

use card_dsl::dsl::{
    constant, discard_self, forced_on_event, modify, native, revelation, Ability, EventPattern,
    EventTiming, ModifierScope, Stat,
};
use game_core::card_registry::NativeEffectFn;
use game_core::state::{CardCode, Zone};
use game_core::{attach_to_location, Cx, EngineOutcome, EvalContext, Event};

/// `ArkhamDB` code for Obscuring Fog.
pub const CODE: &str = "01168";

const LIMIT1_ATTACH: &str = "01168:limit1-attach";

#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![
        revelation(native(LIMIT1_ATTACH)),
        constant(modify(Stat::Shroud, 2, ModifierScope::WhileInPlay)),
        forced_on_event(
            EventPattern::AfterLocationInvestigated,
            EventTiming::After,
            discard_self(),
        ),
    ]
}

/// Resolve this treachery's native-effect tag. Wired into the crate
/// registry's `native_effect_for`.
pub(crate) fn native_effect_for(tag: &str) -> Option<NativeEffectFn> {
    (tag == LIMIT1_ATTACH).then_some(limit1_attach as NativeEffectFn)
}

/// Revelation: attach to the controller's location, enforcing "Limit 1
/// per location". A second copy on the same location is discarded to the
/// encounter discard instead.
///
/// TODO(#373): collapse onto a generalized attach-to-location effect (a
/// by-code form + an optional per-location limit) shared with Barricade
/// 01038's `Effect::AttachSelfToLocation`, retiring this bespoke native.
fn limit1_attach(cx: &mut Cx, ctx: &EvalContext) -> EngineOutcome {
    let Some(loc_id) = cx
        .state
        .investigators
        .get(&ctx.controller)
        .and_then(|inv| inv.current_location)
    else {
        return EngineOutcome::Rejected {
            reason: "01168 limit1-attach: controller has no location".into(),
        };
    };
    let already = cx
        .state
        .locations
        .get(&loc_id)
        .is_some_and(|loc| loc.attachments.iter().any(|c| c.code.as_str() == CODE));
    if already {
        // Limit 1 per location: this copy can't enter play, so discard it.
        cx.state.encounter_discard.push(CardCode::new(CODE));
        cx.events.push(Event::CardDiscarded {
            investigator: ctx.controller,
            code: CardCode::new(CODE),
            from: Zone::LocationAttachment,
        });
        return EngineOutcome::Done;
    }
    attach_to_location(cx, loc_id, CardCode::new(CODE));
    EngineOutcome::Done
}

#[cfg(test)]
mod tests {
    use super::*;
    use card_dsl::dsl::{Effect, Trigger};

    #[test]
    fn abilities_are_attach_shroud_and_forced_discard() {
        let abilities = abilities();
        assert_eq!(abilities.len(), 3);

        // Revelation = limit-1 attach native.
        assert_eq!(abilities[0].trigger, Trigger::Revelation);
        assert!(matches!(&abilities[0].effect, Effect::Native { tag } if tag == LIMIT1_ATTACH));

        // Constant +2 shroud.
        assert_eq!(abilities[1].trigger, Trigger::Constant);
        assert!(matches!(
            &abilities[1].effect,
            Effect::Modify {
                stat: Stat::Shroud,
                delta: 2,
                scope: ModifierScope::WhileInPlay
            }
        ));

        // Forced: after attached location investigated -> discard self.
        assert!(matches!(
            &abilities[2].trigger,
            Trigger::OnEvent {
                pattern: EventPattern::AfterLocationInvestigated,
                timing: EventTiming::After,
                ..
            }
        ));
        assert!(matches!(&abilities[2].effect, Effect::DiscardSelf));

        assert!(native_effect_for(LIMIT1_ATTACH).is_some());
        assert!(native_effect_for("nope").is_none());
    }
}
