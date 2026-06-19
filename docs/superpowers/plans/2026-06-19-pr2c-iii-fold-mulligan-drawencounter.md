# PR 2c-iii (#348, part 3c) — Fold `Mulligan` / `DrawEncounterCard` into `ResolveInput`; cursors → frames — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans (or subagent-driven-development) to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove the last two non-standard `PlayerAction`s (`Mulligan`, `DrawEncounterCard`) by turning the setup-mulligan loop and the Mythos-step-1.4 draw loop into `AwaitingInput`-driven `Continuation` frames resumed via `ResolveInput`. After this, every player-facing suspension resumes through `ResolveInput`, and the `mulligan_pending` / `mythos_draw_pending` cursors are gone.

**Architecture:** This is **control-flow**, not a wire rename. Two independent halves, **each its own PR** (a: Mulligan, b: DrawEncounterCard):
- A loop becomes a `Continuation` frame holding `remaining: Vec<InvestigatorId>` (turn order; head = current actor). The phase driver that *set the cursor* now **pushes the frame + returns `AwaitingInput`** for `remaining[0]`. The old dedicated-action handler becomes a `resume_*` that advances `remaining` and either re-emits `AwaitingInput` for the next actor or, when drained, pops the frame and runs the phase tail.
- The two actions leave `PlayerAction`; `resolve_input` gains a dispatch arm per frame; the per-cursor guards/arms in `apply_player_action` collapse into top-frame dispatch.

**Tech Stack:** Rust — `game-core` (engine), `server`, `web`, and tests across the workspace. **Wire + replay-log contract change** (two actions removed) → touches `server`/`web` more than any prior sub-PR.

This is **PR 2c-iii** (2c-i `PickMultiple` ✅, 2c-ii `PickSingle` ✅; this). Spec §A–B (Tier-C cursors + the action fold). Series: #345 ✅ → #348 (2a ✅ · 2b ✅ · 2c-i ✅ · 2c-ii ✅ · **2c-iii-a · 2c-iii-b**) → #347 → #380.

## Global Constraints

- **CI gauntlet, warnings-as-errors** (all six jobs) before every push.
- **No behavior change** to the *game logic* — only the *protocol* changes: setup and Mythos draw now round-trip through `AwaitingInput`/`ResolveInput` instead of dedicated actions. The mulligan redraw / encounter-draw / surge-chain effects are byte-for-byte preserved.
- **`StartScenario` and the Mythos-phase entry now return `AwaitingInput`** (the first mulligan / first draw prompt) where they previously returned `Done` with a cursor set. Every test that did `StartScenario → (assert Done) → Mulligan` becomes `StartScenario → (assert AwaitingInput) → ResolveInput(PickMultiple)`. This is the largest test-shape change in the series.
- **Branches:** `engine/fold-mulligan` (PR a) then `engine/fold-draw-encounter` (PR b), each off fresh `main`. Commit per task; push only when green.

---

## PR 2c-iii-a — `Mulligan` → `ResolveInput(PickMultiple)`; `mulligan_pending` → `Mulligan` frame

### Surface
- `PlayerAction::Mulligan { investigator, indices_to_redraw }` (`action.rs:156`).
- `apply_player_action` (`mod.rs`): the mulligan guard (`:68-80`), the `Mulligan` dispatch arm (`:194`), and the post-mulligan `investigation_phase` kickoff (`:230-248`).
- `cards::mulligan` (`cards.rs:287-366`) — validates indices, redraws, advances `mulligan_pending` via `next_active_investigator_after`.
- `phases.rs:163` (`start_scenario` sets `mulligan_pending = first_active_investigator`).
- `mulligan_pending` field (`game_state.rs`) + builder `with_mulligan_pending` + builder init.
- ~19 files constructing `PlayerAction::Mulligan`.

### Design
- New `Continuation::Mulligan { remaining: Vec<InvestigatorId> }` (+ classify in the two `as_resolution`/`as_resolution_mut` arms). `remaining[0]` is the current mulliganer; turn order, Active only.
- A shared `fn prompt_mulligan(cx) -> EngineOutcome`: read the top `Mulligan` frame's `remaining[0]`, return `AwaitingInput { request: choice("…mulligan… submit PickMultiple with hand indices to redraw (empty = keep)", hand-as-options-OR-prompt) }`. (Per 2c-i, offered-options population for the hand is deferred to #205; for now `OptionId(i)` = hand index, request may stay a prompt or carry minimal options — match the commit-window decision in 2c-i.)
- `start_scenario` (`phases.rs:163`): replace `mulligan_pending = …` with: push `Continuation::Mulligan { remaining: active_investigators_in_turn_order(state) }`; if `remaining` is empty (no active investigators — degenerate) fall straight through to `investigation_phase`; else `return prompt_mulligan(cx)`. **`start_scenario`'s outcome becomes `AwaitingInput`.**
- `cards::mulligan` → `resume_mulligan(cx, response)`: destructure `PickMultiple { selected }` → `indices: Vec<u8>` (the redraw hand indices, `o.0 as u8`); run the **unchanged** validation + redraw + `MulliganPerformed` event against the frame's `remaining[0]`; then `remaining.remove(0)`; if empty → pop the `Mulligan` frame and call `investigation_phase` (the post-mulligan kickoff moves here from `apply_player_action`); else `prompt_mulligan(cx)` for the next.
- `apply_player_action`: delete the mulligan guard, the `Mulligan` arm, and the post-mulligan-kickoff block. Add `Continuation::Mulligan(_)` to `resolve_input`'s top-frame match → `resume_mulligan`. (Note: `Mulligan` is the only frame that can be top *during setup before the mulligan completes* — top-frame dispatch handles it; the old "reject everything but Mulligan/StartScenario" guard is replaced by the generic input-awaiting reject for frame-suspensions.)
- Remove `PlayerAction::Mulligan`, `mulligan_pending` field, `GameStateBuilder::with_mulligan_pending` (or repoint it to stage a `Mulligan` frame — check callers; a `with_mulligan_remaining([ids])` staging the frame is the natural replacement).

### Tasks
- [ ] **a1 — `Mulligan` frame + `prompt_mulligan` + `resume_mulligan`; `start_scenario` emits `AwaitingInput`.** Add the variant + classifier; add `prompt_mulligan`; convert `cards::mulligan` into `resume_mulligan` (reads `PickMultiple`, drives `remaining`, kicks off `investigation_phase` on drain); rewire `start_scenario`. Keep `PlayerAction::Mulligan` + `mulligan_pending` *temporarily* unused-ish so the build stays green is NOT possible here (the flows are entangled) — so a1 also: delete the guard/arm/kickoff, add the `resolve_input` arm, remove the field + builder helper, and migrate the in-crate engine tests. Big task; the compiler + the exhaustive `as_resolution` match guide it.
- [ ] **a2 — Fold `PlayerAction::Mulligan` → `ResolveInput(PickMultiple)` at all call sites + remove the action.** Mechanical: `Mulligan { investigator, indices_to_redraw: vec![a,b] }` → `ResolveInput { response: PickMultiple { selected: vec![OptionId(a), OptionId(b)] } }` (empty redraw → `selected: vec![]`). The `investigator` is dropped (the frame's `remaining[0]` is the actor). Update `StartScenario`-then-`Mulligan` test shapes (`StartScenario` now returns `AwaitingInput`). Covers `server`/`web` + ~19 test files. Then delete `PlayerAction::Mulligan`. Full gauntlet.

---

## PR 2c-iii-b — `DrawEncounterCard` → `ResolveInput(Confirm)`; `mythos_draw_pending` → `EncounterDraw` frame

### Surface
- `PlayerAction::DrawEncounterCard` (`action.rs:288`); its dispatch arm (`mod.rs:215`).
- `draw_encounter_card` (`encounter.rs:608`) → `mythos_draw_for` → `run_mythos_draw_chain` (surge loop) → `advance_mythos_draw_pending` (`encounter.rs:771`).
- `phases.rs:387` (`mythos_phase` sets `mythos_draw_pending = first_active`; `None` ⇒ skip to the after-draws window).
- `resume_spawn_engage` (`hunters.rs:552`) re-enters the surge chain when `mythos_draw_pending == investigator_to_draw`.
- `mythos_draw_pending` field + ~13 files constructing `DrawEncounterCard`.

### Design
- New `Continuation::EncounterDraw { remaining: Vec<InvestigatorId> }` (+ classify). `remaining[0]` = current drawer.
- `fn prompt_encounter_draw(cx) -> EngineOutcome`: `AwaitingInput { request: prompt("Mythos 1.4: {remaining[0]:?} draws an encounter card; submit ResolveInput::Confirm") }`. (No options — `Confirm` is a binary proceed.)
- `mythos_phase` (`phases.rs:387`): push `EncounterDraw { remaining: active_in_turn_order }`; if empty → the existing `MythosAfterDraws` path; else `return prompt_encounter_draw(cx)`. **Mythos entry now returns `AwaitingInput`** (it's reached from the upkeep→Mythos cascade and from round-1… trace the callers; the cascade currently runs `mythos_phase` inline and returns its outcome — confirm it propagates `AwaitingInput`).
- `draw_encounter_card` → `resume_encounter_draw(cx, response)`: require `Confirm`; run `run_mythos_draw_chain(cx, remaining[0], 0, true)` (unchanged surge logic; it may suspend on a spawn-engage `SpawnEngage` frame — see below); on the chain's `Done`, `remaining.remove(0)`; if empty → pop the `EncounterDraw` frame + open the `MythosAfterDraws` window (what `advance_mythos_draw_pending`→`None` did); else `prompt_encounter_draw` for the next.
- **Surge / spawn-engage interplay (the tricky bit):** today `resume_spawn_engage` re-enters the chain when `mythos_draw_pending == investigator_to_draw`. Replace that read with "the top non-`SpawnEngage` frame is an `EncounterDraw` whose `remaining[0] == investigator_to_draw`" — i.e. the drawer is still mid-Mythos-draw. The `SpawnEngage` frame sits **above** the `EncounterDraw` frame; when it resolves it re-drives the chain for the same drawer. `advance_mythos_draw_pending` is absorbed into `resume_encounter_draw`'s `remaining.remove(0)`.
- `apply_player_action`: delete the `DrawEncounterCard` arm. Add `Continuation::EncounterDraw(_)` → `resume_encounter_draw` in `resolve_input`.
- Remove `PlayerAction::DrawEncounterCard`, `mythos_draw_pending` field.

### Tasks
- [ ] **b1 — `EncounterDraw` frame + `prompt`/`resume`; `mythos_phase` emits `AwaitingInput`; rewire surge + spawn-engage re-entry.** Add variant + classifier; `prompt_encounter_draw`; convert `draw_encounter_card`→`resume_encounter_draw` driving `remaining`; rewire `mythos_phase`; repoint `resume_spawn_engage`'s re-entry condition + `advance_mythos_draw_pending`'s removal onto the frame. Delete the `DrawEncounterCard` arm; add the `resolve_input` arm; remove the field; migrate in-crate engine tests (the `mythos_draw_pending` reads/sets in `encounter.rs`/`phases.rs` tests → inspect/stage the `EncounterDraw` frame).
- [ ] **b2 — Fold `PlayerAction::DrawEncounterCard` → `ResolveInput(Confirm)` at all call sites + remove the action.** Mechanical: `DrawEncounterCard` → `ResolveInput { response: Confirm }`. Update the Mythos-entry test shapes (now `AwaitingInput`). Covers `server`/`web` + ~13 test files. Delete the action. Full gauntlet.

---

## Cross-cutting (do in whichever PR lands second)

- **Spec correction (owed):** update `docs/superpowers/specs/2026-06-19-continuation-stack-cleanup-design.md` — its "Sequencing" still lists the old `#347`-before-`#348` order (now `#345 → #348 → #347 → #380`), and re-add the `InputResponse` normalization (2c-i/ii/iii) to the §1 scope (it was dropped from the original spec). Also note the `PickLocation`/`PickInvestigator` consolidation landed in 2c-ii (not deferred to #205 as the spec's earlier text implied — only the *labels/rendering* defer).
- **Phase-7 doc:** flip the §1 progress line; the `#205` client-render note still lists `PickInvestigator`/`DiscardCards` (now `PickSingle`/`PickMultiple`) — reword.
- After 2c-iii: the engine's only player-input channel is `ResolveInput`; `PlayerAction` is the turn-action set + `StartScenario` + `ResolveInput`. The §1 cleanup's router/taxonomy goals are met; remaining series work is **#347** (token-route `ResolveInput`, now trivial on the unified channel) and **#380** (revelation disposal).

## Self-Review

**Spec/§B coverage:** both actions fold into `ResolveInput` (`PickMultiple` / `Confirm`); `mulligan_pending`/`mythos_draw_pending` become loop frames; setup + Mythos emit `AwaitingInput`; cursors removed. ✓ Tier-C from the spec's §A table is done; `enemy_attack_pending` stays a cursor (it's framework-sequencing, not player-facing — unchanged). ✓

**Placeholder scan:** the mulligan/draw prompt's offered-options population is explicitly deferred to #205 (consistent with 2c-i); the surge/spawn-engage re-entry condition is described precisely (top non-`SpawnEngage` frame is the `EncounterDraw` for the drawer) but the implementer must read `run_mythos_draw_chain` + `resume_spawn_engage` to wire it — flagged as the task's known-tricky point. Mechanical action-site folds give the exact transformation + the file count.

**Risk flags:** (1) `StartScenario`/Mythos-entry returning `AwaitingInput` is a broad test-shape change — most of the churn. (2) The surge-chain + spawn-engage re-entry is the one place logic (not just protocol) is delicate; pin it with the existing `mythos_phase` surge + spawn-engage integration tests (`scenarios/tests/{mythos_phase,encounter_spawn}.rs`), which must pass unchanged in behavior. (3) `server`/`web` removing two actions — confirm no client UI path still constructs them (the web client builds `DrawEncounterCard`? grep `crates/web/src` — if so it migrates to a Confirm control).

**Out of scope:** tokens (#347), revelation disposal (#380), human option labels + client rendering (#205), `enemy_attack_pending` cursor.

---

## Execution notes (from a first pass on 2c-iii-a, reverted clean)

A first execution of **2c-iii-a** got the engine half done cleanly but was reverted before finishing the test surface (end of a long session). What it surfaced — bake these into the next run:

- **The engine half is straightforward** and worked: `Continuation::Mulligan { remaining }` + classifier arms; `cards::prompt_mulligan` + `resume_mulligan` (reads `PickMultiple { selected }` → redraw hand indices, drives `remaining`, `investigation_phase` on drain); `start_scenario` pushes the frame and returns `prompt_mulligan` (so its outcome is `AwaitingInput`, or `Done` straight to `investigation_phase` when no active investigators); the `mulligan_pending` guard in `apply_player_action` → a `Mulligan`-frame guard; remove the `Mulligan` dispatch arm + the post-mulligan kickoff block; add `Some(Continuation::Mulligan { .. }) => cards::resume_mulligan` to `resolve_input`; remove the `mulligan_pending` field.

- **Add a `GameState::current_mulligan() -> Option<InvestigatorId>` accessor** (reads the top `Mulligan` frame's `remaining[0]`). It makes the test read-site migration a clean rename: `state.mulligan_pending` → `state.current_mulligan()`.

- **Builder:** keep a staging field but the helper must stage the *full* remaining queue, not one id — replace `with_mulligan_pending(id)` with `with_mulligan_remaining(impl IntoIterator<Item = InvestigatorId>)`, pushing `Continuation::Mulligan { remaining }` in `build()`. The single-id form breaks the multi-investigator advance tests.

- **Obsolete test (remove it):** the "out-of-order mulligan rejected" unit test (`engine/mod.rs`) tested that `Mulligan { investigator: wrong }` rejects on cursor mismatch. The folded action carries **no** investigator (the frame's `remaining[0]` is the actor), so that rejection path no longer exists — delete the test rather than port it.

- **`StartScenario` now returns `AwaitingInput`:** every test doing `StartScenario → (assert Done) → Mulligan` becomes `StartScenario → (assert AwaitingInput) → ResolveInput(PickMultiple)`. Dropping the action's `investigator` field leaves unused `inv`/`id` bindings in some tests (`_`-prefix or remove).

- **Mechanical action-fold transform** (worked across ~19 files with one perl in the first pass): `PlayerAction::Mulligan { investigator: _, indices_to_redraw: vec![a, b] }` → `PlayerAction::ResolveInput { response: InputResponse::PickMultiple { selected: vec![OptionId(a), OptionId(b)] } }` (empty → `vec![]`). Then add `InputResponse`/`OptionId` imports where the compiler flags (`scenarios/tests/{synthetic_resolution,upkeep_phase,the_gathering,...}.rs`).

- **`server`:** `ws.rs` asserts on `state.mulligan_pending` → `state.current_mulligan()`.

- **`web` client — remove the dead dedicated mulligan UI.** `enabled_controls` already short-circuits to empty on `AwaitingInput` (`legality.rs`), and mulligan is now an `AwaitingInput`, so the `mulligan_pending` legality branch, `ActionControl::Mulligan`, and the `mulligan_picker` component (+ its `mulligan_sel` signal and `web/tests/controls.rs` cases) are all unreachable. Remove them; the mulligan now flows through `input.rs`'s `AwaitingInputView`, which already builds `PickMultiple` from selected hand cards — functionally correct. The prompt-text/labeling polish is **#205**.

The same shape applies to **2c-iii-b** (`DrawEncounterCard` → `Confirm`): expect a `current_encounter_drawer()` accessor, a `with_mythos_draw_remaining` builder helper, `mythos_phase` returning `AwaitingInput`, the analogous server/web touchpoints, and the surge/spawn-engage re-entry rewire called out in the design above.
