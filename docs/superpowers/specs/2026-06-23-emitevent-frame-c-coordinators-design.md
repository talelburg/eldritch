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
4. **Fold the `EndOfTurn` 2+-forced tail onto the `InvestigatorTurn { ending }` frame**
   (the turn frame already models rotation; unify its two redundant resume paths).
5. **Delete `ForcedContinuation` entirely** — with round-end and EndOfTurn off it, the only
   survivor (`Terminal`) is already a no-op, so the mechanism is vestigial. Forced runs carry
   no continuation; close → `Done` → the loop re-dispatches the exposed parent frame.

(4) and (5) followed from working through *why* `ForcedContinuation` still existed; they are
the same thesis as the coordinators — "the loop drives every frame; bespoke
continuation-parking is gone" — so they belong in this slice, not a follow-up.

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

**The 2+ forced run carries no continuation — the parent frame is the resume.** This is the
round-end `at` cell's real path (agenda 01107 doom **+** Dissonant Voices 01165 are both
`At`-`RoundEnded` forced, so 2+ is reachable). Today `open_forced_resolution` requires a
`ForcedContinuation` naming what runs on close; inside a coordinator there is no framework
tail to name — the parent `TimingPoint` beneath the forced window is the resume. Rather than
add a no-op `ForcedContinuation::Coordinator`, this slice **deletes `ForcedContinuation`
entirely** (see "`ForcedContinuation` is now vestigial — delete it"): the forced run closes
to `Done` and the C-plumbing loop re-dispatches the exposed `TimingPoint` at `Reaction`.

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

## EndOfTurn unification — fold the 2+-forced path onto the turn frame

`EndOfTurn` is single-bucket (no coordinator), but it is the **other** site whose 2+-forced
run parks a tail in `ForcedContinuation` today (`EndOfTurnAfterForced` → `resume_end_turn`).
It is eliminable, and doing so here is what lets us delete `ForcedContinuation` outright.

The key fact: **`end_turn` is invoked from exactly one production site — `PlayerAction::EndTurn`**
(everything else is tests; there is no auto-end-on-zero-actions). So the `EndOfTurn` forced
emit always runs **inline in that action handler, before any cede**, and the
`InvestigatorTurn { investigator, ending }` frame already exists with rotation already modeled
as `resume_end_turn`. Today there are two redundant resume paths for that same tail:

- a **single** suspending `EndOfTurn` forced (a skill test — Frozen in Fear) sets
  `InvestigatorTurn.ending = true` and the skill-test commit-resume re-enters `resume_end_turn`;
- **2+** simultaneous `EndOfTurn` forced open a run carrying `EndOfTurnAfterForced` (and an
  `is_forced` guard at the emit site *skips* flagging the frame in this case).

Because the forced emit is always inline-then-cede, `ending: bool` unambiguously means "only
rotation remains" — the suspension (skill test *or* forced run) always sits **above**
`InvestigatorTurn`, and resume always means rotate. **No sub-cursor is needed.** Unify both
paths onto the frame:

- Add a `drive`-loop arm: `InvestigatorTurn { ending: true }` → `resume_end_turn` (when
  `ending` is `false` it stays the idle open-turn sentinel, as today).
- `end_turn`'s `AwaitingInput` branch **always** sets `ending = true` on suspend — delete the
  `is_forced` special-case (lines ~240–266) that skipped flagging for the 2+ case.
- The skill-test commit-resume stops calling `resume_end_turn` directly; it just returns
  `Done` and the loop re-dispatches `InvestigatorTurn { ending: true }`.
- `EndOfTurnAfterForced` deletes (the 2+ forced run carries no continuation; close → `Done`
  → loop re-dispatches `InvestigatorTurn { ending: true }`).

## `ForcedContinuation` is now vestigial — delete it

`ForcedContinuation` has exactly three variants: `Terminal`, `UpkeepAfterRoundEnded`,
`EndOfTurnAfterForced`. This slice deletes the latter two (round-end → coordinator + Upkeep
`AfterRoundEnd` resume; EndOfTurn → `InvestigatorTurn { ending }`). The survivor `Terminal`
is *already* a pure no-op (`resume_forced_continuation` returns `Done`, the loop re-dispatches
the exposed frame). So the mechanism collapses to nothing — **delete the enum** rather than
keep a no-op:

- `TimingMode::Forced(ForcedContinuation)` → `TimingMode::Forced` (no payload).
- `open_forced_resolution` drops the `continuation` arg.
- the forced arm of `close_reaction_window` returns `Done` directly → the C-plumbing loop
  re-dispatches `continuations.last()` (the exposed parent frame: the coordinator's
  `TimingPoint`, `InvestigatorTurn { ending }`, or the move's `ActionResolution`).
- delete the `ForcedContinuation` enum, `resume_forced_continuation`, and
  `TimingEvent::forced_continuation()`.

**The replacing invariant** (the whole point): *any emit site that can produce a 2+-forced
run must have its post-emit tail on a frame.* This slice satisfies it for the two sites that
actually can in scope (Upkeep, EndOfTurn). Sites that **cannot** in scope (EnemyDefeated,
AgendaAdvanced, EnteredLocation, …) keep their existing `debug_assert!(forced == Done)`; if a
future card makes one 2+-capable, that assert fires loudly ("frame-ify this caller") — the
correct failure mode, identical in spirit to today's "2+ needs #213" guard, never a silent
dropped tail.

### Deletion list

- `Continuation::ActRoundEnd(ActRoundEndPending)` + `ActRoundEndPending` struct.
- `resume_act_round_end_advance`, `round_end_advance_window`, `upkeep_round_end_at_and_after`,
  `fire_act_round_end_ability`, `round_end_advance_ability_index` (all in `phases.rs`).
- `Act.round_end_advance` field + `RoundEndAdvance` struct (`game_state.rs`).
- The whole **`ForcedContinuation` enum** + `resume_forced_continuation` +
  `TimingEvent::forced_continuation()`; `TimingMode::Forced` loses its payload;
  `open_forced_resolution` loses its `continuation` arg.
- `EndOfTurnAfterForced`'s logic (folded onto `InvestigatorTurn { ending }`) and the
  `is_forced` special-case in `end_turn`.
- The `ResolveInput`/`apply_player_action` routing arm for `ActRoundEnd`.

**Kept:** `upkeep_round_end_teardown` (re-pointed to the `AfterRoundEnd` resume),
`resume_end_turn` (now reached via the `InvestigatorTurn { ending }` loop arm), and
`the_barrier`'s `When` reaction + `round_end_advance` native (now scanned, not hand-fired).

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
- **EndOfTurn unification behaviour-preserving:** the single-suspend (Frozen in Fear skill
  test) and 2+-forced rotation paths now both route through `InvestigatorTurn { ending }`.
  Backstopped by the end-of-turn tests in `phases.rs` (`end_turn_*`) + any
  `EndOfTurn`-forced coverage; assert rotation/phase-end still fire after a suspending
  EndOfTurn forced.
- Match the full CI gauntlet (fmt / clippy / test / doc / wasm) before pushing.

## What "done" looks like

`Continuation::EmitEvent`/`TimingPoint` are `drive`-loop-dispatched coordinator frames; the
`when → at → after` axis is **structural** (the coordinator scans each cell by ability
timing with per-cell re-scan), not hand-threaded; `emit_event` stays the one chokepoint
(inline for single-bucket, coordinator for the multi-bucket round-end, no local driver);
the `ActRoundEnd` machinery + `Act.round_end_advance` field are deleted and round-end
teardown runs on the Upkeep anchor's `AfterRoundEnd` resume; the `EndOfTurn` rotation tail
runs through the `InvestigatorTurn { ending }` loop arm; and **`ForcedContinuation` is
deleted entirely** (forced runs carry no continuation — close → `Done` → the loop
re-dispatches the exposed frame). The §G re-scan has a regression test. Closes #434 / #435;
Slice D (#423) is parallel.

## Open questions

- **Round-end `when` candidate potential-gate.** Whether to suppress the act-advance
  candidate when the group holds 0 clues (a "potential" eligibility check) or always offer
  it and let the native no-op. Default: always offer (RR "may"); fold any suppression into
  the #368 trigger-eligibility gate when its 3rd consumer lands. Non-blocking.
