//! Act and agenda handlers: doom placement, threshold checking, agenda
//! advancement, clue spending, and act advancement.

use std::borrow::Cow;

use crate::state::{GameState, InvestigatorId, LocationId, Phase};

use super::super::outcome::EngineOutcome;
use super::Cx;

/// Whether the current act advances *only* at the end of the round (its
/// round-end objective — act 01109's `When`-`RoundEnded` group advance), in
/// which case the `AdvanceAct` action is rejected. Detected from the registry
/// (#434 — replaces the former `Act.round_end_advance` data field; the
/// contributor location is now printed-in-card on the ability's native).
fn act_advances_at_round_end(state: &GameState) -> bool {
    let Some(act) = state.act_deck.get(state.act_index) else {
        return false;
    };
    let Some(reg) = crate::card_registry::current() else {
        return false;
    };
    let Some(abilities) = (reg.abilities_for)(&act.code) else {
        return false;
    };
    abilities.iter().any(|a| {
        matches!(
            &a.trigger,
            crate::dsl::Trigger::OnEvent {
                pattern: crate::dsl::EventPattern::RoundEnded,
                timing: crate::dsl::EventTiming::When,
                kind: crate::dsl::TriggerKind::Reaction,
            }
        )
    })
}

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

/// Advance the agenda deck one step (#482): push an
/// [`AdvanceReverse`](crate::state::Continuation::AdvanceReverse) frame and let
/// the `drive` loop run it — emit [`Event::AgendaAdvanced`], optionally pause for
/// the gated acknowledge, fire the leaving agenda's Forced reverse (which may
/// suspend), then reset doom + move the cursor at `Finalize` (RR order; the
/// past-the-end terminal guard lives in `advance_reverse::finalize`).
///
/// Only ever called for a *non-terminal* agenda (`resolution` is `None`); a
/// terminal agenda latches its resolution via `request_resolution` instead.
pub(super) fn advance_agenda(cx: &mut Cx) {
    let from = cx.state.agenda_index;
    let leaving_code = cx.state.agenda_deck[from].code.clone();
    // Defer to the resumable AdvanceReverse sub-process (#482): it pushes the
    // observable event, optionally pauses for the gated acknowledge, fires the
    // leaving agenda's Forced reverse (which may suspend — 01105's ChooseOne),
    // then bumps the cursor at Finalize (RR order — after the reverse resolves).
    // The drive loop owns it from here; the past-the-end terminal guard now lives
    // in `advance_reverse::finalize`.
    cx.state
        .continuations
        .push(crate::state::Continuation::AdvanceReverse {
            deck: crate::state::AdvanceDeck::Agenda,
            from,
            leaving_code,
            step: crate::state::AdvanceStep::AwaitAck,
            // Agenda advances are always game-forced (a doom threshold).
            trigger: crate::state::AdvanceTrigger::Forced,
        });
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

/// Validate the `AdvanceAct` action
/// without mutating: Investigation phase, a modeled act deck, the current act
/// advances via the action (not a round-end objective), and the group holds at
/// least the act's clue threshold. Returns the threshold on success (so the
/// handler can spend it) or the rejection reason on failure. The enumerator
/// (slice 2a-ii-4, #393) calls this in "is-legal?" mode; [`advance_act_action`]
/// calls it then mutates.
pub(crate) fn check_advance_act(
    state: &GameState,
    investigator: InvestigatorId,
) -> Result<u8, Cow<'static, str>> {
    if state.phase != Phase::Investigation {
        return Err(format!(
            "AdvanceAct is only valid during the Investigation phase (was {:?})",
            state.phase
        )
        .into());
    }
    if state.act_deck.is_empty() {
        return Err("AdvanceAct: no act deck is modeled for this scenario".into());
    }
    if act_advances_at_round_end(state) {
        return Err(
            "this act advances only at the end of the round (its round-end \
                    objective), not via the AdvanceAct action"
                .into(),
        );
    }
    let threshold = state.act_deck[state.act_index].clue_threshold;
    // The AdvanceAct action is the deliberate clue-spend advance, which is only
    // meaningful for an act that advances by spending clues (threshold >= 1). A
    // zero threshold (corpus `null`) marks a non-clue objective — e.g. The
    // Gathering's Act 3 (01110), which advances via its Forced EnemyDefeated on
    // the Ghoul Priest. Offering the action there would let the player "spend 0
    // clues to advance", bypassing the objective (#486 — an instant win on a
    // terminal-Won act). Unlike `act_advances_at_round_end` (registry-based, so
    // it silently fails open with no registry installed), this is a self-contained
    // invariant. Act 01109's round-end objective carries a positive threshold and
    // stays excluded by `act_advances_at_round_end` above.
    if threshold == 0 {
        return Err(
            "this act advances on a non-clue objective, not via the AdvanceAct \
                    action"
                .into(),
        );
    }
    let total_clues: u32 = clue_contributors(state, investigator)
        .into_iter()
        .filter_map(|id| state.investigators.get(&id))
        .map(|i| u32::from(i.clues))
        .sum();
    if total_clues < u32::from(threshold) {
        return Err(format!(
            "AdvanceAct: act requires {threshold} clues, group holds {total_clues}"
        )
        .into());
    }
    Ok(threshold)
}

/// Handler for `TurnAction::AdvanceAct`:
/// validate via [`check_advance_act`], then (on success) spend exactly the act's
/// clue threshold (acting investigator first, then the rest in `turn_order`) and
/// either set the resolution latch (terminal act) or emit [`Event::ActAdvanced`]
/// and advance the cursor.
pub(super) fn advance_act_action(cx: &mut Cx, investigator: InvestigatorId) -> EngineOutcome {
    let threshold = match check_advance_act(cx.state, investigator) {
        Ok(t) => t,
        Err(reason) => return EngineOutcome::Rejected { reason },
    };

    // All validations passed — mutate.
    spend_clues(cx.state, investigator, threshold);
    match cx.state.act_deck[cx.state.act_index].resolution.clone() {
        Some(resolution) => request_resolution(cx.state, resolution),
        // The `AdvanceAct` action *is* the player's flip — a deliberate advance.
        None => advance_act(cx, crate::state::AdvanceTrigger::Deliberate),
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

/// Whether the current act's round-end group clue-spend advance is affordable:
/// investigators at `contributor_location_code` hold at least the current act's
/// `clue_threshold`. Shared by the offer-side eligibility predicate (act 01109's
/// `01109:can_advance`) and the resolve-side [`round_end_advance`], so the two
/// can't drift. `false` when there is no current act or the location isn't in
/// play (the "investigators in the Hallway" condition is subsumed — 0
/// contributors ⇒ 0 clues ⇒ not affordable).
#[must_use]
pub fn round_end_advance_affordable(state: &GameState, contributor_location_code: &str) -> bool {
    let Some(act) = state.act_deck.get(state.act_index) else {
        return false;
    };
    let threshold = act.clue_threshold;
    let Some(loc) = crate::engine::location_id_by_code(state, contributor_location_code) else {
        return false;
    };
    clues_held(state, &investigators_at(state, loc)) >= u32::from(threshold)
}

/// Round-end group clue-spend act advance — the generic mechanics behind act
/// 01109's "investigators in the Hallway may, as a group, spend the requisite
/// number of clues to advance" (the only card-specific datum is the contributor
/// location, passed in). If [`round_end_advance_affordable`], spends the act's
/// `clue_threshold` from the contributors (turn order) and advances the act.
///
/// Affordability is gated at the offer side by act 01109's `01109:can_advance`
/// eligibility predicate (which calls [`round_end_advance_affordable`]), so the
/// insufficient-clues branch here is a defensive backstop. Exposed for the
/// `cards` registry's 01109 native handler.
///
/// # Panics
///
/// Panics if `contributor_location_code` is not a location in play. This is
/// unreachable in practice: the affordability check above returns early unless
/// the location is present, so reaching the `expect` would mean
/// [`round_end_advance_affordable`] and [`location_id_by_code`](crate::engine::location_id_by_code)
/// disagree about the same code.
pub fn round_end_advance(cx: &mut Cx, contributor_location_code: &str) -> EngineOutcome {
    if !round_end_advance_affordable(cx.state, contributor_location_code) {
        return EngineOutcome::Rejected {
            reason: "round_end_advance: contributors no longer hold enough clues".into(),
        };
    }
    let threshold = cx.state.act_deck[cx.state.act_index].clue_threshold;
    let loc = crate::engine::location_id_by_code(cx.state, contributor_location_code)
        .expect("affordable ⇒ contributor location in play");
    let contributors = investigators_at(cx.state, loc);
    spend_clues_from(cx.state, &contributors, threshold);
    // The round-end objective (01109) is a deliberate, player-chosen advance.
    advance_act(cx, crate::state::AdvanceTrigger::Deliberate);
    EngineOutcome::Done
}

/// Advance the act deck one step (#482): push an
/// [`AdvanceReverse`](crate::state::Continuation::AdvanceReverse) frame for the
/// `drive` loop to run — emit [`Event::ActAdvanced`], optionally pause for the
/// gated acknowledge, fire the leaving act's Forced reverse (which may suspend),
/// then move the cursor at `Finalize` (RR order; the past-the-end terminal guard
/// lives in `advance_reverse::finalize`). Mirrors [`advance_agenda`]. Only called
/// for a non-terminal act.
///
/// Invariant: the leaving act's on-advance Forced effect must not itself
/// re-advance the act (no in-scope card does; a re-advance from that
/// effect would recurse here). Revisit if such an ability lands.
///
/// `trigger` records why the act is advancing so the sub-process can decide
/// whether to prompt the on-card flip: `Deliberate` for the player-driven
/// `AdvanceAct` action and the round-end objective, `Forced` for 01110's
/// Ghoul-Priest-defeat forced advance (#558).
pub(crate) fn advance_act(cx: &mut Cx, trigger: crate::state::AdvanceTrigger) {
    let from = cx.state.act_index;
    let leaving_code = cx.state.act_deck[from].code.clone();
    // Mirror of advance_agenda (#482): defer to the resumable AdvanceReverse
    // sub-process (observable event → gated acknowledge → leaving act's Forced
    // reverse, which may suspend → bump the cursor at Finalize, RR order). The
    // drive loop owns it; the past-the-end terminal guard lives in
    // `advance_reverse::finalize`.
    cx.state
        .continuations
        .push(crate::state::Continuation::AdvanceReverse {
            deck: crate::state::AdvanceDeck::Act,
            from,
            leaving_code,
            step: crate::state::AdvanceStep::AwaitAck,
            trigger,
        });
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
        // The advance is deferred to an AdvanceReverse frame (#482); drive it
        // (no registry ⇒ the reverse fires nothing ⇒ it drives straight through).
        crate::engine::dispatch::drive(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            EngineOutcome::Done,
        );
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
        // The advance is deferred to an AdvanceReverse frame (#482); drive it.
        crate::engine::dispatch::drive(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            EngineOutcome::Done,
        );
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
    use crate::engine::enumerate::{legal_actions, TurnAction};
    use crate::engine::EngineOutcome;
    use crate::event::Event;
    use crate::state::{InvestigatorId, Phase};
    use crate::test_support::{take_turn_action, test_investigator, GameStateBuilder};
    use crate::{assert_event, assert_no_event};

    #[test]
    fn round_end_advance_affordable_tracks_hallway_clues_vs_threshold() {
        use crate::state::{Act, CardCode, LocationId};
        use crate::test_support::test_location;

        // A location coded "HALL"; the investigator stands on it.
        let mut hall = test_location(1, "Hallway");
        hall.code = CardCode("HALL".into());
        let mut investigator = test_investigator(1);
        investigator.current_location = Some(LocationId(1));
        let mut state = GameStateBuilder::new()
            .with_location(hall)
            .with_investigator(investigator)
            .with_turn_order([InvestigatorId(1)])
            .build();
        state.act_deck = vec![Act {
            code: CardCode("_act".into()),
            clue_threshold: 3,
            resolution: None,
        }];
        state.act_index = 0;

        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .clues = 2;
        assert!(
            !super::round_end_advance_affordable(&state, "HALL"),
            "2 < 3 → not affordable"
        );
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .clues = 3;
        assert!(
            super::round_end_advance_affordable(&state, "HALL"),
            "3 >= 3 → affordable"
        );
    }

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
            .with_phase_anchor(crate::state::Continuation::InvestigationPhase {
                resume: crate::state::InvestigationResume::TurnBegins,
            })
            .with_investigator_turn(inv)
            .build();
        state.act_deck = vec![Act {
            code: CardCode("_test_act".into()),
            clue_threshold: 2,
            resolution: None,
        }];

        // Insufficient clues (1 < 2): AdvanceAct is not legal.
        assert!(
            !legal_actions(&state)
                .iter()
                .any(|a| matches!(a, TurnAction::AdvanceAct { .. })),
            "AdvanceAct must not be legal when clues < threshold"
        );
    }

    #[test]
    fn advance_act_action_rejected_for_zero_clue_threshold_objective() {
        use crate::state::{Act, CardCode};
        // A non-clue-objective act (clue_threshold 0 — e.g. The Gathering's
        // Act 3 01110, which advances when the Ghoul Priest is defeated). The
        // deliberate clue-spend AdvanceAct action is nonsensical here ("spend 0
        // clues to advance" / "spend 0 clues to instantly win"), so it must be
        // neither offered nor accepted — even with no registry installed (this
        // is a pure game-core unit test, so none is).
        let inv = InvestigatorId(1);
        let mut investigator = test_investigator(1);
        investigator.clues = 5; // plenty — reject must be the objective, not affordability
        let mut state = GameStateBuilder::new()
            .with_phase(Phase::Investigation)
            .with_investigator(investigator)
            .with_active_investigator(inv)
            .with_turn_order([inv])
            .with_phase_anchor(crate::state::Continuation::InvestigationPhase {
                resume: crate::state::InvestigationResume::TurnBegins,
            })
            .with_investigator_turn(inv)
            .build();
        state.act_deck = vec![Act {
            code: CardCode("01110".into()),
            clue_threshold: 0,
            resolution: Some(crate::scenario::Resolution::Won { id: "R1".into() }),
        }];

        assert!(
            !legal_actions(&state)
                .iter()
                .any(|a| matches!(a, TurnAction::AdvanceAct { .. })),
            "AdvanceAct must not be offered for a zero-threshold objective act"
        );
        // Bypass the legality menu (the assert above already proves it's not
        // offered) to confirm the handler itself rejects, leaving state unchanged.
        let result = crate::test_support::dispatch_turn_action_unchecked(
            state,
            &TurnAction::AdvanceAct { investigator: inv },
        );
        assert!(
            matches!(result.outcome, EngineOutcome::Rejected { .. }),
            "AdvanceAct must be rejected for a zero-threshold objective act"
        );
        assert!(
            result.state.resolution.is_none(),
            "rejected AdvanceAct must not latch the act's Won resolution"
        );
        assert_eq!(
            result.state.investigators[&inv].clues, 5,
            "rejected AdvanceAct must not spend clues"
        );
    }

    // `advance_act_rejected_for_round_end_advance_act` moved to
    // `crates/cards/tests/act_advancement.rs`: the round-end-only detection is
    // now registry-based (`act_advances_at_round_end` reads act 01109's
    // `When`-RoundEnded reaction, #434), which a lib unit test can't install
    // without polluting its siblings' process-global registry.

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
            .with_phase_anchor(crate::state::Continuation::InvestigationPhase {
                resume: crate::state::InvestigationResume::TurnBegins,
            })
            .with_investigator_turn(inv)
            .build();
        state.act_deck = vec![
            Act {
                code: CardCode("_test_act_1".into()),
                clue_threshold: 2,
                resolution: None,
            },
            Act {
                code: CardCode("_test_act_2".into()),
                clue_threshold: 2,
                resolution: Some(Resolution::Won { id: "demo".into() }),
            },
        ];

        let result = take_turn_action(state, &TurnAction::AdvanceAct { investigator: inv });
        assert!(!matches!(result.outcome, EngineOutcome::Rejected { .. }));
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
            .with_phase_anchor(crate::state::Continuation::InvestigationPhase {
                resume: crate::state::InvestigationResume::TurnBegins,
            })
            .with_investigator_turn(inv)
            .build();
        state.act_deck = vec![Act {
            code: CardCode("_test_act".into()),
            clue_threshold: 2,
            resolution: Some(Resolution::Won { id: "demo".into() }),
        }];

        let result = take_turn_action(state, &TurnAction::AdvanceAct { investigator: inv });
        assert!(!matches!(result.outcome, EngineOutcome::Rejected { .. }));
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
        use crate::test_support::{take_turn_action, test_investigator, GameStateBuilder};
        let inv = InvestigatorId(1);
        let mut investigator = test_investigator(1);
        investigator.clues = 2;
        let mut state = GameStateBuilder::new()
            .with_phase(Phase::Investigation)
            .with_investigator(investigator)
            .with_active_investigator(inv)
            .with_turn_order([inv])
            .with_phase_anchor(crate::state::Continuation::InvestigationPhase {
                resume: crate::state::InvestigationResume::TurnBegins,
            })
            .with_investigator_turn(inv)
            .build();
        state.act_deck = vec![
            Act {
                code: CardCode("01108".into()),
                clue_threshold: 2,
                resolution: None,
            },
            Act {
                code: CardCode("01109".into()),
                clue_threshold: 3,
                resolution: Some(Resolution::Won { id: "R1".into() }),
            },
        ];
        let result = take_turn_action(state, &TurnAction::AdvanceAct { investigator: inv });
        assert!(!matches!(result.outcome, EngineOutcome::Rejected { .. }));
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
            .with_phase_anchor(crate::state::Continuation::InvestigationPhase {
                resume: crate::state::InvestigationResume::TurnBegins,
            })
            .with_investigator_turn(acting)
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
            },
            Act {
                code: CardCode("_test_act_2".into()),
                clue_threshold: 2,
                resolution: None,
            },
        ];

        // Threshold 2: acting (1 clue) drained fully first, then 1 from `other`.
        let result = take_turn_action(
            state,
            &TurnAction::AdvanceAct {
                investigator: acting,
            },
        );
        assert!(!matches!(result.outcome, EngineOutcome::Rejected { .. }));
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
