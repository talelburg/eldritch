# Skill-test ST.1/ST.2 player windows (#374) — design

Tracking: [#374](https://github.com/talelburg/eldritch/issues/374). Phase 7
ordering step 5 (`docs/phases/phase-7-the-gathering.md`). Builds directly on the
skill-test driver reification substrate (PR #430,
[spec](2026-06-22-skill-test-driver-frame-reification-design.md)).

## Why this exists

RR p.26 (Skill Test Timing) opens **two `🌀 PLAYER WINDOW`s** during every skill
test (verified against `data/rules-reference/ahc01_rules_reference_web.pdf` p.26):

- after **ST.1** ("Determine skill of test. Skill test of that type begins.")
  and before **ST.2** ("Commit cards from hand to skill test.");
- after **ST.2** (commit) and before **ST.3** ("Reveal chaos token.").

These are framework Fast/reaction windows — where a player may play a Fast card
or trigger a reaction mid-test, *before* the chaos token is revealed. Today
`advance` (the skill-test driver) never opens them: the only player interaction
during a test is the commit prompt. So no Fast play or pre-token reaction can
occur.

The substrate (PR #430) made `advance` the single driver with an explicit cursor
(`SkillTestStep`), so these windows now slot in as **cursor-step insertions**
rather than bespoke threading.

## Scope

**In:** open the two windows at the correct cursor points, routed through the
existing `open_fast_window` pipeline (auto-skip when nothing is playable);
re-enter `advance` on close. Prove correctness with a synthetic Fast fixture.

**Out (own follow-ups):**

- **#64** — the after-success/after-failure reaction window (a *different* timing
  point, ST.6→ST.8). Separate work-stream.
- **Fire Axe (02032)** — the first *real* Core+Dunwich consumer of these windows
  (`[fast] During an attack using Fire Axe, spend 1 resource: You get +2 [combat]
  for this skill test`). It is currently an unimplemented stub and a substantial
  card (Fight action + conditional damage + fast activated ability with resource
  cost, per-attack limit, mid-test combat boost). Its full implementation is its
  own card PR; this spec only documents it as the motivating consumer and tests
  the windows with a synthetic fixture instead.
- **#423** — effect-call-site migration. Untouched here.

## Background: how framework Fast windows work today

`open_fast_window(cx, kind)` (reaction_windows.rs):

1. emits `WindowOpened { kind }`, pushes a `Continuation::Resolution` frame
   carrying the window;
2. scans `scan_pending_triggers` (in-play `OnEvent` reactions matching the
   window) + `any_fast_play_eligible` (Fast cards/abilities the active player
   could play);
3. **auto-skip** if both are empty: pops the frame, emits `WindowClosed { kind }`,
   and runs `run_window_continuation(cx, kind)` **inline** — no `AwaitingInput`
   round-trip;
4. otherwise leaves the frame on the stack and returns `Done`; the player
   resolves it via `ResolveInput`, and on the final `Skip`/drain
   `close_reaction_window_at` → `run_window_continuation(cx, kind)`.

`run_window_continuation` is a `match kind { … }` that decides *what runs when
the window closes*. Crucially, `WindowKind::PlayerWindow(_) =>
anchor_on_child_pop(cx)` — i.e. the existing framework player windows route their
close to the **phase anchor** beneath them. A skill-test window has a `SkillTest`
frame beneath it, not a phase anchor, so it **cannot reuse `PlayerWindow`** — it
needs its own `WindowKind` whose continuation re-enters `advance`.

## The design

### 1. Cursor & flow

`SkillTestStep` gains two window steps. The sequence becomes:

```
PreCommitWindow → AwaitingCommit → [commit submit] → PreTokenWindow → Resolving → PostFollowUp → PostRetaliate → PostOnResolution
   ST.1→ST.2          ST.2                                ST.2→ST.3      ST.3+
```

- `start_skill_test` sets the initial cursor to **`PreCommitWindow`** (was
  `AwaitingCommit`).
- `finish_skill_test` sets the post-commit cursor to **`PreTokenWindow`** (was
  `Resolving`), after validating + storing the commit indices.

Each window arm in `advance` **pre-advances the cursor, then returns
`open_fast_window`'s outcome directly** — it does *not* fall through to the loop:

```rust
SkillTestStep::PreCommitWindow => {
    // Pre-advance BEFORE opening (the suspend/resume invariant): on resume the
    // driver picks up at AwaitingCommit, not by re-opening this window.
    set cursor = AwaitingCommit;
    return open_fast_window(cx, WindowKind::SkillTestPlayerWindow { before_token: false });
}
SkillTestStep::PreTokenWindow => {
    set cursor = Resolving;
    return open_fast_window(cx, WindowKind::SkillTestPlayerWindow { before_token: true });
}
```

`AwaitingCommit`, `Resolving`, and the rest are unchanged.

**Why `return`, not fall-through** (verified against `open_fast_window` +
`Continuation::awaits_input`). `open_fast_window` returns one of two things, and
both must propagate:

- **Auto-skip** (no pending reaction trigger *and* no Fast play eligible — the
  common 1-player case): it pops the frame and runs `run_window_continuation`
  inline, which for this `WindowKind` is `advance(cx)` (§3). The cursor is
  pre-advanced, so this recurses one level into `advance` at the *next* step and
  runs the test onward, returning that step's outcome (e.g. the commit
  `AwaitingInput`, or `Done` at teardown). Bounded recursion — the cursor advances
  monotonically, so a window step is never re-entered.
- **Parked** (a Fast play is eligible / a reaction is pending): `open_fast_window`
  leaves the `Resolution` frame on the stack and returns `Done`. A pure-Fast
  window has empty `pending_triggers`, so `Continuation::awaits_input` is `false`
  and **no `AwaitingInput` is emitted** — the engine idles at `Done` with the
  window frame on top (exactly like the open turn), and the player either plays a
  Fast card / triggers a reaction or submits `Skip`. Falling through to the
  `advance` loop here would be a bug: `top_reaction_window_index()` skips
  empty-trigger windows, so the loop wouldn't see the parked window and would
  wrongly emit the commit prompt *over* it. Returning `Done` stops the driver with
  the window correctly on top.

(This differs from the phase callers of `open_fast_window`, which `expect` a `Done`
return — their continuation, `anchor_on_child_pop`, doesn't suspend at that point.
The skill-test continuation, `advance`, *can* suspend, so the arm propagates the
outcome rather than asserting.)

### 2. The window

One new variant:

```rust
WindowKind::SkillTestPlayerWindow { before_token: bool }
```

It rides the existing `Continuation::Resolution` frame via `open_fast_window` —
**no new `Continuation` variant**. `before_token=false` is the ST.1→ST.2 window,
`true` the ST.2→ST.3 window. The discriminant feeds `WindowOpened`/`WindowClosed`
observability only; both windows share one continuation. `scan_pending_triggers`
+ `any_fast_play_eligible` already gate auto-skip vs suspend; no new eligibility
logic.

### 3. Re-entry

New arm:

```rust
// run_window_continuation
WindowKind::SkillTestPlayerWindow { .. } => advance(cx),
```

Because the cursor was pre-advanced before the window opened, `advance` resumes at
the correct step (`AwaitingCommit` after window 1, `Resolving` after window 2).
This arm is reached on **both** paths — the `open_fast_window` auto-skip inline
call and the wait-then-close path (`close_reaction_window_at` →
`run_window_continuation`). A Fast play/reaction inside the window resolves
through the existing reaction-window machinery before the window drains and the
continuation fires.

### 4. Correctness interactions

- **Substitution (Mind over Matter, 01036).** ST.1 substitution resolves *before*
  window 1: `start_skill_test` pushes the `SubstitutionPrompt` above the
  `SkillTest` (cursor `PreCommitWindow`) and returns the choice; on resolve,
  `resume_substitution_choice → advance @ PreCommitWindow` opens window 1. RR
  order preserved (substitution at ST.1, window after).
- **Forced-run-below guard** (substrate §4) — unchanged and still load-bearing: a
  window *this test* opens sits **above** the `SkillTest` frame; a forced-run
  `Resolution` frame *below* it stays ignored by `advance`'s window check.
- **Commit `AwaitingInput` propagation** (substrate) — still synchronous. Window 1
  opening before the commit prompt does not change that the commit prompt (when
  reached) propagates up to halt an enclosing forced run.
- **Auto-fail / empty interactions** — windows still open before the token (a Fast
  play could matter pre-token) and auto-skip when nothing is eligible. ST.3+
  unchanged.

### 5. The discriminant is transitional (forward note)

`WindowKind::SkillTestPlayerWindow` exists **only** because `WindowKind` currently
doubles as the continuation key in `run_window_continuation`. In the
EmitEvent-frame end-state ([#431](https://github.com/talelburg/eldritch/issues/431),
the deferred #393 successor), windows dissolve into a generic
`FastWindow { candidates, fast_actors }` driven by pure top-frame dispatch: the
window pops, and the loop drives whatever frame sits **beneath** it — a `SkillTest`
frame advances the test, a `*Phase` anchor advances the phase. At that point the
skill-test fast windows and the phase fast windows are the **same** frame; "which
window" is read from the parent `SkillTest`'s cursor, not stored on the window. So
this variant + its `before_token` discriminant are absorbed wholesale by #431 —
the same category of transitional artifact as the substrate's forced-run-below
guard. (Recorded so the #431 author knows to delete it, not preserve it.)

## Testing

- **Window opens and a Fast play resolves there** (`game-core` engine test, stub
  registry + synthetic Fast-playable fixture): set up a test where the active
  investigator has an eligible Fast play at the window; assert the window opens
  (`WindowOpened { SkillTestPlayerWindow { before_token: false } }`), the Fast
  play resolves inside it, the window closes, and the test continues to a normal
  resolution (`SkillTestEnded`).
- **Auto-skip** (no eligible Fast play / reaction): assert the window opens and
  closes (`WindowOpened`/`WindowClosed` emitted) with **no** `AwaitingInput`, and
  the test resolves in one `apply`. Both windows covered.
- **Substitution → window 1** still flows (MoM path: substitution resolves, then
  window 1 opens, then commit).
- **Behaviour-preserving for the rest:** the full existing suite stays green.
  Existing skill-test tests gain two `WindowOpened`/`WindowClosed` pairs per test;
  `assert_event!`-style assertions (order-insensitive subset) are unaffected, and
  any `assert_eq!`-on-events-slice assertions are updated to include the new
  pairs.

## Event-stream change (flagged)

Every skill test now emits `WindowOpened`/`WindowClosed` for both windows, even
when auto-skipped (consistent with the existing phase player windows, which also
emit when auto-skipped). This is rules-faithful — the windows *do* open and
immediately close — but it is a visible event-stream change for every test. The
churn is confined to exact-slice assertions.

## PR slicing

One PR. The change is small and cohesive: two `SkillTestStep` variants, one
`WindowKind` variant, two `advance` arms, one `run_window_continuation` arm, the
two entry-point cursor inits, and the tests. The phase-doc update (step 5: windows
shipped; + the #431 cross-reference) lands as the final commit once CI is green.

## What "done" looks like

- `advance` opens a Fast/reaction window after ST.1 (before commit) and after ST.2
  (before the token), routed through `open_fast_window`, auto-skipping when empty.
- A Fast play/reaction at either window resolves there (synthetic-fixture test).
- No new `Continuation` variant; the windows ride `Continuation::Resolution`.
- Substitution and the forced-run-below guard still hold; full CI gauntlet green.
- The phase doc records the windows shipped and cross-references #431 for the
  eventual `FastWindow` unification.
