# PR 1 (#345) — Serializable EvalContext with grouped bindings — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `EvalContext` serializable with cohesive grouped bindings, snapshotted whole onto suspended frames, killing the per-frame rebuild duplication and the latent cross-suspend binding-loss.

**Architecture:** Three tasks, each compiles + passes CI. Task 1 introduces accessor/setter *indirection* over the current flat fields (pure refactor, zero behavior change). Task 2 swaps the internal representation to grouped `Option<…Binding>` sub-structs behind that indirection and derives serde. Task 3 makes `ChoiceFrame` snapshot the whole `EvalContext` instead of `controller`+`source`, fixing the latent binding-loss.

**Tech Stack:** Rust, `serde`, the workspace's `game-core` / `cards` / `scenarios` crates.

This is **PR 1 of 4** in the §1 continuation-stack cleanup (spec: `docs/superpowers/specs/2026-06-19-continuation-stack-cleanup-design.md`). It is foundational: it establishes the context-snapshot storage shape the later frame migration (#348) consumes. No behavior change for any in-scope card; the only observable new capability is that window bindings survive a choice suspend.

## Global Constraints

- **CI gauntlet, warnings-as-errors.** Before pushing, all must pass:
  - `RUSTFLAGS="-D warnings" cargo test --all --all-features`
  - `cargo clippy --all-targets --all-features -- -D warnings`
  - `cargo fmt --check`
  - `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`
  - `cargo build -p web --target wasm32-unknown-unknown`
  - `cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings`
- **Crate layering:** `game-core` never depends on `cards`/`scenarios`. The binding types live in `game-core` (`evaluator.rs`); `cards`/`scenarios` consume them through accessors only.
- **`EvalContext` stays `Copy`.** All binding sub-structs are small (`u8` / id newtypes) and must derive `Copy`, so `EvalContext` remains `Copy` — many call sites pass it by value (`eval_ctx: EvalContext`).
- **Single branch** `engine/continuation-stack-cleanup` (already created; the spec commit is on it). Commit per task; do not push until the full gauntlet is green.
- **No `TODO` for same-kind nesting** — corpus-verified moot (see spec §D). Document innermost-only semantics as a plain fact in the doc-comment.

---

### Task 1: Accessor + setter indirection over the flat fields

Introduce methods that read/write the window-bound fields, and migrate every call site to them — *without* changing the struct's representation yet. Pure refactor: after this task `EvalContext` still has the same flat fields, but no code outside `evaluator.rs` touches them directly. This isolates the representation swap (Task 2) to one file.

**Files:**
- Modify: `crates/game-core/src/engine/evaluator.rs` (add methods on `EvalContext`; migrate internal read/write sites)
- Modify: `crates/game-core/src/engine/dispatch/skill_test.rs:344` (failed_by write)
- Modify: `crates/game-core/src/engine/dispatch/reaction_windows.rs:658,665` (attacking_enemy, clue_discovery_count writes)
- Modify: `crates/cards/src/impls/cover_up.rs:69,72` (clue_discovery_count reads)
- Modify: `crates/cards/src/impls/guard_dog.rs:55` (attacking_enemy read)
- Modify: `crates/cards/src/impls/crypt_chill.rs:73` (chosen_option read)
- Modify: `crates/cards/src/impls/dynamite_blast.rs:84` (chosen_option read)
- Modify: `crates/scenarios/src/test_fixtures/synth_cards.rs:221,224` (clue_discovery_count reads)

**Interfaces:**
- Produces (methods on `EvalContext`, all `#[must_use]` on getters):
  - `fn failed_by(&self) -> Option<u8>`
  - `fn clue_discovery_count(&self) -> Option<u8>`
  - `fn attacking_enemy(&self) -> Option<EnemyId>`
  - `fn chosen_investigator(&self) -> Option<InvestigatorId>`
  - `fn chosen_location(&self) -> Option<LocationId>`
  - `fn chosen_enemy(&self) -> Option<EnemyId>`
  - `fn chosen_option(&self) -> Option<OptionId>`
  - `fn set_failed_by(&mut self, margin: u8)`
  - `fn set_clue_discovery_count(&mut self, count: u8)`
  - `fn set_attacking_enemy(&mut self, enemy: EnemyId)`
  - `fn set_chosen_investigator(&mut self, id: InvestigatorId)`
  - `fn set_chosen_location(&mut self, id: LocationId)`
  - `fn set_chosen_enemy(&mut self, id: EnemyId)`
  - `fn set_chosen_option(&mut self, opt: Option<OptionId>)`

- [ ] **Step 1: Add the methods (reading/writing the existing flat fields)**

In `crates/game-core/src/engine/evaluator.rs`, add a new `impl EvalContext` block immediately after the existing one (after `for_controller_with_optional_source`):

```rust
impl EvalContext {
    /// Just-resolved skill test's failure margin (bound only while running an
    /// `on_fail` effect). See [`Effect::ForEachPointFailed`].
    #[must_use]
    pub fn failed_by(&self) -> Option<u8> {
        self.failed_by
    }
    /// Clue count a before-discovery interrupt is replacing (bound only while
    /// resolving a `WouldDiscoverClues` ability's effect).
    #[must_use]
    pub fn clue_discovery_count(&self) -> Option<u8> {
        self.clue_discovery_count
    }
    /// Attacking enemy bound while resolving an `EnemyAttackDamagedSelf` reaction.
    #[must_use]
    pub fn attacking_enemy(&self) -> Option<crate::state::EnemyId> {
        self.attacking_enemy
    }
    /// Investigator picked for an `InvestigatorTarget::Chosen`.
    #[must_use]
    pub fn chosen_investigator(&self) -> Option<crate::state::InvestigatorId> {
        self.chosen_investigator
    }
    /// Location picked for a `LocationTarget::Chosen`.
    #[must_use]
    pub fn chosen_location(&self) -> Option<crate::state::LocationId> {
        self.chosen_location
    }
    /// Enemy picked for an `EnemyTarget::Chosen`.
    #[must_use]
    pub fn chosen_enemy(&self) -> Option<crate::state::EnemyId> {
        self.chosen_enemy
    }
    /// Option picked for a native leaf that suspended for a choice.
    #[must_use]
    pub fn chosen_option(&self) -> Option<crate::engine::OptionId> {
        self.chosen_option
    }

    pub fn set_failed_by(&mut self, margin: u8) {
        self.failed_by = Some(margin);
    }
    pub fn set_clue_discovery_count(&mut self, count: u8) {
        self.clue_discovery_count = Some(count);
    }
    pub fn set_attacking_enemy(&mut self, enemy: crate::state::EnemyId) {
        self.attacking_enemy = Some(enemy);
    }
    pub fn set_chosen_investigator(&mut self, id: crate::state::InvestigatorId) {
        self.chosen_investigator = Some(id);
    }
    pub fn set_chosen_location(&mut self, id: crate::state::LocationId) {
        self.chosen_location = Some(id);
    }
    pub fn set_chosen_enemy(&mut self, id: crate::state::EnemyId) {
        self.chosen_enemy = Some(id);
    }
    pub fn set_chosen_option(&mut self, opt: Option<crate::engine::OptionId>) {
        self.chosen_option = opt;
    }
}
```

- [ ] **Step 2: Verify it compiles (methods added, no sites migrated yet)**

Run: `cargo build -p game-core`
Expected: PASS (new methods, unused for now → may warn; that's fine pre-clippy).

- [ ] **Step 3: Migrate the read sites to getters**

Apply this exact transform (field access → method call) at each site. Worked example — `evaluator.rs:322`:

```rust
// before
let n = eval_ctx.failed_by.unwrap_or(0);
// after
let n = eval_ctx.failed_by().unwrap_or(0);
```

Apply the same field→method rename at every read site:
- `crates/game-core/src/engine/evaluator.rs:322` — `eval_ctx.failed_by` → `eval_ctx.failed_by()`
- `crates/game-core/src/engine/evaluator.rs:1423` — `eval_ctx.chosen_investigator.is_none()` → `eval_ctx.chosen_investigator().is_none()`
- `crates/game-core/src/engine/evaluator.rs:1433` — `eval_ctx.chosen_location.is_none()` → `eval_ctx.chosen_location().is_none()`
- `crates/game-core/src/engine/evaluator.rs:1443` — `eval_ctx.chosen_enemy.is_none()` → `eval_ctx.chosen_enemy().is_none()`
- `crates/game-core/src/engine/evaluator.rs:1644` — `ctx.chosen_investigator.ok_or(` → `ctx.chosen_investigator().ok_or(`
- `crates/game-core/src/engine/evaluator.rs:1662` — `ctx.chosen_location.ok_or(` → `ctx.chosen_location().ok_or(`
- `crates/game-core/src/engine/evaluator.rs:1684` — `ctx.chosen_enemy.ok_or(` → `ctx.chosen_enemy().ok_or(`
- `crates/cards/src/impls/cover_up.rs:69` — `ctx.clue_discovery_count.is_some()` → `ctx.clue_discovery_count().is_some()`
- `crates/cards/src/impls/cover_up.rs:72` — `ctx.clue_discovery_count.unwrap_or(0)` → `ctx.clue_discovery_count().unwrap_or(0)`
- `crates/cards/src/impls/guard_dog.rs:55` — `ctx.attacking_enemy` → `ctx.attacking_enemy()`
- `crates/cards/src/impls/crypt_chill.rs:73` — `ctx.chosen_option` → `ctx.chosen_option()`
- `crates/cards/src/impls/dynamite_blast.rs:84` — `ctx.chosen_option` → `ctx.chosen_option()`
- `crates/scenarios/src/test_fixtures/synth_cards.rs:221` — `ctx.clue_discovery_count.is_some()` → `ctx.clue_discovery_count().is_some()`
- `crates/scenarios/src/test_fixtures/synth_cards.rs:224` — `ctx.clue_discovery_count.unwrap_or(0)` → `ctx.clue_discovery_count().unwrap_or(0)`

- [ ] **Step 4: Migrate the write sites to setters**

- `crates/game-core/src/engine/dispatch/skill_test.rs:344` — `ctx.failed_by = Some(failed_by);` → `ctx.set_failed_by(failed_by);`
- `crates/game-core/src/engine/dispatch/reaction_windows.rs:658` — `eval_ctx.attacking_enemy = Some(enemy);` → `eval_ctx.set_attacking_enemy(enemy);`
- `crates/game-core/src/engine/dispatch/reaction_windows.rs:665` — `eval_ctx.clue_discovery_count = Some(count);` → `eval_ctx.set_clue_discovery_count(count);`
- `crates/game-core/src/engine/evaluator.rs:1290` — `eval_ctx.chosen_option = cursor.take();` → `eval_ctx.set_chosen_option(cursor.take());`
- `crates/game-core/src/engine/evaluator.rs:1467` — in the `bind` closure, `ctx.chosen_investigator = Some(id);` → `ctx.set_chosen_investigator(id);`
- `crates/game-core/src/engine/evaluator.rs:1508` — `ctx.chosen_location = Some(id);` → `ctx.set_chosen_location(id);`
- `crates/game-core/src/engine/evaluator.rs:1549` — `ctx.chosen_enemy = Some(id);` → `ctx.set_chosen_enemy(id);`
- `crates/game-core/src/engine/evaluator.rs:4266` (test) — `eval_ctx.failed_by = Some(2);` → `eval_ctx.set_failed_by(2);`

- [ ] **Step 5: Run the full test suite to verify no behavior change**

Run: `RUSTFLAGS="-D warnings" cargo test --all --all-features`
Expected: PASS (identical behavior; this is pure indirection).

- [ ] **Step 6: Clippy + fmt**

Run: `cargo clippy --all-targets --all-features -- -D warnings && cargo fmt --check`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "engine: route EvalContext window bindings through accessors

Introduce getter/setter methods over the flat window-bound fields and
migrate every read/write site (game-core, cards, scenarios) to them. Pure
indirection, no behavior change — isolates the representation swap (#345).

Refs #345.
Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 2: Swap to grouped serializable bindings

Replace the flat window-bound fields with cohesive `Option<…Binding>` sub-structs, behind the accessors from Task 1. Derive `Serialize`/`Deserialize` so frames can snapshot the whole context (used in Task 3).

**Files:**
- Modify: `crates/game-core/src/engine/evaluator.rs` (binding structs; `EvalContext` fields; constructors; accessor/setter bodies; add serde test)

**Interfaces:**
- Produces (public types in `game-core::engine::evaluator`, re-exported as needed):
  - `struct SkillTestBinding { pub failed_by: u8 }`
  - `struct DiscoveryBinding { pub clue_discovery_count: u8 }`
  - `struct EnemyAttackBinding { pub attacking_enemy: EnemyId }`
  - `struct ChoiceBinding { pub investigator: Option<InvestigatorId>, pub location: Option<LocationId>, pub enemy: Option<EnemyId>, pub option: Option<OptionId> }` (derives `Default`)
  - `EvalContext` gains `Serialize, Deserialize` derives; window-bound fields replaced by `skill_test`, `discovery`, `enemy_attack`, `choice` (each `Option<…Binding>`).
- Consumes: the accessor/setter method names from Task 1 (unchanged signatures).

- [ ] **Step 1: Define the binding sub-structs**

In `crates/game-core/src/engine/evaluator.rs`, immediately before `pub struct EvalContext`:

```rust
/// Failure margin of the just-resolved skill test (bound only while running an
/// `on_fail` effect). Innermost-only: same-kind test nesting is carried by the
/// per-frame snapshot stack, not multiple slots here (corpus-verified moot —
/// no card reads a non-innermost margin; see the §1 cleanup spec §D).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillTestBinding {
    /// Points the test was failed by.
    pub failed_by: u8,
}

/// Clue count a before-discovery interrupt is replacing (bound only while
/// resolving a `WouldDiscoverClues` ability's effect).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveryBinding {
    /// Clues the interrupt is replacing.
    pub clue_discovery_count: u8,
}

/// Attacking enemy bound while resolving an `EnemyAttackDamagedSelf` reaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnemyAttackBinding {
    /// The enemy whose attack is being reacted to.
    pub attacking_enemy: crate::state::EnemyId,
}

/// Controller picks bound while grounding `*::Chosen` targets. Cohesive: the
/// four `*::Chosen` kinds compose on one binding (a single effect may pick an
/// investigator *and* a location). `Default` is all-`None`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ChoiceBinding {
    /// `InvestigatorTarget::Chosen` pick.
    pub investigator: Option<crate::state::InvestigatorId>,
    /// `LocationTarget::Chosen` pick.
    pub location: Option<crate::state::LocationId>,
    /// `EnemyTarget::Chosen` pick.
    pub enemy: Option<crate::state::EnemyId>,
    /// Native-leaf option pick.
    pub option: Option<crate::engine::OptionId>,
}
```

- [ ] **Step 2: Restructure `EvalContext`**

Replace the seven flat window-bound fields (`failed_by` … `chosen_option`) with the four grouped fields, and add serde derives. The `controller` + `source` durable fields are unchanged.

```rust
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[non_exhaustive]
pub struct EvalContext {
    /// The investigator whose card-effect we're resolving — the "you" in card
    /// text. Resolves `InvestigatorTarget::You` / `LocationTarget::YourLocation`.
    pub controller: crate::state::InvestigatorId,
    /// The in-play card-instance that triggered this effect, if any.
    pub source: Option<crate::state::CardInstanceId>,
    /// Skill-test margin binding (bound only during an `on_fail` effect).
    pub skill_test: Option<SkillTestBinding>,
    /// Before-discovery interrupt binding (bound only during `WouldDiscoverClues`).
    pub discovery: Option<DiscoveryBinding>,
    /// Enemy-attack reaction binding (bound only during `EnemyAttackDamagedSelf`).
    pub enemy_attack: Option<EnemyAttackBinding>,
    /// Grounded `*::Chosen` picks (bound during a grounded-choice evaluation).
    pub choice: Option<ChoiceBinding>,
}
```

- [ ] **Step 3: Update the three constructors**

In `for_controller`, `for_controller_with_source`, and the `Some`/`None` arms of `for_controller_with_optional_source`, replace the seven `…: None,` field inits with:

```rust
            skill_test: None,
            discovery: None,
            enemy_attack: None,
            choice: None,
```

(For `for_controller_with_optional_source`, the body delegates to the other two constructors, so no field inits there — leave it as-is.)

- [ ] **Step 4: Update the accessor/setter bodies (from Task 1) to read/write the grouped fields**

Replace the method bodies added in Task 1:

```rust
    #[must_use]
    pub fn failed_by(&self) -> Option<u8> {
        self.skill_test.map(|b| b.failed_by)
    }
    #[must_use]
    pub fn clue_discovery_count(&self) -> Option<u8> {
        self.discovery.map(|b| b.clue_discovery_count)
    }
    #[must_use]
    pub fn attacking_enemy(&self) -> Option<crate::state::EnemyId> {
        self.enemy_attack.map(|b| b.attacking_enemy)
    }
    #[must_use]
    pub fn chosen_investigator(&self) -> Option<crate::state::InvestigatorId> {
        self.choice.and_then(|c| c.investigator)
    }
    #[must_use]
    pub fn chosen_location(&self) -> Option<crate::state::LocationId> {
        self.choice.and_then(|c| c.location)
    }
    #[must_use]
    pub fn chosen_enemy(&self) -> Option<crate::state::EnemyId> {
        self.choice.and_then(|c| c.enemy)
    }
    #[must_use]
    pub fn chosen_option(&self) -> Option<crate::engine::OptionId> {
        self.choice.and_then(|c| c.option)
    }

    pub fn set_failed_by(&mut self, margin: u8) {
        self.skill_test = Some(SkillTestBinding { failed_by: margin });
    }
    pub fn set_clue_discovery_count(&mut self, count: u8) {
        self.discovery = Some(DiscoveryBinding {
            clue_discovery_count: count,
        });
    }
    pub fn set_attacking_enemy(&mut self, enemy: crate::state::EnemyId) {
        self.enemy_attack = Some(EnemyAttackBinding {
            attacking_enemy: enemy,
        });
    }
    pub fn set_chosen_investigator(&mut self, id: crate::state::InvestigatorId) {
        self.choice.get_or_insert_with(Default::default).investigator = Some(id);
    }
    pub fn set_chosen_location(&mut self, id: crate::state::LocationId) {
        self.choice.get_or_insert_with(Default::default).location = Some(id);
    }
    pub fn set_chosen_enemy(&mut self, id: crate::state::EnemyId) {
        self.choice.get_or_insert_with(Default::default).enemy = Some(id);
    }
    pub fn set_chosen_option(&mut self, opt: Option<crate::engine::OptionId>) {
        self.choice.get_or_insert_with(Default::default).option = opt;
    }
```

- [ ] **Step 5: Write the serde round-trip test (failing first)**

Add to the `#[cfg(test)]` mod in `crates/game-core/src/engine/evaluator.rs`:

```rust
#[test]
fn eval_context_round_trips_with_grouped_bindings() {
    let mut ctx = EvalContext::for_controller(InvestigatorId(1));
    ctx.set_failed_by(3);
    ctx.set_chosen_investigator(InvestigatorId(2));
    let json = serde_json::to_string(&ctx).expect("serialize");
    let back: EvalContext = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.failed_by(), Some(3));
    assert_eq!(back.chosen_investigator(), Some(InvestigatorId(2)));
    assert_eq!(back.attacking_enemy(), None);
    assert_eq!(back.chosen_option(), None);
}
```

- [ ] **Step 6: Run the new test + full suite**

Run: `cargo test -p game-core eval_context_round_trips_with_grouped_bindings -- --nocapture`
Expected: PASS.

Run: `RUSTFLAGS="-D warnings" cargo test --all --all-features`
Expected: PASS (accessors unchanged externally → cards/scenarios unaffected).

- [ ] **Step 7: Clippy + fmt + doc**

Run: `cargo clippy --all-targets --all-features -- -D warnings && cargo fmt --check && RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`
Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add -A
git commit -m "engine: group EvalContext window bindings + derive serde

Replace the flat Option fields with cohesive SkillTest/Discovery/EnemyAttack/
Choice binding sub-structs behind the Task-1 accessors, and derive
Serialize/Deserialize so frames can snapshot the whole context. Innermost-only
per kind (corpus-moot; documented). No external API change.

Refs #345.
Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 3: Snapshot the whole EvalContext on ChoiceFrame

`ChoiceFrame` stores `controller` + `source` and rebuilds the context on resume (`choice.rs:124`), which silently drops any window binding active at suspend (the latent cross-suspend binding-loss). Replace those two fields with one `context: EvalContext` snapshot.

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs:495-508` (`ChoiceFrame` fields)
- Modify: `crates/game-core/src/engine/dispatch/choice.rs:62-67` (build), `:124` (resume rebuild)
- Test: `crates/game-core/src/engine/dispatch/choice.rs` `#[cfg(test)]` (binding-survives-suspend test)

**Interfaces:**
- Consumes: `EvalContext: Serialize + Clone + Copy` (Task 2).
- Produces: `ChoiceFrame { decisions, offered, effect, context: EvalContext }` (replaces `controller` + `source`). `EvalContext` exposes `.controller` / `.source` directly, so any reader of the old fields reads `frame.context.controller` / `frame.context.source`.

- [ ] **Step 1: Write the failing test — a skill-test binding survives a choice suspend**

Add to `crates/game-core/src/engine/dispatch/choice.rs` `#[cfg(test)]` mod. This proves the binding is carried across the suspend on the frame (the latent-bug fix):

```rust
#[test]
fn choice_frame_snapshots_active_skill_test_binding() {
    use crate::engine::evaluator::EvalContext;
    use crate::state::{Continuation, InvestigatorId};

    // A context carrying an active on_fail margin when a choice suspends.
    let mut ctx = EvalContext::for_controller(InvestigatorId(1));
    ctx.set_failed_by(2);

    let mut cx = crate::test_support::fixtures::test_cx_minimal();
    let _ = suspend_for_choice(
        &mut cx,
        "pick",
        vec!["a".into(), "b".into()],
        Vec::new(),
        crate::dsl::Effect::Seq(vec![]),
        ctx,
    );

    let Some(Continuation::Choice(frame)) = cx.state.continuations.last() else {
        panic!("expected a Choice frame on the stack");
    };
    assert_eq!(
        frame.context.failed_by(),
        Some(2),
        "the active skill-test margin must be snapshotted onto the ChoiceFrame, \
         not dropped at suspend",
    );
}
```

If `test_support::fixtures::test_cx_minimal()` does not exist, use the crate's existing minimal-`Cx` constructor (grep `fn .*Cx` in `crates/game-core/src/test_support/`); the assertion on `frame.context` is the load-bearing part.

- [ ] **Step 2: Run it — expect a compile failure (field `context` missing)**

Run: `cargo test -p game-core choice_frame_snapshots_active_skill_test_binding`
Expected: FAIL to compile — `ChoiceFrame` has no field `context` (and `controller`/`source` still present).

- [ ] **Step 3: Replace `ChoiceFrame`'s `controller`+`source` with `context`**

In `crates/game-core/src/state/game_state.rs`, in `struct ChoiceFrame`, remove the `controller: InvestigatorId` and `source: Option<CardInstanceId>` fields and their doc-comments, and add:

```rust
    /// The [`EvalContext`](crate::engine::EvalContext) captured at suspend —
    /// durable identity (`controller`/`source`) **and** any active window
    /// bindings (e.g. an `on_fail` margin). Snapshotting the whole context (vs.
    /// re-storing ingredient tuples) means bindings survive the suspend; resume
    /// re-runs the effect with this exact context. (#345.)
    pub context: crate::engine::EvalContext,
```

(`EvalContext` is re-exported at `crate::engine::EvalContext` — see `engine/mod.rs`. If `game_state.rs` cannot reference it without a cycle, use the fully-qualified `crate::engine::evaluator::EvalContext` path that `Continuation` doc-links already use.)

- [ ] **Step 4: Update the `suspend_for_choice` builder**

In `crates/game-core/src/engine/dispatch/choice.rs`, the `Continuation::Choice(ChoiceFrame { … })` literal currently sets `controller: eval_ctx.controller, source: eval_ctx.source`. Replace those two lines with:

```rust
            context: eval_ctx,
```

(`eval_ctx` is already the `EvalContext` argument; it is `Copy`, so this moves a copy onto the frame.)

- [ ] **Step 5: Update the resume rebuild**

In `crates/game-core/src/engine/dispatch/choice.rs:124`, replace:

```rust
    let eval_ctx = EvalContext::for_controller_with_optional_source(frame.controller, frame.source);
```

with:

```rust
    let eval_ctx = frame.context;
```

(Remove the now-unused `EvalContext` import if clippy flags it.)

- [ ] **Step 6: Run the test — expect PASS**

Run: `cargo test -p game-core choice_frame_snapshots_active_skill_test_binding`
Expected: PASS.

- [ ] **Step 7: Full suite — confirm no choice/Crypt-Chill/Dynamite-Blast regressions**

Run: `RUSTFLAGS="-D warnings" cargo test --all --all-features`
Expected: PASS. (Crypt Chill 01167 and Dynamite Blast 01024 exercise the choice-suspend path; their tests must still pass unchanged.)

- [ ] **Step 8: Clippy + fmt + doc + wasm**

Run:
```bash
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
cargo build -p web --target wasm32-unknown-unknown
cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings
```
Expected: PASS.

- [ ] **Step 9: Commit**

```bash
git add -A
git commit -m "engine: snapshot whole EvalContext on ChoiceFrame

Replace ChoiceFrame's controller+source ingredient tuple with a full
EvalContext snapshot, so window bindings (e.g. an on_fail margin) survive a
choice suspend instead of being silently rebuilt away. Adds a regression test.

Refs #345.
Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Self-Review

**Spec coverage (spec §D):**
- "derive `Serialize`" → Task 2 Step 2. ✓
- "grouped optional bindings" → Task 2 Steps 1–2. ✓
- "frames snapshot the whole context" → Task 3 (ChoiceFrame; the only existing snapshot site — `InFlightSkillTest` folds in #348 per spec §A). ✓
- "fixes the latent cross-suspend binding-loss" → Task 3 Step 1 test. ✓
- "4 `chosen_*` collapse into one `ChoiceBinding`" → Task 2 Step 1. ✓
- "innermost-only documented, no TODO" → Task 2 Step 1 doc-comment. ✓
- Rejected alternatives / corpus-moot → recorded in the spec, not re-derived here. ✓

**Placeholder scan:** none — every code step shows complete code; mechanical migrations give one worked example + the exact `file:line` site list (DRY for ~20 identical renames). The one conditional ("if `test_cx_minimal` doesn't exist…") names the exact grep to resolve it.

**Type consistency:** accessor/setter names are identical in Task 1 (flat bodies) and Task 2 (grouped bodies); `ChoiceBinding` field names (`investigator`/`location`/`enemy`/`option`) match the `chosen_*` accessor bodies in Task 2 Step 4; `ChoiceFrame.context` (Task 3 Step 3) matches its reads in Task 3 Steps 1/5.

**Out of scope (deferred to later PRs, per spec):** token plumbing (#347, PR 2), frame migration + `in_flight_skill_test` fold (#348, PR 3), revelation disposal (#380, PR 4).
