# EmitEvent-frame — C-coordinators (`EmitEvent`/`TimingPoint` frames)

**Status:** design (2026-06-23). The C-coordinators remainder of the EmitEvent-frame
arc ([#434](https://github.com/talelburg/eldritch/issues/434), umbrella
[#435](https://github.com/talelburg/eldritch/issues/435)). Follows **C-plumbing**
(#431/PR #443) and **Slice B** (B-i/B-ii/B-iii, PRs #440–#442). Precedes nothing —
this closes #434/#435. Slice D (#423) runs in parallel.

**Parent specs (load-bearing):**
- Slice B coordinators: [`2026-06-23-emitevent-frame-slice-b-coordinators-design.md`](2026-06-23-emitevent-frame-slice-b-coordinators-design.md)
  — the data model + round-end remodel target this spec realizes.
- C-plumbing: [`2026-06-23-emitevent-frame-slice-c-loop-driving-design.md`](2026-06-23-emitevent-frame-slice-c-loop-driving-design.md)
  — the uniform top-frame `drive` loop these coordinators dispatch onto.
- Arc decomposition: [`2026-06-22-emitevent-frame-arc-decomposition-design.md`](2026-06-22-emitevent-frame-arc-decomposition-design.md).

**This spec supersedes the Slice-B "Data model" / "Driving (this slice)" sections.**
Slice B (written before C-plumbing shipped) assumed the coordinators would be introduced
"imperatively driven, not `drive`-loop." C-plumbing (PR #443) shipped first and made the
loop dispatch **every** frame uniformly, which changes the realization — see "What
changed since Slice B."

## What's already done

- **B-i (PR #440):** `EventTiming = { When, At, After }` — the timing axis is first-class
  in the DSL.
- **B-ii (PR #441):** act 01109's round-end advance *logic* moved into a registry
  `When`-`RoundEnded` reaction (`the_barrier::abilities()[1]`, native
  `01109:round_end_advance` → `round_end_advance(cx, HALLWAY)`); the doom abilities
  (agenda 01107, Dissonant Voices 01165) re-tagged `After → At`; `collect_forced_hits`
  is bucket-parameterized.
- **B-iii (PR #442):** `Event::WindowOpened`/`WindowClosed` + `WindowKind` deleted; scan
  eligibility reads from `TimingEvent`.
- **C-plumbing (PR #443):** the `drive` loop dispatches every frame by uniform top-frame
  dispatch (`TimingPointWindow`/`FastWindow`/`SkillTest`/`EncounterCard` arms); drivers
  return `Done` and the loop re-dispatches `continuations.last()`.

## What remains (this slice)

The `when → at → after` axis is still **collapsed into the upkeep round-end hand-thread**.
`upkeep_phase_end` hand-sequences the `when` act window (`round_end_advance_window` +
`Continuation::ActRoundEnd` + `resume_act_round_end_advance`) then the `at` doom
(`upkeep_round_end_at_and_after`) then teardown. Getting that order wrong was the §G bug
(fixed standalone in PR #396, but the order stays *hand-enforced*). This slice makes the
axis **structural**:

1. Introduce `Continuation::EmitEvent { event, bucket }` + `Continuation::TimingPoint
   { event, bucket, sub }` as `drive`-loop-dispatched coordinator frames (Strategy P:
   the general `When → At → After` cursor, not a round-end special-case).
2. Route the **multi-bucket** round-end through them; complete the round-end remodel
   (delete the `ActRoundEnd` machinery + the `Act.round_end_advance` field).
3. Per-cell eligibility **re-scan** + the §G synthetic regression test (the one
   new-behaviour piece).

## What changed since Slice B (two simplifications)

**No local driver, and no behaviour-preserving "migrate every emit site" step.** Working
through the actual call sites established two facts the Slice-B framing missed:

- **Single-bucket events never need a coordinator frame.** Every `TimingEvent` except
  `RoundEnded` sits in exactly one bucket. A single-bucket emit either fires 0/1 forced
  inline (returns `Done`, caller continues — today's path) or, for a reaction-capable
  event, *queues* its reaction window and returns `Done` while the **enclosing driver**
  opens it at its next step boundary (also today's path — forced fires inline first, so
  forced-before-reaction holds without any frame). Neither suspends *during* the emit nor
  loops, so by the #393 Checkpoint-C rule ("a step is a frame iff it suspends or loops")
  it must **not** become a frame. They stay inline in `emit_event`.

- **The multi-bucket case cedes to the global loop — no bounded local driver.** An earlier
  draft proposed a `drive_emit_to_base` mirroring `drive_effect_to_base`. It's
  unnecessary. `RoundEnded` is the only multi-bucket event, and its only emitter
  (`upkeep_phase_end`) is a phase anchor with a resume cursor. So `emit_event(RoundEnded)`
  pushes the `EmitEvent { RoundEnded, When }` coordinator and **returns**; the caller
  cedes (does no synchronous post-emit work — it set its resume cursor first); the global
  `drive` loop takes over, walking the buckets and suspending at the `when` act window via
  the existing window machinery, resuming through the loop on window close. Driving is the
  global loop's job, exactly as C-plumbing intended.

**Consequence: this is one coupled PR, not two.** Because single-bucket events don't
migrate, there is no behaviour-preserving "introduce the frames everywhere" PR ahead of
the round-end remodel. Building the coordinator, routing `RoundEnded` through it, and
deleting the `ActRoundEnd` machinery are inseparable. One PR.

**Why P (general coordinator) despite one consumer.** A purist YAGNI reading says "only
`RoundEnded` is multi-bucket — handle it with a round-end-specific resume cursor (Q)." We
reject Q: it is precisely the localized hand-thread the arc (#435/#212) set out to delete,
and hand-threading this ordering is what produced the §G bug. P is justified not by 2+
consumers but by *making the timing axis structural so the bug class cannot recur*. The
general cursor is a 3-element loop over the `EventTiming` enum the DSL already has — a
mechanism, not a new abstraction — and future round-end / `when X`-`after X` pairs land on
it for free.

## `emit_event` stays the one chokepoint

`emit_event` remains the single entry point (#212). It branches internally; call sites are
unchanged and never learn "is my event framed":

```text
emit_event(event):
    if event is RoundEnded (the multi-bucket case):
        push Continuation::EmitEvent { event, bucket: When }
        return Done                      # cede; the global loop drives it
    else:                                # single-bucket — today's behaviour, verbatim
        if event.opens_reaction_window(): queue_reaction_window(event)   # deferred-open
        match collect_forced_hits(point, <the event's bucket>):
            0 or 1 -> fire_forced_triggers(point, bucket)   # inline, Done
            2+     -> open_forced_resolution(...)            # AwaitingInput, cedes
```

The single-bucket branch keeps the current `debug_assert!(forced == Done)` contract at the
forced-only and reaction-capable sites: "no forced *suspension*; any queued reaction window
is deferred to the enclosing driver." (Note: a stack-top-unchanged assertion would *not*
work for reaction-capable events — they legitimately push a queued-deferred window while
remaining safe to continue inline. The return-value assert is the right invariant.)

## Data model

The bucket cursor **reuses `card_dsl::EventTiming`** directly (no parallel enum — "an
ability's timing *is* its bucket" is an identity).

```rust
enum TimingSub { Forced, Reaction }

/// Coordinator: iterate When → At → After for one game event. `bucket` is the cursor.
Continuation::EmitEvent { event: TimingEvent, bucket: EventTiming }

/// Coordinator: one bucket, run forced then reaction. `sub` is the cursor.
Continuation::TimingPoint { event: TimingEvent, bucket: EventTiming, sub: TimingSub }
```

### `EmitEvent` dispatch (a new `drive`-loop arm)

On each dispatch (initial push, or re-exposure after a child `TimingPoint` pops):

1. **Re-scan eligibility for the current `bucket`** (board state may have changed in the
   prior bucket — see "Per-cell re-scan").
2. If the bucket has any forced *or* reaction candidate, push
   `TimingPoint { event, bucket, sub: Forced }` and yield (`Done` → the loop dispatches the
   child).
3. On the child's pop, advance the cursor `When → At → After`; repeat from 1.
4. After `After` completes, pop `EmitEvent`. (The loop then re-exposes the emitter's frame
   — for round-end, the Upkeep anchor at its `AfterRoundEnd` resume.)

### `TimingPoint` dispatch (a new `drive`-loop arm)

For one `bucket`, the `sub` cursor runs:

- **`Forced`** — `collect_forced_hits(point, bucket)`. 0/1 fire inline via
  `fire_forced_triggers(point, bucket)`; **2+** open the existing lead-ordered forced run
  (`open_forced_resolution`). Either way, **advance `sub = Reaction` *before* opening the
  run / before returning `Done`**, so the re-dispatched `TimingPoint` resumes at `Reaction`
  rather than re-scanning forced (which would loop). Then return `Done` — the loop
  re-dispatches `TimingPoint` at `Reaction` (0/1 case) or dispatches the forced run, whose
  close re-exposes `TimingPoint` at `Reaction` (2+ case).
- **`Reaction`** — scan reactions of `EventTiming == bucket` and **open** the reaction
  window (the `when`-cell act advance for round-end). If candidates, push the
  `TimingPointWindow { mode: Reaction }` and yield; on its pop, pop `TimingPoint`. Empty →
  pop `TimingPoint` immediately.

**The 2+ forced run resumes the coordinator, not a framework tail.** `open_forced_resolution`
requires a `ForcedContinuation` naming what runs on close. Inside a coordinator there is no
framework tail to name — the parent `TimingPoint` (beneath the forced window) is the resume.
Add a new **`ForcedContinuation::Coordinator`** whose `resume_forced_continuation` arm is a
no-op (`Done`); the C-plumbing loop then re-dispatches the exposed `TimingPoint`. (This is
the round-end `at` cell's real path: agenda 01107 doom **+** Dissonant Voices 01165 are both
`At`-`RoundEnded` forced, so 2+ is reachable in scope — exactly the case `UpkeepAfterRoundEnded`
handles today.)

This is "exactly what single-bucket `emit_event` does today," parameterized by bucket and
made frame-resumable — *except* the reaction window is **opened** in the `Reaction` sub
(not queue-deferred), because a multi-bucket walk must fully resolve the `when` reaction
before the `at` forced fires (the §G ordering). That open-vs-defer difference is confined
to the coordinator (round-end); single-bucket events keep queue-deferred opening.

## The round-end remodel — uniform, no special-case

With the bucket first-class and the act/agenda reaction scan in place (below), the
round-end `when` and `at` cells are both **scanned registry abilities** the coordinator
dispatches identically:

- **`when` cell** — act 01109's `When`-`RoundEnded` reaction (already exists,
  `the_barrier`), surfaced as the `when`-bucket reaction window's single board candidate
  (`PickSingle` = advance / `Skip` = decline); its native `round_end_advance(cx, HALLWAY)`
  does the group clue-spend + `advance_act`. The act's on-advance reverse (Parlor reveal +
  Priest spawn) chains off `advance_act` as today.
- **`at` cell** — agenda 01107's doom + Dissonant Voices 01165's discard (already `At`),
  fired by the `at`-bucket `TimingPoint` `Forced` sub.

**Reaction scan reaches the act/agenda.** Today `scan_pending_triggers` scans only
`cards_in_play`. The forced scan already reaches the current act/agenda (`collect_forced_hits`'s
`RoundEnded` arm scans `act_deck[act_index]` / `agenda_deck[agenda_index]` + threat-area
treacheries). Extend the reaction scan to mirror it: scan the current act + agenda for
`Reaction`/`bucket`-timed abilities matching the event, controller = the lead (as the
forced scan does). The act-advance candidate carries `CandidateSource::Board`.

**Affordability gate dropped (more RR-accurate).** The old framework gate
(`round_end_advance_window` reading `Act.round_end_advance`) suppressed the window when the
group couldn't afford. The reaction is now offered whenever the act exposes the `When`
ability — "investigators **may** … spend" (RR) — and the native no-ops/rejects cleanly when
the group can't afford. The contributor location is printed on the card (the native passes
`HALLWAY`), not a framework field. (One open question for the scan: whether to keep a
*potential* eligibility check — suppress the candidate when the group holds 0 clues — under
the #368 trigger-eligibility gate. Default: offer it and let the native handle "can't
afford"; revisit with #368.)

**Teardown moves onto the Upkeep anchor's resume cursor — subsuming
`ForcedContinuation::UpkeepAfterRoundEnded`.** `upkeep_phase_end` becomes: emit
`PhaseEnded { Upkeep }` (single-bucket, inline), set the Upkeep anchor's resume cursor to a
new `UpkeepResume::AfterRoundEnd`, push the `RoundEnded` `EmitEvent` coordinator, return
`Done`. When the coordinator pops, the loop re-exposes the Upkeep anchor at `AfterRoundEnd`
→ `upkeep_round_end_teardown` (expire until-end-of-round effects, pop anchor, Upkeep →
Mythos). The forced-run continuation `UpkeepAfterRoundEnded` is no longer needed — the
framework tail is structural.

### Deletion list

- `Continuation::ActRoundEnd(ActRoundEndPending)` + `ActRoundEndPending` struct.
- `resume_act_round_end_advance`, `round_end_advance_window`, `upkeep_round_end_at_and_after`,
  `fire_act_round_end_ability`, `round_end_advance_ability_index` (all in `phases.rs`).
- `Act.round_end_advance` field + `RoundEndAdvance` struct (`game_state.rs`).
- `ForcedContinuation::UpkeepAfterRoundEnded` + its `resume_forced_continuation` arm (the
  round-end teardown moves to the Upkeep anchor's `AfterRoundEnd` resume; the `at`-cell
  forced run resumes via the new `ForcedContinuation::Coordinator` no-op instead).
  `TimingEvent::RoundEnded`'s `forced_continuation` arm becomes dead (round-end no longer
  reaches `emit_event`'s forced path — its `emit_event` branch pushes the coordinator) and
  folds into the `None`/loud-guard group.
- The `ResolveInput`/`apply_player_action` routing arm for `ActRoundEnd`.

**Added:** `ForcedContinuation::Coordinator` (no-op resume; see `TimingPoint` dispatch).

`upkeep_round_end_teardown` is **kept** (re-pointed to the `AfterRoundEnd` resume).
`the_barrier`'s `When` reaction + `round_end_advance` native are **kept** (now scanned, not
hand-fired).

## Per-cell eligibility re-scan (the one new-behaviour correctness)

The `EmitEvent` cursor re-scans eligibility *entering each cell*: a `when`-cell that mutates
board state can change whether the `at`-cell fires — "a `when` reaction can change whether
an `at` forced even fires," so the grid is **not** pre-computed. The nested frames make
"enter each cell fresh, re-scan" structural (step 1 of `EmitEvent` dispatch).

Conceptual precedent already in the engine: a `When` cancel (Dodge) suppresses the event so
downstream cells never fire. But no in-scope card exercises *cross-bucket suppression at one
round-end emit*, so this is covered by a **synthetic test-only act/agenda fixture** (the §G
class): a `when`-cell advance that flips an `at`-cell forced ability's precondition,
asserting the `at` forced does **not** fire after the `when` cell changed it. Does not wait
on a corpus card.

## Out of scope (Slice D / #423, may run in parallel)

Reifying the **synchronous post-emit tails** of the single-bucket emitters
(`advance_agenda`'s `doom = 0; index += 1`, `enemy_defeat`'s cleanup, the mid-effect-walk
emits) as their own frames is the end-state-B / pure-push-and-return direction. It is
`#423`'s job (migrating effect/framework call sites off synchronous entries to top-frame
dispatch), entangled with the effect-frame substrate. This slice deliberately leaves those
tails synchronous — they fire 0/1 forced and never suspend in scope, and the
`debug_assert!(Done)` contract proves it. Pulling them in would balloon the slice into
#423.

## Testing strategy

- **Behaviour-preserving for every single-bucket event** (byte-identical event log): the
  inline branch is unchanged. Backstopped by the full engine + integration suite —
  `forced_triggers.rs`, `reaction_windows.rs`, `enemy_defeat`/`combat`,
  `the_gathering*.rs`.
- **Round-end behaviour-preserving** for the in-scope `when → at` order (same group spend,
  same advance, same doom timing): `crates/game-core/tests/act_round_end.rs`,
  `crates/cards/tests/act_advancement.rs`, `crates/cards/tests/the_barrier.rs`,
  `crates/scenarios/tests/the_gathering*.rs`. The advance is now reached via a reaction
  `PickSingle`/`Skip` rather than the `ActRoundEnd` `Confirm`/`Skip` — update those input
  shapes (the offered candidate, not a bespoke prompt).
- **New behaviour:** the per-cell re-scan, covered by the synthetic §G act/agenda fixture
  (`when`-cell flips an `at`-cell forced's eligibility). Add as a focused engine test.
- Match the full CI gauntlet (fmt / clippy / test / doc / wasm) before pushing.

## What "done" looks like

`Continuation::EmitEvent`/`TimingPoint` are `drive`-loop-dispatched coordinator frames; the
`when → at → after` axis is **structural** (the coordinator scans each cell by ability
timing with per-cell re-scan), not hand-threaded; `emit_event` stays the one chokepoint
(inline for single-bucket, coordinator for the multi-bucket round-end, no local driver);
the `ActRoundEnd` machinery + `Act.round_end_advance` field + `UpkeepAfterRoundEnded` are
deleted, round-end teardown runs on the Upkeep anchor's `AfterRoundEnd` resume; the §G
re-scan has a regression test. Closes #434 / #435; unblocks nothing further in the arc
(Slice D #423 is parallel).

## Open questions

- **Round-end `when` candidate potential-gate.** Whether to suppress the act-advance
  candidate when the group holds 0 clues (a "potential" eligibility check) or always offer
  it and let the native no-op. Default: always offer (RR "may"); fold any suppression into
  the #368 trigger-eligibility gate when its 3rd consumer lands. Non-blocking.
