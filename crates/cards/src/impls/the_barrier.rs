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
//! This module implements **both** of 01109's abilities. The **front
//! objective** (the round-end clue-spend advance) is a `When`-`RoundEnded`
//! reaction whose native does the group clue-spend + `advance_act`. The
//! `RoundEnded` `EmitEvent` coordinator surfaces it as the round-end `when`-cell
//! reaction candidate (#434); picking it fires this native, `Skip` declines.
//! The contributor location (the Hallway, `01112`) is printed-in-card — passed
//! to the native directly — not a framework data field (the former
//! `Act.round_end_advance` was deleted with the coordinator remodel).
//!
//! The **reverse** is a Forced on-advance ability that fires via
//! `ForcedTriggerPoint::ActAdvanced` when the act advances
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

use card_dsl::dsl::{
    forced_on_event, native, reaction_on_event, Ability, EventPattern, EventTiming,
};
use game_core::card_registry::{EligibilityFn, NativeEffectFn};
use game_core::state::GameState;
use game_core::{
    location_id_by_code, reveal_location, round_end_advance, round_end_advance_affordable,
    spawn_set_aside_enemy, Cx, EngineOutcome, EvalContext,
};

/// `ArkhamDB` code for Act 2, "The Barrier".
pub const CODE: &str = "01109";

/// Native-effect tag for this act's reverse.
const REVERSE: &str = "01109:reverse";

/// Native-effect tag for the front objective's round-end group clue-spend
/// advance ("When the round ends, investigators in the hallway may, as a group,
/// spend the requisite number of clues to advance").
pub(crate) const ROUND_END_ADVANCE: &str = "01109:round_end_advance";

/// Eligibility tag: the round-end advance may be offered only when the Hallway
/// group can afford the act's clue threshold (RR p.2 potential gate). Restores
/// the affordability gate the offer scan lost in the #434 coordinator remodel
/// (#470).
const CAN_ADVANCE: &str = "01109:can_advance";

/// Printed codes the reverse touches.
const GHOUL_PRIEST: &str = "01116";
const HALLWAY: &str = "01112";
const PARLOR: &str = "01115";

/// 01109's two abilities:
/// - the **front objective** — "When the round ends, investigators in the
///   Hallway may, as a group, spend the requisite number of clues to advance":
///   a `When`-timed `RoundEnded` reaction. The round-end `When` window offers
///   this as a single candidate (`PickSingle` = advance, Skip = decline); the
///   native spends + advances. Affordability is gated by the `01109:can_advance`
///   eligibility predicate (shared with the resolve-side
///   [`round_end_advance_affordable`]), so the candidate isn't offered when the
///   Hallway group can't afford the clue threshold (#470).
/// - the **reverse** — a Forced on-advance ability (reveal the Parlor + spawn
///   the Priest) that fires when the act advances.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![
        forced_on_event(
            EventPattern::ActAdvanced,
            EventTiming::After,
            native(REVERSE),
        ),
        reaction_on_event(
            EventPattern::RoundEnded,
            EventTiming::When,
            native(ROUND_END_ADVANCE),
        )
        .with_eligibility(CAN_ADVANCE),
    ]
}

/// Resolve 01109's native tags. Wired into the crate registry's
/// `native_effect_for`.
pub(crate) fn native_effect_for(tag: &str) -> Option<NativeEffectFn> {
    match tag {
        REVERSE => Some(reverse as NativeEffectFn),
        ROUND_END_ADVANCE => Some(advance_via_clue_spend as NativeEffectFn),
        _ => None,
    }
}

/// Resolve 01109's eligibility tag. The round-end advance is offered only when
/// the Hallway (01112) group can afford the act's clue threshold.
pub(crate) fn native_eligibility_for(tag: &str) -> Option<EligibilityFn> {
    match tag {
        CAN_ADVANCE => Some(can_advance as EligibilityFn),
        _ => None,
    }
}

/// True when the Hallway group can afford the current act's clue threshold —
/// the offer-side gate shared with the resolve-side `round_end_advance`.
fn can_advance(state: &GameState, _ctx: &EvalContext) -> bool {
    round_end_advance_affordable(state, HALLWAY)
}

/// Front-objective native: spend the act's `clue_threshold` from Hallway
/// (01112) investigators, then advance the act. Synchronous — the player's
/// choice was the round-end `When` window's `PickSingle`. Delegates to the
/// engine's generic group-spend entry, passing the printed contributor location.
fn advance_via_clue_spend(cx: &mut Cx, _ctx: &EvalContext) -> EngineOutcome {
    round_end_advance(cx, HALLWAY)
}

/// Reveal the Parlor (01115) and spawn the set-aside Ghoul Priest (01116)
/// in the Hallway (01112). Validate-first: the Parlor must be in play
/// before any mutation; the spawn ([`spawn_set_aside_enemy`]) validates
/// the set-aside zone + Hallway internally and rejects without mutating.
/// The Parlor is revealed only after a successful spawn, so a reject
/// leaves the board untouched. (Reveal vs. spawn order is functionally
/// independent — they touch disjoint state.)
fn reverse(cx: &mut Cx, _ctx: &EvalContext) -> EngineOutcome {
    let Some(parlor) = location_id_by_code(cx.state, PARLOR) else {
        return EngineOutcome::Rejected {
            reason: "01109 reverse: Parlor (01115) not in play".into(),
        };
    };
    let spawned = spawn_set_aside_enemy(cx, GHOUL_PRIEST, HALLWAY);
    if !matches!(spawned, EngineOutcome::Done) {
        return spawned;
    }
    reveal_location(cx, parlor);
    EngineOutcome::Done
}

#[cfg(test)]
mod tests {
    use card_dsl::dsl::{Effect, EventPattern, EventTiming, Trigger, TriggerKind};

    #[test]
    fn abilities_are_forced_reverse_then_when_round_end_advance() {
        let abilities = super::abilities();
        assert_eq!(abilities.len(), 2);
        // [0] the reverse: Forced on-advance native.
        assert_eq!(
            abilities[0].trigger,
            Trigger::OnEvent {
                pattern: EventPattern::ActAdvanced,
                timing: EventTiming::After,
                kind: TriggerKind::Forced,
            }
        );
        assert!(
            matches!(&abilities[0].effect, Effect::Native { tag } if tag == "01109:reverse"),
            "the reverse is a card-local native effect, got {:?}",
            abilities[0].effect
        );
        // [1] the front objective: a When-timed RoundEnded reaction → native.
        assert_eq!(
            abilities[1].trigger,
            Trigger::OnEvent {
                pattern: EventPattern::RoundEnded,
                timing: EventTiming::When,
                kind: TriggerKind::Reaction,
            }
        );
        assert!(
            matches!(&abilities[1].effect, Effect::Native { tag } if tag == "01109:round_end_advance"),
            "the round-end advance is a card-local native effect, got {:?}",
            abilities[1].effect
        );
    }

    #[test]
    fn native_effect_for_resolves_both_tags_only() {
        assert!(super::native_effect_for("01109:reverse").is_some());
        assert!(super::native_effect_for("01109:round_end_advance").is_some());
        assert!(super::native_effect_for("01109:other").is_none());
    }
}
