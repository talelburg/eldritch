//! Act and agenda handlers: doom placement, threshold checking, agenda
//! advancement, clue spending, and act advancement.

use crate::state::{GameState, InvestigatorId, LocationId, Phase};

use super::super::outcome::EngineOutcome;
use super::Cx;

/// Mythos step 1.2 (Rules Reference p.24): "Take 1 doom from the token
/// pool, and place it on the current agenda card." No-op when no agenda
/// deck is modeled (tests/fixtures without an agenda).
pub(super) fn place_doom_on_agenda(cx: &mut Cx) {
    if cx.state.agenda_deck.is_empty() {
        return;
    }
    cx.state.agenda_doom = cx.state.agenda_doom.saturating_add(1);
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
pub(super) fn check_doom_threshold(cx: &mut Cx) {
    if cx.state.agenda_deck.is_empty() {
        return;
    }
    let agenda = &cx.state.agenda_deck[cx.state.agenda_index];
    if cx.state.agenda_doom < agenda.doom_threshold {
        return;
    }
    match agenda.resolution.clone() {
        Some(resolution) => request_resolution(cx.state, resolution),
        None => advance_agenda(cx),
    }
}

/// Place 1 doom on the current agenda and run the doom-threshold check
/// (which may advance the agenda or set its resolution). The card-facing
/// combination of `place_doom_on_agenda` + `check_doom_threshold`,
/// exposed `pub` for card-local native effects (Ancient Evils 01166,
/// "Place 1 doom on the current agenda. This effect can cause the current
/// agenda to advance."). No-op on an empty agenda deck — both helpers
/// guard.
pub fn place_doom_on_current_agenda(cx: &mut Cx) {
    place_doom_on_agenda(cx);
    check_doom_threshold(cx);
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
pub(super) fn advance_agenda(cx: &mut Cx) {
    let from = cx.state.agenda_index;
    let leaving_code = cx.state.agenda_deck[from].code.clone();
    cx.events.push(crate::event::Event::AgendaAdvanced { from });
    // Resolve the leaving agenda's Forced on-advance reverse effect before
    // the next agenda becomes current — the mirror of `advance_act`'s
    // `ActAdvanced` firing (`advance_agenda` fired nothing before #281).
    // The Gathering's reverses (01105 lead discard/horror, 01106
    // dig-until-Ghoul) resolve here. `()` return can't propagate a
    // 2+-trigger reject; `debug_assert!` guards it (mirror of `advance_act`).
    let forced = super::emit::emit_event(
        cx,
        &super::emit::TimingEvent::AgendaAdvanced { code: leaving_code },
    );
    debug_assert!(
        matches!(forced, EngineOutcome::Done),
        "advance_agenda on-advance forced did not resolve to Done: {forced:?} (2+ needs #213)"
    );
    cx.state.agenda_doom = 0;
    cx.state.agenda_index += 1;
    if cx.state.agenda_index >= cx.state.agenda_deck.len() {
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
pub(super) fn advance_act_action(cx: &mut Cx, investigator: InvestigatorId) -> EngineOutcome {
    if cx.state.phase != Phase::Investigation {
        return EngineOutcome::Rejected {
            reason: format!(
                "AdvanceAct is only valid during the Investigation phase (was {:?})",
                cx.state.phase
            )
            .into(),
        };
    }
    if cx.state.act_deck.is_empty() {
        return EngineOutcome::Rejected {
            reason: "AdvanceAct: no act deck is modeled for this scenario".into(),
        };
    }
    if cx.state.act_deck[cx.state.act_index]
        .round_end_advance
        .is_some()
    {
        return EngineOutcome::Rejected {
            reason: "this act advances only at the end of the round (its round-end \
                     objective), not via the AdvanceAct action"
                .into(),
        };
    }
    let threshold = cx.state.act_deck[cx.state.act_index].clue_threshold;
    let total_clues: u32 = clue_contributors(cx.state, investigator)
        .into_iter()
        .filter_map(|id| cx.state.investigators.get(&id))
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
    spend_clues(cx.state, investigator, threshold);
    match cx.state.act_deck[cx.state.act_index].resolution.clone() {
        Some(resolution) => request_resolution(cx.state, resolution),
        None => advance_act(cx),
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

/// Investigators currently at `location`, in `turn_order` (deterministic).
/// Used by the act round-end clue-spend window (01109: Hallway investigators).
pub(crate) fn investigators_at(state: &GameState, location: LocationId) -> Vec<InvestigatorId> {
    state
        .turn_order
        .iter()
        .copied()
        .filter(|id| state.investigators.get(id).and_then(|i| i.current_location) == Some(location))
        .collect()
}

/// Total clues held by `ids`.
pub(crate) fn clues_held(state: &GameState, ids: &[InvestigatorId]) -> u32 {
    ids.iter()
        .filter_map(|id| state.investigators.get(id))
        .map(|i| u32::from(i.clues))
        .sum()
}

/// Spend `amount` clues from `ids` in order. Caller must have validated the
/// group holds at least `amount` (via [`clues_held`]). Mirrors [`spend_clues`].
pub(crate) fn spend_clues_from(state: &mut GameState, ids: &[InvestigatorId], amount: u8) {
    let mut remaining = amount;
    for id in ids {
        if remaining == 0 {
            break;
        }
        if let Some(inv) = state.investigators.get_mut(id) {
            let take = inv.clues.min(remaining);
            inv.clues -= take;
            remaining -= take;
        }
    }
    debug_assert_eq!(remaining, 0, "spend_clues_from called without enough clues");
}

/// Advance the act deck one step: emit [`Event::ActAdvanced`], fire the
/// leaving act's Forced on-advance reverse effect via the registry, then
/// move the cursor. Only called for a non-terminal act; the
/// missing-successor case is `unreachable!()` (a terminal act must carry a
/// resolution point — malformed scenario data otherwise). Mirrors
/// [`advance_agenda`].
///
/// Invariant: the leaving act's on-advance Forced effect must not itself
/// re-advance the act (no in-scope card does; a re-advance from that
/// effect would recurse here). Revisit if such an ability lands.
pub(crate) fn advance_act(cx: &mut Cx) {
    let from = cx.state.act_index;
    let leaving_code = cx.state.act_deck[from].code.clone();
    cx.events.push(crate::event::Event::ActAdvanced { from });
    // Resolve the leaving act's Forced on-advance reverse effect (the
    // board world-build) before the next act becomes current — Rules
    // Reference p.3: flip the card, follow the reverse, then the next
    // card becomes current. `()` return can't propagate a 2+-trigger
    // reject; `debug_assert!` guards it (mirror of `upkeep_phase_end`).
    let forced = super::emit::emit_event(
        cx,
        &super::emit::TimingEvent::ActAdvanced { code: leaving_code },
    );
    debug_assert!(
        matches!(forced, EngineOutcome::Done),
        "advance_act on-advance forced did not resolve to Done: {forced:?} (2+ needs #213)"
    );
    cx.state.act_index += 1;
    if cx.state.act_index >= cx.state.act_deck.len() {
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
pub(crate) fn request_resolution(state: &mut GameState, resolution: crate::scenario::Resolution) {
    if state.resolution.is_none() {
        state.resolution = Some(resolution);
    }
}

#[cfg(test)]
mod doom_agenda_tests {
    use super::*;
    use crate::event::Event;
    use crate::test_support::GameStateBuilder;
    use crate::{assert_event, assert_no_event};

    #[test]
    fn place_doom_increments_agenda_doom() {
        use crate::state::{Agenda, CardCode};
        let mut state = GameStateBuilder::new().build();
        state.agenda_deck = vec![Agenda {
            code: CardCode("_test_agenda".into()),
            doom_threshold: 2,
            resolution: None,
        }];
        let mut events = Vec::new();
        place_doom_on_agenda(&mut Cx {
            state: &mut state,
            events: &mut events,
        });
        assert_eq!(state.agenda_doom, 1);
        place_doom_on_agenda(&mut Cx {
            state: &mut state,
            events: &mut events,
        });
        assert_eq!(state.agenda_doom, 2);
    }

    #[test]
    fn place_doom_on_current_agenda_advances_at_threshold() {
        use crate::state::{Agenda, CardCode};
        let mut state = GameStateBuilder::new().build();
        state.agenda_deck = vec![
            Agenda {
                code: CardCode("_agenda_1".into()),
                doom_threshold: 1,
                resolution: None,
            },
            Agenda {
                code: CardCode("_agenda_2".into()),
                doom_threshold: 3,
                resolution: None,
            },
        ];
        let mut events = Vec::new();
        place_doom_on_current_agenda(&mut Cx {
            state: &mut state,
            events: &mut events,
        });
        // Doom reached threshold (1) → agenda advanced + doom reset.
        assert_eq!(state.agenda_index, 1, "agenda advanced at threshold");
        assert_eq!(state.agenda_doom, 0, "doom reset on advance");
    }

    #[test]
    fn doom_threshold_advances_non_terminal_agenda() {
        use crate::scenario::Resolution;
        use crate::state::{Agenda, CardCode};
        let mut state = GameStateBuilder::new().build();
        state.agenda_deck = vec![
            Agenda {
                code: CardCode("_test_agenda_1".into()),
                doom_threshold: 2,
                resolution: None,
            },
            Agenda {
                code: CardCode("_test_agenda_2".into()),
                doom_threshold: 2,
                resolution: Some(Resolution::Lost {
                    reason: "agenda".into(),
                }),
            },
        ];
        state.agenda_doom = 2;
        let mut events = Vec::new();
        check_doom_threshold(&mut Cx {
            state: &mut state,
            events: &mut events,
        });
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
        use crate::state::{Agenda, CardCode};
        let mut state = GameStateBuilder::new().build();
        state.agenda_deck = vec![Agenda {
            code: CardCode("_test_agenda".into()),
            doom_threshold: 2,
            resolution: Some(Resolution::Lost {
                reason: "doom".into(),
            }),
        }];
        state.agenda_doom = 2;
        let mut events = Vec::new();
        check_doom_threshold(&mut Cx {
            state: &mut state,
            events: &mut events,
        });
        assert_eq!(
            state.agenda_index, 0,
            "cursor does not move on a terminal agenda"
        );
        assert!(matches!(state.resolution, Some(Resolution::Lost { .. })));
        assert_no_event!(events, Event::AgendaAdvanced { .. });
    }

    #[test]
    fn doom_threshold_not_met_does_nothing() {
        use crate::state::{Agenda, CardCode};
        let mut state = GameStateBuilder::new().build();
        state.agenda_deck = vec![Agenda {
            code: CardCode("_test_agenda".into()),
            doom_threshold: 3,
            resolution: None,
        }];
        state.agenda_doom = 2;
        let mut events = Vec::new();
        check_doom_threshold(&mut Cx {
            state: &mut state,
            events: &mut events,
        });
        assert_eq!(state.agenda_index, 0);
        assert_eq!(state.agenda_doom, 2);
        assert!(events.is_empty());
    }

    #[test]
    fn request_resolution_is_first_writer_wins() {
        use crate::scenario::Resolution;
        let mut state = GameStateBuilder::new().build();
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
    use crate::test_support::{test_investigator, GameStateBuilder};
    use crate::{assert_event, assert_no_event};

    #[test]
    fn advance_act_rejects_when_clues_insufficient() {
        use crate::state::{Act, CardCode};
        let inv = InvestigatorId(1);
        let mut investigator = test_investigator(1);
        investigator.clues = 1;
        let mut state = GameStateBuilder::new()
            .with_phase(Phase::Investigation)
            .with_investigator(investigator)
            .with_active_investigator(inv)
            .with_turn_order([inv])
            .build();
        state.act_deck = vec![Act {
            code: CardCode("_test_act".into()),
            clue_threshold: 2,
            resolution: None,
            round_end_advance: None,
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
    fn advance_act_rejected_for_round_end_advance_act() {
        use crate::state::{Act, CardCode, RoundEndAdvance};
        let inv = InvestigatorId(1);
        let mut investigator = test_investigator(1);
        investigator.clues = 9; // plenty — reject must be the objective, not affordability
        let mut state = GameStateBuilder::new()
            .with_phase(Phase::Investigation)
            .with_investigator(investigator)
            .with_active_investigator(inv)
            .with_turn_order([inv])
            .build();
        state.act_deck = vec![Act {
            code: CardCode("01109".into()),
            clue_threshold: 3,
            resolution: None,
            round_end_advance: Some(RoundEndAdvance {
                contributor_location: CardCode("01112".into()),
            }),
        }];

        let result = apply(
            state,
            Action::Player(PlayerAction::AdvanceAct { investigator: inv }),
        );
        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
        assert_eq!(result.state.act_index, 0, "act did not advance");
        assert_eq!(result.state.investigators[&inv].clues, 9, "no clues spent");
    }

    #[test]
    fn advance_act_spends_clues_and_advances_non_terminal() {
        use crate::scenario::Resolution;
        use crate::state::{Act, CardCode};
        let inv = InvestigatorId(1);
        let mut investigator = test_investigator(1);
        investigator.clues = 3;
        let mut state = GameStateBuilder::new()
            .with_phase(Phase::Investigation)
            .with_investigator(investigator)
            .with_active_investigator(inv)
            .with_turn_order([inv])
            .build();
        state.act_deck = vec![
            Act {
                code: CardCode("_test_act_1".into()),
                clue_threshold: 2,
                resolution: None,
                round_end_advance: None,
            },
            Act {
                code: CardCode("_test_act_2".into()),
                clue_threshold: 2,
                resolution: Some(Resolution::Won { id: "demo".into() }),
                round_end_advance: None,
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
        use crate::state::{Act, CardCode};
        let inv = InvestigatorId(1);
        let mut investigator = test_investigator(1);
        investigator.clues = 2;
        let mut state = GameStateBuilder::new()
            .with_phase(Phase::Investigation)
            .with_investigator(investigator)
            .with_active_investigator(inv)
            .with_turn_order([inv])
            .build();
        state.act_deck = vec![Act {
            code: CardCode("_test_act".into()),
            clue_threshold: 2,
            resolution: Some(Resolution::Won { id: "demo".into() }),
            round_end_advance: None,
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
    fn advance_act_without_registry_still_advances() {
        use crate::scenario::Resolution;
        use crate::state::{Act, CardCode, InvestigatorId, Phase};
        use crate::test_support::{test_investigator, GameStateBuilder};
        let inv = InvestigatorId(1);
        let mut investigator = test_investigator(1);
        investigator.clues = 2;
        let mut state = GameStateBuilder::new()
            .with_phase(Phase::Investigation)
            .with_investigator(investigator)
            .with_active_investigator(inv)
            .with_turn_order([inv])
            .build();
        state.act_deck = vec![
            Act {
                code: CardCode("01108".into()),
                clue_threshold: 2,
                resolution: None,
                round_end_advance: None,
            },
            Act {
                code: CardCode("01109".into()),
                clue_threshold: 3,
                resolution: Some(Resolution::Won { id: "R1".into() }),
                round_end_advance: None,
            },
        ];
        let result = apply(
            state,
            Action::Player(PlayerAction::AdvanceAct { investigator: inv }),
        );
        assert_eq!(result.outcome, EngineOutcome::Done);
        assert_eq!(
            result.state.act_index, 1,
            "cursor advances even with no forced ability"
        );
    }

    #[test]
    fn advance_act_spends_acting_investigator_first_then_turn_order() {
        use crate::state::{Act, CardCode};
        let acting = InvestigatorId(1);
        let other = InvestigatorId(2);
        let mut inv1 = test_investigator(1);
        inv1.clues = 1;
        let mut inv2 = test_investigator(2);
        inv2.clues = 2;
        let mut state = GameStateBuilder::new()
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
                code: CardCode("_test_act_1".into()),
                clue_threshold: 2,
                resolution: None,
                round_end_advance: None,
            },
            Act {
                code: CardCode("_test_act_2".into()),
                clue_threshold: 2,
                resolution: None,
                round_end_advance: None,
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
