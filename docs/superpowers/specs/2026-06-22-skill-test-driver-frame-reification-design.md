# Skill-test driver frame reification — design

Tracking: substrate for Phase 7 ordering **step 5** (`docs/phases/phase-7-the-gathering.md`).
Successor pattern to the `*Phase`-anchor slices (#393, PRs #397/#398) and the
`AttackLoop`/keystone arc (PRs #412–#425). Behaviour-preserving.

## Why this pass exists

`drive_skill_test` is the last major control-flow driver still run **imperatively**
rather than through the uniform top-frame `drive` loop. Today:

- `start_skill_test` pushes a `Continuation::SkillTest` frame, then returns the
  commit `AwaitingInput` directly (`open_commit_window`).
- The post-commit resolution sequence is an inline `FinishContinuation` cursor
  (`AwaitingCommit → PostFollowUp → PostRetaliate → PostOnResolution`), advanced
  inside `drive_skill_test`'s own loop.
- A reaction window opening mid-test is re-entered **imperatively** from
  `close_reaction_window_at` (reaction_windows.rs:931–935: *"if a skill test is
  mid-resolution, call `drive_skill_test`"*).
- A teardown **tail** is bolted onto the commit-resume handler
  (`resume_skill_test_commit`, mod.rs:351–380): after a test torn down by
  `finish_skill_test`, it re-drives a forced-run sibling (#213) or an
  `InvestigatorTurn { ending: true }` (`resume_end_turn`).

Meanwhile the `drive` loop (dispatch/mod.rs:166–213) already owns the `*Phase`
anchors, `ActionResolution`, and `Effect` frames via a single rule — *advance the
top frame until it blocks or idles*. The `SkillTest` frame is the conspicuous
exception: when it is on top, the loop's `_ => Done` arm treats it as an
already-surfaced suspension and does not drive it.

This pass brings `SkillTest` into the uniform loop, end-to-end, **behaviour-
preserving and with zero new `Continuation` variants**. The payoff is the
substrate for ordering step 5: once the driver is uniform, the #374 framework
windows (around commit), the #64 after-resolution window, and the #423 effect-
call-site migration become drop-ins (push a child frame at a cursor) rather than
bespoke threading.

## Scope

**In:** reify `drive_skill_test` into a single loop-driven `advance(cx)` on the
existing `SkillTest` frame; fold commit emission and the teardown tail into that
driver; delete the imperative re-entry points. Behaviour-preserving.

**Out (each its own later spec):**

- **#374** — the two framework Fast/reaction player windows around commit
  (after ST.1, after ST.2).
- **#64** — the after-success/after-failure trigger window before `SkillTestEnded`.
- **#423** — migrating `on_commit`/`on_success`/`on_fail`/follow-up effect calls
  off the synchronous `apply_effect` bounded entry. They keep using it here.
- **General window-taxonomy rework** — dissolving `Resolution`/`ResolutionKind`/
  `WindowKind` into `FastWindow` + `TimingPointWindow{event, mode}`, and driving
  `Resolution`/`InvestigatorTurn` frames *generally* from the loop. That is the
  EmitEvent-frame slice (#393, post-C, #212 successor). The teardown tail here
  stays a **skill-test-teardown-specific relocation**, not a general loop
  extension. See "The forced-run-below guard" for why this matters.

## The model

### The cursor

Rename `FinishContinuation` → `SkillTestStep` (it now spans the whole test, not
just the "finish"). Add one explicit post-commit step so the body
`finish_skill_test` runs today is a named cursor state rather than an implicit
edge from `AwaitingCommit`:

```
AwaitingCommit  →  Resolving  →  PostFollowUp{ok}  →  PostRetaliate{ok}  →  PostOnResolution{ok}
```

`AwaitingCommit` / `PostFollowUp` / `PostRetaliate` / `PostOnResolution` keep
their current meaning; `Resolving` is the (new, explicit) "commit submitted, run
the resolution body" state.

### One driver: `advance(cx)`

`advance` is the reshaped `drive_skill_test`. It keeps an internal step-loop (as
`drive_skill_test` has today) and on each iteration:

1. **Mid-test window check (verbatim).** If a `Resolution` window sits strictly
   *above* this `SkillTest` frame → `open_queued_reaction_window` → return
   `AwaitingInput`. The `rposition(SkillTest)` vs `top_reaction_window_index()`
   comparison and its forced-run-below guard are copied unchanged (see "The
   forced-run-below guard").
2. **Dispatch on the cursor:**
   - `AwaitingCommit` → **this is the frame's `awaiting()`**: return the commit
     `PickMultiple` `AwaitingInput`. (Body of today's `open_commit_window`.)
   - `Resolving` → run today's `finish_skill_test` body verbatim: sum skill,
     `fire_on_commit`, `resolve_chaos_token_and_emit`, **pre-advance to
     `PostFollowUp`**, then follow-up + `on_success`/`on_fail`; propagate any
     suspension exactly as today.
   - `PostFollowUp` / `PostRetaliate` → unchanged bodies (fire
     `OnSkillTestResolution` triggers; retaliate); pre-advance the cursor before
     any sub-step that can suspend (existing invariant).
   - `PostOnResolution` → teardown + the relocated tail (see below).

### Loop integration

One new arm in `drive` (dispatch/mod.rs), mirroring the `ActionResolution` /
`Effect` arms:

```rust
Some(Continuation::SkillTest(_)) => match skill_test::advance(cx) {
    EngineOutcome::Done => { /* torn down; loop on to the frame beneath */ }
    other => return other,   // commit prompt, mid-test window, or Rejected
}
```

## Entry, commit, substitution

### Entry funnels through `advance` — not `Done`

The commit `AwaitingInput` **must propagate synchronously** out of the entry
path, because that propagation is what halts an enclosing forced run between
candidates (see "The forced-run-below guard" — a `Done` return would let the
forced run immediately fire its next candidate and push a second `SkillTest`,
tripping `has_skill_test_in_flight`). So the entry points **call `advance(cx)`**,
whose `AwaitingCommit` arm emits the prompt:

- `start_skill_test` (no-substitution path) ends with `advance(cx)` instead of
  `open_commit_window`. The `SkillTest` frame is still pushed up front
  (unchanged), `SkillTestStarted` still emitted.
- `resume_substitution_choice` (Mind over Matter) pops the `SubstitutionPrompt`,
  optionally rewrites `skill`/`test_modifier` (unchanged), then ends with
  `advance(cx)` instead of `open_commit_window`.

`open_commit_window` is deleted (its body is `advance`'s `AwaitingCommit` arm).

The **substitution prompt itself is unchanged**: `start_skill_test` pushes
`SubstitutionPrompt` above the frame and returns the choice `AwaitingInput` (a
real suspension; the loop never reaches the `SkillTest` because
`SubstitutionPrompt` is on top).

One driver, two call paths: `advance` is reached **from the entry** (returns
`AwaitingInput` up the call stack — halting any enclosing forced run) and **from
the loop's `SkillTest` arm** (re-driving after a mid-test window closes). Both
emit the commit prompt identically.

### Commit resume shrinks

`resolve_input`'s `SkillTest(_)` arm still routes to `resume_skill_test_commit`.
Its new body: validate the `PickMultiple` indices, store them on the frame, set
`step = Resolving`, return `Done`. `apply_player_action` then runs `drive`, which
drives `advance` → `Resolving`. The entire post-`finish_skill_test` tail (the
#213 forced-run-sibling re-drive and the `InvestigatorTurn { ending }` /
`resume_end_turn` re-entry) **moves out of here** into `PostOnResolution`.

## Resolution, teardown tail, deletions

`advance`'s `Resolving` arm runs today's `finish_skill_test` body verbatim.
`PostFollowUp` / `PostRetaliate` are unchanged.

**The teardown tail relocates into `PostOnResolution`.** Today `PostOnResolution`
discards committed cards, emits `SkillTestEnded`, drains `ThisSkillTest`
modifiers, pops the frame by position, returns `Done` — and the *caller*
(`resume_skill_test_commit`) inspects what's beneath. In the reified model, after
popping the frame, `PostOnResolution` itself runs that tail, the same two checks
moved verbatim:

- a forced-run `Resolution` frame beneath (`f.is_forced()`) → `advance_resolution`
  (fire remaining #213 siblings / close it);
- an `InvestigatorTurn { ending: true }` beneath → `resume_end_turn`;
- otherwise → `Done`, and the loop drives whatever anchor/frame is beneath.

Behaviour-identical: the same tail, triggered at the same moment (test fully torn
down), relocated from the commit-resume handler into the teardown step so it
fires regardless of *which* resume re-entered the driver (commit submit, or a
mid-test window close).

**Deletions:**

- `close_reaction_window_at`'s skill-test special-case (reaction_windows.rs:931–935).
  Window close just pops the child `Resolution` frame and returns `Done`; the loop
  re-drives the `SkillTest` beneath via `advance`.
- `open_commit_window` — folded into `advance`'s `AwaitingCommit` arm.
- The `resume_skill_test_commit` tail — relocated to `PostOnResolution`.
- `finish_skill_test` as a separate public entry — its body becomes the
  `Resolving` arm. (`drive_skill_test` becomes `advance`.)

Net new surface: zero `Continuation` variants; the loop gains one `SkillTest`
arm.

## Load-bearing constraints (preserved verbatim)

### The forced-run-below guard

**Where it comes from.** The multi-candidate forced run (#213 reentrancy). Two
copies of **Frozen in Fear** (01164) in your threat area —
*"Forced – At the end of your turn: Test [willpower] (3). If you succeed, discard
Frozen in Fear."* (verified against `data/arkhamdb-snapshot/pack/core/core_encounter.json`)
— produce one forced run (`Resolution { kind: Forced }`) holding both as
candidates. Firing candidate 1 runs its effect → `Effect::SkillTest` →
`start_skill_test` pushes a `SkillTest` frame **above** the forced `Resolution`
frame. `fire_pending_trigger` removes the *firing* candidate from
`pending_triggers`, but candidate 2 remains — so that forced `Resolution` frame
still has **non-empty `pending_triggers`**, which is exactly what
`top_reaction_window_index()` keys on (game_state.rs:1736–1739). Without the
`win_idx > st` guard, `advance` would mistake that forced-run frame *below* the
test for a mid-test reaction window and wrongly try to suspend on it.

**Why it stays in this substrate.** The guard is an artifact of two things this
substrate does **not** rework: (1) the "queue-then-driver-notices" window-opening
pattern, and (2) the generic `Resolution` frame doing double duty (mid-test
window above vs forced-run parent below). Both are dissolved only by the later
EmitEvent-frame slice (pure top-frame dispatch, where a frame never scans the
stack and `Resolution` splits into `FastWindow`/`TimingPointWindow`). Until then,
positional disambiguation is necessary and is copied unchanged.

**Why entry must funnel through `advance` (not return `Done`).** The same
reentrancy makes the commit `AwaitingInput`'s synchronous propagation
load-bearing: it is what stops the forced run from draining candidate 2 before
candidate 1's test resolves. A `Done` return from the entry would let
`resolve_one`'s caller fire candidate 2 immediately, pushing a second
`SkillTest` and tripping `has_skill_test_in_flight`. Hence `start_skill_test` /
`resume_substitution_choice` end with `advance(cx)`, whose `AwaitingCommit` arm
returns the prompt up the call stack.

### Other invariants

- **Positional `take_skill_test`.** Teardown removes the `SkillTest` frame *by
  position*, not by popping the top — a player-window gate (#69/#70/#71) may
  legitimately sit above it. Unchanged.
- **Single-test invariant.** `has_skill_test_in_flight()` still rejects a second
  overlapping test at `start_skill_test`. `advance` always operates on the
  topmost `SkillTest` via `current_skill_test()`; the forced-run case (a
  `Resolution` beneath) is the only multi-test-on-stack shape, handled by the
  guard. No new nesting introduced.
- **Pre-advance ordering.** Every cursor transition preceding a sub-step that can
  suspend stays *pre-*advanced (set the next step before delegating), so a
  window/choice opening inside the follow-up, `on_success`, `on_fail`, or
  retaliate resumes at the *next* step rather than re-running the suspending one
  (existing invariant, skill_test.rs:308–318, 442–449). Preserved exactly.

## Testing

**Behaviour-preserving is the whole bar.** The full existing engine + integration
suite must stay green through the change — this is structure, not rules. Targeted
unit coverage for the relocations (`game-core` engine tests, `TestGame` builder +
event-assertion macros; registry-gated cases in `crates/cards/tests/`):

1. A mid-test reaction window (e.g. `AfterEnemyDefeated` from a Fight follow-up's
   `damage_enemy`) closes and the test resumes through the **loop** (`advance`),
   not `close_reaction_window_at`.
2. The #213 two-Frozen-in-Fear forced-run sibling still fires after the first
   test tears down (the relocated tail's `is_forced` branch).
3. `InvestigatorTurn { ending: true }` still resumes `end_turn` after an
   end-of-turn test (the tail's other branch).
4. Mind-over-Matter substitution → commit still flows (entry funnels through
   `advance`; the substitution prompt suspension is unchanged).

## PR slicing

One cohesive substrate; default to a **single PR** (smaller than any K-arc PR,
and splitting leaves a half-wired loop mid-slice). Fall back only if review
demands it, along this fault line:

- **PR-1** — relocate the teardown tail into `PostOnResolution` + delete the
  `close_reaction_window_at` special-case. Driver still entered imperatively, but
  re-entry-source-independent.
- **PR-2** — add the `drive` `SkillTest` arm + move commit emission into
  `advance` (`open_commit_window` deleted; `start_skill_test` /
  `resume_substitution_choice` funnel through `advance`).

## What "done" looks like

- `SkillTest` is a loop-driven frame: `drive` has a `SkillTest` arm; `advance` is
  the single driver; the commit prompt is emitted by `advance`'s `AwaitingCommit`
  arm.
- `open_commit_window`, the `close_reaction_window_at` skill-test special-case,
  and the `resume_skill_test_commit` teardown tail are gone (tail relocated to
  `PostOnResolution`).
- Zero new `Continuation` variants; `apply_effect` sub-calls unchanged (#423
  out of scope).
- Full CI gauntlet green; the Gathering plays end-to-end unchanged.
