# EmitEvent-frame Slice C — loop-driven windows / skill-test / encounter + coordinators

**Status:** design (2026-06-23). Slice C of the EmitEvent-frame arc
([#431](https://github.com/talelburg/eldritch/issues/431), umbrella
[#435](https://github.com/talelburg/eldritch/issues/435)). Follows Slice A
(#433, PRs #436–#439) and Slice B (#440–#442); unblocks Slice D
([#423](https://github.com/talelburg/eldritch/issues/423)).

**Parent specs (load-bearing):**
- Arc decomposition: [`2026-06-22-emitevent-frame-arc-decomposition-design.md`](2026-06-22-emitevent-frame-arc-decomposition-design.md)
- Slice B coordinators: [`2026-06-23-emitevent-frame-slice-b-coordinators-design.md`](2026-06-23-emitevent-frame-slice-b-coordinators-design.md)
- North-star: [`2026-06-20-unified-control-flow-model-design.md`](2026-06-20-unified-control-flow-model-design.md)
  §"Named end-states → EmitEvent-frame detail".

**Scope (per the 2026-06-23 brainstorm):** the full Slice C arc — both the
behaviour-preserving plumbing **and** the `EmitEvent`/`TimingPoint` coordinator
frames deferred here from Slice B — sequenced so the coordinators land last,
behind the plumbing that unblocks #423.

## The core mechanical shift

Today `resolve_input` (`dispatch/mod.rs:391`) dispatches on the **top** continuation
frame and the handler runs the **entire** cascade synchronously, never returning to
`drive`. That synchronous cascade *is* the imperative machinery this slice retires:
a window-close reaches *down* the stack and calls `skill_test::advance`; the
`EncounterCard` disposal sits as a chokepoint at `resolve_input`'s tail
(`mod.rs:484`, `teardown_encounter_card_if_top`).

`drive` (`dispatch/mod.rs:166`) already dispatches three frame kinds by uniform
top-frame dispatch — **phase anchors**, **`ActionResolution`**, **`Effect`** —
and falls through `_ => Done` for everything else. Slice C extends that loop to
drive **every** frame: it gains arms for `TimingPointWindow`, `FastWindow`,
`SkillTest`, and `EncounterCard`, and `resolve_input` routes its dispatch back
through `drive`.

The inversion: **each resume handler steps once and returns `Done`; the loop
re-dispatches whatever frame is now on top.** "What runs next" becomes "the loop
dispatches the top frame," not "the handler reaches down and calls it." The five
synchronous re-entry sites and the disposal chokepoint dissolve as a consequence.

### The load-bearing invariant

The whole conversion rests on one invariant the engine *already* nearly maintains:

> **The continuation stack is the resolution order — `continuations.last()` is
> always what resolves or awaits next.**

Under it, `match continuations.last()` is sufficient and unambiguous, and the engine
already has the predicate that disambiguates the only confusing case — a window frame
that *looks* the same whether it's a mandatory prompt or a permissive gate:
`Continuation::awaits_input()` (`game_state.rs:756`).

- **window with pending candidates** → `awaits_input() == true` → a mandatory prompt
  → the loop **advances/resolves it**.
- **empty `FastWindow` gate** → `awaits_input() == false` → a *permissive* Fast-play
  opportunity → an **idle state**, exactly like the open turn. The loop leaves it; the
  player acts via a typed Fast `PlayCard`/`ActivateAbility` or `ResolveInput::Skip`.

So the loop rule is: dispatch `last()`; advance it if it is a phase-anchor /
`ActionResolution` / `Effect` / `SkillTest` / window-with-candidates; **idle** (return
`Done`) if it is `InvestigatorTurn` / an empty-`FastWindow` gate / the empty stack.

**What disappears — and what stays (revised at implementation).** Dispatch and the
window drivers now read **only the top frame**. The genuine *driver self-location* —
`advance`'s `rposition(SkillTest)` + `win_idx > st` — **disappears** (every driver
returns `Done` and the loop dispatches `last()`); and so does the *index threading*:
`resume_reaction_window` / `fire_pending_trigger` operate on `continuations.last()` /
`last_mut()`, `close_reaction_window` `pop()`s the top (no `idx` parameter, no
`window_idx`), and `resume_window` routes on whether the top frame has candidates. The
empty-skipping accessors **`top_reaction_window_index` and `top_reaction_window_mut`
are deleted**. `top_reaction_window` survives **only as a test-inspection accessor**
("is a reaction window open?" in `crates/*/tests`), not in engine control flow. The
two `cards`/`evaluator` "open the window I just queued" sites read `last()` (the
just-pushed window). The legitimate non-top read that stays is **context, not
dispatch**: `current_skill_test` (an effect/resolution step reading its enclosing test
to bind evaluation context). The two genuinely-stacked cases stay correct because they
are **already in resolution order**:

- *Forced/reaction ability starts a skill test* (Frozen in Fear 01164):
  `[forced-run(siblings), SkillTest, ST.1-gate]` — top-first dispatch resolves ST.1 →
  SkillTest → then the forced run resumes its remaining siblings. Correct as-is.
- *A reaction window queued mid-emit while forced resolves into a skill test*:
  `[reaction-window(queued), SkillTest, ST.1-gate]` — RR p.2 forced-before-reaction is
  preserved (the forced skill test, on top, resolves before the queued reaction
  window beneath it opens).

The one shape that *would* break top dispatch — an empty gate sitting **above** a
still-pending mandatory window (`[reaction(pending), gate(empty)]`) — **cannot arise in
play**: a pending mandatory window has `awaits_input() == true`, which gates the
framework from advancing to open a gate (`apply`'s guard, `mod.rs:72`), and the loop
dispatches a pending window rather than advancing past it. The former regression test
`close_reaction_window_at_removes_reaction_window_not_empty_phase_gate_on_top`
hand-`push`ed that gate to defend the old reach-down close; with pure top-frame dispatch
a `Skip` acts on the top (the gate), so the test was **replaced** by a positive
invariant test, `active_reaction_window_is_the_top_continuation_frame`, asserting an
open reaction window is always `continuations.last()` (see Testing). The window arm is
still **guarded by `awaits_input()`** so the loop idles on a permissive empty gate
rather than draining it — that guard is load-bearing regardless.

## Sub-slice decomposition

Three PRs, strictly ordered. The first is the behaviour-preserving plumbing that
unblocks #423; the second is the deferred-from-B coordinator work (the one slice
carrying new behaviour); D is #423 itself, forking after the plumbing.

```
C-plumbing   Loop drives every frame   (atomic)   behaviour-preserving  ← unblocks #423
   └ resolve_input dispatches top frame then returns through drive; drive gains
     arms for TimingPointWindow / FastWindow / SkillTest / EncounterCard; every
     driver returns Done to the loop instead of reaching down; eliminate the
     reach-down accessors (top_reaction_window_index, advance's win_idx>st) +
     the 5 re-entry sites + the resolve_input EncounterCard chokepoint
        │
        ├──────────────► D (#423) effect call-site migration   (may start here)
        ▼
C-coordinators   EmitEvent / TimingPoint            NEW BEHAVIOUR (per-cell re-scan,
   └ emit_event → push EmitEvent + return; when→at→after × forced→reaction          reaction-after-forced scan)
     as nested loop-driven frames; per-cell eligibility re-scan + §G test;
     retire queue-then-defer-open + the ~6 deferred open sites
```

### Why the plumbing is one atomic slice (not C-i then C-ii)

An earlier draft split this into "C-i window arms" then "C-ii skill-test/encounter."
That boundary does **not** hold. Eliminating the reach-down drivers is **holistic**:
you cannot make windows pure-top-frame-dispatched while `advance` still reaches down
(`win_idx > st`), or vice versa, because the two interleave on one stack (the
Frozen-in-Fear case: `[forced-run, SkillTest, gate]`). A half-conversion leaves a
driver reaching past a frame the loop now owns — exactly the contradiction that made
the "generic window arm + kept reach-down accessors" attempt fail its own regression
test. So windows, skill-test, and encounter convert **together**: one PR that flips
every driver to return-to-loop and deletes the reach-down accessors in the same move.

### Dependency spine (why this order)

- **C-plumbing is behaviour-preserving and unblocks #423.** Its four blocked sites —
  `fire_pending_trigger` / `play_fast_event` (under a window), the forced `resolve_one`,
  the enemy revelation (under `EncounterCard`), the skill-test cluster (under
  `SkillTest`) — are all loop-dispatched once it lands.
- **C-coordinators last.** `emit_event` becoming a thin push-and-return only works once
  every caller's parent frame is loop-driven (the plumbing). It is also the one slice
  with new behaviour (per-cell re-scan + the reaction-after-forced scan), kept isolated
  behind the plumbing per the brainstorm's scoping decision.
- **D forks after the plumbing.** It does **not** wait on C-coordinators.

## C-plumbing — loop drives every frame (atomic)

**Goal:** establish the stack invariant (above) and make the `drive` loop dispatch
**every** frame by uniform top-frame dispatch — windows, skill tests, and
encounter-card disposal — removing the genuine driver self-location/reach-downs, the
five synchronous skill-test re-entry sites, and the `resolve_input` disposal
chokepoint. **Behaviour-preserving** — full suite green; the one test delta is the
synthetic gate-above-reaction regression, replaced by a positive invariant test. This
unblocks #423.

**The loop (`drive`) gains arms** for `Continuation::TimingPointWindow`,
`FastWindow`, `SkillTest`, and `EncounterCard`, dispatched off `last()`:

- **window arm** (one merged arm): a `TimingPointWindow` is **always** dispatched
  (its candidates are exhausted only by firing, so empty ⇒ close); a `FastWindow`
  **only when `awaits_input()`** (non-empty). The arm calls `advance_resolution(cx)`,
  which operates on the **top frame** (`last()`) — re-prompt the next candidate, or
  (empty) `close_reaction_window` which `pop()`s the top. An empty `FastWindow` gate
  falls through to **idle** (the permissive case). *This guard is load-bearing*: it is
  what lets the loop leave an empty gate alone (see the invariant section).
- **`SkillTest` arm**: dispatch `skill_test::advance` (now "I am top" — the
  `rposition(SkillTest)` + `win_idx > st` self-location is deleted; its top-of-loop
  check becomes "is `last()` a non-empty window? → return `Done`, let the loop open
  it").
- **`EncounterCard` arm**: `teardown_encounter_card_if_top` (one-shot →
  `encounter_discard`; persistent → skip) + pop.

**`resolve_input`** dispatches the top frame to its one-step resume; `apply` already
runs `drive(cx, outcome)` after it. The handlers **step once and return `Done`**
instead of running the cascade in place:

- `fire_pending_trigger` / `play_fast_event`: read/remove the candidate on the **top**
  window (`last()`/`last_mut()`), fire the effect, return `Done` (the loop re-dispatches
  the window). `play_fast_event` loses its now-unused `window_idx` param.
- `close_reaction_window` (renamed from `close_reaction_window_at`): `pop()` the top
  window, run its continuation, return `Done`. **The skill-test seam (the
  `current_skill_test` → `skill_test::advance` hop) is deleted** — the loop dispatches
  the now-top `SkillTest`. The combat soak re-entry (`run_reaction_continuation` →
  `resume_enemy_attack`) stays as-is (`AttackLoop` not yet a loop arm).

**`run_fast_continuation` stays inline (revised at implementation — do NOT flip it).**
It is the window's **own** continuation, run inline on close *including the open-time
auto-skip path*, which relies on it advancing the phase / skill-test driver
**synchronously** to reach the next suspending step (the commit prompt, the next phase
window). Returning `Done` there makes a skill test that *starts* deep inside a
Revelation effect walk return `Done` prematurely — it never emits its commit prompt
(this regressed the treachery soak-distribution tests). So both arms stay
(`Phase → anchor_on_child_pop`, `SkillTest → skill_test::advance`); it is not a
driver-to-driver reach-down. The genuine reach-down was the *separate* `skill_test::advance`
in `close_reaction_window` *after* this continuation, which is removed.

**Removed driver self-location + chokepoint:**
- `advance`'s `rposition(SkillTest)` + `win_idx > st` self-location (above).
- The `resolve_input` tail chokepoint (`mod.rs:484-486`); `resolve_encounter_card`'s
  synchronous disposal (its own `teardown_encounter_card_if_top` call) stays for the
  no-suspend path; the suspend path now disposes via the loop's `EncounterCard` arm.

**Accessors — deleted vs. kept:** `top_reaction_window_index` and
`top_reaction_window_mut` are **deleted** (dispatch and the window drivers operate on
`last()`/`last_mut()`/`pop()`). `top_reaction_window` survives **only as a
test-inspection accessor** ("is a reaction window open?" in `crates/*/tests`) and
`top_window` for the Fast-play `permissive_window` timing gate — neither is engine
control flow. `current_skill_test` stays as nesting **context** (an effect/resolution
step reading its enclosing test), not dispatch.

**Flip the remaining re-entry sites** to return `Done`:
`resume_before_discover_window` (`reaction_windows.rs:947`), `resume_effect_walk`
(`choice.rs:114`), the `finish_attack_loop` Retaliate tail (`combat.rs:1170`), and the
`advance` `PostOnResolution` teardown's forced-run re-drive (the
`advance_resolution(idx)` for a forced run beneath — the loop's `TimingPointWindow`
arm now dispatches it; the `InvestigatorTurn { ending }` turn-frame resume stays, #235).
**The commit hop is *not* a reach-down** (`finish_skill_test → advance` runs while
`SkillTest` is top — legitimate top-frame resume; unchanged).

**Out of scope (stays imperative):** the combat re-entry
`run_reaction_continuation` → `resume_enemy_attack` (`AttackLoop` is not yet a loop
arm — #411 Shape A); `run_fast_continuation` (the window's own inline continuation).
Converting these is not required to unblock #423.

**Behaviour-preserving claim + backstop.** Event log byte-identical (modulo the one
synthetic test). `crates/cards/tests/revelation_treacheries.rs` (Crypt Chill /
Grasping Hands) is the named guard for the disposal seam — a revelation treachery that
suspends into a skill test must still dispose its card exactly once, after teardown.
`crates/game-core/tests/reaction_windows.rs` + `forced_triggers.rs` guard the window /
forced-run dispatch; the Frozen-in-Fear reentrancy path (forced run beneath a skill
test) is the highest-value characterization case.

## C-coordinators — `EmitEvent` / `TimingPoint` (new behaviour)

**Goal:** reify the `when → at → after × forced → reaction` matrix as nested
loop-driven coordinator frames, so the ordering is structural rather than
hand-threaded, with per-cell eligibility re-scan. This is the genuine #393 end-state
the Slice B spec explicitly deferred to C ("a `TimingPoint` that genuinely owns
'forced then reaction' would have to own reaction-window opening … which Slice C
owns").

**Data model** (reuses `EventTiming` as the bucket cursor, per Slice B):

```rust
enum TimingSub { Forced, Reaction }

Continuation::EmitEvent  { event: TimingEvent, bucket: EventTiming }
Continuation::TimingPoint { event: TimingEvent, bucket: EventTiming, sub: TimingSub }
```

**`emit_event(event)` becomes: push `EmitEvent { event, bucket: When }` and return** —
the `drive` loop takes over.

**The genuine structural model** (replaces queue-then-defer-open):

```
EmitEvent{bucket}
  └ push TimingPoint{bucket, sub: Forced}
        ├ Forced:   run forced effects (inline for 0/1; the #213 lead-ordered
        │           forced run frame for 2+) → pop
        └ Reaction: scan candidates NOW — after forced — push the reaction window
                    (TimingPointWindow{mode: Reaction}); player acts → pop
     TimingPoint pops → EmitEvent re-scans + advances When → At → After → pop
```

- **`EmitEvent` dispatch (on a child pop):** re-scan eligibility *for the current
  bucket*; if populated, push `TimingPoint{bucket, sub: Forced}`, yield; on its pop,
  advance the cursor `When → At → After`; after `After`, pop `EmitEvent`.
- **`TimingPoint` dispatch:** run `sub: Forced` (forced abilities of
  `EventTiming == bucket`, lead-ordered for 2+), then `sub: Reaction` (scan reactions
  of `EventTiming == bucket` **now** and push the reaction window), then pop.

**What this retires:** the queue-then-defer-open hack — `queue_reaction_window`
scanning candidates *before* forced (`reaction_windows.rs:56`) and the ~6 deferred
`open_queued_reaction_window` sites (combat / skill_test / cards / evaluator). With
windows loop-driven (C-plumbing), the loop opens the reaction window structurally when
the `TimingPoint` frame re-exposes it. The stale `emit_event` "Phase ordering" doc
comment (justifying queue-before-forced by a now-deleted `WindowOpened` log) is
removed.

**Why the queue-before-forced ordering can go (verified):** its *only* stated
justification was logging `WindowOpened` before the forced effects' events — and
B-iii (PR #442) deleted `Event::WindowOpened`/`WindowClosed` outright. No other
consumer depends on the queue-time scan.

### New behaviour (the one outcome-changing piece)

Two refinements, same flavour, both covered by a synthetic fixture:

1. **Per-cell eligibility re-scan.** Entering each bucket re-scans eligibility: a
   `when`-cell that mutates board state can change whether an `at`-cell fires. The
   grid is not pre-computed; the nested frames make "enter each cell fresh, re-scan"
   structural.
2. **Reaction candidates scan after forced** (not before, as
   `queue_reaction_window` does today). RR-correct — reaction eligibility is
   determined when the window opens, after forced resolves — and the same shape as
   (1): a forced effect that changes a reaction's eligibility is now reflected.

**No in-scope Gathering card exercises either** (no forced-changes-reaction or
cross-bucket-suppression case at one emit), so the in-scope event log stays
byte-identical. Both are covered by the §G synthetic act/agenda fixture: a `when`-cell
advance that flips an `at`-cell forced ability's eligibility, asserting the `at`
forced does not fire after the `when` cell changes its precondition.

**Single-bucket events stay byte-identical.** Every existing event populates exactly
one bucket, so its `EmitEvent` is a degenerate single-cell iteration; forced events
still precede reaction events. Round-end is the sole multi-cell case, already
remodeled in B-ii.

## Slice D — #423 (forks after C-plumbing)

With `TimingPointWindow`, `SkillTest`, and `EncounterCard` drive-dispatched
(C-plumbing), migrate every `apply_effect` call site to push a root `Effect` frame +
move post-effect logic into the parent frame's `on_child_pop`; reduce `apply_effect` /
`drive_effect_to_base` to test-only or remove. Issue acceptance already crisp; not
re-derived here. D may proceed in parallel with C-coordinators.

## Testing strategy

- **C-plumbing / D: behaviour-preserving.** Full engine + integration suite green at
  the PR boundary; these change structure, not rules. Event log byte-identical, and
  one test delta: the synthetic
  `close_reaction_window_at_removes_reaction_window_not_empty_phase_gate_on_top`
  regression is replaced by `active_reaction_window_is_the_top_continuation_frame` (it
  manufactured a stack the invariant forbids). Named backstops:
  `crates/cards/tests/revelation_treacheries.rs` (the `EncounterCard` disposal seam),
  `crates/cards/tests/non_attack_soak.rs` (multi-point treachery soak distribution — the
  case that caught the `run_fast_continuation` auto-skip regression during
  implementation), and `crates/game-core/tests/{reaction_windows,forced_triggers}.rs`
  (window / forced-run / Frozen-in-Fear reentrancy dispatch).
- **C-coordinators: the only new behaviour.** Single-bucket events byte-identical
  (degenerate one-cell iteration); the per-cell re-scan **and** the reaction-after-forced
  scan are covered by the §G synthetic act/agenda fixture. Round-end ordering stays
  covered by `crates/game-core/tests/act_round_end.rs`,
  `crates/cards/tests/act_advancement.rs`, `crates/cards/tests/the_barrier.rs`,
  `crates/game-core/tests/{reaction_windows,forced_triggers}.rs`,
  and `crates/scenarios/tests/the_gathering*.rs`.
- Each PR matches the full CI gauntlet (fmt / clippy / test / doc / wasm) before push.

## Risk register

| Risk | Mitigation |
|---|---|
| The holistic flip strands a revelation card | `revelation_treacheries` backstop; the loop's `EncounterCard` arm is `teardown_encounter_card_if_top` relocated 1:1 |
| A driver still reaches down after the flip (half-conversion) | Dispatch + window drivers read only `last()`; `top_reaction_window_index`/`_mut` deleted (audit: no engine references remain); `forced_triggers` (Frozen-in-Fear) is the reentrancy backstop |
| Flipping a window's *own inline continuation* (`run_fast_continuation`) breaks the open-time auto-skip | **Realized during implementation:** returning `Done` made a skill test that starts inside a Revelation effect walk skip its commit prompt. Mitigation: `run_fast_continuation` stays inline; `non_attack_soak.rs` is the backstop |
| C-coordinators' reaction-after-forced scan regresses an in-scope candidate set | No in-scope card hits forced-changes-reaction-eligibility at one emit; §G synthetic fixture + round-end suites guard it |

## What "done" looks like

The global `drive` loop drives **every** frame — windows, skill tests, encounter-card
disposal, effect walks, and the `when/at/after × forced/reaction` matrix — by uniform
top-frame dispatch. `resolve_input` is a thin "dispatch top frame, then `drive`";
`emit_event` is a thin `EmitEvent`-coordinator push; the five synchronous skill-test
re-entry sites, the `resolve_input` encounter-disposal chokepoint, the
queue-then-defer-open hack, and the ~6 deferred `open_queued_reaction_window` sites
are gone. Slice D (#423) is unblocked as a consequence; the #393 unified control-flow
model's effect/timing end-state is complete.

## Open questions

None blocking. If a second round-end-window card appears (Dunwich+), revisit whether
the group clue-spend native effect warrants promotion to a shared `Effect` variant
(Slice B's standing note).
