//! Phase-driver functions: start/end scenario, per-phase entrypoints,
//! and the round-cycle stepping logic.

use std::{collections::BTreeSet, iter};

use card_dsl::card_data::CardKind;

use crate::action::{InputResponse, RosterEntry};
use crate::card_registry::CardRegistry;
#[cfg(test)] // only `drive_phase` / `push_anchor_and_drive`, the cfg(test) helpers below
use crate::engine::dispatch;
use crate::engine::dispatch::emit::TimingEvent;
use crate::engine::dispatch::{
    act_agenda, cards, combat, cursor, emit, encounter, engagement, hunters, reaction_windows,
    reveal,
};
use crate::engine::outcome::{EngineOutcome, InputRequest, ResumeToken};
use crate::engine::Cx;
use crate::event::Event;
use crate::state::{
    CardCode, CardInPlay, Continuation, DiscardPile, EncounterDrawFrame, EnemyId, EnemyPhaseFrame,
    EnemyResume, FastWindowKind, GameState, HandSizeDiscard, InvestigationPhaseFrame,
    InvestigationResume, Investigator, InvestigatorId, InvestigatorTurnFrame, Lifetime,
    MythosPhaseFrame, MythosResume, Owner, Phase, PhaseStep, Skills, Status, UpkeepPhaseFrame,
    UpkeepResume, Zone,
};

/// Action points granted to an investigator at the start of their
/// turn during the Investigation phase. Per the Arkham Horror LCG
/// rulebook.
pub(super) const ACTIONS_PER_TURN: u8 = 3;

/// The playable gate: the rejection reason for a roster whose investigator or
/// deck holds a code `reg` refuses as unplayable, naming each offending code
/// once, in first-seen order. `None` when every code is playable.
fn unplayable_roster_cards(reg: &CardRegistry, roster: &[RosterEntry]) -> Option<String> {
    let mut unplayable: Vec<&str> = Vec::new();
    for code in roster
        .iter()
        .flat_map(|entry| iter::once(&entry.investigator).chain(&entry.deck))
    {
        if !(reg.is_playable)(code) && !unplayable.contains(&code.as_str()) {
            unplayable.push(code.as_str());
        }
    }
    (!unplayable.is_empty()).then(|| {
        format!(
            "the roster holds cards with no implementation: {}",
            unplayable.join(", ")
        )
    })
}

/// The rejection reason for a roster asking for more random basic weaknesses,
/// in total, than the weakness pool holds. Draws are without replacement from
/// one pool shared across the roster, so such a roster can't be dealt; it is
/// refused rather than short-drawn. `None` when the pool suffices.
fn weakness_pool_shortfall(reg: &CardRegistry, roster: &[RosterEntry]) -> Option<String> {
    let requested: usize = roster
        .iter()
        .map(|entry| usize::from(entry.random_basic_weaknesses))
        .sum();
    let available = (reg.weakness_pool)().len();
    (requested > available).then(|| {
        format!(
            "the roster asks for {requested} random basic weaknesses, \
             but the weakness pool holds {available}"
        )
    })
}

/// Draw each roster entry's random basic weaknesses from the weakness pool,
/// with the game's RNG: one list of drawn codes per entry, parallel to
/// `roster`, for seating to add to that entry's deck ahead of the setup
/// shuffle.
///
/// Draws go in roster order, from one pool shared across the roster, without
/// replacement (the Official FAQ's pool, `Frequently_Asked_Questions.md` lines
/// 59-61: *"This leaves a pool of 8 weaknesses remaining"* once two players
/// have drawn). Drawn codes skip the playable gate: the pool is trusted, and
/// `cards`' pool test guards it. No event is emitted, since deck contents are
/// hidden. A roster asking for none draws nothing from the RNG, so its seating
/// is exactly what it was before the draw existed.
///
/// The caller has already refused a roster asking for more than the pool
/// holds ([`weakness_pool_shortfall`]).
fn draw_random_basic_weaknesses(cx: &mut Cx, roster: &[RosterEntry]) -> Vec<Vec<CardCode>> {
    let mut pool: Vec<CardCode> = (cx.cards.weakness_pool)().to_vec();
    roster
        .iter()
        .map(|entry| {
            (0..entry.random_basic_weaknesses)
                .map(|_| pool.swap_remove(cx.state.rng.next_index(pool.len())))
                .collect()
        })
        .collect()
}

/// Internal scenario setup: seat the roster, shuffle decks, push the initial
/// phase anchors. Called by [`super::seat_and_open`] (the non-logged engine
/// entry point); never reached via a `PlayerAction` — the action log is
/// `ResolveInput`-only.
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
    // Capacity (health/sanity) is not copied into Investigator fields — the
    // accessors read from the registry directly via
    // `investigator_card.code`. We still validate that the code resolves to a
    // `CardKind::Investigator` here so seating rejects non-investigators.
    let reg = cx.cards;
    let mut resolved: Vec<(Skills, String, Vec<CardCode>, CardCode)> =
        Vec::with_capacity(roster.len());
    for entry in roster {
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

    if let Some(reason) =
        unplayable_roster_cards(reg, roster).or_else(|| weakness_pool_shortfall(reg, roster))
    {
        return EngineOutcome::Rejected {
            reason: reason.into(),
        };
    }

    // A scenario requires at least one investigator. Seating is the sole
    // seater: the roster is mandatory, an empty roster rejects.
    if resolved.is_empty() {
        return EngineOutcome::Rejected {
            reason: "a scenario requires a non-empty roster".into(),
        };
    }

    // --- mutate (all validations passed) ---
    // Each required random basic weakness joins its deck before the setup
    // shuffle mixes it in.
    let drawn = draw_random_basic_weaknesses(cx, roster);
    for ((_, _, deck, _), weaknesses) in resolved.iter_mut().zip(drawn) {
        deck.extend(weaknesses);
    }

    // Seat resolved investigators. Ids are sequential (1-based) in roster
    // order. Seated investigators start at the scenario's starting location
    // (set by setup()). None leaves them unplaced.
    let start = cx.state.starting_location;

    for (idx, (skills, name, deck, card_code)) in resolved.into_iter().enumerate() {
        let id = InvestigatorId(u32::try_from(idx).unwrap_or(0) + 1);
        let inv_card_id = cx.state.card_instance_ids.mint();
        let investigator_card =
            CardInPlay::enter_play(card_code, inv_card_id, Owner::Investigator(id));
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
                additional_actions_this_turn: Vec::new(),
                grants_spent_this_turn: BTreeSet::new(),
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

    // Drain remaining actions, standard and additional, and announce the turn
    // ended. FAQ 1.31 (`glossary/Action.md`): "Any unused actions are forfeited
    // at the end of an investigator's turn". A grant from a card in play is
    // never stored, so arming the turn frame's `ending` flag below is what ends
    // it (`cursor::turn_open`).
    active.additional_actions_this_turn.clear();
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
    // turn just ended, before the turn passes on.
    //
    // A forced effect that initiates a skill test, or a 2+ simultaneous forced
    // run (two Frozen in Fear copies), suspends here (`AwaitingInput`),
    // stranding `end_turn` before rotation. Both cases are handled uniformly:
    // flag the `InvestigatorTurn { ending: true }` frame (beneath the
    // suspension); the `drive` loop re-dispatches it once the suspension
    // resolves and runs [`resume_end_turn`]. The 2+ forced run closes to `Done`
    // (its `Terminal` continuation), so the turn frame is the single resume
    // path.
    //
    // A `Rejected` propagates as-is.
    // Frame-driven rotation: arm the `InvestigatorTurn` frame's
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
/// `InvestigatorTurn { ending: true }` arm: `end_turn` arms
/// that flag and emits `EndOfTurn`, whose forced/reaction effects (Frozen in
/// Fear 01164's willpower test) the loop drives as frames; once they pop, the
/// re-exposed turn frame runs this rotation — whether the forced run was empty,
/// completed immediately, or suspended.
pub(super) fn resume_end_turn(cx: &mut Cx, active_id: InvestigatorId) -> EngineOutcome {
    // The turn is over: pop the InvestigatorTurn frame this turn ran on. It is
    // always on top here — end_turn reaches this after the
    // EndOfTurn forced run resolves, the stranded-skill-test resume after the
    // SkillTest pops, and the forced-run continuation after its Resolution pops.
    let turn = cx.state.continuations.pop_expect::<InvestigatorTurnFrame>();
    debug_assert_eq!(
        turn.investigator, active_id,
        "resume_end_turn: the turn on top is not the active investigator's",
    );

    // 2.2.2 decision: "return to 2.2" for the next investigator, or
    // proceed to 2.3. next_active_investigator_after skips eliminated
    // investigators (Rules Reference p.10) — the same shared helper the
    // Enemy phase uses.
    if let Some(next_id) = cursor::next_active_investigator_after(cx.state, active_id) {
        begin_investigator_turn(cx, next_id)
    } else {
        cx.state.active_investigator = None;
        // 2.3 → Enemy. The cascade may suspend on a hunter-movement tie
        // (Enemy 3.2); propagate its outcome rather than swallowing it.
        investigation_phase_end(cx)
    }
}

/// Entered by [`advance_phase_entry`] on any-to-Investigation transition, and
/// by the mulligan's completion (`cards::resume_mulligan`) for round 1.
/// Owns the `PhaseStarted(Investigation)` emit (Rules Reference p.24 step 2.1)
/// and queues its forced abilities; [`investigation_after_phase_start`] opens
/// the post-2.1 player window once they have resolved. Rotation to the
/// first active investigator (step 2.2) runs in the
/// [`PhaseStep::InvestigationBegins`] continuation via
/// [`begin_investigator_turn`], the first Active investigator in turn order
/// first. TODO(#146): let the players choose who takes the first turn.
pub(super) fn investigation_phase(cx: &mut Cx) -> EngineOutcome {
    // 2.1 Investigation phase begins.
    cx.events.push(Event::PhaseStarted {
        phase: Phase::Investigation,
    });
    // Push the Investigation phase anchor. It persists for the
    // whole phase (across every investigator's turn), beneath the framework
    // windows; popped at investigation_phase_end_transition.
    //
    // **Emits in tail position** (ADR 0003). The anchor is parked at
    // `AfterPhaseStartForced` *before* the emit, because the emit queues frames
    // rather than resolving them: opening the post-2.1 window here would push it
    // above the step-2.1 forced abilities the emit had just queued, and the
    // rotation to the first investigator would follow them rather than the
    // milestone.
    cx.state.continuations.push(InvestigationPhaseFrame {
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
/// single-investigator entry still makes the first investigator in turn order
/// active within the same `apply()` call.
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
    cx.state
        .continuations
        .topmost_of_mut::<InvestigationPhaseFrame>()
        .expect("set_investigation_resume: no InvestigationPhase anchor on the stack")
        .resume = resume;
}

/// 2.2 Next investigator's turn begins. Rotates the active cursor to
/// `who` (the chosen/default investigator) and announces
/// [`TimingEvent::TurnBegan`], whose `after` cell is where On the Lam 01010's
/// *"Play after your turn begins."* is played; the post-2.2 player window opens
/// once that sequence has resolved ([`investigation_after_turn_began`]). Called
/// from the `InvestigationBegins` continuation (first turn of the phase) and
/// from `end_turn` (each subsequent turn, the rules' "return to 2.2"). Step
/// 2.2.1 (the active investigator's actions) follows as player-driven
/// inputs while `InvestigatorTurnBegins` is the "previous player window."
///
/// **Emits in tail position** (ADR 0003): the anchor is parked at
/// [`AfterTurnBegan`](crate::state::InvestigationResume::AfterTurnBegan) before
/// the emit, so the window cannot be pushed above what the emit queues.
///
/// `who` must be an `Active` investigator in `turn_order`; callers
/// resolve it via `first_active_investigator` / `next_active_investigator_after`.
pub(super) fn begin_investigator_turn(cx: &mut Cx, who: InvestigatorId) -> EngineOutcome {
    rotate_to_active(cx, who);
    // A new turn: every card in play grants its additional action again (Leo
    // De Luca 01048). Cleared ahead of the turn-begins window, which is already
    // part of the turn.
    if let Some(inv) = cx.state.investigators.get_mut(&who) {
        inv.grants_spent_this_turn.clear();
    }
    set_investigation_resume(cx, InvestigationResume::AfterTurnBegan);
    emit::queue_event(cx, &TimingEvent::TurnBegan { investigator: who })
}

/// Reached via the Investigation anchor's
/// [`AfterTurnBegan`](crate::state::InvestigationResume::AfterTurnBegan) resume
/// once step 2.2's `TurnBegan` sequence has resolved: open the post-2.2 player
/// window.
///
/// The anchor advances to `TurnBegins` first, so the closing
/// `InvestigatorTurnBegins` window routes to the right `on_child_pop` arm.
fn investigation_after_turn_began(cx: &mut Cx) -> EngineOutcome {
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
    outcome
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
/// stack — phase anchors pop-and-push rather than drain.
fn investigation_phase_end(cx: &mut Cx) -> EngineOutcome {
    cx.events.push(Event::PhaseEnded {
        phase: Phase::Investigation,
    });
    // The open-action turn has finished, so the anchor — the bottom-most
    // Investigation frame — is the top one. It stays: the transition needs it as
    // its resume point. Arm the resume BEFORE emitting: the emit may push an
    // ordering run that suspends across an `apply()` boundary, and the anchor
    // beneath it must already know where to continue.
    cx.state
        .continuations
        .top_mut::<InvestigationPhaseFrame>()
        .resume = InvestigationResume::AfterPhaseEndForced;
    emit::queue_event(
        cx,
        &TimingEvent::PhaseEnded {
            phase: Phase::Investigation,
        },
    )
}

/// Investigation → Enemy, reached via the Investigation
/// anchor's
/// [`AfterPhaseEndForced`](crate::state::InvestigationResume::AfterPhaseEndForced)
/// resume once step 2.3's queued forced abilities have resolved: pop the anchor,
/// advance `state.phase`, and push the Enemy anchor at `Entry`. The main loop's
/// `drive` advances that (runs `enemy_phase`); a hunter-movement-tie suspension
/// surfaces through `drive`.
fn investigation_phase_end_transition(cx: &mut Cx) -> EngineOutcome {
    cx.state
        .continuations
        .pop_expect::<InvestigationPhaseFrame>();
    cx.state.phase = Phase::Enemy;
    cx.state.continuations.push(EnemyPhaseFrame {
        resume: EnemyResume::Entry,
    });
    EngineOutcome::Done
}

/// Test helper: run a phase driver and let the drive loop carry it to its next
/// idle point, the way the `apply` boundary does in production.
///
/// Each phase opener emits its step-N.1 `PhaseStarted` milestone in
/// **tail position** (ADR 0003), so the phase's own opening work runs from the
/// anchor's resume rather than inline. A unit test that calls the opener
/// directly and inspects the board therefore has to drive, exactly as the loop
/// does; without it, it would only ever see the parked anchor.
#[cfg(test)]
fn drive_phase(cx: &mut Cx, outcome: EngineOutcome) -> EngineOutcome {
    dispatch::drive(cx, outcome)
}

/// Entered by [`advance_phase_entry`] on the Upkeep→Mythos transition. Lays
/// out the Rules Reference p.24 sub-steps as discrete named call
/// sites so the rule structure is grep-able.
fn mythos_phase(cx: &mut Cx) -> EngineOutcome {
    // 1.1 Round begins. Mythos phase begins.
    //     Rules Reference p.24: "As this is the first framework event
    //     of the round, it [1.1] also formalizes the beginning of a new
    //     game round." The round-counter increment lives HERE (not in
    //     the transition) so the rule's round-begin point has explicit
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
    cx.state.continuations.push(MythosPhaseFrame {
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
/// Re-parks the anchor at [`Draws`](crate::state::MythosResume::Draws)
/// first: 1.2/1.3 may push an `AdvanceReverse` frame whose reverse suspends
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
    cx.state
        .continuations
        .topmost_of_mut::<MythosPhaseFrame>()
        .expect("set_mythos_resume: no MythosPhase anchor on the stack")
        .resume = resume;
}

/// Test helper: advance to the next phase via the main loop,
/// the way a real transition does — set `state.phase` to the next phase, push
/// its anchor at `Entry`, and `drive`. Production has no synchronous
/// phase-stepping function: the four `*_phase_end`/teardown transitions push the
/// next `{Entry}` anchor and the apply boundary's `drive` advances it. Tests
/// that constructed a state in phase *N* and want phase *N+1* run through the
/// same mechanism here.
#[cfg(test)]
fn step_phase(cx: &mut Cx) -> EngineOutcome {
    let to = cx.state.phase.next();
    cx.state.phase = to;
    let anchor = match to {
        Phase::Mythos => Continuation::MythosPhase(MythosPhaseFrame {
            resume: MythosResume::Entry,
        }),
        Phase::Investigation => Continuation::InvestigationPhase(InvestigationPhaseFrame {
            resume: InvestigationResume::Entry,
        }),
        Phase::Enemy => Continuation::EnemyPhase(EnemyPhaseFrame {
            resume: EnemyResume::Entry,
        }),
        Phase::Upkeep => Continuation::UpkeepPhase(UpkeepPhaseFrame {
            resume: UpkeepResume::Entry,
        }),
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
/// the shared helper used by Mythos 1.4 for the same semantics.
/// The loop body runs in [`anchor_on_child_pop`]'s arms.
///
/// Returns the opened window's [`EngineOutcome`]. The no-active-investigator
/// path opens `AfterAllInvestigatorsAttacked`, whose continuation cascades
/// Enemy → Upkeep; that cascade can now suspend at Upkeep step 4.5
/// (hand-size discard), so the outcome propagates rather than being
/// discarded.
pub(super) fn enemy_attack_kickoff(cx: &mut Cx) -> EngineOutcome {
    // No Active investigators (turn_order empty or all eliminated) → `None`
    // opens the final window directly, mirroring mythos_phase's no-drawer path.
    open_attack_window(cx, cursor::first_active_investigator(cx.state))
}

/// Point the Enemy phase anchor at the next step-3.3 window and open it.
/// `Some(inv)` opens that investigator's `BeforeInvestigatorAttacked` window
/// and the anchor resumes into resolving their attacks
/// ([`EnemyResume::BeforeInvestigatorAttacked`] carries `inv`); `None` means no
/// investigator remains, so open the terminal `AfterAllInvestigatorsAttacked`
/// window and the anchor resumes into `enemy_phase_end`. Deriving the resume
/// and the window from one `Option` here makes a mismatched pairing
/// unrepresentable.
///
/// Shared by [`enemy_attack_kickoff`] (step 3.3 entry, cursor =
/// [`cursor::first_active_investigator`](super::cursor::first_active_investigator))
/// and [`after_enemy_phase_attacks`](super::reaction_windows::after_enemy_phase_attacks)
/// (per-investigator advance, cursor =
/// [`cursor::next_active_investigator_after`](super::cursor::next_active_investigator_after)).
pub(super) fn open_attack_window(cx: &mut Cx, attacking: Option<InvestigatorId>) -> EngineOutcome {
    let (resume, step) = match attacking {
        Some(investigator) => (
            EnemyResume::BeforeInvestigatorAttacked(investigator),
            PhaseStep::BeforeInvestigatorAttacked,
        ),
        None => (
            EnemyResume::AfterAllAttacked,
            PhaseStep::AfterAllInvestigatorsAttacked,
        ),
    };
    set_enemy_anchor(cx, resume);
    reaction_windows::open_fast_window(cx, FastWindowKind::Phase(step))
}

/// Set the Enemy phase anchor's `resume`. The low-level primitive behind
/// [`open_attack_window`]; the anchor is the bottom-most Enemy frame, and this
/// is a no-op if it is absent (only in tests that drive the attack loop in
/// isolation).
pub(super) fn set_enemy_anchor(cx: &mut Cx, resume: EnemyResume) {
    if let Some(anchor) = cx.state.continuations.topmost_of_mut::<EnemyPhaseFrame>() {
        anchor.resume = resume;
    }
}

/// Entered by [`advance_phase_entry`] on the Investigation→Enemy transition. Owns the
/// `PhaseStarted(Enemy)` emit (Rules Reference p.25 step 3.1) and queues its
/// forced abilities; hunter movement (3.2) and the attack-loop kickoff (3.3)
/// run from [`enemy_after_phase_start`] once they have resolved.
fn enemy_phase(cx: &mut Cx) -> EngineOutcome {
    // 3.1 Enemy phase begins.
    cx.events.push(Event::PhaseStarted {
        phase: Phase::Enemy,
    });
    // Push the Enemy phase anchor before hunter movement, so a
    // lead-tie suspension parks above it and the kickoff on resume finds it.
    //
    // **Emits in tail position** (ADR 0003): parked at `AfterPhaseStartForced`
    // *before* the emit, so hunter movement cannot run above the step-3.1 forced
    // abilities the emit queues. `enemy_after_phase_start` sets the running
    // cursor and runs 3.2/3.3 on re-exposure.
    cx.state.continuations.push(EnemyPhaseFrame {
        resume: EnemyResume::AfterPhaseStartForced,
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
    // Advance the anchor off `AfterPhaseStartForced`: step 3.2 is running.
    // No investigator is selected yet, since the kickoff runs after hunter
    // movement; `enemy_attack_kickoff` / `after_enemy_phase_attacks` re-point
    // the anchor before opening each attack window.
    set_enemy_anchor(cx, EnemyResume::HuntersMoving);

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
/// here would bury the queued frame at the bottom of the stack for the rest of
/// the scenario, because phase anchors pop-and-push rather than drain, and the
/// Ghoul would never move.
pub(crate) fn enemy_phase_end(cx: &mut Cx) -> EngineOutcome {
    // 3.4 Enemy phase ends.
    cx.events.push(Event::PhaseEnded {
        phase: Phase::Enemy,
    });
    // The AfterAllInvestigatorsAttacked window has closed, so the anchor is the
    // top frame. It stays: the transition needs it as its
    // resume point. Arm the resume BEFORE emitting: the emit may push an
    // ordering run that suspends across an `apply()` boundary, and the anchor
    // beneath it must already know where to continue. Uniform across 0 / 1 / 2+
    // hits — the loop re-dispatches this anchor in every case.
    *cx.state.continuations.top_mut::<EnemyPhaseFrame>() = EnemyPhaseFrame {
        resume: EnemyResume::AfterPhaseEndForced,
    };
    emit::queue_event(
        cx,
        &TimingEvent::PhaseEnded {
            phase: Phase::Enemy,
        },
    )
}

/// Enemy → Upkeep, reached via the Enemy anchor's
/// [`AfterPhaseEndForced`](crate::state::EnemyResume::AfterPhaseEndForced)
/// resume once step 3.4's queued forced abilities have resolved: pop the anchor,
/// advance `state.phase`, and push the Upkeep anchor at `Entry`. The main loop's
/// `drive` advances that (running `upkeep_phase`, which may suspend at step 4.5's
/// hand-size discard — surfaced through `drive`).
fn enemy_phase_end_transition(cx: &mut Cx) -> EngineOutcome {
    cx.state.continuations.pop_expect::<EnemyPhaseFrame>();
    cx.state.phase = Phase::Upkeep;
    cx.state.continuations.push(UpkeepPhaseFrame {
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
/// bury whatever it queues at the bottom of the stack.
pub(super) fn mythos_phase_end(cx: &mut Cx) -> EngineOutcome {
    // 1.5 Mythos phase ends.
    //     The PhaseEnded(Mythos) emit lives HERE rather than in
    //     the transition so step 1.5 has explicit ownership in the
    //     driver — mirror of step 1.1's PhaseStarted ownership in
    //     mythos_phase. Rules Reference p.24: "This step formalizes
    //     the end of the mythos phase."
    cx.events.push(Event::PhaseEnded {
        phase: Phase::Mythos,
    });
    // The MythosAfterDraws window has closed, so the anchor is the top frame. It
    // stays: the transition needs it as its resume point. Arm the resume BEFORE
    // emitting (the emit may suspend on an ordering run that outlives this
    // `apply()`), then emit in tail position.
    cx.state.continuations.top_mut::<MythosPhaseFrame>().resume = MythosResume::AfterPhaseEndForced;
    emit::queue_event(
        cx,
        &TimingEvent::PhaseEnded {
            phase: Phase::Mythos,
        },
    )
}

/// Mythos → Investigation, reached via the Mythos anchor's
/// [`AfterPhaseEndForced`](crate::state::MythosResume::AfterPhaseEndForced)
/// resume once step 1.5's queued forced abilities have resolved: pop the anchor,
/// advance `state.phase`, and push the Investigation anchor at `Entry`. The main
/// loop's `drive` advances it (runs `investigation_phase`'s opening).
fn mythos_phase_end_transition(cx: &mut Cx) -> EngineOutcome {
    cx.state.continuations.pop_expect::<MythosPhaseFrame>();
    cx.state.phase = Phase::Investigation;
    cx.state.continuations.push(InvestigationPhaseFrame {
        resume: InvestigationResume::Entry,
    });
    EngineOutcome::Done
}

/// Advance a freshly-entered phase anchor: if the top frame is
/// a `*Phase` anchor at `Entry`, pop the placeholder and run that phase's
/// opening via its existing driver (which pushes the running anchor at its first
/// boundary resume + the phase's first child). Returns `None` when the top is
/// not an `Entry` anchor, so [`anchor_on_child_pop`] falls through to its
/// boundary dispatch.
fn advance_phase_entry(cx: &mut Cx, anchor: Option<&Continuation>) -> Option<EngineOutcome> {
    match anchor {
        Some(Continuation::MythosPhase(MythosPhaseFrame {
            resume: MythosResume::Entry,
        })) => {
            cx.state.continuations.pop_expect::<MythosPhaseFrame>();
            Some(mythos_phase(cx))
        }
        Some(Continuation::InvestigationPhase(InvestigationPhaseFrame {
            resume: InvestigationResume::Entry,
        })) => {
            cx.state
                .continuations
                .pop_expect::<InvestigationPhaseFrame>();
            Some(investigation_phase(cx))
        }
        Some(Continuation::EnemyPhase(EnemyPhaseFrame {
            resume: EnemyResume::Entry,
        })) => {
            cx.state.continuations.pop_expect::<EnemyPhaseFrame>();
            Some(enemy_phase(cx))
        }
        Some(Continuation::UpkeepPhase(UpkeepPhaseFrame {
            resume: UpkeepResume::Entry,
        })) => {
            cx.state.continuations.pop_expect::<UpkeepPhaseFrame>();
            Some(upkeep_phase(cx))
        }
        _ => None,
    }
}

/// Mythos step 1.4: re-park the anchor at `AfterDraws`, then seed + open
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
        .push(EncounterDrawFrame { remaining });
    encounter::prompt_encounter_draw(cx)
}

/// Run the top `*Phase` anchor's continuation after one of its framework
/// windows closed, or advance it from `Entry` (via
/// [`advance_phase_entry`]). The window has already been popped by the close
/// path, so the anchor is now the top frame; its `resume` selects the relocated
/// body. Suspension-agnostic: a body that itself suspends returns
/// `AwaitingInput` unchanged. The `resume` is copied out before the body takes
/// `&mut cx`.
#[expect(
    clippy::too_many_lines,
    reason = "one exhaustive match over every phase anchor × resume boundary, the one map of \
              phase boundaries"
)]
pub(super) fn anchor_on_child_pop(cx: &mut Cx) -> EngineOutcome {
    let anchor = cx.state.continuations.top().cloned();
    // `Entry` advances run the phase opening; delegated so this function stays
    // the boundary dispatch.
    if let Some(out) = advance_phase_entry(cx, anchor.as_ref()) {
        return out;
    }
    match anchor {
        // Step 4.1's queued `PhaseStarted { Upkeep }` forced abilities have
        // resolved, re-exposing this anchor: open the post-4.1 window.
        Some(Continuation::UpkeepPhase(UpkeepPhaseFrame {
            resume: UpkeepResume::AfterPhaseStartForced,
        })) => upkeep_after_phase_start(cx),
        Some(Continuation::UpkeepPhase(UpkeepPhaseFrame {
            resume: UpkeepResume::Begins,
        })) => {
            // Structurally impossible under the main loop: a skill test in
            // flight sits *above* its phase anchor, so `drive` never advances
            // the anchor with one pending.
            debug_assert!(
                cx.state.current_skill_test().is_none(),
                "UpkeepBegins advanced with a skill test in flight",
            );
            upkeep_resume(cx)
        }
        // One investigator's step-4.4 draw (its deck-out horror, a drawn
        // weakness's Revelation) has resolved, re-exposing this anchor:
        // the next investigator draws, or 4.4 completes.
        Some(Continuation::UpkeepPhase(UpkeepPhaseFrame {
            resume: UpkeepResume::Drawing { drew },
        })) => upkeep_draws_after(cx, Some(drew)),
        // A staged state rests after step 4.4: run 4.5 (hand size) + 4.6
        // (phase end + transition).
        Some(Continuation::UpkeepPhase(UpkeepPhaseFrame {
            resume: UpkeepResume::AfterDraw,
        })) => upkeep_after_draw(cx),
        // Step 4.6's queued `PhaseEnded { Upkeep }` forced abilities have
        // resolved, re-exposing this anchor: run the round end.
        Some(Continuation::UpkeepPhase(UpkeepPhaseFrame {
            resume: UpkeepResume::AfterPhaseEndForced,
        })) => upkeep_round_end(cx),
        Some(Continuation::UpkeepPhase(UpkeepPhaseFrame {
            resume: UpkeepResume::AfterRoundEnd,
        })) => {
            // The round-end `EmitEvent` coordinator (the `when` act advance + the
            // `at` doom) popped, re-exposing this anchor. Run teardown
            // (expire until-end-of-round effects, Upkeep → Mythos).
            upkeep_round_end_teardown(cx)
        }
        // Step 3.1's queued `PhaseStarted { Enemy }` forced abilities have
        // resolved, re-exposing this anchor: run 3.2 + 3.3.
        Some(Continuation::EnemyPhase(EnemyPhaseFrame {
            resume: EnemyResume::AfterPhaseStartForced,
        })) => enemy_after_phase_start(cx),
        // Never re-exposed: see `EnemyResume::HuntersMoving`.
        Some(Continuation::EnemyPhase(EnemyPhaseFrame {
            resume: EnemyResume::HuntersMoving,
        })) => {
            unreachable!("the EnemyPhase anchor was re-exposed during hunter movement (step 3.2)")
        }
        Some(Continuation::EnemyPhase(EnemyPhaseFrame {
            resume: EnemyResume::BeforeInvestigatorAttacked(investigator),
        })) => {
            // Structurally impossible under the main loop: a skill
            // test in flight sits above its phase anchor, so `drive` never
            // advances the anchor with one pending.
            debug_assert!(
                cx.state.current_skill_test().is_none(),
                "BeforeInvestigatorAttacked advanced with a skill test in flight",
            );
            // Tail position (see `emit::queue_event`): the attack loop parks
            // itself on a `Continuation::AttackLoop` frame and hands each attack
            // to the timing coordinator, so a `Done` here does not mean the
            // attacks are over. The per-investigator cursor advance
            // (`after_enemy_phase_attacks`) therefore runs from the loop's own
            // drain (`finish_attack_loop`), never from this arm.
            combat::resolve_attacks_for_investigator(cx, investigator)
        }
        Some(Continuation::EnemyPhase(EnemyPhaseFrame {
            resume: EnemyResume::AfterAllAttacked,
        })) => {
            // Structurally impossible under the main loop: see the
            // BeforeInvestigatorAttacked arm above.
            debug_assert!(
                cx.state.current_skill_test().is_none(),
                "AfterAllInvestigatorsAttacked advanced with a skill test in flight",
            );
            enemy_phase_end(cx)
        }
        // Step 3.4's queued `PhaseEnded { Enemy }` forced abilities have
        // resolved, re-exposing this anchor: transition to Upkeep.
        Some(Continuation::EnemyPhase(EnemyPhaseFrame {
            resume: EnemyResume::AfterPhaseEndForced,
        })) => enemy_phase_end_transition(cx),
        // Step 2.1's queued `PhaseStarted { Investigation }` forced abilities
        // have resolved, re-exposing this anchor: open the post-2.1
        // window.
        Some(Continuation::InvestigationPhase(InvestigationPhaseFrame {
            resume: InvestigationResume::AfterPhaseStartForced,
        })) => investigation_after_phase_start(cx),
        // Step 2.3's queued `PhaseEnded { Investigation }` forced abilities have
        // resolved, re-exposing this anchor: transition to Enemy.
        Some(Continuation::InvestigationPhase(InvestigationPhaseFrame {
            resume: InvestigationResume::AfterPhaseEndForced,
        })) => investigation_phase_end_transition(cx),
        Some(Continuation::InvestigationPhase(InvestigationPhaseFrame {
            resume: InvestigationResume::Begins,
        })) => {
            // Post-2.1 window closed; start the first investigator's turn
            // (step 2.2). No skill-test-in-flight guard: runs at phase start
            // (no test in flight) and does not transition phase.
            // None branch: no active investigator can take a turn — the
            // cascade-breaker park (the loss already resolved at the defeat
            // site). See the former anchor_on_child_pop arm.
            match cursor::first_active_investigator(cx.state) {
                Some(id) => begin_investigator_turn(cx, id),
                None => EngineOutcome::Done,
            }
        }
        // Step 2.2's queued `TurnBegan` sequence has resolved, re-exposing this
        // anchor: open the post-2.2 window.
        Some(Continuation::InvestigationPhase(InvestigationPhaseFrame {
            resume: InvestigationResume::AfterTurnBegan,
        })) => investigation_after_turn_began(cx),
        Some(Continuation::InvestigationPhase(InvestigationPhaseFrame {
            resume: InvestigationResume::TurnBegins,
        })) => {
            // 2.2.1 — push the InvestigatorTurn frame above the anchor. The
            // anchor stays at TurnBegins beneath it; the frame
            // is the open turn, a prompt (drive surfaces its action menu here).
            // `active_investigator` was set by rotate_to_active in
            // begin_investigator_turn; it is the frame's investigator.
            let investigator = cx.state.active_investigator.unwrap_or_else(|| {
                unreachable!(
                    "TurnBegins reached with no active_investigator; \
                     begin_investigator_turn always sets it"
                )
            });
            cx.state.continuations.push(InvestigatorTurnFrame {
                investigator,
                ending: false,
            });
            EngineOutcome::Done
        }
        // Step 1.1's queued `PhaseStarted { Mythos }` forced abilities have
        // resolved, re-exposing this anchor: run 1.2 + 1.3.
        Some(Continuation::MythosPhase(MythosPhaseFrame {
            resume: MythosResume::AfterPhaseStartForced,
        })) => mythos_after_phase_start(cx),
        Some(Continuation::MythosPhase(MythosPhaseFrame {
            resume: MythosResume::Draws,
        })) => run_mythos_draws(cx),
        // Step 1.5's queued `PhaseEnded { Mythos }` forced abilities have
        // resolved, re-exposing this anchor: transition to Investigation.
        Some(Continuation::MythosPhase(MythosPhaseFrame {
            resume: MythosResume::AfterPhaseEndForced,
        })) => mythos_phase_end_transition(cx),
        Some(Continuation::MythosPhase(MythosPhaseFrame {
            resume: MythosResume::AfterDraws,
        })) => {
            // Structurally impossible under the main loop: a skill
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

/// Entered by [`advance_phase_entry`] on the Enemy→Upkeep transition. Owns the
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
    // Push the Upkeep phase anchor. It persists for the whole
    // phase — beneath the post-4.1 window, any step-4.5 hand-size discard, and
    // the round-end act window — and is popped at upkeep_round_end_teardown (the
    // single Upkeep→Mythos exit, after the round-end sequence finishes).
    //
    // **Emits in tail position** (ADR 0003): parked at `AfterPhaseStartForced`
    // *before* the emit, so the post-4.1 player window cannot open above the
    // step-4.1 forced abilities the emit queues.
    cx.state.continuations.push(UpkeepPhaseFrame {
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

/// The post-4.1 window continuation. Steps 4.2 and 4.3 run inline, then the 4.4
/// draws start from the first active investigator ([`upkeep_draws_after`]),
/// which runs 4.5/4.6 inline once every draw has resolved. Step 4.5
/// ([`check_hand_size`]) may suspend with [`EngineOutcome::AwaitingInput`] when
/// an investigator is over the hand cap — in which case 4.6 runs only once the
/// discard resolves — else it hands to [`upkeep_phase_end`] for 4.6 +
/// transition.
pub(super) fn upkeep_resume(cx: &mut Cx) -> EngineOutcome {
    reset_actions(cx); // 4.2
    ready_exhausted_cards(cx); // 4.3
    upkeep_draws_after(cx, None) // 4.4, then 4.5 + 4.6
}

/// Step 4.4's per-investigator cursor: draw for each active investigator in
/// player order after `drew` (from the first when `None`), one at a time.
///
/// Appendix II 4.4: *"In player order, each investigator draws 1 card. Once
/// those cards have been drawn, each investigator gains 1 resource."* With
/// `glossary/In_Player_Order.md`: *"the lead investigator performs his or her
/// part of the sequence first, followed by the other players in clockwise
/// order."* So one investigator's draw resolves **completely** — the deck-out
/// horror's deal with its soak prompt and reactions, then a drawn weakness's
/// Revelation — before the next investigator draws.
///
/// A draw that pushes frames cedes to the drive loop with the anchor at
/// [`UpkeepResume::Drawing`], whose re-exposure calls back here with that
/// investigator. Once every draw has resolved, each active investigator gains
/// 1 resource and 4.5/4.6 run.
fn upkeep_draws_after(cx: &mut Cx, drew: Option<InvestigatorId>) -> EngineOutcome {
    let mut next = match drew {
        None => cursor::first_active_investigator(cx.state),
        Some(prev) => cursor::next_active_investigator_after(cx.state, prev),
    };
    while let Some(id) = next {
        // Park the cursor before the draw (ADR 0003): the frames it may push
        // resolve above the anchor, which then resumes after `id`.
        set_upkeep_resume(cx, UpkeepResume::Drawing { drew: id });
        // Compares the top frame rather than the stack depth, so a frame
        // inserted lower down (a latched `ScenarioEnd` sits at the *bottom* of
        // the stack) never reads as a push above this anchor.
        let top_before = cx.state.continuations.top().cloned();
        // Tail position for this investigator's draw: when it pushed, the
        // outcome (`Done`) is returned as the cede below.
        let outcome = cards::draw_one_with_deckout(cx, id);
        if cx.state.continuations.top() != top_before.as_ref() {
            return outcome;
        }
        next = cursor::next_active_investigator_after(cx.state, id);
    }
    for id in cursor::active_investigators_in_turn_order(cx.state) {
        cards::grant_resources(cx, id, 1);
    }
    upkeep_after_draw(cx)
}

/// Steps 4.5 (hand size; may suspend) + 4.6 (phase end + round-end transition),
/// once step 4.4 is complete.
fn upkeep_after_draw(cx: &mut Cx) -> EngineOutcome {
    if let outcome @ EngineOutcome::AwaitingInput { .. } = check_hand_size(cx) {
        return outcome; // 4.5 parked for discard; 4.6 runs on resume
    }
    upkeep_phase_end(cx) // 4.6 + transition (may open the act round-end window)
}

/// Owns step 4.6's `PhaseEnded(Upkeep)` emit and queues its forced abilities;
/// the round-end sequence follows from [`upkeep_round_end`] once they have
/// resolved. Exact analog of [`enemy_phase_end`]. No transition emits a
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
/// First, "until the end of the round" lasting effects expire. Appendix
/// II step 4.6: *"Any active "until the end of the round" lasting effects
/// expire at this time."* They expire before `RoundEnded` is emitted, so no
/// round-end ability sees them: `glossary/Lasting_Effects.md`, *"an "until the
/// end of the phase" lasting effect expires before an "at the end of the phase"
/// ability or delayed effect may initiate."* Abilities "at the end of the
/// upkeep phase" have already resolved by now, inside the round, so they do
/// see them.
///
/// The `when the round ends` act advance (act 01109) and the `at the end of the
/// round` doom (agenda 01107, Dissonant Voices 01165) resolve as the `RoundEnded`
/// `EmitEvent` coordinator's `When`/`At` cells — structural ordering, no
/// hand-threading. Sets this anchor's resume so [`upkeep_round_end_teardown`]
/// runs when the coordinator pops, then cedes: the emit pushes the coordinator
/// and returns `Done`; the global loop drives the bucket walk (suspending at the
/// `when` window) and re-exposes this anchor at `AfterRoundEnd` on completion.
fn upkeep_round_end(cx: &mut Cx) -> EngineOutcome {
    cx.state.expire_lasting_effects(Lifetime::Round);
    set_upkeep_resume(cx, UpkeepResume::AfterRoundEnd);
    emit::queue_event(cx, &TimingEvent::RoundEnded)
}

/// Set the [`UpkeepPhase`](crate::state::Continuation::UpkeepPhase) anchor's
/// resume cursor. Reverse-searches the stack (mirroring [`set_enemy_anchor`])
/// so it is robust whether the anchor is on top (`upkeep_phase_end`'s call, made
/// before the round-end coordinator is pushed) or buried beneath a drawn-weakness
/// Revelation pushed by the step-4.4 draw (`upkeep_resume`'s cede).
fn set_upkeep_resume(cx: &mut Cx, resume: UpkeepResume) {
    cx.state
        .continuations
        .topmost_of_mut::<UpkeepPhaseFrame>()
        .expect("set_upkeep_resume: no UpkeepPhase anchor on the stack")
        .resume = resume;
}

/// Teardown after the round-end `EmitEvent` coordinator pops — reached via the
/// Upkeep anchor's [`AfterRoundEnd`](crate::state::UpkeepResume::AfterRoundEnd)
/// resume:
/// transition Upkeep → Mythos. Round-lifetime lasting effects have already
/// expired, ahead of the emit in [`upkeep_round_end`].
pub(super) fn upkeep_round_end_teardown(cx: &mut Cx) -> EngineOutcome {
    // Pop the Upkeep anchor: this is the single Upkeep→Mythos
    // exit, reached after the whole round-end sequence (act window, doom) has
    // resolved, so the anchor is the top frame here.
    cx.state.continuations.pop_expect::<UpkeepPhaseFrame>();
    // Upkeep → Mythos: advance `state.phase` + push the Mythos
    // anchor at `Entry`. The main loop's `drive` advances it (runs mythos_phase —
    // the round bump + PhaseStarted(Mythos) live there).
    cx.state.phase = Phase::Mythos;
    cx.state.continuations.push(MythosPhaseFrame {
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
/// co-located with an investigator engages it via
/// [`engagement::reengage_at_location`]
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
            engagement::reengage_at_location(cx, eid);
        }
    }
}

/// Maximum hand size (Rules Reference p.25 step 4.5: discard down to 8). A module
/// constant rather than a per-investigator field — no card in the
/// current scope modifies the cap. A future hand-size-modifying card
/// introduces the field when it is actually needed.
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
    hand_size_discard_prompt(cx)
}

/// The [`EngineOutcome::AwaitingInput`] a [`HandSizeDiscard`] frame on top
/// waits on: shown by [`park_hand_size_discard`] when it pushes the frame, and
/// again by the main loop when it meets the frame on top. It names the count
/// the pick must hold ([`hand_size_discard_count`] for the investigator
/// prompted), which falls short of reaching the cap when weaknesses fill the
/// overflow, and says that weaknesses can't be chosen.
pub(super) fn hand_size_discard_prompt(cx: &Cx) -> EngineOutcome {
    let count = cx
        .state
        .continuations
        .top_expect::<HandSizeDiscard>()
        .remaining
        .first()
        .map_or(0, |id| hand_size_discard_count(cx, *id));
    let cards = if count == 1 { "card" } else { "cards" };
    EngineOutcome::AwaitingInput {
        request: InputRequest::pick_multiple(format!(
            "You have more than {HAND_SIZE_LIMIT} cards in hand — choose {count} {cards} to \
             discard. Weaknesses can't be chosen.",
        )),
        resume_token: ResumeToken(0),
    }
}

/// How many cards `id` must discard at step 4.5: the overflow over
/// [`HAND_SIZE_LIMIT`], capped at the non-weakness cards in hand.
///
/// `glossary/Weakness.md`: *"A player may not optionally choose to discard a
/// weakness card from hand, unless a card explicitly specifies otherwise."* An
/// overflow the non-weaknesses cannot meet is not met: the hand stays over the
/// cap. The Official FAQ (`data/official-faq/Frequently_Asked_Questions.md`,
/// Forced Learning) answers the same shortfall that way: *"Weaknesses cannot be
/// chosen to be discarded from your hand. … If you draw 2 weaknesses, you cannot
/// discard either one, and must resolve both of them."*
fn hand_size_discard_count(cx: &Cx, id: InvestigatorId) -> usize {
    let hand = &cx.state.investigators[&id].hand;
    let overflow = hand.len().saturating_sub(HAND_SIZE_LIMIT as usize);
    let discardable = hand
        .iter()
        .filter(|code| !cards::is_weakness_code(cx.cards, code))
        .count();
    overflow.min(discardable)
}

/// 4.5 Each investigator checks hand size. In player order, each
/// investigator over [`HAND_SIZE_LIMIT`] with a card it may discard is
/// prompted to discard down to the cap ([`hand_size_discard_count`]). Returns
/// [`EngineOutcome::AwaitingInput`] (parking on the first such investigator)
/// when anyone is, or [`EngineOutcome::Done`] when nobody is — in which case
/// the caller proceeds straight to 4.6.
fn check_hand_size(cx: &mut Cx) -> EngineOutcome {
    let remaining: Vec<InvestigatorId> = over_cap_investigators(cx.state)
        .into_iter()
        .filter(|&id| hand_size_discard_count(cx, id) > 0)
        .collect();
    if remaining.is_empty() {
        return EngineOutcome::Done;
    }
    park_hand_size_discard(cx, remaining)
}

/// Resume a parked upkeep hand-size discard. Validates the
/// `PickMultiple` response against the currently-prompted investigator
/// (`remaining[0]`): the indices must be unique, in-bounds, name no weakness,
/// and be exactly [`hand_size_discard_count`] in count. On success, discards the
/// chosen cards (emitting [`Event::CardDiscarded`] per card), pops the
/// queue front, and either re-prompts the next over-cap investigator or
/// — when the queue drains — runs [`upkeep_phase_end`] (4.6 + transition
/// to Mythos). Rejections leave state and events untouched.
pub(super) fn resume_hand_size_discard(cx: &mut Cx, response: &InputResponse) -> EngineOutcome {
    let pending = cx
        .state
        .continuations
        .top_expect::<HandSizeDiscard>()
        .clone();
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
    let target = hand_size_discard_count(cx, current);
    if indices.len() != target {
        return EngineOutcome::Rejected {
            reason: format!(
                "hand-size discard: {current:?} must discard exactly {target} card(s) \
                 (hand {hand_len}, cap {HAND_SIZE_LIMIT}, weaknesses excluded), got {}",
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
        let code = &inv.hand[i as usize];
        if cards::is_weakness_code(cx.cards, code) {
            return EngineOutcome::Rejected {
                reason: format!(
                    "hand-size discard: {code} at hand index {i} is a weakness, which may not \
                     be chosen to be discarded",
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
            code,
            from: Zone::Hand,
            to: DiscardPile::Investigator(current),
        });
    }

    // ---- advance the queue ----
    let mut remaining = pending.remaining;
    remaining.remove(0);
    cx.state.continuations.pop_expect::<HandSizeDiscard>();
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
/// does not refresh (step 2.2 is just "the turn begins");
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

#[cfg(test)]
mod tests;
