# EmitEvent-frame arc — decomposition & sequencing

**Status:** scoping design (2026-06-22). Defines the slice boundaries, dependency
order, and issue map for the final control-flow arc of Phase 7's solo-correctness
substrate. The *north-star end-state* is already specified — this doc does not
re-derive it; it turns it into a sequenced, issue-tracked plan and details the
first slice (A) enough to start.

**North-star source:** [`2026-06-20-unified-control-flow-model-design.md`](2026-06-20-unified-control-flow-model-design.md)
§"Named end-states → EmitEvent-frame detail" (lines 337-420). Quotes below are
load-bearing; consult that section for the full rationale.

## Why this arc exists now

[#374](https://github.com/talelburg/eldritch/issues/374) (ST.1/ST.2 skill-test
player windows) shipped (PR #432) and [#64](https://github.com/talelburg/eldritch/issues/64)
(after-resolution window) is deferred (its first real consumer, Rabbit's Foot
01075, isn't gate-required). That clears Phase 7 Ordering step 5 down to its
residual: **[#423](https://github.com/talelburg/eldritch/issues/423)** — migrate
every effect-invocation site off the synchronous `apply_effect` bounded entry so
each resumes via `on_child_pop` under the global `drive` loop.

**#423 is blocked.** A call site's post-effect logic can move into an
`on_child_pop` "under the global drive" only if its **parent frame is one the
`drive` loop dispatches**. The loop (`engine/dispatch/mod.rs:171-211`) dispatches
exactly three: **phase anchors**, **`ActionResolution`**, and **`Effect`**.
Everything else falls through `_ => Done` and is driven imperatively. Cross-
referencing #423's six sites against the frame each one's post-effect logic sits
under:

| Site | Parent frame | Drive-dispatched today? | Unblocked by |
|---|---|---|---|
| `resume_activate_ability` (AoO) | `ActionResolution` | ✅ | — (now) |
| non-fast OnPlay (`complete_play`) | `ActionResolution` (#378) | ✅ | — (now) |
| `fire_pending_trigger` / `play_fast_event` | `Resolution` (window) | ❌ imperative | Slice A |
| `resolve_one` (forced) | `Resolution` (forced) | ❌ imperative | Slice A |
| enemy revelation | `EncounterCard` | ❌ imperative | Slice C |
| skill-test cluster (on_success/fail/OnCommit/OnSkillTestResolution) | `SkillTest` | ❌ imperative | Slice C |

So #423 ride on **making `Resolution`, `SkillTest`, and `EncounterCard` disposal
drive-dispatched** — which is precisely the EmitEvent-frame end-state (the
`Resolution` rework) plus [#431](https://github.com/talelburg/eldritch/issues/431)
(the `SkillTest`/`EncounterCard` rework). The #423 issue itself says the migration
rides on *"reifying the reaction-window/skill-test drivers as frames."* That
reification was **not** done by #374 (which only inserted windows inside the
still-Shape-A `advance`); it is this arc.

## The north-star end-state (recap)

Per the #393 spec, the end-state replaces today's single over-indirected window
frame —
`Continuation::Resolution(ResolutionFrame { pending_triggers, kind: ResolutionKind::{Window(WindowBinding{kind: WindowKind, ..}) | Forced(ForcedContinuation)} })`
— with **two purpose-built window frames plus two coordinator frames**:

- **`Continuation::FastWindow { candidates, fast_actors }`** — the red-box
  framework player window. *No* `PhaseStep`: the `*Phase` anchor directly beneath
  it already carries "where in the flow." Anchor-pushed. (Absorbs
  `WindowKind::PlayerWindow(PhaseStep)` **and** `WindowKind::SkillTestPlayerWindow`
  — **the "fast-window simplification."**)
- **`Continuation::TimingPointWindow { event: TimingEvent, mode: Reaction|Forced, candidates }`**
  — one flat variant for **all** event windows, parameterized by the existing
  `emit::TimingEvent`. *"The forced run is not a separate frame — it is a `mode` on
  `TimingPointWindow`."* `ResolutionKind::{Window | Forced}` collapses to
  `mode: Reaction | Forced` (two passes of one frame: forced drained, then
  reaction).
- **`Continuation::EmitEvent { event, bucket }`** — iterates `when → at → after`
  (the unfinished tail of #212).
- **`Continuation::TimingPoint { event, bucket, sub }`** — for one bucket, runs
  `forced → reaction`, *"exactly what T5a's `emit_event` does today, made
  frame-resumable."*

Deleted at the end: `WindowKind`, `ResolutionKind`, `WindowBinding`,
`ResolutionFrame`, `Continuation::Resolution`.

**Correctness caution (the matrix):** the six cells
(`when-forced → when-reaction → at-forced → at-reaction → after-forced → after-reaction`)
must be evaluated **in order with eligibility re-checked at each cell** — a `when`
reaction can change whether an `at` forced even fires — so the grid is *not*
pre-computed. The nested frames make "enter each cell fresh, re-scan" structural.

## Slice decomposition

```
A. Window-taxonomy rework         Resolution → FastWindow + TimingPointWindow{event,mode}
   (#212-successor child A)         + drive-loop arms (top-frame resume) for both
        │                          ⇒ unblocks #423's reaction/forced/OnPlay effect sites
        ▼
B. EmitEvent + TimingPoint        the when→at→after × forced→reaction matrix as nested
   coordinators                    coordinator frames; per-cell eligibility re-scan
   (#212-successor child B)        ⇒ fixes the hand-threaded ordering smell (§G class)
        │
        ▼
C. #431  loop-driven              SkillTest drive-loop arm + EncounterCard disposal
   skill-test/encounter            loop-driven + retire the 5 synchronous re-entry sites
        │                         ⇒ unblocks #423's skill-test/revelation effect sites
        ▼
D. #423  effect call-site         every apply_effect site → push root Effect frame +
   migration                       on_child_pop; reduce apply_effect to test-only/remove
```

**Dependency order:** A → B (coordinators push the new window frames) · A → C (windows
drive-dispatched lets a mid-test reaction resume through the loop, not the imperative
re-entry) · (A, C) → D. **A is the keystone**; B is separable-after-A; C and D ride on A.

**Already done:** the §G Upkeep `when→at` round-end ordering bug (the matrix's
motivating bug) shipped standalone as PR #396 (closes #395) — not part of this arc's
remaining work.

**Coupling to flag:** the window-close → skill-test-resume seam spans A↔C. Slice A
gives the new window frames their `drive`-loop arms but **leaves the skill-test
re-entry sites alone** (a window that closes while a test is mid-resolution still
re-enters `advance` imperatively); Slice C (#431) is where those five re-entry sites
+ the `resolve_input` encounter-disposal chokepoint retire. Keeping that seam on the
old path through A is what keeps each A sub-slice behaviour-preserving.

## Slice A detail (the first chunk)

**Goal:** replace `Continuation::Resolution` with `FastWindow` + `TimingPointWindow`,
both drive-dispatched, **behaviour-preserving**. This is largely mechanical (the
`WindowKind` event variants are *"a near-duplicate"* of `emit::TimingEvent`, so this
is *"de-duplication, not a rename"*) but high-blast-radius — every window site touches
it. Sub-slice it; each sub-slice is independently green.

**Current shape (verified):**
- `emit_event` (`emit.rs:302`) pushes windows via `queue_reaction_window(kind: WindowKind)`
  (→ `Resolution{Window}`) and `open_forced_resolution(..)` (→ `Resolution{Forced}`).
- `resolve_input` routes `Resolution` → `resume_window` → on pick `fire_pending_trigger`
  → `apply_effect` (synchronous) → `advance_resolution` (re-prompt or `close_reaction_window_at`).
- `close_reaction_window_at` runs `run_window_continuation(kind)` (Window) or
  `resume_forced_continuation(cont)` (Forced), then may re-enter `skill_test::advance`.

**Target shape:**
- `Continuation::TimingPointWindow { event: TimingEvent, mode, candidates }` replaces
  `Resolution` for **event windows + the forced run**.
- `Continuation::FastWindow { candidates, fast_actors }` replaces the **framework
  player windows** (`PlayerWindow`, `SkillTestPlayerWindow`); the dead `PhaseStep`
  discriminant is dropped.
- The `drive` loop gains arms for both: on a child pop, the loop dispatches the
  window's resume (advance to next candidate, or close + run continuation) — top-frame
  dispatch replacing the window-side imperative re-entry **that does not touch
  skill-test** (the skill-test seam stays on the old path until C).

**Sub-slices (proposed; the per-sub-slice plan is writing-plans' job):**

- **A-i — `TimingPointWindow` replaces event windows + forced run.** Map each event
  `WindowKind` variant → the existing `emit::TimingEvent`; collapse
  `ResolutionKind::{Window(event) | Forced}` → `TimingPointWindow{event, mode}`.
  `ForcedContinuation` handling rides as the `mode: Forced` close path. **Imperative
  driving preserved** (no `drive` arm yet) — pure data-structure swap, behaviour-
  preserving. Largest mechanical PR. **This is the first chunk to start.**
- **A-ii — `FastWindow` replaces framework player windows.** `WindowKind::PlayerWindow`
  + `SkillTestPlayerWindow` → `Continuation::FastWindow { candidates, fast_actors, kind:
  FastWindowKind }`, where `FastWindowKind = Phase(PhaseStep) | SkillTest { before_token }`.
  **Re-sliced (see "WindowKind's two roles" below): the discriminant is kept, not
  dropped.** It reproduces the exact `WindowKind` for the `Event::WindowOpened/Closed`
  payload (so the event log is byte-identical) and routes the close continuation
  (`Phase → anchor_on_child_pop`, `SkillTest → skill_test::advance`). Behaviour-preserving.
  After A-ii, `Continuation::Resolution` is unused.
- **A-iii — delete the frame-level legacy taxonomy.** Remove `Continuation::Resolution`,
  `ResolutionFrame`, `ResolutionKind`, `WindowBinding` once A-i/A-ii cover every case.
  **Keep `WindowKind`** as the pure `Event::WindowOpened/Closed` observability descriptor,
  derived from `TimingPointWindow`'s `TimingEvent` (`reaction_window()`) + `FastWindow`'s
  `FastWindowKind`. Behaviour-preserving — event log byte-identical.
- **A-iv — `drive`-loop arms → folded into Slice C (#431).** Giving the `drive`
  loop top-frame-dispatch arms for `TimingPointWindow` / `FastWindow` (retiring the
  window-side imperative re-entry) is the same concern as Slice C's loop-driving (the
  `SkillTest` drive arm + retiring the five synchronous re-entry sites + the
  encounter-card disposal seam). Rather than split the imperative re-entry across two
  slices, **A-iv moves to Slice C.** So **Slice A (#433) = A-i/A-ii/A-iii** — the
  taxonomy rework — and closes when A-iii lands; the windows stay imperatively driven
  (via `advance_resolution` / `close_reaction_window_at`) until Slice C makes the loop
  drive everything.

**WindowKind's two roles — and the deferral into Slice B (load-bearing reference for the
EmitEvent-frame / coordinator work).** `WindowKind` is load-bearing in *two independent*
roles: (1) the **frame representation** (`Resolution{Window(WindowBinding{kind})}`, which
A-i/A-ii migrate off), and (2) the **`Event::WindowOpened/Closed` descriptor** (observability;
~46 test assertions depend on it, e.g. `phases.rs` asserts `PlayerWindow(PhaseStep::…)`).
The #393 end-state deletes `WindowKind` entirely and has `WindowOpened/Closed` observability
*read flow-position from the anchor's `resume`* rather than carry a kind — **but that changes
the observable event log**, the only genuine *behaviour* change in the taxonomy rework. To
keep all of Slice A behaviour-preserving, **Slice A deletes only the frame-level taxonomy and
keeps `WindowKind` alive as the event descriptor.** **Deferred to Slice B (#434) / the
EmitEvent-frame coordinator work:** delete `WindowKind` outright and redesign
`Event::WindowOpened/Closed` off it (read-from-anchor observability, drop `PhaseStep`). When
implementing Slice B, this is the place that change lands — it is *not* done in Slice A.

**Acceptance (Slice A):**
- [x] `Continuation::Resolution`, `ResolutionFrame`, `ResolutionKind`, `WindowBinding` are
  gone; windows are `FastWindow` + `TimingPointWindow{event, mode}`. `WindowKind` survives
  **only** as the `Event::WindowOpened/Closed` descriptor (its deletion is Slice B).
- [x] Behaviour-preserving throughout — **event log byte-identical at every sub-slice
  boundary** (no `WindowOpened/Closed` payload change in Slice A).

**Slice A is complete (A-i/A-ii/A-iii shipped: PRs #436, #437, #438, #439).** The `drive`-loop
arms for the window frames (former A-iv) moved to **Slice C** — see below.

## Slice B / C / D scope (sketches; each gets its own plan when started)

- **B — coordinators (the matrix).** Introduce `EmitEvent { event, bucket }` +
  `TimingPoint { event, bucket, sub }`; `emit_event` becomes pushing the `EmitEvent`
  coordinator, which drives `when → at → after`, each bucket running `forced →
  reaction` with **per-cell eligibility re-scan**. New regression tests for the
  ordering (the §G class). Acceptance: the `when/at/after` axis is frame-driven, not
  hand-threaded per site; a `when`-reaction-changes-an-`at`-forced case is covered.
  **Inherited from Slice A (see "WindowKind's two roles"):** Slice A deliberately kept
  `WindowKind` alive as the `Event::WindowOpened/Closed` descriptor. **Slice B owns its
  deletion** + the observability redesign — `WindowOpened/Closed` reads flow-position
  from the anchor's `resume` / the `TimingEvent`, drops `PhaseStep`, and stops carrying
  `WindowKind`. This is the one event-log *behaviour* change of the taxonomy rework; it
  rides Slice B's coordinator work (the `EmitEvent`/`TimingPoint` frames are where the
  window's timing context already lives), not Slice A. Touches the ~46 `WindowOpened/Closed`
  test assertions.
- **C — [#431](https://github.com/talelburg/eldritch/issues/431).** Make `EncounterCard`
  disposal loop/frame-driven (retire the `resolve_input` chokepoint), add the `drive`-loop
  `SkillTest` arm, retire the five synchronous skill-test re-entry sites
  (`close_reaction_window_at`, `resume_before_discover_window`, `resume_effect_walk`,
  the `finish_attack_loop` Retaliate path, the commit hop). Backstopped by
  `crates/cards/tests/revelation_treacheries.rs` (Crypt Chill / Grasping Hands).
  **Folded in from Slice A (former A-iv):** give the `drive` loop top-frame-dispatch
  arms for `TimingPointWindow` / `FastWindow` and retire the window-side imperative
  re-entry (`advance_resolution` / `close_reaction_window_at` reaching down the stack)
  — the same "make the loop drive every frame" concern as the `SkillTest` arm, so it
  belongs here rather than split across Slice A. Slice A left the windows imperatively
  driven; C makes them top-frame-dispatched.
- **D — [#423](https://github.com/talelburg/eldritch/issues/423).** With `TimingPointWindow`
  (A) and `SkillTest` (C) drive-dispatched, migrate every `apply_effect` site to push a
  root `Effect` frame + move post-effect logic into the parent frame's `on_child_pop`;
  reduce `apply_effect` / `drive_effect_to_base` to test-only or remove. Issue acceptance
  already crisp.

## Issue map

| Slice | Issue | State |
|---|---|---|
| A + B umbrella | [#435](https://github.com/talelburg/eldritch/issues/435) "EmitEvent-frame end-state" | filed |
| A | [#433](https://github.com/talelburg/eldritch/issues/433) "EmitEvent-frame A: window-taxonomy rework" | filed |
| B | [#434](https://github.com/talelburg/eldritch/issues/434) "EmitEvent-frame B: coordinators" | filed |
| C | [#431](https://github.com/talelburg/eldritch/issues/431) | ✅ done — re-entry retirement complete (commit hop + substitution resume); A-iv window arms + encounter disposal landed in C-plumbing |
| D | [#423](https://github.com/talelburg/eldritch/issues/423) | open, keep as-is |

## Testing strategy

- **Behaviour-preserving for A + C + D.** The full engine + integration suite stays
  green through every sub-slice (these change *structure*, not rules). Per-sub-slice
  conversions are individually green at each PR boundary.
- **New behaviour for B only.** The matrix ordering is the one place new rules behavior
  lands; it gets dedicated ordering regression tests (the §G class: a `when` reaction
  that changes an `at` forced's eligibility).
- **C's backstop:** `revelation_treacheries` (Crypt Chill / Grasping Hands) must stay
  green — it guards the encounter-card disposal seam #431 reworks.
- **D's invariant:** no effect-invocation site calls the synchronous `apply_effect`
  bounded entry; each pushes a root effect frame and resumes via `on_child_pop`.

## What "done" looks like

The global `drive` loop drives **every** frame — windows, skill tests, encounter-card
disposal, and effect walks — by uniform top-frame dispatch; `emit_event` is a thin
coordinator-push; `apply_effect` is gone (or test-only); the `when/at/after ×
forced/reaction` matrix is structural. #423's acceptance is met as a consequence.
This completes the #393 unified control-flow model's effect/timing end-state.
