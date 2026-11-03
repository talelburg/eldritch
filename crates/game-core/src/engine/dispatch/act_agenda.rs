//! Act and agenda handlers: doom placement, threshold checking, agenda
//! advancement, clue spending, and act advancement.

use std::borrow::Cow;

use card_dsl::dsl::{EventPattern, EventTiming, Trigger, TriggerKind};

use crate::card_registry::CardRegistry;
use crate::engine::dispatch::{clue_allocation, cursor};
use crate::engine::outcome::EngineOutcome;
use crate::engine::{board, evaluator, Cx};
use crate::scenario::ScenarioEnding;
use crate::state::{
    AdvanceDeck, AdvanceReverseFrame, AdvanceStep, AdvanceTrigger, GameState, InvestigatorId,
    LocationId, Phase,
};

/// Whether the current act advances *only* at the end of the round (its
/// round-end objective — act 01109's `When`-`RoundEnded` group advance), in
/// which case the `AdvanceAct` action is rejected. Detected from the registry:
/// the contributor location is printed in the act's own ability native.
fn act_advances_at_round_end(state: &GameState, reg: &CardRegistry) -> bool {
    let Some(act) = state.act_deck.get(state.act_index) else {
        return false;
    };
    let Some(abilities) = (reg.abilities_for)(&act.code) else {
        return false;
    };
    abilities.iter().any(|a| {
        matches!(
            &a.trigger,
            Trigger::OnEvent {
                pattern: EventPattern::RoundEnded,
                timing: EventTiming::When,
                kind: TriggerKind::Reaction,
            }
        )
    })
}

/// Place `count` doom on the current agenda, no threshold check. Mythos step
/// 1.2 (Rules Reference p.24) — "Take 1 doom from the token pool, and place it
/// on the current agenda card." — passes `1`; a card effect passes whatever it
/// prints. No-op on an empty agenda deck, so a card can't place doom on an
/// agenda that isn't there. Places doom silently. TODO(#593): no event
/// announces doom yet.
///
/// A card-local native (They're Getting Out 01107) reaches it as
/// [`NativeCx::place_doom_on_agenda`](crate::engine::NativeCx::place_doom_on_agenda),
/// so it gets the empty-agenda-deck guard rather than writing `agenda_doom`
/// itself.
pub(crate) fn place_doom_on_agenda(cx: &mut Cx, count: u8) {
    if cx.state.agenda_deck.is_empty() {
        return;
    }
    cx.state.agenda_doom = cx.state.agenda_doom.saturating_add(count);
}

/// Mythos step 1.3 (Rules Reference p.24): compare doom in play with the
/// current agenda's threshold; if met, the agenda advances. The doom compared
/// is [`board::doom_in_play`] — the agenda's plus every card's
/// (`glossary/Act_Deck_and_Agenda_Deck.md`: *"doom on the agenda card as well as
/// doom on any other cards in play"*), such as Arcane Initiate 01063's.
///
/// **Every** agenda advances the same way, terminal or not (ADR 0013): emit
/// [`Event::AgendaAdvanced`], pause on the advance-flip acknowledge, fire the
/// leaving agenda's reverse, then reset doom and move the cursor. A terminal
/// agenda's reverse is what ends the scenario — it runs
/// [`Effect::ReachResolution`](card_dsl::dsl::Effect::ReachResolution), or drains
/// the last active investigator — and `advance_reverse::finalize` holds the
/// cursor there and asserts the ending landed, so the last agenda flips like
/// any other and the player sees which branch ended their game.
pub(crate) fn check_doom_threshold(cx: &mut Cx) {
    if cx.state.agenda_deck.is_empty() {
        return;
    }
    let agenda = &cx.state.agenda_deck[cx.state.agenda_index];
    if board::doom_in_play(cx.state) < u32::from(agenda.doom_threshold) {
        return;
    }
    advance_agenda(cx);
}

/// Advance the agenda deck one step: push an
/// [`AdvanceReverse`](crate::state::Continuation::AdvanceReverse) frame and let
/// the `drive` loop run it — emit [`Event::AgendaAdvanced`], optionally pause for
/// the gated acknowledge, fire the leaving agenda's Forced reverse (which may
/// suspend), then reset doom + move the cursor at `Finalize` (RR order; a
/// terminal agenda holds its cursor there instead — `advance_reverse::finalize`).
///
/// Called for every agenda, terminal or not (ADR 0013).
pub(super) fn advance_agenda(cx: &mut Cx) {
    let from = cx.state.agenda_index;
    let leaving_code = cx.state.agenda_deck[from].code.clone();
    // Defer to the resumable AdvanceReverse sub-process: it pushes the
    // observable event, optionally pauses for the gated acknowledge, fires the
    // leaving agenda's Forced reverse (which may suspend — 01105's ChooseOne),
    // then bumps the cursor at Finalize (RR order — after the reverse resolves).
    // The drive loop owns it from here; the terminal-card guard lives in
    // `advance_reverse::finalize`.
    cx.state.continuations.push(AdvanceReverseFrame {
        deck: AdvanceDeck::Agenda,
        from,
        leaving_code,
        step: AdvanceStep::Announce,
        // Agenda advances are always game-forced (a doom threshold).
        trigger: AdvanceTrigger::Forced,
    });
}

/// The investigators who may contribute clues to an `AdvanceAct`: every Active
/// investigator, wherever they stand, in player order. Shared by
/// [`check_advance_act`]'s clue-sufficiency check and the spend
/// [`advance_act_action`] starts, so the validation domain and the spend domain
/// can never diverge.
fn clue_contributors(state: &GameState) -> Vec<InvestigatorId> {
    cursor::player_order(state)
}

/// Validate the `AdvanceAct` action
/// without mutating: Investigation phase, a modeled act deck, the current act
/// advances via the action (not a round-end objective), and the group holds at
/// least the act's clue threshold. Returns the threshold on success (so the
/// handler can spend it) or the rejection reason on failure. The action
/// enumerator calls this in "is-legal?" mode; [`advance_act_action`]
/// calls it then mutates.
pub(crate) fn check_advance_act(
    state: &GameState,
    cards: &CardRegistry,
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
    if act_advances_at_round_end(state, cards) {
        return Err(
            "this act advances only at the end of the round (its round-end \
                    objective), not via the AdvanceAct action"
                .into(),
        );
    }
    // The AdvanceAct action is the deliberate clue-spend advance, which is only
    // meaningful for an act that prints a clue value. An act with none advances
    // on a non-clue objective — e.g. The Gathering's Act 3 (01110), which
    // advances via its Forced EnemyDefeated on the Ghoul Priest. Offering the
    // action there would let the player bypass the objective (an instant win on
    // a terminal-Won act). Unlike `act_advances_at_round_end` (registry-based,
    // so it silently fails open for an act the registry doesn't know), this is
    // a self-contained invariant. Act 01109's round-end objective prints a clue
    // value and stays excluded by `act_advances_at_round_end` above.
    let Some(threshold) = state.act_deck[state.act_index].clues_to_advance(state) else {
        return Err(
            "this act advances on a non-clue objective, not via the AdvanceAct \
                    action"
                .into(),
        );
    };
    let total_clues = clues_held(state, &clue_contributors(state));
    if total_clues < u32::from(threshold) {
        return Err(format!(
            "AdvanceAct: act requires {threshold} clues, group holds {total_clues}"
        )
        .into());
    }
    Ok(threshold)
}

/// Handler for `TurnAction::AdvanceAct`:
/// validate via [`check_advance_act`], then (on success) start the group's spend
/// of the act's clue threshold ([`clue_allocation::start`]), with the acting
/// investigator as its initiator. The allocation advances the act once every
/// share is settled — terminal or not (ADR 0013); a terminal act's ending comes
/// from the reverse the advance fires, not from this handler.
pub(super) fn advance_act_action(cx: &mut Cx, investigator: InvestigatorId) -> EngineOutcome {
    let threshold = match check_advance_act(cx.state, cx.cards) {
        Ok(t) => t,
        Err(reason) => return EngineOutcome::Rejected { reason },
    };

    // All validations passed — mutate.
    let contributors = clue_contributors(cx.state);
    clue_allocation::start(cx, investigator, &contributors, threshold);
    EngineOutcome::Done
}

/// Investigators currently at `location`, in
/// [`player_order`](cursor::player_order). Used by the act round-end
/// clue-spend (01109: Hallway investigators), whose contributors are asked in
/// player order.
pub(crate) fn investigators_at(state: &GameState, location: LocationId) -> Vec<InvestigatorId> {
    cursor::player_order(state)
        .into_iter()
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

/// Whether the current act's round-end group clue-spend advance is affordable.
///
/// It is when the investigators at `contributor_location_code` hold at least
/// the current act's [`clues_to_advance`](crate::state::Act::clues_to_advance).
///
/// Shared by the offer-side eligibility condition (act 01109's
/// `01109:can-advance`) and the resolve-side
/// [`NativeCx::round_end_advance`](crate::engine::NativeCx::round_end_advance),
/// so the two can't drift. `false` when there is no current act, the act has no
/// clue objective, or the location isn't in play (the "investigators in the
/// Hallway" condition is subsumed — 0 contributors ⇒ 0 clues ⇒ not affordable).
#[must_use]
pub fn round_end_advance_affordable(state: &GameState, contributor_location_code: &str) -> bool {
    let Some(act) = state.act_deck.get(state.act_index) else {
        return false;
    };
    let Some(threshold) = act.clues_to_advance(state) else {
        return false;
    };
    let Some(loc) = evaluator::location_id_by_code(state, contributor_location_code) else {
        return false;
    };
    clues_held(state, &investigators_at(state, loc)) >= u32::from(threshold)
}

/// Round-end group clue-spend act advance — the generic mechanics behind act
/// 01109's "investigators in the Hallway may, as a group, spend the requisite
/// number of clues to advance" (the only card-specific datum is the contributor
/// location, passed in). If [`round_end_advance_affordable`], starts the spend
/// of the act's [`clues_to_advance`](crate::state::Act::clues_to_advance) among
/// the investigators there ([`clue_allocation::start`]), with `initiator` — the
/// investigator the round-end reaction was bound to — as its initiator. The
/// allocation advances the act once every share is settled.
///
/// Affordability is gated at the offer side by act 01109's `01109:can-advance`
/// eligibility condition (which calls [`round_end_advance_affordable`]), so the
/// insufficient-clues branch here is a defensive backstop. The 01109 native
/// handler reaches it as
/// [`NativeCx::round_end_advance`](crate::engine::NativeCx::round_end_advance).
///
/// # Panics
///
/// Panics if `contributor_location_code` is not a location in play. This is
/// unreachable in practice: the affordability check above returns early unless
/// the location is present, so reaching the `expect` would mean
/// [`round_end_advance_affordable`] and [`location_id_by_code`](crate::engine::location_id_by_code)
/// disagree about the same code.
pub(crate) fn round_end_advance(
    cx: &mut Cx,
    initiator: InvestigatorId,
    contributor_location_code: &str,
) -> EngineOutcome {
    if !round_end_advance_affordable(cx.state, contributor_location_code) {
        return EngineOutcome::Rejected {
            reason: "round_end_advance: contributors no longer hold enough clues".into(),
        };
    }
    let threshold = cx.state.act_deck[cx.state.act_index]
        .clues_to_advance(cx.state)
        .expect("affordable ⇒ the act has a clue objective");
    let loc = evaluator::location_id_by_code(cx.state, contributor_location_code)
        .expect("affordable ⇒ contributor location in play");
    let contributors = investigators_at(cx.state, loc);
    clue_allocation::start(cx, initiator, &contributors, threshold);
    EngineOutcome::Done
}

/// Advance the act deck one step: push an
/// [`AdvanceReverse`](crate::state::Continuation::AdvanceReverse) frame for the
/// `drive` loop to run — emit [`Event::ActAdvanced`], optionally pause for the
/// gated acknowledge, fire the leaving act's Forced reverse (which may suspend),
/// then move the cursor at `Finalize` (RR order; a terminal act holds its cursor
/// there instead — `advance_reverse::finalize`). Mirrors [`advance_agenda`], and
/// like it is called for every act, terminal or not (ADR 0013).
///
/// Invariant: the leaving act's on-advance Forced effect must not itself
/// re-advance the act (no in-scope card does; a re-advance from that
/// effect would recurse here). Revisit if such an ability lands.
///
/// `trigger` records why the act is advancing so the sub-process can decide
/// whether to prompt the on-card flip: `Deliberate` for the player-driven
/// `AdvanceAct` action and the round-end objective, `Forced` for 01110's
/// Ghoul-Priest-defeat forced advance.
pub(crate) fn advance_act(cx: &mut Cx, trigger: AdvanceTrigger) {
    let from = cx.state.act_index;
    let leaving_code = cx.state.act_deck[from].code.clone();
    // Mirror of advance_agenda: defer to the resumable AdvanceReverse
    // sub-process (observable event → gated acknowledge → leaving act's Forced
    // reverse, which may suspend → bump the cursor at Finalize, RR order). The
    // drive loop owns it; the terminal-card guard lives in
    // `advance_reverse::finalize`.
    cx.state.continuations.push(AdvanceReverseFrame {
        deck: AdvanceDeck::Act,
        from,
        leaving_code,
        step: AdvanceStep::Announce,
        trigger,
    });
}

/// Set the scenario-ending latch and arm the scenario's ending.
/// First-writer-wins: a resolution already latched this scenario is
/// authoritative and a later request is ignored.
///
/// Rules Reference: *"If a resolution point is reached, the scenario ends."* The
/// latch itself cancels nothing — clearing the continuation stack here would
/// destroy the very resolution that reached the point (the in-flight skill test
/// that defeated the Ghoul Priest, the terminal act's own reverse). Instead this
/// pushes a [`ScenarioEnd`](crate::state::Continuation::ScenarioEnd) frame at the
/// **bottom** of the stack and leaves the cancelling to the `drive` loop, which
/// gates frame by frame on
/// [`cancelled_by_scenario_end`](crate::state::Continuation::cancelled_by_scenario_end).
/// Bottom, not top: everything already under way sits above it and resolves
/// first, so the frame is exposed exactly when the ending should run — no
/// insertion index has to guess where "in-flight" stops and "framework" starts.
/// See `docs/adr/0004-a-latched-resolution-cancels-opportunities-not-resolutions.md`.
///
/// Because the frame is pushed under the same first-writer-wins guard, it exists
/// exactly once per scenario, and the apply boundary pops it exactly once — which
/// is what makes it the fire-once marker for
/// [`Event::ScenarioResolved`] + `apply_resolution`, with no second flag on
/// [`GameState`].
///
/// Call this only after a handler's validations pass: on a `Rejected`
/// outcome `apply` clears events but does not roll back `state`, so a
/// latch set on a doomed path would persist. All current callers latch
/// only on their success branches.
pub(crate) fn end_scenario(state: &mut GameState, ending: ScenarioEnding) {
    if state.ending.is_none() {
        state.ending = Some(ending);
        state.continuations.insert_ending_at_bottom();
    }
}

#[cfg(test)]
mod doom_agenda_tests {
    use super::*;
    use crate::engine::dispatch;
    use crate::event::Event;
    use crate::scenario::ResolutionId;
    use crate::state::GameStateBuilder;
    use crate::test_support::{agenda, TEST_REGISTRIES};
    use crate::{assert_event, test_support};

    #[test]
    fn place_doom_increments_agenda_doom() {
        let mut state = GameStateBuilder::new()
            .with_agenda_deck([agenda("_test_agenda", 2)])
            .build();
        let mut events = Vec::new();
        place_doom_on_agenda(&mut Cx::for_test(&mut state, &mut events), 1);
        assert_eq!(state.agenda_doom, 1);
        place_doom_on_agenda(&mut Cx::for_test(&mut state, &mut events), 1);
        assert_eq!(state.agenda_doom, 2);
    }

    #[test]
    fn placing_doom_then_checking_the_threshold_advances_the_agenda() {
        let mut state = GameStateBuilder::new()
            .with_agenda_deck([agenda("_agenda_1", 1), agenda("_agenda_2", 3)])
            .build();
        let mut events = Vec::new();
        place_doom_on_agenda(&mut Cx::for_test(&mut state, &mut events), 1);
        check_doom_threshold(&mut Cx::for_test(&mut state, &mut events));
        // The advance is deferred to an AdvanceReverse frame; drive it
        // (the test registry serves no reverse for it ⇒ it drives straight through).
        dispatch::drive(
            &mut Cx::for_test(&mut state, &mut events),
            EngineOutcome::Done,
        );
        // Doom reached threshold (1) → agenda advanced + doom reset.
        assert_eq!(state.agenda_index, 1, "agenda advanced at threshold");
        assert_eq!(state.agenda_doom, 0, "doom reset on advance");
    }

    #[test]
    fn doom_threshold_advances_non_terminal_agenda() {
        let mut state = GameStateBuilder::new()
            .with_agenda_deck([agenda("_test_agenda_1", 2), agenda("_test_agenda_2", 2)])
            .with_agenda_doom(2)
            .build();
        let mut events = Vec::new();
        check_doom_threshold(&mut Cx::for_test(&mut state, &mut events));
        // The advance is deferred to an AdvanceReverse frame; drive it.
        dispatch::drive(
            &mut Cx::for_test(&mut state, &mut events),
            EngineOutcome::Done,
        );
        assert_eq!(state.agenda_index, 1);
        assert_eq!(state.agenda_doom, 0, "doom resets on advance");
        assert!(
            state.ending.is_none(),
            "non-terminal advance does not resolve"
        );
        assert_event!(events, Event::AgendaAdvanced { from } if *from == 0);
    }

    /// A terminal agenda takes the same advance path as every other one
    /// (ADR 0013): it **emits `AgendaAdvanced`**, its reverse fires, and the
    /// reverse is what latches the ending — the cursor stays put because there
    /// is no next card, so the player sees which agenda ended their game.
    #[test]
    fn terminal_agenda_advances_and_its_reverse_latches_the_ending() {
        // The synthetic terminal card's reverse comes from the registry, and a
        // forced on-advance ability binds the lead — so both are needed.
        let inv = InvestigatorId(1);
        let mut state = GameStateBuilder::new()
            .with_registries(TEST_REGISTRIES)
            .with_seat(test_support::seat(1))
            .with_seating([inv])
            .with_agenda_deck([agenda(test_support::terminal_code(3), 2)])
            .with_agenda_doom(2)
            .build();
        let mut events = Vec::new();
        check_doom_threshold(&mut Cx::for_test(&mut state, &mut events));
        dispatch::drive(
            &mut Cx::for_test(&mut state, &mut events),
            EngineOutcome::Done,
        );
        assert_eq!(
            state.agenda_index, 0,
            "a terminal agenda does not bump the cursor — there is no next card"
        );
        assert_event!(events, Event::AgendaAdvanced { from } if *from == 0);
        // The agenda's printed (→R#) survives into the latch rather than being
        // flattened to "lost" — and it arrives from the reverse, not from a
        // field on the deck entry.
        assert_eq!(
            state.ending,
            Some(ScenarioEnding::Resolution(ResolutionId::new(3)))
        );
    }

    #[test]
    fn doom_threshold_not_met_does_nothing() {
        let mut state = GameStateBuilder::new()
            .with_agenda_deck([agenda("_test_agenda", 3)])
            .with_agenda_doom(2)
            .build();
        let mut events = Vec::new();
        check_doom_threshold(&mut Cx::for_test(&mut state, &mut events));
        assert_eq!(state.agenda_index, 0);
        assert_eq!(state.agenda_doom, 2);
        assert!(events.is_empty());
    }

    #[test]
    fn end_scenario_is_first_writer_wins() {
        let mut state = GameStateBuilder::new().build();
        // The elimination step-6 ending wins over a later resolution point.
        end_scenario(&mut state, ScenarioEnding::NoResolution);
        end_scenario(&mut state, ScenarioEnding::Resolution(ResolutionId::new(2)));
        assert_eq!(state.ending, Some(ScenarioEnding::NoResolution));
    }
}

#[cfg(test)]
mod advance_act_tests {
    use super::*;
    use crate::assert_event;
    use crate::engine::enumerate::{self, TurnAction};
    use crate::engine::outcome::{EngineOutcome, Responder};
    use crate::event::Event;
    use crate::scenario::ResolutionId;
    use crate::state::{CardCode, GameStateBuilder, InvestigatorId};
    use crate::test_support::{
        self, act, location, objective_act, TestSession, TEST_REGISTRIES, TEST_REGISTRY,
    };

    #[test]
    fn round_end_advance_affordable_tracks_hallway_clues_vs_threshold() {
        // A location coded "HALL"; the investigator stands on it.
        let hall = location(1).code(CardCode("HALL".into()));
        let mut state = GameStateBuilder::new()
            .with_registries(TEST_REGISTRIES)
            .with_location(hall)
            .with_seat(test_support::seat(1).at(LocationId(1)))
            .with_seating([InvestigatorId(1)])
            .with_act_deck([act("_act", 3)])
            .build();

        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .clues = 2;
        assert!(
            !round_end_advance_affordable(&state, "HALL"),
            "2 < 3 → not affordable"
        );
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .clues = 3;
        assert!(
            round_end_advance_affordable(&state, "HALL"),
            "3 >= 3 → affordable"
        );
    }

    #[test]
    fn round_end_advance_is_unaffordable_for_an_act_with_no_clue_objective() {
        let hall = location(1).code(CardCode("HALL".into()));
        let state = GameStateBuilder::new()
            .with_registries(TEST_REGISTRIES)
            .with_location(hall)
            .with_seat(test_support::seat(1).at(LocationId(1)).clues(5))
            .with_seating([InvestigatorId(1)])
            .with_act_deck([objective_act("_objective_act")])
            .build();
        assert!(
            !round_end_advance_affordable(&state, "HALL"),
            "no clue objective → no round-end spend, however many clues"
        );
    }

    #[test]
    fn advance_act_rejects_when_clues_insufficient() {
        let inv = InvestigatorId(1);
        let state = GameStateBuilder::new()
            .with_registries(TEST_REGISTRIES)
            .with_seat(test_support::seat(1).clues(1))
            .open_turn(inv)
            .with_act_deck([act("_test_act", 2)])
            .build();

        // Insufficient clues (1 < 2): AdvanceAct is not legal.
        assert!(
            !enumerate::legal_actions(&state, &TEST_REGISTRY)
                .iter()
                .any(|a| matches!(a, TurnAction::AdvanceAct { .. })),
            "AdvanceAct must not be legal when clues < threshold"
        );
    }

    #[test]
    fn advance_act_action_rejected_for_an_act_with_no_clue_objective() {
        // A non-clue-objective act (no printed clue value — e.g. The Gathering's
        // Act 3 01110, which advances when the Ghoul Priest is defeated). The
        // deliberate clue-spend AdvanceAct action is nonsensical here (it would
        // skip the objective, or instantly win), so it must be
        // neither offered nor accepted — even with no forced ability served for
        // it (the game-core test registry knows nothing of the objective act).
        let inv = InvestigatorId(1);
        let state = GameStateBuilder::new()
            .with_registries(TEST_REGISTRIES)
            // Plenty of clues — the reject must be the objective, not affordability.
            .with_seat(test_support::seat(1).clues(5))
            .open_turn(inv)
            .with_act_deck([objective_act("_objective_act")])
            .build();

        assert!(
            !enumerate::legal_actions(&state, &TEST_REGISTRY)
                .iter()
                .any(|a| matches!(a, TurnAction::AdvanceAct { .. })),
            "AdvanceAct must not be offered for an act with no clue objective"
        );
        // Bypass the legality menu (the assert above already proves it's not
        // offered) to confirm the handler itself rejects, leaving state unchanged.
        let session = TestSession::new(state, TEST_REGISTRIES)
            .submit_off_menu(&TurnAction::AdvanceAct { investigator: inv });
        session.expect_rejected();
        assert!(
            session.state().ending.is_none(),
            "rejected AdvanceAct must not latch the act's Won resolution"
        );
        assert_eq!(
            session.state().investigators[&inv].clues,
            5,
            "rejected AdvanceAct must not spend clues"
        );
    }

    // The round-end-only rejection is tested in
    // `crates/cards/tests/integration/act_advancement.rs`: `act_advances_at_round_end`
    // reads act 01109's `When`-RoundEnded reaction from the registry, and the
    // real act is corpus content a game-core unit test can't reach.

    #[test]
    fn advance_act_spends_clues_and_advances_non_terminal() {
        let inv = InvestigatorId(1);
        let state = GameStateBuilder::new()
            .with_registries(TEST_REGISTRIES)
            .with_seat(test_support::seat(1).clues(3))
            .open_turn(inv)
            .with_act_deck([act("_test_act_1", 2), act("_test_act_2", 2)])
            .build();

        let session = TestSession::new(state, TEST_REGISTRIES)
            .take(&TurnAction::AdvanceAct { investigator: inv });
        session.assert_at_turn_menu(inv);
        assert_eq!(session.state().act_index, 1);
        assert_eq!(
            session.state().investigators[&inv].clues,
            1,
            "spent exactly 2 of 3"
        );
        assert!(session.state().ending.is_none());
        assert_event!(session.events(), Event::ActAdvanced { from } if *from == 0);
    }

    /// The act-side mirror of
    /// [`terminal_agenda_advances_and_its_reverse_latches_the_ending`]: a
    /// terminal act **emits `ActAdvanced`** and its reverse latches the ending,
    /// rather than the handler latching a field and skipping the flip (ADR
    /// 0013). The clues are still spent, as before.
    #[test]
    fn terminal_act_advances_and_its_reverse_latches_the_ending() {
        let inv = InvestigatorId(1);
        let state = GameStateBuilder::new()
            .with_registries(TEST_REGISTRIES)
            .with_seat(test_support::seat(1).clues(2))
            .open_turn(inv)
            .with_act_deck([act(test_support::terminal_code(1), 2)])
            .build();

        // The ending is latched, so the game is over: `Done`.
        let result = TestSession::new(state, TEST_REGISTRIES)
            .take(&TurnAction::AdvanceAct { investigator: inv })
            .finish();
        assert_eq!(result.outcome, EngineOutcome::Done);
        assert_eq!(
            result.state.act_index, 0,
            "a terminal act does not bump the cursor — there is no next card"
        );
        assert_event!(result.events, Event::ActAdvanced { from } if *from == 0);
        assert_eq!(
            result.state.ending,
            Some(ScenarioEnding::Resolution(ResolutionId::new(1)))
        );
        assert_eq!(result.state.investigators[&inv].clues, 0);
    }

    #[test]
    fn advance_act_with_no_forced_ability_still_advances() {
        let inv = InvestigatorId(1);
        let state = GameStateBuilder::new()
            .with_registries(TEST_REGISTRIES)
            .with_seat(test_support::seat(1).clues(2))
            .open_turn(inv)
            .with_act_deck([act("_act_1", 2), act("_act_2", 3)])
            .build();
        let session = TestSession::new(state, TEST_REGISTRIES)
            .take(&TurnAction::AdvanceAct { investigator: inv });
        session.assert_at_turn_menu(inv);
        assert_eq!(
            session.state().act_index,
            1,
            "cursor advances even with no forced ability"
        );
    }

    /// A surplus of 1 (acting 1, other 2, threshold 2): the other investigator
    /// is asked first and offered 1 or 2; the acting investigator covers the
    /// rest unasked.
    #[test]
    fn advance_act_asks_the_other_investigator_before_the_initiator() {
        let acting = InvestigatorId(1);
        let other = InvestigatorId(2);
        let state = GameStateBuilder::new()
            .with_registries(TEST_REGISTRIES)
            .with_seat(test_support::seat(1).clues(1))
            .with_seat(test_support::seat(2).clues(2))
            .with_seating([acting, other])
            .open_turn(acting)
            // Two acts so the first is non-terminal and its advance bumps the
            // cursor to 1 (a lone act would be terminal, and would need a
            // reverse that reaches an ending).
            .with_act_deck([act("_test_act_1", 2), act("_test_act_2", 2)])
            .build();

        let session = TestSession::new(state, TEST_REGISTRIES).take(&TurnAction::AdvanceAct {
            investigator: acting,
        });
        let prompt = session.prompt();
        assert_eq!(prompt.responder, Responder::Investigator(other));
        let labels: Vec<&str> = prompt.options.iter().map(|o| o.label.as_str()).collect();
        assert_eq!(labels, ["Contribute 1 clue", "Contribute 2 clues"]);

        // Picks 1: the acting investigator's 1 covers the rest.
        let session = session.pick_nth(0);
        session.assert_at_turn_menu(acting);
        let state = session.state();
        assert_eq!(state.investigators[&acting].clues, 0);
        assert_eq!(state.investigators[&other].clues, 1);
        assert_eq!(state.act_index, 1);
    }
}
