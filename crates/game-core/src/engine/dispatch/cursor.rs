//! Shared leaf helpers for the lead, player order, investigator-cursor
//! navigation and per-investigator scaling.
//!
//! These are pure lookup functions with no side effects; they call only into
//! `crate::state` / `card_dsl::dsl` and are called by multiple dispatch handlers.

use crate::state::{GameState, InvestigatorId, InvestigatorTurnFrame, LocationId, Phase, Status};

/// Investigators (Active, on the map) at `loc`, in [`player_order`], so a
/// prey tie's candidate list starts at the lead.
#[must_use]
pub fn active_investigators_at(state: &GameState, loc: LocationId) -> Vec<InvestigatorId> {
    player_order(state)
        .into_iter()
        .filter(|id| {
            state
                .investigators
                .get(id)
                .is_some_and(|inv| inv.current_location == Some(loc))
        })
        .collect()
}

/// The seating, [`GameState::turn_order`], rotated to start at the stored
/// [`GameState::lead`], eliminated investigators included. Unrotated when the
/// state names no lead, or one not seated.
fn seating_from_lead(state: &GameState) -> impl Iterator<Item = InvestigatorId> + '_ {
    let start = state
        .lead
        .and_then(|lead| state.turn_order.iter().position(|id| *id == lead))
        .unwrap_or(0);
    let (before, from_lead) = state.turn_order.split_at(start);
    from_lead.iter().chain(before).copied()
}

/// Whether `id` is seated and [`Status::Active`]. Eliminated investigators
/// (Defeated / Resigned) are not, per Rules Reference p.10 (Elimination).
fn is_active(state: &GameState, id: InvestigatorId) -> bool {
    state
        .investigators
        .get(&id)
        .is_some_and(|inv| inv.status == Status::Active)
}

/// The Active investigators in player order, starting at the lead.
///
/// `glossary/In_Player_Order.md`: *"the lead investigator performs his or her
/// part of the sequence first, followed by the other players in clockwise
/// order."* That is the seating,
/// [`GameState::turn_order`], rotated to start at the stored lead, with the
/// eliminated left out.
///
/// Every *"in player order"* step walks it: Mythos 1.4's draws, Enemy 3.3's
/// attacks, Upkeep 4.2's action reset and 4.4's draws and resources, the
/// setup mulligan, hand size, and an "each investigator" `Effect::ForEach`.
/// Investigation 2.2's turns walk it as a stand-in for the investigators'
/// choice. TODO(#1185): let the investigators choose who takes each turn.
#[must_use]
pub fn player_order(state: &GameState) -> Vec<InvestigatorId> {
    seating_from_lead(state)
        .filter(|id| is_active(state, *id))
        .collect()
}

/// The lead investigator now: the first investigator in [`player_order`].
///
/// That is the stored lead while they are Active. Once they are eliminated, it
/// is the next Active investigator after them in player order, a stand-in for
/// `glossary/Elimination.md` step 5's *"If the lead investigator is
/// eliminated, the remaining players (if any) choose a new lead
/// investigator."* (ADR 0019). `None` when nobody is Active.
#[must_use]
pub fn the_lead(state: &GameState) -> Option<InvestigatorId> {
    seating_from_lead(state).find(|id| is_active(state, *id))
}

/// The next Active investigator after `current` in player order, or `None`
/// when no Active investigator follows them before the rotation ends, or when
/// `current` is not seated.
///
/// Advances a per-investigator loop's cursor: the Investigation turns, Enemy
/// 3.3's attacks, Upkeep 4.4's draws. `current` may itself have been
/// eliminated mid-loop (defeated by an attack in Enemy 3.3); indexing the
/// whole seating rather than the Active list finds their successor anyway. The
/// rotation is taken at the *stored* lead, which never changes, so eliminating
/// the lead mid-loop cannot shift the walk.
pub(super) fn next_in_player_order(
    state: &GameState,
    current: InvestigatorId,
) -> Option<InvestigatorId> {
    seating_from_lead(state)
        .skip_while(|id| *id != current)
        .skip(1)
        .find(|id| is_active(state, *id))
}

/// `n` per investigator: [`scale_per_investigator`] by [`investigator_count`].
/// A location's per-investigator clue value scales by it.
pub(super) fn per_investigator(state: &GameState, n: u8) -> u8 {
    scale_per_investigator(n, investigator_count(state))
}

/// `n` times `count`, saturating at `u8::MAX`: the one per-investigator
/// scaling. [`per_investigator`] calls it with [`investigator_count`], and
/// [`Enemy::from_metadata`](crate::state::Enemy::from_metadata) with the count
/// its caller read from the same function, to scale a per-investigator health
/// value.
pub(crate) fn scale_per_investigator(n: u8, count: u8) -> u8 {
    n.saturating_mul(count)
}

/// The number of investigators who started the scenario, saturating at
/// `u8::MAX`.
///
/// Eliminated investigators count, as the rules' "started the scenario" asks:
/// the investigators map never drops one (an elimination flips its status), so
/// its length is that count. If that invariant changes, this must switch to a
/// stored started-count.
pub(super) fn investigator_count(state: &GameState) -> u8 {
    u8::try_from(state.investigators.len()).unwrap_or(u8::MAX)
}

/// Why it is not `investigator`'s turn, as [`your_turn`] reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NotYourTurn {
    /// The game is not in the Investigation phase; carries the phase it is in.
    WrongPhase(Phase),
    /// Another investigator, or none, is active; carries who is.
    NotActive(Option<InvestigatorId>),
}

/// Whether it is `investigator`'s turn: the Investigation phase, with them the
/// active investigator.
///
/// The phase is checked first, so outside the Investigation phase the answer is
/// [`NotYourTurn::WrongPhase`] whoever is active. Whether the turn is ending is
/// not asked here; [`turn_open`] adds that.
pub(crate) fn your_turn(
    state: &GameState,
    investigator: InvestigatorId,
) -> Result<(), NotYourTurn> {
    if state.phase != Phase::Investigation {
        return Err(NotYourTurn::WrongPhase(state.phase));
    }
    if state.active_investigator != Some(investigator) {
        return Err(NotYourTurn::NotActive(state.active_investigator));
    }
    Ok(())
}

/// Whether it is `investigator`'s turn and the turn is not yet ending.
///
/// That is the Investigation phase, with them the active investigator, and
/// their [`InvestigatorTurn`](crate::state::Continuation::InvestigatorTurn)
/// frame not armed to end.
///
/// The turn starts before its frame does. Rules Reference Appendix II step 2.2
/// begins the turn and opens a player window, and the frame is pushed when that
/// window closes, for step 2.2.1's actions. An additional action bought in that
/// window is bought during the turn, so this answers `true` there.
///
/// The turn's end forfeits the unused actions and arms the frame's `ending`
/// flag in one step, so from the `EndOfTurn` window on this is `false`: no
/// action is left to take. "Skids" O'Toole 01003's *"During your turn"* reads
/// it for the same reason: an additional action bought once the turn is ending
/// would be forfeited at once.
#[must_use]
pub fn turn_open(state: &GameState, investigator: InvestigatorId) -> bool {
    your_turn(state, investigator).is_ok()
        && !state
            .continuations
            .topmost_of::<InvestigatorTurnFrame>()
            .is_some_and(|turn| turn.investigator == investigator && turn.ending)
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
    state
        .continuations
        .topmost_of_mut::<InvestigatorTurnFrame>()
        .filter(|turn| turn.investigator == investigator)
        .map(|turn| &mut turn.ending)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{GameStateBuilder, Status};
    use crate::test_support::{self, TEST_REGISTRIES};

    #[test]
    fn player_order_excludes_eliminated() {
        // The setup mulligan queue (phases.rs) is seeded from this filter, so a
        // Defeated/Resigned investigator is structurally excluded — it never
        // gets prompted. inv1 is Defeated, inv2 is Active; only inv2 survives.
        let inv1 = InvestigatorId(1);
        let inv2 = InvestigatorId(2);
        let state = GameStateBuilder::new()
            .with_registries(TEST_REGISTRIES)
            .with_seat(test_support::seat(1).status(Status::Defeated))
            .with_seat(test_support::seat(2))
            .with_turn_order([inv1, inv2])
            .build();
        assert_eq!(
            player_order(&state),
            vec![inv2],
            "the eliminated inv1 is excluded; only the Active inv2 remains in player order"
        );
    }

    /// Three seats, with `statuses[i]` for seat `i + 1`, seated `[1, 2, 3]`
    /// with the lead at `lead`.
    fn three_seats(lead: u32, statuses: [Status; 3]) -> GameState {
        let [s1, s2, s3] = statuses;
        GameStateBuilder::new()
            .with_registries(TEST_REGISTRIES)
            .with_seat(test_support::seat(1).status(s1))
            .with_seat(test_support::seat(2).status(s2))
            .with_seat(test_support::seat(3).status(s3))
            .with_turn_order([InvestigatorId(1), InvestigatorId(2), InvestigatorId(3)])
            .with_lead(InvestigatorId(lead))
            .build()
    }

    const ALL_ACTIVE: [Status; 3] = [Status::Active, Status::Active, Status::Active];

    #[test]
    fn player_order_starts_at_the_lead_and_wraps_the_seating() {
        let state = three_seats(2, ALL_ACTIVE);
        assert_eq!(
            player_order(&state),
            vec![InvestigatorId(2), InvestigatorId(3), InvestigatorId(1)],
        );
        assert_eq!(the_lead(&state), Some(InvestigatorId(2)));
    }

    #[test]
    fn the_successor_walks_the_rotation_and_stops_at_its_end() {
        let state = three_seats(2, ALL_ACTIVE);
        assert_eq!(
            next_in_player_order(&state, InvestigatorId(2)),
            Some(InvestigatorId(3))
        );
        assert_eq!(
            next_in_player_order(&state, InvestigatorId(3)),
            Some(InvestigatorId(1)),
            "seat 1 follows seat 3 once the rotation wraps"
        );
        assert_eq!(
            next_in_player_order(&state, InvestigatorId(1)),
            None,
            "seat 1 is last in a rotation that started at seat 2"
        );
    }

    #[test]
    fn the_successor_skips_an_eliminated_seat() {
        let state = three_seats(1, [Status::Active, Status::Defeated, Status::Active]);
        assert_eq!(
            next_in_player_order(&state, InvestigatorId(1)),
            Some(InvestigatorId(3))
        );
    }

    #[test]
    fn the_successor_of_a_seat_not_seated_is_none() {
        let state = three_seats(1, ALL_ACTIVE);
        assert_eq!(next_in_player_order(&state, InvestigatorId(99)), None);
    }

    /// Seat 2 leads and is defeated mid-walk. The rotation stays at the
    /// stored lead, so seat 3 still follows them, and seat 3 now leads.
    #[test]
    fn an_eliminated_lead_keeps_the_rotation_and_passes_the_lead_on() {
        let state = three_seats(2, [Status::Active, Status::Defeated, Status::Active]);
        assert_eq!(
            next_in_player_order(&state, InvestigatorId(2)),
            Some(InvestigatorId(3)),
            "the defeated current still finds their successor"
        );
        assert_eq!(the_lead(&state), Some(InvestigatorId(3)));
        assert_eq!(
            player_order(&state),
            vec![InvestigatorId(3), InvestigatorId(1)]
        );
    }

    #[test]
    fn nobody_leads_when_every_seat_is_eliminated() {
        let state = three_seats(1, [Status::Defeated, Status::Resigned, Status::Defeated]);
        assert_eq!(the_lead(&state), None);
        assert!(player_order(&state).is_empty());
    }

    #[test]
    fn nobody_leads_an_unseated_state() {
        let state = GameStateBuilder::new().build();
        assert_eq!(state.lead, None);
        assert_eq!(the_lead(&state), None);
    }
}
