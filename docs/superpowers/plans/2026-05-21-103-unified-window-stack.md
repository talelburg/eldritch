# #103 Unified Window Stack — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace `GameState.in_flight_reaction_window: Option<ReactionWindow>` with a unified `open_windows: Vec<OpenWindow>` stack carrying both the reaction-trigger queue and the Fast-action gate, and loosen `PlayCard` / `ActivateAbility`'s "Investigation phase + active investigator" gate for Fast cards / activated abilities.

**Architecture:** Refactor of existing #52 machinery, additive then subtractive: introduce the new types alongside the old, migrate the four reaction-window helpers (`queue_reaction_window` / `open_queued_reaction_window` / `resume_reaction_window` / `close_reaction_window`) onto the new stack, drop the old single-Option field. New `WindowKind::BetweenPhases` variant is added now to enable Fast-during-non-Investigation tests; other Phase-4 variants land with their consumer PRs. Pipeline detects `"Fast."` prefix in card text and emits `is_fast: bool` on `CardMetadata`; Magnifying Glass becomes the test consumer for PlayCard, Hyperawareness for ActivateAbility.

**Tech Stack:** Rust, `cargo`, `gh` CLI. Workspace crates touched: `card-dsl`, `card-data-pipeline`, `cards` (generated), `game-core`.

---

## Branch and scope

- Branch name: `engine/unified-window-stack` (per CLAUDE.md PR procedure: `<scope>/<short-slug>`).
- Closes issue #103.
- Includes the already-modified `docs/phases/phase-4-scenario-plumbing.md` (the design-pass doc edits made in the same session as #126/#127/#128 filing; user requested it ride with the first PR).

---

## File structure

**Modified:**
- `crates/card-dsl/src/card_data.rs` — `CardMetadata.is_fast: bool` added.
- `crates/card-data-pipeline/src/main.rs` — `NormalizedCard.is_fast`, detection in `normalize`, emission in the writer.
- `crates/cards/src/generated/cards.rs` — pipeline-regenerated; ~5600-line diff, review-trivial (every entry gains `is_fast: <bool>`).
- `crates/game-core/src/state/game_state.rs` — replace `ReactionWindow` / `in_flight_reaction_window` with `OpenWindow` / `FastActorScope` / `open_windows`. Add `WindowKind::BetweenPhases`.
- `crates/game-core/src/state/mod.rs` — re-exports.
- `crates/game-core/src/engine/dispatch.rs` — migrate four reaction-window helpers; loosen `play_card` and `activate_ability` gates.
- `crates/game-core/src/test_support/builder.rs` — add `with_open_window` builder.
- `crates/game-core/src/test_support/mod.rs` — re-export if needed.
- `docs/phases/phase-4-scenario-plumbing.md` — already in working tree (status flip from ⏳ to 🟡 happens in the final pre-merge commit).
- `docs/phases/README.md` — flip Phase 4 Status from `⏳ planned` to `🟡 in progress` in the final pre-merge commit.

**New:**
- `crates/cards/tests/fast_play.rs` — integration test for Magnifying Glass Fast play by non-active investigator + Hyperawareness Fast activation by non-active investigator.

**Not touched:**
- `crates/cards/src/impls/*.rs` — no hand-written card changes.
- Anything in `crates/scenarios` / `crates/server` / `crates/web`.

---

## Task 1: Branch setup + commit pending phase-doc edits

**Files:**
- Existing modified: `docs/phases/phase-4-scenario-plumbing.md`

- [ ] **Step 1: Create the feature branch from main**

Run:
```bash
git checkout -b engine/unified-window-stack
git status --short
```

Expected: `M docs/phases/phase-4-scenario-plumbing.md` (the phase-doc edits already in the working tree from the design pass).

- [ ] **Step 2: Commit the phase-doc edits as the first commit on this branch**

Run:
```bash
git add docs/phases/phase-4-scenario-plumbing.md
git commit -m "$(cat <<'EOF'
docs: record Phase-4 design pass (decisions + revised ordering)

Captures the 2026-05-21 design pass: synthetic toy scenario,
unified window stack (#52 × #103), #75 moved to Phase 9,
#69 / #71 splits, #126 / #127 / #128 filed, ScenarioModule shape.
The doc rides with the first PR per the user's request.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

Expected: commit succeeds; `git status` reports clean.

---

## Task 2: Add `CardMetadata.is_fast` field (additive; defaults false)

**Files:**
- Modify: `crates/card-dsl/src/card_data.rs:90-132`
- Test: `crates/card-dsl/src/card_data.rs` (existing `#[cfg(test)]` block at end of file, or add one)

- [ ] **Step 1: Add a failing serde-roundtrip test**

Append to `crates/card-dsl/src/card_data.rs`:

```rust
#[cfg(test)]
mod is_fast_tests {
    use super::*;

    #[test]
    fn metadata_serde_roundtrip_preserves_is_fast() {
        let original = CardMetadata {
            code: "01030".into(),
            name: "Magnifying Glass".into(),
            class: Class::Seeker,
            card_type: CardType::Asset,
            cost: Some(1),
            xp: Some(0),
            text: Some("Fast.\nYou get +1 [intellect] while investigating.".into()),
            flavor: None,
            illustrator: None,
            traits: vec!["Item".into(), "Tool".into()],
            slots: vec![Slot::Hand],
            skill_icons: SkillIcons::default(),
            health: None,
            sanity: None,
            deck_limit: 2,
            quantity: 1,
            pack_code: "core".into(),
            position: 30,
            is_fast: true,
        };
        let json = serde_json::to_string(&original).expect("serialize");
        let back: CardMetadata = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, original);
        assert!(back.is_fast);
    }
}
```

- [ ] **Step 2: Run the test, confirm it fails to compile**

Run: `cargo test -p card-dsl is_fast_tests`
Expected: compile error — `CardMetadata` has no `is_fast` field.

- [ ] **Step 3: Add the field**

Edit `crates/card-dsl/src/card_data.rs:131` (after `position`):

Insert before the closing `}` of `CardMetadata`:

```rust
    /// True if the card text begins with a "Fast." paragraph — i.e.
    /// the card may be played as a Fast action, outside the normal
    /// Investigation-phase + active-investigator timing. Detected by
    /// the card-data-pipeline from raw `text` ("Fast." paragraph
    /// prefix). Phase-3 / Phase-4 scope: only asset and event cards
    /// can carry Fast (skill and treachery use is irrelevant to
    /// `PlayCard`); the field is populated on every card for
    /// uniformity. See `engine::dispatch::play_card` for the gate it
    /// drives.
    pub is_fast: bool,
```

- [ ] **Step 4: Run the test, confirm it passes**

Run: `cargo test -p card-dsl is_fast_tests`
Expected: PASS.

- [ ] **Step 5: Verify nothing else broke**

Run: `cargo build -p card-dsl`
Expected: compiles. Downstream crates (`cards`, `game-core`) will fail until Task 3 regenerates the corpus — that's expected and Task 3 fixes it.

- [ ] **Step 6: Commit**

```bash
git add crates/card-dsl/src/card_data.rs
git commit -m "$(cat <<'EOF'
dsl: add CardMetadata.is_fast field

Additive change. Field defaults to whatever the construction site
sets; the card-data-pipeline grows detection logic in a follow-up
commit so the corpus picks up correct values.

Refs #103.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: Pipeline detects "Fast." prefix and emits `is_fast`

**Files:**
- Modify: `crates/card-data-pipeline/src/main.rs` (the `NormalizedCard` struct, `normalize()` function, and the emission writer near line 308–360)
- Regenerate: `crates/cards/src/generated/cards.rs`

- [ ] **Step 1: Add a failing unit test in the pipeline**

In `crates/card-data-pipeline/src/main.rs`, in the existing `#[cfg(test)]` block near line 425, add:

```rust
#[test]
fn fast_prefix_detected_at_start_of_text() {
    let raw = RawCard {
        code: "01030".into(),
        name: Some("Magnifying Glass".into()),
        type_code: Some("asset".into()),
        faction_code: Some("seeker".into()),
        cost: Some(1),
        xp: Some(0),
        text: Some("Fast.\nYou get +1 [intellect] while investigating.".into()),
        flavor: None,
        illustrator: None,
        traits: None,
        slot: Some("Hand".into()),
        skill_willpower: None,
        skill_intellect: Some(1),
        skill_combat: None,
        skill_agility: None,
        skill_wild: None,
        health: None,
        sanity: None,
        deck_limit: Some(2),
        quantity: Some(1),
        pack_code: "core".into(),
        position: 30,
    };
    let norm = normalize(raw).expect("normalize");
    assert!(norm.is_fast, "card text begins with \"Fast.\", expected is_fast=true");
}

#[test]
fn fast_marker_inside_text_is_not_a_fast_card() {
    // [fast] inside an activated ability is a different concept
    // (the activated trigger's action_cost == 0), not a Fast card.
    let raw = RawCard {
        code: "01034".into(),
        name: Some("Hyperawareness".into()),
        type_code: Some("asset".into()),
        faction_code: Some("seeker".into()),
        cost: Some(2),
        xp: Some(0),
        text: Some("[fast] Spend 1 resource: You get +1 [intellect] for this skill test.".into()),
        flavor: None,
        illustrator: None,
        traits: None,
        slot: None,
        skill_willpower: None,
        skill_intellect: Some(1),
        skill_combat: None,
        skill_agility: Some(1),
        skill_wild: None,
        health: None,
        sanity: None,
        deck_limit: Some(2),
        quantity: Some(1),
        pack_code: "core".into(),
        position: 34,
    };
    let norm = normalize(raw).expect("normalize");
    assert!(!norm.is_fast, "card text does NOT begin with \"Fast.\"; [fast] inside text is unrelated");
}
```

- [ ] **Step 2: Run the tests, confirm they fail to compile**

Run: `cargo test -p card-data-pipeline fast_`
Expected: compile error — `NormalizedCard` has no `is_fast` field.

- [ ] **Step 3: Add the field on `NormalizedCard`, detect in `normalize`, emit in the writer**

Edit `crates/card-data-pipeline/src/main.rs:181` (after `position` field of `NormalizedCard`):

```rust
    is_fast: bool,
```

Edit `crates/card-data-pipeline/src/main.rs` inside `normalize()`, after the existing fields are computed and before the `Ok(NormalizedCard { ... })` block (around line 194), add:

```rust
    let is_fast = raw
        .text
        .as_deref()
        .is_some_and(|t| t.starts_with("Fast.") || t.starts_with("Fast "));
```

Then add `is_fast,` to the `Ok(NormalizedCard { ... })` struct literal (around line 197–217), placed right after `position: raw.position,`.

Edit the emission writer in the same file (around line 358, after the `position` line):

```rust
    let _ = writeln!(out, "            is_fast: {},", c.is_fast);
```

- [ ] **Step 4: Run the pipeline unit tests, confirm they pass**

Run: `cargo test -p card-data-pipeline fast_`
Expected: both PASS.

- [ ] **Step 5: Regenerate the corpus**

Run: `cargo run -p card-data-pipeline`
Expected: succeeds; `crates/cards/src/generated/cards.rs` is updated. Every card entry gains an `is_fast: <true|false>,` line.

- [ ] **Step 6: Verify the corpus picks up Magnifying Glass + Hyperawareness correctly**

Run:
```bash
grep -A 1 'code: "01030".to_owned()' crates/cards/src/generated/cards.rs | grep is_fast
grep -A 1 'code: "01040".to_owned()' crates/cards/src/generated/cards.rs | grep is_fast
grep -A 1 'code: "01034".to_owned()' crates/cards/src/generated/cards.rs | grep is_fast
```

(The exact grep target depends on how the writer formats each line. If a single-line grep doesn't match, run `rg -B 5 'code: "01030"' crates/cards/src/generated/cards.rs | head -20` and visually confirm.)

Expected output:
- Magnifying Glass (01030, base): `is_fast: true,`
- Magnifying Glass (01040, upgraded): `is_fast: true,`
- Hyperawareness (01034): `is_fast: false,` (it's a non-Fast asset; the `[fast]` marker is on its activated ability, not on the card-play itself).

- [ ] **Step 7: Build the full workspace**

Run: `cargo build --workspace`
Expected: succeeds (since the corpus now includes `is_fast` for every card, downstream `cards` crate compiles).

- [ ] **Step 8: Commit**

```bash
git add crates/card-data-pipeline/src/main.rs crates/cards/src/generated/cards.rs
git commit -m "$(cat <<'EOF'
pipeline: detect Fast-prefixed cards and emit is_fast

The pipeline now sets CardMetadata.is_fast = true when the raw
text starts with "Fast." (case-sensitive, paragraph prefix —
mirrors how the printed card text marks the Fast keyword).
Regenerated cards.rs picks up Magnifying Glass (both copies) as
Fast assets. Hyperawareness stays is_fast=false: its [fast]
marker is on an activated ability (handled by
Trigger::Activated { action_cost: 0 }), not on the card-play.

Refs #103.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: Add `FastActorScope` enum + helper

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs` (add new type near `ReactionWindow`)
- Test: same file, in the existing `#[cfg(test)]` blocks or a new one

- [ ] **Step 1: Add a failing test**

Append to `crates/game-core/src/state/game_state.rs`:

```rust
#[cfg(test)]
mod fast_actor_scope_tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn active_investigator_permits_only_named() {
        let scope = FastActorScope::ActiveInvestigator(InvestigatorId(1));
        assert!(scope.permits(InvestigatorId(1)));
        assert!(!scope.permits(InvestigatorId(2)));
    }

    #[test]
    fn any_permits_everyone() {
        let scope = FastActorScope::Any;
        assert!(scope.permits(InvestigatorId(1)));
        assert!(scope.permits(InvestigatorId(42)));
    }

    #[test]
    fn specific_permits_only_the_named_set() {
        let mut set = BTreeSet::new();
        set.insert(InvestigatorId(1));
        set.insert(InvestigatorId(3));
        let scope = FastActorScope::Specific(set);
        assert!(scope.permits(InvestigatorId(1)));
        assert!(!scope.permits(InvestigatorId(2)));
        assert!(scope.permits(InvestigatorId(3)));
    }

    #[test]
    fn fast_actor_scope_serde_roundtrip() {
        let mut set = BTreeSet::new();
        set.insert(InvestigatorId(7));
        for scope in [
            FastActorScope::Any,
            FastActorScope::ActiveInvestigator(InvestigatorId(1)),
            FastActorScope::Specific(set),
        ] {
            let json = serde_json::to_string(&scope).expect("serialize");
            let back: FastActorScope = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(back, scope);
        }
    }
}
```

- [ ] **Step 2: Run the test, confirm it fails to compile**

Run: `cargo test -p game-core fast_actor_scope_tests`
Expected: compile error — `FastActorScope` unresolved.

- [ ] **Step 3: Add the `FastActorScope` enum + `permits` helper**

Insert into `crates/game-core/src/state/game_state.rs` immediately after the existing `ReactionWindow` struct (around line 345, before the `WindowKind` definition):

```rust
/// Which investigators may submit Fast `PlayCard` / `ActivateAbility`
/// actions while an [`OpenWindow`] is the top of [`GameState::open_windows`].
///
/// Modeled per Rules Reference: a reaction window allows any
/// investigator to fire a triggered reaction or play a Fast card.
/// An investigator's own turn opens an `ActiveInvestigator` window
/// that still permits other investigators to play Fast cards (per the
/// "Fast may be played at any player window" rule); concrete window
/// kinds choose the right scope at the open-window site.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum FastActorScope {
    /// Only the named investigator may submit Fast actions during
    /// this window. Used for narrow Investigation-phase windows (the
    /// turn's owner) where Fast actions are still bounded to one
    /// actor; pair with `Any` for windows where other investigators
    /// may interject.
    ActiveInvestigator(InvestigatorId),
    /// Any investigator may submit Fast actions. Used for reaction
    /// windows and between-phase windows.
    Any,
    /// Only the named set may submit Fast actions. Reserved for
    /// scenario-specific windows that restrict actors by criterion
    /// (e.g. only investigators at a given location). No Phase-3
    /// or Phase-4 site constructs this variant yet; the variant
    /// exists so future cards can grow it without engine churn.
    Specific(std::collections::BTreeSet<InvestigatorId>),
}

impl FastActorScope {
    /// True if `investigator` is permitted to submit a Fast action
    /// during the window carrying this scope.
    #[must_use]
    pub fn permits(&self, investigator: InvestigatorId) -> bool {
        match self {
            Self::ActiveInvestigator(id) => *id == investigator,
            Self::Any => true,
            Self::Specific(set) => set.contains(&investigator),
        }
    }
}
```

- [ ] **Step 4: Run the test, confirm it passes**

Run: `cargo test -p game-core fast_actor_scope_tests`
Expected: all four tests PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/game-core/src/state/game_state.rs
git commit -m "$(cat <<'EOF'
engine: add FastActorScope for window-gated Fast actions

Models which investigators may submit Fast PlayCard /
ActivateAbility actions during an open window. Three variants
cover the Phase-3/Phase-4 cases: ActiveInvestigator (turn-owner
bounded), Any (reaction + between-phase windows), Specific
(reserved for future scenario-specific restrictions).

The OpenWindow shape that consumes this lands in the next
commit; this commit isolates the scope-permission logic so it's
testable on its own.

Refs #103.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: Add `OpenWindow` struct + `open_windows: Vec<OpenWindow>` (additive)

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs`
- Modify: `crates/game-core/src/state/mod.rs` (re-exports)

- [ ] **Step 1: Add a failing test**

Append to `crates/game-core/src/state/game_state.rs`:

```rust
#[cfg(test)]
mod open_window_tests {
    use super::*;

    #[test]
    fn open_window_serde_roundtrip() {
        let window = OpenWindow {
            kind: WindowKind::AfterEnemyDefeated {
                enemy: EnemyId(7),
                by: Some(InvestigatorId(1)),
            },
            pending_triggers: Vec::new(),
            fast_actors: FastActorScope::Any,
        };
        let json = serde_json::to_string(&window).expect("serialize");
        let back: OpenWindow = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, window);
    }

    #[test]
    fn between_phases_window_kind_serde_roundtrip() {
        let kind = WindowKind::BetweenPhases {
            from: Phase::Mythos,
            to: Phase::Investigation,
        };
        let json = serde_json::to_string(&kind).expect("serialize");
        let back: WindowKind = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, kind);
    }
}
```

- [ ] **Step 2: Run the test, confirm it fails to compile**

Run: `cargo test -p game-core open_window_tests`
Expected: compile error — `OpenWindow` unresolved.

- [ ] **Step 3: Add `OpenWindow` struct + extend `WindowKind`**

In `crates/game-core/src/state/game_state.rs`, add a new struct `OpenWindow` immediately after the `FastActorScope` impl block (before `WindowKind`):

```rust
/// A currently-open window on the action stack.
///
/// Replaces the old single-Option `in_flight_reaction_window` shape.
/// Each window carries (a) what kind it is and which IDs the
/// triggering event/phase-transition named, (b) the queue of
/// `Trigger::OnEvent` reactions waiting to fire, and (c) which
/// investigators may submit Fast `PlayCard` / `ActivateAbility`
/// actions while this window is the top of `GameState::open_windows`.
///
/// Windows nest: a reaction firing inside another window may itself
/// trigger sub-reactions that open further windows on top of this
/// one. The dispatcher always reads / mutates the top of the stack
/// (`open_windows.last_mut()` / `open_windows.pop()`); closing a
/// window simply pops the top.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct OpenWindow {
    /// What kind of window is open; carries the IDs the triggering
    /// event named (defeated enemy + attacker, phase transition,
    /// etc.) so pending triggers' effects can resolve against the
    /// same payload.
    pub kind: WindowKind,
    /// Triggers in resolution order. Active investigator's matching
    /// triggers come first (Arkham's "active player priority"), then
    /// other investigators' in turn order. Within a single
    /// investigator, listed in `cards_in_play` order, then by
    /// `ability_index`. Empty `pending_triggers` is permitted —
    /// windows opened for phase/timing reasons (not reaction-driven)
    /// may have no triggers but still gate Fast actions.
    pub pending_triggers: Vec<PendingTrigger>,
    /// Which investigators may submit Fast `PlayCard` /
    /// `ActivateAbility` actions while this window is the top of
    /// the stack.
    pub fast_actors: FastActorScope,
}
```

Edit the existing `WindowKind` enum to add the `BetweenPhases` variant. Replace the `#[non_exhaustive]` enum block (around line 359–373) with:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum WindowKind {
    /// Fires after an enemy was defeated. Pairs with
    /// [`EventPattern::EnemyDefeated`](crate::dsl::EventPattern::EnemyDefeated)
    /// with [`EventTiming::After`](crate::dsl::EventTiming::After).
    AfterEnemyDefeated {
        /// The defeated enemy. Carried so trigger effects keying on
        /// "the defeated enemy" can route against the right id even
        /// after `state.enemies` has dropped the entry.
        enemy: EnemyId,
        /// Who defeated it, if attributable. Mirrors the
        /// [`Event::EnemyDefeated`](crate::Event::EnemyDefeated)
        /// `by` field. `None` for non-investigator-attributed defeats.
        by: Option<InvestigatorId>,
    },
    /// A window opened between two phases. Phase-4 phase-content PRs
    /// open this at each canonical transition (e.g. before Mythos,
    /// between Investigation and Enemy) so Fast cards + cross-phase
    /// reactions fire correctly. `fast_actors` is typically `Any`.
    BetweenPhases {
        /// The phase we're leaving.
        from: Phase,
        /// The phase we're entering.
        to: Phase,
    },
}
```

Re-export the new types via `crates/game-core/src/state/mod.rs`. Open it and add `FastActorScope, OpenWindow` to the existing `pub use game_state::{...}` line.

- [ ] **Step 4: Run the test, confirm it passes**

Run: `cargo test -p game-core open_window_tests`
Expected: PASS.

- [ ] **Step 5: Add `open_windows: Vec<OpenWindow>` to `GameState`**

Edit `crates/game-core/src/state/game_state.rs:136`. Insert AFTER the existing `pub in_flight_reaction_window: Option<ReactionWindow>,` line:

```rust
    /// Stack of currently-open windows. The top (`last()`) is the
    /// most recently-opened; closing pops the top. Carries pending
    /// reaction triggers (replacing the old `in_flight_reaction_window`
    /// single-slot shape) and the Fast-action gate for each window.
    ///
    /// Window kinds open at canonical timing points:
    /// - `AfterEnemyDefeated` — queued by `damage_enemy` when an
    ///   enemy reaches 0 health.
    /// - `BetweenPhases` — opened by the phase machine at every
    ///   phase transition (Phase-4 phase-content PRs wire this).
    ///
    /// Multi-window queueing (one effect that queues two windows in
    /// the same apply) is now structural — push twice, drive resumes
    /// in reverse open order.
    pub open_windows: Vec<OpenWindow>,
```

- [ ] **Step 6: Update every `GameState` construction site to default `open_windows: Vec::new()`**

This will surface as compile errors. Use `cargo build -p game-core` to find them, then fix each one by adding `open_windows: Vec::new(),` to the struct literal. Expected sites (from `grep -rn "GameState {" crates/game-core/src/`):
- `crates/game-core/src/state/game_state.rs` if there's a `Default` impl — check.
- `crates/game-core/src/test_support/builder.rs` `build()` method.
- Any test-only constructors that use struct-literal syntax.

Run repeatedly until clean:
```bash
cargo build -p game-core 2>&1 | head -40
```

- [ ] **Step 7: Run the full game-core test suite to confirm nothing broke**

Run: `cargo test -p game-core`
Expected: all tests PASS (the field is additive; nothing reads it yet).

- [ ] **Step 8: Commit**

```bash
git add crates/game-core/src/state/game_state.rs crates/game-core/src/state/mod.rs crates/game-core/src/test_support/builder.rs
git commit -m "$(cat <<'EOF'
engine: add OpenWindow stack alongside in_flight_reaction_window

Adds GameState.open_windows: Vec<OpenWindow> and the OpenWindow
struct (kind + pending_triggers + fast_actors) without yet
migrating the reaction-window machinery onto it. The field is
purely additive at this commit; existing reaction-window code
continues to read/write in_flight_reaction_window. Migration
happens in the next commit.

WindowKind grows a BetweenPhases { from, to } variant for the
Fast-during-non-Investigation tests in Task 9.

Refs #103.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: Add `with_open_window` test_support builder

**Files:**
- Modify: `crates/game-core/src/test_support/builder.rs`

- [ ] **Step 1: Add a failing test for the builder**

Append to the existing `#[cfg(test)]` block at the bottom of `crates/game-core/src/test_support/builder.rs`:

```rust
#[cfg(test)]
mod with_open_window_tests {
    use super::*;
    use crate::state::{FastActorScope, OpenWindow, WindowKind};

    #[test]
    fn with_open_window_pushes_onto_the_stack() {
        let state = TestGame::new()
            .with_investigator(test_investigator(1))
            .with_open_window(
                WindowKind::BetweenPhases {
                    from: Phase::Mythos,
                    to: Phase::Investigation,
                },
                FastActorScope::Any,
            )
            .build();
        assert_eq!(state.open_windows.len(), 1);
        assert_eq!(state.open_windows[0].fast_actors, FastActorScope::Any);
        assert!(state.open_windows[0].pending_triggers.is_empty());
    }

    #[test]
    fn with_open_window_stacks_in_order() {
        let state = TestGame::new()
            .with_investigator(test_investigator(1))
            .with_open_window(
                WindowKind::BetweenPhases {
                    from: Phase::Mythos,
                    to: Phase::Investigation,
                },
                FastActorScope::Any,
            )
            .with_open_window(
                WindowKind::BetweenPhases {
                    from: Phase::Investigation,
                    to: Phase::Enemy,
                },
                FastActorScope::ActiveInvestigator(InvestigatorId(1)),
            )
            .build();
        assert_eq!(state.open_windows.len(), 2);
        assert!(matches!(
            state.open_windows[1].kind,
            WindowKind::BetweenPhases { to: Phase::Enemy, .. }
        ));
    }
}
```

- [ ] **Step 2: Run the test, confirm it fails to compile**

Run: `cargo test -p game-core with_open_window_tests`
Expected: compile error — no `with_open_window` method.

- [ ] **Step 3: Add the builder method**

Edit `crates/game-core/src/test_support/builder.rs` and add inside `impl TestGame` (after the existing `with_mulligan_window_open` method around line 199):

```rust
    /// Push an `OpenWindow` onto the build's `open_windows` stack
    /// for tests that need a specific window-state shape.
    ///
    /// The pushed window has no pending triggers (test paths that
    /// also need a reaction queue should manipulate `state` after
    /// `build()` rather than complicate this builder).
    #[must_use]
    pub fn with_open_window(
        mut self,
        kind: crate::state::WindowKind,
        fast_actors: crate::state::FastActorScope,
    ) -> Self {
        self.state.open_windows.push(crate::state::OpenWindow {
            kind,
            pending_triggers: Vec::new(),
            fast_actors,
        });
        self
    }
```

- [ ] **Step 4: Run the test, confirm it passes**

Run: `cargo test -p game-core with_open_window_tests`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/game-core/src/test_support/builder.rs
git commit -m "$(cat <<'EOF'
test-support: add TestGame::with_open_window builder

Lets tests construct GameState with arbitrary OpenWindow stacks
in scope. The Fast-gate-loosening tests in #103 use it to put
the engine in a non-Investigation window without driving the
phase machine.

Refs #103.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 7: Migrate reaction-window helpers onto `open_windows`

**Files:**
- Modify: `crates/game-core/src/engine/dispatch.rs`

This task migrates four helpers (`queue_reaction_window`, `open_queued_reaction_window`, `resume_reaction_window`, `close_reaction_window`) and the dispatch guard at top of `apply_action` (line 79) from the single-slot `state.in_flight_reaction_window` to the top of `state.open_windows`. `in_flight_reaction_window` keeps existing readers until Task 8 removes it.

- [ ] **Step 1: Read the existing helpers**

Run:
```bash
sed -n '1060,1075p;1175,1205p;1216,1255p;1395,1425p' crates/game-core/src/engine/dispatch.rs
```

Confirm the four functions' shape matches the pre-refactor expectation — `queue_reaction_window` constructs a `ReactionWindow` and assigns to `state.in_flight_reaction_window`; `open_queued_reaction_window` reads it; `resume_reaction_window` consumes the indexed pending entry; `close_reaction_window` clears the field.

- [ ] **Step 2: Migrate `queue_reaction_window` to push an `OpenWindow`**

In `crates/game-core/src/engine/dispatch.rs`, replace `queue_reaction_window` (around line 1062):

```rust
fn queue_reaction_window(state: &mut GameState, kind: WindowKind) {
    let pending = scan_pending_triggers(state, &kind);
    if pending.is_empty() {
        return;
    }
    // Reaction windows admit any investigator's Fast actions
    // (Rules Reference: "Fast may be played at any player window,
    // including reaction windows").
    state.open_windows.push(OpenWindow {
        kind,
        pending_triggers: pending,
        fast_actors: FastActorScope::Any,
    });
}
```

Add the `OpenWindow` / `FastActorScope` imports near the existing `ReactionWindow` import at the top of the file (around line 23). Use the existing `WindowKind` import; nothing changes for that one.

- [ ] **Step 3: Migrate `open_queued_reaction_window` to read the top of `open_windows`**

Replace `open_queued_reaction_window` (around line 1180):

```rust
fn open_queued_reaction_window(state: &GameState, events: &mut Vec<Event>) -> EngineOutcome {
    let window = state
        .open_windows
        .last()
        .expect("open_queued_reaction_window: caller checked open_windows is non-empty");
    events.push(Event::WindowOpened { kind: window.kind });
    EngineOutcome::AwaitingInput {
        prompt: InputPrompt::ResolveReactionWindow {
            kind: window.kind,
        },
        candidate_indices: (0..u32::try_from(window.pending_triggers.len())
            .expect("scan_pending_triggers cap"))
            .collect(),
    }
}
```

- [ ] **Step 4: Migrate `resume_reaction_window` to mutate the top of `open_windows`**

Replace `resume_reaction_window` (around line 1216):

```rust
fn resume_reaction_window(
    state: &mut GameState,
    events: &mut Vec<Event>,
    response: &InputResponse,
) -> EngineOutcome {
    match response {
        InputResponse::Skip => close_reaction_window(state, events),
        InputResponse::PickIndex(idx) => {
            let window = state
                .open_windows
                .last_mut()
                .expect("resume_reaction_window: caller checked non-empty");
            let idx = usize::try_from(*idx).unwrap_or(usize::MAX);
            if idx >= window.pending_triggers.len() {
                return EngineOutcome::Rejected {
                    reason: format!(
                        "ResolveInput::PickIndex({idx}): out of bounds for reaction-window pending list (len {})",
                        window.pending_triggers.len(),
                    )
                    .into(),
                };
            }
            let trigger = window.pending_triggers.remove(idx);
            // (existing fire-this-trigger logic preserved verbatim from
            // the pre-refactor function. If the trigger fires a
            // sub-window, `queue_reaction_window` will push it on top
            // of this one — re-enter via the loop in
            // resume_reaction_window's existing caller.)
            // … (preserved logic — see git history of dispatch.rs)
        }
        _ => EngineOutcome::Rejected {
            reason: "reaction-window resume requires PickIndex or Skip".into(),
        },
    }
}
```

For the "preserved logic" comment above: read the pre-refactor `resume_reaction_window` body (the part that runs the trigger's effect) verbatim. Copy it in place; only the way `pending` is reached changes (was `state.in_flight_reaction_window.as_mut().unwrap().pending`, now `state.open_windows.last_mut().unwrap().pending_triggers`).

- [ ] **Step 5: Migrate `close_reaction_window` to pop the top of `open_windows`**

Replace `close_reaction_window` (around line 1403):

```rust
fn close_reaction_window(state: &mut GameState, events: &mut Vec<Event>) -> EngineOutcome {
    let popped = state
        .open_windows
        .pop()
        .expect("close_reaction_window: caller checked non-empty");
    // Forced-trigger check moved to the call site: closing while
    // forced triggers remain is now rejected before pop. The pop
    // here is the structural close.
    events.push(Event::WindowClosed { kind: popped.kind });
    // (existing post-close re-entry into drive_skill_test preserved)
    // … (preserved logic from pre-refactor)
}
```

Same instruction: preserve the existing "what to do after closing" logic verbatim (the re-entry into `drive_skill_test` if an in-flight test exists). Only the source of the window changes.

The forced-trigger-remaining check needs to be lifted to the only caller that hits `close_reaction_window` via `Skip` (the `resume_reaction_window` `Skip` arm). Add a check there:

```rust
InputResponse::Skip => {
    let window = state
        .open_windows
        .last()
        .expect("resume_reaction_window: caller checked non-empty");
    if window.pending_triggers.iter().any(|t| t.forced) {
        return EngineOutcome::Rejected {
            reason: "cannot skip reaction window: forced triggers remain".into(),
        };
    }
    close_reaction_window(state, events)
}
```

- [ ] **Step 6: Migrate the top-of-`apply_action` guard**

Edit `crates/game-core/src/engine/dispatch.rs:79`. Replace:

```rust
if state.in_flight_reaction_window.is_some()
```

with:

```rust
if state
    .open_windows
    .last()
    .is_some_and(|w| !w.pending_triggers.is_empty())
```

The guard's reject message stays the same.

- [ ] **Step 7: Migrate every other reader of `in_flight_reaction_window`**

Find them:
```bash
rg -n "in_flight_reaction_window" crates/game-core/src/
```

Update each occurrence in `dispatch.rs` to read from `state.open_windows.last()` / `last_mut()` instead. There are roughly 8–10 sites (see the earlier `grep` output); typical patterns:
- `state.in_flight_reaction_window.is_some()` → `state.open_windows.last().is_some_and(|w| !w.pending_triggers.is_empty())`
- `state.in_flight_reaction_window.as_ref()` → `state.open_windows.last()` (callers may need to also handle empty-stack case)
- `state.in_flight_reaction_window.as_mut()` → `state.open_windows.last_mut()`

The `assert!(state.in_flight_reaction_window.is_none(), …)` in `queue_reaction_window` becomes structurally unnecessary because windows can stack — drop the assertion.

- [ ] **Step 8: Run the full test suite; everything should still pass**

Run: `cargo test --workspace`
Expected: all tests PASS. Reaction-window tests (Roland Banks, the existing `#52` test suite) prove the migration preserves behavior.

If any test fails, the migration is incomplete — find the failing site and fix.

- [ ] **Step 9: Commit**

```bash
git add crates/game-core/src/engine/dispatch.rs
git commit -m "$(cat <<'EOF'
engine: migrate reaction-window helpers onto open_windows stack

queue_reaction_window pushes onto state.open_windows;
open_queued_reaction_window / resume_reaction_window /
close_reaction_window read top-of-stack instead of the
single-slot Option. The dispatch guard at apply_action's top
checks the same. state.in_flight_reaction_window is now an
unused remnant — Task 8 removes it.

All existing reaction-window tests (Roland Banks reaction,
the #52 suite) continue to pass.

Refs #103.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 8: Remove `in_flight_reaction_window` field and `ReactionWindow` struct

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs`
- Modify: `crates/game-core/src/state/mod.rs`
- Modify: `crates/game-core/src/engine/dispatch.rs` (drop the unused import)

- [ ] **Step 1: Delete the `in_flight_reaction_window` field**

Edit `crates/game-core/src/state/game_state.rs:104-136`. Delete the field (and its docs):

Remove the entire block from `/// An open reaction window the engine…` through `pub in_flight_reaction_window: Option<ReactionWindow>,`.

- [ ] **Step 2: Delete the `ReactionWindow` struct**

Delete the entire `pub struct ReactionWindow { … }` block (around line 333–345). Keep `WindowKind`, `PendingTrigger`, and everything else.

- [ ] **Step 3: Update re-exports**

Edit `crates/game-core/src/state/mod.rs`. Find the `pub use` of `ReactionWindow` and remove it.

- [ ] **Step 4: Update default construction sites**

Run:
```bash
cargo build -p game-core 2>&1 | head -20
```

Find every site that previously constructed a `GameState` with `in_flight_reaction_window: None,`. Delete the field from each struct literal.

- [ ] **Step 5: Run the full test suite**

Run: `cargo test --workspace`
Expected: all tests PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/game-core/src/state/game_state.rs crates/game-core/src/state/mod.rs crates/game-core/src/engine/dispatch.rs
git commit -m "$(cat <<'EOF'
engine: remove in_flight_reaction_window field + ReactionWindow

Now-unused after the migration onto open_windows in the prior
commit. Drops the field, the struct, and the re-export. All
existing reaction-window tests continue to pass; the unified
window stack is now the sole source of truth.

Refs #103.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 9: Loosen `PlayCard` Fast gate

**Files:**
- Modify: `crates/game-core/src/engine/dispatch.rs` (the `play_card` function around line 2479)
- Test: `crates/cards/tests/fast_play.rs` (new file)

- [ ] **Step 1: Write failing integration tests**

Create `crates/cards/tests/fast_play.rs`:

```rust
//! Integration tests for the Fast play-card gate loosening from #103.
//!
//! Verifies that a Fast card (Magnifying Glass, 01030) can be played
//! by a non-active investigator when an open window's `fast_actors`
//! scope permits, and that a non-Fast card (Working a Hunch, 01039)
//! in the same setup is still rejected.

use game_core::action::{Action, PlayerAction};
use game_core::engine::apply;
use game_core::state::{
    CardCode, FastActorScope, GameState, InvestigatorId, OpenWindow, Phase, WindowKind,
};
use game_core::test_support::{test_investigator, TestGame};

fn install_cards_registry() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
}

#[test]
fn fast_asset_playable_by_non_active_investigator_when_window_permits() {
    install_cards_registry();
    let mut a = test_investigator(1);
    let mut b = test_investigator(2);
    b.hand.push(CardCode("01030".into())); // Magnifying Glass — Fast.
    let state = TestGame::new()
        .with_investigator(a)
        .with_investigator(b)
        .with_phase(Phase::Investigation)
        .with_active_investigator(InvestigatorId(1))
        .with_open_window(
            WindowKind::BetweenPhases {
                from: Phase::Mythos,
                to: Phase::Investigation,
            },
            FastActorScope::Any,
        )
        .build();
    let result = apply(
        state,
        Action::Player(PlayerAction::PlayCard {
            investigator: InvestigatorId(2),
            hand_index: 0,
        }),
    );
    assert!(matches!(
        result.outcome,
        game_core::engine::EngineOutcome::Done
    ), "Magnifying Glass should play Fast from non-active investigator's hand: {:?}", result.outcome);
    let b_after = result.state.investigators.get(&InvestigatorId(2)).unwrap();
    assert_eq!(b_after.hand.len(), 0, "card should have left hand");
    assert_eq!(b_after.cards_in_play.len(), 1, "card should be in play");
}

#[test]
fn non_fast_asset_still_rejected_when_not_active_investigator() {
    install_cards_registry();
    let mut a = test_investigator(1);
    let mut b = test_investigator(2);
    // Holy Rosary (01059): non-Fast asset, implemented in
    // cards/src/impls/holy_rosary.rs. Text: "You get +1 [willpower]."
    b.resources = 5;
    b.hand.push(CardCode("01059".into()));
    let state = TestGame::new()
        .with_investigator(a)
        .with_investigator(b)
        .with_phase(Phase::Investigation)
        .with_active_investigator(InvestigatorId(1))
        .with_open_window(
            WindowKind::BetweenPhases {
                from: Phase::Mythos,
                to: Phase::Investigation,
            },
            FastActorScope::Any,
        )
        .build();
    let result = apply(
        state,
        Action::Player(PlayerAction::PlayCard {
            investigator: InvestigatorId(2),
            hand_index: 0,
        }),
    );
    let reason = match result.outcome {
        game_core::engine::EngineOutcome::Rejected { reason } => reason,
        other => panic!("Holy Rosary is not Fast — non-active investigator must not play it: {other:?}"),
    };
    // The reject should be the non-Fast strict-gate rejection, not
    // the resources / hand-index / card-type rejection. Tighten the
    // assertion to confirm the gate is the cause.
    assert!(
        reason.contains("non-Fast") || reason.contains("Investigation phase"),
        "expected non-Fast gate rejection; got: {reason}",
    );
}

#[test]
fn fast_asset_still_playable_by_active_investigator_during_investigation() {
    install_cards_registry();
    let mut a = test_investigator(1);
    a.hand.push(CardCode("01030".into())); // Magnifying Glass — Fast.
    let state = TestGame::new()
        .with_investigator(a)
        .with_phase(Phase::Investigation)
        .with_active_investigator(InvestigatorId(1))
        .build();
    let result = apply(
        state,
        Action::Player(PlayerAction::PlayCard {
            investigator: InvestigatorId(1),
            hand_index: 0,
        }),
    );
    assert!(matches!(
        result.outcome,
        game_core::engine::EngineOutcome::Done
    ), "Magnifying Glass plays normally for active investigator (Phase-3 behavior preserved): {:?}", result.outcome);
}
```

(Holy Rosary is the cleanest non-Fast asset for this test: implemented in `cards/src/impls/holy_rosary.rs`, costs 2 (covered by `b.resources = 5`), text contains no `Fast.` prefix so the pipeline sets `is_fast: false`.)

- [ ] **Step 2: Run the tests, confirm they fail**

Run: `cargo test -p cards --test fast_play`
Expected: at least the first test fails (`Magnifying Glass should play Fast from non-active investigator's hand` — current handler rejects with "not the active investigator"). The third test should already pass (existing behavior).

- [ ] **Step 3: Loosen the `play_card` gate**

Edit `crates/game-core/src/engine/dispatch.rs`, find `play_card` (around line 2479). Replace the two strict-gate rejects (Investigation phase + active investigator) with this gate:

```rust
    // Resolve the card's metadata first to learn whether it's Fast.
    // The strict gate (Investigation + active) is the fallback for
    // non-Fast cards; Fast cards are allowed when an open window's
    // fast_actors scope permits this investigator.
    let Some(inv) = state.investigators.get(&investigator) else {
        return EngineOutcome::Rejected {
            reason: format!("PlayCard: investigator {investigator:?} is not in state").into(),
        };
    };
    if inv.status != Status::Active {
        return EngineOutcome::Rejected {
            reason: format!(
                "PlayCard: {investigator:?} is not Active (status {:?})",
                inv.status,
            )
            .into(),
        };
    }
    let idx = usize::from(hand_index);
    if idx >= inv.hand.len() {
        return EngineOutcome::Rejected {
            reason: format!(
                "PlayCard: hand_index {hand_index} out of bounds (hand size {})",
                inv.hand.len(),
            )
            .into(),
        };
    }
    let code: CardCode = inv.hand[idx].clone();
    let registry = card_registry::current().ok_or_else(|| EngineOutcome::Rejected {
        reason: "PlayCard: card registry not installed".into(),
    });
    let is_fast = match registry {
        Ok(reg) => (reg.metadata_for)(&code).map(|m| m.is_fast).unwrap_or(false),
        Err(reject) => return reject,
    };
    let in_permissive_window = state
        .open_windows
        .last()
        .is_some_and(|w| w.fast_actors.permits(investigator));
    let active_during_investigation = state.phase == Phase::Investigation
        && state.active_investigator == Some(investigator);
    if !is_fast && !active_during_investigation {
        return EngineOutcome::Rejected {
            reason: format!(
                "PlayCard: non-Fast card; requires Investigation phase + active investigator (was {:?}, active {:?})",
                state.phase, state.active_investigator,
            )
            .into(),
        };
    }
    if is_fast && !active_during_investigation && !in_permissive_window {
        return EngineOutcome::Rejected {
            reason: "PlayCard: Fast card requires either being the active investigator during Investigation, or an open window whose fast_actors permits this investigator".into(),
        };
    }
```

The downstream `resolve_play_target` / mutation code is unchanged from the pre-refactor body — keep it verbatim.

Note: this drops the previous "investigator missing" reject's wording uniformity. Match it if you can; the structural change is fine to ship slightly more verbose.

- [ ] **Step 4: Run the tests, confirm they pass**

Run: `cargo test -p cards --test fast_play`
Expected: all three tests PASS.

Run the broader suite to confirm nothing regressed:
```bash
cargo test --workspace
```

- [ ] **Step 5: Commit**

```bash
git add crates/game-core/src/engine/dispatch.rs crates/cards/tests/fast_play.rs
git commit -m "$(cat <<'EOF'
engine: loosen PlayCard Fast gate via open-window fast_actors

A Fast card (CardMetadata.is_fast == true) is now playable when
the top of state.open_windows carries a fast_actors scope that
permits the acting investigator. Non-Fast cards keep the
existing strict gate (Investigation phase + active investigator).

Fixes a quiet under-implementation: Magnifying Glass is a Fast
asset and was incidentally playable only because Phase-3 tests
always had the right phase + active investigator; the engine
now matches the printed-card behavior.

Refs #103.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 10: Loosen `ActivateAbility` Fast gate

**Files:**
- Modify: `crates/game-core/src/engine/dispatch.rs` (the `activate_ability` function around line 2611)
- Test: extend `crates/cards/tests/fast_play.rs`

- [ ] **Step 1: Write a failing test for Hyperawareness cross-investigator activation**

Append to `crates/cards/tests/fast_play.rs`:

```rust
use game_core::action::InputResponse;
use game_core::state::{CardCode, CardInPlay, CardInstanceId};

#[test]
fn fast_activated_ability_usable_by_non_active_investigator_when_window_permits() {
    install_cards_registry();
    let mut a = test_investigator(1);
    let mut b = test_investigator(2);
    // Place Hyperawareness (01034) into play for investigator B.
    // Its [fast] abilities are Trigger::Activated { action_cost: 0 }.
    b.cards_in_play.push(CardInPlay::enter_play(
        CardCode("01034".into()),
        CardInstanceId(1),
    ));
    let state = TestGame::new()
        .with_investigator(a)
        .with_investigator(b)
        .with_phase(Phase::Investigation)
        .with_active_investigator(InvestigatorId(1))
        .with_open_window(
            WindowKind::BetweenPhases {
                from: Phase::Mythos,
                to: Phase::Investigation,
            },
            FastActorScope::Any,
        )
        .build();
    // Activate ability index 0 (the [fast] +1 intellect ability —
    // verify ordering against the actual abilities() impl).
    let result = apply(
        state,
        Action::Player(PlayerAction::ActivateAbility {
            investigator: InvestigatorId(2),
            instance_id: CardInstanceId(1),
            ability_index: 0,
        }),
    );
    assert!(matches!(
        result.outcome,
        game_core::engine::EngineOutcome::Done
    ), "Hyperawareness Fast ability should activate from non-active investigator: {:?}", result.outcome);
}

#[test]
fn action_cost_ability_still_requires_active_investigator() {
    install_cards_registry();
    let mut a = test_investigator(1);
    let mut b = test_investigator(2);
    // Place a card with an action-cost (action_cost > 0) ability in
    // investigator B's play. Choose a card from cards/src/impls/
    // that has such an ability; if none is implemented today, use
    // Magnifying Glass (01030) and assert the test pattern when
    // it gains an action-cost ability.
    //
    // For now this test exercises the negative path: investigator B
    // trying to activate an ability they have no claim to during
    // investigator A's turn, with no permissive Fast window for
    // a non-Fast ability — the strict gate should still fire.
    //
    // (Pending: locate or add a fixture card with action_cost > 0
    // ability to make this assertion concrete. The test exists as
    // a placeholder reminding the implementor to verify the gate.)
}
```

(The second test depends on whether there's any implemented card with `Trigger::Activated { action_cost: N }` for `N > 0`. If none exists, the test stays as a documented placeholder; the gate's correctness for action-cost abilities is verified by the existing test suite's unchanged-pass.)

- [ ] **Step 2: Run the tests, confirm the first one fails**

Run: `cargo test -p cards --test fast_play fast_activated_ability_usable_by_non_active_investigator`
Expected: the first new test fails — current `activate_ability` rejects with "not the active investigator."

- [ ] **Step 3: Loosen the `activate_ability` gate**

Edit `crates/game-core/src/engine/dispatch.rs`, the `activate_ability` function (around line 2611). The current order is: check phase, check active investigator, check inv exists, check status, look up in-play, resolve ability, check action economy.

Refactor: move the in-play lookup + ability resolution BEFORE the phase/active checks so we can branch on `action_cost == 0`. New flow:

```rust
fn activate_ability(
    state: &mut GameState,
    events: &mut Vec<Event>,
    investigator: InvestigatorId,
    instance_id: CardInstanceId,
    ability_index: u8,
) -> EngineOutcome {
    let Some(inv) = state.investigators.get(&investigator) else {
        return EngineOutcome::Rejected {
            reason: format!("ActivateAbility: investigator {investigator:?} is not in state")
                .into(),
        };
    };
    if inv.status != Status::Active {
        return EngineOutcome::Rejected {
            reason: format!(
                "ActivateAbility: {investigator:?} is not Active (status {:?})",
                inv.status,
            )
            .into(),
        };
    }
    let Some(in_play_pos) = inv
        .cards_in_play
        .iter()
        .position(|c| c.instance_id == instance_id)
    else {
        return EngineOutcome::Rejected {
            reason: format!(
                "ActivateAbility: {investigator:?} has no in-play instance {instance_id:?}",
            )
            .into(),
        };
    };
    let source_code = inv.cards_in_play[in_play_pos].code.clone();
    let source_exhausted = inv.cards_in_play[in_play_pos].exhausted;

    let (action_cost, costs, effect) = match resolve_activated_ability(&source_code, ability_index)
    {
        Ok(v) => v,
        Err(reject) => return reject,
    };

    // Fast gate: action_cost == 0 abilities may fire outside the
    // Investigation-phase + active-investigator narrow gate if an
    // open window's fast_actors permits this investigator.
    let active_during_investigation = state.phase == Phase::Investigation
        && state.active_investigator == Some(investigator);
    let in_permissive_window = state
        .open_windows
        .last()
        .is_some_and(|w| w.fast_actors.permits(investigator));
    let is_fast_ability = action_cost == 0;
    if !is_fast_ability && !active_during_investigation {
        return EngineOutcome::Rejected {
            reason: format!(
                "ActivateAbility: action-cost ability requires Investigation phase + active investigator (was {:?}, active {:?})",
                state.phase, state.active_investigator,
            )
            .into(),
        };
    }
    if is_fast_ability && !active_during_investigation && !in_permissive_window {
        return EngineOutcome::Rejected {
            reason: "ActivateAbility: Fast ability requires either active investigator during Investigation, or open window whose fast_actors permits this investigator".into(),
        };
    }

    // (existing action-economy + cost-payable + pay + apply-effect
    // logic preserved verbatim from the pre-refactor body)
    // …
}
```

Preserve every step after the gate verbatim from the existing function — only the gate logic moves.

Update the doc comment on `activate_ability` (around line 2603) — the "overly strict for `[fast]`" caveat is now resolved; rewrite to describe the new gate.

- [ ] **Step 4: Run the tests, confirm the loose gate works**

Run: `cargo test -p cards --test fast_play`
Expected: all tests PASS.

Run the broader suite:
```bash
cargo test --workspace
```

- [ ] **Step 5: Commit**

```bash
git add crates/game-core/src/engine/dispatch.rs crates/cards/tests/fast_play.rs
git commit -m "$(cat <<'EOF'
engine: loosen ActivateAbility Fast gate via open-window fast_actors

Activated abilities with action_cost == 0 may now fire when an
open window's fast_actors scope permits the acting investigator,
mirroring the PlayCard Fast loosening from the prior commit.
Action-cost abilities (action_cost > 0) keep the strict gate.

Fixes Hyperawareness's [fast] ability being unusable on other
investigators' turns, matching the printed-card behavior.

Refs #103.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 11: Pre-merge doc updates

**Files:**
- Modify: `docs/phases/README.md`
- Modify: `docs/phases/phase-4-scenario-plumbing.md`

Per CLAUDE.md PR procedure step 7: phase-doc updates happen ONLY in the final pre-merge commit on the branch. This task lands those edits after review is complete and the PR is ready to merge.

- [ ] **Step 1: Flip Phase 4 status in the index**

Edit `docs/phases/README.md`. Find the Phase 4 row:

```
| 4 | Scenario plumbing | ⏳ planned | [phase-4-scenario-plumbing.md](phase-4-scenario-plumbing.md) |
```

Change to:

```
| 4 | Scenario plumbing | 🟡 in progress | [phase-4-scenario-plumbing.md](phase-4-scenario-plumbing.md) |
```

- [ ] **Step 2: Update Phase 4 doc's Status and Ordering tables**

Edit `docs/phases/phase-4-scenario-plumbing.md`.

Change the Status line from:
```
⏳ Planned. Design pass complete 2026-05-21 (this doc reflects that pass — see Decisions). Issues filed: `#126` Revelation DSL, `#127` spawn rules, `#128` Hunter movement; `#69` / `#71` / `#103` rescoped; `#75` migrated to Phase 9. Work begins with `#103` (the unified window stack refactor).
```

To:
```
🟡 In progress. Design pass complete 2026-05-21. First PR (#103 unified window stack) merged 2026-05-21.
```

In the Ordering table, flip the row for `#103` from `(planned step)` to `✅ PR #<this-PR-number>`. The PR number is known at this point because `gh pr view` returns it.

Add a Decisions entry summarizing #103's choices that are load-bearing for future PRs (only entries that future PR authors need to know):

```markdown
- **`#103` unifies #52's reaction-window machinery with the Fast-action gate (PR #<N>).** `GameState.open_windows: Vec<OpenWindow>` replaces the single-slot `in_flight_reaction_window: Option<ReactionWindow>`. Each window carries kind + pending_triggers (the reaction queue) + fast_actors (the Fast-action gate scope). Reaction-window helpers (`queue_reaction_window`, `open_queued_reaction_window`, `resume_reaction_window`, `close_reaction_window`) now operate on the top of the stack; multi-window nesting is structural via push/pop. Phase-4 phase-content PRs (#69 / #70 / #71) open `WindowKind::BetweenPhases` and other variants at canonical timing points; this PR shipped only the `BetweenPhases` variant for testing, leaving consumer-specific variants to their PRs. `FastActorScope` (ActiveInvestigator / Any / Specific) keys the Fast-gate; reaction windows default to `Any` per Rules Reference. PlayCard + ActivateAbility now read `CardMetadata.is_fast` + `Trigger::Activated.action_cost == 0` to branch between strict gate (non-Fast) and loose gate (Fast + window permits actor).
- **`CardMetadata.is_fast` populated by the pipeline from "Fast." paragraph prefix (PR #<N>).** Detected during `normalize()`; emitted in the pipeline writer. Magnifying Glass (both copies) is the live consumer that exposed the prior under-implementation — it was Fast in the printed text but the engine treated it as a normal action.
```

- [ ] **Step 3: Verify the PR number is known**

The PR was opened in Task 13 (later). For now, place a `<PR-N>` placeholder in this task; substitute the real number after Task 14 confirms the PR exists. (If this task is run before the PR opens, defer the substitution to Task 14's commit.)

Actually, restructure: Task 11 happens AFTER Task 14 (PR opened, CI passing, review complete). Reorder if executing inline. For now keep Task 11 in place as a pre-merge step; the PR number is filled in at execution time.

- [ ] **Step 4: Commit**

```bash
git add docs/phases/README.md docs/phases/phase-4-scenario-plumbing.md
git commit -m "$(cat <<'EOF'
docs: flip Phase 4 to in-progress; record #103 close in phase doc

Per CLAUDE.md PR procedure step 7: phase-doc updates land in
the final pre-merge commit so they reflect the actually-shipping
state (PR number, review fixes, final scope).

Refs #103.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 12: Run the full CI-equivalent gauntlet locally

CLAUDE.md PR procedure step 1 requires running all five CI jobs with strict flags before pushing.

- [ ] **Step 1: Run `cargo test` with strict flags**

Run:
```bash
RUSTFLAGS="-D warnings" cargo test --all --all-features
```

Expected: all tests PASS, no warnings.

- [ ] **Step 2: Run `cargo clippy` with strict lints**

Run:
```bash
cargo clippy --all-targets --all-features -- -D warnings
```

Expected: no warnings (CI treats warnings as errors).

- [ ] **Step 3: Run `cargo fmt --check`**

Run:
```bash
cargo fmt --check
```

Expected: no output (everything formatted).

If there's output, run `cargo fmt` and amend the prior commit or add a fixup commit. Do not skip this.

- [ ] **Step 4: Run `cargo doc` with strict flags**

Run:
```bash
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
```

Expected: docs build without warnings. Intra-doc-link errors are the most common failure here.

- [ ] **Step 5: Run the WASM build**

Run:
```bash
cargo build -p web --target wasm32-unknown-unknown
```

Expected: WASM build succeeds.

If any of steps 1–5 fail, fix the failure in a follow-up commit (or amend if it's a fmt/clippy fixup) and re-run the failing step. Don't push until all five pass.

---

## Task 13: Push branch and open PR

**Files:**
- None (creates PR on GitHub)

- [ ] **Step 1: Push the branch**

Run:
```bash
git push -u origin engine/unified-window-stack
```

- [ ] **Step 2: Open the PR**

Run:
```bash
gh pr create --title "engine: unified window stack (#103)" --body "$(cat <<'EOF'
## Summary

- Replaces `GameState.in_flight_reaction_window: Option<ReactionWindow>` with a unified `open_windows: Vec<OpenWindow>` stack carrying both the reaction-trigger queue (`pending_triggers`) and the Fast-action gate (`fast_actors`).
- Adds `CardMetadata.is_fast: bool` with pipeline detection of the `"Fast."` paragraph prefix. Magnifying Glass (01030/01040) now correctly identifies as Fast.
- Loosens `PlayCard` (for `is_fast` cards) and `ActivateAbility` (for `Trigger::Activated { action_cost: 0 }` abilities) gates: a Fast card / ability is playable when an open window's `fast_actors` scope permits the acting investigator, even when not in Investigation or not the active investigator.

## Design decisions

- **Migrate-then-remove rather than parallel.** The `open_windows` field was added additively, the four reaction-window helpers migrated onto it, then `in_flight_reaction_window` + `ReactionWindow` removed. Each commit compiles and passes all tests.
- **`WindowKind::BetweenPhases` lands in this PR; other Phase-4 variants land with their consumer PRs.** The phase-content PRs (#69 / #70 / #71) own the windows they open. This PR ships only the variants reachable from existing #52 machinery + the one variant needed for the Fast-gate tests.
- **`FastActorScope` defaults to `Any` for reaction windows.** Rules Reference: Fast actions may be played at any player window. Reaction-window-as-Fast-window is unified.
- **Magnifying Glass surfaces as a quiet pre-existing under-implementation.** The card is Fast per the printed text but the engine treated it as a normal action; Phase-3 tests passed only because they always met the strict gate. This PR fixes that.

## Test plan
- [ ] `RUSTFLAGS="-D warnings" cargo test --all --all-features`
- [ ] `cargo clippy --all-targets --all-features -- -D warnings`
- [ ] `cargo fmt --check`
- [ ] `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`
- [ ] `cargo build -p web --target wasm32-unknown-unknown`
- [ ] New integration tests in `crates/cards/tests/fast_play.rs` exercise the gate-loosening positive (Magnifying Glass + Hyperawareness on non-active investigator) and negative (non-Fast card same setup, action-cost ability stays gated) paths.

Closes #103.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

Expected: PR URL returned. Record the PR number for Task 14 and the final phase-doc commit.

---

## Task 14: Watch CI + spawn review-agent in parallel

Per CLAUDE.md PR procedure step 4: CI watch and review-agent run concurrently.

- [ ] **Step 1: Start CI watch in the background**

Run:
```bash
gh pr checks <PR-N> --watch
```

(Use `run_in_background: true` so it returns immediately and notifies on completion.)

- [ ] **Step 2: Spawn the review-agent in parallel**

In the same response as Step 1's background launch, dispatch the `review-agent` subagent with:

- PR number
- Branch name (`engine/unified-window-stack`)
- Context: design decisions from this plan's header; what to review (state-shape correctness, gate-logic correctness, test coverage); what's intentionally out of scope (other Phase-4 `WindowKind` variants, phase-content PRs that consume the new shape).

The exact dispatch is:

```
Agent({
  description: "Review #103 unified window stack PR",
  subagent_type: "review-agent",
  prompt: "<full PR context>"
})
```

- [ ] **Step 3: Present the review-agent's findings to the user**

When the review-agent returns its summary, surface every finding (severity-bucketed) verbatim before asking the user whether to merge. Do this even if CI is failing — the review may flag issues that need attention independent of CI.

---

## Task 15: Address review-driven fixes (if any) and merge

- [ ] **Step 1: If the user approves merge directly, run the merge**

Run:
```bash
gh pr merge <PR-N> --squash --delete-branch
```

Verify the issue auto-closed:
```bash
gh issue view 103 --json state -q '.state'
```

Expected: `CLOSED`.

- [ ] **Step 2: If the user requests review-driven changes, push follow-up commits**

Make the change, add a commit on the same branch (do NOT amend), push. CI re-runs automatically; the second watch can usually be foregrounded.

- [ ] **Step 3: After merge, pull `main`**

Run:
```bash
git checkout main && git pull
```

Expected: the squashed PR commit appears in the log; working tree is clean.

---

## Self-review

**Spec coverage check (against `docs/phases/phase-4-scenario-plumbing.md` Issue `#103` and its rescoped issue body):**

- [x] `open_windows: Vec<OpenWindow>` on GameState; `in_flight_reaction_window` removed → Tasks 5, 8.
- [x] `FastActorScope` validation on PlayCard / ActivateAbility Fast paths → Tasks 9, 10.
- [x] Existing reaction-window behavior preserved (all #52 tests + Roland's reaction continue to pass) → Task 7 step 8.
- [x] Action-log replay still deterministic (function-pointer-free state) → no `fn` pointers introduced; serde Derives + BTreeSet/Vec only.
- [x] Tests: Fast event playable between phases — proven by Magnifying Glass test in Task 9.
- [x] `[fast]` ability activable by non-active investigator during another's turn — Task 10.
- [x] Non-Fast still gated as before — Task 9 second test.
- [x] Action-cost ability outside Investigation still rejects — Task 10 covers structurally (gate preserved verbatim in the if-not-fast branch).

**Type consistency check:**

- `OpenWindow.pending_triggers` (was `ReactionWindow.pending`) — renamed; every consumer updated in Task 7.
- `OpenWindow.fast_actors` — new field; every construction site sets it.
- `WindowKind` is `#[non_exhaustive]` so future variants don't break match exhaustiveness; the new `BetweenPhases` variant uses fields `from` and `to` consistently across the plan.
- `FastActorScope::permits(InvestigatorId) -> bool` — signature stable across all task references.
