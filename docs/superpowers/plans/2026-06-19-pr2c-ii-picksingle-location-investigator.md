# PR 2c-ii (#348, part 3b) — `PickLocation`/`PickInvestigator` → `PickSingle` via structured options — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Retire `InputResponse::PickLocation` / `PickInvestigator` by migrating the three windows that use them (hunter-move, hunter-engage, spawn-engage) to the existing Axis-A structured-choice contract — the request carries the candidates as labeled `ChoiceOption`s and resume comes back as `PickSingle(OptionId)`. After this, `InputResponse` = `PickSingle · PickMultiple · Skip · Confirm`.

**Architecture:** The three windows already hold their candidate lists on the suspended frame (`HunterChoice::{Move,Engage}.candidates`, `SpawnEngagePending.candidates`). The only new engine work is **emitting** those candidates as `ChoiceOption { id: OptionId(i), label: "{candidate:?}" }` at suspend, and **indexing** back (`candidates[i]`) at resume. Labels are debug reprs for now; **#205 owns turning them into human names + rendering the control** — this PR settles only the *data contract*. The web client has **no** production use of these variants (verified), so it is untouched.

**Tech Stack:** Rust — `game-core` (engine + `test_support` resolver), scenario integration tests. No `web`/`cards`/`server` changes.

This is **PR 2c-ii** (2c-i `PickMultiple` ✅; this; 2c-iii action fold). Umbrella §2–3 (structured request + `PickSingle` consolidation). Series: #345 ✅ → #348 (2a ✅ · 2b ✅ · 2c-i ✅ · **2c-ii** · 2c-iii) → #347 → #380.

## Global Constraints

- **CI gauntlet, warnings-as-errors** (all before pushing): test / clippy `--all-targets --all-features` / fmt / `RUSTDOCFLAGS="-D warnings" doc` / `wasm-build` / `wasm-clippy`.
- **No behavior change.** `OptionId(i)` ⇒ `candidates[i]`; the move/engage/spawn-engage effects and their validation (candidate membership) are preserved — `contains(id)` becomes `i < candidates.len()` against the same list.
- **Wire/replay break** (the two variants leave `InputResponse`). Acceptable pre-1.0.
- **Branch:** `engine/input-picksingle-spatial` off fresh `main`. Commit per task; push only when the full gauntlet is green.

## Surface (surveyed)

- **Suspend sites:** `hunters.rs:~388-401` (`suspend_hunter_choice` — Move + Engage prompts) and `encounter.rs:~463-477` (spawn-engage). Both build `InputRequest::prompt(…)` today.
- **Resume handlers:** `resume_hunter_choice` (`hunters.rs` — matches `(HunterChoice::Move, PickLocation)` / `(HunterChoice::Engage, PickInvestigator)`, validates `candidates.contains`) and `resume_spawn_engage` (`hunters.rs:~513` — `let PickInvestigator(who) = response`, validates `candidates.contains`).
- **Guard/prompt/error strings naming the variants:** `mod.rs:124,140`; `hunters.rs:390,394,463,471,516`; `encounter.rs:477`.
- **`ScriptedResolver`:** `pick_location` / `pick_investigator` helpers (`resolver.rs:121-129`) push the literal variant; the FIFO test (`:426-427,436,440`) uses them.
- **Engine unit tests constructing the variants directly:** `hunters.rs:1072,1124,1177,1287,1346,1409`; `phases.rs:2477`; `encounter.rs:1699`.
- **Integration tests (via the resolver):** `scenarios/tests/{hunter_movement,encounter_spawn,mythos_phase}.rs`.
- **No web/src production use** (only docs/legacy-free).

---

### Task 1: Migrate the three windows to structured options + `PickSingle`

Suspend emits candidates as options; resume reads `PickSingle`; rework the resolver; migrate all test sites. `PickLocation`/`PickInvestigator` become unused (removed in Task 2).

**Files:** `hunters.rs`, `encounter.rs`, `mod.rs` (guard messages), `test_support/resolver.rs`, the unit-test sites above, and the three scenario integration tests.

**Interfaces:**
- Suspend produces `InputRequest::choice(prompt, candidates.iter().enumerate().map(|(i,c)| ChoiceOption { id: OptionId(i as u32), label: format!("{c:?}") }).collect())`.
- Resume consumes `InputResponse::PickSingle(OptionId(i))` ⇒ `candidates[i as usize]`.
- `ScriptedResolver`: `pick_location(LocationId)` / `pick_investigator(InvestigatorId)` keep their signatures but resolve at `next()` time to `PickSingle` by matching the request's option whose `label == format!("{id:?}")`.

- [ ] **Step 1: Hunter suspend → structured options**

In `suspend_hunter_choice` (`hunters.rs`), the prompt-build branches for `HunterChoice::Move { enemy, candidates }` and `Engage { enemy, candidates }`: replace the final `EngineOutcome::AwaitingInput { request: InputRequest::prompt(prompt), … }` with a `InputRequest::choice(prompt, opts)` where

```rust
let opts: Vec<ChoiceOption> = candidates
    .iter()
    .enumerate()
    .map(|(i, c)| ChoiceOption {
        id: OptionId(u32::try_from(i).expect("candidate count fits u32")),
        label: format!("{c:?}"),
    })
    .collect();
```

(keep the existing human prompt text; import `ChoiceOption`, `OptionId`). Both Move and Engage candidate lists are `Vec<LocationId>` / `Vec<InvestigatorId>` — `format!("{c:?}")` gives `"LocationId(99)"` / `"InvestigatorId(2)"`.

- [ ] **Step 2: `resume_hunter_choice` → `PickSingle`**

Replace the two `match (&pending, response)` arms. Instead of matching `PickLocation(loc)` / `PickInvestigator(who)`, match `InputResponse::PickSingle(OptionId(i))` and resolve against the pending choice's candidates:

```rust
let current_enemy = match (&pending, response) {
    (HunterChoice::Move { enemy, candidates }, InputResponse::PickSingle(OptionId(i))) => {
        let Some(&loc) = candidates.get(*i as usize) else {
            return EngineOutcome::Rejected {
                reason: format!("ResolveInput: hunter move option {i} out of range (0..{})", candidates.len()).into(),
            };
        };
        cx.state.continuations.pop(); // validated; pop the HunterMove frame
        move_hunter_to(cx, *enemy, loc);
        if let Some(choice) = engage_on_arrival(cx, *enemy) {
            return suspend_hunter_choice(cx, choice);
        }
        *enemy
    }
    (HunterChoice::Engage { enemy, candidates }, InputResponse::PickSingle(OptionId(i))) => {
        let Some(&who) = candidates.get(*i as usize) else {
            return EngineOutcome::Rejected {
                reason: format!("ResolveInput: hunter engage option {i} out of range (0..{})", candidates.len()).into(),
            };
        };
        cx.state.continuations.pop();
        engage_enemy_with(cx, *enemy, who);
        *enemy
    }
    (_, other) => {
        return EngineOutcome::Rejected {
            reason: format!("ResolveInput: hunter choice expects InputResponse::PickSingle, got {other:?}").into(),
        };
    }
};
```

(Preserves validate-first: pop only after the index is validated. The two prior shape-mismatch arms collapse into the one `(_ , other)` reject.)

- [ ] **Step 3: Spawn-engage suspend + resume**

`encounter.rs` spawn-engage suspend: same `InputRequest::choice` treatment over `tied` (the candidate list). `resume_spawn_engage` (`hunters.rs`): replace `let PickInvestigator(who) = response else {…}` + `candidates.contains` with the `PickSingle(OptionId(i))` → `pending.candidates.get(i)` form (mirror Step 2's Engage arm; pop after validation as 2b established).

- [ ] **Step 4: Reword guard/prompt/error strings**

`mod.rs:124,140` (guard messages), `hunters.rs:390,394` (prompts — though the prompt text is now paired with structured options, keep it human and drop the `submit InputResponse::PickLocation` clause), `hunters.rs:463,471,516`, `encounter.rs:477`: replace `PickLocation`/`PickInvestigator` references with `PickSingle`.

- [ ] **Step 5: Rework `ScriptedResolver`**

In `test_support/resolver.rs`: add a `ScriptedStep::PickByLabel(String)` variant. Change `pick_location` / `pick_investigator` to push `ScriptedStep::PickByLabel(format!("{id:?}"))` (keep their `&mut self`-chaining signatures). In `next()`, resolve it:

```rust
ScriptedStep::PickByLabel(label) => {
    let opt = request.options.iter().find(|o| o.label == label).unwrap_or_else(|| {
        panic!("ScriptedResolver::pick_*: no offered option labeled {label:?}; prompt {:?}, options {:?}", request.prompt, request.options)
    });
    InputResponse::PickSingle(opt.id)
}
```

Update the resolver's own FIFO test (`:424-441`): the `pick_location`/`pick_investigator` lines now need an option-bearing request — simplest is to switch those two lines + their assertions to `pick_single(OptionId(..))` against `req("pick")` (the FIFO test only checks ordering, not window semantics; the label-matching path is covered by the integration tests).

- [ ] **Step 6: Migrate the engine unit-test direct constructions**

`hunters.rs:1072,1124,1177,1287,1346,1409`, `phases.rs:2477`, `encounter.rs:1699`: each constructs `&InputResponse::PickLocation(LocationId(n))` / `PickInvestigator(InvestigatorId(n))` and feeds it to a resume handler. Replace with `&InputResponse::PickSingle(OptionId(i))` where `i` is that location/investigator's **index in the test's candidate list**. Read each test's setup to determine the candidate order (the candidates are the equidistant-nearest locations / co-located investigators the test arranges); pick the index whose `candidates[i]` equals the id the test previously passed. Add a short `// candidates: [LocationId(2), LocationId(3)] → option 0 is LocationId(2)` comment where the ordering isn't obvious.

- [ ] **Step 7: Build + fix + gauntlet**

`cargo build -p game-core --all-targets` (fix stragglers); then full gauntlet. Integration tests (`hunter_movement`, `encounter_spawn`, `mythos_phase`) exercise the resolver's label-matching end-to-end. Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add -A
git commit -m "engine: structured-options + PickSingle for hunter/spawn windows (#348)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 2: Remove `PickLocation` / `PickInvestigator`

**Files:** `crates/game-core/src/action.rs` (+ any doc-link fixups).

- [ ] **Step 1: Remove the variants** from `InputResponse` (and their doc-comments). The `PickSingle` doc currently says they "consolidate into it in a follow-up (2c-ii)" — update it to drop that forward-reference (they're gone now).

- [ ] **Step 2: Compile** — `cargo build -p game-core --all-targets` + `cargo build -p web --target wasm32-unknown-unknown`. Any error is a missed Task-1 site → fix to `PickSingle`.

- [ ] **Step 3: Doc links** — grep `PickLocation`/`PickInvestigator` for surviving intra-doc `[\`…\`]` links (e.g. the `resolver.rs` `ScriptedResolver` helper-list doc) and repoint/reword. `RUSTDOCFLAGS="-D warnings" cargo doc` confirms.

- [ ] **Step 4: Full gauntlet** — all six. Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "engine: remove PickLocation/PickInvestigator (folded into PickSingle) (#348)

InputResponse is now PickSingle / PickMultiple / Skip / Confirm.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Self-Review

**Spec/umbrella coverage:** the three location/investigator-pick windows adopt the structured-choice contract (`InputRequest::choice` + `PickSingle`); `PickLocation`/`PickInvestigator` removed; `InputResponse` reaches the umbrella's `PickSingle · PickMultiple · Skip · Confirm`. The *data* contract (options + `PickSingle`) lands now; #205 inherits only human labels + client rendering (stated in Architecture). ✓

**Placeholder scan:** Step 6 says "read each test's setup to determine the candidate order" rather than hard-coding indices, because the index depends on each test's board arrangement (the implementer must read the candidate list). All other steps show concrete code. The resolver label-match is fully specified.

**Type consistency:** `OptionId(u32)` is the candidate index throughout (suspend builds it, resume `candidates.get(i as usize)`, resolver returns `opt.id`); `ChoiceOption { id, label }` matches the Axis-A type used by the choice/reaction windows.

**Risk flag:** label-matching in the resolver couples to `format!("{id:?}")` equality between suspend and resolver. It's deterministic (both sides use the same `Debug`), but if a future window labels options differently, the resolver helper won't match — acceptable for test infra, and the panic message surfaces it loudly.

**Out of scope:** human-readable labels + client rendering of these picks (#205); `Mulligan`/`DrawEncounterCard` action fold (2c-iii); tokens (#347); revelation disposal (#380).
