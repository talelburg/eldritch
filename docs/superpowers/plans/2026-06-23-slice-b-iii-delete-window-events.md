# Slice B-iii — Delete `WindowOpened`/`WindowClosed` + `WindowKind` Implementation Plan

> **For agentic workers:** Use superpowers:executing-plans. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Delete the `Event::WindowOpened`/`WindowClosed` events and the `WindowKind` type. `WindowKind` has three consumers — event payload (delete), scan eligibility, and window close-continuation routing — the latter two migrate onto `TimingEvent` (reaction windows) / `FastWindowKind` (fast windows), the single source of truth already on the frames.

**Architecture:** 3 commits, each green. (1) Migrate the scan + close-routing + gate off `WindowKind` onto `TimingEvent`/`FastWindowKind`, keeping `WindowKind` alive only for the event payload (behaviour-preserving). (2) Delete the events + migrate tests. (3) Delete `WindowKind` + the dead derivations.

**Parent spec:** [`2026-06-23-emitevent-frame-slice-b-coordinators-design.md`](../specs/2026-06-23-emitevent-frame-slice-b-coordinators-design.md) §"`WindowKind` + `WindowOpened`/`WindowClosed` deletion". Issue: #434 (Slice B-iii). Branch: `engine/delete-window-events`.

## Global Constraints
- **Commit 1 behaviour-preserving** (event log byte-identical — `WindowOpened/Closed` still emitted via the surviving `window_kind()`/`reaction_window()`). Commits 2–3 delete the events (the deliberate log change) but no game-outcome change.
- CI gauntlet before push (test / fmt / clippy host+wasm / doc / wasm-build).
- The isomorphism the migration relies on (verbatim from `TimingEvent::reaction_window()`): `EnemyDefeated{enemy,by}↔AfterEnemyDefeated`, `EnemyAttackDamagedSelf{asset,enemy,controller}↔AfterEnemyAttackDamagedAsset`, `SuccessfullyInvestigated{investigator}↔AfterSuccessfulInvestigate`, `EnemyAttacks{enemy,investigator}↔BeforeEnemyAttack`, `WouldDiscoverClues{investigator,location,count}↔BeforeDiscoverClues`, `EnteredPlay{instance,controller}↔AfterEnteredPlay`.

---

### Task 1 (commit 1): Migrate scan + close-routing + gate onto `TimingEvent`/`FastWindowKind`

**Files:** `crates/game-core/src/engine/dispatch/reaction_windows.rs` (scans, `trigger_matches`, `run_window_continuation`, `close_reaction_window_at`, `queue_reaction_window`).

- [ ] **Step 1 — `scan_pending_triggers(state, event: &TimingEvent)`** (was `kind: WindowKind`). Rewrite the eligibility `if let WindowKind::X = kind` arms to match the `TimingEvent`:
  - `BeforeEnemyAttack{investigator}` → `TimingEvent::EnemyAttacks{investigator, ..}` (co-location gate).
  - `BeforeDiscoverClues{investigator, location}` → `TimingEvent::WouldDiscoverClues{investigator, location, ..}` (you + your-location gate, + the `card.clues == 0` gate).
  - `AfterEnemyAttackDamagedAsset{asset}` → `TimingEvent::EnemyAttackDamagedSelf{asset, ..}` (instance self-bind).
  - `AfterEnteredPlay{instance}` → `TimingEvent::EnteredPlay{instance, ..}` (instance self-bind).
- [ ] **Step 2 — `scan_hand_fast_events(state, event: &TimingEvent)`**: same `BeforeEnemyAttack` → `EnemyAttacks` co-location gate.
- [ ] **Step 3 — `trigger_matches(event: &TimingEvent, pattern, timing, controller)`** (was `kind: WindowKind`). Rewrite the `match (kind, pattern)` to `match (event, pattern)`:
  - When-timing: `(EnemyAttacks, EnemyAttacks)` / `(WouldDiscoverClues, WouldDiscoverClues)` → true.
  - `(EnemyDefeated{by,..}, EnemyDefeated{by_controller})` → `by_controller ? by==Some(controller) : true`.
  - `(EnemyAttackDamagedSelf, EnemyAttackDamagedSelf)` → true; `(SuccessfullyInvestigated{investigator}, SuccessfullyInvestigated)` → `investigator==controller`; `(EnteredPlay{controller:wc,..}, EnteredPlay)` → `wc==controller`.
  - Catch-all → false. (The `TimingEvent` is the exhaustive scrutinee now; no `PlayerWindow`/`SkillTest` arms — fast windows never reach `trigger_matches`.)
- [ ] **Step 4 — `queue_reaction_window`**: pass `event` to `scan_pending_triggers`/`scan_hand_fast_events`. **Keep** `let kind = event.reaction_window()…` for the `WindowOpened { kind }` emit (deleted in Task 2).
- [ ] **Step 5 — `run_window_continuation` / `close_reaction_window_at`**: dispatch the close routing on the frame content, not `removed.window_kind()`. For a `TimingPointWindow{event}` reaction close, match the `TimingEvent`: `EnemyAttacks | EnemyAttackDamagedSelf → resume_enemy_attack`; `WouldDiscoverClues → resume_before_discover_window`; `EnemyDefeated | SuccessfullyInvestigated | EnteredPlay → Done`. For a `FastWindow{kind}` close, match `FastWindowKind`: `Phase → anchor_on_child_pop`; `SkillTest → skill_test::advance`. **Keep** the `WindowClosed { kind }` emit (via `window_kind()`) for now.
- [ ] **Step 6 — build + full suite green** (behaviour-preserving; event log unchanged). `cargo build --all` then `RUSTFLAGS="-D warnings" cargo test --all --all-features --no-fail-fast`. Expect 0 failures.
- [ ] **Step 7 — commit** `engine: migrate the window scan + close-routing off WindowKind onto TimingEvent (Slice B-iii task 1)`.

---

### Task 2 (commit 2): Delete `Event::WindowOpened` / `WindowClosed`

**Files:** `crates/game-core/src/event.rs` (variants), `reaction_windows.rs` (3 emit sites), `phases.rs`/`skill_test.rs`/`encounter.rs` (fast-window emits via `open_fast_window`), + test files.

- [ ] **Step 1 — delete the emit sites**: `queue_reaction_window` `cx.events.push(Event::WindowOpened{kind})` (drop the now-unused `kind` binding); `close_reaction_window_at` `WindowClosed{kind}`; `open_fast_window`'s `WindowOpened` + auto-skip `WindowClosed`.
- [ ] **Step 2 — delete the `Event::WindowOpened` / `WindowClosed` variants** + doc comments (`event.rs:474–511`); drop the now-unused `WindowKind` import in `event.rs`.
- [ ] **Step 3 — migrate tests.** Drop the sequence-anchor assertions (skill_test.rs auto-skip-windows test; `crates/scenarios/tests/mythos_phase.rs` lines ~755/763/866; the `WindowClosed` anchor in `crates/cards/tests/evidence.rs:169`; and the `WindowOpened/Closed` anchors in the combat/reaction card tests — `dodge*.rs`, `guard_dog_soak.rs`, `retaliate_windows.rs`, `roland_banks.rs`, `play_card_aoo.rs`, `activate_ability_aoo.rs`, `phases.rs` unit tests). **Rewrite the primary window-presence assertions** (`evidence.rs:96` "after-defeat window opens and offers Evidence" — assert the `apply` returned `AwaitingInput` with Evidence among the offered options instead of `WindowOpened{AfterEnemyDefeated}`). Grep `WindowOpened\|WindowClosed` to find every site; the compiler + the grep are the safety net.
- [ ] **Step 4 — build + full suite green.** The deliberate change: `WindowOpened/Closed` no longer in any event log. 0 failures.
- [ ] **Step 5 — commit** `engine: delete Event::WindowOpened/WindowClosed (Slice B-iii task 2)`.

---

### Task 3 (commit 3): Delete `WindowKind`

**Files:** `game_state.rs` (`WindowKind` enum, `Continuation::window_kind()`, `FastWindowKind::window_kind()`), `emit.rs` (`reaction_window()`), `builder.rs` (`with_open_window`).

- [ ] **Step 1 — replace the `reaction_window().is_some()` gate.** Add `TimingEvent::opens_reaction_window(&self) -> bool` (the reaction-capable variants: `EnemyDefeated | EnemyAttackDamagedSelf | SuccessfullyInvestigated | EnemyAttacks | WouldDiscoverClues | EnteredPlay`) and use it at the `emit_event` gate (`emit.rs:305`) in place of `event.reaction_window().is_some()`.
- [ ] **Step 2 — delete** `TimingEvent::reaction_window()`, `Continuation::window_kind()`, `FastWindowKind::window_kind()`, and the `WindowKind` enum itself.
- [ ] **Step 3 — `with_open_window` builder** (`builder.rs:267`): take `FastWindowKind` directly instead of `WindowKind` (drop the `WindowKind→FastWindowKind` mapping). Update its callers in tests.
- [ ] **Step 4 — build (compiler confirms `WindowKind` is fully gone)** + full suite + gauntlet. Grep `WindowKind` → only the spec/plan docs remain.
- [ ] **Step 5 — commit** `engine: delete WindowKind (Slice B-iii task 3)`.

---

### Task 4: Gauntlet + PR
- [ ] Full gauntlet (six jobs). Push `engine/delete-window-events`. Open PR — title `engine: delete WindowOpened/Closed + WindowKind (Slice B-iii)`; body: events are pure output + 1:1 redundant with AwaitingInput; WindowKind's scan + close-routing migrated to TimingEvent/FastWindowKind. "Closes #434 / #435" (B-iii finishes Slice B). Watch CI.
- [ ] **Phase doc:** Slice B is now complete (B-i/B-ii/B-iii) — update `docs/phases/phase-7-the-gathering.md` Ordering step 6 + the EmitEvent-frame arc bullet to mark Slice B shipped and the coordinator frames + §G as moved to Slice C. (This is also the deferred doc-reconciliation flagged in the B-ii recap.)

## Self-Review
- Coverage: events deleted (T2) ✓; scan migrated (T1) ✓; close-routing migrated (T1) ✓; gate replaced (T3) ✓; WindowKind deleted (T3) ✓; builder + tests (T2/T3) ✓.
- `PhaseStep` is **kept** (used by `FastWindowKind::Phase`); only `WindowKind`'s `PlayerWindow`/`SkillTestPlayerWindow` go.
- Atomicity: T1 behaviour-preserving (WindowKind alive for the emit); T2 deletes events once nothing else uses them; T3 deletes the type once both other consumers are migrated.
