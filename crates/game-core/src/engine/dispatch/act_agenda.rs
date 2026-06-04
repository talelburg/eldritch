//! Act and agenda handlers: doom placement, threshold checking, agenda
//! advancement, clue spending, and act advancement.

use crate::event::Event;
use crate::state::{GameState, InvestigatorId};

use super::super::outcome::EngineOutcome;

/// Mythos step 1.2 (Rules Reference p.24): "Take 1 doom from the token
/// pool, and place it on the current agenda card." No-op when no agenda
/// deck is modeled (tests/fixtures without an agenda).
pub(super) fn place_doom_on_agenda(state: &mut GameState, _events: &mut Vec<Event>) {
    if state.agenda_deck.is_empty() {
        return;
    }
    state.agenda_doom = state.agenda_doom.saturating_add(1);
}

/// Mythos step 1.3 (Rules Reference p.24): compare doom in play with the
/// current agenda's threshold; if met, the agenda advances. We model
/// doom only on the agenda (no corpus card carries doom yet — summing
/// "doom on each other card in play" would add zero).
///
/// TODO(#73 follow-up): sum doom on other cards in play once a
/// doom-bearing card exists.
///
/// If the current agenda is terminal (carries a `resolution`), advancing
/// it ends the scenario: set the resolution latch instead of moving the
/// cursor. Otherwise emit [`Event::AgendaAdvanced`], reset doom, and make
/// the next agenda current.
pub(super) fn check_doom_threshold(state: &mut GameState, events: &mut Vec<Event>) {
    if state.agenda_deck.is_empty() {
        return;
    }
    let agenda = &state.agenda_deck[state.agenda_index];
    if state.agenda_doom < agenda.doom_threshold {
        return;
    }
    match agenda.resolution.clone() {
        Some(resolution) => request_resolution(state, resolution),
        None => advance_agenda(state, events),
    }
}

/// Advance the agenda deck one step: emit [`Event::AgendaAdvanced`],
/// reset doom (Rules Reference p.24: "remove all doom from play"), and
/// move the cursor to the next agenda.
///
/// Only ever called for a *non-terminal* agenda (one whose `resolution`
/// is `None`). A non-terminal agenda must have a successor; reaching the
/// end of the deck without a resolution firing is malformed scenario
/// data (the final agenda must carry a `(→R#)` resolution point), so the
/// missing-successor case is `unreachable!()` — mirrors the surge-chain
/// malformation guards from #69.
pub(super) fn advance_agenda(state: &mut GameState, events: &mut Vec<Event>) {
    let from = state.agenda_index;
    events.push(Event::AgendaAdvanced { from });
    state.agenda_doom = 0;
    state.agenda_index += 1;
    if state.agenda_index >= state.agenda_deck.len() {
        unreachable!(
            "advance_agenda: agenda {from} advanced past the end of the deck without a \
             resolution firing — a terminal agenda must carry a resolution point; this is \
             malformed scenario data"
        );
    }
}

/// The investigators who may contribute clues to advance the act, in the
/// deterministic spend order: the acting investigator first, then the rest
/// of `turn_order`. Shared by [`advance_act_action`]'s clue-sufficiency
/// check and [`spend_clues`] so the validation domain and the spend domain
/// can never diverge.
fn clue_contributors(state: &GameState, acting: InvestigatorId) -> Vec<InvestigatorId> {
    std::iter::once(acting)
        .chain(state.turn_order.iter().copied().filter(|id| *id != acting))
        .collect()
}

/// Handler for [`PlayerAction::AdvanceAct`] — a prototype clue-spend to
/// advance the current act (see the action's doc comment and the design
/// spec). Validate-first: reject outside the Investigation phase, when no
/// act deck is modeled, or when the group holds fewer clues than the
/// current act's `clue_threshold`. On success spend exactly the threshold
/// (acting investigator first, then the rest in `turn_order`) and either
/// set the resolution latch (terminal act) or emit [`Event::ActAdvanced`]
/// and advance the cursor.
pub(super) fn advance_act_action(
    state: &mut GameState,
    events: &mut Vec<Event>,
    investigator: InvestigatorId,
) -> EngineOutcome {
    if state.phase != crate::state::Phase::Investigation {
        return EngineOutcome::Rejected {
            reason: format!(
                "AdvanceAct is only valid during the Investigation phase (was {:?})",
                state.phase
            )
            .into(),
        };
    }
    if state.act_deck.is_empty() {
        return EngineOutcome::Rejected {
            reason: "AdvanceAct: no act deck is modeled for this scenario".into(),
        };
    }
    let threshold = state.act_deck[state.act_index].clue_threshold;
    let total_clues: u32 = clue_contributors(state, investigator)
        .into_iter()
        .filter_map(|id| state.investigators.get(&id))
        .map(|i| u32::from(i.clues))
        .sum();
    if total_clues < u32::from(threshold) {
        return EngineOutcome::Rejected {
            reason: format!(
                "AdvanceAct: act requires {threshold} clues, group holds {total_clues}"
            )
            .into(),
        };
    }

    // All validations passed — mutate.
    spend_clues(state, investigator, threshold);
    match state.act_deck[state.act_index].resolution.clone() {
        Some(resolution) => request_resolution(state, resolution),
        None => advance_act(state, events),
    }
    EngineOutcome::Done
}

/// Spend `amount` clues from the group, deterministically: the acting
/// investigator's clues first, then the remaining investigators in
/// `turn_order`. Callers must have already validated the group holds at
/// least `amount` clues, so the spend always completes.
///
/// TODO(#153): let players choose who contributes when the group holds a
/// surplus (an `AwaitingInput` allocation prompt). The fixed order here is
/// outcome-equivalent single-player.
fn spend_clues(state: &mut GameState, acting: InvestigatorId, amount: u8) {
    let mut remaining = amount;
    for id in clue_contributors(state, acting) {
        if remaining == 0 {
            break;
        }
        if let Some(inv) = state.investigators.get_mut(&id) {
            let take = inv.clues.min(remaining);
            inv.clues -= take;
            remaining -= take;
        }
    }
    debug_assert_eq!(
        remaining, 0,
        "spend_clues called without enough clues in the group"
    );
}

/// Advance the act deck one step: emit [`Event::ActAdvanced`] and move the
/// cursor. Only called for a non-terminal act; the missing-successor case
/// is `unreachable!()` (a terminal act must carry a resolution point —
/// malformed scenario data otherwise). Mirrors [`advance_agenda`].
fn advance_act(state: &mut GameState, events: &mut Vec<Event>) {
    let from = state.act_index;
    events.push(Event::ActAdvanced { from });
    state.act_index += 1;
    if state.act_index >= state.act_deck.len() {
        unreachable!(
            "advance_act: act {from} advanced past the end of the deck without a resolution \
             firing — a terminal act must carry a resolution point; this is malformed \
             scenario data"
        );
    }
}

/// Set the scenario-resolution latch. First-writer-wins: a resolution
/// already latched this scenario is authoritative and a later request is
/// ignored. The `apply` hook (in `engine::mod`) observes the `None`→`Some`
/// transition to emit [`Event::ScenarioResolved`] and run the scenario
/// module's `apply_resolution` exactly once.
///
/// Call this only after a handler's validations pass: on a `Rejected`
/// outcome `apply` clears events but does not roll back `state`, so a
/// latch set on a doomed path would persist. All current callers latch
/// only on their success branches.
pub(super) fn request_resolution(state: &mut GameState, resolution: crate::scenario::Resolution) {
    if state.resolution.is_none() {
        state.resolution = Some(resolution);
    }
}

#[cfg(test)]
mod doom_agenda_tests {
    use super::*;
    use crate::event::Event;
    use crate::test_support::TestGame;
    use crate::{assert_event, assert_no_event};

    #[test]
    fn place_doom_increments_agenda_doom() {
        use crate::state::Agenda;
        let mut state = TestGame::new().build();
        state.agenda_deck = vec![Agenda {
            doom_threshold: 2,
            resolution: None,
        }];
        let mut events = Vec::new();
        place_doom_on_agenda(&mut state, &mut events);
        assert_eq!(state.agenda_doom, 1);
        place_doom_on_agenda(&mut state, &mut events);
        assert_eq!(state.agenda_doom, 2);
    }

    #[test]
    fn doom_threshold_advances_non_terminal_agenda() {
        use crate::scenario::Resolution;
        use crate::state::Agenda;
        let mut state = TestGame::new().build();
        state.agenda_deck = vec![
            Agenda {
                doom_threshold: 2,
                resolution: None,
            },
            Agenda {
                doom_threshold: 2,
                resolution: Some(Resolution::Lost {
                    reason: "agenda".into(),
                }),
            },
        ];
        state.agenda_doom = 2;
        let mut events = Vec::new();
        check_doom_threshold(&mut state, &mut events);
        assert_eq!(state.agenda_index, 1);
        assert_eq!(state.agenda_doom, 0, "doom resets on advance");
        assert!(
            state.resolution.is_none(),
            "non-terminal advance does not resolve"
        );
        assert_event!(events, Event::AgendaAdvanced { from } if *from == 0);
    }

    #[test]
    fn doom_threshold_on_terminal_agenda_sets_resolution_latch() {
        use crate::scenario::Resolution;
        use crate::state::Agenda;
        let mut state = TestGame::new().build();
        state.agenda_deck = vec![Agenda {
            doom_threshold: 2,
            resolution: Some(Resolution::Lost {
                reason: "doom".into(),
            }),
        }];
        state.agenda_doom = 2;
        let mut events = Vec::new();
        check_doom_threshold(&mut state, &mut events);
        assert_eq!(
            state.agenda_index, 0,
            "cursor does not move on a terminal agenda"
        );
        assert!(matches!(state.resolution, Some(Resolution::Lost { .. })));
        assert_no_event!(events, Event::AgendaAdvanced { .. });
    }

    #[test]
    fn doom_threshold_not_met_does_nothing() {
        use crate::state::Agenda;
        let mut state = TestGame::new().build();
        state.agenda_deck = vec![Agenda {
            doom_threshold: 3,
            resolution: None,
        }];
        state.agenda_doom = 2;
        let mut events = Vec::new();
        check_doom_threshold(&mut state, &mut events);
        assert_eq!(state.agenda_index, 0);
        assert_eq!(state.agenda_doom, 2);
        assert!(events.is_empty());
    }

    #[test]
    fn request_resolution_is_first_writer_wins() {
        use crate::scenario::Resolution;
        let mut state = TestGame::new().build();
        request_resolution(
            &mut state,
            Resolution::Lost {
                reason: "first".into(),
            },
        );
        request_resolution(
            &mut state,
            Resolution::Won {
                id: "second".into(),
            },
        );
        assert!(
            matches!(state.resolution, Some(Resolution::Lost { ref reason }) if reason == "first")
        );
    }
}

#[cfg(test)]
mod advance_act_tests {
    use crate::action::{Action, PlayerAction};
    use crate::engine::{apply, EngineOutcome};
    use crate::event::Event;
    use crate::state::{InvestigatorId, Phase};
    use crate::test_support::{test_investigator, TestGame};
    use crate::{assert_event, assert_no_event};

    #[test]
    fn advance_act_rejects_when_clues_insufficient() {
        use crate::state::Act;
        let inv = InvestigatorId(1);
        let mut investigator = test_investigator(1);
        investigator.clues = 1;
        let mut state = TestGame::new()
            .with_phase(Phase::Investigation)
            .with_investigator(investigator)
            .with_active_investigator(inv)
            .with_turn_order([inv])
            .build();
        state.act_deck = vec![Act {
            clue_threshold: 2,
            resolution: None,
        }];

        let result = apply(
            state,
            Action::Player(PlayerAction::AdvanceAct { investigator: inv }),
        );
        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
        assert_eq!(result.state.act_index, 0);
        assert_eq!(
            result.state.investigators[&inv].clues, 1,
            "no clues spent on reject"
        );
    }

    #[test]
    fn advance_act_spends_clues_and_advances_non_terminal() {
        use crate::scenario::Resolution;
        use crate::state::Act;
        let inv = InvestigatorId(1);
        let mut investigator = test_investigator(1);
        investigator.clues = 3;
        let mut state = TestGame::new()
            .with_phase(Phase::Investigation)
            .with_investigator(investigator)
            .with_active_investigator(inv)
            .with_turn_order([inv])
            .build();
        state.act_deck = vec![
            Act {
                clue_threshold: 2,
                resolution: None,
            },
            Act {
                clue_threshold: 2,
                resolution: Some(Resolution::Won { id: "demo".into() }),
            },
        ];

        let result = apply(
            state,
            Action::Player(PlayerAction::AdvanceAct { investigator: inv }),
        );
        assert_eq!(result.outcome, EngineOutcome::Done);
        assert_eq!(result.state.act_index, 1);
        assert_eq!(
            result.state.investigators[&inv].clues, 1,
            "spent exactly 2 of 3"
        );
        assert!(result.state.resolution.is_none());
        assert_event!(result.events, Event::ActAdvanced { from } if *from == 0);
    }

    #[test]
    fn advance_act_on_terminal_act_sets_resolution_latch() {
        use crate::scenario::Resolution;
        use crate::state::Act;
        let inv = InvestigatorId(1);
        let mut investigator = test_investigator(1);
        investigator.clues = 2;
        let mut state = TestGame::new()
            .with_phase(Phase::Investigation)
            .with_investigator(investigator)
            .with_active_investigator(inv)
            .with_turn_order([inv])
            .build();
        state.act_deck = vec![Act {
            clue_threshold: 2,
            resolution: Some(Resolution::Won { id: "demo".into() }),
        }];

        let result = apply(
            state,
            Action::Player(PlayerAction::AdvanceAct { investigator: inv }),
        );
        assert_eq!(result.outcome, EngineOutcome::Done);
        assert_eq!(
            result.state.act_index, 0,
            "cursor does not move on a terminal act"
        );
        assert!(matches!(
            result.state.resolution,
            Some(Resolution::Won { .. })
        ));
        assert_no_event!(result.events, Event::ActAdvanced { .. });
        assert_eq!(result.state.investigators[&inv].clues, 0);
    }

    #[test]
    fn advance_act_spends_acting_investigator_first_then_turn_order() {
        use crate::state::Act;
        let acting = InvestigatorId(1);
        let other = InvestigatorId(2);
        let mut inv1 = test_investigator(1);
        inv1.clues = 1;
        let mut inv2 = test_investigator(2);
        inv2.clues = 2;
        let mut state = TestGame::new()
            .with_phase(Phase::Investigation)
            .with_investigator(inv1)
            .with_investigator(inv2)
            .with_active_investigator(acting)
            .with_turn_order([acting, other])
            .build();
        // Two acts so the non-terminal first act can advance the cursor to 1
        // (a terminal `resolution: None` act at the end would hit the
        // advance-past-end `unreachable!`). The successor's contents are
        // irrelevant to this spend-order test.
        state.act_deck = vec![
            Act {
                clue_threshold: 2,
                resolution: None,
            },
            Act {
                clue_threshold: 2,
                resolution: None,
            },
        ];

        // Threshold 2: acting (1 clue) drained fully first, then 1 from `other`.
        let result = apply(
            state,
            Action::Player(PlayerAction::AdvanceAct {
                investigator: acting,
            }),
        );
        assert_eq!(result.outcome, EngineOutcome::Done);
        assert_eq!(
            result.state.investigators[&acting].clues, 0,
            "acting drained first"
        );
        assert_eq!(
            result.state.investigators[&other].clues, 1,
            "remainder taken from turn_order"
        );
        assert_eq!(result.state.act_index, 1);
    }
}
