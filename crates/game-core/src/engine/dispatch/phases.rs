//! Phase-driver functions: start/end scenario, per-phase entrypoints,
//! and the round-cycle stepping logic.

use std::collections::BTreeSet;

use card_dsl::card_data::CardKind;

use crate::action::{InputResponse, RosterEntry};
use crate::card_registry;
#[cfg(test)] // only `drive_phase` / `push_anchor_and_drive`, the cfg(test) helpers below
use crate::engine::dispatch;
use crate::engine::dispatch::emit::TimingEvent;
use crate::engine::dispatch::{
    act_agenda, cards, combat, cursor, emit, encounter, hunters, reaction_windows, reveal,
};
use crate::engine::outcome::{EngineOutcome, InputRequest, ResumeToken};
use crate::engine::Cx;
use crate::event::Event;
use crate::state::{
    CardCode, CardInPlay, Continuation, EnemyId, EnemyResume, FastWindowKind, GameState,
    HandSizeDiscard, InvestigationResume, Investigator, InvestigatorId, MythosResume, Phase,
    PhaseStep, Skills, Status, UpkeepResume, Zone,
};

/// Action points granted to an investigator at the start of their
/// turn during the Investigation phase. Per the Arkham Horror LCG
/// rulebook.
pub(super) const ACTIONS_PER_TURN: u8 = 3;

/// Internal scenario setup: seat the roster, shuffle decks, push the initial
/// phase anchors. Called by [`super::seat_and_open`] (the non-logged engine
/// entry point); never reached via a `PlayerAction` — the action log is
/// `ResolveInput`-only after #447/#459.
pub(super) fn start_scenario(cx: &mut Cx, roster: &[RosterEntry]) -> EngineOutcome {
    // Replaying on an already-started state is a bug, not a no-op — reject so
    // callers notice rather than silently double-emitting `ScenarioStarted`.
    if cx.state.round != 0 {
        return EngineOutcome::Rejected {
            reason: "start_scenario called on a state that is already in progress".into(),
        };
    }

    // Validate-first: resolve every roster entry's stats from card data
    // before mutating anything. Any failure rejects with state unchanged.
    // Capacity (health/sanity) is no longer copied into Investigator fields
    // (#448 cp4) — the accessors read from the registry directly via
    // `investigator_card.code`. We still validate that the code resolves to a
    // `CardKind::Investigator` here so seating rejects non-investigators.
    let registry = card_registry::current();
    let mut resolved: Vec<(Skills, String, Vec<CardCode>, CardCode)> =
        Vec::with_capacity(roster.len());
    for entry in roster {
        let Some(reg) = registry else {
            return EngineOutcome::Rejected {
                reason: "no card registry installed; cannot resolve investigator stats".into(),
            };
        };
        let Some(meta) = (reg.metadata_for)(&entry.investigator) else {
            return EngineOutcome::Rejected {
                reason: format!("unknown investigator code {}", entry.investigator).into(),
            };
        };
        let CardKind::Investigator { skills, .. } = meta.kind else {
            return EngineOutcome::Rejected {
                reason: format!("card {} is not a seatable investigator", entry.investigator)
                    .into(),
            };
        };
        resolved.push((
            skills,
            meta.name.clone(),
            entry.deck.clone(),
            entry.investigator.clone(),
        ));
    }

    // A scenario requires at least one investigator. Seating is the sole
    // seater (#224): the roster is mandatory, an empty roster rejects.
    if resolved.is_empty() {
        return EngineOutcome::Rejected {
            reason: "a scenario requires a non-empty roster".into(),
        };
    }

    // --- mutate (all validations passed) ---
    // Seat resolved investigators. Ids are sequential (1-based) in roster
    // order. Seated investigators start at the scenario's starting location
    // (set by setup()). None leaves them unplaced.
    let start = cx.state.starting_location;

    for (idx, (skills, name, deck, card_code)) in resolved.into_iter().enumerate() {
        let id = InvestigatorId(u32::try_from(idx).unwrap_or(0) + 1);
        let inv_card_id = cx.state.card_instance_ids.mint();
        let investigator_card = CardInPlay::enter_play(card_code, inv_card_id);
        cx.state.investigators.insert(
            id,
            Investigator {
                id,
                name,
                current_location: start,
                skills,
                clues: 0,
                resources: 5,
                actions_remaining: 0,
                status: Status::Active,
                deck,
                hand: Vec::new(),
                discard: Vec::new(),
                setaside: Vec::new(),
                cards_in_play: Vec::new(),
                threat_area: Vec::new(),
                removed_from_game: Vec::new(),
                action_surcharge_spent_this_round: BTreeSet::new(),
                investigator_card,
            },
        );
        cx.state.turn_order.push(id);
    }
    // Reveal the starting location on first entry (Rules Reference p.14).
    // investigators.len() is now final (all roster entries seated), so
    // per-investigator clue counts are correct. No-op when start is None
    // (pre-seated test path) or already revealed.
    if let Some(loc) = start {
        reveal::reveal_location(cx, loc);
    }

    // Round 1: scenario starts directly in Investigation phase —
    // Mythos is skipped entirely per Rules Reference p.24 "During
    // the first round of the game, skip the mythos phase." No
    // PhaseStarted(Mythos) / PhaseEnded(Mythos) fire — the phase
    // doesn't happen.
    cx.state.round = 1;
    cx.state.phase = Phase::Investigation;
    cx.events.push(Event::ScenarioStarted);

    // For each investigator (sorted by id for determinism), shuffle
    // their deck, deal an initial hand of up to 5, then set aside any
    // weaknesses per Rules Reference setup step 8.
    let inv_ids: Vec<InvestigatorId> = cx.state.investigators.keys().copied().collect();
    for inv_id in inv_ids {
        cards::shuffle_player_deck(cx, inv_id);
        cards::draw_cards(cx, inv_id, cards::INITIAL_HAND_SIZE);
        cards::replace_opening_hand_weaknesses(cx, inv_id);
    }

    // Shuffle the shared encounter deck with the same scenario-start RNG
    // (Rules Reference p.21: the encounter deck is shuffled during setup).
    // `setup()` seeds it in deterministic construction order; this is the
    // single randomizing step. A <2-card deck (the synthetic test fixture)
    // shuffles to a no-op (no event).
    encounter::shuffle_encounter_deck(cx);

    // Round-1 action seed: round 1 skips Mythos, so there's no Upkeep 4.2
    // to grant the first round's actions. Every Active investigator → ACTIONS_PER_TURN.
    reset_actions(cx);

    // Begin the setup mulligan loop. Each Active investigator submits a single
    // mulligan (a `ResolveInput(PickMultiple)`) in player order; the loop
    // advances after each and drains once all have gone, at which point setup
    // ends and the Investigation phase begins (see `resume_mulligan`). While
    // the `Mulligan` frame is on the stack, every non-`ResolveInput` action is
    // rejected. An empty/all-eliminated `turn_order` skips the loop entirely:
    // setup ends immediately and we begin Investigation here.
    let remaining = cursor::active_investigators_in_turn_order(cx.state);
    if remaining.is_empty() {
        return investigation_phase(cx);
    }
    cards::prompt_mulligan(cx, remaining)
}

pub(super) fn end_turn(cx: &mut Cx) -> EngineOutcome {
    if cx.state.phase != Phase::Investigation {
        return EngineOutcome::Rejected {
            reason: "EndTurn is only valid during the Investigation phase".into(),
        };
    }
    let Some(active_id) = cx.state.active_investigator else {
        return EngineOutcome::Rejected {
            reason: "EndTurn requires an active investigator".into(),
        };
    };
    // The Some(active_investigator) invariant is paired with that ID
    // existing in the investigators map; a missing entry would be state
    // corruption, not a normal rejection. Surface it loudly rather than
    // hiding behind Rejected.
    let active = cx
        .state
        .investigators
        .get_mut(&active_id)
        .unwrap_or_else(|| {
            unreachable!(
                "active_investigator {active_id:?} is not in the investigators map; \
                 this is a state-corruption invariant violation"
            )
        });

    // Drain remaining actions and announce the turn ended.
    if active.actions_remaining != 0 {
        active.actions_remaining = 0;
        cx.events.push(Event::ActionsRemainingChanged {
            investigator: active_id,
            new_count: 0,
        });
    }
    cx.events.push(Event::TurnEnded {
        investigator: active_id,
    });

    // Forced "at the end of your turn" abilities (threat-area cards such as
    // Frozen in Fear 01164's willpower test) fire for the investigator whose
    // turn just ended, before the turn passes on (first consumer: C4c, #235).
    //
    // A forced effect that initiates a skill test, or a 2+ simultaneous forced
    // run (#213, two Frozen in Fear copies), suspends here (`AwaitingInput`),
    // stranding `end_turn` before rotation. Both cases are handled uniformly
    // (#434): flag the `InvestigatorTurn { ending: true }` frame (beneath the
    // suspension); the `drive` loop re-dispatches it once the suspension
    // resolves and runs [`resume_end_turn`]. The 2+ forced run closes to `Done`
    // (its `Terminal` continuation), so it no longer carries a bespoke
    // `EndOfTurnAfterForced` — the turn frame is the single resume path.
    //
    // A `Rejected` propagates as-is.
    // Frame-driven rotation (Slice D, #423): arm the `InvestigatorTurn` frame's
    // `ending` flag BEFORE emitting `EndOfTurn`, then emit. `queue_event` pushes
    // the forced/reaction abilities (Frozen in Fear 01164's willpower test, a 2+
    // forced run) as frames; the `drive` loop drives them and, once they pop,
    // re-dispatches the `InvestigatorTurn { ending: true }` frame → `resume_end_turn`
    // for the rotation (RR p.24 step 2.2.2). Uniform whether the forced run is
    // empty, completes immediately, or suspends — there is no inline-resume branch.
    // A `Rejected` from `queue_event` rolls back the armed flag with the rest of
    // the apply (transactional snapshot).
    let ending = cursor::turn_frame_ending_mut(cx.state, active_id).unwrap_or_else(|| {
        unreachable!("end_turn: no InvestigatorTurn({active_id:?}) on the stack")
    });
    *ending = true;
    emit::queue_event(
        cx,
        &TimingEvent::EndOfTurn {
            investigator: active_id,
        },
    )
}

/// Run the post-`EndOfTurn`-forced portion of [`end_turn`] (Rules
/// Reference p.24 step 2.2.2): rotate to the next active investigator, or
/// end the Investigation phase. Reached uniformly through the `drive` loop's
/// `InvestigatorTurn { ending: true }` arm (Slice D, #423): `end_turn` arms
/// that flag and emits `EndOfTurn`, whose forced/reaction effects (Frozen in
/// Fear 01164's willpower test) the loop drives as frames; once they pop, the
/// re-exposed turn frame runs this rotation — whether the forced run was empty,
/// completed immediately, or suspended.
pub(super) fn resume_end_turn(cx: &mut Cx, active_id: InvestigatorId) -> EngineOutcome {
    // The turn is over: pop the InvestigatorTurn frame this turn ran on (slice
    // 2a-i, #393). It is always on top here — end_turn reaches this after the
    // EndOfTurn forced run resolves, the stranded-skill-test resume after the
    // SkillTest pops, and the forced-run continuation after its Resolution pops.
    debug_assert!(
        matches!(
            cx.state.continuations.last(),
            Some(Continuation::InvestigatorTurn { investigator, .. })
                if *investigator == active_id
        ),
        "resume_end_turn: expected InvestigatorTurn({active_id:?}) on top, got {:?}",
        cx.state.continuations.last(),
    );
    cx.state.continuations.pop();

    // 2.2.2 decision: "return to 2.2" for the next investigator, or
    // proceed to 2.3. next_active_investigator_after skips eliminated
    // investigators (Rules Reference p.10) — the same shared helper the
    // Enemy phase uses.
    if let Some(next_id) = cursor::next_active_investigator_after(cx.state, active_id) {
        begin_investigator_turn(cx, next_id);
        EngineOutcome::Done
    } else {
        cx.state.active_investigator = None;
        // 2.3 → Enemy. The cascade may suspend on a hunter-movement tie
        // (Enemy 3.2); propagate its outcome rather than swallowing it.
        investigation_phase_end(cx)
    }
}

/// Entered by [`step_phase`] on any-to-Investigation transition, and by
/// the mulligan-completion site in [`apply_player_action`] for round 1.
/// Owns the `PhaseStarted(Investigation)` emit (Rules Reference p.24 step 2.1)
/// and queues its forced abilities; [`investigation_after_phase_start`] opens
/// the post-2.1 player window once they have resolved. Rotation to the
/// first active investigator (step 2.2) runs in the
/// [`PhaseStep::InvestigationBegins`] continuation via
/// [`begin_investigator_turn`], lead-first by default; explicit
/// player-pick within this window is deferred to #146.
pub(super) fn investigation_phase(cx: &mut Cx) -> EngineOutcome {
    // 2.1 Investigation phase begins.
    cx.events.push(Event::PhaseStarted {
        phase: Phase::Investigation,
    });
    // Push the Investigation phase anchor (slice 1a, #393). It persists for the
    // whole phase (across every investigator's turn), beneath the framework
    // windows; popped at investigation_phase_end_transition.
    //
    // **Emits in tail position** (ADR 0003). The anchor is parked at
    // `AfterPhaseStartForced` *before* the emit, because the emit queues frames
    // rather than resolving them: opening the post-2.1 window here would push it
    // above the step-2.1 forced abilities the emit had just queued, and the
    // rotation to the first investigator would follow them rather than the
    // milestone.
    cx.state
        .continuations
        .push(Continuation::InvestigationPhase {
            resume: InvestigationResume::AfterPhaseStartForced,
        });
    emit::queue_event(
        cx,
        &TimingEvent::PhaseStarted {
            phase: Phase::Investigation,
        },
    )
}

/// Reached via the Investigation anchor's
/// [`AfterPhaseStartForced`](crate::state::InvestigationResume::AfterPhaseStartForced)
/// resume once step 2.1's queued forced abilities have resolved: open the
/// post-2.1 player window.
///
/// PLAYER WINDOW (post-2.1). Rotation to the first investigator (step 2.2) runs
/// in this window's continuation (`anchor_on_child_pop` →
/// [`Begins`](crate::state::InvestigationResume::Begins)), so the printed order
/// 2.1 → window → 2.2 holds. Auto-skips inline when nothing is Fast-eligible, so
/// single-investigator entry still lands the lead active within the same
/// `apply()` call.
fn investigation_after_phase_start(cx: &mut Cx) -> EngineOutcome {
    set_investigation_resume(cx, InvestigationResume::Begins);
    let outcome = reaction_windows::open_fast_window(
        cx,
        FastWindowKind::Phase(PhaseStep::InvestigationBegins),
    );
    debug_assert_eq!(
        outcome,
        EngineOutcome::Done,
        "open_fast_window(InvestigationBegins) unexpectedly suspended; this window has no suspending continuation",
    );
    outcome
}

/// Set the [`InvestigationPhase`](crate::state::Continuation::InvestigationPhase)
/// anchor's resume cursor. Reverse-searches the stack, mirroring
/// [`set_enemy_anchor`] / [`set_upkeep_resume`], so it is robust whether the
/// anchor is on top or buried beneath a frame the phase's own work pushed.
fn set_investigation_resume(cx: &mut Cx, resume: InvestigationResume) {
    if let Some(c) = cx
        .state
        .continuations
        .iter_mut()
        .rev()
        .find(|c| matches!(c, Continuation::InvestigationPhase { .. }))
    {
        *c = Continuation::InvestigationPhase { resume };
    } else {
        unreachable!("set_investigation_resume: no InvestigationPhase anchor on the stack");
    }
}

/// 2.2 Next investigator's turn begins. Rotates the active cursor to
/// `who` (the chosen/default investigator) and opens the post-2.2
/// player window. Called from the `InvestigationBegins` continuation
/// (first turn of the phase) and from `end_turn` (each subsequent turn,
/// the rules' "return to 2.2"). Step
/// 2.2.1 (the active investigator's actions) follows as player-driven
/// inputs while `InvestigatorTurnBegins` is the "previous player window."
///
/// `who` must be an `Active` investigator in `turn_order`; callers
/// resolve it via `first_active_investigator` / `next_active_investigator_after`.
pub(super) fn begin_investigator_turn(cx: &mut Cx, who: InvestigatorId) {
    rotate_to_active(cx, who);
    // Advance the Investigation anchor to `TurnBegins` so the closing
    // InvestigatorTurnBegins window routes to the right on_child_pop arm
    // (slice 1a, #393).
    set_investigation_resume(cx, InvestigationResume::TurnBegins);
    let outcome = reaction_windows::open_fast_window(
        cx,
        FastWindowKind::Phase(PhaseStep::InvestigatorTurnBegins),
    );
    debug_assert_eq!(
        outcome,
        EngineOutcome::Done,
        "open_fast_window(InvestigatorTurnBegins) unexpectedly suspended; this window has no suspending continuation",
    );
}

/// 2.3 Investigation phase ends. Owns the `PhaseEnded(Investigation)` emit and
/// queues its forced abilities; the Investigation → Enemy transition runs from
/// [`investigation_phase_end_transition`] once they have resolved. Called only
/// from `end_turn`'s terminal branch (the last investigator has taken a turn
/// this round).
///
/// **Emits in tail position** (ADR 0003), the shape `enemy_phase_end` and
/// `upkeep_phase_end` already had: the anchor is re-parked at
/// [`AfterPhaseEndForced`](crate::state::InvestigationResume::AfterPhaseEndForced)
/// *beneath* whatever the emit queues rather than popped, because popping it and
/// pushing the Enemy anchor here would bury those frames at the bottom of the
/// stack — phase anchors pop-and-push rather than drain (#569).
fn investigation_phase_end(cx: &mut Cx) -> EngineOutcome {
    // The open-action turn has finished, so the anchor — the bottom-most
    // Investigation frame — is the top one. It stays: the transition needs it as
    // its resume point.
    debug_assert!(
        matches!(
            cx.state.continuations.last(),
            Some(Continuation::InvestigationPhase { .. })
        ),
        "investigation_phase_end: expected InvestigationPhase anchor on top, got {:?}",
        cx.state.continuations.last(),
    );
    cx.events.push(Event::PhaseEnded {
        phase: Phase::Investigation,
    });
    // Arm the resume BEFORE emitting: the emit may push an ordering run that
    // suspends across an `apply()` boundary, and the anchor beneath it must
    // already know where to continue.
    set_investigation_resume(cx, InvestigationResume::AfterPhaseEndForced);
    emit::queue_event(
        cx,
        &TimingEvent::PhaseEnded {
            phase: Phase::Investigation,
        },
    )
}

/// Investigation → Enemy (slice 1b, #393), reached via the Investigation
/// anchor's
/// [`AfterPhaseEndForced`](crate::state::InvestigationResume::AfterPhaseEndForced)
/// resume once step 2.3's queued forced abilities have resolved: pop the anchor,
/// advance `state.phase`, and push the Enemy anchor at `Entry`. The main loop's
/// `drive` advances that (runs `enemy_phase`); a hunter-movement-tie suspension
/// surfaces through `drive`.
fn investigation_phase_end_transition(cx: &mut Cx) -> EngineOutcome {
    debug_assert!(
        matches!(
            cx.state.continuations.last(),
            Some(Continuation::InvestigationPhase { .. })
        ),
        "investigation_phase_end_transition: expected InvestigationPhase anchor on top, got {:?}",
        cx.state.continuations.last(),
    );
    cx.state.continuations.pop();
    cx.state.phase = Phase::Enemy;
    cx.state.continuations.push(Continuation::EnemyPhase {
        resume: EnemyResume::Entry,
        attacking: None,
    });
    EngineOutcome::Done
}

/// Test helper: run a phase driver and let the drive loop carry it to its next
/// idle point, the way the `apply` boundary does in production.
///
/// Since #697 each phase opener emits its step-N.1 `PhaseStarted` milestone in
/// **tail position** (ADR 0003), so the phase's own opening work runs from the
/// anchor's resume rather than inline. A unit test that calls the opener
/// directly and inspects the board therefore has to drive, exactly as the loop
/// does; without it, it would only ever see the parked anchor.
#[cfg(test)]
fn drive_phase(cx: &mut Cx, outcome: EngineOutcome) -> EngineOutcome {
    dispatch::drive(cx, outcome)
}

/// Entered by [`step_phase`] on the Upkeep→Mythos transition. Lays
/// out the Rules Reference p.24 sub-steps as discrete named call
/// sites so the rule structure is grep-able and #73 / #139
/// fills in TODO bodies without changing the driver shape.
fn mythos_phase(cx: &mut Cx) -> EngineOutcome {
    // 1.1 Round begins. Mythos phase begins.
    //     Rules Reference p.24: "As this is the first framework event
    //     of the round, it [1.1] also formalizes the beginning of a new
    //     game round." The round-counter increment lives HERE (not in
    //     step_phase) so the rule's round-begin point has explicit
    //     driver ownership, mirroring PhaseStarted(Mythos). Round 1 is
    //     bypassed: start_scenario sets round = 1 directly (Mythos
    //     skipped). This is also the future home for a RoundStarted
    //     event when a consumer lands.
    cx.state.round = cx.state.round.saturating_add(1);
    // New round: clear each investigator's per-round "first-applicable
    // action surcharge already spent" set (Frozen in Fear 01164).
    for inv in cx.state.investigators.values_mut() {
        inv.action_surcharge_spent_this_round.clear();
    }
    cx.events.push(Event::PhaseStarted {
        phase: Phase::Mythos,
    });
    // Push the Mythos phase anchor.
    //
    // **Emits in tail position** (ADR 0003): parked at `AfterPhaseStartForced`
    // *before* the emit, so steps 1.2/1.3 cannot run above the step-1.1 forced
    // abilities the emit queues. `mythos_after_phase_start` re-parks it at
    // `Draws` and runs them on re-exposure.
    cx.state.continuations.push(Continuation::MythosPhase {
        resume: MythosResume::AfterPhaseStartForced,
    });
    emit::queue_event(
        cx,
        &TimingEvent::PhaseStarted {
            phase: Phase::Mythos,
        },
    )
}

/// Mythos steps 1.2 + 1.3, reached via the Mythos anchor's
/// [`AfterPhaseStartForced`](crate::state::MythosResume::AfterPhaseStartForced)
/// resume once step 1.1's queued forced abilities have resolved.
///
/// Re-parks the anchor at [`Draws`](crate::state::MythosResume::Draws) first
/// (#482): 1.2/1.3 may push an `AdvanceReverse` frame whose reverse suspends
/// (01105's `ChooseOne`), and the 1.4 draws run from that resume once the frame
/// pops, never before — RR order has the agenda's on-advance effect resolve
/// before the encounter draws.
fn mythos_after_phase_start(cx: &mut Cx) -> EngineOutcome {
    set_mythos_resume(cx, MythosResume::Draws);

    // 1.2 Place 1 doom on the current agenda.
    act_agenda::place_doom_on_agenda(cx, 1);

    // 1.3 Check doom threshold (may push an AdvanceReverse frame above the anchor).
    act_agenda::check_doom_threshold(cx);

    // 1.4 runs from the anchor's `Draws` resume, after any advance sub-process
    // resolves. Cede to the loop.
    EngineOutcome::Done
}

/// Set the [`MythosPhase`](crate::state::Continuation::MythosPhase) anchor's
/// resume cursor. Reverse-searches the stack, mirroring [`set_upkeep_resume`].
fn set_mythos_resume(cx: &mut Cx, resume: MythosResume) {
    if let Some(c) = cx
        .state
        .continuations
        .iter_mut()
        .rev()
        .find(|c| matches!(c, Continuation::MythosPhase { .. }))
    {
        *c = Continuation::MythosPhase { resume };
    } else {
        unreachable!("set_mythos_resume: no MythosPhase anchor on the stack");
    }
}

/// Test helper (slice 1b, #393): advance to the next phase via the main loop,
/// the way a real transition does — set `state.phase` to the next phase, push
/// its anchor at `Entry`, and `drive`. Production no longer has a synchronous
/// phase-stepping function: the four `*_phase_end`/teardown transitions push the
/// next `{Entry}` anchor and the apply boundary's `drive` advances it. Tests
/// that constructed a state in phase *N* and want phase *N+1* run through the
/// same mechanism here.
#[cfg(test)]
fn step_phase(cx: &mut Cx) -> EngineOutcome {
    let to = cx.state.phase.next();
    cx.state.phase = to;
    let anchor = match to {
        Phase::Mythos => Continuation::MythosPhase {
            resume: MythosResume::Entry,
        },
        Phase::Investigation => Continuation::InvestigationPhase {
            resume: InvestigationResume::Entry,
        },
        Phase::Enemy => Continuation::EnemyPhase {
            resume: EnemyResume::Entry,
            attacking: None,
        },
        Phase::Upkeep => Continuation::UpkeepPhase {
            resume: UpkeepResume::Entry,
        },
    };
    cx.state.continuations.push(anchor);
    dispatch::drive(cx, EngineOutcome::Done)
}

/// Set `active_investigator` to `id`. Does NOT refresh actions —
/// actions are reset at Upkeep step 4.2 (`reset_actions`) for the whole
/// next round, and seeded for round 1 by `start_scenario`. By the time
/// an investigator becomes active, `actions_remaining` already holds
/// this round's allotment.
///
/// `id` must refer to an investigator in `state.investigators` (a
/// whole-program invariant for ids drawn from `turn_order`).
fn rotate_to_active(cx: &mut Cx, id: InvestigatorId) {
    debug_assert!(
        cx.state.investigators.contains_key(&id),
        "rotate_to_active: investigator {id:?} not in investigators (state corruption)"
    );
    cx.state.active_investigator = Some(id);
}

/// 3.3 Seed the per-investigator attack cursor and open the first
/// attack window — or the final window directly if there is no Active
/// investigator. Called once hunter movement (step 3.2) completes:
/// from [`enemy_phase`] on the no-tie path, and from
/// [`resume_hunter_choice`] once all hunters resolve.
///
/// Seeds the cursor to the first Active investigator in `turn_order`.
/// Eliminated investigators (Defeated / Resigned) are skipped per
/// Rules Reference p.10 (Elimination); [`cursor::first_active_investigator`] is
/// the shared helper used by Mythos 1.4 (#69) for the same semantics.
/// The loop body runs in [`anchor_on_child_pop`]'s arms.
///
/// Returns the opened window's [`EngineOutcome`]. The no-active-investigator
/// path opens `AfterAllInvestigatorsAttacked`, whose continuation cascades
/// Enemy → Upkeep; that cascade can now suspend at Upkeep step 4.5
/// (hand-size discard, #111), so the outcome propagates rather than being
/// discarded.
pub(super) fn enemy_attack_kickoff(cx: &mut Cx) -> EngineOutcome {
    // No Active investigators (turn_order empty or all eliminated) → `None`
    // opens the final window directly, mirroring mythos_phase's no-drawer path.
    open_attack_window(cx, cursor::first_active_investigator(cx.state))
}

/// Point the Enemy phase anchor at the next step-3.3 window and open it. The
/// `attacking` cursor is the single source of truth (#411): `Some(inv)` opens
/// that investigator's `BeforeInvestigatorAttacked` window and the anchor
/// resumes into resolving their attacks; `None` means no investigator remains,
/// so open the terminal `AfterAllInvestigatorsAttacked` window and the anchor
/// resumes into `enemy_phase_end`. Deriving the resume, the window, and the
/// cursor from one `Option` here makes a mismatched pairing unrepresentable.
///
/// Shared by [`enemy_attack_kickoff`] (step 3.3 entry, cursor =
/// [`cursor::first_active_investigator`](super::cursor::first_active_investigator))
/// and [`after_enemy_phase_attacks`](super::reaction_windows::after_enemy_phase_attacks)
/// (per-investigator advance, cursor =
/// [`cursor::next_active_investigator_after`](super::cursor::next_active_investigator_after)).
pub(super) fn open_attack_window(cx: &mut Cx, attacking: Option<InvestigatorId>) -> EngineOutcome {
    let (resume, step) = match attacking {
        Some(_) => (
            EnemyResume::BeforeInvestigatorAttacked,
            PhaseStep::BeforeInvestigatorAttacked,
        ),
        None => (
            EnemyResume::AfterAllAttacked,
            PhaseStep::AfterAllInvestigatorsAttacked,
        ),
    };
    set_enemy_anchor(cx, resume, attacking);
    reaction_windows::open_fast_window(cx, FastWindowKind::Phase(step))
}

/// Set the Enemy phase anchor's `resume` and `attacking` cursor together (slice
/// 1a / #411) so neither is dropped. The low-level primitive behind
/// [`open_attack_window`]; the anchor is the bottom-most Enemy frame, and this
/// is a no-op if it is absent (only in tests that drive the attack loop in
/// isolation).
pub(super) fn set_enemy_anchor(
    cx: &mut Cx,
    resume: EnemyResume,
    attacking: Option<InvestigatorId>,
) {
    if let Some(c) = cx
        .state
        .continuations
        .iter_mut()
        .rev()
        .find(|c| matches!(c, Continuation::EnemyPhase { .. }))
    {
        *c = Continuation::EnemyPhase { resume, attacking };
    }
}

/// Entered by [`step_phase`] on the Investigation→Enemy transition. Owns the
/// `PhaseStarted(Enemy)` emit (Rules Reference p.25 step 3.1) and queues its
/// forced abilities; hunter movement (3.2) and the attack-loop kickoff (3.3)
/// run from [`enemy_after_phase_start`] once they have resolved.
fn enemy_phase(cx: &mut Cx) -> EngineOutcome {
    // 3.1 Enemy phase begins.
    cx.events.push(Event::PhaseStarted {
        phase: Phase::Enemy,
    });
    // Push the Enemy phase anchor (slice 1a, #393) before hunter movement, so a
    // lead-tie suspension parks above it and the kickoff on resume finds it.
    //
    // **Emits in tail position** (ADR 0003): parked at `AfterPhaseStartForced`
    // *before* the emit, so hunter movement cannot run above the step-3.1 forced
    // abilities the emit queues. `enemy_after_phase_start` sets the running
    // cursor and runs 3.2/3.3 on re-exposure.
    cx.state.continuations.push(Continuation::EnemyPhase {
        resume: EnemyResume::AfterPhaseStartForced,
        attacking: None,
    });
    emit::queue_event(
        cx,
        &TimingEvent::PhaseStarted {
            phase: Phase::Enemy,
        },
    )
}

/// Enemy steps 3.2 + 3.3, reached via the Enemy anchor's
/// [`AfterPhaseStartForced`](crate::state::EnemyResume::AfterPhaseStartForced)
/// resume once step 3.1's queued forced abilities have resolved.
///
/// If hunter movement suspends on a lead-investigator tie, this returns the
/// [`EngineOutcome::AwaitingInput`] unchanged — the attack-loop kickoff is
/// deferred to [`resume_hunter_choice`], which runs it once the last hunter
/// resolves.
fn enemy_after_phase_start(cx: &mut Cx) -> EngineOutcome {
    // Advance the anchor off `AfterPhaseStartForced` onto the phase's running
    // cursor. `attacking` stays `None`: the kickoff runs after hunter movement,
    // so no investigator is selected yet. `enemy_attack_kickoff` /
    // `after_enemy_phase_attacks` set both fields again before opening each
    // attack window.
    set_enemy_anchor(cx, EnemyResume::BeforeInvestigatorAttacked, None);

    // 3.2 Hunter enemies move. Park on a lead-investigator tie; the
    //     attack-loop kickoff then happens on resume.
    match hunters::drive_hunter_moves(cx) {
        outcome @ EngineOutcome::AwaitingInput { .. } => return outcome,
        // drive_hunter_moves only ever returns Done or AwaitingInput, never Rejected.
        EngineOutcome::Rejected { reason } => {
            unreachable!("enemy_after_phase_start: hunter movement rejected unexpectedly: {reason}")
        }
        EngineOutcome::Done => {}
    }

    // 3.3 Kick off the per-investigator attack loop.
    enemy_attack_kickoff(cx)
}

/// Called from [`anchor_on_child_pop`]'s
/// [`PhaseStep::AfterAllInvestigatorsAttacked`] arm. Emits step 3.4's
/// `PhaseEnded(Enemy)` marker and queues its forced abilities; the Enemy →
/// Upkeep transition runs from [`enemy_phase_end_transition`] once they have
/// resolved. Exact analog of [`upkeep_phase_end`].
///
/// **Emits in tail position** (ADR 0003). The emit *queues* frames — agenda
/// 01107's *"Forced – At the end of the enemy phase: Each unengaged [[Ghoul]]
/// enemy moves 1 location towards the Parlor"* — so the anchor is re-parked at
/// [`AfterPhaseEndForced`](crate::state::EnemyResume::AfterPhaseEndForced)
/// *beneath* them rather than popped. Popping it and pushing the Upkeep anchor
/// here (the pre-#569 shape) buried the queued frame at the bottom of the stack
/// for the rest of the scenario, because phase anchors pop-and-push rather than
/// drain: the Ghoul movement never happened in real play.
pub(crate) fn enemy_phase_end(cx: &mut Cx) -> EngineOutcome {
    // The AfterAllInvestigatorsAttacked window has closed, so the anchor is the
    // top frame (slice 1a, #393). It stays: the transition needs it as its
    // resume point.
    debug_assert!(
        matches!(
            cx.state.continuations.last(),
            Some(Continuation::EnemyPhase { .. })
        ),
        "enemy_phase_end: expected EnemyPhase anchor on top, got {:?}",
        cx.state.continuations.last(),
    );
    // 3.4 Enemy phase ends.
    cx.events.push(Event::PhaseEnded {
        phase: Phase::Enemy,
    });
    // Arm the resume BEFORE emitting: the emit may push an ordering run that
    // suspends across an `apply()` boundary, and the anchor beneath it must
    // already know where to continue. Uniform across 0 / 1 / 2+ hits — the loop
    // re-dispatches this anchor in every case.
    set_enemy_anchor(
        cx,
        EnemyResume::AfterPhaseEndForced,
        // The per-investigator attack cursor is spent by step 3.4; carrying it
        // into the transition would misreport the phase's state.
        None,
    );
    emit::queue_event(
        cx,
        &TimingEvent::PhaseEnded {
            phase: Phase::Enemy,
        },
    )
}

/// Enemy → Upkeep (slice 1b, #393), reached via the Enemy anchor's
/// [`AfterPhaseEndForced`](crate::state::EnemyResume::AfterPhaseEndForced)
/// resume once step 3.4's queued forced abilities have resolved: pop the anchor,
/// advance `state.phase`, and push the Upkeep anchor at `Entry`. The main loop's
/// `drive` advances that (running `upkeep_phase`, which may suspend at step 4.5's
/// hand-size discard — surfaced through `drive`).
fn enemy_phase_end_transition(cx: &mut Cx) -> EngineOutcome {
    debug_assert!(
        matches!(
            cx.state.continuations.last(),
            Some(Continuation::EnemyPhase { .. })
        ),
        "enemy_phase_end_transition: expected EnemyPhase anchor on top, got {:?}",
        cx.state.continuations.last(),
    );
    cx.state.continuations.pop();
    cx.state.phase = Phase::Upkeep;
    cx.state.continuations.push(Continuation::UpkeepPhase {
        resume: UpkeepResume::Entry,
    });
    EngineOutcome::Done
}

/// Called after the post-1.4 window closes (via the Mythos anchor's
/// [`AfterDraws`](crate::state::MythosResume::AfterDraws) resume). Emits 1.5's
/// `PhaseEnded(Mythos)` marker and queues its forced abilities; the Mythos →
/// Investigation transition runs from [`mythos_phase_end_transition`] once they
/// have resolved. Rotation is owned by `investigation_phase` (step 2.2), not by
/// `mythos_phase_end`.
///
/// **Emits in tail position** (ADR 0003). Wizard of the Order 01170 prints
/// *"**Forced** - At the end of the mythos phase: Place 1 doom on Wizard of the
/// Order."*, so this emit has a live corpus consumer once a card in play can
/// carry doom; popping the anchor and pushing the Investigation one here would
/// bury whatever it queues at the bottom of the stack, the #569 shape.
pub(super) fn mythos_phase_end(cx: &mut Cx) -> EngineOutcome {
    // The MythosAfterDraws window has closed, so the anchor is the top frame. It
    // stays: the transition needs it as its resume point.
    debug_assert!(
        matches!(
            cx.state.continuations.last(),
            Some(Continuation::MythosPhase { .. })
        ),
        "mythos_phase_end: expected MythosPhase anchor on top, got {:?}",
        cx.state.continuations.last(),
    );
    // 1.5 Mythos phase ends.
    //     The PhaseEnded(Mythos) emit lives HERE rather than in
    //     step_phase so step 1.5 has explicit ownership in the
    //     driver — mirror of step 1.1's PhaseStarted ownership in
    //     mythos_phase. Rules Reference p.24: "This step formalizes
    //     the end of the mythos phase."
    cx.events.push(Event::PhaseEnded {
        phase: Phase::Mythos,
    });
    // Arm the resume BEFORE emitting (the emit may suspend on an ordering run
    // that outlives this `apply()`), then emit in tail position.
    set_mythos_resume(cx, MythosResume::AfterPhaseEndForced);
    emit::queue_event(
        cx,
        &TimingEvent::PhaseEnded {
            phase: Phase::Mythos,
        },
    )
}

/// Mythos → Investigation (slice 1b, #393), reached via the Mythos anchor's
/// [`AfterPhaseEndForced`](crate::state::MythosResume::AfterPhaseEndForced)
/// resume once step 1.5's queued forced abilities have resolved: pop the anchor,
/// advance `state.phase`, and push the Investigation anchor at `Entry`. The main
/// loop's `drive` advances it (runs `investigation_phase`'s opening).
fn mythos_phase_end_transition(cx: &mut Cx) -> EngineOutcome {
    debug_assert!(
        matches!(
            cx.state.continuations.last(),
            Some(Continuation::MythosPhase { .. })
        ),
        "mythos_phase_end_transition: expected MythosPhase anchor on top, got {:?}",
        cx.state.continuations.last(),
    );
    cx.state.continuations.pop();
    cx.state.phase = Phase::Investigation;
    cx.state
        .continuations
        .push(Continuation::InvestigationPhase {
            resume: InvestigationResume::Entry,
        });
    EngineOutcome::Done
}

/// Advance a freshly-entered phase anchor (slice 1b, #393): if the top frame is
/// a `*Phase` anchor at `Entry`, pop the placeholder and run that phase's
/// opening via its existing driver (which pushes the running anchor at its first
/// boundary resume + the phase's first child). Returns `None` when the top is
/// not an `Entry` anchor, so [`anchor_on_child_pop`] falls through to its
/// boundary dispatch.
fn advance_phase_entry(cx: &mut Cx, anchor: Option<&Continuation>) -> Option<EngineOutcome> {
    match anchor {
        Some(Continuation::MythosPhase {
            resume: MythosResume::Entry,
        }) => {
            cx.state.continuations.pop();
            Some(mythos_phase(cx))
        }
        Some(Continuation::InvestigationPhase {
            resume: InvestigationResume::Entry,
        }) => {
            cx.state.continuations.pop();
            Some(investigation_phase(cx))
        }
        Some(Continuation::EnemyPhase {
            resume: EnemyResume::Entry,
            ..
        }) => {
            cx.state.continuations.pop();
            Some(enemy_phase(cx))
        }
        Some(Continuation::UpkeepPhase {
            resume: UpkeepResume::Entry,
        }) => {
            cx.state.continuations.pop();
            Some(upkeep_phase(cx))
        }
        _ => None,
    }
}

/// Mythos step 1.4 (#482): re-park the anchor at `AfterDraws`, then seed + open
/// the encounter draws. Reached from `anchor_on_child_pop`'s `MythosPhase{Draws}`
/// arm — i.e. once any `AdvanceReverse` frame (the agenda's on-advance reverse +
/// acknowledge) above the anchor has popped, so the agenda effect resolves before
/// any encounter is drawn (RR order).
fn run_mythos_draws(cx: &mut Cx) -> EngineOutcome {
    set_mythos_resume(cx, MythosResume::AfterDraws);
    // Per Rules Reference p.10 (Elimination), eliminated investigators (Defeated,
    // Resigned) do not draw — seed Active only.
    let remaining = cursor::active_investigators_in_turn_order(cx.state);
    if remaining.is_empty() {
        // No Active drawers: open + auto-skip the post-1.4 window inline (its
        // continuation runs mythos_phase_end → Investigation).
        let outcome = reaction_windows::open_fast_window(
            cx,
            FastWindowKind::Phase(PhaseStep::MythosAfterDraws),
        );
        debug_assert_eq!(
            outcome,
            EngineOutcome::Done,
            "open_fast_window(MythosAfterDraws) unexpectedly suspended",
        );
        return EngineOutcome::Done;
    }
    cx.state
        .continuations
        .push(Continuation::EncounterDraw { remaining });
    encounter::prompt_encounter_draw(cx)
}

/// Run the top `*Phase` anchor's continuation after one of its framework
/// windows closed (slice 1a, #393), or advance it from `Entry` (slice 1b, via
/// [`advance_phase_entry`]). The window has already been popped by the close
/// path, so the anchor is now the top frame; its `resume` selects the relocated
/// body. Suspension-agnostic: a body that itself suspends returns
/// `AwaitingInput` unchanged. The `resume` is copied out before the body takes
/// `&mut cx`.
// A single exhaustive dispatch over every anchor × resume boundary; splitting it
// would only obscure the phase-boundary map it draws.
#[allow(clippy::too_many_lines)]
pub(super) fn anchor_on_child_pop(cx: &mut Cx) -> EngineOutcome {
    let anchor = cx.state.continuations.last().cloned();
    // `Entry` advances (slice 1b, #393) run the phase opening; delegated so this
    // function stays the boundary-dispatch it was in slice 1a.
    if let Some(out) = advance_phase_entry(cx, anchor.as_ref()) {
        return out;
    }
    match anchor {
        // Step 4.1's queued `PhaseStarted { Upkeep }` forced abilities have
        // resolved, re-exposing this anchor (#697): open the post-4.1 window.
        Some(Continuation::UpkeepPhase {
            resume: UpkeepResume::AfterPhaseStartForced,
        }) => upkeep_after_phase_start(cx),
        Some(Continuation::UpkeepPhase {
            resume: UpkeepResume::Begins,
        }) => {
            // Structurally impossible under the main loop (slice 1b, #393): a
            // skill test in flight sits *above* its phase anchor, so `drive`
            // never advances the anchor with one pending. (Was an `unreachable!`
            // gated on "no Upkeep-phase skill-test source"; now a cheap assert.)
            debug_assert!(
                cx.state.current_skill_test().is_none(),
                "UpkeepBegins advanced with a skill test in flight",
            );
            upkeep_resume(cx)
        }
        // A step-4.4 drawn-weakness Revelation (#509) drained, re-exposing this
        // anchor: run 4.5 (hand size) + 4.6 (phase end + transition).
        Some(Continuation::UpkeepPhase {
            resume: UpkeepResume::AfterDraw,
        }) => upkeep_after_draw(cx),
        // Step 4.6's queued `PhaseEnded { Upkeep }` forced abilities have
        // resolved, re-exposing this anchor (#569): run the round end.
        Some(Continuation::UpkeepPhase {
            resume: UpkeepResume::AfterPhaseEndForced,
        }) => upkeep_round_end(cx),
        Some(Continuation::UpkeepPhase {
            resume: UpkeepResume::AfterRoundEnd,
        }) => {
            // The round-end `EmitEvent` coordinator (the `when` act advance + the
            // `at` doom) popped, re-exposing this anchor (#434). Run teardown
            // (expire until-end-of-round effects, Upkeep → Mythos).
            upkeep_round_end_teardown(cx)
        }
        // Step 3.1's queued `PhaseStarted { Enemy }` forced abilities have
        // resolved, re-exposing this anchor (#697): run 3.2 + 3.3.
        Some(Continuation::EnemyPhase {
            resume: EnemyResume::AfterPhaseStartForced,
            ..
        }) => enemy_after_phase_start(cx),
        Some(Continuation::EnemyPhase {
            resume: EnemyResume::BeforeInvestigatorAttacked,
            attacking,
        }) => {
            // Structurally impossible under the main loop (slice 1b): a skill
            // test in flight sits above its phase anchor, so `drive` never
            // advances the anchor with one pending.
            debug_assert!(
                cx.state.current_skill_test().is_none(),
                "BeforeInvestigatorAttacked advanced with a skill test in flight",
            );
            // Cursor expect-Some: BeforeInvestigatorAttacked is only ever opened
            // after the anchor's `attacking` cursor is set to Some(_). A None
            // here is a state-corruption invariant violation.
            let investigator = attacking.unwrap_or_else(|| {
                unreachable!(
                    "BeforeInvestigatorAttacked closed with the EnemyPhase anchor's \
                     `attacking` cursor == None; state-corruption invariant violation"
                )
            });
            // Tail position (ADR 0003): since #704 the attack loop parks itself
            // on a `Continuation::AttackLoop` frame and hands each attack to the
            // timing coordinator, so a `Done` here means *queued*, not *the
            // attacks are over*. The per-investigator cursor advance
            // (`after_enemy_phase_attacks`) therefore runs from the loop's own
            // drain (`finish_attack_loop`), never from this arm.
            combat::resolve_attacks_for_investigator(cx, investigator)
        }
        Some(Continuation::EnemyPhase {
            resume: EnemyResume::AfterAllAttacked,
            ..
        }) => {
            // Structurally impossible under the main loop (slice 1b): see the
            // BeforeInvestigatorAttacked arm above.
            debug_assert!(
                cx.state.current_skill_test().is_none(),
                "AfterAllInvestigatorsAttacked advanced with a skill test in flight",
            );
            enemy_phase_end(cx)
        }
        // Step 3.4's queued `PhaseEnded { Enemy }` forced abilities have
        // resolved, re-exposing this anchor (#569): transition to Upkeep.
        Some(Continuation::EnemyPhase {
            resume: EnemyResume::AfterPhaseEndForced,
            ..
        }) => enemy_phase_end_transition(cx),
        // Step 2.1's queued `PhaseStarted { Investigation }` forced abilities
        // have resolved, re-exposing this anchor (#697): open the post-2.1
        // window.
        Some(Continuation::InvestigationPhase {
            resume: InvestigationResume::AfterPhaseStartForced,
        }) => investigation_after_phase_start(cx),
        // Step 2.3's queued `PhaseEnded { Investigation }` forced abilities have
        // resolved, re-exposing this anchor (#697): transition to Enemy.
        Some(Continuation::InvestigationPhase {
            resume: InvestigationResume::AfterPhaseEndForced,
        }) => investigation_phase_end_transition(cx),
        Some(Continuation::InvestigationPhase {
            resume: InvestigationResume::Begins,
        }) => {
            // Post-2.1 window closed; start the first investigator's turn
            // (step 2.2). No skill-test-in-flight guard: runs at phase start
            // (no test in flight) and does not transition phase.
            if let Some(id) = cursor::first_active_investigator(cx.state) {
                begin_investigator_turn(cx, id);
            }
            // None branch: no active investigator can take a turn — the
            // cascade-breaker park (the loss already resolved at the defeat
            // site). See the former anchor_on_child_pop arm.
            EngineOutcome::Done
        }
        Some(Continuation::InvestigationPhase {
            resume: InvestigationResume::TurnBegins,
        }) => {
            // 2.2.1 — push the InvestigatorTurn frame above the anchor (slice
            // 2a-i, #393). The anchor stays at TurnBegins beneath it; the frame
            // is the open-turn idle point (drive breaks here, returning Done).
            // `active_investigator` was set by rotate_to_active in
            // begin_investigator_turn; it is the frame's investigator.
            let investigator = cx.state.active_investigator.unwrap_or_else(|| {
                unreachable!(
                    "TurnBegins reached with no active_investigator; \
                     begin_investigator_turn always sets it"
                )
            });
            cx.state.continuations.push(Continuation::InvestigatorTurn {
                investigator,
                ending: false,
            });
            EngineOutcome::Done
        }
        // Step 1.1's queued `PhaseStarted { Mythos }` forced abilities have
        // resolved, re-exposing this anchor (#697): run 1.2 + 1.3.
        Some(Continuation::MythosPhase {
            resume: MythosResume::AfterPhaseStartForced,
        }) => mythos_after_phase_start(cx),
        Some(Continuation::MythosPhase {
            resume: MythosResume::Draws,
        }) => run_mythos_draws(cx),
        // Step 1.5's queued `PhaseEnded { Mythos }` forced abilities have
        // resolved, re-exposing this anchor (#697): transition to Investigation.
        Some(Continuation::MythosPhase {
            resume: MythosResume::AfterPhaseEndForced,
        }) => mythos_phase_end_transition(cx),
        Some(Continuation::MythosPhase {
            resume: MythosResume::AfterDraws,
        }) => {
            // Structurally impossible under the main loop (slice 1b): a skill
            // test in flight sits above its phase anchor, so `drive` never
            // advances the anchor with one pending.
            debug_assert!(
                cx.state.current_skill_test().is_none(),
                "MythosAfterDraws advanced with a skill test in flight",
            );
            mythos_phase_end(cx)
        }
        other => {
            unreachable!("anchor_on_child_pop: top frame is not a known phase anchor: {other:?}")
        }
    }
}

/// Entered by [`step_phase`] on the Enemy→Upkeep transition. Owns the
/// `PhaseStarted(Upkeep)` emit (step 4.1) and queues its forced abilities;
/// [`upkeep_after_phase_start`] opens the post-4.1 player window once they have
/// resolved, and steps 4.2 onward run as that window's continuation
/// ([`upkeep_resume`]). Mirror of [`mythos_phase`], inverted: Mythos's
/// window sits at the END, so its driver runs content then opens;
/// Upkeep's sits at the START, so the driver opens immediately and the
/// content is the continuation.
fn upkeep_phase(cx: &mut Cx) -> EngineOutcome {
    // 4.1 Upkeep phase begins.
    cx.events.push(Event::PhaseStarted {
        phase: Phase::Upkeep,
    });
    // Push the Upkeep phase anchor (slice 1a, #393). It persists for the whole
    // phase — beneath the post-4.1 window, any step-4.5 hand-size discard, and
    // the round-end act window — and is popped at upkeep_round_end_teardown (the
    // single Upkeep→Mythos exit, after the round-end sequence finishes).
    //
    // **Emits in tail position** (ADR 0003): parked at `AfterPhaseStartForced`
    // *before* the emit, so the post-4.1 player window cannot open above the
    // step-4.1 forced abilities the emit queues.
    cx.state.continuations.push(Continuation::UpkeepPhase {
        resume: UpkeepResume::AfterPhaseStartForced,
    });
    emit::queue_event(
        cx,
        &TimingEvent::PhaseStarted {
            phase: Phase::Upkeep,
        },
    )
}

/// The post-4.1 player window, reached via the Upkeep anchor's
/// [`AfterPhaseStartForced`](crate::state::UpkeepResume::AfterPhaseStartForced)
/// resume once step 4.1's queued forced abilities have resolved. Auto-skips
/// inline (running [`upkeep_resume`] via the anchor's on-child-pop) when nothing
/// is Fast-eligible.
fn upkeep_after_phase_start(cx: &mut Cx) -> EngineOutcome {
    set_upkeep_resume(cx, UpkeepResume::Begins);
    reaction_windows::open_fast_window(cx, FastWindowKind::Phase(PhaseStep::UpkeepBegins))
}

/// The post-4.1 window continuation. Steps 4.2–4.4 run inline as named call
/// sites. The 4.4 draw may resolve a drawn-weakness Revelation (#509), which
/// `push_effect`s onto the stack above this anchor; when that happens
/// `upkeep_resume` cedes (resume `AfterDraw`) so the drive loop drains the
/// Revelation, then re-runs 4.5/4.6 via [`upkeep_after_draw`] on re-exposure.
/// Otherwise 4.5/4.6 run inline: step 4.5 ([`check_hand_size`]) may suspend
/// with [`EngineOutcome::AwaitingInput`] when an investigator is over the hand
/// cap — in which case 4.6 runs only once the discard resolves — else it hands
/// to [`upkeep_phase_end`] for 4.6 + transition.
pub(super) fn upkeep_resume(cx: &mut Cx) -> EngineOutcome {
    reset_actions(cx); // 4.2
    ready_exhausted_cards(cx); // 4.3
                               // Sentinel for "4.4 pushed a continuation *above us*". Captured after
                               // 4.2/4.3 because those are pure state mutations that never push — so a
                               // change here is attributable to the 4.4 draw (a drawn-weakness
                               // Revelation; or any future pusher, which the cede below handles
                               // uniformly).
                               //
                               // Compares the top frame rather than the stack depth: 4.4's draw can
                               // deal lethal harm, and eliminating the last investigator latches a
                               // resolution, which inserts a `ScenarioEnd` frame at the *bottom* of the
                               // stack (#566). That grows the depth without pushing anything above this
                               // anchor, so a length sentinel would cede to a Revelation that does not
                               // exist.
    let top_before = cx.state.continuations.last().cloned();
    upkeep_draw_and_resource(cx); // 4.4 — may push a drawn-weakness Revelation (#509)
    if cx.state.continuations.last() != top_before.as_ref() {
        // 4.4 pushed a drawn-weakness Revelation above the (now-buried) UpkeepPhase
        // anchor. Cede: the drive loop resolves the Revelation, then re-exposes the
        // anchor at AfterDraw (anchor_on_child_pop → upkeep_after_draw) for 4.5/4.6.
        set_upkeep_resume(cx, UpkeepResume::AfterDraw);
        return EngineOutcome::Done;
    }
    upkeep_after_draw(cx) // 4.5 + 4.6 inline — common case, nothing pushed
}

/// Steps 4.5 (hand size; may suspend) + 4.6 (phase end + round-end transition).
/// Run inline by `upkeep_resume` when 4.4 pushed nothing, or via
/// `anchor_on_child_pop`'s `AfterDraw` arm once a 4.4 drawn-weakness Revelation
/// has drained.
fn upkeep_after_draw(cx: &mut Cx) -> EngineOutcome {
    if let outcome @ EngineOutcome::AwaitingInput { .. } = check_hand_size(cx) {
        return outcome; // 4.5 parked for discard; 4.6 runs on resume
    }
    upkeep_phase_end(cx) // 4.6 + transition (may open the act round-end window)
}

/// Owns step 4.6's `PhaseEnded(Upkeep)` emit and queues its forced abilities;
/// the round-end sequence follows from [`upkeep_round_end`] once they have
/// resolved. Exact analog of [`enemy_phase_end`]. `step_phase` emits no
/// `PhaseEnded` itself — every phase's `*_end` helper owns its own.
///
/// **Emits in tail position** (ADR 0003). The emit queues rather than resolves,
/// so the round-end emit that follows it cannot run here: it would push the
/// `RoundEnded` coordinator *above* the phase-end forced abilities and resolve
/// the round end first. The anchor is re-parked at
/// [`AfterPhaseEndForced`](crate::state::UpkeepResume::AfterPhaseEndForced) and
/// the loop re-exposes it when they are done.
pub(crate) fn upkeep_phase_end(cx: &mut Cx) -> EngineOutcome {
    // 4.6 Upkeep phase ends.
    cx.events.push(Event::PhaseEnded {
        phase: Phase::Upkeep,
    });
    // Arm the resume BEFORE emitting (the emit may suspend on an ordering run
    // that outlives this `apply()`), then emit in tail position.
    set_upkeep_resume(cx, UpkeepResume::AfterPhaseEndForced);
    emit::queue_event(
        cx,
        &TimingEvent::PhaseEnded {
            phase: Phase::Upkeep,
        },
    )
}

/// "Round ends" (RR p.24), reached via the Upkeep anchor's
/// [`AfterPhaseEndForced`](crate::state::UpkeepResume::AfterPhaseEndForced)
/// resume once step 4.6's queued `PhaseEnded { Upkeep }` forced abilities have
/// resolved.
///
/// The `when the round ends` act advance (act 01109) and the `at the end of the
/// round` doom (agenda 01107, Dissonant Voices 01165) resolve as the `RoundEnded`
/// `EmitEvent` coordinator's `When`/`At` cells (#434) — structural ordering, no
/// hand-threading. Sets this anchor's resume so [`upkeep_round_end_teardown`]
/// runs when the coordinator pops, then cedes: the emit pushes the coordinator
/// and returns `Done`; the global loop drives the bucket walk (suspending at the
/// `when` window) and re-exposes this anchor at `AfterRoundEnd` on completion.
fn upkeep_round_end(cx: &mut Cx) -> EngineOutcome {
    set_upkeep_resume(cx, UpkeepResume::AfterRoundEnd);
    emit::queue_event(cx, &TimingEvent::RoundEnded)
}

/// Set the [`UpkeepPhase`](crate::state::Continuation::UpkeepPhase) anchor's
/// resume cursor. Reverse-searches the stack (mirroring [`set_enemy_anchor`])
/// so it is robust whether the anchor is on top (`upkeep_phase_end`'s call, made
/// before the round-end coordinator is pushed) or buried beneath a drawn-weakness
/// Revelation pushed by the step-4.4 draw (#509, `upkeep_resume`'s cede).
fn set_upkeep_resume(cx: &mut Cx, resume: UpkeepResume) {
    if let Some(c) = cx
        .state
        .continuations
        .iter_mut()
        .rev()
        .find(|c| matches!(c, Continuation::UpkeepPhase { .. }))
    {
        *c = Continuation::UpkeepPhase { resume };
    } else {
        unreachable!("set_upkeep_resume: no UpkeepPhase anchor on the stack");
    }
}

/// Teardown after the round-end `EmitEvent` coordinator pops — reached via the
/// Upkeep anchor's [`AfterRoundEnd`](crate::state::UpkeepResume::AfterRoundEnd)
/// resume (#434, subsuming the former `ForcedContinuation::UpkeepAfterRoundEnded`):
/// expire active "until the end of the round" lasting effects (Mind over Matter
/// 01036's substitution — RR p.24, "after the round-end forced abilities have
/// resolved"), then transition Upkeep → Mythos.
pub(super) fn upkeep_round_end_teardown(cx: &mut Cx) -> EngineOutcome {
    cx.state.skill_substitutions.clear();
    // Pop the Upkeep anchor (slice 1a, #393): this is the single Upkeep→Mythos
    // exit, reached after the whole round-end sequence (act window, doom) has
    // resolved, so the anchor is the top frame here.
    debug_assert!(
        matches!(
            cx.state.continuations.last(),
            Some(Continuation::UpkeepPhase { .. })
        ),
        "upkeep_round_end_teardown: expected UpkeepPhase anchor on top, got {:?}",
        cx.state.continuations.last(),
    );
    cx.state.continuations.pop();
    // Upkeep → Mythos (slice 1b, #393): advance `state.phase` + push the Mythos
    // anchor at `Entry`. The main loop's `drive` advances it (runs mythos_phase —
    // the round bump + PhaseStarted(Mythos) live there). Replaces the former
    // synchronous `step_phase(cx)`. With all four transitions now loop-driven,
    // `step_phase` is gone.
    cx.state.phase = Phase::Mythos;
    cx.state.continuations.push(Continuation::MythosPhase {
        resume: MythosResume::Entry,
    });
    EngineOutcome::Done
}

/// 4.3 Ready exhausted cards. Rules Reference p.25: "Simultaneously
/// ready each exhausted card." "Each exhausted card" is every exhausted
/// card in play regardless of controller — investigator in-play cards
/// AND enemies. Simultaneous, so iteration order is immaterial; we
/// iterate deterministically (investigator id then in-play order; then
/// enemy id) for reproducible event streams. Already-ready cards emit
/// nothing.
///
/// After readying, each enemy that became ready while unengaged and
/// co-located with an investigator engages it via [`reengage_at_location`]
/// (Rules Reference p.10: "if an exhausted enemy at the same location as an
/// investigator becomes ready, it engages as soon as it is readied").
fn ready_exhausted_cards(cx: &mut Cx) {
    let inv_ids: Vec<InvestigatorId> = cx.state.investigators.keys().copied().collect();
    for id in inv_ids {
        let inv = cx.state.investigators.get_mut(&id).expect("id from keys");
        for card in &mut inv.cards_in_play {
            if card.exhausted {
                card.exhausted = false;
                cx.events.push(Event::CardReadied {
                    investigator: id,
                    instance_id: card.instance_id,
                    code: card.code.clone(),
                });
            }
        }
    }
    let enemy_ids: Vec<EnemyId> = cx.state.enemies.keys().copied().collect();
    let mut newly_readied: Vec<EnemyId> = Vec::new();
    for eid in enemy_ids {
        let enemy = cx.state.enemies.get_mut(&eid).expect("id from keys");
        if enemy.exhausted {
            enemy.exhausted = false;
            cx.events.push(Event::EnemyReadied { enemy: eid });
            newly_readied.push(eid);
        }
    }
    // RR p.10: "if an exhausted enemy at the same location as an investigator
    // becomes ready, it engages as soon as it is readied." Runs after the
    // (simultaneous, RR p.25) readying pass. Only newly-readied enemies are
    // checked ("becomes ready"), and only those still unengaged —
    // reengage_at_location's precondition is engaged_with == None, so an enemy
    // that readied while still engaged keeps its existing engagement.
    // newly_readied is in ascending EnemyId order (BTreeMap key order).
    for eid in newly_readied {
        if cx.state.enemies[&eid].engaged_with.is_none() {
            hunters::reengage_at_location(cx, eid);
        }
    }
}

/// Maximum hand size (Rules Reference p.25 step 4.5: discard down to 8). A module
/// constant rather than a per-investigator field — no card in the
/// current scope modifies the cap. A future hand-size-modifying card
/// introduces the field when it is actually needed (#111 spec).
pub(super) const HAND_SIZE_LIMIT: u8 = 8;

/// Active investigators, in player order, whose hand exceeds
/// [`HAND_SIZE_LIMIT`]. Empty when nobody is over the cap.
pub(super) fn over_cap_investigators(state: &GameState) -> Vec<InvestigatorId> {
    cursor::active_investigators_in_turn_order(state)
        .into_iter()
        .filter(|id| state.investigators[id].hand.len() > HAND_SIZE_LIMIT as usize)
        .collect()
}

/// Pushes a `HandSizeDiscard(remaining)` frame and returns the
/// [`EngineOutcome::AwaitingInput`] that prompts `remaining[0]` to discard.
/// Used by both [`check_hand_size`] (first suspension) and
/// [`resume_hand_size_discard`] (re-prompt after a queue pop).
///
/// `remaining` must be non-empty; callers ensure this before calling.
fn park_hand_size_discard(cx: &mut Cx, remaining: Vec<InvestigatorId>) -> EngineOutcome {
    cx.state
        .continuations
        .push(Continuation::HandSizeDiscard(HandSizeDiscard { remaining }));
    EngineOutcome::AwaitingInput {
        request: InputRequest::pick_multiple(format!(
            "You have more than {HAND_SIZE_LIMIT} cards in hand — choose cards to discard \
             down to {HAND_SIZE_LIMIT}.",
        )),
        resume_token: ResumeToken(0),
    }
}

/// 4.5 Each investigator checks hand size. In player order, each
/// investigator over [`HAND_SIZE_LIMIT`] is prompted to discard down to
/// the cap. Returns [`EngineOutcome::AwaitingInput`] (parking on the
/// first over-cap investigator) when anyone is over, or
/// [`EngineOutcome::Done`] when nobody is — in which case the caller
/// proceeds straight to 4.6.
fn check_hand_size(cx: &mut Cx) -> EngineOutcome {
    let remaining = over_cap_investigators(cx.state);
    if remaining.is_empty() {
        return EngineOutcome::Done;
    }
    park_hand_size_discard(cx, remaining)
}

/// Resume a parked upkeep hand-size discard (#111). Validates the
/// `PickMultiple` response against the currently-prompted investigator
/// (`remaining[0]`): the indices must be unique, in-bounds, and exactly
/// `hand.len() - HAND_SIZE_LIMIT` in count. On success, discards the
/// chosen cards (emitting [`Event::CardDiscarded`] per card), pops the
/// queue front, and either re-prompts the next over-cap investigator or
/// — when the queue drains — runs [`upkeep_phase_end`] (4.6 + transition
/// to Mythos). Rejections leave state and events untouched.
pub(super) fn resume_hand_size_discard(cx: &mut Cx, response: &InputResponse) -> EngineOutcome {
    let Some(Continuation::HandSizeDiscard(pending)) = cx.state.continuations.last() else {
        unreachable!("resume_hand_size_discard: no HandSizeDiscard frame on top of the stack")
    };
    let pending = pending.clone();
    let current = pending.remaining[0];

    let InputResponse::PickMultiple { selected } = response else {
        return EngineOutcome::Rejected {
            reason: format!(
                "ResolveInput: hand-size discard expects InputResponse::PickMultiple, got {response:?}",
            )
            .into(),
        };
    };
    // Each OptionId is a hand index.
    let indices: Vec<u32> = selected.iter().map(|o| o.0).collect();

    // ---- validate (state untouched on any failure) ----
    let inv = cx.state.investigators.get(&current).unwrap_or_else(|| {
        unreachable!("resume_hand_size_discard: prompted investigator {current:?} vanished")
    });
    let hand_len = inv.hand.len();
    let target = hand_len.saturating_sub(HAND_SIZE_LIMIT as usize);
    if indices.len() != target {
        return EngineOutcome::Rejected {
            reason: format!(
                "hand-size discard: {current:?} must discard exactly {target} card(s) \
                 (hand {hand_len}, cap {HAND_SIZE_LIMIT}), got {}",
                indices.len(),
            )
            .into(),
        };
    }
    let mut seen = BTreeSet::new();
    for &i in &indices {
        if !seen.insert(i) {
            return EngineOutcome::Rejected {
                reason: format!("hand-size discard: duplicate hand index {i}").into(),
            };
        }
        if i as usize >= hand_len {
            return EngineOutcome::Rejected {
                reason: format!(
                    "hand-size discard: hand index {i} out of bounds (hand size {hand_len})",
                )
                .into(),
            };
        }
    }

    // ---- mutate ----
    let discarded: Vec<CardCode> = {
        let inv = cx
            .state
            .investigators
            .get_mut(&current)
            .expect("validated above");
        let mut sorted: Vec<u32> = indices.clone();
        sorted.sort_unstable();
        let codes: Vec<CardCode> = sorted
            .iter()
            .map(|&i| inv.hand[i as usize].clone())
            .collect();
        for &i in sorted.iter().rev() {
            inv.hand.remove(i as usize);
        }
        inv.discard.extend(codes.iter().cloned());
        codes
    };
    for code in discarded {
        cx.events.push(Event::CardDiscarded {
            investigator: current,
            code,
            from: Zone::Hand,
        });
    }

    // ---- advance the queue ----
    let mut remaining = pending.remaining;
    remaining.remove(0);
    // Pop the current HandSizeDiscard frame (validated above; it is the top frame).
    cx.state.continuations.pop();
    if remaining.is_empty() {
        upkeep_phase_end(cx) // 4.6 + transition (may open the act round-end window)
    } else {
        park_hand_size_discard(cx, remaining)
    }
}

/// 4.2 Reset actions. Rules Reference p.25: "Flip each investigator's
/// mini card back to its colored side. This indicates that the
/// investigator's actions have been reset for his or her next turn."
///
/// The canonical action-refresh site. Sets `actions_remaining` to
/// `ACTIONS_PER_TURN` for each Active investigator and emits
/// `ActionsRemainingChanged` when the value changes. `rotate_to_active`
/// no longer refreshes (step 2.2 is just "the turn begins");
/// `start_scenario` seeds round 1. Eliminated investigators are skipped
/// (Rules Reference p.10).
fn reset_actions(cx: &mut Cx) {
    for id in cursor::active_investigators_in_turn_order(cx.state) {
        let inv = cx
            .state
            .investigators
            .get_mut(&id)
            .expect("id from active_investigators_in_turn_order");
        if inv.actions_remaining != ACTIONS_PER_TURN {
            inv.actions_remaining = ACTIONS_PER_TURN;
            cx.events.push(Event::ActionsRemainingChanged {
                investigator: id,
                new_count: ACTIONS_PER_TURN,
            });
        }
    }
}

/// 4.4 Each investigator draws 1 card and gains 1 resource. Rules
/// Reference p.25: "In player order, each investigator draws 1 card.
/// Once those cards have been drawn, each investigator gains 1
/// resource." Two passes to honor that ordering: all draws first, then
/// all resource gains.
fn upkeep_draw_and_resource(cx: &mut Cx) {
    let ids = cursor::active_investigators_in_turn_order(cx.state);
    for &id in &ids {
        cards::draw_one_with_deckout(cx, id);
    }
    for &id in &ids {
        cards::grant_resources(cx, id, 1);
    }
}

#[cfg(test)]
mod tests;
