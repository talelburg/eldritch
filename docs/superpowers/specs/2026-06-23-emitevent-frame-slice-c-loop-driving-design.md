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

## Sub-slice decomposition

Four PRs, strictly ordered. The first two are behaviour-preserving plumbing that
unblocks #423; the third is the deferred-from-B coordinator work (the one slice
carrying new behaviour); D is #423 itself, forking after C-ii.

```
C-i   Window drive-loop arms                    behaviour-preserving
       └ drive arms for TimingPointWindow + FastWindow; resolve_input routes
         through drive; retire window-side imperative re-entry EXCEPT the
         skill-test seam (kept imperative for now)
            │
            ▼
C-ii  Skill-test / encounter core   (atomic)    behaviour-preserving  ← unblocks #423
       └ SkillTest drive arm + EncounterCard disposal as a loop arm
         + retire the 5 re-entry sites + retire the resolve_input chokepoint
            │
            ├──────────────► D (#423) effect call-site migration  (may start here)
            ▼
C-iii  EmitEvent / TimingPoint coordinators      NEW BEHAVIOUR (per-cell re-scan,
       └ emit_event → push EmitEvent + return; when→at→after × forced→reaction      reaction-after-forced scan)
         as nested loop-driven frames; per-cell eligibility re-scan + §G test;
         retire queue-then-defer-open + the ~6 deferred open sites
```

### Dependency spine (why this order)

- **C-i before C-ii.** Windows must be loop-dispatched before the skill-test seam
  can retire — but C-i *keeps* the seam: the window arm's close path still calls
  `skill_test::advance` imperatively for the mid-test case, while non-test closes
  pop to the loop. That preserved seam is what makes C-i independently green.
- **C-ii is the coupled core — one atomic PR.** You cannot add the `SkillTest`
  drive arm without *simultaneously* making `EncounterCard` disposal loop-driven
  (else the loop's old `_ => Done` strands a revelation treachery's card once a
  skill-test resume runs through the loop) **and** flipping all five re-entry sites
  at once (a half-flipped site would double-drive against the new arm). These three
  changes cannot be sub-sliced apart — see "Why C-ii cannot be split."
- **C-iii last.** `emit_event` becoming a thin push-and-return only works once every
  caller's parent frame is loop-driven (C-i + C-ii). It is also the one slice with
  new behaviour (per-cell re-scan + the reaction-after-forced scan), kept isolated
  behind the plumbing per the brainstorm's scoping decision.
- **D forks after C-ii.** #423's four remaining blocked sites — `fire_pending_trigger`
  / `play_fast_event` (under the window), the forced `resolve_one`, the enemy
  revelation (under `EncounterCard`), the skill-test cluster (under `SkillTest`) —
  are all loop-dispatched once C-ii lands. **D does not wait on C-iii.**

## C-i — Window `drive`-loop arms

**Goal:** the loop drives `TimingPointWindow` and `FastWindow` by top-frame dispatch,
**behaviour-preserving**, with the skill-test re-entry seam left on the old path.

- Add `drive`-loop arms for `Continuation::TimingPointWindow` and
  `Continuation::FastWindow`: on a child pop the loop dispatches the window's resume
  (advance to the next candidate and re-suspend, or close + run its continuation).
- Make `resolve_input` call `drive(cx, outcome)` after dispatching, so window resumes
  return to the loop rather than running the whole cascade in place.
- Convert the window resume path to **step-and-return**: `resume_window` /
  `fire_pending_trigger` / `play_fast_event` fire one candidate's effect, then return
  `Done`; the loop re-dispatches the window frame.
- **Retire the imperative window re-entry that does NOT touch skill-test:**
  `advance_resolution`'s "re-prompt or close" loop role moves into the
  `TimingPointWindow` arm; `run_fast_continuation`'s `Phase` path pops to the
  anchor's `on_child_pop` via the loop.
- **Kept (the seam):** `close_reaction_window_at` / `run_fast_continuation`'s
  `SkillTest` path still call `skill_test::advance` imperatively for a mid-test
  close. The `EncounterCard` chokepoint at `resolve_input`'s tail is untouched.
  Keeping these on the old path is what keeps C-i behaviour-preserving.

**Behaviour-preserving claim.** Event log byte-identical. The loop never sees a bare
`SkillTest`-on-top it cannot handle: the window arm's close path either bridges to
`skill_test::advance` (mid-test) or pops to a frame the loop already drives.

## C-ii — Skill-test / encounter core (atomic)

**Goal:** the loop drives `SkillTest` and `EncounterCard` disposal; the five
synchronous re-entry sites and the `resolve_input` chokepoint are gone.
**Behaviour-preserving.** This unblocks #423.

- Add a `drive`-loop `SkillTest` arm that dispatches `skill_test::advance`.
- Add a `drive`-loop `EncounterCard` arm carrying the disposal logic (one-shot →
  `encounter_discard`; persistent → skip) + pop — i.e. `teardown_encounter_card_if_top`'s
  body, now loop-reachable as a top-frame dispatch.
- **Delete** the `resolve_input` tail chokepoint (`mod.rs:484-486`); the synchronous
  disposal in `resolve_encounter_card` (`encounter.rs:193`) collapses to "push frame,
  return to loop."
- **Flip all five re-entry sites** to return `Done` (the loop dispatches `SkillTest`):
  `close_reaction_window_at` (the `reaction_windows.rs:874` hop),
  `resume_before_discover_window` (`reaction_windows.rs:947`), `resume_effect_walk`
  (`choice.rs:114`), the `fire_retaliate_if_any` → `drive_retaliate` Retaliate tail
  (`combat.rs:1170`), and the commit hop (`resume_skill_test_commit`,
  `mod.rs:354/430`). The implementation plan pins the exact set.
- **Optional, flag-don't-force:** `skill_test::advance`'s `rposition` /
  `top_reaction_window_index` self-location logic (`skill_test.rs:445-454`) can
  simplify toward "I am top" once the loop drives it. Defer if it widens the diff —
  it is not load-bearing for C-ii's correctness.

**Why C-ii cannot be split.** The three changes are mutually entangled: (1) the
`SkillTest` arm needs `EncounterCard` disposal loop-driven, because a revelation
treachery whose Revelation suspends into a skill test parks `EncounterCard` beneath
`SkillTest`; once skill-test resumption runs through the loop, the loop must dispose
the now-top `EncounterCard` itself. (2) `EncounterCard` disposal being loop-driven
makes the `resolve_input` chokepoint dead — but the chokepoint is also what disposes
the card today, so it cannot be removed until the arm exists. (3) Each re-entry site
returns the result of `skill_test::advance` today; flipping one to return `Done`
while the arm does *not* yet exist strands the test, and leaving one imperative once
the arm *does* exist double-drives. So arm + disposal + all five flips + chokepoint
removal land together.

**Behaviour-preserving claim + backstop.** Event log byte-identical;
`crates/cards/tests/revelation_treacheries.rs` (Crypt Chill / Grasping Hands) is the
named guard for the disposal seam — a revelation treachery that suspends into a skill
test must still dispose its card exactly once, after teardown.

## C-iii — `EmitEvent` / `TimingPoint` coordinators (new behaviour)

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
windows loop-driven (C-i/C-ii), the loop opens the reaction window structurally when
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

## Slice D — #423 (forks after C-ii)

With `TimingPointWindow` (C-i) and `SkillTest` / `EncounterCard` (C-ii) drive-dispatched,
migrate every `apply_effect` call site to push a root `Effect` frame + move post-effect
logic into the parent frame's `on_child_pop`; reduce `apply_effect` / `drive_effect_to_base`
to test-only or remove. Issue acceptance already crisp; not re-derived here. D may proceed
in parallel with C-iii.

## Testing strategy

- **C-i / C-ii / D: behaviour-preserving.** Full engine + integration suite green at
  every PR boundary; these change structure, not rules. Event log byte-identical
  through C-i/C-ii. C-ii's named backstop is
  `crates/cards/tests/revelation_treacheries.rs` (the disposal seam).
- **C-iii: the only new behaviour.** Single-bucket events byte-identical (degenerate
  one-cell iteration); the per-cell re-scan **and** the reaction-after-forced scan
  are covered by the §G synthetic act/agenda fixture. Round-end ordering stays
  covered by `crates/game-core/tests/act_round_end.rs`,
  `crates/cards/tests/act_advancement.rs`, `crates/cards/tests/the_barrier.rs`,
  `crates/game-core/tests/reaction_windows.rs`, `crates/game-core/tests/forced_triggers.rs`,
  and `crates/scenarios/tests/the_gathering*.rs`.
- Each PR matches the full CI gauntlet (fmt / clippy / test / doc / wasm) before push.

## Risk register

| Risk | Mitigation |
|---|---|
| C-ii's atomic flip strands a revelation card | `revelation_treacheries` backstop; the `EncounterCard` loop arm is `teardown_encounter_card_if_top`'s body relocated 1:1 |
| C-i window step-and-return reorders the event log | Preserve queue-then-defer-open through C-i (only C-iii changes it); assert byte-identical log per sub-slice |
| C-iii's reaction-after-forced scan regresses an in-scope candidate set | No in-scope card hits forced-changes-reaction-eligibility at one emit; §G synthetic fixture + the round-end suites guard it; assert in-scope log unchanged |
| C-ii self-location simplification over-reaches | Treat as optional; defer if it widens the diff |
| C-iii `emit_event` push-and-return changes single-bucket ordering | Degenerate single-cell iteration; forced still precedes reaction; byte-identical log assertion |

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

None blocking. The C-ii self-location cleanup is explicitly optional. If a second
round-end-window card appears (Dunwich+), revisit whether the group clue-spend native
effect warrants promotion to a shared `Effect` variant (Slice B's standing note).
