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

## Task 2: Migrate the forced run to `TimingPointWindow { mode: Forced }`

PR2 — branch `engine/timing-point-forced` (off `main` after PR1 merges, or off PR1's branch if stacked). Self-contained: the forced run has no Fast plays and no per-`WindowKind` continuation — only a `ForcedContinuation`.

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/reaction_windows.rs` — `open_forced_resolution` (constructs `Resolution{Forced}`), `close_reaction_window_at` (the `forced_continuation()` branch → `resume_forced_continuation`), `advance_resolution`, `resume_window`/`resume_reaction_window`, and the forced-run reader in `skill_test::advance`'s teardown tail (`Some(Continuation::Resolution(f)) if f.is_forced()`).
- Modify: `crates/game-core/src/engine/dispatch/mod.rs` — `resolve_input` routes the forced `TimingPointWindow` (replace the Task-1 stub for `mode: Forced`).
- Test: `crates/cards/tests/retaliate_windows.rs` and any forced-run engine unit tests asserting on `Resolution{Forced}`.

**Interfaces:**
- Consumes: `Continuation::TimingPointWindow { event, mode: TimingMode::Forced(continuation), candidates }` from Task 1.
- Produces: `open_forced_resolution(cx, event: TimingEvent, candidates, continuation)` — signature gains the `event` (the emit's `&TimingEvent`, cloned) so the frame carries it. Callers in `emit_event` pass the in-flight event.

- [ ] **Step 1: Write/adjust the failing test.** In an engine unit test (or extend an existing forced-run test), assert that a 2+-simultaneous-forced emit parks a `Continuation::TimingPointWindow { mode: TimingMode::Forced(_), .. }` (not `Resolution`). Use the `TestGame` builder + a board producing 2 forced hits at one point (mirror the existing `open_forced_resolution` test setup — locate via `grep -rn 'open_forced_resolution\|is_forced' crates/game-core/src`).
- [ ] **Step 2: Run it — verify it fails** (`Resolution` still constructed). Run the specific test; expect FAIL on the variant assertion.
- [ ] **Step 3: Flip `open_forced_resolution`** to push `TimingPointWindow { event, mode: Forced(continuation), candidates }`. Thread `event: TimingEvent` from `emit_event` (it has `event: &TimingEvent`; clone it).
- [ ] **Step 4: Flip the close/resume path.** `close_reaction_window_at` (and/or a new `TimingPointWindow`-aware close) reads `mode: Forced(continuation)` → `resume_forced_continuation(cx, continuation)`. Update `is_forced()` readers (`skill_test::advance` teardown tail, any `as_resolution().is_forced()`) to also recognise `TimingPointWindow { mode: Forced(..), .. }`. Introduce a small helper `fn top_forced_continuation(cx) -> Option<ForcedContinuation>` if the pattern repeats.
- [ ] **Step 5: Route `resolve_input`.** Replace Task-1's `mode: Forced` stub: forced runs are not player-prompted for *which* (order only when 2+), so route to the existing forced resolve/advance logic — match the current `Resolution{Forced}` routing.
- [ ] **Step 6: Run the failing test — verify PASS.**
- [ ] **Step 7: Update broken tests** asserting `Resolution{Forced}` → `TimingPointWindow{Forced}`. Re-run `retaliate_windows.rs` + forced-run unit tests.
- [ ] **Step 8: Full suite green** (`RUSTFLAGS="-D warnings" cargo test --all --all-features`) + clippy + fmt + doc.
- [ ] **Step 9: Commit + CI gauntlet + push + PR** (`engine: migrate forced run to TimingPointWindow (Slice A-i task 2). Part of #433.`). **Checkpoint: stop for review before Task 3.**

---

## Task 3: Migrate event reaction windows to `TimingPointWindow { mode: Reaction }` (+ tests)

PR3 — branch `engine/timing-point-reaction-windows`. The larger of the three: flips the event-window queue + the `WindowKind`-keyed `eval_ctx` binding and continuation to key off `TimingEvent`. Framework `PlayerWindow`/`SkillTestPlayerWindow` stay on `Resolution`.

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/emit.rs` — `emit_event` (line 302-329): `queue_reaction_window(cx, kind: WindowKind)` becomes pushing a `TimingPointWindow { event, mode: Reaction }`. `reaction_window()` currently maps `TimingEvent → WindowKind`; the migrated frame stores the `TimingEvent` itself, so the `Some(kind)` guard becomes a `if event.opens_reaction_window()` boolean check (add that predicate, or reuse `reaction_window().is_some()`).
- Modify: `crates/game-core/src/engine/dispatch/reaction_windows.rs`:
  - `queue_reaction_window` / `open_queued_reaction_window` — construct/emit the `TimingPointWindow` (event windows) while still handling `Resolution` (framework windows).
  - `fire_pending_trigger` — the `eval_ctx` binding `match …kind() { AfterEnemyAttackDamagedAsset{enemy,..} => set_attacking_enemy, BeforeDiscoverClues{count,..} => set_clue_discovery_count }` re-keys off `TimingEvent::{EnemyAttackDamagedSelf{enemy,..}, WouldDiscoverClues{count,..}}`.
  - `run_window_continuation(kind: WindowKind)` — for event windows, the per-kind close continuation (e.g. `BeforeDiscoverClues` → discover, `BeforeEnemyAttack` → proceed) keys off `TimingEvent` instead. `AfterEnemyDefeated`/`AfterSuccessfulInvestigate`/`AfterEnteredPlay`/`AfterEnemyAttackDamagedAsset` "simply pop" — confirm via the current arms.
  - `build_resolution_options` / `advance_resolution` / `close_reaction_window_at` — handle both frame types (the `WindowClosed { kind }` event still needs a `WindowKind`; derive it from the `TimingEvent` via `reaction_window()` for the event so observability is unchanged).
- Modify: `crates/game-core/src/engine/dispatch/mod.rs` — `resolve_input` routes the reaction `TimingPointWindow` to `resume_window`.
- Test: `crates/cards/tests/{evidence,dodge,dodge_aoo,guard_dog_soak,roland_banks,play_card_aoo,fast_play,mind_over_matter,activate_ability_aoo}.rs` — update any `WindowKind::*` event-window assertions; the `Event::WindowOpened/Closed { kind }` payloads should stay identical (behaviour-preserving), so most assertions on *events* are unchanged — only assertions reaching into the *continuation stack* shape change.

**Interfaces:**
- Consumes: Task 1's `TimingPointWindow`, Task 2's forced migration (the close/route helpers now also serve reaction mode).
- Produces: event reaction windows live as `TimingPointWindow { event, mode: Reaction, candidates }`. `WindowKind` is now constructed **only** for framework `PlayerWindow`/`SkillTestPlayerWindow` (+ derived transiently for the `WindowOpened/Closed` event payload). This is what Slice A-ii (FastWindow) and A-iii (delete `WindowKind`) build on.

- [ ] **Step 1: Write the failing test.** Engine unit test: a `SuccessfullyInvestigated` emit (Dr. Milan-style after-investigate window) parks a `Continuation::TimingPointWindow { event: TimingEvent::SuccessfullyInvestigated{..}, mode: TimingMode::Reaction, .. }`. Assert the firing behaviour (the reaction fires, `eval_ctx` bound) is unchanged via the event-assertion macros.
- [ ] **Step 2: Run it — verify it fails** (still `Resolution{Window}`).
- [ ] **Step 3: Flip the queue path.** `emit_event` + `queue_reaction_window` push `TimingPointWindow{event, mode: Reaction}` for event windows; framework windows untouched.
- [ ] **Step 4: Re-key the `eval_ctx` binding** in `fire_pending_trigger` off `TimingEvent` (enemy from `EnemyAttackDamagedSelf`/`EnemyAttacks`, count from `WouldDiscoverClues`).
- [ ] **Step 5: Re-key `run_window_continuation`** off `TimingEvent`; preserve the `WindowClosed { kind }` event payload by deriving `WindowKind` from the event (`event.reaction_window()`).
- [ ] **Step 6: Route `resolve_input`** reaction `TimingPointWindow` → `resume_window`.
- [ ] **Step 7: Run the failing test — verify PASS.**
- [ ] **Step 8: Update broken card/unit tests** (enumerated above). Re-run each touched `crates/cards/tests/*.rs`.
- [ ] **Step 9: Full suite green** + clippy + fmt + doc + wasm jobs.
- [ ] **Step 10: Commit + CI gauntlet + push + PR** (`engine: migrate event reaction windows to TimingPointWindow (Slice A-i task 3). Part of #433.`). **Checkpoint: Slice A-i complete; reassess A-ii.**

---

## Self-Review

**Spec coverage (against `2026-06-22-emitevent-frame-arc-decomposition-design.md` §"Slice A detail" A-i):**
- "TimingPointWindow replaces event windows + forced run" → Tasks 2 (forced) + 3 (event). ✓
- "map event WindowKind → emit::TimingEvent" → Task 3 stores `TimingEvent` directly; mapping via existing `reaction_window()`. ✓
- "ForcedContinuation rides the mode: Forced close path" → Task 2 step 4. ✓
- "imperative driving preserved (no drive arm yet)" → no `drive`-loop arm added; `resolve_input` + `advance_resolution`/`close_reaction_window_at` keep imperative re-entry. ✓
- "framework windows stay on Resolution" → Tasks explicitly leave `PlayerWindow`/`SkillTestPlayerWindow` on `Resolution`. ✓
- "behaviour-preserving" → every task ends on full-suite-green; `WindowOpened/Closed` payloads preserved. ✓

**Surfaced design decision (not in the spec, decided here):** `TimingEvent` must gain `Serialize`/`Deserialize` (Continuation serializes). It does **not** need to relocate to `state` — `Continuation::Effect`/`EffectFrame` already holds `crate::engine::EvalContext` (#345), so a `state` variant referencing the in-`engine` `TimingEvent` is established precedent. Reference-in-place avoids 59 sites of relocation churn. Folded into Task 1.

**Placeholder scan:** none — every step names exact files/sites/commands. The per-site `eval_ctx`/continuation arms are enumerated by their existing `WindowKind` variants (Task 3 lists them); exact arm bodies are read from the current code at execution time (inline execution).

**Type consistency:** `TimingEvent` (state), `TimingMode { Reaction, Forced(ForcedContinuation) }`, `TimingPointWindow { event, mode, candidates }` used consistently across tasks. `open_forced_resolution` gains `event: TimingEvent` (Task 2) consistent with the frame field.

**Risk note:** Task 3's `run_window_continuation` re-key is the subtle step — the `Before*` windows (`BeforeDiscoverClues`, `BeforeEnemyAttack`) have real close continuations (discover / proceed-with-attack), unlike the `After*` windows that just pop. Verify each arm against the current `run_window_continuation` body before flipping, and keep a unit test per `Before*` window.
