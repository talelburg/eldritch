# Axis-B T5 — `emit_event` + `TimingEvent` + two-phase dispatch (implementation plan)

> **For agentic workers:** sub-plan of the Axis-B foundation
> (`2026-06-16-trigger-dispatch-axis-b-foundation-design.md`, task 5). Steps use
> checkbox syntax. Decomposed into **T5a** (chokepoint, behavior-preserving,
> closes #212) and **T5b** (iterative ordering + reentrancy, closes #213/#294).
> The semantic change; a pre-push `/code-review` pass runs on **T5b** before its PR.

**Goal:** route every forced/reaction trigger dispatch through one
`emit_event(cx, TimingEvent)` chokepoint, then replace the forced path's
fixed-order resolution with the rules-correct iterative lead-investigator
ordering loop with reentrancy.

**Why split:** T5a is mechanical + behavior-preserving (like T3/T4) — it unifies
the *call sites*. T5b is the actual behavior change (player-chosen forced order,
suspend-mid-forced reentrancy). Splitting keeps the semantic diff small and
reviewable, and maps cleanly to the issues (#212 vs #213/#294).

---

## Current dispatch call sites → `TimingEvent` mapping

Every site today calls `fire_forced_triggers(ForcedTriggerPoint::X)` and/or
`queue_reaction_window(WindowKind::Y)`. After T5a each becomes one
`emit_event(cx, TimingEvent::Z { … })`. `TimingEvent` is the **union** of
`ForcedTriggerPoint` + the event-driven `WindowKind` variants; each maps to a
phase-1 forced `EventPattern` and an optional phase-2 reaction `WindowKind`.

| Site | Today | `TimingEvent` | phase-1 forced pattern | phase-2 reaction window |
|---|---|---|---|---|
| combat.rs:95+108 (enemy defeat) | `queue(AfterEnemyDefeated)` **+** `forced(EnemyDefeated)` | `EnemyDefeated { enemy, by, code }` | `EnemyDefeated` | `AfterEnemyDefeated { enemy, by }` |
| skill_test.rs:670+790 (successful investigate) | `queue(AfterSuccessfulInvestigate)` **+** `forced(AfterLocationInvestigated)` | `SuccessfullyInvestigated { investigator, location }` | `AfterLocationInvestigated` | `AfterSuccessfulInvestigate { investigator }` |
| combat.rs:661 (soak) | `queue(AfterEnemyAttackDamagedAsset)` | `EnemyAttackDamagedSelf { asset, enemy, controller }` | — | `AfterEnemyAttackDamagedAsset { … }` |
| act_agenda.rs:245 | `forced(ActAdvanced { code })` | `ActAdvanced { code }` | `ActAdvanced` | — |
| act_agenda.rs:77 | `forced(AgendaAdvanced { code })` | `AgendaAdvanced { code }` | `AgendaAdvanced` | — |
| mod.rs:230 | `forced(GameEnd)` | `GameEnd` | `GameEnd` | — |
| phases.rs:229 | `forced(EndOfTurn { investigator })` | `EndOfTurn { investigator }` | `EndOfTurn` | — |
| phases.rs:555,647 | `forced(PhaseEnded { phase })` | `PhaseEnded { phase }` | `PhaseEnded` | — |
| phases.rs:661 | `forced(RoundEnded)` | `RoundEnded` | `RoundEnded` | — |
| actions.rs:299 | `forced(EnteredLocation { investigator, location })` | `EnteredLocation { investigator, location }` | `EnteredLocation` | — |

Framework `PlayerWindow(PhaseStep)` windows (`open_fast_window`) have no
`EventPattern` and are **not** `TimingEvent`s — they stay as explicit
`open_fast_window` calls. The two **dual** sites (defeat, investigate) collapse
two calls into one `emit_event`; the investigate one also collapses the C6a twin
patterns (`AfterLocationInvestigated` forced + `SuccessfullyInvestigated`
reaction) into one timing point.

**Log events stay at their call sites.** `emit_event` is dispatch-only — it does
*not* push the logged `Event` (callers already emit `EnemyDefeated`,
`InvestigatorMoved`, etc.). This keeps T5a a pure dispatch-chokepoint
unification with no change to event emission.

**Ordering preservation (T5a).** Each dual site today calls `queue_reaction_window`
*before* `fire_forced_triggers`, so `WindowOpened` is emitted before the forced
effect's events; the forced effect still *resolves* synchronously before the
player can act on the window (RR-correct). `emit_event` replicates this exactly:
**phase-2 queues the reaction window first, then phase-1 resolves forced** —
preserving event order. (T5b revisits forced *resolution* order, not the queue.)

---

## T5a — `TimingEvent` + `emit_event` chokepoint (closes #212, behavior-preserving)

**Files:** create `crates/game-core/src/engine/dispatch/emit.rs`; modify
`forced_triggers.rs` (port collection to `TimingEvent`), the 10 call sites,
`dispatch/mod.rs` (module wiring), `engine/mod.rs` (re-exports for `test_support`).

- [ ] **Step 1: Define `TimingEvent`** in `emit.rs` — one variant per the mapping
  table's middle column, each carrying its binding. Add:
  - `fn forced_pattern(&self) -> EventPattern` (phase-1 match key + `code` narrowing where present).
  - `fn forced_controller(&self, state) -> binding` — port the per-variant controller/source/scan binding from `collect_forced_hits` (lead investigator for board cards; the entering/ending investigator; each instance's controller for GameEnd; etc.).
  - `fn reaction_window(&self) -> Option<WindowKind>` — `Some` only for `EnemyDefeated`, `SuccessfullyInvestigated`, `EnemyAttackDamagedSelf`; `None` otherwise.

- [ ] **Step 2: Port `collect_forced_hits` / `resolve_one`** from `forced_triggers.rs`
  into `emit.rs`, keyed off `TimingEvent` instead of `ForcedTriggerPoint`. The
  body is the same per-variant scan (board cards → threat-area/attachment
  instances, `BTreeMap` order); the resolution is the same fixed-deterministic
  loop (`fire_forced` today) — **no semantic change in T5a**. Delete
  `ForcedTriggerPoint` and `forced_triggers.rs`.

- [ ] **Step 3: Implement `emit_event`**:
  ```
  fn emit_event(cx, te: TimingEvent) -> EngineOutcome {
      // phase 2 first to preserve WindowOpened-before-forced event order
      if let Some(wk) = te.reaction_window() { queue_reaction_window(cx, wk); }
      // phase 1: forced, fixed-order (the ported fire_forced logic)
      run_forced(cx, &te)   // returns Done / AwaitingInput / Rejected as fire_forced did
  }
  ```
  (T5b reorders/replaces phase-1's internals; T5a keeps `run_forced` == today's
  `fire_forced_triggers`.)

- [ ] **Step 4: Migrate the 10 call sites** per the table — each `fire_forced` /
  `queue_reaction` (or the dual pair) becomes one `emit_event(cx, TimingEvent::Z { … })`.
  Update `test_support`'s `fire_forced_at` to drive `emit_event`.

- [ ] **Step 5: Gauntlet** — the full existing suite is the behavior-preserving
  gate (defeat→Roland reaction + act-3 advance; investigate→Milan + Obscuring Fog;
  soak→Guard Dog; RoundEnded; phase-end). All green, unchanged.

- [ ] **Step 6: Commit** — `Closes #212.`

## T5b — iterative forced-ordering loop + reentrancy (closes #213, #294)

**Files:** `emit.rs` (the forced run), `game_state.rs` (the forced-loop frame),
`dispatch/mod.rs` (resume routing), `combat.rs` (remove the #294 `debug_assert`).

- [ ] **Step 1 (failing test): 2+ simultaneous forced → player picks order.**
  Agenda 01107 `RoundEnded` doom + Dissonant Voices 01165 `RoundEnded` discard:
  assert the engine surfaces a choice (lead investigator orders the two) rather
  than resolving in fixed order. (Integration test with the real registry.)

- [ ] **Step 2: The forced run as the *shared* parameterized resolution loop.**
  Per the already-settled "One loop, two phases" decision (umbrella §1 + Axis-B
  spec — we explicitly rejected a separate `ForcedOrdering` frame when
  consolidating forced + reaction ordering): the forced phase is the **same
  `Continuation::Resolution` loop** as the reaction window, run with
  `can_skip=false`, `decider=Lead`, candidates = the collected forced hits.
  So T5b's work is to **generalize the existing `Resolution` frame** (today it
  wraps an `OpenWindow` — the reaction run) to carry the loop params
  (`can_skip` / `decider` / candidate source), then drive a forced run through
  it. Resolve one (lead-chosen when 2+) → re-collect → repeat; a forced hit whose
  effect suspends (Frozen in Fear's `EndOfTurn` test) parks the frame and resumes
  the remaining hits — **reentrancy**. No new frame variant.

- [ ] **Step 3: Reentrancy test** — Frozen in Fear `EndOfTurn` forced effect
  suspends on its willpower test; after the test resolves, dispatch continues /
  completes without abandoning siblings.

- [ ] **Step 4: Dissolve #294** — remove the `drive_attack_loop` `debug_assert`
  (one attack damaging two `EnemyAttackDamagedSelf` reactors) and add the
  two-Guard-Dog multi-soak test: both windows drain, cursor advances once.

- [ ] **Step 5: `/code-review` pass** (pre-push, per the inline-execution plan) —
  present findings before opening the PR.

- [ ] **Step 6: Gauntlet + commit** — `Closes #213. Closes #294.`

## Out of scope (T5)

Axes A/C/D; the Before-timing firing (Axis D); migrating the orthogonal
`pending_*` modes; the #117 index (T6). Newly-arising forced hits mid-loop
(delayed effects) stay out — no Slice-1+ card produces them.

## Risks

- **T5a ordering** — the WindowOpened-before-forced event order at dual sites;
  preserved by queuing the window before resolving forced. The suite's
  event-order assertions are the check.
- **T5b frame shape** — the forced-loop continuation frame must compose with the
  existing `Resolution`/`SkillTest` frames (a forced hit that opens a reaction
  window, or starts a skill test, nests above the forced-loop frame). The
  Frozen-in-Fear + Dissonant-Voices tests exercise this.
