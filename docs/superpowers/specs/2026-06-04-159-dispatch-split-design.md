# Split `engine/dispatch.rs` into per-domain submodules (#159)

**Date:** 2026-06-04
**Issue:** #159
**Type:** Pure refactor — behavior-preserving relocation. No logic changes.

## Problem

`crates/game-core/src/engine/dispatch.rs` is **9,569 lines** — the largest
file in the workspace. It holds **98 production functions (~5,290 lines)** in
one flat module plus **22 `#[cfg(test)] mod *_tests` blocks (~4,280 lines)**.
It accreted one phase-content PR at a time across Phase 4. The handler
contract and cursor-driven phase drivers are sound; the file is simply too big
to navigate, and Phase 5's server boundary work should land against a
navigable module tree.

## Why this is a clean move

Verified before designing:

- **No module-level state, macros, or statics.** Only three `const`s
  (`ACTIONS_PER_TURN`, `INITIAL_HAND_SIZE`, `MAX_SURGE_CHAIN`) and **one**
  private type (`enum PreyResolution`).
- **Rust sibling submodules call each other freely** via `pub(super)` /
  `pub(crate)` — there is no acyclicity constraint to untangle. The split is
  organizational, not a dependency rework.
- **The existing test suite is the correctness oracle.** No behavior changes,
  so green CI at every step proves the move.

## Design

### Shape

`dispatch.rs` → `dispatch/mod.rs` + sibling module files under
`crates/game-core/src/engine/dispatch/`.

- `mod.rs` keeps the two public entry dispatchers (`apply_player_action`,
  `apply_engine_record`) and the `resolve_input` resume-router — all top-level
  routers — declares the submodules, and re-exports nothing new.
- Cross-module calls use `pub(super)` (or `pub(crate)` where `engine/mod.rs`
  already reaches in, e.g. `drive_hunter_moves`).
- **No function becomes part of the crate's public API that wasn't already.**
  The only externally-visible surface stays `dispatch::apply_player_action` /
  `dispatch::apply_engine_record`, called from `engine/mod.rs`.

### Module layout (flat — no nested subdirs)

| Module | Owns |
|---|---|
| `mod.rs` | `apply_player_action`, `apply_engine_record`, the guard ladder, `resolve_input` |
| `cursor` | shared leaf helpers: `active_investigators_at`, `active_investigators_in_turn_order`, `first_active_investigator`, `next_active_investigator_after`, `stat_to_skill_kind` |
| `phases` | `start_scenario`, `end_turn`, `step_phase`, every `*_phase` / `*_phase_end` / `*_resume`, `rotate_to_active`, `reset_actions`, `ready_exhausted_cards`, `check_hand_size`, `upkeep_draw_and_resource`, `enemy_attack_kickoff`, `begin_investigator_turn` |
| `skill_test` | `perform_skill_test`, `finish_skill_test`, `drive_skill_test`, `validate_commit_indices`, `sum_skill_value`, `sum_committed_icons`, `resolve_chaos_token_and_emit`, `discard_committed_cards`, `apply_skill_test_follow_up`, `fire_on_skill_test_resolution`, `peril_check` |
| `reaction_windows` | `queue_reaction_window`, `scan_pending_triggers`, `trigger_matches`, `open_queued_reaction_window`, `resume_reaction_window`, `fire_pending_trigger`, `bump_usage_counter`, `close_reaction_window_at`, `run_window_continuation`, + Fast-eligibility helpers (`open_fast_window`, `any_fast_play_eligible`, `check_play_card`, `check_activate_ability`) |
| `actions` | `investigate`, `move_action`, `fight`, `evade`, `validate_engaged_action`, `spend_one_action` |
| `combat` | `damage_enemy`, `apply_damage_numeric`, `apply_horror_numeric`, `enemy_attack`, `fire_attacks_of_opportunity`, `resolve_attacks_for_investigator` |
| `elimination` | `apply_investigator_defeat`, `run_elimination_steps`, `take_horror`, `check_all_defeated` |
| `hunters` | hunter movement (`is_eligible_hunter`, `hunter_destinations`, `move_hunter_to`, `engage_enemy_with`, `engage_on_arrival`, `reengage_at_location`, `process_one_hunter`, `next_eligible_hunter`, `drive_hunter_moves`, `suspend_hunter_choice`, `resume_hunter_choice`), `resume_spawn_engage`, and `resolve_prey` + `PreyResolution` (shared with `encounter` via `super::hunters::resolve_prey`) |
| `encounter` | `encounter_card_revealed`, `resolve_encounter_card`, `spawn_enemy`, `mythos_draw_for`, `run_mythos_draw_chain`, `advance_mythos_draw_pending`, `draw_encounter_card`, `deck_shuffled`, `encounter_deck_shuffled` |
| `act_agenda` | `place_doom_on_agenda`, `check_doom_threshold`, `advance_agenda`, `clue_contributors`, `advance_act_action`, `spend_clues`, `advance_act`, `request_resolution` |
| `cards` | `play_card`, `resolve_play_target`, `mulligan`, `draw`, `draw_one_with_deckout`, `reshuffle_discard_into_deck`, `grant_resources` |
| `abilities` | `activate_ability`, `pay_activation_costs`, `resolve_activated_ability`, `check_cost_payable` |

The function→module assignment above is the working grouping; the
implementation plan finalizes any edge cases discovered while moving (a few
helpers — e.g. `grant_resources` — are used across clusters and land where
their primary owner sits, called cross-module otherwise).

Notes on judgment calls:

- **`actions` and `combat` stay separate.** `investigate` / `move` aren't
  combat; `fight` / `evade` are short and lead into skill tests, so they sit
  with the other basic board actions.
- **`resolve_prey` lives in `hunters`** (prey resolution is hunter/prey
  logic) and `encounter::spawn_enemy` calls it via `super::hunters`.
- **Consts move to their natural home:** `ACTIONS_PER_TURN` and
  `INITIAL_HAND_SIZE` → `phases`, `MAX_SURGE_CHAIN` → `encounter`.

### Tests

Each `#[cfg(test)] mod *_tests` block moves to the bottom of whichever new
module owns the code it covers (CLAUDE.md convention: unit tests live
per-module in `#[cfg(test)]`). This keeps every module's move self-contained.
The 22 existing test-module names already map cleanly onto the clusters.

### Granularity

Flat sibling modules — no nested subdirectories. Average ~400 lines/module;
the largest (`phases`) lands under ~1k. Nesting (e.g. `phases/mythos.rs`) is
trivial to add later if one module proves unwieldy; YAGNI for now.

### Execution

- Single branch `engine/dispatch-split`, **one commit per module
  extraction**, one squash-merge PR closing #159.
- Move leaf / shared modules first (`cursor`) so later extractions reference a
  stable target.
- Each commit must compile and pass the **full CI gauntlet** before the next:
  `fmt` / `clippy --all-targets --all-features -D warnings` / `test` (with
  `RUSTFLAGS="-D warnings"`) / `doc` (`RUSTDOCFLAGS="-D warnings"`) /
  `wasm-build`.

## Success criteria (verifiable)

1. `git diff` against the pre-split file shows only relocation — function
   bodies unchanged except for mechanical adjustments forced by the move:
   `use` lines, visibility keywords, and cross-module path qualifiers at call
   sites (e.g. `resolve_prey(...)` → `super::hunters::resolve_prey(...)`). No
   control-flow or logic edits.
2. Full CI gauntlet green at every commit, not just the final one.
3. No new crate-public API: the only externally-visible dispatch surface stays
   `apply_player_action` / `apply_engine_record`, and `engine/mod.rs`'s call
   sites are unchanged.
4. No resulting file exceeds ~1k lines.

## Out of scope

- The `(&mut GameState, &mut Vec<Event>)` threading consolidation (**#160**).
- The `apply()` two-phase validate/apply refactor (**#161**, supersedes
  `TODO(#17+)` at `engine/mod.rs:56`).

This PR moves code only. Both follow-ups are sequenced after the split per
their issue notes (cleaner diff, shared context design).
