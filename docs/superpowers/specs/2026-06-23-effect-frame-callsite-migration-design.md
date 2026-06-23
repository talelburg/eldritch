# Effect-frame call-site migration (Slice D, #423) — design

**Status:** approved design, 2026-06-23. Successor to #422 (effect-evaluator-as-frames); Slice D of the EmitEvent-frame arc (umbrella #435), gated on Slice C (#431, merged in #445).

## Why this exists

`apply_effect` (`crates/game-core/src/engine/evaluator.rs:318`) is a **thin synchronous bounded entry** into the single global `drive`: it pushes the effect's root `Continuation::Effect(frame)` and calls `drive_effect_to_base` to drive it to completion/suspension, returning an `EngineOutcome` the caller then acts on. Every effect-invocation site rides this wrapper today, which is why the suite is behaviour-preserving — but it is the last place the engine is *not* "every step is a frame; top-frame dispatch only" (#393's end-state).

This slice migrates every production call site off the synchronous wrapper: push the effect root frame, let the global `drive` own it, and move each site's *post-effect logic* into an enclosing frame the drive loop dispatches when the effect frame pops. Then **delete** `apply_effect` and `drive_effect_to_base` outright (not demote to test-only) and rework the tests that used them to exercise the real frame path.

## What stays vs. goes

- **Stays (production internals the drive loop already uses):** `frame_of` (builds an `EffectFrame` from an `Effect`), `step_effect_frame` (steps the top `Continuation::Effect` once), and the drive loop's existing `Continuation::Effect(_)` arm (`dispatch/mod.rs:200`).
- **Goes (deleted):** `apply_effect`, `drive_effect_to_base`. No test-only survivor.
- **New production helper:** `push_effect(cx, effect, ctx)` — pushes `Continuation::Effect(frame_of(effect, ctx))` and returns `()`. Every push-and-return site calls it then returns `EngineOutcome::Done`. Tests call it then the **real** `drive(cx, EngineOutcome::Done)`.

## Two structural changes

### New: `Continuation::PlayFromHand { investigator, card, stage }`

The unified "play a card from hand" resolution frame. A card played from hand runs its effect, then is **disposed by type**: an event goes to discard (`CardDiscarded { from: Zone::Hand }`); an asset enters `cards_in_play` (`EnteredPlay`, plus any reaction window the entrance opens). Fast vs. non-Fast is orthogonal (a play-cost concern, not a disposition concern), so the *same* frame serves all three entry points:

- the normal `PlayCard` action's no-AoO fast path (`play_card`, `cards.rs`),
- the normal `PlayCard` action's post-AoO resume (`resume_play_card`, reached after `ActionResolution` pops),
- the Fast-event play inside a reaction window (`play_fast_event`, `reaction_windows.rs`).

`stage` is a resume marker for the disposal's own multi-step shape: an asset's entrance emits `EnteredPlay` and may open a reaction window that itself suspends, so `PlayFromHand` must resume *after* that window closes. The drive loop gets a `PlayFromHand` arm that runs the stage-keyed disposal when the effect child pops.

**Relationship to `ActionResolution` (a different layer, not a competitor).** `ActionResolution { investigator, resume: ActionResume }` is the generic action-AoO frame: it wraps any AoO-provoking action and, on the AoO loop's pop, `resume_action_resolution` (`mod.rs:293`) **pops it**, runs §D actor-still-active re-validation, then dispatches to the primary (`ActionResume::PlayCard` → `resume_play_card`; `ActivateAbility` → `resume_activate_ability`). It is gone before the card effect runs, and it never covers Fast plays. So `PlayFromHand` stacks *below* it: `resume_play_card` (post-`ActionResolution`) pushes `PlayFromHand`; the no-AoO and Fast paths push it directly. `ActionResolution` is unchanged by this slice.

### Extended: `EncounterCard` gains `disposition: EncounterDisposition`

`EncounterCard { card }` becomes `EncounterCard { card, disposition }` where `EncounterDisposition` is `Discard | Spawn`. Today only the treachery arm pushes `EncounterCard` (drive-loop arm disposes via `teardown_encounter_card_if_top`); the enemy arm pushes nothing and calls `spawn_enemy` inline. After this slice both arms push `EncounterCard` with the right disposition, and the drive-loop `EncounterCard` arm runs `teardown` (Discard) or `spawn_enemy` (Spawn) on resume. This unifies treachery + enemy revelation under one frame + one arm.

## Per-site migration

The mapping below is the authoritative per-site plan. "Push-and-return" means: replace the synchronous `apply_effect(cx, &e, ctx)` (whose result the site returned or asserted `Done`) with `push_effect(cx, &e, ctx); EngineOutcome::Done` (or, where the effect is the last of several, push and let the existing enclosing frame's resume continue). Each site is behaviour-preserving because the global `drive` runs immediately after the dispatch returns and drives the pushed effect via the `Effect` arm.

| # | Site (file:line, fn) | Post-effect logic today | Migration |
|---|---|---|---|
| 1a | `abilities.rs:124` `activate_ability` (Fast/exempt) | none (tail-return) | `push_effect` + `Done`. No enclosing frame needed. |
| 1b | `abilities.rs:170` `resume_activate_ability` | none (tail-return) | `push_effect` + `Done`. (`ActionResolution` already popped; nothing to own.) |
| 6 | `forced_triggers.rs:443` `resolve_one` | none (tail-return) | `push_effect` + `Done`. The forced-run `TimingPointWindow` frame beneath resumes the remaining siblings via `advance_resolution`. |
| 3a | `encounter.rs:179` treachery Revelation | `teardown_encounter_card_if_top` on synchronous completion | Push `EncounterCard { disposition: Discard }` (already pushed today); `push_effect` the Revelation effects; drop the inline `teardown` — the `EncounterCard` arm disposes on resume (it already does on the suspend path). |
| 3b | `encounter.rs:218` enemy Revelation | `spawn_enemy` | Push `EncounterCard { disposition: Spawn }` (new for this arm); `push_effect` the Revelation effects; the `EncounterCard` arm runs `spawn_enemy` on resume. (Dormant in scope — no in-scope enemy Revelation — but structurally unified.) |
| 4a | `skill_test.rs:339` `on_success` | none (shared `Done` tail); `debug_assert! Done` in scope | `push_effect` as a child of the live `SkillTest` frame (already pre-advanced to `PostFollowUp` at `:317`); the `SkillTest` drive arm resumes at teardown when it pops. Preserve follow-up→on_success ordering (on_success only runs if the follow-up completed synchronously). |
| 4b | `skill_test.rs:353` `on_fail` | none (shared tail); real `AwaitingInput` early-return (Crypt Chill 01167) | `push_effect` as a child of `SkillTest`; the suspend case is now automatic (the effect frame parks, the loop resumes `SkillTest` at teardown). Preserve the "on_fail does not re-run on resume" semantics via the pre-advanced cursor. |
| 5a | `reaction_windows.rs:758` `fire_pending_trigger` (in-play ability) | `bump_usage_counter` on `Done` | Move `bump_usage_counter` to run **before** the push (usage is consumed on fire; the post-`Done` conditionality is purely defensive against an `unreachable!` `Rejected`). Then `push_effect` + `Done`; the window frame beneath resumes its candidate scan. |
| 2 | `cards.rs:632` `complete_play` OnPlay | multi-ability OnPlay loop + asset enter-play tail (`EnteredPlay`, conditional reaction window) | Push `PlayFromHand`; it `push_effect`s the OnPlay effects (`Seq`-combined if several) and disposes on resume. Replaces the synchronous `complete_play` tail. |
| 5b | `reaction_windows.rs:842` `play_fast_event` | `flush_pending_played_event` on `Done` | Push `PlayFromHand` (above the window); it `push_effect`s the event effect and discards the event on resume; pops, exposing the window, which resumes its candidate scan. Replaces the inline flush. |

After all sites migrate, delete `apply_effect` + `drive_effect_to_base`.

## Test rework

`apply_effect`'s deletion strands its callers in test modules — chiefly the ~30 calls in `evaluator.rs`'s `#[cfg(test)] mod tests` (≈ line 2154+), plus the test-only call in `choice.rs:150`. Rework each to the real path: `push_effect(cx, &effect, ctx)` then `drive(cx, EngineOutcome::Done)`. This runs the production drive loop's `Effect` arm + `step_effect_frame` — real code, not a synchronous crutch. A test that asserted `apply_effect(...) == Done` becomes "push, drive, assert `Done` + the same end-state"; a test that asserted `AwaitingInput` (a controller pick) becomes "push, drive, assert the same `AwaitingInput`" (the `Effect` `Leaf` suspends in place exactly as before). Where many tests share the shape, a small `#[cfg(test)]` helper `fn run(cx, effect, ctx) -> EngineOutcome { push_effect(cx, &effect, ctx); drive(cx, EngineOutcome::Done) }` is acceptable — it is a thin alias over the **real** drive, carrying no resolution logic of its own.

## Behaviour-preservation strategy

The whole slice is behaviour-preserving at the `apply` boundary: a dispatch handler returns to `apply_player_action`, which runs `drive(cx, outcome)`. Pushing the effect root and returning `Done` hands the same work to the same loop. The net is the existing suite:

- **Card tests** (`crates/cards/src/impls/<name>.rs`) and **integration tests** (`crates/cards/tests/*`) go through real `apply`/`drive` and must stay green **untouched**. Load-bearing cases per site: Dynamite Blast 01024 (suspending OnPlay/Fast event — sites 2, 5b), Crypt Chill 01167 (suspending on_fail — 4b), Frozen in Fear 01164 (forced/reaction effect initiating a skill test — 5a, 6), the `revelation_treacheries` suite (Crypt Chill / Grasping Hands — 3a), and any asset-enter-play + on-play-reaction-window card (2).
- **Engine unit tests** in the touched modules may need updating where they call a migrated helper directly (mirroring the Slice C pattern: add `drive` after, or assert the new park-then-drive contract).
- Only the `evaluator`/`choice` effect tests are *expected* to change (the wrapper they call is deleted).

## Commit ordering (one PR, bisectable commits)

1. **`push_effect` helper + push-and-return sites (1a, 1b, 6, 5a).** Extract `push_effect`; migrate the zero-/trivial-post-logic sites (5a includes the `bump_usage` reorder). `apply_effect` still exists for the rest.
2. **`EncounterCard` disposition (3a, 3b).** Add `disposition`; push it from both arms; route the drive arm.
3. **Skill-test cluster (4a, 4b).** Push on_success/on_fail as `SkillTest` children; preserve ordering + suspend semantics.
4. **`PlayFromHand` frame (2, 5b).** New variant + drive arm + `stage` disposal; migrate `complete_play`, `resume_play_card`, the no-AoO play path, and `play_fast_event`.
5. **Delete `apply_effect` + `drive_effect_to_base`; rework the effect tests** to `push_effect` + real `drive`.

Each commit keeps the full strict gauntlet green. The wrapper is removed only in commit 5, once no production site references it.

## What "done" looks like

- No production site calls `apply_effect`/`drive_effect_to_base`; both are deleted; `grep` finds no references outside the (reworked) tests' use of `push_effect` + `drive`.
- `Continuation::PlayFromHand` owns normal + Fast hand-plays; `EncounterCard { disposition }` owns treachery + enemy revelation.
- Full suite green (strict `test`/`clippy`/`fmt`/`doc` + `wasm-build`/`wasm-clippy`), card + integration suites untouched.

## Open questions

- **`PlayFromHand.stage` shape.** The exact stage enum (e.g. `RunEffect` → `EnterPlay` → `AfterEnterWindow`) is settled during implementation against `complete_play`'s current tail; the spec fixes the *responsibility* (run effect, then type-disposition with a resumable asset-entrance), not the variant names.
- **Event "stash for discard" timing.** `complete_play`/`play_card` already stash a played event for discard-on-completion (`ActionResume::PlayCard` docs note "an event has also left hand and is stashed for discard-on-completion"). The migration must keep the discard firing exactly once, from `PlayFromHand`'s disposal, for both the normal and Fast event paths — reconciled against the current stash mechanism during commit 4.
