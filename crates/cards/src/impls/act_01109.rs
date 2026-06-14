//! The Barrier (The Gathering Act 2, 01109).
//!
//! ```text
//! Act 2 — The Barrier. Clues: 3.
//! Objective - When the round ends, investigators in the hallway may, as
//!   a group, spend the requisite number of clues to advance.
//! (reverse) The barrier blocking passage into the parlor has vanished.
//!   Reveal the Parlor.
//!   Put the set-aside Lita Chantler into play in the Parlor.
//!   Spawn the set-aside Ghoul Priest in the Hallway.
//! ```
//!
//! The front objective (the round-end clue-spend window) is a kernel
//! `Act.round_end_advance` field, set in `the_gathering::setup()` (C3d).
//! This module implements the **reverse**: a Forced on-advance ability
//! that fires via `ForcedTriggerPoint::ActAdvanced` when the act advances
//! (Rules Reference p.3: flip the card, follow the reverse). It reveals
//! the Parlor (01115) and spawns the set-aside Ghoul Priest (01116) in the
//! Hallway (01112), making act 3's "If the Ghoul Priest is Defeated,
//! advance" objective reachable in real play.
//!
//! "Put the set-aside Lita Chantler into play in the Parlor" is **out of
//! scope** here — Lita / the Parlor barrier / Resign are tracked in #258.
//!
//! Like 01108's board build, the reverse is board-dependent, single-use
//! scenario logic, so it lives card-locally as a [`card_dsl::dsl::Effect::Native`]
//! handler (#276) rather than as shared `Effect` variants. The spawn reuses
//! the engine's [`game_core::spawn_set_aside_enemy`], which mints the
//! Priest's combat stats / keywords / per-investigator health from the
//! corpus.
//!
//! Solo-scope note: with one investigator in the Hallway the Priest's
//! `Prey - Highest [combat]` resolves to that lone investigator and the
//! spawn engages inline (`Done`). A 2+-investigator Hallway with tied
//! combat would suspend (`AwaitingInput`) — unreachable through the
//! single-trigger forced-advance path until #213/#212; consistent with
//! the rest of Slice 1's solo-first scope.

use card_dsl::dsl::{native, on_event, Ability, EventPattern, EventTiming};
use game_core::card_registry::NativeEffectFn;
use game_core::{
    location_id_by_code, reveal_location, spawn_set_aside_enemy, Cx, EngineOutcome, EvalContext,
};

/// `ArkhamDB` code for Act 2, "The Barrier".
pub const CODE: &str = "01109";

/// Native-effect tag for this act's reverse.
const REVERSE: &str = "01109:reverse";

/// Printed codes the reverse touches.
const GHOUL_PRIEST: &str = "01116";
const HALLWAY: &str = "01112";
const PARLOR: &str = "01115";

/// 01109's Forced on-advance reverse: reveal the Parlor + spawn the Priest.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![on_event(
        EventPattern::ActAdvanced,
        EventTiming::After,
        native(REVERSE),
    )]
}

/// Resolve [`REVERSE`] if `tag` matches. Wired into the crate registry's
/// `native_effect_for`.
pub(crate) fn native_effect_for(tag: &str) -> Option<NativeEffectFn> {
    (tag == REVERSE).then_some(reverse as NativeEffectFn)
}

/// Reveal the Parlor (01115) and spawn the set-aside Ghoul Priest (01116)
/// in the Hallway (01112). Validate-first: the Parlor must be in play
/// before any mutation; the spawn ([`spawn_set_aside_enemy`]) validates
/// the set-aside zone + Hallway internally and rejects without mutating.
/// The Parlor is revealed only after a successful spawn, so a reject
/// leaves the board untouched. (Reveal vs. spawn order is functionally
/// independent — they touch disjoint state.)
fn reverse(cx: &mut Cx, ctx: &EvalContext) -> EngineOutcome {
    let Some(parlor) = location_id_by_code(cx.state, PARLOR) else {
        return EngineOutcome::Rejected {
            reason: "01109 reverse: Parlor (01115) not in play".into(),
        };
    };
    let spawned = spawn_set_aside_enemy(cx, ctx.controller, GHOUL_PRIEST, HALLWAY);
    if !matches!(spawned, EngineOutcome::Done) {
        return spawned;
    }
    reveal_location(cx, parlor);
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
                pattern: EventPattern::ActAdvanced,
                timing: EventTiming::After
            }
        );
        assert!(
            matches!(&abilities[0].effect, Effect::Native { tag } if tag == "01109:reverse"),
            "the reverse is a card-local native effect, got {:?}",
            abilities[0].effect
        );
    }

    #[test]
    fn native_effect_for_resolves_only_the_reverse_tag() {
        assert!(super::native_effect_for("01109:reverse").is_some());
        assert!(super::native_effect_for("01109:other").is_none());
    }
}
