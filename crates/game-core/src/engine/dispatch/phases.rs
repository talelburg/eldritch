//! Phase-driver functions: start/end scenario, per-phase entrypoints,
//! and the round-cycle stepping logic.

use crate::action::InputResponse;
use crate::engine::outcome::{EngineOutcome, InputRequest, ResumeToken};
use crate::event::Event;
use crate::state::{
    CardCode, EnemyId, FastWindowKind, GameState, HandSizeDiscard, InvestigatorId, Phase,
    PhaseStep, Zone,
};

use crate::action::RosterEntry;
use crate::card_data::CardKind;
use crate::state::{CardInPlay, Investigator, Skills, Status};

use super::Cx;

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
    let registry = crate::card_registry::current();
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
                cards_in_play: Vec::new(),
                threat_area: Vec::new(),
                removed_from_game: Vec::new(),
                action_surcharge_spent_this_round: std::collections::BTreeSet::new(),
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
        super::reveal::reveal_location(cx, loc);
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
    // their deck and deal an initial hand of up to 5.
    let inv_ids: Vec<InvestigatorId> = cx.state.investigators.keys().copied().collect();
    for inv_id in inv_ids {
        super::cards::shuffle_player_deck(cx, inv_id);
        super::cards::draw_cards(cx, inv_id, super::cards::INITIAL_HAND_SIZE);
    }

    // Shuffle the shared encounter deck with the same scenario-start RNG
    // (Rules Reference p.21: the encounter deck is shuffled during setup).
    // `setup()` seeds it in deterministic construction order; this is the
    // single randomizing step. A <2-card deck (the synthetic test fixture)
    // shuffles to a no-op (no event).
    super::encounter::shuffle_encounter_deck(cx);

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
    let remaining = super::cursor::active_investigators_in_turn_order(cx.state);
    if remaining.is_empty() {
        investigation_phase(cx);
        return EngineOutcome::Done;
    }
    super::cards::prompt_mulligan(cx, remaining)
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
    // `ending` flag BEFORE emitting `EndOfTurn`, then emit. `emit_event` pushes
    // the forced/reaction abilities (Frozen in Fear 01164's willpower test, a 2+
    // forced run) as frames; the `drive` loop drives them and, once they pop,
    // re-dispatches the `InvestigatorTurn { ending: true }` frame → `resume_end_turn`
    // for the rotation (RR p.24 step 2.2.2). Uniform whether the forced run is
    // empty, completes immediately, or suspends — there is no inline-resume branch.
    // A `Rejected` from `emit_event` rolls back the armed flag with the rest of
    // the apply (transactional snapshot).
    let ending = cx
        .state
        .continuations
        .iter_mut()
        .rev()
        .find_map(|c| match c {
            crate::state::Continuation::InvestigatorTurn {
                investigator,
                ending,
            } if *investigator == active_id => Some(ending),
            _ => None,
        })
        .unwrap_or_else(|| {
            unreachable!("end_turn: no InvestigatorTurn({active_id:?}) on the stack")
        });
    *ending = true;
    super::emit::emit_event(
        cx,
        &super::emit::TimingEvent::EndOfTurn {
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
            Some(crate::state::Continuation::InvestigatorTurn { investigator, .. })
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
    if let Some(next_id) = super::cursor::next_active_investigator_after(cx.state, active_id) {
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
/// Owns the `PhaseStarted(Investigation)` emit (Rules Reference p.24
/// step 2.1) and opens the post-2.1 player window. Rotation to the
/// first active investigator (step 2.2) runs in the
/// [`PhaseStep::InvestigationBegins`] continuation via
/// [`begin_investigator_turn`], lead-first by default; explicit
/// player-pick within this window is deferred to #146.
///
/// The window auto-skips inline when nothing is Fast-eligible
/// ([`any_fast_play_eligible`] returns `false` — e.g. no Fast card in any
/// hand, which is always the case in unit tests with no card registry
/// installed), so single-investigator entry still lands the lead active
/// within the same `apply()` call.
pub(super) fn investigation_phase(cx: &mut Cx) {
    // 2.1 Investigation phase begins.
    cx.events.push(Event::PhaseStarted {
        phase: Phase::Investigation,
    });
    // Push the Investigation phase anchor (slice 1a, #393). It persists for the
    // whole phase (across every investigator's turn), beneath the framework
    // windows; popped at investigation_phase_end. Starts at `Begins` (the
    // post-2.1 InvestigationBegins window opens next).
    cx.state
        .continuations
        .push(crate::state::Continuation::InvestigationPhase {
            resume: crate::state::InvestigationResume::Begins,
        });
    // PLAYER WINDOW (post-2.1). Rotation to the first investigator
    // (step 2.2) runs in this window's continuation
    // (`anchor_on_child_pop` → `InvestigationBegins`), so the printed
    // order 2.1 → window → 2.2 holds. Auto-skips inline when nothing is
    // Fast-eligible, so single-investigator entry still lands the lead
    // active within the same apply() call.
    let outcome = super::reaction_windows::open_fast_window(
        cx,
        FastWindowKind::Phase(PhaseStep::InvestigationBegins),
    );
    debug_assert_eq!(
        outcome,
        EngineOutcome::Done,
        "open_fast_window(InvestigationBegins) unexpectedly suspended; this window has no suspending continuation",
    );
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
    // (slice 1a, #393). The anchor is the bottom-most Investigation frame.
    if let Some(anchor) = cx
        .state
        .continuations
        .iter_mut()
        .rev()
        .find(|c| matches!(c, crate::state::Continuation::InvestigationPhase { .. }))
    {
        *anchor = crate::state::Continuation::InvestigationPhase {
            resume: crate::state::InvestigationResume::TurnBegins,
        };
    }
    let outcome = super::reaction_windows::open_fast_window(
        cx,
        FastWindowKind::Phase(PhaseStep::InvestigatorTurnBegins),
    );
    debug_assert_eq!(
        outcome,
        EngineOutcome::Done,
        "open_fast_window(InvestigatorTurnBegins) unexpectedly suspended; this window has no suspending continuation",
    );
}

/// 2.3 Investigation phase ends. Owns the `PhaseEnded(Investigation)`
/// emit — lifted out of `step_phase`, mirroring `mythos_phase_end` /
/// `enemy_phase_end` / `upkeep_phase_end` — then transitions to the
/// Enemy phase. Called only from `end_turn`'s terminal branch (the last
/// investigator has taken a turn this round).
fn investigation_phase_end(cx: &mut Cx) -> EngineOutcome {
    // Pop the Investigation anchor (slice 1a, #393): the phase ends here (last
    // investigator's turn is over), so the anchor — the bottom-most
    // Investigation frame, top once the open-action turn finished — is disposed.
    debug_assert!(
        matches!(
            cx.state.continuations.last(),
            Some(crate::state::Continuation::InvestigationPhase { .. })
        ),
        "investigation_phase_end: expected InvestigationPhase anchor on top, got {:?}",
        cx.state.continuations.last(),
    );
    cx.state.continuations.pop();
    // No forced-trigger dispatch here: only Enemy and Upkeep phase-ends have
    // slice consumers (agenda 01107). A `PhaseEnded { Investigation }` forced
    // ability would NOT fire until #212's emit_event restructure centralises
    // forced dispatch across all framework windows.
    cx.events.push(Event::PhaseEnded {
        phase: Phase::Investigation,
    });
    // Investigation → Enemy (slice 1b, #393): advance `state.phase` + push the
    // Enemy anchor at `Entry`. The main loop's `drive` advances it (runs
    // enemy_phase); a hunter-movement-tie suspension surfaces through `drive`.
    // Replaces the former synchronous `step_phase(cx)`.
    cx.state.phase = Phase::Enemy;
    cx.state
        .continuations
        .push(crate::state::Continuation::EnemyPhase {
            resume: crate::state::EnemyResume::Entry,
            attacking: None,
        });
    EngineOutcome::Done
}

/// Entered by [`step_phase`] on the Upkeep→Mythos transition. Lays
/// out the Rules Reference p.24 sub-steps as discrete named call
/// sites so the rule structure is grep-able and #73 / future-peril-PR
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
    // Push the Mythos phase anchor (slice 1a, #393). It sits beneath the
    // phase's framework windows; the post-1.4 MythosAfterDraws window's close
    // routes to its on_child_pop. AfterDraws is the only Mythos boundary, so
    // the resume is fixed at entry.
    cx.state
        .continuations
        .push(crate::state::Continuation::MythosPhase {
            resume: crate::state::MythosResume::AfterDraws,
        });

    // 1.2 Place 1 doom on the current agenda.
    super::act_agenda::place_doom_on_agenda(cx);

    // 1.3 Check doom threshold.
    super::act_agenda::check_doom_threshold(cx);

    // 1.4 Each investigator draws 1 encounter card.
    //     Push the `EncounterDraw` loop frame; the actual draws are
    //     player-driven via `ResolveInput(Confirm)` against the top frame
    //     (#348), and `resume_encounter_draw` advances the queue after each
    //     chain. Per Rules Reference p.10 (Elimination), eliminated
    //     investigators (Killed, Insane, Resigned) do not draw — the queue is
    //     seeded with the Active investigators only.
    let remaining = super::cursor::active_investigators_in_turn_order(cx.state);
    if remaining.is_empty() {
        // No Active investigators to draw (turn_order is empty or all
        // investigators are eliminated). Open the post-1.4 window
        // immediately; open_fast_window's auto-skip path triggers
        // because nothing is eligible, runs the MythosAfterDraws
        // continuation (mythos_phase_end), which transitions to
        // Investigation. All in this same apply.
        let outcome = super::reaction_windows::open_fast_window(
            cx,
            FastWindowKind::Phase(PhaseStep::MythosAfterDraws),
        );
        debug_assert_eq!(
            outcome,
            EngineOutcome::Done,
            "open_fast_window(MythosAfterDraws) unexpectedly suspended; this window has no suspending continuation",
        );
        return EngineOutcome::Done;
    }
    cx.state
        .continuations
        .push(crate::state::Continuation::EncounterDraw { remaining });
    super::encounter::prompt_encounter_draw(cx)
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
    use crate::state::{
        Continuation, EnemyResume, InvestigationResume, MythosResume, UpkeepResume,
    };
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
    super::drive(cx, EngineOutcome::Done)
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
/// Eliminated investigators (Killed / Insane / Resigned) are skipped per
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
    open_attack_window(cx, super::cursor::first_active_investigator(cx.state))
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
            crate::state::EnemyResume::BeforeInvestigatorAttacked,
            PhaseStep::BeforeInvestigatorAttacked,
        ),
        None => (
            crate::state::EnemyResume::AfterAllAttacked,
            PhaseStep::AfterAllInvestigatorsAttacked,
        ),
    };
    set_enemy_anchor(cx, resume, attacking);
    super::reaction_windows::open_fast_window(cx, FastWindowKind::Phase(step))
}

/// Set the Enemy phase anchor's `resume` and `attacking` cursor together (slice
/// 1a / #411) so neither is dropped. The low-level primitive behind
/// [`open_attack_window`]; the anchor is the bottom-most Enemy frame, and this
/// is a no-op if it is absent (only in tests that drive the attack loop in
/// isolation).
pub(super) fn set_enemy_anchor(
    cx: &mut Cx,
    resume: crate::state::EnemyResume,
    attacking: Option<InvestigatorId>,
) {
    if let Some(c) = cx
        .state
        .continuations
        .iter_mut()
        .rev()
        .find(|c| matches!(c, crate::state::Continuation::EnemyPhase { .. }))
    {
        *c = crate::state::Continuation::EnemyPhase { resume, attacking };
    }
}

/// Entered by [`step_phase`] on the Investigation→Enemy transition.
/// Owns the `PhaseStarted(Enemy)` emit (Rules Reference p.25 step 3.1),
/// runs hunter movement (step 3.2) via [`drive_hunter_moves`], then
/// kicks off the per-investigator attack loop (step 3.3) via
/// [`enemy_attack_kickoff`].
///
/// If hunter movement suspends on a lead-investigator tie, this returns
/// the [`EngineOutcome::AwaitingInput`] unchanged — the attack-loop
/// kickoff is deferred to [`resume_hunter_choice`], which runs it once
/// the last hunter resolves. Otherwise the kickoff runs inline here and
/// this returns its outcome — usually [`EngineOutcome::Done`], but the
/// Enemy → Upkeep cascade can suspend at step 4.5 (hand-size discard,
/// #111), so that `AwaitingInput` now propagates rather than being dropped.
fn enemy_phase(cx: &mut Cx) -> EngineOutcome {
    // 3.1 Enemy phase begins.
    cx.events.push(Event::PhaseStarted {
        phase: Phase::Enemy,
    });
    // Push the Enemy phase anchor (slice 1a, #393) before hunter movement, so a
    // lead-tie suspension parks above it and the kickoff on resume finds it.
    // `enemy_attack_kickoff` / `after_enemy_phase_attacks` set its `resume` and
    // `attacking` cursor before opening each attack window. The placeholder
    // resume + `None` cursor are overwritten before the first window opens
    // (kickoff runs after hunter movement, which is why `attacking` starts
    // `None` here — no investigator is selected yet).
    cx.state
        .continuations
        .push(crate::state::Continuation::EnemyPhase {
            resume: crate::state::EnemyResume::BeforeInvestigatorAttacked,
            attacking: None,
        });

    // 3.2 Hunter enemies move. Park on a lead-investigator tie; the
    //     attack-loop kickoff then happens on resume.
    match super::hunters::drive_hunter_moves(cx) {
        outcome @ EngineOutcome::AwaitingInput { .. } => return outcome,
        // drive_hunter_moves only ever returns Done or AwaitingInput, never Rejected.
        EngineOutcome::Rejected { reason } => {
            unreachable!("enemy_phase: hunter movement rejected unexpectedly: {reason}")
        }
        EngineOutcome::Done => {}
    }

    // 3.3 Kick off the per-investigator attack loop.
    enemy_attack_kickoff(cx)
}

/// Called from [`anchor_on_child_pop`]'s
/// [`PhaseStep::AfterAllInvestigatorsAttacked`] arm. Emits step
/// 3.4's `PhaseEnded(Enemy)` marker, then transitions to Upkeep.
/// Exact analog of [`mythos_phase_end`] / [`upkeep_phase_end`].
pub(super) fn enemy_phase_end(cx: &mut Cx) -> EngineOutcome {
    // Pop the Enemy anchor (slice 1a, #393): the AfterAllInvestigatorsAttacked
    // window has closed, so the anchor is the top frame, and the phase ends.
    debug_assert!(
        matches!(
            cx.state.continuations.last(),
            Some(crate::state::Continuation::EnemyPhase { .. })
        ),
        "enemy_phase_end: expected EnemyPhase anchor on top, got {:?}",
        cx.state.continuations.last(),
    );
    cx.state.continuations.pop();
    // 3.4 Enemy phase ends.
    cx.events.push(Event::PhaseEnded {
        phase: Phase::Enemy,
    });
    // Fire forced act/agenda abilities keyed to `PhaseEnded { Enemy }`.
    // Single-trigger path: 0 → Done (no-op); 1 → resolves immediately;
    // 2+ → rejects loudly (#213 adds the ordering loop).
    let forced = super::emit::emit_event(
        cx,
        &super::emit::TimingEvent::PhaseEnded {
            phase: Phase::Enemy,
        },
    );
    if !matches!(forced, EngineOutcome::Done) {
        return forced; // 2+-trigger loud reject (unreachable in-slice); propagate
    }
    // Enemy → Upkeep (slice 1b, #393): advance `state.phase` + push the Upkeep
    // anchor at `Entry`. The main loop's `drive` advances it (runs upkeep_phase,
    // which may suspend at step 4.5 hand-size discard — surfaced through
    // `drive`). Replaces the former synchronous `step_phase(cx)`.
    cx.state.phase = Phase::Upkeep;
    cx.state
        .continuations
        .push(crate::state::Continuation::UpkeepPhase {
            resume: crate::state::UpkeepResume::Entry,
        });
    EngineOutcome::Done
}

/// Called after the post-1.4 window closes. Emits 1.5's
/// `PhaseEnded(Mythos)` marker, then transitions to Investigation.
/// Rotation is owned by `investigation_phase` (step 2.2), not by
/// `mythos_phase_end`. Invoked from `close_reaction_window`'s
/// kind-aware tail when a `MythosAfterDraws` window pops, and from
/// `open_fast_window`'s auto-skip path inline.
pub(super) fn mythos_phase_end(cx: &mut Cx) {
    // Pop the Mythos anchor (slice 1a, #393): the MythosAfterDraws window has
    // closed, so the anchor is the top frame. The transition below leaves the
    // Mythos phase, so the anchor's lifetime ends here.
    debug_assert!(
        matches!(
            cx.state.continuations.last(),
            Some(crate::state::Continuation::MythosPhase { .. })
        ),
        "mythos_phase_end: expected MythosPhase anchor on top, got {:?}",
        cx.state.continuations.last(),
    );
    cx.state.continuations.pop();
    // 1.5 Mythos phase ends.
    //     The PhaseEnded(Mythos) emit lives HERE rather than in
    //     step_phase so step 1.5 has explicit ownership in the
    //     driver — mirror of step 1.1's PhaseStarted ownership in
    //     mythos_phase. Rules Reference p.24: "This step formalizes
    //     the end of the mythos phase."
    // No forced-trigger dispatch here: only Enemy and Upkeep phase-ends have
    // slice consumers (agenda 01107). A `PhaseEnded { Mythos }` forced ability
    // would NOT fire until #212's emit_event restructure centralises forced
    // dispatch across all framework windows.
    cx.events.push(Event::PhaseEnded {
        phase: Phase::Mythos,
    });
    // Mythos → Investigation (slice 1b, #393): advance `state.phase` and push the
    // next phase's anchor at `Entry`. The main loop's `drive` advances it (runs
    // investigation_phase's opening). Replaces the former synchronous
    // `step_phase(cx)` call — the transition is now loop-driven.
    cx.state.phase = Phase::Investigation;
    cx.state
        .continuations
        .push(crate::state::Continuation::InvestigationPhase {
            resume: crate::state::InvestigationResume::Entry,
        });
}

/// Advance a freshly-entered phase anchor (slice 1b, #393): if the top frame is
/// a `*Phase` anchor at `Entry`, pop the placeholder and run that phase's
/// opening via its existing driver (which pushes the running anchor at its first
/// boundary resume + the phase's first child). Returns `None` when the top is
/// not an `Entry` anchor, so [`anchor_on_child_pop`] falls through to its
/// boundary dispatch.
fn advance_phase_entry(
    cx: &mut Cx,
    anchor: Option<&crate::state::Continuation>,
) -> Option<EngineOutcome> {
    use crate::state::{
        Continuation, EnemyResume, InvestigationResume, MythosResume, UpkeepResume,
    };
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
            investigation_phase(cx);
            Some(EngineOutcome::Done)
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

/// Run the top `*Phase` anchor's continuation after one of its framework
/// windows closed (slice 1a, #393), or advance it from `Entry` (slice 1b, via
/// [`advance_phase_entry`]). The window has already been popped by the close
/// path, so the anchor is now the top frame; its `resume` selects the relocated
/// body. Suspension-agnostic: a body that itself suspends returns
/// `AwaitingInput` unchanged. The `resume` is copied out before the body takes
/// `&mut cx`.
pub(super) fn anchor_on_child_pop(cx: &mut Cx) -> EngineOutcome {
    use crate::state::{
        Continuation, EnemyResume, InvestigationResume, MythosResume, UpkeepResume,
    };
    let anchor = cx.state.continuations.last().cloned();
    // `Entry` advances (slice 1b, #393) run the phase opening; delegated so this
    // function stays the boundary-dispatch it was in slice 1a.
    if let Some(out) = advance_phase_entry(cx, anchor.as_ref()) {
        return out;
    }
    match anchor {
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
        Some(Continuation::UpkeepPhase {
            resume: UpkeepResume::AfterRoundEnd,
        }) => {
            // The round-end `EmitEvent` coordinator (the `when` act advance + the
            // `at` doom) popped, re-exposing this anchor (#434). Run teardown
            // (expire until-end-of-round effects, Upkeep → Mythos).
            upkeep_round_end_teardown(cx)
        }
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
            let outcome = super::combat::resolve_attacks_for_investigator(cx, investigator);
            // The attack loop suspended on a mid-loop soak reaction window (C5b
            // #237): surface it WITHOUT advancing the cursor. The cursor advances
            // later, once the loop truly finishes, via resume_enemy_attack →
            // after_enemy_phase_attacks on window close.
            if matches!(outcome, EngineOutcome::AwaitingInput { .. }) {
                return outcome;
            }
            debug_assert!(
                matches!(outcome, EngineOutcome::Done),
                "resolve_attacks_for_investigator returned unexpected {outcome:?}",
            );
            super::reaction_windows::after_enemy_phase_attacks(cx, investigator)
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
        Some(Continuation::InvestigationPhase {
            resume: InvestigationResume::Begins,
        }) => {
            // Post-2.1 window closed; start the first investigator's turn
            // (step 2.2). No skill-test-in-flight guard: runs at phase start
            // (no test in flight) and does not transition phase.
            if let Some(id) = super::cursor::first_active_investigator(cx.state) {
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
            mythos_phase_end(cx);
            EngineOutcome::Done
        }
        other => {
            unreachable!("anchor_on_child_pop: top frame is not a known phase anchor: {other:?}")
        }
    }
}

/// Entered by [`step_phase`] on the Enemy→Upkeep transition. Owns the
/// `PhaseStarted(Upkeep)` emit (step 4.1) and opens the post-4.1 player
/// window. Steps 4.2 onward run as the window's continuation
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
    cx.state
        .continuations
        .push(crate::state::Continuation::UpkeepPhase {
            resume: crate::state::UpkeepResume::Begins,
        });
    // PLAYER WINDOW (post-4.1). Auto-skips inline (running upkeep_resume
    // via the anchor's on_child_pop) when nothing is Fast-eligible.
    super::reaction_windows::open_fast_window(cx, FastWindowKind::Phase(PhaseStep::UpkeepBegins))
}

/// The post-4.1 window continuation. Steps 4.2–4.4 run inline as named
/// call sites; step 4.5 ([`check_hand_size`]) may suspend with
/// [`EngineOutcome::AwaitingInput`] when an investigator is over the hand
/// cap — in which case `upkeep_resume` short-circuits and 4.6 runs only
/// once the discard resolves. Otherwise it hands to [`upkeep_phase_end`]
/// for 4.6 + transition.
pub(super) fn upkeep_resume(cx: &mut Cx) -> EngineOutcome {
    reset_actions(cx); // 4.2
    ready_exhausted_cards(cx); // 4.3
    upkeep_draw_and_resource(cx); // 4.4
    if let outcome @ EngineOutcome::AwaitingInput { .. } = check_hand_size(cx) {
        return outcome; // 4.5 parked for discard; 4.6 runs on resume
    }
    upkeep_phase_end(cx) // 4.6 + transition (may open the act round-end window)
}

/// Owns step 4.6's `PhaseEnded(Upkeep)` emit, then transitions to
/// Mythos. Exact analog of [`mythos_phase_end`]. `step_phase` emits no
/// `PhaseEnded` itself — every phase's `*_end` helper owns its own.
pub(crate) fn upkeep_phase_end(cx: &mut Cx) -> EngineOutcome {
    // 4.6 Upkeep phase ends. Round ends.
    cx.events.push(Event::PhaseEnded {
        phase: Phase::Upkeep,
    });
    // `PhaseEnded { Upkeep }` ("at end of phase") is single-bucket and fires
    // inline. No slice-1 card keys here, so it resolves synchronously; a
    // 2+/suspending hit is caught structurally by `emit_event`'s loud guard.
    let forced = super::emit::emit_event(
        cx,
        &super::emit::TimingEvent::PhaseEnded {
            phase: Phase::Upkeep,
        },
    );
    debug_assert!(
        matches!(forced, EngineOutcome::Done),
        "upkeep_phase_end PhaseEnded(Upkeep) forced did not resolve to Done: {forced:?}"
    );
    // "Round ends" (RR p.24). The `when the round ends` act advance (act 01109)
    // and the `at the end of the round` doom (agenda 01107, Dissonant Voices
    // 01165) resolve as the `RoundEnded` `EmitEvent` coordinator's `When`/`At`
    // cells (#434) — structural ordering, no hand-threading. Set this anchor's
    // resume so `upkeep_round_end_teardown` runs when the coordinator pops, then
    // cede: `emit_event(RoundEnded)` pushes the coordinator and returns `Done`;
    // the global loop drives the bucket walk (suspending at the `when` window)
    // and re-exposes this anchor at `AfterRoundEnd` on completion.
    set_upkeep_resume(cx, crate::state::UpkeepResume::AfterRoundEnd);
    super::emit::emit_event(cx, &super::emit::TimingEvent::RoundEnded)
}

/// Set the top [`UpkeepPhase`](crate::state::Continuation::UpkeepPhase) anchor's
/// resume cursor. The anchor is the top frame when `upkeep_phase_end` runs (the
/// round-end coordinator is pushed *after* this call).
fn set_upkeep_resume(cx: &mut Cx, resume: crate::state::UpkeepResume) {
    match cx.state.continuations.last_mut() {
        Some(crate::state::Continuation::UpkeepPhase { resume: slot }) => *slot = resume,
        other => {
            unreachable!("set_upkeep_resume: expected UpkeepPhase anchor on top, got {other:?}")
        }
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
            Some(crate::state::Continuation::UpkeepPhase { .. })
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
    cx.state
        .continuations
        .push(crate::state::Continuation::MythosPhase {
            resume: crate::state::MythosResume::Entry,
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
            super::hunters::reengage_at_location(cx, eid);
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
    super::cursor::active_investigators_in_turn_order(state)
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
    let next = remaining[0];
    cx.state
        .continuations
        .push(crate::state::Continuation::HandSizeDiscard(
            HandSizeDiscard { remaining },
        ));
    EngineOutcome::AwaitingInput {
        request: InputRequest::pick_multiple(format!(
            "Upkeep step 4.5: {next:?} has more than {HAND_SIZE_LIMIT} cards in hand; \
             submit InputResponse::PickMultiple with the hand indices (as option ids) to \
             discard down to {HAND_SIZE_LIMIT}.",
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
    let Some(crate::state::Continuation::HandSizeDiscard(pending)) = cx.state.continuations.last()
    else {
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
    let mut seen = std::collections::BTreeSet::new();
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
    for id in super::cursor::active_investigators_in_turn_order(cx.state) {
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
    let ids = super::cursor::active_investigators_in_turn_order(cx.state);
    for &id in &ids {
        super::cards::draw_one_with_deckout(cx, id);
    }
    for &id in &ids {
        super::cards::grant_resources(cx, id, 1);
    }
}

#[cfg(test)]
mod investigation_phase_tests {
    use super::*;
    use crate::action::PlayerAction;
    use crate::engine::dispatch::apply_player_action;
    use crate::engine::outcome::EngineOutcome;
    use crate::state::{InvestigatorId, Phase, Status};
    use crate::test_support::{test_investigator, GameStateBuilder};

    #[test]
    fn investigator_turn_defaults_to_not_ending() {
        use crate::state::Continuation;
        // The builder-staged open-turn frame is not mid-end-turn.
        let state = GameStateBuilder::default()
            .with_investigator(test_investigator(1))
            .with_phase(Phase::Investigation)
            .with_active_investigator(InvestigatorId(1))
            .with_turn_order([InvestigatorId(1)])
            .with_phase_anchor(Continuation::InvestigationPhase {
                resume: crate::state::InvestigationResume::TurnBegins,
            })
            .with_investigator_turn(InvestigatorId(1))
            .build();
        assert_eq!(
            state.continuations.last(),
            Some(&Continuation::InvestigatorTurn {
                investigator: InvestigatorId(1),
                ending: false,
            }),
        );
    }

    #[test]
    fn open_turn_leaves_investigator_turn_frame_on_top() {
        use crate::state::{Continuation, InvestigationResume};
        // Reach the open turn the way production does: enter the Investigation
        // phase for a single investigator (no Fast cards → windows auto-skip).
        let mut state = GameStateBuilder::default()
            .with_investigator(test_investigator(1))
            .with_phase(Phase::Investigation)
            .build();
        state.turn_order = vec![InvestigatorId(1)];

        let mut events = Vec::new();
        let outcome = {
            let mut cx = crate::engine::Cx {
                state: &mut state,
                events: &mut events,
            };
            // investigation_phase pushes the anchor + opens (auto-skips) both
            // windows, landing the first investigator's open turn.
            investigation_phase(&mut cx);
            super::super::drive(&mut cx, EngineOutcome::Done)
        };

        // The open turn surfaces its action menu as AwaitingInput (2b, #447).
        assert!(matches!(outcome, EngineOutcome::AwaitingInput { .. }));
        // Top frame is the InvestigatorTurn for investigator 1...
        assert_eq!(
            state.continuations.last(),
            Some(&Continuation::InvestigatorTurn {
                investigator: InvestigatorId(1),
                ending: false,
            }),
        );
        // ...sitting above the still-present InvestigationPhase anchor.
        assert!(state.continuations.iter().any(|c| matches!(
            c,
            Continuation::InvestigationPhase {
                resume: InvestigationResume::TurnBegins
            }
        )));
    }

    #[test]
    fn mulligan_completion_kicks_off_investigation_phase() {
        // After the last investigator mulligans, setup ends and the
        // Investigation phase begins (Rules Reference p.27: no action
        // windows during setup; the game begins after mulligans).
        // active_investigator defaults to None (set when the phase rotates).
        let mut state = GameStateBuilder::default()
            .with_investigator(test_investigator(1))
            .with_phase(Phase::Investigation)
            .with_turn_order([InvestigatorId(1)])
            .with_mulligan_remaining([InvestigatorId(1)])
            .build();

        let mut events = Vec::new();
        let outcome = apply_player_action(
            &mut crate::engine::Cx {
                state: &mut state,
                events: &mut events,
            },
            &PlayerAction::ResolveInput {
                response: crate::action::InputResponse::PickMultiple { selected: vec![] },
            },
        );

        assert!(matches!(outcome, EngineOutcome::AwaitingInput { .. }));
        assert_eq!(
            state.current_mulligan(),
            None,
            "mulligan loop drains once every investigator has mulliganed"
        );
        assert_eq!(
            state.active_investigator,
            Some(InvestigatorId(1)),
            "Investigation phase kicks off and rotates to the lead after mulligan completes"
        );
        // PhaseStarted(Investigation) fires at mulligan completion (not
        // during scenario setup).
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::PhaseStarted {
                    phase: Phase::Investigation
                }
            )),
            "PhaseStarted(Investigation) must fire"
        );
    }

    #[test]
    fn investigation_anchor_pushed_and_persists_through_turn() {
        // The Investigation driver pushes its anchor at entry; it persists
        // beneath the open-action turn (slice 1a) after the framework windows
        // auto-skip closed.
        let id = InvestigatorId(1);
        let mut state = GameStateBuilder::default()
            .with_investigator(test_investigator(1))
            .with_phase(Phase::Investigation)
            .build();
        state.turn_order = vec![id];
        let mut events = Vec::new();
        investigation_phase(&mut Cx {
            state: &mut state,
            events: &mut events,
        });
        assert!(
            state
                .continuations
                .iter()
                .any(|c| matches!(c, crate::state::Continuation::InvestigationPhase { .. })),
            "InvestigationPhase anchor present during the turn; stack = {:?}",
            state.continuations,
        );
    }

    #[test]
    fn investigation_phase_emits_phase_started_and_rotates_to_lead() {
        // Two investigators; investigation_phase should emit
        // PhaseStarted(Investigation), open the post-2.1 InvestigationBegins
        // window (which auto-skips in tests — no card registry installed),
        // and then rotate to the first investigator in turn_order
        // (Rules Reference p.24 step 2.1 → window → step 2.2 lead-first).
        let mut state = GameStateBuilder::default()
            .with_investigator(test_investigator(1))
            .with_investigator(test_investigator(2))
            .with_phase(Phase::Mythos)
            .build();
        state.turn_order = vec![InvestigatorId(1), InvestigatorId(2)];
        state.active_investigator = None;

        let mut events = Vec::new();
        investigation_phase(&mut Cx {
            state: &mut state,
            events: &mut events,
        });

        assert_eq!(
            state.active_investigator,
            Some(InvestigatorId(1)),
            "investigation_phase must rotate to the lead (first in turn_order)"
        );
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::PhaseStarted {
                    phase: Phase::Investigation
                }
            )),
            "PhaseStarted(Investigation) must be emitted"
        );
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, Event::ActionsRemainingChanged { .. })),
            "rotate no longer emits ActionsRemainingChanged (actions reset at Upkeep 4.2)"
        );
    }

    #[test]
    fn investigation_phase_with_empty_turn_order_parks() {
        // Degenerate (cannot occur in real gameplay): no investigators.
        // The InvestigationBegins continuation finds no active
        // investigator and PARKS — active stays None, no PhaseEnded, no
        // advance. Locks in the cascade-breaker behavior (see spec
        // "All-eliminated / no-active-investigator handling").
        //
        // Phase starts as Investigation (matching the real call-site
        // shape: step_phase sets state.phase before calling
        // investigation_phase).
        let mut state = GameStateBuilder::default()
            .with_phase(Phase::Investigation)
            .build();
        state.turn_order.clear();
        state.active_investigator = None;

        let mut events = Vec::new();
        investigation_phase(&mut Cx {
            state: &mut state,
            events: &mut events,
        });

        assert_eq!(
            state.active_investigator, None,
            "no investigator to rotate to"
        );
        assert_eq!(state.phase, Phase::Investigation, "phase must not advance");
        assert!(
            !events.iter().any(|e| matches!(
                e,
                Event::PhaseEnded {
                    phase: Phase::Investigation
                }
            )),
            "parking must NOT end the phase (auto-advancing would loop the round)"
        );
    }

    #[test]
    fn investigation_phase_skips_defeated_lead_and_picks_first_active() {
        // Investigator 1 (lead) is Killed; investigator 2 is Active.
        // investigation_phase must skip Id(1) and rotate to Id(2).
        let mut state = GameStateBuilder::default()
            .with_investigator(test_investigator(1))
            .with_investigator(test_investigator(2))
            .with_phase(Phase::Mythos)
            .build();
        state.turn_order = vec![InvestigatorId(1), InvestigatorId(2)];
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .status = Status::Killed;
        state.active_investigator = None;

        let mut events = Vec::new();
        investigation_phase(&mut Cx {
            state: &mut state,
            events: &mut events,
        });

        assert_eq!(
            state.active_investigator,
            Some(InvestigatorId(2)),
            "investigation_phase must skip the Killed lead and rotate to the first Active investigator"
        );
    }

    #[test]
    fn end_turn_for_last_investigator_ends_phase_and_steps_to_enemy() {
        // Single investigator ends their turn: TurnEnded (2.2.2), then
        // PhaseEnded(Investigation) (2.3) from investigation_phase_end,
        // then the cascade enters the Enemy phase.
        let mut state = GameStateBuilder::default()
            .with_investigator(test_investigator(1))
            .with_phase(Phase::Investigation)
            .with_active_investigator(InvestigatorId(1))
            .with_turn_order([InvestigatorId(1)])
            // Mid-Investigation invariant: the InvestigationPhase anchor (slice
            // 1a) + the open-turn frame (slice 2a-i) the driver leaves mid-turn.
            .with_phase_anchor(crate::state::Continuation::InvestigationPhase {
                resume: crate::state::InvestigationResume::TurnBegins,
            })
            .with_investigator_turn(InvestigatorId(1))
            .build();

        let mut events = Vec::new();
        let outcome = {
            // end_turn may push the next phase's Entry anchor (slice 1b); drive
            // completes the transition, as the apply boundary does in production.
            let mut cx = Cx {
                state: &mut state,
                events: &mut events,
            };
            let o = end_turn(&mut cx);
            super::super::drive(&mut cx, o)
        };

        // Single investigator with no enemies: the round-ending EndTurn
        // cascades Investigation → Enemy → Upkeep → Mythos and pauses at the
        // step-1.4 encounter-draw prompt (AwaitingInput).
        assert!(matches!(outcome, EngineOutcome::AwaitingInput { .. }));
        assert!(
            events.iter().any(|e| matches!(e, Event::TurnEnded { investigator } if *investigator == InvestigatorId(1))),
            "step 2.2.2 emits TurnEnded"
        );
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::PhaseEnded {
                    phase: Phase::Investigation
                }
            )),
            "step 2.3 emits PhaseEnded(Investigation) via investigation_phase_end"
        );
        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(
                    e,
                    Event::PhaseEnded {
                        phase: Phase::Investigation
                    }
                ))
                .count(),
            1,
            "exactly one PhaseEnded(Investigation) — step_phase must not also emit it"
        );
        assert_ne!(
            state.phase,
            Phase::Investigation,
            "phase advanced past Investigation"
        );
    }

    #[test]
    fn end_turn_rotates_to_next_active_and_opens_turn_window() {
        // Two investigators: ending #1's turn returns to 2.2 for #2 and
        // opens the InvestigatorTurnBegins window for them.
        let mut state = GameStateBuilder::default()
            .with_investigator(test_investigator(1))
            .with_investigator(test_investigator(2))
            .with_phase(Phase::Investigation)
            .with_active_investigator(InvestigatorId(1))
            .with_turn_order([InvestigatorId(1), InvestigatorId(2)])
            // Mid-Investigation invariant: the InvestigationPhase anchor (slice
            // 1a) + the open-turn frame (slice 2a-i) the driver leaves mid-turn.
            .with_phase_anchor(crate::state::Continuation::InvestigationPhase {
                resume: crate::state::InvestigationResume::TurnBegins,
            })
            .with_investigator_turn(InvestigatorId(1))
            .build();

        let mut events = Vec::new();
        let outcome = {
            // end_turn may push the next phase's Entry anchor (slice 1b); drive
            // completes the transition, as the apply boundary does in production.
            let mut cx = Cx {
                state: &mut state,
                events: &mut events,
            };
            let o = end_turn(&mut cx);
            super::super::drive(&mut cx, o)
        };

        assert!(matches!(outcome, EngineOutcome::AwaitingInput { .. }));
        assert_eq!(
            state.active_investigator,
            Some(InvestigatorId(2)),
            "rotates to the next active investigator (return to 2.2)"
        );
        assert_eq!(
            state.phase,
            Phase::Investigation,
            "phase does not end mid-round"
        );
        assert!(
            !events.iter().any(|e| matches!(
                e,
                Event::PhaseEnded {
                    phase: Phase::Investigation
                }
            )),
            "phase must not end while an investigator is still to take a turn"
        );
    }

    #[test]
    fn step_phase_emits_no_phase_ended() {
        // step_phase no longer emits PhaseEnded for any phase — each
        // phase's *_end helper owns it. Direct Investigation→Enemy step:
        // step_phase must NOT emit PhaseEnded(Investigation); the
        // downstream cascade may emit PhaseEnded for Enemy/Upkeep via
        // their own *_end helpers, but that's correct and expected.
        let mut state = GameStateBuilder::default()
            .with_investigator(test_investigator(1))
            .with_phase(Phase::Investigation)
            .build();
        state.turn_order = vec![InvestigatorId(1)];

        let mut events = Vec::new();
        step_phase(&mut Cx {
            state: &mut state,
            events: &mut events,
        });

        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(e, Event::PhaseEnded { phase: Phase::Investigation }))
                .count(),
            0,
            "step_phase must emit no PhaseEnded(Investigation) — investigation_phase_end owns it. events = {events:?}"
        );
    }

    #[test]
    fn investigation_entry_emits_phase_started_then_windows_then_lead_active() {
        // Round ≥2 entry via step_phase (Mythos→Investigation) auto-skips
        // both windows (no registry → nothing Fast-eligible) and lands
        // the lead active, with no PhaseEnded yet.
        let mut state = GameStateBuilder::default()
            .with_investigator(test_investigator(1))
            .with_phase(Phase::Mythos)
            .build();
        state.turn_order = vec![InvestigatorId(1)];
        state.active_investigator = None;

        let mut events = Vec::new();
        step_phase(&mut Cx {
            state: &mut state,
            events: &mut events,
        }); // Mythos→Investigation

        assert_eq!(state.phase, Phase::Investigation);
        assert_eq!(state.active_investigator, Some(InvestigatorId(1)));
        assert!(events.iter().any(|e| matches!(
            e,
            Event::PhaseStarted {
                phase: Phase::Investigation
            }
        )));
        assert!(!events.iter().any(|e| matches!(
            e,
            Event::PhaseEnded {
                phase: Phase::Investigation
            }
        )));
    }
}

#[cfg(test)]
mod mythos_phase_tests {
    use super::*;
    use crate::state::{InvestigatorId, Phase, Status};
    use crate::test_support::{test_investigator, GameStateBuilder};

    #[test]
    fn mythos_phase_emits_phase_started_and_prompts_first_drawer() {
        let mut state = GameStateBuilder::default()
            .with_investigator(test_investigator(1))
            .with_investigator(test_investigator(2))
            .with_phase(Phase::Mythos)
            .build();
        state.turn_order = vec![InvestigatorId(1), InvestigatorId(2)];
        let mut events = Vec::new();

        let outcome = mythos_phase(&mut Cx {
            state: &mut state,
            events: &mut events,
        });

        let EngineOutcome::AwaitingInput { request, .. } = &outcome else {
            panic!("mythos_phase opens the first encounter-draw prompt, got {outcome:?}");
        };
        // The draw is a binary acknowledge: kind Confirm, not skippable.
        assert_eq!(request.kind, crate::engine::InputKind::Confirm);
        assert!(!request.skippable);
        assert_eq!(state.current_encounter_drawer(), Some(InvestigatorId(1)));
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::PhaseStarted {
                    phase: Phase::Mythos
                }
            )),
            "must emit PhaseStarted(Mythos); events = {events:?}"
        );
    }

    #[test]
    fn mythos_drives_from_entry_via_the_loop() {
        // slice 1b: a MythosPhase{Entry} anchor advanced by `drive` runs the
        // phase opening (PhaseStarted + round bump + push the EncounterDraw
        // loop) and suspends at the first drawer prompt — same as the old
        // synchronous mythos_phase entry.
        let mut state = GameStateBuilder::default()
            .with_investigator(test_investigator(1))
            .with_phase(Phase::Mythos)
            .with_phase_anchor(crate::state::Continuation::MythosPhase {
                resume: crate::state::MythosResume::Entry,
            })
            .build();
        state.turn_order = vec![InvestigatorId(1)];
        let mut events = Vec::new();
        let outcome = super::super::drive(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            EngineOutcome::Done,
        );
        assert!(
            matches!(outcome, EngineOutcome::AwaitingInput { .. }),
            "drive advances the Entry anchor and suspends at the draw prompt; got {outcome:?}",
        );
        assert!(events.iter().any(|e| matches!(
            e,
            Event::PhaseStarted {
                phase: Phase::Mythos
            }
        )));
        assert!(state
            .continuations
            .iter()
            .any(|c| matches!(c, crate::state::Continuation::EncounterDraw { .. })));
    }

    #[test]
    fn mythos_anchor_pushed_during_phase() {
        // The Mythos driver pushes its anchor at entry; it sits beneath the
        // encounter-draw loop while the phase is suspended (slice 1a).
        let mut state = GameStateBuilder::default()
            .with_investigator(test_investigator(1))
            .with_phase(Phase::Mythos)
            .build();
        state.turn_order = vec![InvestigatorId(1)];
        let mut events = Vec::new();
        let outcome = mythos_phase(&mut Cx {
            state: &mut state,
            events: &mut events,
        });
        assert!(matches!(outcome, EngineOutcome::AwaitingInput { .. }));
        assert!(
            state
                .continuations
                .iter()
                .any(|c| matches!(c, crate::state::Continuation::MythosPhase { .. })),
            "MythosPhase anchor on the stack during the phase; stack = {:?}",
            state.continuations,
        );
    }

    #[test]
    fn mythos_phase_with_empty_turn_order_opens_after_draws_window_inline() {
        let mut state = GameStateBuilder::default()
            .with_phase(Phase::Mythos)
            .build();
        state.turn_order.clear();
        let mut events = Vec::new();

        // No drawers → MythosAfterDraws auto-skips, mythos_phase_end pushes the
        // Investigation anchor; `drive` then advances it (slice 1b) — completing
        // the Mythos→Investigation transition that was synchronous in slice 1a.
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };
        let outcome = mythos_phase(&mut cx);
        let outcome = super::super::drive(&mut cx, outcome);

        assert_eq!(outcome, EngineOutcome::Done);
        assert_eq!(state.current_encounter_drawer(), None);
        assert_eq!(state.phase, Phase::Investigation);
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::PhaseEnded {
                    phase: Phase::Mythos
                }
            )),
            "must emit PhaseEnded(Mythos); events = {events:?}"
        );
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::PhaseStarted {
                    phase: Phase::Investigation
                }
            )),
            "must emit PhaseStarted(Investigation); events = {events:?}"
        );
    }

    #[test]
    fn mythos_phase_end_emits_phase_ended_and_steps_to_investigation() {
        // mythos_phase_end now runs only with the MythosPhase anchor on top
        // (slice 1a) — it pops the anchor as its first act.
        let mut state = GameStateBuilder::default()
            .with_investigator(test_investigator(1))
            .with_phase(Phase::Mythos)
            .with_turn_order([InvestigatorId(1)])
            .with_phase_anchor(crate::state::Continuation::MythosPhase {
                resume: crate::state::MythosResume::AfterDraws,
            })
            .build();
        let mut events = Vec::new();

        // mythos_phase_end pops the Mythos anchor + pushes the Investigation
        // anchor (Entry); `drive` advances it to run investigation_phase (slice
        // 1b) — completing the transition.
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };
        mythos_phase_end(&mut cx);
        let _ = super::super::drive(&mut cx, EngineOutcome::Done);

        assert!(
            !state
                .continuations
                .iter()
                .any(|c| matches!(c, crate::state::Continuation::MythosPhase { .. })),
            "mythos_phase_end pops the Mythos anchor (the cascade into \
             Investigation then pushes its own anchor)",
        );
        assert_eq!(state.phase, Phase::Investigation);
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::PhaseEnded {
                    phase: Phase::Mythos
                }
            )),
            "must emit PhaseEnded(Mythos); events = {events:?}"
        );
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::PhaseStarted {
                    phase: Phase::Investigation
                }
            )),
            "must emit PhaseStarted(Investigation); events = {events:?}"
        );
    }

    /// Site 1 fix (Rules Reference p.10): when the lead investigator in
    /// `turn_order` is eliminated, `mythos_phase` must seed the encounter-draw
    /// queue from the first Active investigator rather than blindly taking
    /// `turn_order.first()`.
    #[test]
    fn mythos_phase_skips_eliminated_lead_when_seeding_queue() {
        let mut state = GameStateBuilder::default()
            .with_investigator(test_investigator(1))
            .with_investigator(test_investigator(2))
            .with_phase(Phase::Mythos)
            .build();
        state.turn_order = vec![InvestigatorId(1), InvestigatorId(2)];
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .status = Status::Killed;
        let mut events = Vec::new();

        mythos_phase(&mut Cx {
            state: &mut state,
            events: &mut events,
        });

        assert_eq!(
            state.current_encounter_drawer(),
            Some(InvestigatorId(2)),
            "the queue must prompt the first Active investigator, not the Killed lead"
        );
    }

    /// All investigators in `turn_order` are eliminated. `mythos_phase`
    /// must treat this the same as an empty `turn_order`: seed to None
    /// and open `MythosAfterDraws` inline, which auto-skips and drives
    /// `mythos_phase_end`, transitioning to Investigation.
    ///
    /// This is the non-empty-`turn_order` analogue of
    /// `mythos_phase_with_empty_turn_order_opens_after_draws_window_inline`.
    #[test]
    fn mythos_phase_with_all_investigators_eliminated_opens_after_draws_window() {
        let mut state = GameStateBuilder::default()
            .with_investigator(test_investigator(1))
            .with_phase(Phase::Mythos)
            .build();
        state.turn_order = vec![InvestigatorId(1)];
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .status = Status::Killed;
        let mut events = Vec::new();

        mythos_phase(&mut Cx {
            state: &mut state,
            events: &mut events,
        });

        assert_eq!(state.current_encounter_drawer(), None);
        assert_eq!(
            state.phase,
            Phase::Investigation,
            "no Active drawers → MythosAfterDraws fires inline → Investigation"
        );
    }

    /// Site 2 fix (Rules Reference p.10): when advancing the encounter-draw
    /// queue after a completed draw, eliminated investigators in the middle of
    /// the queue must be skipped. Here inv2 is Killed; the queue must advance
    /// from inv1 to inv3.
    #[test]
    fn advance_encounter_draw_skips_eliminated_middle_investigator() {
        let mut state = GameStateBuilder::default()
            .with_investigator(test_investigator(1))
            .with_investigator(test_investigator(2))
            .with_investigator(test_investigator(3))
            .with_phase(Phase::Mythos)
            .with_turn_order([InvestigatorId(1), InvestigatorId(2), InvestigatorId(3)])
            .with_mythos_draw_remaining([InvestigatorId(1), InvestigatorId(2), InvestigatorId(3)])
            .build();
        state
            .investigators
            .get_mut(&InvestigatorId(2))
            .unwrap()
            .status = Status::Killed;
        let mut events = Vec::new();

        // inv1 has just completed their draw chain: advance drops inv1 and must
        // skip the Killed inv2, landing on inv3.
        let outcome = super::super::encounter::advance_encounter_draw(&mut super::super::Cx {
            state: &mut state,
            events: &mut events,
        });

        assert!(
            matches!(outcome, EngineOutcome::AwaitingInput { .. }),
            "a remaining drawer re-prompts, got {outcome:?}"
        );
        assert_eq!(
            state.current_encounter_drawer(),
            Some(InvestigatorId(3)),
            "the queue must skip the Killed inv2 and land on Active inv3"
        );
    }

    #[test]
    fn first_active_investigator_finds_first_active_skipping_eliminated() {
        let mut state = GameStateBuilder::default()
            .with_investigator(test_investigator(1))
            .with_investigator(test_investigator(2))
            .with_investigator(test_investigator(3))
            .build();
        state.turn_order = vec![InvestigatorId(1), InvestigatorId(2), InvestigatorId(3)];
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .status = Status::Killed;
        state
            .investigators
            .get_mut(&InvestigatorId(2))
            .unwrap()
            .status = Status::Insane;

        assert_eq!(
            super::super::cursor::first_active_investigator(&state),
            Some(InvestigatorId(3)),
            "first Active in turn_order after skipping eliminated"
        );
    }

    #[test]
    fn first_active_investigator_returns_none_when_all_eliminated() {
        let mut state = GameStateBuilder::default()
            .with_investigator(test_investigator(1))
            .build();
        state.turn_order = vec![InvestigatorId(1)];
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .status = Status::Killed;

        assert_eq!(
            super::super::cursor::first_active_investigator(&state),
            None
        );
    }

    #[test]
    fn first_active_investigator_returns_none_when_turn_order_empty() {
        let state = GameStateBuilder::default().build();
        assert_eq!(
            super::super::cursor::first_active_investigator(&state),
            None
        );
    }

    #[test]
    fn next_active_investigator_after_skips_eliminated_middle() {
        let mut state = GameStateBuilder::default()
            .with_investigator(test_investigator(1))
            .with_investigator(test_investigator(2))
            .with_investigator(test_investigator(3))
            .with_investigator(test_investigator(4))
            .build();
        state.turn_order = vec![
            InvestigatorId(1),
            InvestigatorId(2),
            InvestigatorId(3),
            InvestigatorId(4),
        ];
        state
            .investigators
            .get_mut(&InvestigatorId(2))
            .unwrap()
            .status = Status::Killed;

        assert_eq!(
            super::super::cursor::next_active_investigator_after(&state, InvestigatorId(1)),
            Some(InvestigatorId(3)),
            "advance from 1 skips Killed 2, lands on 3"
        );
        assert_eq!(
            super::super::cursor::next_active_investigator_after(&state, InvestigatorId(3)),
            Some(InvestigatorId(4)),
            "advance from 3 lands on 4"
        );
        assert_eq!(
            super::super::cursor::next_active_investigator_after(&state, InvestigatorId(4)),
            None,
            "advance past the last entry returns None"
        );
    }

    #[test]
    fn next_active_investigator_after_returns_none_when_current_not_in_turn_order() {
        let mut state = GameStateBuilder::default()
            .with_investigator(test_investigator(1))
            .build();
        state.turn_order = vec![InvestigatorId(1)];

        assert_eq!(
            super::super::cursor::next_active_investigator_after(&state, InvestigatorId(99)),
            None
        );
    }

    #[test]
    fn next_active_investigator_after_works_when_current_is_non_active() {
        // Defeated-mid-loop semantics: `current` may be Killed by the
        // time we advance from them. The cursor still finds the right
        // successor.
        let mut state = GameStateBuilder::default()
            .with_investigator(test_investigator(1))
            .with_investigator(test_investigator(2))
            .build();
        state.turn_order = vec![InvestigatorId(1), InvestigatorId(2)];
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .status = Status::Killed;

        assert_eq!(
            super::super::cursor::next_active_investigator_after(&state, InvestigatorId(1)),
            Some(InvestigatorId(2)),
            "current=1 is non-Active but turn_order still anchors the index"
        );
    }
}

#[cfg(test)]
mod upkeep_phase_tests {
    use super::*;
    use crate::engine::enumerate::TurnAction;
    use crate::engine::EngineOutcome;
    use crate::event::Event;
    use crate::state::{
        CardCode, CardInPlay, CardInstanceId, EnemyId, InvestigatorId, LocationId, Phase, Status,
    };
    use crate::test_support::{
        take_turn_action, test_enemy, test_investigator, test_location, GameStateBuilder,
    };
    use crate::{assert_event, assert_event_sequence, assert_no_event};

    #[test]
    fn upkeep_phase_emits_phase_started_and_auto_skips_to_mythos() {
        // No Fast-eligible cards / no reactions installed → the post-4.1
        // window auto-skips inline, the continuation runs, and the
        // cascade lands in Mythos.
        let id = InvestigatorId(1);
        let mut state = GameStateBuilder::default()
            .with_investigator(test_investigator(1))
            .with_phase(Phase::Enemy)
            .build();
        state.turn_order = vec![id];
        state.active_investigator = None;

        let mut events = Vec::new();
        step_phase(&mut Cx {
            state: &mut state,
            events: &mut events,
        }); // Enemy → Upkeep, cascades to Mythos

        let pos = |pred: &dyn Fn(&Event) -> bool| events.iter().position(pred);
        let started = pos(&|e| {
            matches!(
                e,
                Event::PhaseStarted {
                    phase: Phase::Upkeep
                }
            )
        })
        .expect("PhaseStarted(Upkeep)");
        let ended = pos(&|e| {
            matches!(
                e,
                Event::PhaseEnded {
                    phase: Phase::Upkeep
                }
            )
        })
        .expect("PhaseEnded(Upkeep)");
        let mythos = pos(&|e| {
            matches!(
                e,
                Event::PhaseStarted {
                    phase: Phase::Mythos
                }
            )
        })
        .expect("PhaseStarted(Mythos)");
        assert!(
            started < ended && ended < mythos,
            "upkeep sub-step events must be ordered 4.1 → 4.6 → Mythos 1.1; \
             events = {events:?}"
        );
        assert_eq!(state.phase, Phase::Mythos, "cascade lands in Mythos");
        assert!(
            state.open_windows().is_empty(),
            "UpkeepBegins must not persist on the stack"
        );
    }

    #[test]
    fn ready_exhausted_cards_readies_investigator_cards_and_enemies() {
        let inv_id = InvestigatorId(1);
        let enemy_id = EnemyId(1);
        let mut inv = test_investigator(1);
        let mut card = CardInPlay::enter_play(CardCode("01000".into()), CardInstanceId(1));
        card.exhausted = true;
        inv.cards_in_play = vec![card];
        let mut enemy = test_enemy(1, "Test Enemy");
        enemy.exhausted = true;
        let mut state = GameStateBuilder::default()
            .with_investigator(inv)
            .with_enemy(enemy)
            .build();
        let mut events = Vec::new();

        ready_exhausted_cards(&mut Cx {
            state: &mut state,
            events: &mut events,
        });

        assert!(
            !state.investigators[&inv_id].cards_in_play[0].exhausted,
            "card readied"
        );
        assert!(!state.enemies[&enemy_id].exhausted, "enemy readied");
        assert!(events.iter().any(|e| matches!(
            e, Event::CardReadied { investigator, instance_id, .. }
            if *investigator == inv_id && *instance_id == CardInstanceId(1))));
        assert!(events.iter().any(|e| matches!(
            e, Event::EnemyReadied { enemy } if *enemy == enemy_id)));
    }

    #[test]
    fn ready_exhausted_cards_reengages_co_located_unengaged_enemy() {
        let inv_id = InvestigatorId(1);
        let enemy_id = EnemyId(1);
        let loc = test_location(10, "Synth Loc");
        let mut enemy = test_enemy(1, "Test Enemy");
        enemy.exhausted = true; // exhausted + disengaged, e.g. survived a successful Evade
        enemy.current_location = Some(LocationId(10));
        let mut state = GameStateBuilder::default()
            .with_investigator_at(test_investigator(1), LocationId(10))
            .with_location(loc)
            .with_enemy(enemy)
            .with_turn_order([inv_id])
            .build();
        let mut events = Vec::new();

        ready_exhausted_cards(&mut Cx {
            state: &mut state,
            events: &mut events,
        });

        assert!(!state.enemies[&enemy_id].exhausted, "enemy readied");
        assert_eq!(
            state.enemies[&enemy_id].engaged_with,
            Some(inv_id),
            "readied enemy re-engages the co-located investigator (RR p.10)"
        );
        assert_event!(events, Event::EnemyReadied { enemy } if *enemy == enemy_id);
        assert_event!(events, Event::EnemyEngaged { investigator, .. } if *investigator == inv_id);
        assert_event_sequence!(
            events,
            Event::EnemyReadied { .. },
            Event::EnemyEngaged { .. },
        );
    }

    #[test]
    fn ready_exhausted_cards_leaves_ready_cards_untouched() {
        let mut enemy = test_enemy(1, "Test Enemy");
        enemy.exhausted = false; // already ready
        let mut state = GameStateBuilder::default()
            .with_investigator(test_investigator(1))
            .with_enemy(enemy)
            .build();
        let mut events = Vec::new();

        ready_exhausted_cards(&mut Cx {
            state: &mut state,
            events: &mut events,
        });

        assert!(
            events.is_empty(),
            "no readying events for already-ready cards"
        );
    }

    #[test]
    fn ready_exhausted_cards_no_engage_when_no_co_located_investigator() {
        let enemy_id = EnemyId(1);
        let inv_id = InvestigatorId(1);
        let loc = test_location(10, "Synth Loc");
        let mut enemy = test_enemy(1, "Test Enemy");
        enemy.exhausted = true;
        enemy.current_location = Some(LocationId(10));
        let mut state = GameStateBuilder::default()
            .with_investigator(test_investigator(1)) // current_location stays None — NOT co-located
            .with_location(loc)
            .with_enemy(enemy)
            .with_turn_order([inv_id])
            .build();
        let mut events = Vec::new();

        ready_exhausted_cards(&mut Cx {
            state: &mut state,
            events: &mut events,
        });

        assert!(!state.enemies[&enemy_id].exhausted, "enemy readied");
        assert_eq!(
            state.enemies[&enemy_id].engaged_with, None,
            "no investigator at the enemy's location → no engagement"
        );
        assert_no_event!(events, Event::EnemyEngaged { .. });
    }

    #[test]
    fn ready_exhausted_cards_keeps_existing_engagement_no_duplicate() {
        let enemy_id = EnemyId(1);
        let inv_id = InvestigatorId(1);
        let mut enemy = test_enemy(1, "Test Enemy");
        enemy.exhausted = true; // exhausted but still engaged (e.g. attacked last Enemy phase)
        enemy.engaged_with = Some(inv_id);
        let mut state = GameStateBuilder::default()
            .with_investigator(test_investigator(1))
            .with_enemy(enemy)
            .build();
        let mut events = Vec::new();

        ready_exhausted_cards(&mut Cx {
            state: &mut state,
            events: &mut events,
        });

        assert!(!state.enemies[&enemy_id].exhausted, "enemy readied");
        assert_eq!(
            state.enemies[&enemy_id].engaged_with,
            Some(inv_id),
            "an already-engaged enemy keeps its engagement"
        );
        assert_no_event!(events, Event::EnemyEngaged { .. });
    }

    #[test]
    fn upkeep_draw_and_resource_draws_and_grants_per_active_investigator() {
        let (a, b, c) = (InvestigatorId(1), InvestigatorId(2), InvestigatorId(3));
        let mut inv_a = test_investigator(1);
        inv_a.deck = vec![CardCode::new("01000")];
        let mut inv_b = test_investigator(2);
        inv_b.deck = vec![CardCode::new("01001")];
        let mut inv_c = test_investigator(3);
        inv_c.status = Status::Resigned; // eliminated → skipped
        inv_c.deck = vec![CardCode::new("01002")];
        let res_a = inv_a.resources;
        let res_b = inv_b.resources;
        let res_c = inv_c.resources;
        let hand_a = inv_a.hand.len();
        let mut state = GameStateBuilder::default()
            .with_investigator(inv_a)
            .with_investigator(inv_b)
            .with_investigator(inv_c)
            .build();
        state.turn_order = vec![a, b, c];
        let mut events = Vec::new();

        upkeep_draw_and_resource(&mut Cx {
            state: &mut state,
            events: &mut events,
        });

        assert_eq!(state.investigators[&a].resources, res_a + 1);
        assert_eq!(state.investigators[&b].resources, res_b + 1);
        assert_eq!(
            state.investigators[&c].resources, res_c,
            "eliminated investigator skipped"
        );
        assert_eq!(state.investigators[&a].hand.len(), hand_a + 1);
        assert_eq!(
            state.investigators[&c].deck.len(),
            1,
            "eliminated investigator did not draw"
        );
    }

    #[test]
    fn upkeep_draw_and_resource_two_pass_ordering() {
        // All CardsDrawn events precede all ResourcesGained events.
        let (a, b) = (InvestigatorId(1), InvestigatorId(2));
        let mut inv_a = test_investigator(1);
        inv_a.deck = vec![CardCode::new("01000")];
        let mut inv_b = test_investigator(2);
        inv_b.deck = vec![CardCode::new("01001")];
        let mut state = GameStateBuilder::default()
            .with_investigator(inv_a)
            .with_investigator(inv_b)
            .build();
        state.turn_order = vec![a, b];
        let mut events = Vec::new();

        upkeep_draw_and_resource(&mut Cx {
            state: &mut state,
            events: &mut events,
        });

        let last_draw = events
            .iter()
            .rposition(|e| matches!(e, Event::CardsDrawn { .. }))
            .expect("draws");
        let first_gain = events
            .iter()
            .position(|e| matches!(e, Event::ResourcesGained { .. }))
            .expect("gains");
        assert!(
            last_draw < first_gain,
            "all draws must precede all resource gains"
        );
    }

    #[test]
    fn reset_actions_sets_active_to_per_turn_and_skips_eliminated() {
        let (a, b) = (InvestigatorId(1), InvestigatorId(2));
        let mut inv_a = test_investigator(1);
        inv_a.actions_remaining = 0;
        let mut inv_b = test_investigator(2);
        inv_b.actions_remaining = 0;
        inv_b.status = Status::Killed;
        let mut state = GameStateBuilder::default()
            .with_investigator(inv_a)
            .with_investigator(inv_b)
            .build();
        state.turn_order = vec![a, b];
        let mut events = Vec::new();

        reset_actions(&mut Cx {
            state: &mut state,
            events: &mut events,
        });

        assert_eq!(state.investigators[&a].actions_remaining, ACTIONS_PER_TURN);
        assert_eq!(
            state.investigators[&b].actions_remaining, 0,
            "eliminated skipped"
        );
        assert!(events.iter().any(|e| matches!(
            e, Event::ActionsRemainingChanged { investigator, new_count }
            if *investigator == a && *new_count == ACTIONS_PER_TURN)));
        assert!(!events.iter().any(|e| matches!(
            e, Event::ActionsRemainingChanged { investigator, .. } if *investigator == b)));
    }

    #[test]
    fn reset_actions_emits_nothing_for_already_full() {
        // Emit-on-change semantics: when actions_remaining already equals
        // ACTIONS_PER_TURN, reset_actions makes no state change and emits
        // no ActionsRemainingChanged event.
        let id = InvestigatorId(1);
        let mut inv = test_investigator(1);
        inv.actions_remaining = ACTIONS_PER_TURN;
        let mut state = GameStateBuilder::default().with_investigator(inv).build();
        state.turn_order = vec![id];
        let mut events = Vec::new();

        reset_actions(&mut Cx {
            state: &mut state,
            events: &mut events,
        });

        assert_eq!(state.investigators[&id].actions_remaining, ACTIONS_PER_TURN);
        assert!(events.is_empty(), "no event when value is unchanged");
    }

    #[test]
    fn rotate_to_active_does_not_refresh_actions() {
        let id = InvestigatorId(1);
        let mut inv = test_investigator(1);
        inv.actions_remaining = 1;
        let mut state = GameStateBuilder::default().with_investigator(inv).build();
        let mut events = Vec::new();

        rotate_to_active(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            id,
        );

        assert_eq!(state.active_investigator, Some(id));
        assert_eq!(
            state.investigators[&id].actions_remaining, 1,
            "rotate must not refresh actions"
        );
        assert!(
            events.is_empty(),
            "rotate no longer emits ActionsRemainingChanged"
        );
    }

    #[test]
    fn round_increments_on_mythos_entry_via_driver() {
        // After the Upkeep→Mythos cascade, state.round bumps by 1.
        // The bump now lives in mythos_phase step 1.1 (this task);
        // the test asserts observable behavior, which is unchanged.
        let id = InvestigatorId(1);
        let mut state = GameStateBuilder::default()
            .with_investigator(test_investigator(1))
            .with_phase(Phase::Upkeep)
            .build();
        state.turn_order = vec![id];
        state.active_investigator = None;
        state.round = 4;

        let mut events = Vec::new();
        step_phase(&mut Cx {
            state: &mut state,
            events: &mut events,
        }); // Upkeep → ... → Mythos via the cascade

        assert_eq!(state.round, 5, "round bumps on Mythos entry");
        assert_eq!(state.phase, Phase::Mythos);
    }

    #[test]
    fn end_turn_cascades_through_upkeep_to_mythos_draw_prompt() {
        // Single investigator, non-empty deck, an exhausted in-play card.
        // After EndTurn: card readied, hand +1, resources +1, landed in
        // Mythos paused at the encounter-draw prompt and round bumped.
        let id = InvestigatorId(1);
        let mut inv = test_investigator(1);
        inv.actions_remaining = 0;
        inv.deck = vec![CardCode::new("01000"), CardCode::new("01001")];
        let mut card = CardInPlay::enter_play(CardCode::new("01002"), CardInstanceId(1));
        card.exhausted = true;
        inv.cards_in_play = vec![card];
        let res_before = inv.resources;
        let hand_before = inv.hand.len();
        let state = GameStateBuilder::default()
            .with_investigator(inv)
            .with_phase(Phase::Investigation)
            .with_turn_order([id])
            .with_active_investigator(id)
            .with_round(1)
            // Mid-Investigation invariant: the InvestigationPhase anchor (slice
            // 1a) + the open-turn frame (slice 2a-i) the driver leaves mid-turn.
            .with_phase_anchor(crate::state::Continuation::InvestigationPhase {
                resume: crate::state::InvestigationResume::TurnBegins,
            })
            .with_investigator_turn(id)
            .build();

        let result = take_turn_action(state, &TurnAction::EndTurn);

        // The round-ending EndTurn cascades into Mythos and pauses at the
        // step-1.4 encounter-draw prompt (AwaitingInput).
        assert!(
            matches!(result.outcome, EngineOutcome::AwaitingInput { .. }),
            "round-ending EndTurn pauses at the Mythos draw prompt, got {:?}",
            result.outcome
        );
        assert_eq!(result.state.phase, Phase::Mythos);
        assert_eq!(result.state.round, 2, "round bumped on Mythos entry");
        assert_eq!(result.state.current_encounter_drawer(), Some(id));
        assert_eq!(result.state.active_investigator, None);
        assert!(
            !result.state.investigators[&id].cards_in_play[0].exhausted,
            "readied"
        );
        assert_eq!(
            result.state.investigators[&id].resources,
            res_before + 1,
            "gained 1"
        );
        assert_eq!(
            result.state.investigators[&id].hand.len(),
            hand_before + 1,
            "drew 1"
        );
    }
}

#[cfg(test)]
mod enemy_phase_tests {
    use super::*;
    use crate::action::{Action, InputResponse, PlayerAction};
    use crate::assert_event;
    use crate::engine::dispatch::resolve_input;
    use crate::engine::{apply, EngineOutcome};
    use crate::state::{EnemyId, FastActorScope, InvestigatorId, LocationId, Phase, Status};
    use crate::test_support::{test_enemy, test_investigator, test_location, GameStateBuilder};

    #[test]
    fn enemy_phase_runs_hunters_then_attack_loop_when_no_tie() {
        let mut loc_a = test_location(1, "A");
        let mut loc_b = test_location(2, "B");
        loc_a.connections = vec![LocationId(2)];
        loc_b.connections = vec![LocationId(1)];
        let mut inv = test_investigator(1);
        inv.current_location = Some(LocationId(2));
        let mut hunter = test_enemy(1, "Hunter");
        hunter.hunter = true;
        hunter.current_location = Some(LocationId(1));
        let mut state = GameStateBuilder::new()
            .with_phase(Phase::Investigation)
            .with_location(loc_a)
            .with_location(loc_b)
            .with_investigator(inv)
            .with_active_investigator(InvestigatorId(1))
            .with_turn_order([InvestigatorId(1)])
            .with_enemy(hunter)
            // Mid-Investigation invariant: the InvestigationPhase anchor (slice
            // 1a) + the open-turn frame (slice 2a-i) the driver leaves mid-turn.
            // These tests construct the state directly (bypassing
            // investigation_phase), so stage both explicitly.
            .with_phase_anchor(crate::state::Continuation::InvestigationPhase {
                resume: crate::state::InvestigationResume::TurnBegins,
            })
            .with_investigator_turn(InvestigatorId(1))
            .build();
        let mut events = Vec::new();
        let outcome = {
            // end_turn may push the next phase's Entry anchor (slice 1b); drive
            // completes the transition, as the apply boundary does in production.
            let mut cx = Cx {
                state: &mut state,
                events: &mut events,
            };
            let o = end_turn(&mut cx);
            super::super::drive(&mut cx, o)
        };
        // No registry installed → the attack window auto-skips inline and the
        // cascade runs Enemy→Upkeep→Mythos within this same call, pausing at the
        // step-1.4 encounter-draw prompt (AwaitingInput). The hunter still moved
        // + engaged during step 3.2 — asserted via the event stream below.
        assert!(matches!(outcome, EngineOutcome::AwaitingInput { .. }));
        assert_eq!(state.phase, Phase::Mythos);
        assert_eq!(
            state.enemies[&EnemyId(1)].current_location,
            Some(LocationId(2))
        );
        assert_event!(events, Event::EnemyEngaged { enemy, .. } if *enemy == EnemyId(1));
    }

    #[test]
    fn enemy_phase_suspends_on_hunter_tie_then_resumes_into_attack_loop() {
        let mut loc_a = test_location(1, "A");
        let mut loc_b = test_location(2, "B");
        let mut loc_c = test_location(3, "C");
        let mut loc_d = test_location(4, "D");
        loc_a.connections = vec![LocationId(2), LocationId(3)];
        loc_b.connections = vec![LocationId(1), LocationId(4)];
        loc_c.connections = vec![LocationId(1), LocationId(4)];
        loc_d.connections = vec![LocationId(2), LocationId(3)];
        let mut inv = test_investigator(1);
        inv.current_location = Some(LocationId(4));
        let mut hunter = test_enemy(1, "Hunter");
        hunter.hunter = true;
        hunter.current_location = Some(LocationId(1));
        let mut state = GameStateBuilder::new()
            .with_phase(Phase::Investigation)
            .with_location(loc_a)
            .with_location(loc_b)
            .with_location(loc_c)
            .with_location(loc_d)
            .with_investigator(inv)
            .with_active_investigator(InvestigatorId(1))
            .with_turn_order([InvestigatorId(1)])
            .with_enemy(hunter)
            // Mid-Investigation invariant: the InvestigationPhase anchor (slice
            // 1a) + the open-turn frame (slice 2a-i) the driver leaves mid-turn.
            // These tests construct the state directly (bypassing
            // investigation_phase), so stage both explicitly.
            .with_phase_anchor(crate::state::Continuation::InvestigationPhase {
                resume: crate::state::InvestigationResume::TurnBegins,
            })
            .with_investigator_turn(InvestigatorId(1))
            .build();
        let mut events = Vec::new();
        let outcome = {
            // end_turn may push the next phase's Entry anchor (slice 1b); drive
            // completes the transition, as the apply boundary does in production.
            let mut cx = Cx {
                state: &mut state,
                events: &mut events,
            };
            let o = end_turn(&mut cx);
            super::super::drive(&mut cx, o)
        };
        assert!(matches!(outcome, EngineOutcome::AwaitingInput { .. }));
        assert_eq!(state.phase, Phase::Enemy);
        // The EnemyPhase anchor (slice 1a) is on the stack beneath the
        // suspended hunter-movement choice.
        assert!(
            state
                .continuations
                .iter()
                .any(|c| matches!(c, crate::state::Continuation::EnemyPhase { .. })),
            "EnemyPhase anchor present while suspended in the Enemy phase; stack = {:?}",
            state.continuations,
        );
        let mut ev2 = Vec::new();
        // Pick LocationId(2) by its offered option id (candidates ride the request).
        let crate::engine::EngineOutcome::AwaitingInput { request, .. } = &outcome else {
            unreachable!("asserted AwaitingInput above");
        };
        let pick = request
            .options
            .iter()
            .find(|o| o.label == format!("{:?}", LocationId(2)))
            .expect("LocationId(2) among offered options")
            .id;
        let resumed = {
            let mut cx = crate::engine::Cx {
                state: &mut state,
                events: &mut ev2,
            };
            let o = resolve_input(&mut cx, &InputResponse::PickSingle(pick));
            super::super::drive(&mut cx, o) // slice 1b: complete the cascade
        };
        // With no registry the attack window auto-skips and the cascade runs
        // Enemy->Upkeep->Mythos within the same resume call, pausing at the
        // step-1.4 encounter-draw prompt (AwaitingInput).
        assert!(matches!(resumed, EngineOutcome::AwaitingInput { .. }));
        assert_eq!(state.phase, Phase::Mythos);
    }

    #[test]
    fn resolve_attacks_for_investigator_fires_engaged_ready_enemy_and_exhausts() {
        let inv_id = InvestigatorId(1);
        let enemy_id = EnemyId(1);
        let mut enemy = test_enemy(1, "Test Enemy");
        enemy.engaged_with = Some(inv_id);
        enemy.attack_damage = 1;
        enemy.attack_horror = 0;
        let mut state = GameStateBuilder::default()
            .with_investigator(test_investigator(1))
            .with_enemy(enemy)
            .build();
        let mut events = Vec::new();

        super::super::combat::resolve_attacks_for_investigator(
            &mut super::super::Cx {
                state: &mut state,
                events: &mut events,
            },
            inv_id,
        );

        // Damage placed.
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::DamageTaken { investigator, amount: 1 } if *investigator == inv_id
            )),
            "expected DamageTaken {{ amount: 1 }}; events = {events:?}"
        );

        // Enemy exhausted in state and event.
        assert!(
            state.enemies[&enemy_id].exhausted,
            "enemy must be exhausted"
        );
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::EnemyExhausted { enemy } if *enemy == enemy_id
            )),
            "expected EnemyExhausted; events = {events:?}"
        );

        // Ordering: DamageTaken precedes EnemyExhausted (post-attack exhaust).
        let damage_pos = events
            .iter()
            .position(|e| matches!(e, Event::DamageTaken { .. }))
            .unwrap();
        let exhaust_pos = events
            .iter()
            .position(|e| matches!(e, Event::EnemyExhausted { .. }))
            .unwrap();
        assert!(
            damage_pos < exhaust_pos,
            "DamageTaken must precede EnemyExhausted; events = {events:?}"
        );
    }

    #[test]
    fn resolve_attacks_for_investigator_excludes_exhausted_and_unengaged_enemies() {
        let inv_id = InvestigatorId(1);

        // Engaged but exhausted — must NOT attack.
        let mut e1 = test_enemy(1, "Exhausted Engaged");
        e1.engaged_with = Some(inv_id);
        e1.exhausted = true;
        e1.attack_damage = 5;

        // Ready but unengaged — must NOT attack.
        let mut e2 = test_enemy(2, "Ready Unengaged");
        e2.engaged_with = None;
        e2.attack_damage = 5;

        // Ready engaged — the only one that attacks.
        let mut e3 = test_enemy(3, "Ready Engaged");
        e3.engaged_with = Some(inv_id);
        e3.attack_damage = 1;

        let mut state = GameStateBuilder::default()
            .with_investigator(test_investigator(1))
            .with_enemy(e1)
            .with_enemy(e2)
            .with_enemy(e3)
            .build();
        let mut events = Vec::new();

        super::super::combat::resolve_attacks_for_investigator(
            &mut super::super::Cx {
                state: &mut state,
                events: &mut events,
            },
            inv_id,
        );

        // Exactly one DamageTaken (from e3, amount 1).
        let damages: Vec<&Event> = events
            .iter()
            .filter(|e| matches!(e, Event::DamageTaken { .. }))
            .collect();
        assert_eq!(
            damages.len(),
            1,
            "exactly one attacker should fire; events = {events:?}"
        );
        assert!(matches!(damages[0], Event::DamageTaken { amount: 1, .. }));

        // Only e3 exhausted; e1 already was; e2 must remain ready.
        assert!(
            state.enemies[&EnemyId(1)].exhausted,
            "e1 was already exhausted; still is"
        );
        assert!(
            !state.enemies[&EnemyId(2)].exhausted,
            "e2 must NOT exhaust (didn't attack)"
        );
        assert!(
            state.enemies[&EnemyId(3)].exhausted,
            "e3 attacked and exhausted"
        );

        // Exactly one EnemyExhausted event (e3). e1's prior-state exhausted doesn't re-emit.
        let exhausted_events: Vec<&Event> = events
            .iter()
            .filter(|e| matches!(e, Event::EnemyExhausted { .. }))
            .collect();
        assert_eq!(exhausted_events.len(), 1);
        assert!(matches!(
            exhausted_events[0],
            Event::EnemyExhausted { enemy: EnemyId(3) }
        ));
    }

    #[test]
    fn resolve_attacks_for_investigator_pick_overrides_enemy_id_order() {
        use crate::engine::OptionId;
        crate::test_support::install_test_registry();

        let inv_id = InvestigatorId(1);

        let mut e_lower = test_enemy(2, "Lower id"); // EnemyId(2), dmg 1
        e_lower.engaged_with = Some(inv_id);
        e_lower.attack_damage = 1;

        let mut e_higher = test_enemy(10, "Higher id"); // EnemyId(10), dmg 2
        e_higher.engaged_with = Some(inv_id);
        e_higher.attack_damage = 2;

        let mut state = GameStateBuilder::default()
            .with_investigator(test_investigator(1)) // TEST_INV: 8 health; 1+2=3 total damage < 8
            .with_turn_order([inv_id])
            .with_enemy(e_higher) // inserted non-id order: BTreeMap still snapshots 2 then 10
            .with_enemy(e_lower)
            .with_phase_anchor(crate::state::Continuation::EnemyPhase {
                resume: crate::state::EnemyResume::BeforeInvestigatorAttacked,
                attacking: Some(inv_id),
            })
            .build();
        let mut events = Vec::new();

        // 2 ready engaged enemies → suspend on the order pick (#143), not EnemyId order.
        let outcome = super::super::combat::resolve_attacks_for_investigator(
            &mut super::super::Cx {
                state: &mut state,
                events: &mut events,
            },
            inv_id,
        );
        let EngineOutcome::AwaitingInput { request, .. } = outcome else {
            panic!("expected an attack-order prompt, got {outcome:?}");
        };
        // Options are the snapshotted attackers in EnemyId order: option 0 =
        // EnemyId(2), option 1 = EnemyId(10). Pick the higher-id enemy (dmg 2) to
        // strike FIRST, proving the player's pick overrides the deterministic order.
        let pick = request
            .options
            .iter()
            .find(|o| o.label == format!("{:?}", EnemyId(10)))
            .expect("EnemyId(10) offered")
            .id;
        assert_eq!(
            pick,
            OptionId(1),
            "EnemyId(10) is option 1 in EnemyId order"
        );

        let resumed = resolve_input(
            &mut super::super::Cx {
                state: &mut state,
                events: &mut events,
            },
            &InputResponse::PickSingle(pick),
        );
        // Both attacks resolved; the chosen (EnemyId 10, dmg 2) struck first.
        assert!(
            !matches!(resumed, EngineOutcome::AwaitingInput { .. }),
            "loop drained, got {resumed:?}"
        );
        let damages: Vec<u8> = events
            .iter()
            .filter_map(|e| match e {
                Event::DamageTaken { amount, .. } => Some(*amount),
                _ => None,
            })
            .collect();
        assert_eq!(
            damages,
            vec![2, 1],
            "chosen EnemyId(10) (dmg 2) attacked before EnemyId(2) (dmg 1)"
        );
        assert!(state.enemies[&EnemyId(2)].exhausted && state.enemies[&EnemyId(10)].exhausted);
    }

    #[test]
    fn resolve_attacks_for_investigator_early_breaks_when_target_defeated_mid_loop() {
        // Registry needed for max_health()/max_sanity() after cp2a.
        crate::test_support::install_test_registry();
        let inv_id = InvestigatorId(1);

        // EnemyId(1) deals the killing blow on its attack.
        let mut e1 = test_enemy(1, "Killer");
        e1.engaged_with = Some(inv_id);
        e1.attack_damage = 1;

        // EnemyId(2) must NOT attack (active check fails at loop top).
        let mut e2 = test_enemy(2, "Bystander");
        e2.engaged_with = Some(inv_id);
        e2.attack_damage = 5;

        let mut state = GameStateBuilder::default()
            .with_investigator({
                let mut inv = test_investigator(1);
                // Pre-load accumulated_damage so remaining health = 1 (lethal with attack_damage=1).
                // max_health()=8 from TEST_INV; 7+1=8=defeated.
                inv.investigator_card.accumulated_damage = 7;
                inv
            })
            .with_turn_order([inv_id])
            .with_enemy(e1)
            .with_enemy(e2)
            .with_phase_anchor(crate::state::Continuation::EnemyPhase {
                resume: crate::state::EnemyResume::BeforeInvestigatorAttacked,
                attacking: Some(inv_id),
            })
            .build();
        let mut events = Vec::new();

        // 2 engaged → order pick first (#143). Pick EnemyId(1) (the killer) to
        // strike first; after it defeats the investigator, the active check at the
        // loop top early-breaks before any re-prompt, so EnemyId(2) never attacks.
        let outcome = super::super::combat::resolve_attacks_for_investigator(
            &mut super::super::Cx {
                state: &mut state,
                events: &mut events,
            },
            inv_id,
        );
        let EngineOutcome::AwaitingInput { request, .. } = outcome else {
            panic!("expected an order pick, got {outcome:?}");
        };
        let pick = request
            .options
            .iter()
            .find(|o| o.label == format!("{:?}", EnemyId(1)))
            .expect("EnemyId(1) offered")
            .id;
        let _ = resolve_input(
            &mut super::super::Cx {
                state: &mut state,
                events: &mut events,
            },
            &InputResponse::PickSingle(pick),
        );

        // e1 attacked + exhausted.
        assert!(
            state.enemies[&EnemyId(1)].exhausted,
            "e1 attacked, must exhaust"
        );
        // e2 did NOT attack and did NOT exhaust.
        assert!(
            !state.enemies[&EnemyId(2)].exhausted,
            "e2 must not exhaust (early-break)"
        );

        let damages: Vec<&Event> = events
            .iter()
            .filter(|e| matches!(e, Event::DamageTaken { .. }))
            .collect();
        assert_eq!(
            damages.len(),
            1,
            "only e1's attack lands; events = {events:?}"
        );

        let exhausted_events: Vec<&Event> = events
            .iter()
            .filter(|e| matches!(e, Event::EnemyExhausted { .. }))
            .collect();
        assert_eq!(exhausted_events.len(), 1);
        assert!(matches!(
            exhausted_events[0],
            Event::EnemyExhausted { enemy: EnemyId(1) }
        ));

        // Investigator was defeated.
        assert_eq!(state.investigators[&inv_id].status, Status::Killed);
    }

    #[test]
    fn enemy_phase_emits_phase_started_and_cascades_to_mythos_in_no_eligibility_case() {
        // 1 Active investigator, no engaged enemies. Auto-skip
        // cascades through both windows + enemy_phase_end +
        // Upkeep → Mythos.
        let inv_id = InvestigatorId(1);
        let mut state = GameStateBuilder::default()
            .with_investigator(test_investigator(1))
            .with_phase(Phase::Investigation)
            .build();
        state.turn_order = vec![inv_id];
        state.active_investigator = None;
        let mut events = Vec::new();

        step_phase(&mut Cx {
            state: &mut state,
            events: &mut events,
        }); // Investigation → Enemy

        // Positional ordering of the major events.
        let pos = |pred: &dyn Fn(&Event) -> bool| events.iter().position(pred);
        let started = pos(&|e| {
            matches!(
                e,
                Event::PhaseStarted {
                    phase: Phase::Enemy
                }
            )
        })
        .expect("PhaseStarted(Enemy)");
        let ended = pos(&|e| {
            matches!(
                e,
                Event::PhaseEnded {
                    phase: Phase::Enemy
                }
            )
        })
        .expect("PhaseEnded(Enemy)");
        let upkeep_started = pos(&|e| {
            matches!(
                e,
                Event::PhaseStarted {
                    phase: Phase::Upkeep
                }
            )
        })
        .expect("PhaseStarted(Upkeep)");

        assert!(
            started < ended && ended < upkeep_started,
            "ordered: 3.1 → 3.4 → Upkeep 4.1; events = {events:?}"
        );
        assert_eq!(state.phase, Phase::Mythos, "cascade lands in Mythos");
        assert!(
            !state
                .continuations
                .iter()
                .any(|c| matches!(c, crate::state::Continuation::EnemyPhase { .. })),
            "EnemyPhase anchor popped at phase end (cursor gone with it)"
        );
    }

    #[test]
    fn enemy_phase_with_two_investigators_iterates_in_turn_order() {
        // Each investigator is engaged with a ready enemy; the per-investigator
        // attack step must fire for both, in turn order — observable as a
        // DamageTaken per investigator (id1 before id2).
        let id1 = InvestigatorId(1);
        let id2 = InvestigatorId(2);
        let mut e1 = test_enemy(1, "Enemy 1");
        e1.engaged_with = Some(id1);
        e1.attack_damage = 1;
        let mut e2 = test_enemy(2, "Enemy 2");
        e2.engaged_with = Some(id2);
        e2.attack_damage = 1;
        let mut state = GameStateBuilder::default()
            .with_investigator(test_investigator(1))
            .with_investigator(test_investigator(2))
            .with_enemy(e1)
            .with_enemy(e2)
            .with_phase(Phase::Investigation)
            .build();
        state.turn_order = vec![id1, id2];
        state.active_investigator = None;
        let mut events = Vec::new();

        step_phase(&mut Cx {
            state: &mut state,
            events: &mut events,
        }); // Investigation → Enemy

        // One per-investigator attack landed for each, in turn order.
        let dmg1 = events
            .iter()
            .position(
                |e| matches!(e, Event::DamageTaken { investigator, .. } if *investigator == id1),
            )
            .expect("id1 attacked");
        let dmg2 = events
            .iter()
            .position(
                |e| matches!(e, Event::DamageTaken { investigator, .. } if *investigator == id2),
            )
            .expect("id2 attacked");
        assert!(
            dmg1 < dmg2,
            "investigators attacked in turn order (id1 before id2); events = {events:?}"
        );
    }

    #[test]
    fn enemy_phase_skips_eliminated_investigator_in_advance() {
        // All three investigators are engaged with a ready enemy, but id2 is
        // Insane (eliminated). The per-investigator attack step must skip id2 —
        // observable as DamageTaken for id1 and id3 only, none for id2.
        let id1 = InvestigatorId(1);
        let id2 = InvestigatorId(2);
        let id3 = InvestigatorId(3);
        let mut e1 = test_enemy(1, "Enemy 1");
        e1.engaged_with = Some(id1);
        e1.attack_damage = 1;
        let mut e2 = test_enemy(2, "Enemy 2");
        e2.engaged_with = Some(id2);
        e2.attack_damage = 1;
        let mut e3 = test_enemy(3, "Enemy 3");
        e3.engaged_with = Some(id3);
        e3.attack_damage = 1;
        let mut state = GameStateBuilder::default()
            .with_investigator(test_investigator(1))
            .with_investigator(test_investigator(2))
            .with_investigator(test_investigator(3))
            .with_enemy(e1)
            .with_enemy(e2)
            .with_enemy(e3)
            .with_phase(Phase::Investigation)
            .build();
        state.turn_order = vec![id1, id2, id3];
        state.active_investigator = None;
        state.investigators.get_mut(&id2).unwrap().status = Status::Insane;
        let mut events = Vec::new();

        step_phase(&mut Cx {
            state: &mut state,
            events: &mut events,
        }); // Investigation → Enemy

        assert!(
            events.iter().any(
                |e| matches!(e, Event::DamageTaken { investigator, .. } if *investigator == id1)
            ),
            "id1 attacked"
        );
        assert!(
            events.iter().any(
                |e| matches!(e, Event::DamageTaken { investigator, .. } if *investigator == id3)
            ),
            "id3 attacked"
        );
        assert!(
            !events.iter().any(
                |e| matches!(e, Event::DamageTaken { investigator, .. } if *investigator == id2)
            ),
            "Insane id2 must be skipped (no attack against it); events = {events:?}"
        );
    }

    #[test]
    fn enemy_phase_with_all_eliminated_opens_after_all_directly() {
        let id1 = InvestigatorId(1);
        let mut state = GameStateBuilder::default()
            .with_investigator(test_investigator(1))
            .with_phase(Phase::Investigation)
            .build();
        state.turn_order = vec![id1];
        state.active_investigator = None;
        state.investigators.get_mut(&id1).unwrap().status = Status::Killed;
        let mut events = Vec::new();

        step_phase(&mut Cx {
            state: &mut state,
            events: &mut events,
        }); // Investigation → Enemy

        // With all investigators eliminated, no per-investigator attack step
        // runs (nobody to attack) and the cascade keeps going:
        // Enemy → Upkeep (no-op steps for empty Active set) → Mythos
        // (mythos_draw_pending = None → auto-skip path) → Investigation.
        // With no investigators left to attack there is no per-investigator
        // effect to observe; the cascade landing in Investigation is the
        // structural signal that the Enemy phase ran to completion.
        assert_eq!(state.phase, Phase::Investigation);
    }

    #[test]
    fn enemy_phase_attack_lands_in_full_cascade() {
        // 1 investigator engaged with 1 ready enemy. Full Investigation→Enemy→Upkeep→Mythos
        // cascade; attack lands inside the BeforeInvestigatorAttacked continuation.
        let inv_id = InvestigatorId(1);
        let enemy_id = EnemyId(1);
        let mut enemy = test_enemy(1, "Test Enemy");
        enemy.engaged_with = Some(inv_id);
        enemy.attack_damage = 1;
        let mut state = GameStateBuilder::default()
            .with_investigator(test_investigator(1))
            .with_enemy(enemy)
            .with_phase(Phase::Investigation)
            .build();
        state.turn_order = vec![inv_id];
        state.active_investigator = None;
        let mut events = Vec::new();

        step_phase(&mut Cx {
            state: &mut state,
            events: &mut events,
        }); // Investigation → Enemy

        // The attack landed. Event-stream evidence — state.enemies's
        // `exhausted` flag is reset by Upkeep step 4.3 later in the
        // cascade (ready_exhausted_cards), so checking the post-cascade
        // state directly would race the readying step. The
        // DamageTaken + EnemyExhausted events emitted inside the
        // BeforeInvestigatorAttacked continuation are the authoritative
        // signal that the attack landed.
        assert!(events.iter().any(|e| matches!(
            e,
            Event::DamageTaken { investigator, amount: 1 } if *investigator == inv_id
        )));
        assert!(events.iter().any(|e| matches!(
            e,
            Event::EnemyExhausted { enemy } if *enemy == enemy_id
        )));

        // Cascade landed in Mythos.
        assert_eq!(state.phase, Phase::Mythos);
    }

    #[test]
    fn step_phase_from_enemy_does_not_emit_phase_ended_enemy() {
        // Direct unit-level check: step_phase emits no PhaseEnded itself,
        // so the Enemy→Upkeep step must not emit PhaseEnded(Enemy)
        // (enemy_phase_end owns that emit).
        let mut state = GameStateBuilder::default()
            .with_investigator(test_investigator(1))
            .with_phase(Phase::Enemy)
            .build();
        state.turn_order = vec![InvestigatorId(1)];
        state.active_investigator = None;
        // Use a state where Upkeep's cascade can complete (Active investigator exists).
        let mut events = Vec::new();

        step_phase(&mut Cx {
            state: &mut state,
            events: &mut events,
        }); // Enemy → Upkeep

        // step_phase itself MUST NOT emit PhaseEnded(Enemy); only
        // enemy_phase_end is allowed to (which doesn't run here — we
        // started in Enemy and stepped out, simulating the "phase
        // transition without driver-owned end emit" path).
        let phase_ended_enemy_count = events
            .iter()
            .filter(|e| {
                matches!(
                    e,
                    Event::PhaseEnded {
                        phase: Phase::Enemy
                    }
                )
            })
            .count();
        assert_eq!(
            phase_ended_enemy_count, 0,
            "step_phase must NOT emit PhaseEnded(Enemy); only enemy_phase_end may. events = {events:?}"
        );
    }

    #[test]
    fn enemy_phase_resumes_via_skip_input() {
        // Construct the state mid-pause: a BeforeInvestigatorAttacked
        // window is on the stack with empty pending_triggers (the
        // "pure-Fast window" shape that open_fast_window pushes when
        // Fast play is eligible), and the cursor points at inv1.
        //
        // Submitting PlayerAction::ResolveInput(InputResponse::Skip)
        // routes through resolve_input's "open_windows non-empty +
        // no reaction triggers" branch → close_reaction_window →
        // anchor_on_child_pop's BeforeInvestigatorAttacked arm →
        // resolve_attacks_for_investigator → cursor advance to None →
        // open AfterAllInvestigatorsAttacked → auto-skip continuation
        // → enemy_phase_end → cascade Upkeep → Mythos.
        //
        // The synthetic empty window (staged via with_open_window) fakes the
        // pause point because a real Fast-eligibility setup would require either
        // a card-registry install (heavyweight integration test) or a Fast
        // event card in hand with resources — neither tractable in
        // the engine layer. The Skip path itself is the load-bearing
        // resume mechanism this test exercises.
        let inv_id = InvestigatorId(1);
        let enemy_id = EnemyId(1);
        let mut enemy = test_enemy(1, "Test Enemy");
        enemy.engaged_with = Some(inv_id);
        enemy.attack_damage = 1;
        // active_investigator defaults to None. The EnemyPhase anchor (slice 1a)
        // sits beneath the synthetic BeforeInvestigatorAttacked window staged
        // above it; the window's close routes to anchor_on_child_pop.
        let state = GameStateBuilder::default()
            .with_investigator(test_investigator(1))
            .with_enemy(enemy)
            .with_phase(Phase::Enemy)
            .with_turn_order([inv_id])
            .with_phase_anchor(crate::state::Continuation::EnemyPhase {
                resume: crate::state::EnemyResume::BeforeInvestigatorAttacked,
                attacking: Some(inv_id),
            })
            .with_open_window(
                FastWindowKind::Phase(PhaseStep::BeforeInvestigatorAttacked),
                FastActorScope::Any,
            )
            .build();

        let result = apply(
            state,
            Action::Player(PlayerAction::ResolveInput {
                response: InputResponse::Skip,
            }),
        );

        // The Skip resumes the continuation; the cascade runs into Mythos and
        // pauses at the step-1.4 encounter-draw prompt (AwaitingInput).
        match result.outcome {
            EngineOutcome::AwaitingInput { .. } => {}
            ref other => panic!(
                "expected AwaitingInput (Mythos draw prompt) after Skip; got {other:?}; events = {:?}",
                result.events
            ),
        }
        assert_eq!(
            result.state.phase,
            Phase::Mythos,
            "cascade lands in Mythos after Skip resumed the continuation"
        );
        assert!(
            result.events.iter().any(|e| matches!(
                e,
                Event::DamageTaken { investigator, amount: 1 } if *investigator == inv_id
            )),
            "attack should have landed during the resumed continuation; events = {:?}",
            result.events
        );
        assert!(
            result.events.iter().any(|e| matches!(
                e,
                Event::EnemyExhausted { enemy } if *enemy == enemy_id
            )),
            "EnemyExhausted should fire during the resumed continuation; events = {:?}",
            result.events
        );
        assert!(
            !result
                .state
                .continuations
                .iter()
                .any(|c| matches!(c, crate::state::Continuation::EnemyPhase { .. })),
            "the EnemyPhase anchor (with its `attacking` cursor) is gone after the \
             continuation advances past the last Active investigator and the AfterAll \
             window auto-skips"
        );
    }

    // TODO(#71 follow-up): pause-on-Fast-eligibility test — needs a
    // tractable Fast-eligibility fixture at the engine layer (Fast
    // event card in hand + resources + card-registry install, which
    // would push this into the cards crate's integration tests). The
    // Skip-resume test above proves the resume path is correct; the
    // pause shape is exercised indirectly via the existing
    // any_fast_play_eligible-driven open_fast_window tests at
    // dispatch.rs's open_fast_window_tests block.
}

#[cfg(test)]
mod hand_size_tests {
    use super::*;
    use crate::assert_no_event;
    use crate::engine::OptionId;
    use crate::state::{CardCode, InvestigatorId};
    use crate::test_support::{test_investigator, GameStateBuilder};

    #[test]
    fn over_cap_investigators_lists_only_over_eight_in_player_order() {
        let inv1 = InvestigatorId(1);
        let inv2 = InvestigatorId(2);
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_investigator(test_investigator(2))
            .with_turn_order([inv2, inv1]) // player order: inv2 first
            .build();
        // inv1: 9 cards (over), inv2: 8 cards (at cap, not over).
        state.investigators.get_mut(&inv1).unwrap().hand = vec![CardCode("x".into()); 9];
        state.investigators.get_mut(&inv2).unwrap().hand = vec![CardCode("x".into()); 8];

        assert_eq!(over_cap_investigators(&state), vec![inv1]);

        // Push inv2 over too: order must follow turn_order (inv2 then inv1).
        state.investigators.get_mut(&inv2).unwrap().hand = vec![CardCode("x".into()); 10];
        assert_eq!(over_cap_investigators(&state), vec![inv2, inv1]);
    }

    #[test]
    fn upkeep_anchor_present_while_suspended_at_hand_size() {
        // upkeep_phase pushes the UpkeepPhase anchor at entry; it sits beneath
        // the step-4.5 hand-size discard while the phase is suspended (slice 1a).
        let id = InvestigatorId(1);
        let mut inv = test_investigator(1);
        inv.hand = vec![CardCode("x".into()); 10];
        inv.deck = vec![CardCode("y".into())]; // step 4.4 draws 1
        let mut state = GameStateBuilder::new()
            .with_investigator(inv)
            .with_turn_order([id])
            .with_phase(Phase::Upkeep)
            .build();
        let mut events = Vec::new();
        let outcome = upkeep_phase(&mut Cx {
            state: &mut state,
            events: &mut events,
        });
        assert!(
            matches!(outcome, EngineOutcome::AwaitingInput { .. }),
            "suspends at step 4.5 hand-size discard; got {outcome:?}",
        );
        assert!(
            state
                .continuations
                .iter()
                .any(|c| matches!(c, crate::state::Continuation::UpkeepPhase { .. })),
            "UpkeepPhase anchor present while suspended; stack = {:?}",
            state.continuations,
        );
    }

    #[test]
    fn check_hand_size_suspends_for_over_cap_investigator() {
        use crate::state::CardCode;
        let id = InvestigatorId(1);
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_turn_order([id])
            .with_phase(Phase::Upkeep)
            .build();
        state.investigators.get_mut(&id).unwrap().hand = vec![CardCode("x".into()); 10];

        let mut events = Vec::new();
        let outcome = check_hand_size(&mut Cx {
            state: &mut state,
            events: &mut events,
        });

        assert!(
            matches!(outcome, EngineOutcome::AwaitingInput { .. }),
            "over-cap investigator must suspend; got {outcome:?}"
        );
        assert_eq!(
            state.continuations.iter().rev().find_map(|c| match c {
                crate::state::Continuation::HandSizeDiscard(p) => Some(p.remaining.clone()),
                _ => None,
            }),
            Some(vec![id]),
        );
    }

    #[test]
    fn check_hand_size_is_noop_when_all_at_or_below_cap() {
        use crate::state::CardCode;
        let id = InvestigatorId(1);
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_turn_order([id])
            .with_phase(Phase::Upkeep)
            .build();
        state.investigators.get_mut(&id).unwrap().hand = vec![CardCode("x".into()); 8];

        let mut events = Vec::new();
        let outcome = check_hand_size(&mut Cx {
            state: &mut state,
            events: &mut events,
        });

        assert_eq!(outcome, EngineOutcome::Done);
        assert!(!matches!(
            state.continuations.last(),
            Some(crate::state::Continuation::HandSizeDiscard(_))
        ));
    }

    #[test]
    fn upkeep_resume_parks_at_hand_size_discard() {
        use crate::state::CardCode;
        let id = InvestigatorId(1);
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_turn_order([id])
            .with_phase(Phase::Upkeep)
            .build();
        // 13 cards in hand after the step-4.4 draw, still above the 8-card cap;
        // a small deck so the draw doesn't deck out.
        state.investigators.get_mut(&id).unwrap().hand =
            (0..12).map(|i| CardCode(format!("h{i}"))).collect();
        state.investigators.get_mut(&id).unwrap().deck =
            (0..3).map(|i| CardCode(format!("d{i}"))).collect();

        let mut events = Vec::new();
        let outcome = upkeep_resume(&mut Cx {
            state: &mut state,
            events: &mut events,
        });

        assert!(matches!(outcome, EngineOutcome::AwaitingInput { .. }));
        assert!(matches!(
            state.continuations.last(),
            Some(crate::state::Continuation::HandSizeDiscard(_))
        ));
        assert_eq!(
            state.phase,
            Phase::Upkeep,
            "4.6 must NOT have run while parked"
        );
        assert_no_event!(
            events,
            Event::PhaseEnded {
                phase: Phase::Upkeep
            }
        );
    }

    #[test]
    fn resume_hand_size_discard_discards_overflow_and_advances_to_mythos() {
        use crate::action::InputResponse;
        use crate::state::CardCode;
        let id = InvestigatorId(1);
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_turn_order([id])
            .with_phase(Phase::Upkeep)
            // UpkeepPhase anchor (slice 1a) sits beneath the staged hand-size
            // discard; the round-end teardown pops it.
            .with_phase_anchor(crate::state::Continuation::UpkeepPhase {
                resume: crate::state::UpkeepResume::Begins,
            })
            .with_hand_size_discard_pending([id])
            .build();
        // 10-card hand: discard exactly 2 (indices 0 and 1) → land at 8.
        state.investigators.get_mut(&id).unwrap().hand =
            (0..10).map(|i| CardCode(format!("c{i}"))).collect();

        let mut events = Vec::new();
        let outcome = {
            let mut cx = Cx {
                state: &mut state,
                events: &mut events,
            };
            let o = resume_hand_size_discard(
                &mut cx,
                &InputResponse::PickMultiple {
                    selected: vec![OptionId(0), OptionId(1)],
                },
            );
            super::super::drive(&mut cx, o) // slice 1b: loop-driven Upkeep→Mythos
        };

        // The discard drains the hand-size queue, so 4.6 runs and the cascade
        // steps into Mythos, pausing at the step-1.4 encounter-draw prompt.
        assert!(matches!(outcome, EngineOutcome::AwaitingInput { .. }));
        assert!(!matches!(
            state.continuations.last(),
            Some(crate::state::Continuation::HandSizeDiscard(_))
        ));
        assert_eq!(state.investigators[&id].hand.len(), 8);
        assert_eq!(state.investigators[&id].discard.len(), 2);
        assert_eq!(
            state.phase,
            Phase::Mythos,
            "queue drained → 4.6 runs → next round Mythos"
        );
        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(
                    e,
                    Event::CardDiscarded {
                        from: crate::state::Zone::Hand,
                        ..
                    }
                ))
                .count(),
            2,
        );
        assert_eq!(
            state.investigators[&id].discard,
            vec![CardCode("c0".into()), CardCode("c1".into())],
            "the cards at the submitted indices (0,1) must be the ones discarded",
        );
    }

    #[test]
    fn resume_hand_size_discard_rejects_wrong_count() {
        use crate::action::InputResponse;
        use crate::state::CardCode;
        let id = InvestigatorId(1);
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_turn_order([id])
            .with_phase(Phase::Upkeep)
            .with_hand_size_discard_pending([id])
            .build();
        state.investigators.get_mut(&id).unwrap().hand =
            (0..10).map(|i| CardCode(format!("c{i}"))).collect();

        let mut events = Vec::new();
        // Need to discard 2; submitting 1 must reject with state untouched.
        let outcome = resume_hand_size_discard(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            &InputResponse::PickMultiple {
                selected: vec![OptionId(0)],
            },
        );

        assert!(matches!(outcome, EngineOutcome::Rejected { .. }));
        assert_eq!(
            state.investigators[&id].hand.len(),
            10,
            "rejected: hand untouched"
        );
        assert!(
            matches!(
                state.continuations.last(),
                Some(crate::state::Continuation::HandSizeDiscard(_))
            ),
            "rejected: still pending"
        );
        assert!(events.is_empty(), "rejected: no events");
    }

    #[test]
    fn resume_hand_size_discard_rejects_duplicate_and_oob_indices() {
        use crate::action::InputResponse;
        use crate::state::CardCode;
        let id = InvestigatorId(1);
        let build = || {
            let mut s = GameStateBuilder::new()
                .with_investigator(test_investigator(1))
                .with_turn_order([id])
                .with_phase(Phase::Upkeep)
                .with_hand_size_discard_pending([id])
                .build();
            s.investigators.get_mut(&id).unwrap().hand =
                (0..10).map(|i| CardCode(format!("c{i}"))).collect();
            s
        };

        // Duplicate index (count is 2 but both point at slot 0).
        let mut s1 = build();
        let mut e1 = Vec::new();
        let o1 = resume_hand_size_discard(
            &mut Cx {
                state: &mut s1,
                events: &mut e1,
            },
            &InputResponse::PickMultiple {
                selected: vec![OptionId(0), OptionId(0)],
            },
        );
        assert!(matches!(o1, EngineOutcome::Rejected { .. }));
        assert_eq!(s1.investigators[&id].hand.len(), 10);

        // Out-of-bounds index.
        let mut s2 = build();
        let mut e2 = Vec::new();
        let o2 = resume_hand_size_discard(
            &mut Cx {
                state: &mut s2,
                events: &mut e2,
            },
            &InputResponse::PickMultiple {
                selected: vec![OptionId(0), OptionId(99)],
            },
        );
        assert!(matches!(o2, EngineOutcome::Rejected { .. }));
        assert_eq!(s2.investigators[&id].hand.len(), 10);
    }

    #[test]
    fn resume_hand_size_discard_sequences_investigators_in_player_order() {
        use crate::action::InputResponse;
        use crate::state::CardCode;
        let inv1 = InvestigatorId(1);
        let inv2 = InvestigatorId(2);
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_investigator(test_investigator(2))
            .with_turn_order([inv1, inv2])
            .with_phase(Phase::Upkeep)
            .with_hand_size_discard_pending([inv1, inv2])
            .build();
        state.investigators.get_mut(&inv1).unwrap().hand =
            (0..9).map(|i| CardCode(format!("a{i}"))).collect(); // discard 1
        state.investigators.get_mut(&inv2).unwrap().hand =
            (0..9).map(|i| CardCode(format!("b{i}"))).collect(); // discard 1

        // inv1 resolves first → still pending for inv2, phase still Upkeep.
        let mut events = Vec::new();
        let o1 = resume_hand_size_discard(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            &InputResponse::PickMultiple {
                selected: vec![OptionId(0)],
            },
        );
        assert!(matches!(o1, EngineOutcome::AwaitingInput { .. }));
        assert_eq!(
            state.continuations.iter().rev().find_map(|c| match c {
                crate::state::Continuation::HandSizeDiscard(p) => Some(p.remaining.clone()),
                _ => None,
            }),
            Some(vec![inv2]),
        );
        assert_eq!(state.phase, Phase::Upkeep);
        assert_eq!(state.investigators[&inv1].hand.len(), 8);
    }

    #[test]
    fn resume_hand_size_discard_rejects_wrong_response_kind() {
        use crate::action::InputResponse;
        use crate::state::CardCode;
        let id = InvestigatorId(1);
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_turn_order([id])
            .with_phase(Phase::Upkeep)
            .with_hand_size_discard_pending([id])
            .build();
        state.investigators.get_mut(&id).unwrap().hand =
            (0..10).map(|i| CardCode(format!("c{i}"))).collect();

        let mut events = Vec::new();
        let outcome = resume_hand_size_discard(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            &InputResponse::Skip,
        );

        assert!(matches!(outcome, EngineOutcome::Rejected { .. }));
        assert_eq!(
            state.investigators[&id].hand.len(),
            10,
            "rejected: hand untouched"
        );
        assert!(
            matches!(
                state.continuations.last(),
                Some(crate::state::Continuation::HandSizeDiscard(_))
            ),
            "rejected: still pending"
        );
        assert!(events.is_empty(), "rejected: no events");
    }
}

#[cfg(test)]
mod start_scenario_tests {
    use super::*;
    use crate::action::RosterEntry;
    use crate::seat_and_open;
    use crate::state::CardCode;
    use crate::state::GameStateBuilder;
    use crate::test_support::fixtures::test_investigator;
    use crate::test_support::{install_test_registry, TEST_INV};

    #[test]
    fn start_scenario_rejects_when_roster_would_seat_zero_investigators() {
        let state = GameStateBuilder::new().build();
        let result = seat_and_open(state, &[]);
        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
        assert_eq!(result.state.round, 0, "state unchanged on reject");
        assert!(result.events.is_empty(), "no events on reject");
    }

    #[test]
    fn round_end_clears_round_scoped_skill_substitutions() {
        // RR p.24 step 4.6: "until the end of the round" effects expire as the
        // round ends — in upkeep_round_end_teardown (after the round-end forced
        // abilities), not the next Mythos step.
        use crate::card_data::SkillKind;
        use crate::state::{InvestigatorId, SkillSubstitution};
        let id = InvestigatorId(1);
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_turn_order([id])
            .with_active_investigator(id)
            // upkeep_round_end_teardown pops the UpkeepPhase anchor (slice 1a).
            .with_phase_anchor(crate::state::Continuation::UpkeepPhase {
                resume: crate::state::UpkeepResume::Begins,
            })
            .build();
        state.round = 1;
        state.skill_substitutions.push(SkillSubstitution {
            investigator: id,
            use_skill: SkillKind::Intellect,
            for_skills: vec![SkillKind::Combat, SkillKind::Agility],
        });
        let mut events = Vec::new();
        super::upkeep_round_end_teardown(&mut Cx {
            state: &mut state,
            events: &mut events,
        });
        assert!(
            state.skill_substitutions.is_empty(),
            "round end (step 4.6) clears round-scoped substitutions",
        );
    }

    #[test]
    fn seat_and_open_rejects_an_empty_roster() {
        install_test_registry();
        let state = GameStateBuilder::new().build();
        let result = seat_and_open(state, &[]);
        assert!(
            matches!(result.outcome, EngineOutcome::Rejected { .. }),
            "an empty roster must reject, got {:?}",
            result.outcome
        );
    }

    // A non-empty roster whose entry cannot be resolved to investigator
    // stats rejects with state unchanged. game-core unit tests install no
    // real `CardRegistry`, so resolution fails — via the "no registry"
    // path, or (if another test in this binary already installed a fake
    // registry, since `card_registry::current()` is a process-global
    // `OnceLock`) via the "unknown code" path, as "01001" is not in the
    // fake. Either way it rejects; the registry-backed happy and
    // unknown-code paths are pinned deterministically by the
    // `crates/cards` integration test, which installs `cards::REGISTRY`.
    /// `seat_and_open` shuffles the shared encounter deck (like the player
    /// decks) with the scenario-start RNG: the deck's multiset is preserved
    /// and `EncounterDeckShuffled` fires.
    #[test]
    fn start_scenario_shuffles_the_encounter_deck() {
        use crate::state::CardCode;
        install_test_registry();
        let mut state = GameStateBuilder::new().build();
        let codes = ["e1", "e2", "e3", "e4", "e5"];
        state.encounter_deck = codes.iter().map(|c| CardCode::new(*c)).collect();
        let roster = vec![RosterEntry {
            investigator: CardCode::new(TEST_INV),
            deck: vec![],
        }];

        let result = seat_and_open(state, &roster);

        assert!(matches!(
            result.outcome,
            EngineOutcome::AwaitingInput { .. }
        ));
        crate::assert_event!(result.events, crate::event::Event::EncounterDeckShuffled);
        let mut after: Vec<&str> = result
            .state
            .encounter_deck
            .iter()
            .map(CardCode::as_str)
            .collect();
        after.sort_unstable();
        assert_eq!(after, codes, "shuffle preserves the deck's contents");
    }

    #[test]
    fn start_scenario_rejects_unresolvable_roster_entry() {
        let state = GameStateBuilder::new().build();
        let roster = vec![RosterEntry {
            investigator: CardCode::new("01001"),
            deck: vec![],
        }];
        let result = seat_and_open(state, &roster);
        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
        assert_eq!(result.state.round, 0, "state unchanged on reject");
        assert!(result.events.is_empty());
    }
}
