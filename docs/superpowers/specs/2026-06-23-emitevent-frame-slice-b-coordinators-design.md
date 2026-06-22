# EmitEvent-frame Slice B — `EmitEvent`/`TimingPoint` coordinators

**Status:** design (2026-06-23). Slice B of the EmitEvent-frame arc
([#434](https://github.com/talelburg/eldritch/issues/434), umbrella
[#435](https://github.com/talelburg/eldritch/issues/435)). Follows Slice A
(#433, PRs #436–#439); precedes Slice C (#431) + D (#423).

**Parent specs (load-bearing):**
- Arc decomposition: [`2026-06-22-emitevent-frame-arc-decomposition-design.md`](2026-06-22-emitevent-frame-arc-decomposition-design.md)
- North-star: [`2026-06-20-unified-control-flow-model-design.md`](2026-06-20-unified-control-flow-model-design.md)
  §"Named end-states → EmitEvent-frame detail".

**Scope note vs. the arc decomposition:** that spec sketched Slice B as "`EmitEvent` +
`TimingPoint` coordinators + the inherited `WindowKind` deletion." This design **expands**
it with a DSL timing rework (`EventTiming::{When, At, After}`) — the enabler that lets the
coordinator scan buckets uniformly and dissolves the round-end framework special-case
rather than relocating it. The expansion is deliberate (see "Why the DSL rework").

## Problem

`emit_event` (the T5a chokepoint, PR #342) models the RR p.2 **forced → reaction** axis:
it queues the reaction window, then resolves forced abilities (0/1 synchronously, 2+ via
the #213 lead-ordering run). That axis is structural.

The orthogonal RR "At" axis — **`when` → `at` → `after`** — is **not** structural, for a
root reason at the *ability* level: `EventTiming` is only `Before | After`, and the forced
scanner fires on `After` only (`forced_triggers.rs:379`: `if *timing == EventTiming::After`).
So "at the end of the round" abilities (agenda 01107's doom, Dissonant Voices 01165) are
tagged `After`, indistinguishable from genuine after-event reactions — the `when/at/after`
axis is **collapsed**. With no bucket to read, ordering can only be hand-threaded.

It is hand-threaded at exactly one site: the upkeep round-end. There, two buckets coincide,
with content from two *different* mechanisms:

- **`when` the round ends** → act 01109's clue-spend advancement, today a bespoke framework
  field `Act.round_end_advance` read by `upkeep_phase_end`, resolved through
  `Continuation::ActRoundEnd(ActRoundEndPending)` + `resume_act_round_end_advance`.
- **`at` the end of the round** → `emit_event(TimingEvent::RoundEnded)`, the agenda 01107
  doom — a **registry forced ability** scanned via `EventPattern::RoundEnded` (tagged
  `After`).

`phases.rs` hand-threads them: `upkeep_phase_end` opens the act window, then
`resume_act_round_end_advance` calls `upkeep_round_end_at_and_after` →
`emit_event(RoundEnded)` → `upkeep_round_end_teardown`. **That hand-thread is the "#212
smell," and getting its order backwards was the §G bug** (fixed standalone in PR #396 by
reordering those calls — but the ordering stays hand-enforced, not structural).

Every *other* `emit_event` site is single-bucket: each `TimingEvent` sits in one fixed
bucket, so there is no ordering to thread.

**Two asymmetries this slice resolves:**
1. The `at` cell (agenda doom) is a scanned registry ability; the `when` cell (act
   advancement) is a hardcoded framework field. Even within the act crate this is
   inconsistent — act 01110's advancement is *already* a registry ability
   (`cards::what_have_you_done`'s Forced `EnemyDefeated`); only 01109's is a field.
2. The `when/at/after` ordering is hand-enforced per site rather than structural.

## Goal

Reify the `when → at → after × forced → reaction` matrix as two coordinator frames, with the
bucket made first-class at the ability level so the coordinator scans every cell uniformly.
(The frames are introduced here but driven imperatively; their `drive`-loop dispatch is
Slice C.) Behaviour-preserving except for: (a) the per-cell eligibility re-scan (new ordering
correctness, only changes outcomes in cases no in-scope card hits — synthetic regression
test), and (b) the deliberate `WindowKind` event-log change (B-iii).

## Why the DSL rework (`EventTiming::{When, At, After}`)

The alternative — keeping `round_end_advance` a framework field and special-casing the
round-end `when` cell in the coordinator — **relocates** the irregularity (a heterogeneous
coordinator cell, or a new `CandidateSource` variant for a framework option) rather than
removing it. The clean fix is to give abilities a bucket to self-declare, so act 01109 and
agenda 01107 each name their own timing and the coordinator scans them identically.

**The rework:**
- `EventTiming::Before` → **`When`** (rename; the cancel/replacement interrupt timing *is*
  the RR `when ... would` bucket — Dodge, Cover Up). ~15 sites, mechanical.
- Add **`At`** (new; "at the end of the round" — between `when` and `after`).
- Result: `EventTiming = { When, At, After }`, matching the RR ordering exactly.

**Cancel-vs-when subtlety (not a blocker):** a `When`-timed ability may or may not cancel
the event. Dodge (`When` + `Effect::Cancel`) cancels; act 01109 (`When`, no cancel) does
not. The cancel behaviour is a property of the *effect* and the specific Before-timing
points (`EnemyAttacks`, `WouldDiscoverClues` open cancel windows), **not** of the timing
label — so the rename is safe; `When` does not imply "cancels."

**Forced scanning becomes bucket-aware:** the `timing == After` filter generalizes to "the
bucket currently being scanned" — `forced_point(bucket)` collects forced abilities whose
`EventTiming == bucket`; `reaction_window(bucket)` likewise for reactions. This is the
mechanism that lets the coordinator place each ability in its cell.

The DSL exposing `When/At/After` is *not* a speculative primitive: it names the RR timing
axis the engine already needs to order correctly, with live consumers in both the `When`
(Dodge/Cover Up, 15 sites) and `At` (round-end doom) buckets.

## Data model

The coordinator's bucket cursor **reuses `EventTiming`** directly (game-core already depends
on and re-exports `card_dsl`) — no parallel enum; "an ability's timing *is* its bucket" is
then an identity, not a mapping.

```rust
enum TimingSub { Forced, Reaction }

/// Coordinator: iterate When → At → After for one game event. `bucket` is the cursor.
Continuation::EmitEvent { event: TimingEvent, bucket: EventTiming }

/// Coordinator: one bucket, run forced then reaction. `sub` is the cursor.
Continuation::TimingPoint { event: TimingEvent, bucket: EventTiming, sub: TimingSub }
```

`emit_event(event)` becomes: **push `EmitEvent { event, bucket: When }` and return** — the
`drive` loop takes over.

**`EmitEvent` dispatch (on a child pop):**
1. Re-scan eligibility *for the current bucket* (board state may have changed in the prior
   bucket — see "Per-cell re-scan").
2. If the bucket is populated, push `TimingPoint { event, bucket, sub: Forced }`, yield.
3. On the `TimingPoint` child's pop, advance the cursor `When → At → After`; repeat.
4. After `After` completes, pop `EmitEvent`.

**`TimingPoint` dispatch:** runs `sub: Forced` (scan abilities of `EventTiming == bucket` +
the #213 lead-ordering run for 2+), then `sub: Reaction` (the `TimingPointWindow` from
Slice A, scanning reactions of `EventTiming == bucket`), then pops — "exactly what T5a's
`emit_event` does today" parameterized by bucket.

**Driving (this slice): imperative, not `drive`-loop.** Like Slice A's windows, the
coordinators are introduced as frames but resumed by the existing imperative entry points
(`open_queued_reaction_window` / `close_reaction_window_at`), **not** by a `drive`-loop arm —
that dispatch is Slice C. Critically, `emit_event`'s forced/reaction *internals* are
preserved verbatim: the reaction window is still **queued** (logging `WindowOpened`) before
forced abilities resolve, and **opened later** at the existing deferred sites. The
coordinators wrap the *bucket iteration* around that unchanged core, so the event log is
byte-identical for every single-bucket event.

**Bucket population** is `forced_point(bucket)` / `reaction_window(bucket)` (today's
functions gain a bucket parameter, filtering scanned abilities by `EventTiming`). Every
existing event populates exactly one bucket, so its `EmitEvent` is a degenerate single-cell
iteration — behaviour-identical to today; round-end is the sole multi-cell case.

## The round-end composite — uniform remodel (no special-case)

With the bucket first-class, the round-end `when` and `at` cells are both **scanned registry
abilities**; the coordinator dispatches them identically.

- **`when` cell — act 01109 advancement, remodeled to a registry ability.** Act 01109 gains
  an `abilities()` impl: a `When`-timed `OnEvent { RoundEnded }` **reaction** (the printed
  "investigators … *may* … spend clues to advance" — optional ⇒ reaction) whose effect is
  `Effect::Native { tag: "act_round_end_advance" }`. The native effect reuses the existing
  group clue-spend (`spend_clues_from(contributors, threshold)` + advance); the
  `ActRoundEndPending` logic moves *into* it, surfaced as the `when`-bucket reaction window's
  candidate (a group Confirm/Skip). The contributor location (the Hallway, 01112) is printed
  on 01109, so it is card text the ability carries — not scenario plumbing.
- **`at` cell — agenda 01107 doom + Dissonant Voices 01165**, re-tagged `After → At` (RR
  "at the end of the round"). Behaviour-preserving: they fire at the same point, after the
  `when` cell, with no competing `After`-RoundEnded ability in scope.
- **Delete** `Act.round_end_advance`, `RoundEndAdvance`, `Continuation::ActRoundEnd` /
  `ActRoundEndPending`, `resume_act_round_end_advance`, and the `round_end_advance_window`
  check in `upkeep_phase_end`. The framework wart is **removed**, not relocated.
- `upkeep_phase_end` becomes: emit `PhaseEnded { Upkeep }`, push the `RoundEnded` `EmitEvent`
  coordinator; `upkeep_round_end_teardown` (expire until-end-of-round effects, Upkeep →
  Mythos) runs after the coordinator pops.

## Per-cell eligibility re-scan (the one new-behaviour correctness)

The cursor re-scans eligibility *entering each cell*: a `when`-cell that mutates board state
can change whether the `at`-cell fires — "a `when` reaction can change whether an `at`
forced even fires," so the grid is **not** pre-computed. The nested frames make "enter each
cell fresh, re-scan" structural.

The conceptual precedent already in the engine: a `When` cancel (Dodge) suppresses the
event, so downstream `At`/`After` cells never fire — same shape. But no in-scope card
exercises a *cross-bucket suppression at one round-end emit*, so this is covered by a
**synthetic test-only act/agenda fixture** (the §G class): a `when`-cell advance that flips
an `at`-cell forced ability's eligibility, asserting the `at` forced does *not* fire after
the `when` cell changes its precondition. Does not wait on a corpus card.

## `WindowKind` deletion + observability redesign (inherited from Slice A)

Slice A kept `WindowKind` alive *only* as the `Event::WindowOpened/Closed` descriptor
(deleting it changes the observable event log — the one genuine behaviour change of the
taxonomy rework, deferred here). Slice B owns it:

- **Event windows** → `WindowOpened/Closed` carries the `TimingEvent` (+ `mode`/bucket),
  which the `TimingPoint` frame already holds; the per-window payload (which enemy /
  investigator / count) is derivable from it.
- **Fast windows** → flow-position read from the anchor beneath; **`PhaseStep` dropped** from
  the payload.
- `WindowKind` is deleted outright.

**Deliberate event-log fidelity change** (accepted): the log loses the explicit per-step
fast-window label (`PlayerWindow(MythosAfterDraws)` etc.); it becomes derivable-from-anchor.
Touches the ~46 `WindowOpened/Closed` test assertions. Irreversible-ish for replay
consumers — hence its own sub-slice.

## Sub-slicing (each independently green)

> **Re-sliced 2026-06-23 (after reading the real emit/window machinery).** An earlier draft
> split the coordinators into a standalone `TimingPoint`-frame PR (old B-ii) ahead of
> `EmitEvent` (old B-iii). That boundary does **not** hold: reaction-window *opening* is
> deliberately deferred across ~6 framework sites (`open_queued_reaction_window` in
> `combat.rs` / `skill_test.rs` / `cards.rs` / `evaluator.rs`), and `WindowOpened` is logged
> at *queue* time **before** forced abilities resolve (`emit_event` doc: *"WindowOpened is
> emitted before the forced effects' events"*) — both load-bearing for the event log. A
> `TimingPoint` that genuinely owns "forced *then* reaction" would have to own reaction-window
> opening, which is entangled with the loop-driving **Slice C** owns; a naive
> `Forced`-sub→`Reaction`-sub frame would emit forced events before `WindowOpened`, changing
> the log. So the coordinators land **together** (restoring the arc decomposition's original
> "EmitEvent + TimingPoint = one slice" grouping), keeping `emit_event`'s forced/reaction
> internals — queue-then-defer-open, `WindowOpened`-at-queue — **exactly as today**.

- **B-i — DSL timing rework.** ✅ shipped (PR #440). `EventTiming::Before → When` (rename, 14
  sites) + dormant `At` variant. Behaviour-preserving.
- **B-ii — the coordinators + round-end remodel.** Introduce `Continuation::EmitEvent` +
  `Continuation::TimingPoint` (the bucket axis), **driven imperatively** — like Slice A left
  its windows; the `drive`-loop dispatch of `TimingPoint`/windows stays in Slice C. Make
  `forced_point`/`reaction_window` bucket-parameterized; remodel act 01109 as a `When`
  reaction ability + re-tag the round-end doom abilities to `At` (deleting
  `round_end_advance` / `ActRoundEnd`); per-cell re-scan; the synthetic §G regression test.
  `emit_event`'s forced/reaction internals (queue-then-defer-open, `WindowOpened`-at-queue)
  are unchanged. Behaviour-preserving **except** the §G re-scan (new ordering correctness).
- **B-iii — `WindowKind` deletion + observability redesign.** Delete `WindowKind`; redesign
  `WindowOpened/Closed` (read-from-anchor, drop `PhaseStep`); update the ~46 assertions. The
  event-log-change PR.

## Testing strategy

- **B-i / B-iii: behaviour-preserving** (B-iii changes window *payload*, no game outcome).
  Full engine + integration suite green at each boundary.
- **B-ii: new behaviour** in the §G re-scan only — covered by the synthetic act/agenda
  fixture. The bucket-iteration wrapper and the 01109 remodel are behaviour-preserving (same
  group spend, same `when→at` order, byte-identical event log for single-bucket events),
  backstopped by `crates/game-core/tests/act_round_end.rs`,
  `crates/cards/tests/act_advancement.rs`, `crates/cards/tests/the_barrier.rs`,
  `crates/game-core/tests/reaction_windows.rs`, `crates/game-core/tests/forced_triggers.rs`,
  and `crates/scenarios/tests/the_gathering*.rs`.
- Match the full CI gauntlet (fmt / clippy / test / doc / wasm) before each push.

## What "done" looks like

`EventTiming` is `When | At | After`; the `when/at/after` axis is frame-driven
(`EmitEvent`/`TimingPoint` coordinators scanning each cell by ability timing), not
hand-threaded per emit site; per-cell eligibility is re-scanned with a regression test; act
01109's round-end advancement is a `When` registry ability (no `round_end_advance` framework
field), and the round-end doom is `At`; `WindowKind` is deleted and `WindowOpened/Closed`
reads flow-position from the anchor. Unblocks Slice C (#431) and D (#423).

## Open questions

None blocking. If a second round-end-window card appears (Dunwich+), revisit whether the
group clue-spend native effect warrants promotion to a shared `Effect` variant (the repo's
≥2-reuse rule); single consumer today, so it stays `Effect::Native`.
