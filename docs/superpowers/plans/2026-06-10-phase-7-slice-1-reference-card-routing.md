# Phase 7 Slice 1 B1 — reference_card field + symbol-token lookup plumbing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give every `ScenarioModule` a `reference_card` and a registry-backed accessor that fetches the active scenario's reference card, so Group C can hang the real symbol-token evaluation off it — without changing live symbol resolution yet.

**Architecture:** Add a plain-data `reference_card: &'static str` field to `ScenarioModule` (const-friendly, `Copy`-preserving, matching how card impls already expose `CODE: &str`). Add `scenario::active_reference_card(state) -> Option<&'static str>` that routes `scenario_id → scenario_registry::current() → module_for → reference_card`. The live skill-test path (`resolve_token(token, &state.token_modifiers)`) is **unchanged**; the dynamic reference-card evaluation (`01104` skull = −(Ghouls at your location)) lands in Group C when `01104` is implemented.

**Tech Stack:** Rust, `game-core` kernel crate, `scenarios` content crate, `server` integration tests.

---

## Scope & key decision

**Decision (resolved with user at plan time):** B1 ships the **field + lookup plumbing only**; symbol tokens keep resolving via the static `TokenModifiers` path. The reference-card *evaluation* is deferred to Group C. So this plan adds (a) the data field, threaded through every `ScenarioModule` literal, and (b) the accessor that Group C will call — the seam, with a unit test, but no change to skill-test outcomes.

**Decision (type — deviation from spec text):** The spec writes `reference_card: CardCode`, but `CardCode` wraps a `String` (`crates/game-core/src/state/card.rs:19`) — not `Copy`, not const-constructible — and `ScenarioModule` is `#[derive(Copy)]` built in `const MODULE` / `static FAKE_MODULE` literals. A `CardCode` field would break both. Use `reference_card: &'static str` instead: const-friendly, keeps `Copy`, and matches the `CODE: &str` convention every card impl already uses. Bridge to `CardCode`/`&str`-taking APIs at the call site in Group C (`cards::abilities_for` takes `&str` anyway).

**Empty string = "no reference card":** test fixtures and synthetic modules carry no symbol content, so they set `reference_card: ""`. `active_reference_card` returns the raw module value (possibly `""`); callers in Group C only evaluate for the real scenario whose `reference_card` is `"01104"`. Documented on both the field and the accessor.

## File map

- **Modify** `crates/game-core/src/scenario.rs` — add `reference_card: &'static str` field to `ScenarioModule` (+ doc); add `active_reference_card` fn + its `#[cfg(test)]` tests.
- **Modify** `crates/game-core/src/scenario_registry.rs` — `FAKE_MODULE` literal (in `#[cfg(test)]`) gains the field.
- **Modify** `crates/game-core/src/engine/mod.rs` — `STAMP_MODULE` literal (in `#[cfg(test)]`, ~line 3909) gains the field.
- **Modify** `crates/scenarios/src/test_fixtures/synthetic.rs` — `pub const MODULE` literal (~line 135) gains the field.
- **Modify** `crates/server/tests/common/mod.rs` — `TEST_MODULE` literal (~line 38) gains the field.
- **Modify** `crates/server/tests/game_session.rs` — `TEST_MODULE` literal (~line 29) gains the field.

The compiler enforces completeness: until every literal is updated, the workspace will not build. That is the verification for Task 1; Task 2 carries the behavioral test.

---

### Task 1: Add `reference_card` field to `ScenarioModule` and thread it through every literal

**Files:**
- Modify: `crates/game-core/src/scenario.rs:86-100`
- Modify: `crates/game-core/src/scenario_registry.rs` (`FAKE_MODULE`)
- Modify: `crates/game-core/src/engine/mod.rs` (`STAMP_MODULE`, ~3909)
- Modify: `crates/scenarios/src/test_fixtures/synthetic.rs` (`MODULE`, ~135)
- Modify: `crates/server/tests/common/mod.rs` (`TEST_MODULE`, ~38)
- Modify: `crates/server/tests/game_session.rs` (`TEST_MODULE`, ~29)

- [ ] **Step 1: Add the field to the struct definition**

In `crates/game-core/src/scenario.rs`, add the field to `ScenarioModule` (keep the existing `setup` / `apply_resolution` fields):

```rust
#[derive(Debug, Clone, Copy)]
pub struct ScenarioModule {
    /// `ArkhamDB` card code of this scenario's single reference card —
    /// the card whose chaos **symbol** abilities (skull / cultist /
    /// tablet / elder-thing) are printed on it (e.g. `"01104"` for The
    /// Gathering). Plain data: ownership of the symbol effect stays on
    /// the card, but access flows through the scenario module.
    ///
    /// `&'static str` (not [`CardCode`](crate::state::CardCode)) so the
    /// struct stays [`Copy`] and const-constructible in `static` /
    /// `const` module literals, matching the `CODE: &str` convention
    /// card impls already use. Empty string means the scenario has no
    /// reference card (test fixtures / synthetic modules).
    ///
    /// Slice 1 B1 only *routes* to this code (see
    /// [`active_reference_card`]); evaluating the symbol ability against
    /// board state lands in Group C with the `01104` impl.
    pub reference_card: &'static str,
    /// Build the scenario's initial [`GameState`]. Places locations,
    /// populates encounter / act / agenda decks, sets chaos-bag
    /// modifiers, etc.
    pub setup: fn() -> GameState,
    /// Apply the resolution's effects (XP, trauma, scenario-end cleanup).
    /// Called by [`apply`](crate::engine::apply) exactly once, when the
    /// engine observes `GameState.resolution` transition from `None` to
    /// `Some` during an apply. Receives the events buffer so changes are
    /// observable to clients.
    ///
    /// For the Phase-4 synthetic fixture this is a no-op. Phase 9 fills in
    /// real bodies once the campaign log lands.
    pub apply_resolution: fn(&Resolution, &mut GameState, &mut Vec<Event>),
}
```

- [ ] **Step 2: Run the build to confirm every literal now fails to compile**

Run: `cargo build -p game-core 2>&1 | grep -c "missing field \`reference_card\`"`
Expected: a non-zero count — the compiler lists each `ScenarioModule { … }` literal missing the field. This is the to-do list for Step 3. (Workspace literals in `scenarios` / `server` surface when those crates build in Step 4.)

- [ ] **Step 3: Update every `ScenarioModule` literal to set `reference_card`**

`crates/game-core/src/scenario_registry.rs` — `FAKE_MODULE`:

```rust
    static FAKE_MODULE: ScenarioModule = ScenarioModule {
        reference_card: "",
        setup: empty_state,
        apply_resolution: no_op_apply,
    };
```

`crates/game-core/src/engine/mod.rs` — `STAMP_MODULE` (~3909):

```rust
    static STAMP_MODULE: ScenarioModule = ScenarioModule {
        reference_card: "",
        setup: unused_setup,
        apply_resolution: stamp_apply,
    };
```

`crates/scenarios/src/test_fixtures/synthetic.rs` — `MODULE` (~135):

```rust
pub const MODULE: ScenarioModule = ScenarioModule {
    reference_card: "",
    setup,
    apply_resolution,
};
```

`crates/server/tests/common/mod.rs` — `TEST_MODULE` (~38):

```rust
static TEST_MODULE: ScenarioModule = ScenarioModule {
    reference_card: "",
    setup: test_setup,
    apply_resolution: noop_resolution,
};
```

`crates/server/tests/game_session.rs` — `TEST_MODULE` (~29):

```rust
static TEST_MODULE: ScenarioModule = ScenarioModule {
    reference_card: "",
    setup: test_setup,
    apply_resolution: noop_resolution,
};
```

- [ ] **Step 4: Build the whole workspace to confirm all literals are updated**

Run: `RUSTFLAGS="-D warnings" cargo build --all --all-features --tests`
Expected: clean build, no `missing field` errors anywhere (including the `server` test binaries).

- [ ] **Step 5: Commit**

```bash
git add crates/game-core/src/scenario.rs \
        crates/game-core/src/scenario_registry.rs \
        crates/game-core/src/engine/mod.rs \
        crates/scenarios/src/test_fixtures/synthetic.rs \
        crates/server/tests/common/mod.rs \
        crates/server/tests/game_session.rs
git commit -m "scenario: add reference_card field to ScenarioModule

Plain-data &'static str (const-friendly, keeps Copy) naming each
scenario's single chaos-symbol reference card. Threaded through every
ScenarioModule literal; empty for fixtures with no symbol content.
Live symbol resolution unchanged.

Part of #220."
```

---

### Task 2: Add `active_reference_card` accessor (the routing seam) with tests

**Files:**
- Modify: `crates/game-core/src/scenario.rs` (add fn + `#[cfg(test)]` module)
- Test: `crates/game-core/src/scenario.rs` (`#[cfg(test)] mod tests`)

The accessor is what Group C calls on a symbol token. It routes `state.scenario_id → scenario_registry::current() → module_for → reference_card`. Returns `None` when there's no active scenario id or no registry installed (mirrors `apply_with_scenario_registry`'s early-return shape at `engine/mod.rs:183-187`).

- [ ] **Step 1: Write the failing tests**

Add to the bottom of `crates/game-core/src/scenario.rs`. The tests install a registry via `scenario_registry`; to avoid colliding with the process-global `OnceLock`, drive the lookup through an explicitly-passed registry by testing a small private helper that takes the registry as a parameter, then have the public fn delegate to it using `scenario_registry::current()`.

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::{ScenarioModule, ScenarioRegistry};
    use crate::state::GameState;
    use crate::test_support::TestGame;

    fn dummy_setup() -> GameState {
        TestGame::new().build()
    }
    fn dummy_resolution(_: &Resolution, _: &mut GameState, _: &mut Vec<Event>) {}

    static GATHERING_MODULE: ScenarioModule = ScenarioModule {
        reference_card: "01104",
        setup: dummy_setup,
        apply_resolution: dummy_resolution,
    };

    fn module_for(id: &ScenarioId) -> Option<&'static ScenarioModule> {
        (id.as_str() == "the-gathering").then_some(&GATHERING_MODULE)
    }

    fn registry() -> ScenarioRegistry {
        ScenarioRegistry { module_for }
    }

    #[test]
    fn returns_reference_card_for_active_scenario() {
        let state = TestGame::new()
            .with_scenario_id(ScenarioId::new("the-gathering"))
            .build();
        assert_eq!(
            reference_card_with_registry(&state, Some(&registry())),
            Some("01104"),
        );
    }

    #[test]
    fn returns_none_when_no_scenario_id() {
        let state = TestGame::new().build();
        assert_eq!(
            reference_card_with_registry(&state, Some(&registry())),
            None,
        );
    }

    #[test]
    fn returns_none_when_no_registry_installed() {
        let state = TestGame::new()
            .with_scenario_id(ScenarioId::new("the-gathering"))
            .build();
        assert_eq!(reference_card_with_registry(&state, None), None);
    }

    #[test]
    fn returns_none_for_unknown_scenario() {
        let state = TestGame::new()
            .with_scenario_id(ScenarioId::new("nonexistent"))
            .build();
        assert_eq!(
            reference_card_with_registry(&state, Some(&registry())),
            None,
        );
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p game-core scenario::tests 2>&1 | head -20`
Expected: FAIL — `cannot find function \`reference_card_with_registry\``.

- [ ] **Step 3: Implement the accessor and its registry-parameterized helper**

Add to `crates/game-core/src/scenario.rs` (after the `ScenarioModule` impl/types, before the test module). Confirm `GameState` is already imported at the top (`use crate::state::GameState;`):

```rust
/// The active scenario's reference-card code, or `None`.
///
/// Routes `state.scenario_id` → the installed scenario registry →
/// `module_for` → [`ScenarioModule::reference_card`]. Returns `None`
/// when there is no active scenario, no registry is installed, or the
/// id is unknown — the same tolerant shape as
/// [`apply`](crate::engine::apply)'s resolution lookup.
///
/// The returned code may be the empty string for fixture/synthetic
/// modules with no symbol content; callers that evaluate symbol
/// abilities (Group C) treat `""` as "no reference card".
#[must_use]
pub fn active_reference_card(state: &GameState) -> Option<&'static str> {
    reference_card_with_registry(state, crate::scenario_registry::current())
}

/// Registry-parameterized core of [`active_reference_card`], split out so
/// tests can pass an explicit [`ScenarioRegistry`] instead of relying on
/// the process-global `OnceLock`.
fn reference_card_with_registry(
    state: &GameState,
    registry: Option<&ScenarioRegistry>,
) -> Option<&'static str> {
    let id = state.scenario_id.as_ref()?;
    let module = (registry?.module_for)(id)?;
    Some(module.reference_card)
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p game-core scenario::tests 2>&1 | tail -10`
Expected: PASS — all four tests green.

- [ ] **Step 5: Commit**

```bash
git add crates/game-core/src/scenario.rs
git commit -m "scenario: add active_reference_card accessor (symbol-routing seam)

Routes scenario_id -> registry -> module -> reference_card; tolerant
None on missing id/registry/unknown id. Group C's symbol-token
evaluation will call this; live resolution still unchanged.

Part of #220."
```

---

### Task 3: Full strict gauntlet + final commit

**Files:** none (verification only, unless a strict-flag failure surfaces a fix).

- [ ] **Step 1: Run the full CI gauntlet locally**

```bash
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
cargo build -p web --target wasm32-unknown-unknown
cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings
```

Expected: all green. The new `pub fn active_reference_card` has a doc comment (doc job); intra-doc links (`[\`CardCode\`]`, `[\`apply\`]`, `[\`ScenarioModule::reference_card\`]`) must resolve — fix any broken link the `doc` job reports.

- [ ] **Step 2: Push the branch and open the PR**

```bash
git push -u origin scenario/reference-card-routing
gh pr create --fill --label scenario,engine
```

PR body must include a design-decisions paragraph noting: (1) `&'static str` instead of the spec's `CardCode` (const/`Copy` constraint, matches `CODE: &str`); (2) live symbol resolution deliberately unchanged — evaluation deferred to Group C per the plan-time scope decision. End with `Closes #220.`

- [ ] **Step 3: Watch CI**

Run: `gh pr checks <PR#> --watch` (background)
Expected: all seven jobs green.

- [ ] **Step 4: Update the phase doc (final commit, only once CI is green)**

In `docs/phases/phase-7-the-gathering.md`, update the Slice 1 table: the Group B row now references the filed issues. Move `#220` to a completed-state marker (`✅ PR #<PR#>`) once merged per the standard procedure; add a **Decisions made** entry only if load-bearing for a future PR — candidate: *"Scenario reference card is `&'static str` on `ScenarioModule`, not `CardCode` (const/`Copy`); `scenario::active_reference_card` is the symbol-routing seam Group C evaluates against."* Apply the would-a-future-author-choose-differently test before including it.

---

## Self-Review

**Spec coverage (spec step 5 — "Symbol-token resolution"):**
- `ScenarioModule += reference_card` → Task 1. ✅ (type adjusted to `&'static str` with recorded rationale)
- "resolver asks the module for its reference card" → Task 2's `active_reference_card` provides exactly that lookup. ✅ The *evaluation* half ("evaluates that card's symbol ability") is explicitly deferred to Group C per the plan-time scope decision — not a gap, a scoped cut.
- "01104 board-count logic is a Rust impl" → out of scope for B1 (Group C). ✅

**Placeholder scan:** No TBD/TODO/"handle edge cases"; every step has concrete code or an exact command. ✅

**Type consistency:** `reference_card: &'static str` used identically in the struct def (Task 1), all five literals (Task 1), the test module's `GATHERING_MODULE` (Task 2), and the accessor return type (Task 2). Helper `reference_card_with_registry` referenced in tests (Step 1) matches its definition (Step 3). `active_reference_card` named consistently in fn, doc links, and phase-doc decision. ✅
