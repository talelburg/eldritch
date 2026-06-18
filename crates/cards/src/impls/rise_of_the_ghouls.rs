//! Rise of the Ghouls (The Gathering Agenda 2, 01106).
//!
//! ```text
//! Agenda 2 — Rise of the Ghouls. Doom: 7.
//! (reverse) Shuffle the encounter discard pile into the encounter deck.
//!   Discard cards from the top of the encounter deck until a [[Ghoul]]
//!   enemy is discarded. The lead investigator draws that enemy.
//! ```
//!
//! Forced on-advance reverse, fired via `ForcedTriggerPoint::AgendaAdvanced`
//! from `advance_agenda` (the mirror of the act path). Board-dependent,
//! single-use scenario logic, so it lives card-locally as an
//! `Effect::Native` handler (#276), orchestrating engine primitives
//! ([`reshuffle_encounter_discard`], [`resolve_encounter_card`]) over the
//! encounter deck.
//!
//! The dug-up enemy is a *generic* `Ghoul` from the encounter deck —
//! distinct from act-2's set-aside Ghoul Priest (01116, #280). "Draws that
//! enemy" resolves it through the normal encounter-draw path
//! ([`resolve_encounter_card`]): `CardRevealed`, any Revelation, then spawn.
//!
//! Empty-deck note: The Gathering's encounter deck isn't assembled yet, so
//! today the reshuffle finds an empty discard and the dig an empty deck —
//! the reverse is a faithful no-op (RR: "discard … until a `Ghoul` …";
//! a deck with no Ghoul yields none). The algorithm is exercised against a
//! seeded deck in `cards/tests/agenda_reverses.rs`.
//!
//! Solo-scope note: spawning the drawn Ghoul engages inline (`Done`) with
//! one investigator; a tied-prey multi-investigator spawn would suspend
//! (`AwaitingInput`) — unreachable through the single-trigger
//! forced-advance path until #212/#213, consistent with Slice 1.

use card_dsl::card_data::CardType;
use card_dsl::dsl::{forced_on_event, native, Ability, EventPattern, EventTiming};
use game_core::card_registry::NativeEffectFn;
use game_core::{
    reshuffle_encounter_discard, resolve_encounter_card, Cx, EngineOutcome, EvalContext,
};

/// `ArkhamDB` code for Agenda 2, "Rise of the Ghouls".
pub const CODE: &str = "01106";

/// Native-effect tag for this agenda's reverse.
const REVERSE: &str = "01106:reverse";

#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![forced_on_event(
        EventPattern::AgendaAdvanced,
        EventTiming::After,
        native(REVERSE),
    )]
}

/// Resolve [`REVERSE`] if `tag` matches. Wired into the crate registry's
/// `native_effect_for`.
pub(crate) fn native_effect_for(tag: &str) -> Option<NativeEffectFn> {
    (tag == REVERSE).then_some(reverse as NativeEffectFn)
}

/// `true` if `code` is an encounter **enemy** carrying the `Ghoul` trait.
fn is_ghoul_enemy(code: &str) -> bool {
    crate::by_code(code)
        .is_some_and(|m| m.card_type() == CardType::Enemy && m.traits.iter().any(|t| t == "Ghoul"))
}

/// Shuffle the encounter discard into the deck, then dig from the top —
/// discarding each non-`Ghoul`-enemy — until a `Ghoul` enemy is found, and
/// have the lead investigator draw it. A deck exhausted without a Ghoul is
/// a no-op (nothing to draw).
fn reverse(cx: &mut Cx, ctx: &EvalContext) -> EngineOutcome {
    reshuffle_encounter_discard(cx);
    // Pop directly from the deck (not `draw_encounter_top`, which would
    // reshuffle the just-discarded cards back in and loop). The deck is
    // finite and shrinks by one each iteration, so this terminates.
    while let Some(code) = cx.state.encounter_deck.pop_front() {
        if is_ghoul_enemy(code.as_str()) {
            let metadata = crate::by_code(code.as_str()).expect("is_ghoul_enemy looked it up");
            return resolve_encounter_card(cx, ctx.controller, code, metadata);
        }
        cx.state.encounter_discard.push(code);
    }
    EngineOutcome::Done
}

#[cfg(test)]
mod tests {
    use card_dsl::dsl::{Effect, EventPattern, EventTiming, Trigger};

    #[test]
    fn abilities_are_one_forced_on_advance_native_reverse() {
        let abilities = super::abilities();
        assert_eq!(abilities.len(), 1);
        assert_eq!(
            abilities[0].trigger,
            Trigger::OnEvent {
                pattern: EventPattern::AgendaAdvanced,
                timing: EventTiming::After,
                kind: card_dsl::dsl::TriggerKind::Forced,
            }
        );
        assert!(
            matches!(&abilities[0].effect, Effect::Native { tag } if tag == "01106:reverse"),
            "the reverse is a card-local native effect, got {:?}",
            abilities[0].effect
        );
    }

    #[test]
    fn native_effect_for_resolves_only_the_reverse_tag() {
        assert!(super::native_effect_for("01106:reverse").is_some());
        assert!(super::native_effect_for("01106:other").is_none());
    }

    #[test]
    fn is_ghoul_enemy_recognises_ghoul_enemies_only() {
        assert!(
            super::is_ghoul_enemy("01160"),
            "Ghoul Minion is a Ghoul enemy"
        );
        assert!(
            super::is_ghoul_enemy("01116"),
            "Ghoul Priest is a Ghoul enemy",
        );
        assert!(
            !super::is_ghoul_enemy("01105"),
            "an agenda is not a Ghoul enemy",
        );
    }
}
