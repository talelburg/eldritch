# Slice A-i: `TimingPointWindow` replaces event windows + forced run — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task (inline execution with checkpoints — chosen). Steps use checkbox (`- [ ]`) syntax.

**Goal:** Replace `Continuation::Resolution(ResolutionFrame{kind: ResolutionKind::{Window(event windows) | Forced}})` with a new `Continuation::TimingPointWindow { event: TimingEvent, mode: TimingMode, candidates }`, **behaviour-preserving**. Framework player windows (`PlayerWindow`/`SkillTestPlayerWindow`) stay on the old `Resolution` path (Slice A-ii's job). Imperative driving preserved (no `drive`-loop arm yet — Slice A-iv).

**Architecture:** The event windows and the #213 forced run are two `mode`s of one frame keyed by the `emit::TimingEvent` that opened them. `TimingEvent`'s data is what `WindowKind`'s event variants already carry (`reaction_window()` proves the 1:1 map), so the migration stores the `TimingEvent` directly and reads `enemy`/`count`/continuation from it instead of from `WindowKind`. During A-i, the shared drivers (`advance_resolution`, `close_reaction_window_at`, `resume_window`) route **both** `Resolution` (framework windows) and `TimingPointWindow` (event + forced) — intentional transitional coexistence.

**Tech Stack:** Rust, `cargo` workspace. Engine crate `crates/game-core`.

## Global Constraints

- **Behaviour-preserving.** The full engine + integration suite stays green at each task boundary. No rules change.
- **CI gauntlet before every push** (`CLAUDE.md` Commands): `RUSTFLAGS="-D warnings" cargo test --all --all-features`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo fmt --check`, `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`, plus the wasm jobs.
- **Validate-first / mutate-second** handler contract preserved.
- **PR per task** toward #433 (closes only when all of Slice A lands). Branch `<scope>/<slug>`, commit `engine: <desc>`, body ends `Closes #NN.` where applicable (these are partial — reference `Part of #433`).
- **Card text/rules:** never paraphrase from memory — look up ArkhamDB if any card behaviour is in question.

---

## Task 1: Introduce the new taxonomy types (`TimingEvent` serde + `TimingMode` + `Continuation::TimingPointWindow`)

PR1 — branch `engine/timing-point-window` (already created; the planning-docs commit is here). Purely additive; nothing constructs the new frame yet.

**No relocation needed.** `TimingEvent` stays in `engine::dispatch::emit`; the new `state` variant references it in place. Precedent: `EffectFrame` (state, `game_state.rs:622`) already holds `ctx: crate::engine::EvalContext`, so `Continuation` referencing an `engine` type is established (#345). This avoids the 59-site import churn a relocation would cost.

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/emit.rs` — add `Serialize, Deserialize` to `TimingEvent`'s derive **and change `pub(crate) enum` → `pub enum`** (it becomes a field of the `pub` `Continuation`, so `private_interfaces` requires `pub` — matches `WindowKind`/`ResolutionFrame`/`EvalContext`). Its fields (`InvestigatorId`, `LocationId`, `Phase`, `CardCode`, `EnemyId`, `CardInstanceId`, `u8`) all already derive serde via their use in `Event` (event.rs). The type-level doc must **de-link** its intra-doc refs to private items (`ForcedTriggerPoint`, `Self::forced_point`, `Self::reaction_window`) to plain code spans, else `RUSTDOCFLAGS="-D warnings"` fails once the type is `pub`.
- Modify: `crates/game-core/src/engine/mod.rs` — split the re-export (line 46) so `TimingEvent` is `pub use` (matching `EvalContext`) while `emit_event` stays `pub(crate) use`.
- Modify: `crates/game-core/src/state/game_state.rs` — add `enum TimingMode` and the `Continuation::TimingPointWindow` variant (referencing `crate::engine::TimingEvent`, the re-export).
- Modify: every `match continuation` / `match top` site that must now be exhaustive over the new variant — stub arms.

**Interfaces:**
- Produces:
  ```rust
  // crate::state (game_state.rs)
  #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
  pub enum TimingMode {
      /// A reaction/fast window: skippable, admits Fast plays.
      Reaction,
      /// The #213 forced run: mandatory, no Fast plays; resumes the framework
      /// flow named by the continuation on close.
      Forced(ForcedContinuation),
  }

  // Continuation enum, new variant:
  /// An event reaction window or the #213 forced run, keyed by the
  /// `TimingEvent` that opened it. Replaces `Resolution{Window(event) | Forced}`
  /// (Slice A-i). Framework player windows stay on `Resolution` until A-ii.
  TimingPointWindow {
      event: crate::engine::TimingEvent,
      mode: TimingMode,
      candidates: Vec<ResolutionCandidate>,
  },
  ```
- `TimingEvent`'s mapping methods (`forced_point`/`reaction_window`/`forced_continuation`) stay in `emit.rs` unchanged (only its derive, visibility, and type-level doc links change).

- [ ] **Step 1: Make `TimingEvent` serializable + `pub`.** Change its derive (emit.rs:46) to `#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]` and `pub(crate) enum` → `pub enum`; add `use serde::{Deserialize, Serialize};` to `emit.rs`. De-link the type-level doc's refs to private items (`ForcedTriggerPoint`/`Self::forced_point`/`Self::reaction_window`) to plain code spans. Split the `engine/mod.rs:46` re-export so `TimingEvent` is `pub use`.

- [ ] **Step 2: Build.** Run `cargo build -p game-core`. Expected: compiles, no `private_interfaces` or doc warnings. (Field types all derive serde via `Event`.)

- [ ] **Step 3: Add `TimingMode` + the `Continuation::TimingPointWindow` variant.** Insert both into `game_state.rs` as in Interfaces.

- [ ] **Step 4: Build — find exhaustiveness gaps.** Run `cargo build -p game-core 2>&1 | grep -A3 'non-exhaustive\|not covered'`. Every `match` over `Continuation` now errors. List them (expect: `resolve_input` in `dispatch/mod.rs`, the `drive` loop's `match top`, any `is_phase_anchor`/serialization/debug helpers).

- [ ] **Step 5: Stub the new arms (unreachable — nothing constructs it yet).** For each gap:
  - `resolve_input` (`dispatch/mod.rs`): add
    ```rust
    Some(Continuation::TimingPointWindow { .. }) => EngineOutcome::Rejected {
        reason: "ResolveInput: TimingPointWindow not yet constructed (Slice A-i task 2/3)".into(),
    },
    ```
  - `drive` loop `match top` (`dispatch/mod.rs`): the existing `_ => return EngineOutcome::Done` already covers it — confirm no explicit arm needed.
  - Any other `match` (e.g. a `Continuation` Debug/label helper): add a minimal arm mirroring the `Resolution` arm's shape.

- [ ] **Step 6: Full suite green.** Run `RUSTFLAGS="-D warnings" cargo test --all --all-features`. Expected: PASS (additive; no constructor reachable). Then `cargo clippy --all-targets --all-features -- -D warnings` and `cargo fmt --check`.

- [ ] **Step 7: Commit + push + PR.**
  ```bash
  git add -A && git commit -m "engine: TimingPointWindow taxonomy types (Slice A-i task 1)
  # body: add serde to TimingEvent; add TimingMode and
  # Continuation::TimingPointWindow (additive, stubbed, referenced in place). Part of #433.
  "
  ```
  Run the full CI gauntlet, push branch, open PR referencing `Part of #433` under umbrella #435. **Checkpoint: stop for review before Task 2.**

---

## Task 2: Migrate event windows + forced run to `TimingPointWindow` (+ tests)

PR2 — branch `engine/timing-point-forced`. **Re-cut from the original Task 2/Task 3 split:** the forced run and event reaction windows share one driver (`open_forced_resolution` calls `open_queued_reaction_window`, the same path reaction windows use; `advance_resolution` / `fire_pending_trigger` / `close_reaction_window_at` / `build_resolution_options` all key on `as_resolution()`, differing only by `kind: Window | Forced`). Splitting them would rework that driver twice, so they migrate together. The framework player windows (`WindowKind::PlayerWindow` / `SkillTestPlayerWindow`) are the genuinely separable piece and **stay on `Resolution`** until Slice A-ii — so the driver dual-handles `Resolution` (framework only) + `TimingPointWindow` (event + forced) for the duration of A-i.

**Strategy:** introduce a frame-agnostic accessor pair so the shared driver reads candidates + mode uniformly across both representations, then flip *construction* (forced + event) to `TimingPointWindow`. Keep `WindowOpened`/`WindowClosed { kind }` event payloads byte-identical (derive the `WindowKind` from the `TimingEvent` via `reaction_window()` at the emit/close site) so behaviour is preserved.

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/reaction_windows.rs`:
  - `open_forced_resolution(cx, event, candidates, continuation)` — gains `event: TimingEvent`; pushes `TimingPointWindow { event, mode: Forced(continuation), candidates }`.
  - `queue_reaction_window` / `open_queued_reaction_window` — event windows push `TimingPointWindow { event, mode: Reaction, candidates }`; framework windows keep pushing `Resolution`.
  - The shared driver (`advance_resolution`, `fire_pending_trigger`, `close_reaction_window_at`, `build_resolution_options`, `resume_window`/`resume_reaction_window`) reads through new accessors that handle both frames (see Interfaces).
  - `fire_pending_trigger` `eval_ctx` binding — re-key off `TimingEvent::{EnemyAttackDamagedSelf{enemy,..}, EnemyAttacks{enemy,..}, WouldDiscoverClues{count,..}}` instead of the `WindowKind` fields.
  - `run_window_continuation` — for event windows the per-event close continuation (`WouldDiscoverClues` → discover, `EnemyAttacks` → proceed-with-attack; the `After*` events just pop) keys off `TimingEvent`. The forced run's close path (`mode: Forced`) → `resume_forced_continuation(cx, continuation)`.
- Modify: `crates/game-core/src/engine/dispatch/emit.rs` — `emit_event` (lines 302-329): `queue_reaction_window(cx, event.reaction_window())` becomes pushing the `TimingPointWindow{Reaction}` when `event.reaction_window().is_some()` (keep the `WindowKind` only to gate "does this open a window" + to fill the `WindowOpened` payload); `open_forced_resolution` gains the `event` arg (clone the in-flight `&TimingEvent`).
- Modify: `crates/game-core/src/engine/dispatch/skill_test.rs` — the teardown-tail forced reader `Some(Continuation::Resolution(f)) if f.is_forced()` (line ~556) also recognises `TimingPointWindow { mode: Forced(..), .. }`.
- Modify: `crates/game-core/src/engine/dispatch/mod.rs` — `resolve_input` routes both modes of `TimingPointWindow` to `resume_window` (replacing the Task-1 reject stub). Forced runs aren't prompted for *which* (order-only when 2+); reaction windows route exactly as `Resolution{Window}` does today.
- Test: engine unit tests + `crates/cards/tests/{evidence,dodge,dodge_aoo,guard_dog_soak,roland_banks,play_card_aoo,fast_play,mind_over_matter,activate_ability_aoo,retaliate_windows}.rs` — update any assertions reaching into the *continuation-stack shape* (`Resolution{..}` → `TimingPointWindow{..}`). Event-payload assertions (`WindowOpened/Closed { kind }`) stay unchanged (payloads preserved).

**Interfaces:**
- Consumes: Task 1's `Continuation::TimingPointWindow { event, mode, candidates }` + `TimingMode`.
- Produces: new accessors on `Continuation` (game_state.rs) so the driver is representation-agnostic, e.g.
  ```rust
  /// Candidates of an open window/run, whether `Resolution` (framework) or
  /// `TimingPointWindow` (event/forced).
  fn pending_candidates_mut(&mut self) -> Option<&mut Vec<ResolutionCandidate>>;
  /// `Some(continuation)` iff this is a forced run (either representation).
  fn forced_continuation(&self) -> Option<ForcedContinuation>;
  fn is_forced(&self) -> bool;
  ```
  After this task `WindowKind` is constructed only for framework windows (+ derived transiently for `WindowOpened/Closed`). Forced runs + event windows live as `TimingPointWindow`. This is what A-ii (FastWindow) and A-iii (delete `WindowKind`) build on.

- [ ] **Step 1: Add the frame-agnostic accessors** (`pending_candidates`/`_mut`, `forced_continuation`, `is_forced`) on `Continuation`, handling both `Resolution` and `TimingPointWindow`. Behaviour-preserving (nothing constructs `TimingPointWindow` yet). Build green.
- [ ] **Step 2: Route the shared driver through the accessors.** Replace direct `as_resolution()`/`is_forced()`/`forced_continuation()` reads in `advance_resolution`, `fire_pending_trigger`, `close_reaction_window_at`, `build_resolution_options`, `resume_window`, and `skill_test::advance`'s teardown tail. Still all on `Resolution` — full suite green (pure refactor checkpoint).
- [ ] **Step 3: Write the failing tests.** (a) a 2+-simultaneous-forced emit parks `TimingPointWindow { mode: Forced(_), .. }`; (b) a `SuccessfullyInvestigated` emit parks `TimingPointWindow { event: SuccessfullyInvestigated{..}, mode: Reaction, .. }`. Assert firing behaviour unchanged via the event-assertion macros. Run — verify FAIL.
- [ ] **Step 4: Flip construction.** `open_forced_resolution` → `TimingPointWindow{Forced}`; `queue_reaction_window`/`emit_event` → `TimingPointWindow{Reaction}` for event windows. Re-key the `eval_ctx` binding + `run_window_continuation` off `TimingEvent`; preserve `WindowOpened/Closed { kind }` via `event.reaction_window()`.
- [ ] **Step 5: Route `resolve_input`** both `TimingPointWindow` modes → `resume_window` (replace Task-1 stub).
- [ ] **Step 6: Run the new tests — verify PASS.**
- [ ] **Step 7: Update broken stack-shape assertions** in the enumerated tests. Re-run each.
- [ ] **Step 8: Full strict gauntlet green** (`RUSTFLAGS="-D warnings" cargo test --all --all-features`, host + wasm clippy, fmt, doc, wasm build).
- [ ] **Step 9: Commit + CI gauntlet + push + PR** (`engine: migrate event windows + forced run to TimingPointWindow (Slice A-i task 2). Part of #433.`). **Checkpoint: Slice A-i nearly done — only A-iii (delete the now-unused `ResolutionKind::Forced` arm + dead WindowKind event variants) + A-ii (framework → FastWindow) remain.**

---

## Self-Review

**Spec coverage (against `2026-06-22-emitevent-frame-arc-decomposition-design.md` §"Slice A detail" A-i):**
- "TimingPointWindow replaces event windows + forced run" → Tasks 2 (forced) + 3 (event). ✓
- "map event WindowKind → emit::TimingEvent" → Task 2 stores `TimingEvent` directly; mapping via existing `reaction_window()`. ✓
- "ForcedContinuation rides the mode: Forced close path" → Task 2 step 4. ✓
- "imperative driving preserved (no drive arm yet)" → no `drive`-loop arm added; `resolve_input` + `advance_resolution`/`close_reaction_window_at` keep imperative re-entry. ✓
- "framework windows stay on Resolution" → Tasks explicitly leave `PlayerWindow`/`SkillTestPlayerWindow` on `Resolution`. ✓
- "behaviour-preserving" → every task ends on full-suite-green; `WindowOpened/Closed` payloads preserved. ✓

**Surfaced design decision (not in the spec, decided here):** `TimingEvent` must gain `Serialize`/`Deserialize` (Continuation serializes). It does **not** need to relocate to `state` — `Continuation::Effect`/`EffectFrame` already holds `crate::engine::EvalContext` (#345), so a `state` variant referencing the in-`engine` `TimingEvent` is established precedent. Reference-in-place avoids 59 sites of relocation churn. Folded into Task 1.

**Placeholder scan:** none — every step names exact files/sites/commands. The per-site `eval_ctx`/continuation arms are enumerated by their existing `WindowKind` variants (Task 2 lists them); exact arm bodies are read from the current code at execution time (inline execution).

**Type consistency:** `TimingEvent` (state), `TimingMode { Reaction, Forced(ForcedContinuation) }`, `TimingPointWindow { event, mode, candidates }` used consistently across tasks. `open_forced_resolution` gains `event: TimingEvent` (Task 2) consistent with the frame field.

**Risk note:** Task 2's `run_window_continuation` re-key is the subtle step — the `Before*` windows (`BeforeDiscoverClues`, `BeforeEnemyAttack`) have real close continuations (discover / proceed-with-attack), unlike the `After*` windows that just pop. Verify each arm against the current `run_window_continuation` body before flipping, and keep a unit test per `Before*` window.
