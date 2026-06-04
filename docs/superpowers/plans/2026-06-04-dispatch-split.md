# Split `engine/dispatch.rs` into per-domain submodules — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Relocate the 9,569-line `crates/game-core/src/engine/dispatch.rs` into ~13 flat per-domain submodules under `engine/dispatch/`, with zero behavior change.

**Architecture:** Pure mechanical move. `dispatch.rs` becomes `dispatch/mod.rs` (entry dispatchers + `resolve_input` router + submodule declarations); each cluster of functions + its `#[cfg(test)] mod *_tests` block moves to a sibling file. Cross-module calls use `pub(super)`/`pub(crate)` and path qualifiers. The existing test suite is the correctness oracle.

**Tech Stack:** Rust 2021, `game-core` kernel crate (no I/O, `wasm32`-clean).

---

## How this plan differs from a feature plan

This is a **behavior-preserving relocation**, not new behavior, so there is no red→green TDD loop. The verification at every task is: **the existing suite still passes and the diff is move-only.** Function bodies move **verbatim** — the *only* permitted edits are:

- `use` imports at the top of each new module,
- visibility keywords (`pub(super)` ↔ `pub(crate)` where a cross-module reach requires it),
- cross-module path qualifiers at call sites (`foo(...)` → `super::cards::foo(...)`).

No control-flow, no logic, no renames, no doc-comment rewrites. Because the bodies already exist and are named precisely below (by symbol + current line range in the pre-split file), this plan **identifies what to move** rather than reproducing 5,000 lines of unchanged code.

### THE GAUNTLET (run after every task before committing)

```sh
cargo fmt                                                            # normalize moved code
cargo fmt --check
RUSTFLAGS="-D warnings"     cargo test --all --all-features
                            cargo clippy --all-targets --all-features -- -D warnings
RUSTDOCFLAGS="-D warnings"  cargo doc --workspace --no-deps --all-features
                            cargo build -p web --target wasm32-unknown-unknown
```

Expected after every task: all five green. The `test` and `clippy` runs are the fast inner loop and catch unresolved paths / visibility errors immediately; `doc`/`wasm`/`fmt` are quick confirmations. Per the spec, **every commit must be green** — do not stack a broken intermediate.

### Per-task move procedure (applies to Tasks 1–12)

1. Create `crates/game-core/src/engine/dispatch/<module>.rs`.
2. Add `mod <module>;` to `dispatch/mod.rs` (alphabetical with the others).
3. Cut the listed functions (+ const, + private type if listed) from `dispatch/mod.rs` and paste them into `<module>.rs`, **bodies verbatim**.
4. Cut the listed `#[cfg(test)] mod *_tests` block(s) and paste at the bottom of `<module>.rs`.
5. Add a `use` block at the top of `<module>.rs` importing the types/functions the moved code references (crate paths like `crate::state::{...}`, `crate::event::Event`; sibling helpers like `super::cursor::first_active_investigator`). Let `clippy`'s `unused_imports`/`unresolved` guide the exact set.
6. **Back-patch call sites:** anything in `mod.rs`, `resolve_input`, the entry dispatchers, or already-extracted sibling modules that called the just-moved functions now needs `super::<module>::<fn>` (or `crate::engine::dispatch::<module>::<fn>`). The compiler flags every missed one.
7. Run THE GAUNTLET. Fix paths/visibility until green.
8. Commit.

### Module extraction order (callees before callers, to minimize back-patching)

`cursor` → `skill_test` → `reaction_windows` → `actions` → `combat` → `elimination` → `hunters` → `encounter` → `act_agenda` → `cards` → `abilities` → `phases` (extracted last; it calls into nearly everything, so by the time it moves its callees already sit in their final modules).

> Line ranges below are **as of the current pre-split `dispatch.rs`** (commit at branch point). They locate symbols; they shift as earlier tasks remove code, so locate by symbol name, using the range as a hint.

---

## Task 0: Convert `dispatch.rs` to a directory module

**Files:**
- Rename: `crates/game-core/src/engine/dispatch.rs` → `crates/game-core/src/engine/dispatch/mod.rs`

- [ ] **Step 1: Move the file with git**

```sh
cd crates/game-core/src/engine
mkdir dispatch
git mv dispatch.rs dispatch/mod.rs
```

- [ ] **Step 2: Run THE GAUNTLET**

Expected: all green. This is a no-op for the module tree (`engine::dispatch` still resolves to the same code), so nothing else changes. `engine/mod.rs`'s `mod dispatch;` is unaffected.

- [ ] **Step 3: Commit**

```sh
git add -A
git commit -m "engine: convert dispatch.rs to dispatch/mod.rs (#159)

Pure file move ahead of the per-domain submodule split. No code change.

Refs #159."
```

---

## Task 1: Extract `cursor` (shared leaf helpers)

**Files:**
- Create: `crates/game-core/src/engine/dispatch/cursor.rs`
- Modify: `crates/game-core/src/engine/dispatch/mod.rs`

Move these functions (all currently private; keep `pub(super)` so siblings can call them):

| Symbol | ~line |
|---|---|
| `stat_to_skill_kind` | 680 |
| `active_investigators_in_turn_order` | 4751 |
| `first_active_investigator` | 4775 |
| `next_active_investigator_after` | 4803 |
| `active_investigators_at` | 3442 |

No `*_tests` block is dedicated to these (they're exercised transitively). No const, no type.

- [ ] **Step 1:** Follow the per-task move procedure. These are leaves — they call only into `crate::state` / `crate::card_data`, so the `use` block is small and there are no sibling-call back-patches *within* the moved code. Mark each moved fn `pub(super)`.
- [ ] **Step 2:** Back-patch: every caller of these five (many, across `mod.rs`) becomes `super::cursor::<fn>` once extracted. The compiler lists them.
- [ ] **Step 3:** Run THE GAUNTLET → green.
- [ ] **Step 4: Commit**

```sh
git add -A
git commit -m "engine: extract dispatch::cursor (#159)"
```

---

## Task 2: Extract `skill_test`

**Files:**
- Create: `crates/game-core/src/engine/dispatch/skill_test.rs`
- Modify: `dispatch/mod.rs`

Move (note `start_skill_test` is `pub(super)` and stays so — it's referenced only by doc-comments outside dispatch, no code path):

| Symbol | ~line | Visibility |
|---|---|---|
| `start_skill_test` | 1328 | `pub(super)` |
| `finish_skill_test` | 1435 | priv |
| `drive_skill_test` | 1526 | priv |
| `validate_commit_indices` | 1581 | priv |
| `sum_skill_value` | 1629 | priv |
| `sum_committed_icons` | 1657 | priv |
| `resolve_chaos_token_and_emit` | 1689 | priv |
| `discard_committed_cards` | 1733 | priv |
| `apply_skill_test_follow_up` | 1764 | priv |
| `fire_on_skill_test_resolution` | 1831 | priv |
| `perform_skill_test` | 2448 | priv |
| `peril_check` | 6414 | priv |

Test blocks: none of the 22 `*_tests` are skill-test-specific (skill-test coverage lives in `crates/game-core/tests/` integration files + `on_skill_test_resolution.rs`); move none here.

- [ ] **Step 1:** Move per procedure. `drive_skill_test`/`finish_skill_test` call into reaction-window helpers (still in `mod.rs` at this point) and `cursor` — use `super::<fn>` for the still-in-`mod.rs` ones and `super::cursor::<fn>` for cursor. When `reaction_windows` is extracted (Task 3) those `super::` paths get back-patched to `super::reaction_windows::`.
- [ ] **Step 2:** Back-patch callers in `mod.rs`/`resolve_input` (`perform_skill_test`, `drive_skill_test`, `fire_on_skill_test_resolution`, etc.) to `super::skill_test::<fn>`.
- [ ] **Step 3:** THE GAUNTLET → green.
- [ ] **Step 4: Commit** `engine: extract dispatch::skill_test (#159)`

---

## Task 3: Extract `reaction_windows`

**Files:**
- Create: `crates/game-core/src/engine/dispatch/reaction_windows.rs`
- Modify: `dispatch/mod.rs`

Move:

| Symbol | ~line | Visibility |
|---|---|---|
| `queue_reaction_window` | 1998 | priv |
| `scan_pending_triggers` | 2021 | priv |
| `trigger_matches` | 2091 | priv |
| `open_queued_reaction_window` | 2148 | priv |
| `resume_reaction_window` | 2182 | priv |
| `fire_pending_trigger` | 2213 | priv |
| `bump_usage_counter` | 2349 | priv |
| `close_reaction_window_at` | 2394 | priv |
| `run_window_continuation` | 4881 | priv |
| `check_play_card` | 4213 | `pub(super)` |
| `check_activate_ability` | 4454 | `pub(super)` |
| `any_fast_play_eligible` | 4577 | `pub(super)` |
| `open_fast_window` | 5041 | `pub(super)` |

Test blocks to move here: `check_play_card_tests` (5925), `check_activate_ability_tests` (5956), `any_fast_play_eligible_tests` (5987), `open_fast_window_tests` (6007).

- [ ] **Step 1:** Move per procedure. `run_window_continuation` dispatches into many phase/skill continuations — those targets are still in `mod.rs` (or `skill_test`) now; reference via `super::<fn>` / `super::skill_test::<fn>`, back-patched later as their owners extract. `check_play_card`/`check_activate_ability` reference card-registry + `crate::card_data` — bring those `use`s.
- [ ] **Step 2:** Back-patch all callers (`drive_skill_test` in `skill_test`, the entry dispatchers, etc.) to `super::reaction_windows::<fn>`. Update `skill_test`'s earlier `super::` references to the window helpers → `super::reaction_windows::`.
- [ ] **Step 3:** THE GAUNTLET → green.
- [ ] **Step 4: Commit** `engine: extract dispatch::reaction_windows (#159)`

---

## Task 4: Extract `actions`

**Files:**
- Create: `crates/game-core/src/engine/dispatch/actions.rs`
- Modify: `dispatch/mod.rs`

Move:

| Symbol | ~line |
|---|---|
| `investigate` | 2481 |
| `move_action` | 2587 |
| `validate_engaged_action` | 2732 |
| `spend_one_action` | 2796 |
| `fight` | 2819 |
| `evade` | 2856 |

Test blocks: none dedicated (action coverage is in `crates/game-core/tests/` + inline elsewhere). Move none.

- [ ] **Step 1:** Move per procedure. `investigate`/`fight`/`evade` call `super::skill_test::perform_skill_test`/`start_skill_test` and `super::cursor`; `move_action` calls hunter/engage helpers still in `mod.rs` → `super::<fn>` (back-patched when `hunters` extracts).
- [ ] **Step 2:** Back-patch entry-dispatcher arms (`Investigate`/`Move`/`Fight`/`Evade`) → `super::actions::<fn>`.
- [ ] **Step 3:** THE GAUNTLET → green.
- [ ] **Step 4: Commit** `engine: extract dispatch::actions (#159)`

---

## Task 5: Extract `combat`

**Files:**
- Create: `crates/game-core/src/engine/dispatch/combat.rs`
- Modify: `dispatch/mod.rs`

Move:

| Symbol | ~line |
|---|---|
| `damage_enemy` | 2894 |
| `apply_damage_numeric` | 2956 |
| `apply_horror_numeric` | 2990 |
| `enemy_attack` | 3288 |
| `fire_attacks_of_opportunity` | 3325 |
| `resolve_attacks_for_investigator` | 3391 |

Test blocks: none dedicated (`enemy_phase_tests` covers the driver, moves to `phases` in Task 12). Move none here.

- [ ] **Step 1:** Move per procedure. `enemy_attack`/`resolve_attacks_for_investigator` call `super::elimination::*` (Task 6) and `super::cursor::*`; reference still-in-`mod.rs` elimination helpers via `super::` for now.
- [ ] **Step 2:** Back-patch callers (`actions::fight`, the enemy-phase driver in `mod.rs`, `fire_attacks_of_opportunity` callers) → `super::combat::<fn>`.
- [ ] **Step 3:** THE GAUNTLET → green.
- [ ] **Step 4: Commit** `engine: extract dispatch::combat (#159)`

---

## Task 6: Extract `elimination`

**Files:**
- Create: `crates/game-core/src/engine/dispatch/elimination.rs`
- Modify: `dispatch/mod.rs`

Move:

| Symbol | ~line |
|---|---|
| `apply_investigator_defeat` | 3027 |
| `run_elimination_steps` | 3067 |
| `take_horror` | 3196 |
| `check_all_defeated` | 3230 |

Test block to move here: `elimination_tests` (8859).

- [ ] **Step 1:** Move per procedure. `run_elimination_steps` calls `reengage_at_location` (still in `mod.rs`, → `super::reengage_at_location` now; back-patched to `super::hunters::` in Task 7) and `super::act_agenda::request_resolution` (Task 9 — reference via `super::request_resolution` for now). `take_horror`/`apply_investigator_defeat` are called from `combat` and `cards` → back-patch those.
- [ ] **Step 2:** Back-patch `combat`, `cards`-to-be, and `mod.rs` callers → `super::elimination::<fn>`. Update `combat`'s earlier `super::` elimination refs → `super::elimination::`.
- [ ] **Step 3:** THE GAUNTLET → green.
- [ ] **Step 4: Commit** `engine: extract dispatch::elimination (#159)`

---

## Task 7: Extract `hunters`

**Files:**
- Create: `crates/game-core/src/engine/dispatch/hunters.rs`
- Modify: `dispatch/mod.rs`

Move (including the private `enum PreyResolution` at ~615 and `resolve_prey`; `drive_hunter_moves` stays `pub(crate)`):

| Symbol | ~line | Visibility |
|---|---|---|
| `enum PreyResolution` | 615 | priv |
| `resolve_prey` | 631 | `pub(super)` |
| `is_eligible_hunter` | 3433 | priv |
| `hunter_destinations` | 3459 | priv |
| `move_hunter_to` | 3513 | priv |
| `engage_enemy_with` | 3531 | priv |
| `engage_on_arrival` | 3551 | priv |
| `reengage_at_location` | 3597 | `pub(super)` |
| `process_one_hunter` | 3616 | priv |
| `next_eligible_hunter` | 3647 | priv |
| `drive_hunter_moves` | 3660 | `pub(crate)` |
| `suspend_hunter_choice` | 3673 | priv |
| `resume_hunter_choice` | 3696 | priv |
| `resume_spawn_engage` | 3782 | priv |

Test blocks to move here: `resolve_prey_tests` (8242), `hunter_movement_tests` (8318), `hunter_resume_tests` (8515), `reengage_tests` (9155). (`reengage_at_location` lives here, so `reengage_tests` comes too.)

- [ ] **Step 1:** Move per procedure. Uses `super::pathfinding` (the BFS helpers in `engine/pathfinding.rs` — reference as `crate::engine::pathfinding::*`, unchanged), `super::cursor::*`, `super::engage`/`combat`. `resolve_prey` is called by `encounter::spawn_enemy` (Task 8) and by `elimination::run_elimination_steps` (`reengage_at_location` path) — make sure `pub(super)` reaches them (siblings: yes).
- [ ] **Step 2:** Back-patch: `elimination`'s `super::reengage_at_location` → `super::hunters::reengage_at_location`; `actions::move_action`'s hunter refs → `super::hunters::`; `mod.rs`/`resolve_input` hunter-resume refs → `super::hunters::`. `drive_hunter_moves` keeps `pub(crate)`; no external full-path caller exists, so nothing outside dispatch changes.
- [ ] **Step 3:** THE GAUNTLET → green.
- [ ] **Step 4: Commit** `engine: extract dispatch::hunters (#159)`

---

## Task 8: Extract `encounter`

**Files:**
- Create: `crates/game-core/src/engine/dispatch/encounter.rs`
- Modify: `dispatch/mod.rs`

Move (including const `MAX_SURGE_CHAIN` at ~48; encounter-deck helpers are `pub(super)`):

| Symbol | ~line | Visibility |
|---|---|---|
| `const MAX_SURGE_CHAIN` | 48 | priv |
| `encounter_deck_shuffled` | 266 | priv |
| `encounter_card_revealed` | 304 | priv |
| `resolve_encounter_card` | 345 | priv |
| `spawn_enemy` | 487 | priv |
| `shuffle_encounter_deck` | 743 | `pub(super)` |
| `reshuffle_encounter_discard` | 774 | `pub(super)` |
| `draw_encounter_top` | 789 | `pub(super)` |
| `mythos_draw_for` | 6478 | priv |
| `run_mythos_draw_chain` | 6514 | priv |
| `advance_mythos_draw_pending` | 6600 | priv |
| `draw_encounter_card` | 6430 | `pub(super)` |

Test blocks to move here: `encounter_card_revealed_tests` (5291), `encounter_deck_helper_tests` (5370), `spawn_enemy_tests` (5575), `mythos_draw_for_tests` (6613), `draw_encounter_card_tests` (6670).

- [ ] **Step 1:** Move per procedure. `spawn_enemy` calls `super::hunters::resolve_prey` and engage helpers in `hunters`; `run_mythos_draw_chain` calls `super::reaction_windows::open_fast_window` and `super::hunters::resume_spawn_engage` suspension. Bring `crate::card_registry`, `crate::card_data::{Spawn, SpawnLocation, ...}` `use`s.
- [ ] **Step 2:** Back-patch entry-dispatcher `DrawEncounterCard` arm and `mythos_phase` (still in `mod.rs`) refs → `super::encounter::<fn>`.
- [ ] **Step 3:** THE GAUNTLET → green.
- [ ] **Step 4: Commit** `engine: extract dispatch::encounter (#159)`

---

## Task 9: Extract `act_agenda`

**Files:**
- Create: `crates/game-core/src/engine/dispatch/act_agenda.rs`
- Modify: `dispatch/mod.rs`

Move:

| Symbol | ~line | Visibility |
|---|---|---|
| `place_doom_on_agenda` | 1063 | priv |
| `check_doom_threshold` | 1082 | priv |
| `advance_agenda` | 1106 | priv |
| `clue_contributors` | 1125 | priv |
| `advance_act_action` | 1139 | priv |
| `spend_clues` | 1190 | priv |
| `advance_act` | 1212 | priv |
| `request_resolution` | 1235 | `pub(super)` |

Test blocks to move here: `doom_agenda_tests` (9298), `advance_act_tests` (9409).

- [ ] **Step 1:** Move per procedure. `place_doom_on_agenda`/`check_doom_threshold` are called by `mythos_phase` (still in `mod.rs`); `request_resolution` is called by `elimination` and the act/agenda paths. `pub(super)` on `request_resolution` reaches `elimination` (sibling). Back-patch `elimination`'s `super::request_resolution` → `super::act_agenda::request_resolution`.
- [ ] **Step 2:** Back-patch entry-dispatcher `AdvanceAct` arm + `mythos_phase` refs → `super::act_agenda::<fn>`.
- [ ] **Step 3:** THE GAUNTLET → green.
- [ ] **Step 4: Commit** `engine: extract dispatch::act_agenda (#159)`

---

## Task 10: Extract `cards`

**Files:**
- Create: `crates/game-core/src/engine/dispatch/cards.rs`
- Modify: `dispatch/mod.rs`, `crates/game-core/src/engine/evaluator.rs:319`

Move (including const `INITIAL_HAND_SIZE` at ~40; player-deck helpers; `grant_resources` becomes `pub(crate)`):

| Symbol | ~line | Visibility |
|---|---|---|
| `const INITIAL_HAND_SIZE` | 40 | priv |
| `deck_shuffled` | 246 | priv |
| `shuffle_player_deck` | 699 | `pub(super)` |
| `draw_cards` | 810 | `pub(super)` |
| `grant_resources` | 845 | **`pub(crate)`** (was `pub(super)`) |
| `resolve_play_target` | 4143 | priv |
| `play_card` | 4364 | priv |
| `draw_one_with_deckout` | 3941 | priv |
| `reshuffle_discard_into_deck` | 3913 | priv |
| `mulligan` | 4053 | priv |
| `draw` | 3990 | priv |

Test blocks to move here: `grant_resources_tests` (7050), `draw_one_with_deckout_tests` (7089).

- [ ] **Step 1:** Move per procedure. `play_card` calls `super::reaction_windows::*` (eligibility) and `super::evaluator`/`apply_effect` (`crate::engine::evaluator::*`, unchanged path). `grant_resources` is called from **outside dispatch** by `evaluator.rs:319` as `crate::engine::dispatch::grant_resources(...)`.
- [ ] **Step 2: Update the one external call site.** In `crates/game-core/src/engine/evaluator.rs:319` change `crate::engine::dispatch::grant_resources(...)` → `crate::engine::dispatch::cards::grant_resources(...)`. Mark `grant_resources` `pub(crate)` so it reaches `engine::evaluator` (a sibling of `engine::dispatch`, which `pub(super)` would NOT reach after the move).
- [ ] **Step 3:** Back-patch entry-dispatcher arms (`PlayCard`/`Mulligan`/`Draw`) + upkeep/draw refs in `mod.rs` → `super::cards::<fn>`.
- [ ] **Step 4:** THE GAUNTLET → green. (The `doc` job confirms no broken intra-doc links; the prose mentions of these helpers in `event.rs`/`game_state.rs`/`action.rs` are plain code spans, not links, so they don't break — leave them.)
- [ ] **Step 5: Commit** `engine: extract dispatch::cards (#159)`

---

## Task 11: Extract `abilities`

**Files:**
- Create: `crates/game-core/src/engine/dispatch/abilities.rs`
- Modify: `dispatch/mod.rs`

Move:

| Symbol | ~line |
|---|---|
| `activate_ability` | 5117 |
| `pay_activation_costs` | 5162 |
| `resolve_activated_ability` | 5214 |
| `check_cost_payable` | 5257 |

Test blocks: none inline here (`activate_ability` integration coverage is `crates/game-core/tests/activate_ability.rs`). Move none.

- [ ] **Step 1:** Move per procedure. Calls `super::reaction_windows::check_activate_ability` and `super::evaluator`. Back-patch entry-dispatcher `ActivateAbility` arm → `super::abilities::activate_ability`.
- [ ] **Step 2:** THE GAUNTLET → green.
- [ ] **Step 3: Commit** `engine: extract dispatch::abilities (#159)`

---

## Task 12: Extract `phases` (final cluster)

**Files:**
- Create: `crates/game-core/src/engine/dispatch/phases.rs`
- Modify: `dispatch/mod.rs`

Move (including const `ACTIONS_PER_TURN` at ~36; everything phase/turn-flow):

| Symbol | ~line |
|---|---|
| `const ACTIONS_PER_TURN` | 36 |
| `start_scenario` | 865 |
| `end_turn` | 912 |
| `investigation_phase` | 975 |
| `begin_investigator_turn` | 999 |
| `investigation_phase_end` | 1009 |
| `mythos_phase` | 1020 |
| `step_phase` | 1266 |
| `rotate_to_active` | 1307 |
| `enemy_attack_kickoff` | 3844 |
| `enemy_phase` | 3868 |
| `enemy_phase_end` | 3894 |
| `mythos_phase_end` | 4620 |
| `upkeep_phase` | 4648 |
| `upkeep_resume` | 4661 |
| `upkeep_phase_end` | 4672 |
| `ready_exhausted_cards` | 4699 |
| `check_hand_size` | 4739 |
| `reset_actions` | 4831 |
| `upkeep_draw_and_resource` | 4852 |

Test blocks to move here: `investigation_phase_tests` (6070), `mythos_phase_tests` (6723), `upkeep_phase_tests` (7122), `enemy_phase_tests` (7543).

- [ ] **Step 1:** Move per procedure. By now every callee (`encounter`, `act_agenda`, `combat`, `elimination`, `hunters`, `cards`, `reaction_windows`, `cursor`) is in its final module, so write `super::<module>::<fn>` paths directly. `start_scenario`/`mythos_phase` call `super::act_agenda::*` + `super::encounter::*`; `enemy_phase` calls `super::combat::*` + `super::hunters::drive_hunter_moves`; upkeep calls `super::cards::*` + `super::cursor::*`.
- [ ] **Step 2:** Back-patch the entry dispatchers + `resolve_input` (in `mod.rs`) → `super::phases::<fn>` (e.g. `StartScenario`/`EndTurn` arms, the post-mulligan `investigation_phase` kickoff).
- [ ] **Step 3: Confirm `mod.rs` is now just routers.** After this task `dispatch/mod.rs` should contain only: the `use` block, the `mod <name>;` declarations, `apply_player_action`, `apply_engine_record`, and `resolve_input`.
- [ ] **Step 4:** THE GAUNTLET → green.
- [ ] **Step 5: Commit** `engine: extract dispatch::phases (#159)`

---

## Task 13: Verification sweep + phase-doc note

**Files:**
- Modify: `docs/phases/` (none — Phase 4 is closed and Phase 5 not yet open; this refactor is unmilestoned cleanup, so no phase-doc table changes. Skip if no phase doc applies.)

- [ ] **Step 1: Confirm no file exceeds ~1k lines** (spec success criterion 4)

```sh
wc -l crates/game-core/src/engine/dispatch/*.rs | sort -rn | head
```

Expected: every file under ~1,000 lines. If `phases.rs` is over, note it for a possible follow-up nest (`phases/<sub>.rs`) — do NOT nest in this PR unless it clearly exceeds (YAGNI per spec).

- [ ] **Step 2: Confirm move-only diff** (spec success criterion 1)

```sh
git diff main --stat -- crates/game-core/src
```

Expected: only files under `crates/game-core/src/engine/dispatch/` (+ the one `evaluator.rs` line). Spot-check a sample of moved functions against `git show main:crates/game-core/src/engine/dispatch.rs` to confirm bodies are byte-identical modulo `use`/visibility/path-qualifier edits.

- [ ] **Step 3: Confirm public API unchanged** (spec success criterion 3)

```sh
grep -rn 'dispatch::' crates/game-core/src/engine/mod.rs
```

Expected: only `dispatch::apply_player_action` / `dispatch::apply_engine_record` — unchanged from before.

- [ ] **Step 4: Final full GAUNTLET** → all five green.

- [ ] **Step 5:** No commit needed if Steps 1–4 pass clean (all work already committed per task). If a fix was required, commit it: `engine: dispatch-split verification fixes (#159)`.

---

## Spec coverage check

- Shape (`dispatch.rs`→`dispatch/mod.rs` + siblings): Task 0 + Tasks 1–12 ✅
- Module layout (13 modules): one task per module ✅
- `mod.rs` keeps entry dispatchers + `resolve_input`: Task 12 Step 3 ✅
- No new crate-public API: Task 13 Step 3 ✅
- Test blocks move with their code: each task names its `*_tests` blocks; all 22 mapped (encounter ×5, reaction_windows ×4, phases ×4, hunters ×4, elimination ×1, cards ×2, act_agenda ×2) ✅
- Consts relocated: `ACTIONS_PER_TURN`/`INITIAL_HAND_SIZE`→ phases/cards (Tasks 12/10), `MAX_SURGE_CHAIN`→ encounter (Task 8) ✅
- `PreyResolution` relocated: Task 7 ✅
- One external dep (`evaluator.rs:319`): Task 10 Step 2 ✅
- One-commit-per-module, gauntlet green at each: every task ✅
- Success criteria 1–4: Task 13 ✅
- Out of scope (#160 threading, #161 two-phase): not touched ✅
