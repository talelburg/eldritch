# CardMetadata → struct + CardKind enum: Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the flat `CardMetadata` struct with an identity core + a `CardKind` enum carrying type-specific data, and give investigators a real `Skills` type.

**Architecture:** Two commits. (1) Move `Skills`/`SkillKind` down into `card-dsl` (re-exported from `game_core::state`). (2) An *atomic* reshape — a struct field change can't compile in pieces — defining `CardKind`, rewriting the pipeline emitter, regenerating the corpus, and migrating the 3 stat-readers + the mock literals, all in one green commit.

**Tech Stack:** Rust workspace (`card-dsl` ← `game-core` ← `cards`/`scenarios`; `card-data-pipeline` generates `cards/src/generated/cards.rs`).

**Spec:** `docs/superpowers/specs/2026-06-11-cardmetadata-cardkind-remodel-design.md`
**Issue:** #254

---

## File Structure

- `crates/card-dsl/src/card_data.rs` — gains `Skills`/`SkillKind` (moved in); `CardMetadata` reshaped; `CardKind` defined; `card_type()`/`class()` accessors.
- `crates/card-dsl/src/lib.rs` — export `Skills`, `SkillKind`, `CardKind`, `Spawn`.
- `crates/game-core/src/state/investigator.rs` — `Skills`/`SkillKind` definitions removed.
- `crates/game-core/src/state/mod.rs` — re-export `Skills`/`SkillKind` from `card-dsl`.
- `crates/card-data-pipeline/src/main.rs` — `render_card` emits `kind: CardKind::…`; drop the three cosmetic fields.
- `crates/cards/src/generated/cards.rs` — regenerated (never hand-edited).
- Readers: `game-core/src/engine/dispatch/{phases.rs, encounter.rs, cards.rs}`.
- Mock literals: `game-core/src/card_registry.rs`, `scenarios/src/test_fixtures/synth_cards.rs`, `card-dsl/src/card_data.rs` (own tests), and `game-core/tests/{forced_triggers,activate_ability,reaction_windows,on_skill_test_resolution}.rs`, `cards/tests/reject_rollback.rs`.

---

## Task 1: Move `Skills` + `SkillKind` into `card-dsl`

Pure relocation — `card-dsl` is the pure-data layer and already owns `Stat`/`SkillTestKind`. Re-exports keep every `game_core::state::{Skills, SkillKind}` reference compiling. Behavior-preserving.

**Files:**
- Modify: `crates/card-dsl/src/card_data.rs` (add the types + a test)
- Modify: `crates/card-dsl/src/lib.rs` (export)
- Modify: `crates/game-core/src/state/investigator.rs` (remove defs)
- Modify: `crates/game-core/src/state/mod.rs` (re-export)

- [ ] **Step 1: Add `Skills` + `SkillKind` to `card-dsl`**

In `crates/card-dsl/src/card_data.rs`, add (these are the exact current definitions from `game-core/src/state/investigator.rs`, including the `value` method):

```rust
/// An investigator's four base skill values (Rules Reference "skills").
///
/// Deliberately NOT `#[non_exhaustive]`: the four skills are fixed by
/// FFG's rules. Pure data — lives in `card-dsl`; `game-core` re-exports
/// it at `game_core::state::Skills`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Skills {
    /// Used for tests against effects of the will / fear.
    pub willpower: i8,
    /// Used for investigate tests.
    pub intellect: i8,
    /// Used for fight tests.
    pub combat: i8,
    /// Used for evade tests.
    pub agility: i8,
}

impl Skills {
    /// Lookup the value for a given [`SkillKind`].
    #[must_use]
    pub fn value(&self, kind: SkillKind) -> i8 {
        match kind {
            SkillKind::Willpower => self.willpower,
            SkillKind::Intellect => self.intellect,
            SkillKind::Combat => self.combat,
            SkillKind::Agility => self.agility,
        }
    }
}

/// Which of the four skill values a skill test is being made against.
///
/// Deliberately NOT `#[non_exhaustive]` — same rationale as [`Skills`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SkillKind {
    /// Tests against the will, fear, sanity-eroding effects.
    Willpower,
    /// Tests for investigating, deduction, lore.
    Intellect,
    /// Tests for fighting, combat, physical strength.
    Combat,
    /// Tests for evading, dexterity, speed.
    Agility,
}
```

Confirm `use serde::{Deserialize, Serialize};` is present in `card_data.rs` (it is — `CardMetadata` derives serde).

- [ ] **Step 2: Export from `card-dsl`**

In `crates/card-dsl/src/lib.rs`, extend the `card_data` re-export:

```rust
pub use card_data::{CardMetadata, CardType, Class, SkillIcons, SkillKind, Skills, Slot};
```

- [ ] **Step 3: Remove the defs from game-core and re-export**

In `crates/game-core/src/state/investigator.rs`, delete the `pub struct Skills { … }`, its `impl Skills { … }`, and `pub enum SkillKind { … }` (now in card-dsl). Keep everything else (`Investigator`, `Status`, `DefeatCause`, etc.).

In `crates/game-core/src/state/mod.rs`, change the investigator re-export line so `Skills`/`SkillKind` come from card-dsl. The current line is:

```rust
pub use investigator::{DefeatCause, Investigator, InvestigatorId, SkillKind, Skills, Status};
```

Replace with:

```rust
pub use card_dsl::card_data::{SkillKind, Skills};
pub use investigator::{DefeatCause, Investigator, InvestigatorId, Status};
```

(`investigator.rs` still needs `Skills` in scope for the `Investigator.skills` field — it resolves via `crate::state::Skills` or add `use card_dsl::card_data::Skills;` at the top of `investigator.rs` if the compiler asks.)

- [ ] **Step 4: Add a card-dsl test**

In `card_data.rs`'s `#[cfg(test)] mod tests`:

```rust
#[test]
fn skills_value_indexes_each_kind() {
    let s = Skills { willpower: 3, intellect: 2, combat: 4, agility: 1 };
    assert_eq!(s.value(SkillKind::Willpower), 3);
    assert_eq!(s.value(SkillKind::Intellect), 2);
    assert_eq!(s.value(SkillKind::Combat), 4);
    assert_eq!(s.value(SkillKind::Agility), 1);
}
```

- [ ] **Step 5: Verify the workspace compiles + tests pass (behavior unchanged)**

Run: `cargo test -p card-dsl -p game-core skills`
Then: `cargo build --all`
Expected: green — every `Skills`/`SkillKind` reference resolves via the re-export.

- [ ] **Step 6: Commit**

```bash
git add crates/card-dsl/src/card_data.rs crates/card-dsl/src/lib.rs \
        crates/game-core/src/state/investigator.rs crates/game-core/src/state/mod.rs
git commit -m "card-dsl: move Skills + SkillKind down from game-core"
# end with: Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
```

---

## Task 2: Reshape `CardMetadata` → identity core + `CardKind` (atomic)

A struct field reshape can't compile in pieces, so this is **one commit**: define the types, rewrite the emitter, regenerate, migrate readers + mocks, gauntlet. Steps are ordered work toward a single green tree.

**Files:** all listed in File Structure above.

- [ ] **Step 1: Reshape the types in `card_data.rs`**

Replace the `pub struct CardMetadata { … }` (the flat one) with the identity core + `kind`, and add `CardKind` + accessors. Drop `flavor`, `illustrator`, `position` entirely.

```rust
/// One card's normalized metadata: an identity core shared by every card,
/// plus type-specific data in [`kind`](CardMetadata::kind).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct CardMetadata {
    /// Five-character ArkhamDB code. Identity / registry lookup key.
    pub code: String,
    /// Display name.
    pub name: String,
    /// Traits ("Ghoul", "Item", …); empty when none.
    pub traits: Vec<String>,
    /// Printed game-rules text, as printed.
    pub text: Option<String>,
    /// Pack the card belongs to ("core", "dwl").
    pub pack_code: String,
    /// Type-specific data.
    pub kind: CardKind,
}

/// Per-card-type data. The discriminant mirrors [`CardType`]; read it via
/// [`CardMetadata::card_type`]. Player variants carry [`Class`]; encounter
/// variants do not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum CardKind {
    /// Investigator (the player character; never deckbuilt).
    Investigator { class: Class, skills: Skills, health: u8, sanity: u8 },
    /// Asset — played to a play area; may hold health/sanity (ally soak).
    Asset {
        class: Class, cost: Option<i8>, xp: Option<u8>, slots: Vec<Slot>,
        health: Option<u8>, sanity: Option<u8>, skill_icons: SkillIcons,
        is_fast: bool, deck_limit: u8,
    },
    /// Event — played from hand, then discarded.
    Event { class: Class, cost: Option<i8>, xp: Option<u8>, skill_icons: SkillIcons, is_fast: bool, deck_limit: u8 },
    /// Skill — committed to a skill test.
    Skill { class: Class, xp: Option<u8>, skill_icons: SkillIcons, deck_limit: u8 },
    /// Enemy — encounter (or weakness) creature.
    Enemy { health: Option<u8>, spawn: Option<Spawn>, surge: bool, peril: bool, quantity: u8 },
    /// Treachery — one-shot encounter card resolved on reveal.
    Treachery { surge: bool, peril: bool, quantity: u8 },
}

impl CardMetadata {
    /// The card's [`CardType`] discriminant, derived from [`kind`](Self::kind).
    #[must_use]
    pub fn card_type(&self) -> CardType {
        match self.kind {
            CardKind::Investigator { .. } => CardType::Investigator,
            CardKind::Asset { .. } => CardType::Asset,
            CardKind::Event { .. } => CardType::Event,
            CardKind::Skill { .. } => CardType::Skill,
            CardKind::Enemy { .. } => CardType::Enemy,
            CardKind::Treachery { .. } => CardType::Treachery,
        }
    }

    /// The player [`Class`], or `None` for encounter cards (which have none).
    #[must_use]
    pub fn class(&self) -> Option<Class> {
        match &self.kind {
            CardKind::Investigator { class, .. }
            | CardKind::Asset { class, .. }
            | CardKind::Event { class, .. }
            | CardKind::Skill { class, .. } => Some(*class),
            CardKind::Enemy { .. } | CardKind::Treachery { .. } => None,
        }
    }
}
```

Update `card-dsl/src/lib.rs` to export `CardKind` and `Spawn`:

```rust
pub use card_data::{CardKind, CardMetadata, CardType, Class, SkillIcons, SkillKind, Skills, Slot, Spawn};
```

- [ ] **Step 2: Update `card_data.rs`'s own tests + add accessor tests**

The existing tests in `card_data.rs` build flat `CardMetadata { … }` literals — rewrite each to the new shape (identity fields + `kind: CardKind::X { … }`). Add:

```rust
#[test]
fn card_type_is_derived_from_kind() {
    let m = CardMetadata {
        code: "x".into(), name: "X".into(), traits: vec![], text: None,
        pack_code: "core".into(),
        kind: CardKind::Skill { class: Class::Seeker, xp: None,
            skill_icons: SkillIcons::default(), deck_limit: 2 },
    };
    assert_eq!(m.card_type(), CardType::Skill);
    assert_eq!(m.class(), Some(Class::Seeker));
}

#[test]
fn encounter_cards_have_no_class() {
    let m = CardMetadata {
        code: "y".into(), name: "Y".into(), traits: vec![], text: None,
        pack_code: "core".into(),
        kind: CardKind::Treachery { surge: false, peril: false, quantity: 1 },
    };
    assert_eq!(m.card_type(), CardType::Treachery);
    assert_eq!(m.class(), None);
}
```

(`SkillIcons` derives `Default`, so `SkillIcons::default()` works.)

Run: `cargo test -p card-dsl`
Expected: PASS — `card-dsl` compiles and its tests pass in isolation (it has no downstream deps).

- [ ] **Step 3: Rewrite the pipeline emitter**

In `crates/card-data-pipeline/src/main.rs`:

(a) Update the generated-file `use` line in `render()` from
`use card_dsl::card_data::{CardMetadata, CardType, Class, SkillIcons, Slot};`
to
`use card_dsl::card_data::{CardKind, CardMetadata, Class, SkillIcons, Slot};`
(drop `CardType` — no longer named in output; keep `Spawn` out since the corpus only emits `spawn: None`, but it IS named: write `spawn: None` needs no import).

(b) Replace `render_card` so it emits the identity core then a `kind:` matching `c.card_type`. The `NormalizedCard` already carries every value needed. Concretely:

```rust
fn render_card(out: &mut String, c: &NormalizedCard) {
    let _ = writeln!(out, "        CardMetadata {{");
    let _ = writeln!(out, "            code: {}.to_owned(),", str_lit(&c.code));
    let _ = writeln!(out, "            name: {}.to_owned(),", str_lit(&c.name));
    let _ = writeln!(out, "            traits: {},", string_vec(&c.traits));
    let _ = writeln!(out, "            text: {},", opt_owned_str(c.text.as_deref()));
    let _ = writeln!(out, "            pack_code: {}.to_owned(),", str_lit(&c.pack_code));
    let _ = writeln!(out, "            kind: {},", render_kind(c));
    let _ = writeln!(out, "        }},");
}
```

Add `render_kind` returning a `String`, one arm per `c.card_type` (a `&str` like `"Asset"` today). Use the existing helpers (`opt_i8`, `opt_u8`, `slot_vec`) and inline `SkillIcons { … }` / `Skills { … }` literals. Sketch:

```rust
fn render_kind(c: &NormalizedCard) -> String {
    let icons = format!(
        "SkillIcons {{ willpower: {}, intellect: {}, combat: {}, agility: {}, wild: {} }}",
        c.skill_willpower, c.skill_intellect, c.skill_combat, c.skill_agility, c.skill_wild,
    );
    match c.card_type {
        "Investigator" => format!(
            "CardKind::Investigator {{ class: Class::{}, skills: Skills {{ willpower: {}, intellect: {}, combat: {}, agility: {} }}, health: {}, sanity: {} }}",
            c.class, c.skill_willpower as i8, c.skill_intellect as i8, c.skill_combat as i8, c.skill_agility as i8,
            c.health.unwrap_or(0), c.sanity.unwrap_or(0),
        ),
        "Asset" => format!(
            "CardKind::Asset {{ class: Class::{}, cost: {}, xp: {}, slots: {}, health: {}, sanity: {}, skill_icons: {}, is_fast: {}, deck_limit: {} }}",
            c.class, opt_i8(c.cost), opt_u8(c.xp), slot_vec(&c.slots), opt_u8(c.health), opt_u8(c.sanity), icons, c.is_fast, c.deck_limit,
        ),
        "Event" => format!(
            "CardKind::Event {{ class: Class::{}, cost: {}, xp: {}, skill_icons: {}, is_fast: {}, deck_limit: {} }}",
            c.class, opt_i8(c.cost), opt_u8(c.xp), icons, c.is_fast, c.deck_limit,
        ),
        "Skill" => format!(
            "CardKind::Skill {{ class: Class::{}, xp: {}, skill_icons: {}, deck_limit: {} }}",
            c.class, opt_u8(c.xp), icons, c.deck_limit,
        ),
        "Enemy" => format!(
            "CardKind::Enemy {{ health: {}, spawn: None, surge: false, peril: false, quantity: {} }}",
            opt_u8(c.health), c.quantity,
        ),
        "Treachery" => format!(
            "CardKind::Treachery {{ surge: false, peril: false, quantity: {} }}",
            c.quantity,
        ),
        other => panic!("card {}: unsupported card_type {other:?} for CardKind (encounter types land in #252)", c.code),
    }
}
```

Drop the now-unused emit lines for `flavor`/`illustrator`/`position` and remove those fields from `RawCard`/`NormalizedCard` and from `normalize` (the pipeline no longer reads them). Investigator health/sanity are required `u8` in the variant — investigators always have both in the snapshot, so `unwrap_or(0)` is a safe floor; if any investigator is missing them the value is 0 (none are).

Run: `cargo build -p card-data-pipeline`
Expected: compiles.

- [ ] **Step 4: Regenerate the corpus**

Run: `cargo run -p card-data-pipeline`
Then: `cargo build -p cards`
Expected: `cards.rs` rewritten with `kind: CardKind::…`; the `cards` crate compiles.

- [ ] **Step 5: Migrate the 3 stat-readers**

`game-core/src/engine/dispatch/phases.rs` — the seating path. Replace the `investigator_skills(meta)` helper + its use. Where it currently does `(investigator_skills(meta), meta.health, meta.sanity)`, match the kind instead:

```rust
let (skills, health, sanity) = match &meta.kind {
    card_dsl::card_data::CardKind::Investigator { skills, health, sanity } => (*skills, *health, *sanity),
    _ => return EngineOutcome::Rejected {
        reason: format!("card {} is not a seatable investigator", entry.investigator).into(),
    },
};
```

Delete the now-unused `investigator_skills` fn and its two unit tests (or repoint them at the new match). `skills` is already a `Skills` — no `try_from`.

`game-core/src/engine/dispatch/encounter.rs` — spawn reads health. Replace `metadata.health.unwrap_or(1)` with a kind match:

```rust
let max_health = match &metadata.kind {
    card_dsl::card_data::CardKind::Enemy { health, .. } => health.unwrap_or(1),
    _ => 1,
};
```

`game-core/src/engine/dispatch/cards.rs` — `let is_fast = metadata.is_fast;` becomes:

```rust
let is_fast = matches!(
    &metadata.kind,
    card_dsl::card_data::CardKind::Asset { is_fast: true, .. }
        | card_dsl::card_data::CardKind::Event { is_fast: true, .. }
);
```

(Use whatever import alias is already in scope; these files already `use` `card_dsl`/`card_data` types — match the existing path.)

- [ ] **Step 6: Migrate the mock `CardMetadata` literals**

Each of these builds a flat `CardMetadata { … }` — rewrite to identity core + `kind: CardKind::X { … }`, choosing the variant matching the mock's intent (an investigator mock → `Investigator`, a treachery mock → `Treachery`, etc.). Files:
`game-core/src/card_registry.rs`, `scenarios/src/test_fixtures/synth_cards.rs`, `cards/tests/reject_rollback.rs`, and `game-core/tests/{forced_triggers,activate_ability,reaction_windows,on_skill_test_resolution}.rs`.

Worked example — a synth investigator mock becomes:

```rust
CardMetadata {
    code: "synth_inv".into(), name: "Synth Investigator".into(),
    traits: vec![], text: None, pack_code: "synth".into(),
    kind: CardKind::Investigator {
        class: Class::Neutral,
        skills: Skills { willpower: 3, intellect: 3, combat: 3, agility: 3 },
        health: 5, sanity: 5,
    },
}
```

(Carry over whatever stats the old literal set; map old `skill_icons`/`health`/`sanity` into the variant.)

- [ ] **Step 7: Full strict gauntlet**

```bash
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --all && cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
cargo build -p web --target wasm32-unknown-unknown
cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings
```

Expected: all green. The 3 readers keep their existing behavior; seating-stats / spawn-health / fast-play tests pass unchanged.

- [ ] **Step 8: Commit**

```bash
git add -A
git commit -m "card-dsl: remodel CardMetadata as identity core + CardKind enum"
# body: note the dropped cosmetic fields + relocated class/quantity/surge/peril; regenerated corpus. Closes #254.
# end with: Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
```

---

## Task 3: Gauntlet + PR + (no phase doc)

- [ ] **Step 1:** Confirm the full gauntlet is green (Task 2 Step 7).
- [ ] **Step 2:** Push `engine/cardkind-remodel`; open the PR (`Closes #254`); watch `gh pr checks <PR#> --watch`.
- [ ] **Step 3:** No phase-doc update — this is an `[engine]` refactor, not a phase issue.

---

## Self-Review

**Spec coverage:**
- Identity core + `CardKind` (6 variants) → Task 2 Step 1. ✓
- Drop position/flavor/illustrator → Task 2 Steps 1, 3. ✓
- class → player variants; quantity/surge/peril → Enemy/Treachery; spawn → Enemy → Task 2 Step 1. ✓
- `card_type()` derived accessor (+ `class()`) → Task 2 Step 1. ✓
- Move Skills/SkillKind to card-dsl, re-export, Investigator carries `skills: Skills`, seating drops `try_from` → Task 1 + Task 2 Step 5. ✓
- Pipeline emits new shape; corpus regenerated → Task 2 Steps 3–4. ✓
- 3 readers + mocks migrated; behavior-preserving → Task 2 Steps 5–6. ✓
- Out of scope (encounter ingestion) → not touched. ✓

**Placeholder scan:** No "TBD"/"implement later". The `render_kind` panic arm is intentional (loud refusal for encounter types that arrive in #252), not a stub.

**Type consistency:** `CardKind` variant names + fields match between Task 2 Step 1, the pipeline `render_kind` (Step 3), the readers (Step 5), and mocks (Step 6). `Skills`/`SkillKind` from card-dsl used consistently after Task 1. `class()` returns `Option<Class>`; `card_type()` returns `CardType`.

**Atomicity note:** Task 2 is intentionally one commit — a struct reshape has no compiling half-state. Task 1 is independent and green on its own.
