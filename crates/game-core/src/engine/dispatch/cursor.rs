//! Shared leaf helpers for investigator-cursor navigation and stat mapping.
//!
//! These are pure lookup functions with no side effects; they call only into
//! `crate::state` / `crate::dsl` and are called by multiple dispatch handlers.

use crate::state::{GameState, InvestigatorId, LocationId, Status};

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
/// Enemy 3.3 attacks ([`enemy_phase`] seeds the `EnemyPhase` anchor's
/// `attacking` cursor).
///
/// [`turn_order`]: GameState::turn_order
/// [`mythos_phase`]: super::phases::mythos_phase
/// [`enemy_phase`]: super::phases::enemy_phase
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
/// `anchor_on_child_pop`'s `BeforeInvestigatorAttacked` arm after
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

/// Mutable access to the `ending` flag on `investigator`'s open
/// [`InvestigatorTurn`](crate::state::Continuation::InvestigatorTurn) frame, or
/// `None` when they hold no open turn.
///
/// Setting the flag is how a turn is brought to Rules Reference Appendix II step
/// 2.2.2 without running the rotation inline: `drive`'s `ending: true` arm picks
/// the frame up once everything above it has unwound and runs
/// [`resume_end_turn`](super::phases::resume_end_turn). Two callers arm it, and
/// they differ only in whether a missing frame is possible —
/// [`end_turn`](super::phases::end_turn) is dispatched *from* the open turn so
/// its absence is state corruption, while
/// [`end_turn_on_elimination`](super::elimination) fires from anywhere a defeat
/// can happen and treats `None` as "not their turn, nothing to end". Keyed by
/// investigator rather than by "the topmost turn frame" so a bystander's defeat
/// cannot end the *active* investigator's turn.
///
/// Searched from the top down: only one `InvestigatorTurn` is ever on the stack
/// (2.2's "that investigator must complete the turn before another investigator
/// may take his or her turn"), so the direction is a formality.
pub(super) fn turn_frame_ending_mut(
    state: &mut GameState,
    investigator: InvestigatorId,
) -> Option<&mut bool> {
    state.continuations.iter_mut().rev().find_map(|c| match c {
        crate::state::Continuation::InvestigatorTurn {
            investigator: whose,
            ending,
        } if *whose == investigator => Some(ending),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::Status;
    use crate::test_support::{test_investigator, GameStateBuilder};

    #[test]
    fn active_investigators_in_turn_order_excludes_eliminated() {
        // The setup mulligan queue (phases.rs) is seeded from this filter, so a
        // Killed/Insane/Resigned investigator is structurally excluded — it never
        // gets prompted. inv1 is Killed, inv2 is Active; only inv2 survives.
        let inv1 = InvestigatorId(1);
        let inv2 = InvestigatorId(2);
        let mut a = test_investigator(1);
        a.status = Status::Killed;
        let b = test_investigator(2);
        let state = GameStateBuilder::new()
            .with_investigator(a)
            .with_investigator(b)
            .with_turn_order([inv1, inv2])
            .build();
        assert_eq!(
            active_investigators_in_turn_order(&state),
            vec![inv2],
            "the eliminated inv1 is excluded; only the Active inv2 remains in turn order"
        );
    }
}
