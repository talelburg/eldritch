# #127 — Enemy spawn rules — implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Land the `Spawn` keyword surface on `CardMetadata`, the `EventPattern::EnemySpawned` DSL pattern, the engine's `spawn_enemy` handler, and wire it into `encounter_card_revealed` (replacing the `#126` enemy-arm stub reject). End-to-end proof: a synthetic spawn-bearing enemy in the test fixture is revealed from the encounter deck and lands in play at the correct location, engaged with the drawing investigator when appropriate.

**Architecture:** Mirror the existing `Trigger::OnPlay` / `Trigger::Revelation` shape in `card-dsl` for the new DSL pieces, and the `encounter_card_revealed` validate-first / mutate-second shape in `game-core/engine/dispatch.rs` for the new `spawn_enemy` handler. `Spawn` on `CardMetadata` is a nested `Option<Spawn>` struct (not flat fields) so it can grow over time. `Location` gets a `code: CardCode` field because spawn rules reference printed location identifiers; locations are cards in Arkham, so reusing `CardCode` rather than introducing a new `LocationCode` type is the YAGNI-respecting call (the spec's open question on this is resolved below). Enemy ID minting reuses a new `state.next_enemy_id: u32` counter mirroring the existing `next_card_instance_id`. The synthetic spawn-bearing enemy lives next to the existing synthetic treachery in `crates/scenarios/src/test_fixtures/synth_cards.rs`; the integration test gets its own cargo binary at `crates/scenarios/tests/encounter_spawn.rs`.

**Tech Stack:** Rust 2021, `serde`, `card-dsl` (pure data), `game-core` (kernel), `scenarios` (synthetic fixtures), `card-data-pipeline` (corpus regeneration).

**Spec:** `docs/superpowers/specs/2026-05-22-127-enemy-spawn-rules-design.md` is the authoritative design — re-read it when starting and when in doubt about a decision.

**Branch name:** `engine/enemy-spawn-rules`.

**PR procedure:** CLAUDE.md's 7-step PR procedure applies. This plan covers steps 1 (local CI gauntlet), 2 (commits on a feature branch), 6 (phase-doc update as last commit), and the PR-open hand-off. CI watch + addressing CI failures and the user-approved merge are driven by the human after the PR opens.

---

## Design decisions locked in before coding

These resolve the spec's "Open items resolved at implementation time" section and a few additional choices made while reading current code. If implementation surfaces a reason to revisit any of these, raise it before pressing on.

1. **`SpawnLocation::Specific(CardCode)`, not a new `LocationCode` newtype.** Spec asked "verify `LocationCode` exists as a distinct type from `CardCode`; if not, add it." Resolved: don't add it. Locations in Arkham are cards with printed `ArkhamDB` codes (Study = `01111`, etc.); the namespace is *already* shared at the data level. Introducing a newtype now would only block accidental cross-use *at the engine level*, with no real consumer asking for that distinction. If a future PR has a concrete reason (e.g. a struct that holds both kinds and needs the compiler to distinguish them), it can introduce `LocationCode` then. YAGNI. The plan's `Spawn` struct uses `CardCode`.
2. **`Location` gets a new `code: CardCode` field.** Spec required this — the spawn handler does a `state.locations.iter().find(|(_, loc)| &loc.code == loc_code)` lookup. Field is non-optional: every location at scenario setup time has a code. `test_location` fixture defaults it to `CardCode(format!("_test_loc_{id}"))` to keep the test surface ergonomic (no caller-visible change beyond fresh assertions that reference `loc.code`).
3. **`state.next_enemy_id: u32` counter for enemy minting.** Mirrors `state.next_card_instance_id`. The existing engine has no enemy-minting site (tests place enemies by hard-coded `EnemyId(...)`); this is the first one. Adding a separate counter avoids conflating "enemy in play" and "card in play" ID spaces — `EnemyId` and `CardInstanceId` are already distinct types.
4. **`Event::EnemySpawned` is extended in place with `code` and `engaged_with`.** Grep confirms no current consumers; the existing variant is structural-only. `Event` is `#[non_exhaustive]`, but variant-field changes are still breaking by Rust rules — safe here because there are zero matches against it.
5. **No companion `Event::EnemyEngaged` on on-spawn engagement.** The spec emits `EnemySpawned` with `engaged_with: Option<InvestigatorId>` and stops. Following the spec — denormalize the engagement state onto `EnemySpawned`. A future PR with a real "after an enemy engages an investigator" listener gets to add the separate event then (concrete-consumer-first, matching the project's pattern).
6. **Revelation before spawn for enemies.** When an encounter enemy with Revelation effects is revealed, Revelation runs *before* the spawn handler. Matches Rules Reference page 24 ("1. Resolve any 'Revelation' effects … 2. If it is an Enemy: …spawn instructions / default engagement"). No Phase-4-scope enemy has Revelation, so this is structural — the loop is in place for Phase-7+ enemies. Quote the rules ref clause verbatim in the doc comment.
7. **Pipeline emits `spawn: None` literally for every enemy.** No upstream parsing of spawn text yet — Phase-7's first spawn-bearing real card forces that work. The pipeline change is one new `writeln!` per emit and one new field on `NormalizedCard` (defaulted at construction). The regenerated `cards/src/generated/cards.rs` adds `spawn: None,` on ~ 600 lines (one per card); regenerate via `cargo run -p card-data-pipeline` and commit the diff.
8. **`SYNTH_LOC_CODE = "_synth_loc"`.** Underscore prefix mirrors `_synth_treachery` from #126; can't collide with real ArkhamDB codes (which are digit-prefixed five-char strings). Used both as the synthetic location's `code` field and as the spawn target for `_synth_enemy`.
9. **`_synth_enemy` carries minimal stats.** Health 1, fight 1, evade 1, attack damage 0, attack horror 0, no traits, not aloof / hunter / etc. (those fields don't exist yet). Just enough to land in play; not exercised in combat by this PR.
10. **Multi-investigator engagement-on-spawn rejects.** Spec is explicit — return `EngineOutcome::Rejected` with a reason naming `#128`, because resolving "which investigator at the location gets engaged" requires the `Prey` shape that's the core of #128's hunter-movement work. Single-investigator (the synthetic fixture's case) and zero-investigator (enemy-only at a non-investigator location) cases handle in this PR.

---

## File map

- **Create:**
  - `crates/scenarios/tests/encounter_spawn.rs` — integration test binary (own process; installs `TEST_REGISTRY`).
- **Modify:**
  - `crates/card-dsl/src/card_data.rs` — `Spawn` struct + `SpawnLocation` enum, `spawn: Option<Spawn>` on `CardMetadata`, unit tests.
  - `crates/card-dsl/src/dsl.rs` — `EventPattern::EnemySpawned` variant + unit tests.
  - `crates/game-core/src/state/location.rs` — `code: CardCode` field on `Location` + serde test.
  - `crates/game-core/src/state/game_state.rs` — `next_enemy_id: u32` field + serde test.
  - `crates/game-core/src/state/game_state.rs` — `GameState::default()` (and / or fixture wiring) initializes the new field to 0.
  - `crates/game-core/src/event.rs` — extend `Event::EnemySpawned` with `code: CardCode` + `engaged_with: Option<InvestigatorId>`; serde test.
  - `crates/game-core/src/test_support/fixtures.rs` — `test_location` populates `code` with a deterministic default.
  - `crates/game-core/src/engine/dispatch.rs` — `spawn_enemy` handler; replace the `CardType::Enemy` reject arm in `encounter_card_revealed` with the real call; unit tests.
  - `crates/card-data-pipeline/src/main.rs` — `NormalizedCard.spawn: Option<()>` always-None field + emit `spawn: None,` for every card.
  - `crates/cards/src/generated/cards.rs` — regenerated; every entry gets `spawn: None,`.
  - `crates/scenarios/src/test_fixtures/synth_cards.rs` — add `SYNTH_LOC_CODE` const, `SYNTH_ENEMY_CODE` const, synthetic enemy metadata, extend `metadata_for` and `abilities_for` for the new code.
  - `crates/scenarios/src/test_fixtures/synthetic.rs` — `setup()` populates the demo location's `code` with `SYNTH_LOC_CODE` (location currently doesn't carry one; `test_location` will default it but the fixture should overwrite to the canonical synth code).
  - `docs/phases/phase-4-scenario-plumbing.md` — LAST commit only; do not touch mid-PR.

Every commit must compile cleanly with the full CI gauntlet:

```sh
RUSTFLAGS="-D warnings"    cargo test --all --all-features
                           cargo clippy --all-targets --all-features -- -D warnings
                           cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
                           cargo build -p web --target wasm32-unknown-unknown
```

---

## Task 1: Set up the feature branch + commit the plan

**Files:**
- Add: `docs/superpowers/plans/2026-05-22-127-enemy-spawn-rules.md` (this file).

- [ ] **Step 1: Create the feature branch from main**

```bash
git checkout main
git pull
git checkout -b engine/enemy-spawn-rules
```

- [ ] **Step 2: Commit the plan file**

`docs/superpowers/` is tracked in git (PR #132 + #133 set the convention). Add this plan as the branch's first commit:

```bash
git add docs/superpowers/plans/2026-05-22-127-enemy-spawn-rules.md
git commit -m "$(cat <<'EOF'
docs: implementation plan for #127

Refs #127.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: Add `Spawn` + `SpawnLocation` types and the `spawn` field on `CardMetadata`

**Files:**
- Modify: `crates/card-dsl/src/card_data.rs`

- [ ] **Step 1: Write the failing test**

Append a new `#[cfg(test)] mod spawn_tests` block at the bottom of `crates/card-dsl/src/card_data.rs`:

```rust
#[cfg(test)]
mod spawn_tests {
    use super::*;

    #[test]
    fn spawn_specific_round_trips_through_serde_json() {
        let original = Spawn {
            location: SpawnLocation::Specific("01112".to_owned()),
        };
        let json = serde_json::to_string(&original).expect("serialize");
        let recovered: Spawn = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(original, recovered);
    }

    #[test]
    fn card_metadata_serde_roundtrip_preserves_spawn_specific() {
        let original = CardMetadata {
            code: "_synth_enemy".into(),
            name: "Synth Enemy".into(),
            class: Class::Mythos,
            card_type: CardType::Enemy,
            cost: None,
            xp: None,
            text: None,
            flavor: None,
            illustrator: None,
            traits: Vec::new(),
            slots: Vec::new(),
            skill_icons: SkillIcons::default(),
            health: Some(1),
            sanity: None,
            deck_limit: 1,
            quantity: 1,
            pack_code: "_synth".into(),
            position: 1,
            is_fast: false,
            spawn: Some(Spawn {
                location: SpawnLocation::Specific("_synth_loc".into()),
            }),
        };
        let json = serde_json::to_string(&original).expect("serialize");
        let back: CardMetadata = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, original);
    }

    #[test]
    fn card_metadata_serde_roundtrip_preserves_spawn_none() {
        let original = CardMetadata {
            code: "01000".into(),
            name: "Random Basic Weakness".into(),
            class: Class::Neutral,
            card_type: CardType::Treachery,
            cost: None,
            xp: None,
            text: None,
            flavor: None,
            illustrator: None,
            traits: Vec::new(),
            slots: Vec::new(),
            skill_icons: SkillIcons::default(),
            health: None,
            sanity: None,
            deck_limit: 0,
            quantity: 1,
            pack_code: "core".into(),
            position: 0,
            is_fast: false,
            spawn: None,
        };
        let json = serde_json::to_string(&original).expect("serialize");
        let back: CardMetadata = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, original);
        assert!(back.spawn.is_none());
    }
}
```

- [ ] **Step 2: Confirm compile failure**

Run:
```bash
cargo test -p card-dsl spawn 2>&1 | head -30
```

Expected: compile errors — `cannot find type Spawn`, `no field spawn on CardMetadata`.

- [ ] **Step 3: Add the `SpawnLocation` enum and `Spawn` struct**

In `crates/card-dsl/src/card_data.rs`, add immediately above `pub struct CardMetadata`:

```rust
/// Where on the location map an encounter enemy spawns.
///
/// Phase-4 minimal set: just a printed location code. Future variants
/// (`LeadInvestigator`, `LowestSanityInvestigator`, `NearestUnexplored`,
/// etc.) land with the first Phase-7+ card that needs them.
///
/// **Why a [`String`] code rather than a `LocationCode` newtype.**
/// Locations in Arkham are cards with `ArkhamDB` codes; the namespace
/// is shared at the data level. Introducing a distinct
/// `LocationCode` newtype would block accidental cross-use at the
/// engine level without a concrete consumer asking for that
/// distinction. Reuse `CardCode` (which is a [`String`] newtype in
/// `game-core::state::card`) by passing the bare string here; the
/// engine's spawn handler wraps it on lookup.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SpawnLocation {
    /// Fixed-location spawn — the named location's printed code.
    Specific(String),
}

/// Spawn rule for an encounter-deck enemy.
///
/// `None` on [`CardMetadata::spawn`] means "no spawn instruction" — per
/// Rules Reference p.24, the enemy spawns engaged with the drawing
/// investigator, placed in that investigator's threat area.
///
/// **Why a nested struct, not flat fields on `CardMetadata`.** So
/// spawn-related fields can grow (e.g. `engagement:
/// EngagementOnSpawn` for Aloof / "spawn unengaged" cards,
/// `also_spawn_doom_at: ...` for the rare multi-effect spawns)
/// without churning every enemy declaration in the generated corpus.
/// Phase-4 ships only `location`; later variants land alongside the
/// cards that force them.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Spawn {
    /// Where the enemy spawns.
    pub location: SpawnLocation,
}
```

- [ ] **Step 4: Add the `spawn` field on `CardMetadata`**

Inside `pub struct CardMetadata`, append after `is_fast`:

```rust
    /// Spawn rule for encounter-deck enemies. `None` for enemies
    /// that don't spawn from the encounter deck (placed at scenario
    /// setup directly), for non-enemy card types, and as the
    /// pipeline's default for all generated entries until Phase-7's
    /// structured-spawn-text parsing lands.
    pub spawn: Option<Spawn>,
```

- [ ] **Step 5: Update the existing `is_fast_tests::metadata_serde_roundtrip_preserves_is_fast` to compile with the new field**

Inside the existing struct literal in that test, append `spawn: None,` so the literal stays exhaustive (the struct is **not** `#[non_exhaustive]`, so every field must be named):

```rust
        let original = CardMetadata {
            // ... existing fields ...
            is_fast: true,
            spawn: None,
        };
```

- [ ] **Step 6: Run the tests, verify pass**

Run:
```bash
cargo test -p card-dsl spawn
cargo test -p card-dsl is_fast
```

Expected: 3 new tests pass + the existing is_fast test still passes.

- [ ] **Step 7: Full card-dsl gauntlet**

```bash
RUSTFLAGS="-D warnings" cargo test -p card-dsl --all-features
cargo clippy -p card-dsl --all-targets --all-features -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc -p card-dsl --no-deps --all-features
```

Expected: all green. The `cards` crate will fail to compile separately because of the new field — that's expected and gets fixed by the pipeline regeneration in Task 3.

- [ ] **Step 8: Commit**

```bash
git add crates/card-dsl/src/card_data.rs
git commit -m "$(cat <<'EOF'
infra: add Spawn / SpawnLocation + spawn field on CardMetadata

Adds the static-metadata surface for encounter-deck enemy spawning.
SpawnLocation has one variant (Specific) for now; the nested Spawn
struct gives later spawn-related fields (engagement-on-spawn, multi-
effect spawns) somewhere to grow without churning every enemy
declaration.

CardMetadata.spawn defaults to None for non-enemy cards, for enemies
placed at scenario setup, and as the pipeline's default until Phase-7
parses upstream spawn text. The cards crate's generated corpus is
regenerated in the next commit.

Refs #127.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: Regenerate the card corpus with `spawn: None`

**Files:**
- Modify: `crates/card-data-pipeline/src/main.rs`
- Modify (generated): `crates/cards/src/generated/cards.rs`

- [ ] **Step 1: Write the failing pipeline test (snapshot-style)**

In `crates/card-data-pipeline/src/main.rs`, find the existing tests near the bottom (after `is_fast` detection tests) and append:

```rust
    #[test]
    fn emitted_card_includes_spawn_none_field() {
        // Pipeline should emit `spawn: None,` as the last field of
        // every generated card literal so the cards crate compiles
        // against the new CardMetadata.spawn field.
        let card = NormalizedCard {
            code: "01001".into(),
            name: "Test".into(),
            class: "Mythos",
            card_type: "Treachery",
            cost: None,
            xp: None,
            text: None,
            flavor: None,
            illustrator: None,
            traits: Vec::new(),
            slots: Vec::new(),
            skill_willpower: 0,
            skill_intellect: 0,
            skill_combat: 0,
            skill_agility: 0,
            skill_wild: 0,
            health: None,
            sanity: None,
            deck_limit: 0,
            quantity: 1,
            pack_code: "core".into(),
            position: 1,
            is_fast: false,
        };
        let mut buf = Vec::new();
        emit_card(&mut buf, &card);
        let s = String::from_utf8(buf).expect("utf8");
        assert!(
            s.contains("spawn: None,"),
            "emitted card should include `spawn: None,` field; got:\n{s}",
        );
    }
```

(Adjust the `NormalizedCard` literal to match the current struct shape — the test_module section of `main.rs` already constructs `NormalizedCard` for other tests; copy that shape rather than reconstructing from scratch. The exact field names above match what `lines 160–183` show; the `class` and `card_type` are `&'static str` so use `"Mythos"` / `"Treachery"` literals.)

- [ ] **Step 2: Confirm test failure**

```bash
cargo test -p card-data-pipeline emitted_card_includes_spawn_none 2>&1 | head -20
```

Expected: assertion failure — `spawn: None,` not present in output.

- [ ] **Step 3: Emit `spawn: None` for every card**

In `crates/card-data-pipeline/src/main.rs`, find `emit_card` (~line 320–367). After the `is_fast` writeln (line 366), insert:

```rust
    // spawn: None for every generated card. Pipeline doesn't yet
    // parse upstream spawn text — the first Phase-7+ PR that needs
    // structured spawn data adds the parser and starts emitting
    // Some(...) for spawn-bearing enemies. Until then, the corpus
    // expresses "default spawn (engaged with drawing investigator)"
    // for every enemy, which is the Rules Reference p.24 fallback.
    let _ = writeln!(out, "            spawn: None,");
```

- [ ] **Step 4: Verify the new pipeline test passes**

```bash
cargo test -p card-data-pipeline emitted_card_includes_spawn_none
```

Expected: pass.

- [ ] **Step 5: Regenerate the corpus**

```bash
cargo run -p card-data-pipeline
```

Expected: `crates/cards/src/generated/cards.rs` is overwritten. Every card now ends with `spawn: None,`.

Smoke-check the diff:

```bash
git diff --stat crates/cards/src/generated/cards.rs
```

Expected: a large additions-only diff (one `+ spawn: None,` line per card, ~ 600 lines).

- [ ] **Step 6: Run the cards crate's tests to confirm it compiles against the new metadata shape**

```bash
RUSTFLAGS="-D warnings" cargo test -p cards --all-features
```

Expected: green. The cards crate was broken between Task 2 and now; this verifies the regeneration repaired it.

- [ ] **Step 7: Full pipeline + cards gauntlet**

```bash
RUSTFLAGS="-D warnings" cargo test -p card-data-pipeline --all-features
cargo clippy -p card-data-pipeline --all-targets --all-features -- -D warnings
cargo clippy -p cards --all-targets --all-features -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc -p card-data-pipeline --no-deps --all-features
RUSTDOCFLAGS="-D warnings" cargo doc -p cards --no-deps --all-features
```

Expected: all green.

- [ ] **Step 8: Commit**

```bash
git add crates/card-data-pipeline/src/main.rs crates/cards/src/generated/cards.rs
git commit -m "$(cat <<'EOF'
cards: regenerate corpus with spawn: None on every card

Pipeline emits `spawn: None,` as the last field of every card
literal. No upstream spawn-text parsing yet — that lands with the
first Phase-7+ PR consuming structured spawn rules. Until then,
"default spawn" (engaged with drawing investigator, per Rules
Reference p.24) is correct behavior for every enemy.

Refs #127.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: Add `EventPattern::EnemySpawned`

**Files:**
- Modify: `crates/card-dsl/src/dsl.rs`

- [ ] **Step 1: Write the failing pattern test**

Append to the existing `#[cfg(test)] mod tests` block at the bottom of `crates/card-dsl/src/dsl.rs`:

```rust
    #[test]
    fn enemy_spawned_pattern_round_trips_through_serde_json() {
        let original = EventPattern::EnemySpawned;
        let json = serde_json::to_string(&original).expect("serialize");
        let recovered: EventPattern = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(original, recovered);
    }

    #[test]
    fn enemy_spawned_distinct_from_other_patterns() {
        let spawned = EventPattern::EnemySpawned;
        let defeated = EventPattern::EnemyDefeated {
            by_controller: true,
        };
        let revealed = EventPattern::CardRevealed { card_type: None };
        assert_ne!(spawned, defeated);
        assert_ne!(spawned, revealed);
    }
```

- [ ] **Step 2: Confirm compile failure**

```bash
cargo test -p card-dsl enemy_spawned 2>&1 | head -20
```

Expected: `no variant EnemySpawned found for enum EventPattern`.

- [ ] **Step 3: Add the variant**

In `crates/card-dsl/src/dsl.rs`, inside `enum EventPattern { ... }`, add the new variant after the existing `CardRevealed` variant:

```rust
    /// An enemy spawned at a location (entered play from the
    /// encounter deck via the on-draw resolution path).
    ///
    /// Intentionally bare (no narrowing fields). YAGNI on
    /// `by_controller` / `card_type` / `location_filter` until a
    /// real listener forces a shape. Concrete-consumer-first.
    ///
    /// First listener will likely be a Phase-7+ "after an enemy
    /// spawns at your location" reaction; that PR gets to extend
    /// this variant with whatever narrowing field it needs.
    EnemySpawned,
```

- [ ] **Step 4: Run the new tests, verify pass**

```bash
cargo test -p card-dsl enemy_spawned
```

Expected: 2 new tests pass.

- [ ] **Step 5: Fix any exhaustive matches the compiler flags**

```bash
cargo check --all --all-features 2>&1 | grep -E "non-exhaustive|error" | head -20
```

The primary site is `trigger_matches` in `crates/game-core/src/engine/dispatch.rs` — it `match`es `(kind, pattern)` exhaustively. Open the file at line ~1352 and add an arm for `EnemySpawned`:

```rust
        (_, EventPattern::EnemySpawned) => {
            // No window kind opens specifically for "enemy spawned"
            // in Phase 4. A future PR (likely Phase-7+) that wants
            // to react to spawns will add the corresponding
            // WindowKind variant and extend this arm.
            false
        }
```

Place this arm just before the existing catch-all `(WindowKind::BetweenPhases { .. } | WindowKind::AfterEnemyDefeated { .. }, _) => false,` line so the new pattern is named explicitly. After adding, double-check by re-running `cargo check --all --all-features`.

- [ ] **Step 6: Full card-dsl + game-core gauntlet**

```bash
RUSTFLAGS="-D warnings" cargo test -p card-dsl --all-features
RUSTFLAGS="-D warnings" cargo test -p game-core --all-features
cargo clippy -p card-dsl --all-targets --all-features -- -D warnings
cargo clippy -p game-core --all-targets --all-features -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc -p card-dsl --no-deps --all-features
RUSTDOCFLAGS="-D warnings" cargo doc -p game-core --no-deps --all-features
```

Expected: all green.

- [ ] **Step 7: Commit**

```bash
git add crates/card-dsl/src/dsl.rs crates/game-core/src/engine/dispatch.rs
git commit -m "$(cat <<'EOF'
infra: add EventPattern::EnemySpawned

Adds the OnEvent pattern for listeners that key off "an enemy
spawned" — Phase-7+ reaction cards ("after an enemy spawns at your
location", etc.) will use this surface.

Intentionally bare: YAGNI on narrowing fields until a real listener
forces the shape. trigger_matches in dispatch.rs adds an explicit
false arm for the new pattern, since no WindowKind opens
specifically for spawns in Phase 4.

Refs #127.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: Add `code: CardCode` to `Location` and update fixtures

**Files:**
- Modify: `crates/game-core/src/state/location.rs`
- Modify: `crates/game-core/src/test_support/fixtures.rs`
- Modify: `crates/scenarios/src/test_fixtures/synthetic.rs`

- [ ] **Step 1: Write the failing serde / field test**

Append at the bottom of `crates/game-core/src/state/location.rs`:

```rust
#[cfg(test)]
mod location_code_tests {
    use super::*;
    use crate::state::CardCode;

    #[test]
    fn location_carries_code_field() {
        let loc = Location {
            id: LocationId(1),
            code: CardCode("01112".into()),
            name: "Hallway".into(),
            shroud: 2,
            clues: 0,
            revealed: true,
            connections: Vec::new(),
        };
        assert_eq!(loc.code, CardCode("01112".into()));
    }

    #[test]
    fn location_serde_roundtrip_preserves_code() {
        let original = Location {
            id: LocationId(2),
            code: CardCode("_synth_loc".into()),
            name: "Demo Location".into(),
            shroud: 1,
            clues: 3,
            revealed: false,
            connections: vec![LocationId(1)],
        };
        let json = serde_json::to_string(&original).expect("serialize");
        let back: Location = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.id, original.id);
        assert_eq!(back.code, original.code);
        assert_eq!(back.name, original.name);
    }
}
```

- [ ] **Step 2: Confirm compile failure**

```bash
cargo test -p game-core location_code 2>&1 | head -20
```

Expected: `no field code on Location`.

- [ ] **Step 3: Add the `code` field**

In `crates/game-core/src/state/location.rs`, inside `pub struct Location { ... }`, add the field right after `pub id: LocationId,`:

```rust
    /// Printed `ArkhamDB` location code (e.g. `"01111"` for Study).
    /// Stable across instances of the same printed location — two
    /// copies of the same card in play would carry the same `code`
    /// but distinct `id`s.
    ///
    /// Used by encounter-enemy spawn rules to address a specific
    /// location by its printed identifier (see
    /// [`card_dsl::card_data::SpawnLocation::Specific`]).
    pub code: CardCode,
```

Update the imports at the top of the file to include `CardCode`:

```rust
use super::card::CardCode;
```

(Verify whether `super::card::CardCode` resolves — `card.rs` is a sibling module in `state/`. If the import path doesn't compile, try `use crate::state::CardCode;` instead; either should work given `state/mod.rs` re-exports `CardCode`.)

- [ ] **Step 4: Update `test_location` to populate `code`**

In `crates/game-core/src/test_support/fixtures.rs`, modify `test_location`:

```rust
#[must_use]
pub fn test_location(id: u32, name: impl Into<String>) -> Location {
    Location {
        id: LocationId(id),
        code: CardCode(format!("_test_loc_{id}")),
        name: name.into(),
        shroud: 2,
        clues: 0,
        revealed: true,
        connections: Vec::new(),
    }
}
```

Update the import line at the top of the file to include `CardCode`:

```rust
use crate::state::{
    CardCode, Enemy, EnemyId, Investigator, InvestigatorId, Location, LocationId, Skills, Status,
};
```

Update the doc comment on `test_location` to mention the default code:

```rust
/// A stock location with reasonable defaults.
///
/// - Shroud 2, 0 clues, revealed.
/// - No connections (caller adds them).
/// - `code` defaults to `CardCode("_test_loc_{id}")` — underscore-
///   prefixed so it can't collide with real `ArkhamDB` codes. Callers
///   that care about the code (encounter-spawn tests, etc.) should
///   mutate it directly after construction.
```

- [ ] **Step 5: Override the synthetic location's `code` in the demo fixture**

In `crates/scenarios/src/test_fixtures/synthetic.rs`, `setup()` currently uses `test_location(10, "Demo Location")`. The default `code` will be `_test_loc_10` — but the synthetic spawn-bearing enemy needs to target this location by its canonical synthetic code (`_synth_loc`). Adjust `setup()` to mutate the location's code:

```rust
pub fn setup() -> GameState {
    let mut location = test_location(10, "Demo Location");
    location.code = CardCode(super::synth_cards::SYNTH_LOC_CODE.into());

    let mut state = TestGame::new()
        .with_investigator(test_investigator(1))
        .with_location(location)
        .with_turn_order([InvestigatorId(1)])
        .with_scenario_id(ScenarioId::new(ID))
        .build();
    state
        .encounter_deck
        .push_back(CardCode(SYNTH_TREACHERY_CODE.into()));
    state
}
```

(The `SYNTH_LOC_CODE` const lands in Task 8; this code is forward-referencing it, so the synthetic.rs change can't be committed until Task 8 lands. **Defer this step into Task 8's commit** — see Task 8 step 4. Skip this step here; just don't commit synthetic.rs in this task.)

- [ ] **Step 6: Run the tests, verify pass**

```bash
cargo test -p game-core location_code
cargo test -p game-core test_location
```

Expected: pass.

- [ ] **Step 7: Verify Location's existing serde tests still pass**

```bash
cargo test -p game-core --all-features location
```

Expected: every location test that previously passed still passes. Any test that constructed a `Location` struct literal (instead of going through `test_location`) will need a `code: CardCode("...".into()),` field added — search for them and fix:

```bash
grep -rn "Location {" /home/talel/eldritch/crates/game-core/src 2>/dev/null
```

Inspect each hit; add `code: CardCode("...".into()),` where a struct literal is used. (`Location` is `#[non_exhaustive]`, so out-of-crate literals don't compile anyway — only in-crate construction sites need updating. Likely zero or one site beyond `test_location` itself.)

- [ ] **Step 8: Full game-core gauntlet**

```bash
RUSTFLAGS="-D warnings" cargo test -p game-core --all-features
cargo clippy -p game-core --all-targets --all-features -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc -p game-core --no-deps --all-features
```

Expected: all green. (`scenarios` will not yet compile because Task 8 hasn't added `SYNTH_LOC_CODE`; that's fine — we're not running its tests in this task.)

- [ ] **Step 9: Commit**

Stage only the game-core files. `synthetic.rs` is intentionally untouched in this task — its modification lands in Task 8 alongside `SYNTH_LOC_CODE`.

```bash
git add crates/game-core/src/state/location.rs crates/game-core/src/test_support/fixtures.rs
git commit -m "$(cat <<'EOF'
engine: add code field to Location

Locations now carry the printed ArkhamDB code (e.g. "01111" for
Study) alongside the in-scenario `id`. The code is the addressable
identifier for spawn rules — SpawnLocation::Specific(code) will
look up the location by this field.

test_location defaults code to "_test_loc_{id}"; underscore prefix
guarantees no collision with real ArkhamDB codes.

Refs #127.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: Add `next_enemy_id` counter to `GameState` + extend `Event::EnemySpawned`

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs`
- Modify: `crates/game-core/src/event.rs`

- [ ] **Step 1: Write the failing GameState test**

Append at the bottom of the existing `#[cfg(test)]` block in `crates/game-core/src/state/game_state.rs`:

```rust
    #[test]
    fn game_state_has_next_enemy_id_counter_starting_at_zero() {
        let state = GameState::default();
        assert_eq!(state.next_enemy_id, 0);
    }

    #[test]
    fn next_enemy_id_round_trips_through_serde() {
        let mut state = GameState::default();
        state.next_enemy_id = 42;
        let json = serde_json::to_string(&state).expect("serialize");
        let back: GameState = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.next_enemy_id, 42);
    }
```

- [ ] **Step 2: Confirm compile failure**

```bash
cargo test -p game-core next_enemy_id 2>&1 | head -20
```

Expected: `no field next_enemy_id on GameState`.

- [ ] **Step 3: Add the field**

In `crates/game-core/src/state/game_state.rs`, inside `pub struct GameState { ... }`, add the field right after `next_card_instance_id`:

```rust
    /// Monotonic counter for assigning [`EnemyId`]s when enemies
    /// enter play via the encounter deck (see
    /// `crate::engine::dispatch::spawn_enemy`). Starts at 0 and
    /// increments after each assignment; guarantees uniqueness within
    /// a scenario and deterministic ids across replays.
    ///
    /// Distinct from [`next_card_instance_id`](Self::next_card_instance_id)
    /// because [`EnemyId`] and [`CardInstanceId`] are distinct types —
    /// enemies aren't tracked in the `CardInPlay` registry.
    pub next_enemy_id: u32,
```

Update `Default::default()` for `GameState` (if implemented explicitly) so the new field initializes to `0`. If `GameState` uses `#[derive(Default)]`, this is automatic — `u32::default()` is `0`. Check by reading the surrounding code: if a `Default` impl is hand-written, find the construction site (likely at the bottom of `game_state.rs`) and add `next_enemy_id: 0,`.

- [ ] **Step 4: Run the GameState tests, verify pass**

```bash
cargo test -p game-core next_enemy_id
```

Expected: 2 tests pass.

- [ ] **Step 5: Write the failing Event::EnemySpawned test**

Append at the bottom of `crates/game-core/src/event.rs`:

```rust
#[cfg(test)]
mod enemy_spawned_event_tests {
    use super::*;
    use crate::state::{CardCode, EnemyId, InvestigatorId, LocationId};

    #[test]
    fn enemy_spawned_with_engagement_serde_roundtrip() {
        let ev = Event::EnemySpawned {
            enemy: EnemyId(7),
            code: CardCode("_synth_enemy".into()),
            location: LocationId(10),
            engaged_with: Some(InvestigatorId(1)),
        };
        let json = serde_json::to_string(&ev).expect("serialize");
        let back: Event = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, ev);
    }

    #[test]
    fn enemy_spawned_without_engagement_serde_roundtrip() {
        let ev = Event::EnemySpawned {
            enemy: EnemyId(8),
            code: CardCode("_synth_enemy".into()),
            location: LocationId(10),
            engaged_with: None,
        };
        let json = serde_json::to_string(&ev).expect("serialize");
        let back: Event = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, ev);
    }
}
```

- [ ] **Step 6: Confirm compile failure**

```bash
cargo test -p game-core enemy_spawned_event 2>&1 | head -20
```

Expected: `struct variant Event::EnemySpawned has no field code` (or similar).

- [ ] **Step 7: Extend the `Event::EnemySpawned` variant**

In `crates/game-core/src/event.rs`, find the existing `EnemySpawned` variant (around line 169) and replace it with the extended shape:

```rust
    /// An enemy entered play at a location from the encounter deck.
    ///
    /// Emitted by [`spawn_enemy`](crate::engine::dispatch) when an
    /// encounter card resolved as an enemy lands in
    /// [`GameState::enemies`](crate::state::GameState::enemies).
    /// `engaged_with` is `Some(investigator)` when the spawn caused
    /// engagement-on-spawn (Rules Reference p.10) and `None` when the
    /// enemy spawned at an empty location.
    EnemySpawned {
        /// The newly-spawned enemy's stable id (freshly minted from
        /// [`GameState::next_enemy_id`](crate::state::GameState::next_enemy_id)).
        enemy: EnemyId,
        /// Printed code of the spawned enemy.
        code: CardCode,
        /// Where the enemy spawned on the location map.
        location: LocationId,
        /// If the spawn engaged an investigator on arrival, who.
        /// `None` if the enemy spawned at a location with no
        /// investigators.
        engaged_with: Option<InvestigatorId>,
    },
```

If `CardCode` isn't already imported at the top of `event.rs`, add it. Search for the existing imports:

```bash
grep -n "^use " /home/talel/eldritch/crates/game-core/src/event.rs | head -10
```

Add `CardCode` to the existing `crate::state::{...}` use (likely already importing `EnemyId`, `LocationId`, `InvestigatorId` — append `CardCode` to the list).

- [ ] **Step 8: Run the tests, verify pass**

```bash
cargo test -p game-core enemy_spawned_event
```

Expected: 2 tests pass.

- [ ] **Step 9: Fix any exhaustive matches the compiler flags**

```bash
cargo check --all --all-features 2>&1 | grep -E "non-exhaustive|error" | head -20
```

`Event` is `#[non_exhaustive]`, but matches that bind to `EnemySpawned`'s old field set will fail to compile. Grep confirmed earlier that no code matches against `Event::EnemySpawned`, so no fixes expected. If the check surfaces any, add the new fields to those match arms.

- [ ] **Step 10: Full game-core gauntlet**

```bash
RUSTFLAGS="-D warnings" cargo test -p game-core --all-features
cargo clippy -p game-core --all-targets --all-features -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc -p game-core --no-deps --all-features
```

Expected: all green.

- [ ] **Step 11: Commit**

```bash
git add crates/game-core/src/state/game_state.rs crates/game-core/src/event.rs
git commit -m "$(cat <<'EOF'
engine: add next_enemy_id counter + extend Event::EnemySpawned

next_enemy_id mints stable EnemyIds when encounter-deck enemies
spawn into play. Mirrors next_card_instance_id; counters are
separate because EnemyId and CardInstanceId are distinct types.

Event::EnemySpawned now carries the printed code and engaged_with
(Option<InvestigatorId>) so reaction listeners can filter on spawn
identity / engagement without a separate lookup. No external
consumers — the existing variant was structural only.

Refs #127.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 7: Implement `spawn_enemy` and wire it into `encounter_card_revealed`

**Files:**
- Modify: `crates/game-core/src/engine/dispatch.rs`

This is the load-bearing task. The handler resolves the spawn location, resolves on-spawn engagement, mints the enemy, places it in play, and emits `Event::EnemySpawned`. Then `encounter_card_revealed`'s `CardType::Enemy` arm runs Revelation effects first (no Phase-4 enemy has any) and calls `spawn_enemy`.

- [ ] **Step 1: Write the failing tests for `spawn_enemy`**

Append a new test module at the bottom of `crates/game-core/src/engine/dispatch.rs`:

```rust
#[cfg(test)]
mod spawn_enemy_tests {
    use super::*;
    use crate::state::{CardCode, Location, LocationId, InvestigatorId};
    use crate::test_support::{test_investigator, test_location, TestGame};
    use card_dsl::card_data::{CardMetadata, CardType, Class, SkillIcons, Spawn, SpawnLocation};

    fn synth_enemy_metadata(spawn: Option<Spawn>) -> CardMetadata {
        CardMetadata {
            code: "_synth_enemy".into(),
            name: "Synth Enemy".into(),
            class: Class::Mythos,
            card_type: CardType::Enemy,
            cost: None,
            xp: None,
            text: None,
            flavor: None,
            illustrator: None,
            traits: Vec::new(),
            slots: Vec::new(),
            skill_icons: SkillIcons::default(),
            health: Some(1),
            sanity: None,
            deck_limit: 1,
            quantity: 1,
            pack_code: "_synth".into(),
            position: 1,
            is_fast: false,
            spawn,
        }
    }

    #[test]
    fn spawn_at_specific_location_with_one_investigator_engages_them() {
        let mut loc = test_location(10, "Synth Loc");
        loc.code = CardCode("_synth_loc".into());
        let mut state = TestGame::new()
            .with_investigator(test_investigator(1))
            .with_location(loc)
            .build();
        // Place investigator 1 at location 10.
        state.investigators.get_mut(&InvestigatorId(1)).unwrap().current_location =
            Some(LocationId(10));

        let metadata = synth_enemy_metadata(Some(Spawn {
            location: SpawnLocation::Specific("_synth_loc".into()),
        }));
        let mut events = Vec::new();

        let outcome = spawn_enemy(
            &mut state,
            &mut events,
            InvestigatorId(1),
            CardCode("_synth_enemy".into()),
            &metadata,
        );

        assert!(matches!(outcome, EngineOutcome::Done), "{outcome:?}");
        assert_eq!(state.enemies.len(), 1);
        let (_, enemy) = state.enemies.iter().next().unwrap();
        assert_eq!(enemy.current_location, Some(LocationId(10)));
        assert_eq!(enemy.engaged_with, Some(InvestigatorId(1)));

        assert_event!(
            events,
            Event::EnemySpawned { code, location, engaged_with, .. }
                if *code == CardCode("_synth_enemy".into())
                    && *location == LocationId(10)
                    && *engaged_with == Some(InvestigatorId(1))
        );
    }

    #[test]
    fn spawn_at_specific_location_with_no_investigators_leaves_unengaged() {
        let mut loc = test_location(10, "Synth Loc");
        loc.code = CardCode("_synth_loc".into());
        let mut state = TestGame::new()
            .with_investigator(test_investigator(1))
            .with_location(loc)
            .build();
        // Investigator 1 is NOT at location 10 (current_location is None).

        let metadata = synth_enemy_metadata(Some(Spawn {
            location: SpawnLocation::Specific("_synth_loc".into()),
        }));
        let mut events = Vec::new();

        let outcome = spawn_enemy(
            &mut state,
            &mut events,
            InvestigatorId(1),
            CardCode("_synth_enemy".into()),
            &metadata,
        );

        assert!(matches!(outcome, EngineOutcome::Done), "{outcome:?}");
        let (_, enemy) = state.enemies.iter().next().unwrap();
        assert_eq!(enemy.engaged_with, None);
    }

    #[test]
    fn spawn_at_specific_location_rejects_when_location_not_in_play() {
        let mut state = TestGame::new()
            .with_investigator(test_investigator(1))
            .build();
        let metadata = synth_enemy_metadata(Some(Spawn {
            location: SpawnLocation::Specific("_nonexistent_loc".into()),
        }));
        let mut events = Vec::new();
        let outcome = spawn_enemy(
            &mut state,
            &mut events,
            InvestigatorId(1),
            CardCode("_synth_enemy".into()),
            &metadata,
        );
        match outcome {
            EngineOutcome::Rejected { reason } => {
                assert!(
                    reason.contains("spawn location not in play"),
                    "unexpected reason: {reason:?}",
                );
            }
            other => panic!("expected Rejected, got {other:?}"),
        }
        assert!(state.enemies.is_empty());
    }

    #[test]
    fn spawn_with_no_instruction_places_at_drawing_investigators_location() {
        let mut loc = test_location(10, "Demo");
        loc.code = CardCode("_demo_loc".into());
        let mut state = TestGame::new()
            .with_investigator(test_investigator(1))
            .with_location(loc)
            .build();
        state.investigators.get_mut(&InvestigatorId(1)).unwrap().current_location =
            Some(LocationId(10));
        let metadata = synth_enemy_metadata(None);
        let mut events = Vec::new();

        let outcome = spawn_enemy(
            &mut state,
            &mut events,
            InvestigatorId(1),
            CardCode("_synth_enemy".into()),
            &metadata,
        );

        assert!(matches!(outcome, EngineOutcome::Done), "{outcome:?}");
        let (_, enemy) = state.enemies.iter().next().unwrap();
        assert_eq!(enemy.current_location, Some(LocationId(10)));
        assert_eq!(enemy.engaged_with, Some(InvestigatorId(1)));
    }

    #[test]
    fn spawn_with_no_instruction_rejects_when_drawing_investigator_has_no_location() {
        let mut state = TestGame::new()
            .with_investigator(test_investigator(1))
            .build();
        // Investigator has no current_location.
        let metadata = synth_enemy_metadata(None);
        let mut events = Vec::new();
        let outcome = spawn_enemy(
            &mut state,
            &mut events,
            InvestigatorId(1),
            CardCode("_synth_enemy".into()),
            &metadata,
        );
        match outcome {
            EngineOutcome::Rejected { reason } => {
                assert!(
                    reason.contains("drawing investigator has no location"),
                    "unexpected reason: {reason:?}",
                );
            }
            other => panic!("expected Rejected, got {other:?}"),
        }
    }

    #[test]
    fn spawn_with_multi_investigator_engagement_rejects_until_128() {
        let mut loc = test_location(10, "Crowded");
        loc.code = CardCode("_crowded_loc".into());
        let mut state = TestGame::new()
            .with_investigator(test_investigator(1))
            .with_investigator(test_investigator(2))
            .with_location(loc)
            .build();
        state.investigators.get_mut(&InvestigatorId(1)).unwrap().current_location =
            Some(LocationId(10));
        state.investigators.get_mut(&InvestigatorId(2)).unwrap().current_location =
            Some(LocationId(10));
        let metadata = synth_enemy_metadata(Some(Spawn {
            location: SpawnLocation::Specific("_crowded_loc".into()),
        }));
        let mut events = Vec::new();

        let outcome = spawn_enemy(
            &mut state,
            &mut events,
            InvestigatorId(1),
            CardCode("_synth_enemy".into()),
            &metadata,
        );

        match outcome {
            EngineOutcome::Rejected { reason } => {
                assert!(
                    reason.contains("#128") && reason.contains("Prey"),
                    "unexpected reason: {reason:?}",
                );
            }
            other => panic!("expected Rejected for multi-investigator engagement, got {other:?}"),
        }
        assert!(state.enemies.is_empty(), "no enemy should be placed on reject");
    }

    #[test]
    fn spawn_mints_distinct_enemy_ids() {
        let mut loc = test_location(10, "L");
        loc.code = CardCode("_l".into());
        let mut state = TestGame::new()
            .with_investigator(test_investigator(1))
            .with_location(loc)
            .build();
        state.investigators.get_mut(&InvestigatorId(1)).unwrap().current_location =
            Some(LocationId(10));
        let metadata = synth_enemy_metadata(Some(Spawn {
            location: SpawnLocation::Specific("_l".into()),
        }));
        let mut events = Vec::new();

        let _ = spawn_enemy(
            &mut state,
            &mut events,
            InvestigatorId(1),
            CardCode("_synth_enemy".into()),
            &metadata,
        );
        let _ = spawn_enemy(
            &mut state,
            &mut events,
            InvestigatorId(1),
            CardCode("_synth_enemy".into()),
            &metadata,
        );
        assert_eq!(state.enemies.len(), 2, "two spawns should produce two distinct enemies");
    }
}
```

- [ ] **Step 2: Confirm compile / runtime failure**

```bash
cargo test -p game-core spawn_enemy_tests 2>&1 | head -30
```

Expected: compile failure — `spawn_enemy` function is not defined.

- [ ] **Step 3: Implement `spawn_enemy`**

In `crates/game-core/src/engine/dispatch.rs`, add the handler. Place it just above (or just below) `encounter_card_revealed` so the two related handlers live together.

Imports at the top of the file: verify `Spawn`, `SpawnLocation` are accessible (they're in `card_dsl::card_data` — `game-core` re-exports `card_data` under `crate::card_data`, so `use crate::card_data::{Spawn, SpawnLocation};` or qualify inline as `card_data::Spawn`). Verify with:

```bash
grep -n "use card_data\|use crate::card_data" /home/talel/eldritch/crates/game-core/src/engine/dispatch.rs | head -5
```

Add to the existing card_data import line if needed.

Function body:

```rust
/// Spawn one encounter-deck enemy into play.
///
/// Called by [`encounter_card_revealed`] after `Event::CardRevealed`
/// has fired and any [`Trigger::Revelation`](card_dsl::dsl::Trigger::Revelation)
/// abilities on the enemy have resolved.
///
/// # Spawn-location resolution
///
/// Rules Reference page 24 (1.4 Each investigator draws 1 encounter
/// card):
///
/// > If the encountered enemy has no spawn instruction, the enemy
/// > spawns engaged with the investigator encountering the card and
/// > is placed in that investigator's threat area.
///
/// We model threat-area placement as
/// `enemy.current_location = drawing investigator's location` +
/// `engaged_with = drawing investigator`. The named-location case
/// (`SpawnLocation::Specific`) looks the location up by its
/// printed [`code`](crate::state::Location::code).
///
/// # Engagement-on-spawn
///
/// Rules Reference page 10 (Enemy Engagement):
///
/// > Any time a ready unengaged enemy is at the same location as an
/// > investigator, it engages that investigator, and is placed in
/// > that investigator's threat area. If there are multiple
/// > investigators at the same location as a ready unengaged enemy,
/// > follow the enemy's prey instructions to determine which
/// > investigator is engaged.
///
/// Phase-4 handles the 0- and 1-investigator cases inline. The
/// multi-investigator case requires `Prey` resolution — the same
/// machinery #128 will land for hunter-movement target selection —
/// and rejects here with a reason pointing at #128. #128's author
/// inherits the work of unifying the prey resolver across spawn and
/// hunter-movement.
///
/// # Validate-first contract
///
/// All preconditions (location resolution, engagement resolution) are
/// checked before any mutation. Reject paths leave `state` and
/// `events` unchanged from the caller's perspective; only the happy
/// path inserts into `state.enemies`, bumps `next_enemy_id`, and
/// pushes `Event::EnemySpawned`.
fn spawn_enemy(
    state: &mut GameState,
    events: &mut Vec<Event>,
    investigator: InvestigatorId,
    code: CardCode,
    metadata: &CardMetadata,
) -> EngineOutcome {
    // 1. Resolve spawn location (validate-first).
    let location_id = match &metadata.spawn {
        Some(Spawn {
            location: SpawnLocation::Specific(loc_code),
        }) => match state
            .locations
            .iter()
            .find(|(_, loc)| loc.code.as_str() == loc_code.as_str())
        {
            Some((id, _)) => *id,
            None => {
                return EngineOutcome::Rejected {
                    reason: format!(
                        "spawn_enemy: spawn location not in play (code {loc_code:?})",
                    )
                    .into(),
                };
            }
        },
        None => match state
            .investigators
            .get(&investigator)
            .and_then(|inv| inv.current_location)
        {
            Some(loc) => loc,
            None => {
                return EngineOutcome::Rejected {
                    reason: format!(
                        "spawn_enemy: drawing investigator has no location \
                         (investigator {investigator:?})",
                    )
                    .into(),
                };
            }
        },
    };

    // 2. Resolve engagement-on-spawn (validate-first).
    let investigators_at_loc: Vec<InvestigatorId> = state
        .investigators
        .iter()
        .filter(|(_, inv)| inv.current_location == Some(location_id))
        .map(|(id, _)| *id)
        .collect();
    let engaged_with = match investigators_at_loc.as_slice() {
        [] => None,
        [single] => Some(*single),
        _ => {
            return EngineOutcome::Rejected {
                reason:
                    "spawn_enemy: multi-investigator engagement-on-spawn requires Prey \
                     (lands in #128)"
                        .into(),
            };
        }
    };

    // 3. Mint and place (mutate-second).
    let enemy_id = EnemyId(state.next_enemy_id);
    state.next_enemy_id = state.next_enemy_id.saturating_add(1);

    let enemy = Enemy {
        id: enemy_id,
        name: metadata.name.clone(),
        fight: 1,
        evade: 1,
        max_health: metadata.health.unwrap_or(1),
        damage: 0,
        attack_damage: 0,
        attack_horror: 0,
        current_location: Some(location_id),
        exhausted: false,
        traits: metadata.traits.clone(),
        engaged_with,
    };
    state.enemies.insert(enemy_id, enemy);

    events.push(Event::EnemySpawned {
        enemy: enemy_id,
        code,
        location: location_id,
        engaged_with,
    });

    EngineOutcome::Done
}
```

Notes on the body:
- `metadata.spawn` is `Option<Spawn>`; `Spawn` and `SpawnLocation` come from `card_data`.
- Stats (`fight: 1`, `evade: 1`, `attack_damage: 0`, `attack_horror: 0`) are placeholders — `CardMetadata` doesn't yet carry per-enemy fight/evade/attack-damage/attack-horror fields. Those land with a future PR that adds enemy-specific metadata. For now, hardcoded defaults are fine: the synthetic enemy doesn't enter combat in this PR, and the corpus's spawn-bearing enemies are all stubbed `spawn: None` so won't reach this path in production code today. **Document this as an explicit limitation** in the doc comment.
- `metadata.health.unwrap_or(1)` — enemy metadata has `health: Option<u8>` already.

Add a one-paragraph "Per-enemy stat fields not yet on `CardMetadata`" note in the doc comment above `spawn_enemy`:

```rust
/// # Stat fields TODO
///
/// `CardMetadata` doesn't yet carry per-enemy `fight` / `evade` /
/// `attack_damage` / `attack_horror`. This handler hardcodes
/// `fight: 1, evade: 1, attack_damage: 0, attack_horror: 0` until
/// a future PR (Phase-7+, alongside the first real spawn-bearing
/// enemy) extends `CardMetadata` with enemy-specific stat fields and
/// this handler reads them. Health uses `metadata.health.unwrap_or(1)`
/// because `CardMetadata.health` already exists.
```

- [ ] **Step 4: Wire the handler into `encounter_card_revealed`**

In `crates/game-core/src/engine/dispatch.rs`, find the existing `CardType::Enemy` reject arm in `encounter_card_revealed` (line ~309–311):

```rust
        CardType::Enemy => EngineOutcome::Rejected {
            reason: "EncounterCardRevealed: encounter enemy spawn lands in #127".into(),
        },
```

Replace it with the real wiring — Revelation effects first, then spawn:

```rust
        CardType::Enemy => {
            // Revelation effects on enemies (rare, but printed on
            // some encounter enemies — e.g. "Revelation - Discard
            // 1 card from your hand at random.") fire BEFORE the
            // enemy spawns into play, per Rules Reference p.24:
            // "1. Resolve any 'Revelation' effects on that card.
            //  2. If it is an Enemy: spawn instructions / default
            //     engagement."
            //
            // No Phase-4-scope enemy has a Revelation effect; this
            // loop is structural for Phase-7+ enemies.
            let abilities = (registry.abilities_for)(&code).unwrap_or_default();
            let ctx = EvalContext::for_controller(investigator);
            for ability in abilities
                .iter()
                .filter(|a| a.trigger == Trigger::Revelation)
            {
                let outcome = apply_effect(state, events, &ability.effect, ctx);
                if !matches!(outcome, EngineOutcome::Done) {
                    return outcome;
                }
            }
            spawn_enemy(state, events, investigator, code, metadata)
        }
```

Two things to verify when implementing:
1. The `metadata` binding in `encounter_card_revealed` is the same `&CardMetadata` already obtained at line 279 — re-use it; don't look up again.
2. The signature `apply_effect(state, events, effect, ctx)` matches the existing treachery arm (line ~301). If `EvalContext::for_controller` isn't already in scope at this site, leave it — the treachery arm already uses it, so the import is there.

- [ ] **Step 5: Run the unit tests, verify pass**

```bash
cargo test -p game-core spawn_enemy_tests
```

Expected: 7 tests pass.

Also re-run the encounter_card_revealed tests to confirm the wiring didn't regress them:

```bash
cargo test -p game-core encounter_card_revealed_tests
```

Expected: the existing registry-missing test still passes. (The "enemy lands in #127" reject test, if present, will need updating — search for `"lands in #127"` in dispatch.rs's test module and replace its expected-reason assertion with one of the new spawn-arm test patterns, or just remove the obsolete test if it was a placeholder.)

```bash
grep -n "lands in #127" /home/talel/eldritch/crates/game-core/src/engine/dispatch.rs
```

If any matching unit test exists in `dispatch.rs`, delete it — the spawn arm tests above cover the same surface with stronger assertions.

- [ ] **Step 6: Full game-core gauntlet**

```bash
RUSTFLAGS="-D warnings" cargo test -p game-core --all-features
cargo clippy -p game-core --all-targets --all-features -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc -p game-core --no-deps --all-features
```

Expected: all green. Watch for:
- Clippy: `too_many_lines` on `spawn_enemy` — if so, allow it inline with `#[allow(clippy::too_many_lines)]` above the function (the existing dispatch handlers already use this pattern).
- Doc warnings on intra-doc links — fall back to inline-code if needed.

- [ ] **Step 7: Commit**

```bash
git add crates/game-core/src/engine/dispatch.rs
git commit -m "$(cat <<'EOF'
engine: spawn_enemy handler + wire into encounter_card_revealed

Replaces the #126 enemy-arm reject stub with the real handler.
Resolves spawn location (Specific by code, or drawing investigator's
location when spawn is None), resolves engagement-on-spawn (0 / 1
investigators handled here; multi-investigator rejects pointing at
#128 for Prey), mints a fresh EnemyId, places the enemy, and emits
Event::EnemySpawned.

Revelation effects on enemies fire BEFORE spawn, per Rules Reference
p.24 ordering. No Phase-4-scope enemy has Revelation; the loop is
structural for Phase-7+.

Stat fields (fight/evade/attack_damage/attack_horror) are hardcoded
defaults until CardMetadata grows enemy-specific stat fields — see
the handler's doc comment for the deferral note.

Refs #127.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 8: Add synthetic enemy + `SYNTH_LOC_CODE` to the test fixture

**Files:**
- Modify: `crates/scenarios/src/test_fixtures/synth_cards.rs`
- Modify: `crates/scenarios/src/test_fixtures/synthetic.rs`

- [ ] **Step 1: Write the failing tests in `synth_cards.rs`**

Open `crates/scenarios/src/test_fixtures/synth_cards.rs`. In the existing `#[cfg(test)] mod tests` block at the bottom, append:

```rust
    #[test]
    fn metadata_for_resolves_synth_enemy() {
        let code = CardCode(SYNTH_ENEMY_CODE.into());
        let meta = metadata_for(&code).expect("synth enemy must resolve");
        assert_eq!(meta.code, SYNTH_ENEMY_CODE);
        assert_eq!(meta.card_type, game_core::card_data::CardType::Enemy);
        let spawn = meta.spawn.as_ref().expect("synth enemy must carry a spawn rule");
        match &spawn.location {
            game_core::card_data::SpawnLocation::Specific(code) => {
                assert_eq!(code, SYNTH_LOC_CODE);
            }
        }
    }

    #[test]
    fn abilities_for_synth_enemy_returns_none() {
        let code = CardCode(SYNTH_ENEMY_CODE.into());
        assert!(abilities_for(&code).is_none());
    }
```

- [ ] **Step 2: Confirm compile failure**

```bash
cargo test -p scenarios --features test_fixtures synth_enemy 2>&1 | head -20
```

Expected: `cannot find value SYNTH_ENEMY_CODE / SYNTH_LOC_CODE in this scope`.

- [ ] **Step 3: Add `SYNTH_LOC_CODE`, `SYNTH_ENEMY_CODE`, and the synth-enemy metadata**

In `crates/scenarios/src/test_fixtures/synth_cards.rs`, add at the top of the constants section (right after `SYNTH_TREACHERY_CODE`):

```rust
/// Code for the synthetic location used by [`synth_enemy`]'s spawn
/// rule. Underscore prefix guarantees no collision with `ArkhamDB`'s
/// digit-prefixed real codes. Referenced from
/// [`crate::test_fixtures::synthetic::setup`] when stamping the demo
/// location's `code` field.
pub const SYNTH_LOC_CODE: &str = "_synth_loc";

/// Code for the synthetic spawn-bearing enemy.
///
/// Carries `SpawnLocation::Specific(SYNTH_LOC_CODE)` so the on-draw
/// path's enemy arm has something to spawn during the integration
/// test in `crates/scenarios/tests/encounter_spawn.rs`. No abilities
/// (no Revelation, no Activated triggers) — the proof we need is
/// "enemy spawns at the right location, engages the right
/// investigator," not anything ability-driven.
pub const SYNTH_ENEMY_CODE: &str = "_synth_enemy";
```

Then add the enemy's metadata constructor + static below the existing treachery's:

```rust
fn synth_enemy_metadata() -> CardMetadata {
    CardMetadata {
        code: SYNTH_ENEMY_CODE.to_owned(),
        name: "Synthetic Enemy".to_owned(),
        class: Class::Mythos,
        card_type: CardType::Enemy,
        cost: None,
        xp: None,
        text: Some(
            "Spawn: Synthetic Location. (Synthetic; not a printed card.)".to_owned(),
        ),
        flavor: None,
        illustrator: None,
        traits: Vec::new(),
        slots: Vec::new(),
        skill_icons: SkillIcons {
            willpower: 0,
            intellect: 0,
            combat: 0,
            agility: 0,
            wild: 0,
        },
        health: Some(1),
        sanity: None,
        deck_limit: 1,
        quantity: 1,
        pack_code: "_synth".to_owned(),
        position: 2,
        is_fast: false,
        spawn: Some(game_core::card_data::Spawn {
            location: game_core::card_data::SpawnLocation::Specific(
                SYNTH_LOC_CODE.to_owned(),
            ),
        }),
    }
}

fn synth_enemy_metadata_static() -> &'static CardMetadata {
    static M: OnceLock<CardMetadata> = OnceLock::new();
    M.get_or_init(synth_enemy_metadata)
}
```

(Import `Spawn` / `SpawnLocation` at the top of the file if you prefer unqualified usage — the existing import line is `use game_core::card_data::{CardMetadata, CardType, Class, SkillIcons};`. Append `, Spawn, SpawnLocation` to that list and drop the inline qualification.)

- [ ] **Step 4: Extend `metadata_for` and `abilities_for` to recognize the new code**

Replace the existing `metadata_for` body with a `match`:

```rust
fn metadata_for(code: &CardCode) -> Option<&'static CardMetadata> {
    match code.as_str() {
        SYNTH_TREACHERY_CODE => Some(synth_treachery_metadata_static()),
        SYNTH_ENEMY_CODE => Some(synth_enemy_metadata_static()),
        _ => None,
    }
}
```

`abilities_for` already uses a match; add an arm:

```rust
fn abilities_for(code: &CardCode) -> Option<Vec<Ability>> {
    match code.as_str() {
        SYNTH_TREACHERY_CODE => Some(vec![revelation(gain_resources(
            InvestigatorTarget::Controller,
            1,
        ))]),
        // SYNTH_ENEMY_CODE intentionally returns None — the synthetic
        // enemy has no Revelation effect; the spawn handler is the
        // only thing exercised by the integration test.
        _ => None,
    }
}
```

- [ ] **Step 5: Update the synthetic location's `code` in `synthetic.rs`**

In `crates/scenarios/src/test_fixtures/synthetic.rs`, modify `setup()` (deferred from Task 5):

```rust
pub fn setup() -> GameState {
    let mut location = test_location(10, "Demo Location");
    location.code = CardCode(super::synth_cards::SYNTH_LOC_CODE.into());

    let mut state = TestGame::new()
        .with_investigator(test_investigator(1))
        .with_location(location)
        .with_turn_order([InvestigatorId(1)])
        .with_scenario_id(ScenarioId::new(ID))
        .build();
    state
        .encounter_deck
        .push_back(CardCode(SYNTH_TREACHERY_CODE.into()));
    state
}
```

Update the doc comment to mention the canonical synth-location code:

```rust
/// Build the initial [`GameState`] for this fixture: one
/// investigator, one location (with `code` set to
/// [`synth_cards::SYNTH_LOC_CODE`]), `scenario_id` set, `turn_order`
/// populated, encounter deck seeded with one copy of
/// [`synth_cards::SYNTH_TREACHERY_CODE`]. Phase = Mythos, round =
/// 0 — ready for
/// [`PlayerAction::StartScenario`](game_core::PlayerAction::StartScenario).
///
/// The encounter-deck seeding gives the #126 / #127 integration
/// tests something to draw from; integration tests that want to
/// exercise spawn-bearing enemy reveals push the synthetic enemy
/// code (`synth_cards::SYNTH_ENEMY_CODE`) onto the deck themselves
/// after calling `setup()`.
///
/// [`synth_cards::SYNTH_LOC_CODE`]: super::synth_cards::SYNTH_LOC_CODE
/// [`synth_cards::SYNTH_TREACHERY_CODE`]: super::synth_cards::SYNTH_TREACHERY_CODE
/// [`synth_cards::SYNTH_ENEMY_CODE`]: super::synth_cards::SYNTH_ENEMY_CODE
```

- [ ] **Step 6: Run the synth_cards tests, verify pass**

```bash
cargo test -p scenarios --features test_fixtures synth_cards
```

Expected: existing 4 tests + 2 new tests = 6 pass.

- [ ] **Step 7: Re-run the existing scenarios integration tests**

```bash
cargo test -p scenarios --test synthetic_resolution
cargo test -p scenarios --test encounter_reveal
```

Expected: both still pass. `encounter_reveal` is the #126 integration test — confirms the treachery path didn't regress.

- [ ] **Step 8: Full scenarios gauntlet**

```bash
RUSTFLAGS="-D warnings" cargo test -p scenarios --all-features
cargo clippy -p scenarios --all-targets --all-features -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc -p scenarios --no-deps --all-features
```

Expected: all green.

- [ ] **Step 9: Commit**

```bash
git add crates/scenarios/src/test_fixtures/synth_cards.rs crates/scenarios/src/test_fixtures/synthetic.rs
git commit -m "$(cat <<'EOF'
test: add synthetic spawn-bearing enemy + canonical synth loc code

Adds SYNTH_LOC_CODE = "_synth_loc" and SYNTH_ENEMY_CODE =
"_synth_enemy" to the synth_cards fixture, plus the enemy's
CardMetadata carrying Spawn { location: Specific(SYNTH_LOC_CODE) }.

The demo location's `code` is now stamped with SYNTH_LOC_CODE in
synthetic::setup so spawn-by-code resolution can find it during the
encounter_spawn.rs integration test.

abilities_for returns None for the synth enemy — no Revelation
effect; the spawn handler is what the test exercises.

Refs #127.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 9: Integration test (`crates/scenarios/tests/encounter_spawn.rs`)

**Files:**
- Create: `crates/scenarios/tests/encounter_spawn.rs`

This file is its own cargo binary — fresh process, can install `TEST_REGISTRY` without colliding with other test binaries.

- [ ] **Step 1: Write the integration test**

Create `crates/scenarios/tests/encounter_spawn.rs`:

```rust
//! End-to-end test of the spawn-on-reveal path (#127).
//!
//! Installs the synthetic `TEST_REGISTRY` (same registry used by
//! `encounter_reveal.rs`) so the on-draw path resolves against the
//! synthetic enemy code rather than a real corpus card. The test
//! exercises:
//!
//! - Happy path: revealing the synthetic enemy from the encounter
//!   deck emits `Event::CardRevealed` (kind Enemy), then
//!   `Event::EnemySpawned` at the right location, engaged with the
//!   drawing investigator. The enemy lands in `state.enemies` and
//!   does NOT appear in `encounter_discard`.
//! - Default-spawn path: a synth enemy with `spawn: None` spawns at
//!   the drawing investigator's location and engages them. (Tested
//!   by stripping the metadata-served spawn rule via the registry
//!   override — see test body.)
//!     [Defer: the synth enemy's metadata always carries a Spawn.
//!      Default-spawn coverage lives in `spawn_enemy_tests` in
//!      `dispatch.rs` instead, where we control the metadata
//!      inline. Skip in this binary.]
//! - Multi-investigator reject: two investigators at the spawn
//!   location → the spawn rejects with a reason naming #128.
//! - Spawn-location-not-in-play reject: synth enemy whose
//!   `Specific` target is a code not present → reject cleanly.
//!     [Same as above — covered by the dispatch unit tests, no need
//!      to repeat at the integration layer.]
//!
//! Lives in `crates/scenarios/tests/` because the `cards`-crate
//! dependency direction prevents game-core tests from constructing
//! real card-shaped registries, and because `card_registry::install`
//! is process-global — an integration test binary gets its own
//! process, so this install doesn't collide with `cards::REGISTRY`
//! installs in other test binaries (e.g.
//! `crates/cards/tests/play_card.rs`).

use std::sync::Once;

use game_core::action::EngineRecord;
use game_core::card_data::CardType;
use game_core::engine::{apply, EngineOutcome};
use game_core::event::Event;
use game_core::state::{CardCode, InvestigatorId, LocationId};
use game_core::{assert_event, assert_event_sequence, Action};
use scenarios::test_fixtures::synth_cards::{
    SYNTH_ENEMY_CODE, SYNTH_LOC_CODE, TEST_REGISTRY,
};
use scenarios::test_fixtures::synthetic;

static INSTALL: Once = Once::new();

fn install_test_registry() {
    INSTALL.call_once(|| {
        let _ = game_core::card_registry::install(TEST_REGISTRY);
    });
}

#[test]
fn revealing_synth_enemy_spawns_at_specific_location_engaged_with_drawer() {
    install_test_registry();
    let mut state = synthetic::setup();
    // Place the drawing investigator at the synth location so the
    // engagement-on-spawn resolves to them.
    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .unwrap()
        .current_location = Some(LocationId(10));
    // Replace the seeded treachery on top of the deck with the synth
    // enemy.
    state.encounter_deck.clear();
    state
        .encounter_deck
        .push_back(CardCode(SYNTH_ENEMY_CODE.into()));

    let result = apply(
        state,
        Action::Engine(EngineRecord::EncounterCardRevealed {
            investigator: InvestigatorId(1),
        }),
    );

    assert_eq!(result.outcome, EngineOutcome::Done);

    // CardRevealed (Enemy) fires first; EnemySpawned follows.
    assert_event_sequence!(
        result.events,
        Event::CardRevealed { card_type, code, .. }
            if *card_type == CardType::Enemy
                && *code == CardCode(SYNTH_ENEMY_CODE.into()),
        Event::EnemySpawned { code, location, engaged_with, .. }
            if *code == CardCode(SYNTH_ENEMY_CODE.into())
                && *location == LocationId(10)
                && *engaged_with == Some(InvestigatorId(1)),
    );

    // Enemy is in play.
    assert_eq!(
        result.state.enemies.len(),
        1,
        "exactly one enemy should be in play after spawn",
    );
    let enemy = result.state.enemies.values().next().unwrap();
    assert_eq!(enemy.current_location, Some(LocationId(10)));
    assert_eq!(enemy.engaged_with, Some(InvestigatorId(1)));

    // Enemy is NOT in encounter_discard (enemies stay in play; only
    // treacheries discard after Revelation).
    assert!(
        !result
            .state
            .encounter_discard
            .contains(&CardCode(SYNTH_ENEMY_CODE.into())),
        "spawned enemy must not appear in encounter_discard",
    );

    // Sanity: the synth location's code is what spawn_enemy looked up.
    let loc = result.state.locations.get(&LocationId(10)).unwrap();
    assert_eq!(loc.code, CardCode(SYNTH_LOC_CODE.into()));
}

#[test]
fn revealing_synth_enemy_with_two_investigators_at_loc_rejects_pointing_at_128() {
    install_test_registry();
    let mut state = synthetic::setup();
    // Add a second investigator at the same location.
    let mut inv2 = game_core::test_support::test_investigator(2);
    inv2.current_location = Some(LocationId(10));
    state.investigators.insert(InvestigatorId(2), inv2);
    state.turn_order.push(InvestigatorId(2));
    // First investigator also at LocationId(10).
    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .unwrap()
        .current_location = Some(LocationId(10));

    // Swap deck to the synth enemy.
    state.encounter_deck.clear();
    state
        .encounter_deck
        .push_back(CardCode(SYNTH_ENEMY_CODE.into()));

    let result = apply(
        state,
        Action::Engine(EngineRecord::EncounterCardRevealed {
            investigator: InvestigatorId(1),
        }),
    );

    match result.outcome {
        EngineOutcome::Rejected { reason } => {
            assert!(
                reason.contains("#128") && reason.contains("Prey"),
                "unexpected reject reason: {reason:?}",
            );
        }
        other => panic!("expected Rejected, got {other:?}"),
    }

    // No enemy placed.
    assert!(
        result.state.enemies.is_empty(),
        "no enemy should be in play on multi-investigator reject",
    );
    // Card was drawn off the top BEFORE the reject (validate-first
    // exception is documented in encounter_card_revealed).
    assert!(
        result.state.encounter_deck.is_empty(),
        "encounter card was drawn before the engagement reject",
    );
}
```

A note on the two "Defer" blocks in the file header: the default-spawn case and the spawn-location-not-in-play case are exercised by the `spawn_enemy_tests` unit tests in `dispatch.rs` (Task 7), where we can construct synth metadata inline. The integration test focuses on the wire-up that the unit tests can't reach: the full `apply()` → `encounter_card_revealed` → `spawn_enemy` pipeline with a real registry installed.

- [ ] **Step 2: Run the integration test, verify pass**

```bash
cargo test -p scenarios --test encounter_spawn
```

Expected: 2 tests pass.

- [ ] **Step 3: Full workspace gauntlet (CI parity)**

```bash
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
cargo build -p web --target wasm32-unknown-unknown
```

Expected: all five jobs green. This is the same gauntlet CI runs — if everything passes here, the PR's CI should pass too.

- [ ] **Step 4: Commit**

```bash
git add crates/scenarios/tests/encounter_spawn.rs
git commit -m "$(cat <<'EOF'
test: end-to-end encounter-enemy spawn integration test

New cargo binary at crates/scenarios/tests/encounter_spawn.rs.
Installs TEST_REGISTRY (process-isolated; doesn't collide with
cards::REGISTRY installs in other test binaries). Exercises:
- Happy path: synth enemy reveals → CardRevealed (Enemy) +
  EnemySpawned at the specific location, engaged with the drawer.
- Multi-investigator engagement-on-spawn rejects with a reason
  naming #128.

Default-spawn and location-not-in-play cases live in dispatch.rs's
spawn_enemy_tests, where we can construct synth metadata inline.

Refs #127.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 10: Update the Phase-4 doc as the final commit

**Files:**
- Modify: `docs/phases/phase-4-scenario-plumbing.md`

Per CLAUDE.md's PR procedure step 6: the phase doc update is the LAST commit on the branch — only after the PR number is known (so we know the entry to write), all review-driven fixes are folded in, and scope is settled.

For the plan, write the edits as-of "PR #N" — the actual PR number gets filled in once `gh pr create` returns it. **Open the PR first (Task 11), then come back and run this task with the real number.**

- [ ] **Step 1: Find the PR number**

After Task 11 (`gh pr create`) returns, capture the PR number — e.g. `#134`. Substitute `<PR>` below.

- [ ] **Step 2: Move `#127` from Open → Closed**

In `docs/phases/phase-4-scenario-plumbing.md`, find the "Issues" table. Delete the row for `#127`:

```markdown
| `#127` | enemy spawn rules (`Spawn { location: SpawnLocation }`, engagement-on-spawn, `EventPattern::EnemySpawned`) | Split out of `#69`. First consumer is a synthetic spawn-bearing enemy. |
```

Add a new row to the "Closed" table (immediately above `#103`'s entry, since closed rows are in newest-first order):

```markdown
| `#127` | enemy spawn rules | #<PR> | `Spawn { location: SpawnLocation }` + `Option<Spawn>` on `CardMetadata`; engagement-on-spawn for 0 / 1 investigators (multi rejects, pointing at #128); `EventPattern::EnemySpawned` (bare); `Event::EnemySpawned` extended with `code` + `engaged_with`; new `state.next_enemy_id` counter; `Location.code: CardCode` field; pipeline emits `spawn: None` for every card; synthetic enemy in `synth_cards.rs` proves the wiring end-to-end. |
```

- [ ] **Step 3: Flip the Ordering / Arc row**

In the "Ordering (Shape B)" table, find row 5:

```markdown
| 5 | `#127` enemy spawn rules | First consumer is a synthetic spawn-bearing card. |
```

Change to:

```markdown
| 5 | `#127` enemy spawn rules | ✅ PR #<PR>. First consumer is a synthetic spawn-bearing card. Establishes the `Spawn` keyword surface + `spawn_enemy` handler that #69's Mythos draw loop calls into via `encounter_card_revealed`. |
```

- [ ] **Step 4: Add Decision entries**

Below the existing `#126` decision entries in "Decisions made," add:

```markdown
- **Multi-investigator engagement-on-spawn defers to #128 (`#127`, PR #<PR>).** Single-investigator and zero-investigator cases handled per Rules Reference p.10 ("Any time a ready unengaged enemy is at the same location as an investigator, it engages that investigator") and p.24 (default-spawn fallback). The multi-investigator path requires Prey resolution — which shares its shape with hunter-movement target selection in #128 — so rather than build a single-use prey resolver here, engagement-on-spawn rejects with `"spawn_enemy: multi-investigator engagement-on-spawn requires Prey (lands in #128)"`. #128's author inherits the work of unifying the prey resolver across spawn and hunter-movement.

- **Default spawn (`spawn: None`) goes to drawing investigator's location (`#127`, PR #<PR>).** Per Rules Reference p.24: "If the encountered enemy has no spawn instruction, the enemy spawns engaged with the investigator encountering the card and is placed in that investigator's threat area." We model threat-area placement as `enemy.current_location = drawing investigator's location` + `engaged_with = drawing investigator` (via the same engagement resolution as Specific spawns). Future spawn-keyword expansions need to know this is the no-instruction fallback.

- **`SpawnLocation::Specific(String)` reuses `CardCode`'s namespace (`#127`, PR #<PR>).** Spec's open question asked whether to add a distinct `LocationCode` newtype. Resolved: don't — locations in Arkham are cards with `ArkhamDB` codes, the namespace is shared at the data level, and introducing a newtype now would only block accidental cross-use at the engine level with no concrete consumer asking for the distinction. If a later PR has a struct that holds both kinds and needs the compiler to distinguish them, the newtype can land then. (Engineering-only constraint, not a rules-driven one — load-bearing because future spawn-related field designs inherit the shared-namespace assumption.)

- **`Location.code: CardCode` is a required field (`#127`, PR #<PR>).** Locations now carry their printed `ArkhamDB` code alongside the in-scenario `id`. `test_location` defaults the code to `"_test_loc_{id}"` (underscore prefix can't collide with real codes); production scenario setup populates it explicitly from the location's metadata. The new field is the addressable identifier for `SpawnLocation::Specific` lookup — a #128-style "move enemy toward location code X" effect or a #56 Study-implementation would use the same field.

- **`state.next_enemy_id: u32` mints fresh enemy ids (`#127`, PR #<PR>).** First engine site that spawns enemies (existing tests place by hard-coded `EnemyId(...)`). Adding a separate counter avoids conflating "enemy in play" and "card in play" id spaces — `EnemyId` and `CardInstanceId` are distinct types, mirrored by distinct counters. Initializes to 0 via `Default`; bumped via `saturating_add(1)` after every mint (same pattern as `next_card_instance_id`).

- **`Event::EnemySpawned` carries `code` + `engaged_with` denormalized (`#127`, PR #<PR>).** Listeners can filter on the spawned card and the engagement state without a separate registry lookup or `state.enemies` scan. No companion `Event::EnemyEngaged` fires for the engagement-on-spawn case — when the first real "after an enemy engages an investigator" listener lands (Phase-7+), that PR adds the separate event and re-evaluates whether to fire it from `spawn_enemy` as well.

- **Per-enemy stat fields on `CardMetadata` deferred (`#127`, PR #<PR>).** `spawn_enemy` hardcodes `fight: 1, evade: 1, attack_damage: 0, attack_horror: 0` because `CardMetadata` doesn't yet carry those fields — `health` is the only enemy stat present. The first Phase-7+ PR that needs combat-relevant spawned enemies inherits the work of extending `CardMetadata` and threading the new fields through `spawn_enemy`. Synthetic enemy + Phase-4 demo don't enter combat, so the deferral is safe.
```

- [ ] **Step 5: Remove the settled open question**

The phase doc's "Open questions" section has no current entry that #127 settles (the multi-investigator case stays open via #128 — re-checked at plan-write time). Skip this step. If the doc has acquired a new open question between plan-write and PR-open that #127 settles, drop it.

- [ ] **Step 6: Commit the phase-doc update**

```bash
git add docs/phases/phase-4-scenario-plumbing.md
git commit -m "$(cat <<'EOF'
docs: update phase-4 doc for #127 (enemy spawn rules)

Move #127 from Open → Closed, flip ordering row 5 to ✅ PR #<PR>,
add Decision entries for:
- multi-investigator engagement-on-spawn deferring to #128
- default-spawn going to drawing investigator's location
- SpawnLocation::Specific reusing CardCode's namespace
- Location.code as a new required field
- state.next_enemy_id as a fresh counter
- Event::EnemySpawned carrying code + engaged_with denormalized
- per-enemy stat fields on CardMetadata deferred to Phase 7+

Refs #127.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

- [ ] **Step 7: Push the phase-doc commit**

```bash
git push
```

CI will re-run on the new commit; the gauntlet is already green from Task 9 so this should be a no-op for verification but a yes-op for the human reviewer who'll see the final phase-doc state.

---

## Task 11: Open the PR

**Files:**
- (none)

- [ ] **Step 1: Push the branch**

```bash
git push -u origin engine/enemy-spawn-rules
```

- [ ] **Step 2: Open the PR**

```bash
gh pr create --title "engine: enemy spawn rules" --body "$(cat <<'EOF'
## Summary

Lands the encounter-enemy spawn pipeline:

- `Spawn` / `SpawnLocation` types + `spawn: Option<Spawn>` field on `CardMetadata`.
- `EventPattern::EnemySpawned` (bare; narrowing fields land with the first real listener).
- `Event::EnemySpawned` extended with `code` + `engaged_with` (no current consumers; safe to extend in place).
- `spawn_enemy` handler in `engine/dispatch.rs`, wired into `encounter_card_revealed`'s `CardType::Enemy` arm (replacing the #126 reject stub).
- `Location.code: CardCode` field (new) + `state.next_enemy_id: u32` counter (new).
- Pipeline regeneration of `crates/cards/src/generated/cards.rs` with `spawn: None` on every card.
- Synthetic spawn-bearing enemy (`SYNTH_ENEMY_CODE`) + canonical synth location code (`SYNTH_LOC_CODE`) in `synth_cards.rs`.
- Integration test (`crates/scenarios/tests/encounter_spawn.rs`) exercising the happy path + multi-investigator reject.

## Rules-reference citations

- **Engagement-on-spawn**, Rules Reference p.10: "Any time a ready unengaged enemy is at the same location as an investigator, it engages that investigator, and is placed in that investigator's threat area. If there are multiple investigators at the same location as a ready unengaged enemy, follow the enemy's prey instructions to determine which investigator is engaged."
- **Default spawn / encounter-card resolution order**, Rules Reference p.24: "If the encountered enemy has no spawn instruction, the enemy spawns engaged with the investigator encountering the card and is placed in that investigator's threat area." And: "1. Resolve any 'Revelation' effects on that card. 2. If it is an Enemy: …spawn instructions / default engagement…"

Both are quoted verbatim in `spawn_enemy`'s doc comment and in the relevant Phase-4 phase-doc Decision entries.

## Design notes

- `SpawnLocation::Specific(String)` reuses the `CardCode` namespace rather than introducing a new `LocationCode` newtype. Locations in Arkham are cards; the namespace is shared at the data level. YAGNI on the type-level distinction until a concrete consumer asks for it.
- `Location.code` is required (not `Option`). Every location at scenario setup time has a printed code; the synthetic fixture's location stamps `SYNTH_LOC_CODE` explicitly, and `test_location` defaults to `"_test_loc_{id}"`.
- Multi-investigator engagement-on-spawn rejects pointing at #128. #128's hunter-movement work shares the `Prey` resolution machinery; this PR's reject defers that work to #128 rather than build a single-use resolver.
- `Event::EnemySpawned` carries `code` and `engaged_with` denormalized. No companion `Event::EnemyEngaged` fires for the on-spawn engagement — first real "after an enemy engages" listener (Phase-7+) gets to add it.
- `spawn_enemy` hardcodes `fight: 1, evade: 1, attack_damage: 0, attack_horror: 0` because `CardMetadata` doesn't yet carry per-enemy combat stats. Synthetic enemy + Phase-4 demo don't enter combat; the first Phase-7+ PR with a combat-relevant spawned enemy extends `CardMetadata`.

## Test plan

- [x] Per-handler unit tests in `spawn_enemy_tests`: specific spawn engages drawer, zero-investigator location leaves unengaged, location-not-in-play rejects, default spawn places at drawer's location, no-location-for-drawer rejects, multi-investigator rejects pointing at #128, distinct ids minted.
- [x] Integration test in `crates/scenarios/tests/encounter_spawn.rs`: full `apply()` happy path + multi-investigator reject.
- [x] `RUSTFLAGS="-D warnings" cargo test --all --all-features` green.
- [x] `cargo clippy --all-targets --all-features -- -D warnings` green.
- [x] `cargo fmt --check` green.
- [x] `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features` green.
- [x] `cargo build -p web --target wasm32-unknown-unknown` green.

Closes #127.
EOF
)"
```

- [ ] **Step 3: Capture the PR number**

`gh pr create` prints a URL with the PR number. Note it (e.g. `#134`). This is the `<PR>` substitution for Task 10's phase-doc commit, which is the last thing pushed before merge.

- [ ] **Step 4: Hand off to the human**

The remainder is human-driven per CLAUDE.md's PR procedure: CI watch + addressing failures, user approval, `gh pr merge <PR> --squash --delete-branch`, post-merge sync. The plan is done.

---

## Self-review checklist (run before saving)

**Spec coverage:**

- ✅ `Spawn` + `SpawnLocation::Specific` on `CardMetadata` — Task 2.
- ✅ `EventPattern::EnemySpawned` bare — Task 4.
- ✅ `Event::EnemySpawned` extended — Task 6.
- ✅ `spawn_enemy` handler with both spawn-location resolution paths — Task 7.
- ✅ Engagement-on-spawn (0 / 1 / multi-investigator → reject) — Task 7 + Task 9.
- ✅ Wire into `encounter_card_revealed` (Revelation first, then spawn; replace #126 reject stub) — Task 7.
- ✅ Pipeline emits `spawn: None` + corpus regenerated — Task 3.
- ✅ Synthetic spawn-bearing enemy + canonical synth-loc code — Task 8.
- ✅ Integration test with happy path + multi-investigator reject — Task 9.
- ✅ Phase-doc update with Decision entries — Task 10.
- ✅ PR description with verbatim rules citations — Task 11.

**Placeholder scan:** no "TBD" / "implement later" / "similar to Task N" patterns. Every step has either runnable commands or pasteable code.

**Type consistency:**
- `SpawnLocation::Specific(String)` throughout (consistent with the YAGNI decision).
- `Location.code: CardCode` field name matches across Task 5, Task 7, Task 8, Task 9.
- `state.next_enemy_id` (snake_case) consistent.
- `Event::EnemySpawned { enemy, code, location, engaged_with }` field names match across Tasks 6, 7, 9.
- `SYNTH_LOC_CODE` / `SYNTH_ENEMY_CODE` consistent across Tasks 8, 9.
- `spawn_enemy(state, events, investigator, code, metadata)` signature matches Task 7 definition and Task 9 use.

All clear.
