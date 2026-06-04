//! Shared leaf helpers for investigator-cursor navigation and stat mapping.
//!
//! These are pure lookup functions with no side effects; they call only into
//! `crate::state` / `crate::dsl` and are called by multiple dispatch handlers.

use crate::dsl::Stat;
use crate::state::{GameState, InvestigatorId, LocationId, SkillKind, Status};

/// Map a prey `Stat` to the `SkillKind` used for investigator lookup.
/// Only the four base skills are valid prey stats in Phase-4 scope; a
/// `MaxHealth`/`MaxSanity` prey would be a card-impl bug.
pub(super) fn stat_to_skill_kind(stat: Stat) -> SkillKind {
    match stat {
        Stat::Willpower => SkillKind::Willpower,
        Stat::Intellect => SkillKind::Intellect,
        Stat::Combat => SkillKind::Combat,
        Stat::Agility => SkillKind::Agility,
        Stat::MaxHealth | Stat::MaxSanity => unreachable!(
            "resolve_prey: prey stat {stat:?} is not a base skill; no in-scope \
             prey instruction uses MaxHealth/MaxSanity — card-impl bug"
        ),
    }
}

/// Investigators (Active, on the map) at `loc`, in `turn_order` order
/// so prey ties carry a deterministic, lead-first candidate list.
pub(super) fn active_investigators_at(state: &GameState, loc: LocationId) -> Vec<InvestigatorId> {
    state
        .turn_order
        .iter()
        .copied()
        .filter(|id| {
            state.investigators.get(id).is_some_and(|inv| {
                inv.status == Status::Active && inv.current_location == Some(loc)
            })
        })
        .collect()
}

/// `turn_order` entries whose status is `Active`, in turn order. Shared
/// by per-investigator Upkeep steps (4.2 reset, 4.4 draw + resource).
/// Eliminated investigators (Killed / Insane / Resigned) are excluded
/// per Rules Reference p.10.
pub(super) fn active_investigators_in_turn_order(state: &GameState) -> Vec<InvestigatorId> {
    state
        .turn_order
        .iter()
        .copied()
        .filter(|id| {
            state
                .investigators
                .get(id)
                .is_some_and(|inv| inv.status == Status::Active)
        })
        .collect()
}

/// First investigator in [`turn_order`] whose status is
/// [`Status::Active`]. Eliminated investigators
/// ([`Status::Killed`] / [`Status::Insane`] / [`Status::Resigned`])
/// are skipped per Rules Reference p.10 (Elimination).
///
/// Used by per-investigator phase loops to seed their cursor:
/// Mythos 1.4 draws ([`mythos_phase`] seeds `mythos_draw_pending`),
/// Enemy 3.3 attacks ([`enemy_phase`] seeds `enemy_attack_pending`).
///
/// [`turn_order`]: GameState::turn_order
/// [`mythos_phase`]: super::mythos_phase
/// [`enemy_phase`]: super::enemy_phase
pub(super) fn first_active_investigator(state: &GameState) -> Option<InvestigatorId> {
    state.turn_order.iter().copied().find(|id| {
        state
            .investigators
            .get(id)
            .is_some_and(|inv| inv.status == Status::Active)
    })
}

/// First investigator in [`turn_order`] whose status is
/// [`Status::Active`], positioned strictly after `current`. Returns
/// `None` when no Active investigator follows `current` in
/// `turn_order`, or when `current` is not in `turn_order` at all.
///
/// Eliminated investigators are skipped per Rules Reference p.10
/// (same predicate as [`first_active_investigator`]).
///
/// Used by per-investigator phase loops to advance their cursor:
/// `advance_mythos_draw_pending` after a draw chain completes, and
/// `run_window_continuation`'s `BeforeInvestigatorAttacked` arm after
/// one investigator's engaged-enemy attacks resolve.
///
/// Notable: `current` may itself be non-Active (e.g. defeated mid-loop
/// in Enemy phase) — using `turn_order` as the index basis (rather
/// than the filtered-Active list) makes this case the same single-pass
/// lookup.
///
/// [`turn_order`]: GameState::turn_order
pub(super) fn next_active_investigator_after(
    state: &GameState,
    current: InvestigatorId,
) -> Option<InvestigatorId> {
    state
        .turn_order
        .iter()
        .position(|id| *id == current)
        .and_then(|idx| {
            state.turn_order.iter().skip(idx + 1).copied().find(|id| {
                state
                    .investigators
                    .get(id)
                    .is_some_and(|inv| inv.status == Status::Active)
            })
        })
}
