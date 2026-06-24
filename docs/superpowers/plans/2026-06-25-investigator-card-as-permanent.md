# Investigator-card-as-permanent Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Represent each seated investigator's investigator card as a real `CardInPlay` holding health/sanity and acting as the default damage/horror soaker, deleting the four bespoke `Investigator` harm/capacity fields and the two #118-bridge fields.

**Architecture:** A new dedicated `Investigator.investigator_card: CardInPlay` field (not in `cards_in_play`, not `Option`). Harm lives in its `accumulated_damage`/`accumulated_horror`; capacity is read from `CardKind::Investigator { health, sanity }` metadata via the registry (uniform with assets). A unified `controlled_card_instances()` iterator that prepends the investigator card wires its abilities/reactions/constant-modifiers in for free. The six deleted fields become accessor methods. Migrated behind an **accessor seam**: accessors delegate to the old fields first (Checkpoint 1), then swap to read the card (Checkpoint 2), so every checkpoint stays green and behaviour-identical.

**Tech Stack:** Rust, `serde`, the game-core kernel (`crates/game-core`), the card DSL (`crates/card-dsl`), the web client (`crates/web`).

## Global Constraints

- CI runs seven jobs, all warnings-as-errors. Every checkpoint must pass the full local gauntlet before commit:
  - `RUSTFLAGS="-D warnings" cargo test --all --all-features`
  - `cargo clippy --all-targets --all-features -- -D warnings`
  - `cargo fmt --check`
  - `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`
  - `cargo build -p web --target wasm32-unknown-unknown`
  - `cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings`
- **Validate-first / mutate-second** dispatch contract (CLAUDE.md): handlers check all preconditions before mutating.
- **Behaviour-preserving:** no rules-behaviour change. Any latent bug surfaced gets its own issue, not a silent fix.
- One feature branch `engine/investigator-card-permanent`; four checkpoints = four (or more) commits, each green. The branch already carries the design-spec commit.
- **Never hand-edit `crates/cards/src/generated/cards.rs`** (generated).
- Design spec: `docs/superpowers/specs/2026-06-25-investigator-card-as-permanent-design.md`.

---

## CHECKPOINT 1 — The accessor seam (the card exists; reads go through accessors)

Goal: introduce `investigator_card` + a synthetic test registry, mint the card everywhere an `Investigator` is built, and route every read of the four harm/capacity fields (and `card_code`) through accessor methods that *delegate to the existing fields*. No behaviour change — pure seam introduction.

### Task 1: Add the `investigator_card` field + synthetic test-registry infrastructure

**Files:**
- Modify: `crates/game-core/src/state/investigator.rs` (struct + a constructor helper)
- Modify: `crates/game-core/src/test_support/fixtures.rs` (mint the card in `test_investigator`)
- Create/Modify: `crates/game-core/src/test_support/mod.rs` (a `test_registry` install helper + synthetic `TEST_INV` metadata)
- Modify: `crates/game-core/src/engine/dispatch/phases.rs:97–126` (mint the card at seating)
- Modify: any other site that constructs an `Investigator` literal (compiler will list them)

**Interfaces:**
- Produces: `Investigator.investigator_card: CardInPlay`; `const TEST_INV: &str = "TEST_INV"`; `fn install_test_registry()` in `test_support` registering `TEST_INV` with `health: 8, sanity: 8` and skills; `test_investigator(id)` now mints `investigator_card` with `code == TEST_INV`, instance id `CardInstanceId(u32::MAX - id)` (a high, collision-free id distinct from gameplay instances).
- Consumes: `CardInPlay::enter_play(code, instance_id)` (existing), `card_registry::install` (existing), `CardKind::Investigator { skills, health, sanity }` (existing metadata variant).

- [ ] **Step 1: Write the failing test** (in `crates/game-core/src/state/investigator.rs` tests)

```rust
#[test]
fn test_investigator_has_an_investigator_card_with_the_synthetic_code() {
    let inv = crate::test_support::test_investigator(1);
    assert_eq!(inv.investigator_card.code.as_str(), crate::test_support::TEST_INV);
    assert_eq!(inv.investigator_card.accumulated_damage, 0);
    assert_eq!(inv.investigator_card.accumulated_horror, 0);
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p game-core test_investigator_has_an_investigator_card -- --nocapture`
Expected: FAIL — no field `investigator_card` on `Investigator` (compile error).

- [ ] **Step 3: Add the field to the struct**

In `crates/game-core/src/state/investigator.rs`, add to `struct Investigator` (keep all existing fields for now):

```rust
    /// The investigator's own card as a real in-play permanent: it holds the
    /// investigator's health/sanity capacity (from `CardKind::Investigator`
    /// metadata) and is the default damage/horror soaker via its
    /// `accumulated_damage` / `accumulated_horror`. Lives here rather than in
    /// `cards_in_play` so loops over "cards the player played" never touch it
    /// (#448). Required on the wire.
    pub investigator_card: CardInPlay,
```

- [ ] **Step 4: Add the synthetic test card + registry helper**

In `crates/game-core/src/test_support/mod.rs` (follow the existing `weapon_fight.rs` mock-registry pattern; define a `static` metadata and a `OnceLock`-guarded install):

```rust
/// Synthetic investigator-card code for unit tests. Registered by
/// [`install_test_registry`] with 8 health / 8 sanity (mirroring the legacy
/// `test_investigator` capacity).
pub const TEST_INV: &str = "TEST_INV";

fn test_inv_metadata() -> &'static crate::card_data::CardMetadata {
    use crate::card_data::{CardKind, CardMetadata, Skills};
    static M: std::sync::OnceLock<CardMetadata> = std::sync::OnceLock::new();
    M.get_or_init(|| CardMetadata {
        code: TEST_INV.to_owned(),
        name: "Test Investigator".to_owned(),
        traits: vec![],
        text: None,
        pack_code: "_test".to_owned(),
        kind: CardKind::Investigator {
            skills: Skills { willpower: 3, intellect: 3, combat: 3, agility: 3 },
            health: 8,
            sanity: 8,
        },
    })
}

/// Install a minimal game-core test registry that knows `TEST_INV` (and only
/// it). Idempotent; safe to call from any test. Capacity-reading code
/// (`max_health()` / `max_sanity()` / soak / defeat) needs this installed.
pub fn install_test_registry() {
    use crate::state::CardCode;
    static INSTALL: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    INSTALL.get_or_init(|| {
        fn metadata_for(code: &CardCode) -> Option<&'static crate::card_data::CardMetadata> {
            (code.as_str() == TEST_INV).then(test_inv_metadata)
        }
        fn abilities_for(_: &CardCode) -> Option<Vec<crate::dsl::Ability>> { None }
        let _ = crate::card_registry::install(crate::card_registry::CardRegistry {
            metadata_for,
            abilities_for,
            native_effect_for: |_| None,
        });
    });
}
```

> NOTE: confirm the exact `CardKind::Investigator { .. }` and `CardMetadata` field shape against `crates/card-dsl/src/card_data.rs` before writing — copy the real field set verbatim. If a registry is already installed by another test in the same binary, `install` is a no-op (OnceLock); that is fine because `TEST_INV` only needs to resolve where capacity is actually read.

- [ ] **Step 5: Mint the card in `test_investigator`**

In `crates/game-core/src/test_support/fixtures.rs`, inside `test_investigator(id)`, build the card and set the field (keep `max_health: 8` etc. as-is for the seam):

```rust
    let mut investigator_card =
        CardInPlay::enter_play(CardCode::new(TEST_INV), CardInstanceId(u32::MAX - id));
    investigator_card.accumulated_damage = 0;
    investigator_card.accumulated_horror = 0;
    // … in the returned struct literal:
    //     investigator_card,
```

- [ ] **Step 6: Mint the card at seating**

In `crates/game-core/src/engine/dispatch/phases.rs` (the `start_scenario` mutate loop ~`:97–126`), where each `Investigator` is built from the roster entry's `card_code`, also mint its `investigator_card` from the **same** code with a freshly minted instance id (`state.card_instance_ids.mint()`), emitting **no** `EnteredPlay`/play event:

```rust
    let inv_card_id = state.card_instance_ids.mint();
    let investigator_card = CardInPlay::enter_play(card_code.clone(), inv_card_id);
    // … set `investigator_card` in the Investigator literal alongside the existing fields.
```

- [ ] **Step 7: Fix remaining construction sites**

Run `cargo build -p game-core` and add `investigator_card` to every `Investigator { .. }` literal the compiler flags (mostly tests). For test literals not going through `test_investigator`, mint with `CardInPlay::enter_play(CardCode::new(TEST_INV), CardInstanceId(u32::MAX - <id>))`.

- [ ] **Step 8: Run the test to verify it passes**

Run: `cargo test -p game-core test_investigator_has_an_investigator_card -- --nocapture`
Expected: PASS.

- [ ] **Step 9: Full gauntlet + commit**

Run the six gauntlet commands from Global Constraints. Then:

```bash
git add -A
git commit -m "engine: add Investigator.investigator_card field + test registry (#448 cp1a)"
```

### Task 2: Route reads through accessor methods (delegating to the old fields)

**Files:**
- Modify: `crates/game-core/src/state/investigator.rs` (accessor methods)
- Modify: every read site of `inv.damage` / `inv.horror` / `inv.max_health` / `inv.max_sanity` / `inv.card_code` across `crates/game-core`, `crates/web`, `crates/scenarios`, `crates/server` (compiler-guided after the fields are made private — see Step 4).

**Interfaces:**
- Produces: `Investigator::{damage(&self) -> u8, horror(&self) -> u8, max_health(&self) -> u8, max_sanity(&self) -> u8}`; identity read via `inv.investigator_card.code`. In Checkpoint 1 the four accessors **delegate to the existing fields** (no card/registry reads yet).

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn harm_accessors_match_the_underlying_fields_during_the_seam() {
    let mut inv = crate::test_support::test_investigator(1);
    inv.damage = 2;
    inv.horror = 1;
    assert_eq!(inv.damage(), 2);
    assert_eq!(inv.horror(), 1);
    assert_eq!(inv.max_health(), inv.max_health);
    assert_eq!(inv.max_sanity(), inv.max_sanity);
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p game-core harm_accessors_match_the_underlying_fields`
Expected: FAIL — no method `damage` on `Investigator`.

- [ ] **Step 3: Add the delegating accessors**

In `impl Investigator`:

```rust
    /// Physical damage currently on the investigator. Reads the investigator
    /// card's accumulated damage once harm moves there (#448 cp2); during the
    /// seam it delegates to the legacy field.
    #[must_use]
    pub fn damage(&self) -> u8 { self.damage }
    /// Horror currently on the investigator. See [`Self::damage`].
    #[must_use]
    pub fn horror(&self) -> u8 { self.horror }
    /// Maximum health (printed). Reads `CardKind::Investigator` metadata once
    /// the seam closes (#448 cp2); during the seam it delegates to the field.
    #[must_use]
    pub fn max_health(&self) -> u8 { self.max_health }
    /// Maximum sanity (printed). See [`Self::max_health`].
    #[must_use]
    pub fn max_sanity(&self) -> u8 { self.max_sanity }
```

- [ ] **Step 4: Migrate read sites (compiler-guided)**

Temporarily rename the fields to force the compiler to enumerate every read: in the struct, rename `pub damage` → `damage` (drop `pub`) one at a time, `cargo build --all`, and for each `field is private` / external-read error replace `X.damage` with `X.damage()` (and likewise horror/max_health/max_sanity). For `inv.card_code` reads, replace with `inv.investigator_card.code` / `&inv.investigator_card.code`. **Do not** convert *write* sites (`inv.damage = …`) yet — those stay as field writes through Checkpoint 1; keep those four fields assignable within the crate (leave them `pub` for now and instead grep: `rg -n "\.damage\b" crates --type rust` and convert reads by hand, leaving assignments). Use the web crate too: `crates/web/src/board.rs:106–107` → `inv.damage()` / `inv.max_health()` / `inv.horror()` / `inv.max_sanity()`.

> Guidance: the clean mechanical rule is "every *read* of the four fields and of `card_code` becomes the accessor; every *write* stays a field assignment until its Checkpoint." Reads vastly outnumber writes (writes are the handful in the consumer-surface appendix).

- [ ] **Step 5: Run the test + full suite**

Run: `cargo test -p game-core harm_accessors_match_the_underlying_fields` then `RUSTFLAGS="-D warnings" cargo test --all --all-features`
Expected: PASS, all green.

- [ ] **Step 6: Full gauntlet + commit**

```bash
git add -A
git commit -m "engine: route investigator harm/capacity reads through accessors (#448 cp1b)"
```

---

## CHECKPOINT 2 — Move harm, soak, and defeat onto the investigator card

Goal: make the investigator card the source of truth for harm and a real soaker; swap the accessor internals from the fields to the card; defeat reads the card. The four old fields become vestigial (still present, no readers/writers) until Checkpoint 4.

### Task 3: Move harm writes + healing onto the card; swap accessor internals

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/combat.rs:343–397` (`apply_damage_numeric` / `apply_horror_numeric`)
- Modify: `crates/game-core/src/engine/evaluator.rs:1461–1501` (`heal_effect`)
- Modify: `crates/game-core/src/engine/dispatch/actions.rs:852,988,1108,1289` and `cards.rs:866` (the `inv.damage = 0` / `inv.horror = 0` resets)
- Modify: `crates/game-core/src/state/investigator.rs` (accessor bodies)

**Interfaces:**
- Consumes: `inv.investigator_card.accumulated_damage` / `accumulated_horror` (mutable), `inv.max_health()` / `max_sanity()`.
- Produces: `damage()` now reads `self.investigator_card.accumulated_damage`; `horror()` reads `accumulated_horror`; `max_health()`/`max_sanity()` read `CardKind::Investigator` metadata via `card_registry::current()`.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn damage_application_accumulates_on_the_investigator_card() {
    crate::test_support::install_test_registry();
    let mut inv = crate::test_support::test_investigator(1);
    // Drive the harm entry point used by attacks/effects (adjust to the real
    // signature found in combat.rs):
    let defeated = crate::engine::dispatch::combat::apply_damage_numeric(&mut inv, 3);
    assert_eq!(inv.investigator_card.accumulated_damage, 3);
    assert_eq!(inv.damage(), 3);
    assert!(!defeated, "3 < 8 health");
}
```

> Adjust the call to the real `apply_damage_numeric` signature/visibility (it may take `&mut Cx` / an id). If it is not unit-callable, write the test at the existing harness level used by the current damage tests in `combat.rs`.

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p game-core damage_application_accumulates_on_the_investigator_card`
Expected: FAIL — `accumulated_damage` stays 0 (harm still writes the field).

- [ ] **Step 3: Move the harm writes**

In `apply_damage_numeric`, replace `inv.damage = inv.damage.saturating_add(amount)` with `inv.investigator_card.accumulated_damage = inv.investigator_card.accumulated_damage.saturating_add(amount)`, and change the defeat check from `inv.damage >= inv.max_health` to `inv.investigator_card.accumulated_damage >= inv.max_health()`. Mirror for `apply_horror_numeric` (horror/sanity). In `heal_effect`, reduce `inv.investigator_card.accumulated_damage` / `accumulated_horror`. Convert the `inv.damage = 0` / `inv.horror = 0` resets to `inv.investigator_card.accumulated_damage = 0` / `accumulated_horror = 0`.

- [ ] **Step 4: Swap the accessor internals**

```rust
    pub fn damage(&self) -> u8 { self.investigator_card.accumulated_damage }
    pub fn horror(&self) -> u8 { self.investigator_card.accumulated_horror }
    pub fn max_health(&self) -> u8 { investigator_capacity(&self.investigator_card.code).0 }
    pub fn max_sanity(&self) -> u8 { investigator_capacity(&self.investigator_card.code).1 }
```

Add a private helper that reads the metadata (panicking loudly if the registry/metadata is absent — no silent default, per project norms):

```rust
/// (health, sanity) printed capacity for an investigator card, from the
/// installed registry. Panics if the registry is uninstalled or the code is
/// not an investigator card — a state-shape invariant violation, surfaced
/// rather than silently defaulted.
fn investigator_capacity(code: &CardCode) -> (u8, u8) {
    let reg = crate::card_registry::current()
        .expect("investigator capacity read before a CardRegistry was installed");
    match &(reg.metadata_for)(code)
        .expect("investigator card code absent from registry")
        .kind
    {
        crate::card_data::CardKind::Investigator { health, sanity, .. } => (*health, *sanity),
        _ => panic!("investigator_card.code does not resolve to a CardKind::Investigator"),
    }
}
```

- [ ] **Step 5: Run the test + suite**

Run: `cargo test -p game-core damage_application_accumulates_on_the_investigator_card` then the full test job.
Expected: PASS. Fix any test that built an unseated investigator and read capacity without `install_test_registry()` — add the install call.

- [ ] **Step 6: Full gauntlet + commit**

```bash
git add -A
git commit -m "engine: move investigator harm + capacity onto the investigator card (#448 cp2a)"
```

### Task 4: Make the investigator card a soaker; unify defeat (overflow → eliminate)

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/combat.rs:457–495` (`build_soakers`), `:164–185` (`assign_attack`), `:211–256` (`defeat_overflowed_assets`), `:278–322` (`place_assignment`)

**Interfaces:**
- Consumes: the investigator card as a soaker with `(remaining_health, remaining_sanity) = (max_health() − accumulated_damage, max_sanity() − accumulated_horror)`.
- Produces: `build_soakers` returns the investigator card as the always-eligible default soaker (filled per RR after assets); `defeat_overflowed_assets` (or its caller) runs investigator elimination when the *investigator card* overflows instead of discarding it.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn an_asset_soaks_first_then_the_investigator_card_takes_the_remainder() {
    crate::test_support::install_test_registry();
    // Build a seated investigator with one 1-health soaker asset in play, deal
    // 3 damage: asset takes 1 (and is defeated), investigator card takes 2.
    // (Use the existing soak-test harness in combat.rs as the template.)
    // assert asset discarded; assert inv.investigator_card.accumulated_damage == 2.
}
```

> Flesh this out from the nearest existing soak test in `combat.rs` (search `build_soakers` / `soak_and_place` tests). The assertion that matters: remainder lands on `investigator_card.accumulated_damage`, not a field.

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p game-core an_asset_soaks_first_then_the_investigator_card`
Expected: FAIL (investigator card not yet a soaker / remainder mis-routed).

- [ ] **Step 3: Add the investigator card to `build_soakers`**

In `build_soakers`, after collecting asset soakers from `cards_in_play`, append a soaker entry for `inv.investigator_card` whose remaining capacity is `inv.max_health() − accumulated_damage` / `inv.max_sanity() − accumulated_horror`, flagged as the default/mandatory-remainder target. Remove the `assign_attack` branch that dumped the remainder onto the investigator field; the remainder now flows into the investigator-card soaker like any other (it is just always last/always eligible per the RR "must be assigned to the investigator" clause).

- [ ] **Step 4: Branch defeat on the investigator card**

In `defeat_overflowed_assets` (or `place_assignment`'s post-apply defeat sweep), when the overflowed instance is the investigator card (`instance_id == inv.investigator_card.instance_id`), call the existing investigator-elimination path (`apply_investigator_defeat` with the right `DefeatCause`) instead of discarding to the owner's pile.

- [ ] **Step 5: Run the test + suite**

Run the soak test then the full test job.
Expected: PASS. Re-run the existing soak/defeat tests — they should still pass with harm on the card.

- [ ] **Step 6: Full gauntlet + commit**

```bash
git add -A
git commit -m "engine: investigator card is the default soaker; unify defeat (#448 cp2b)"
```

---

## CHECKPOINT 3 — Wire abilities/reactions/constant-mods; retire the #118 bridge

### Task 5: Unify `controlled_card_instances()` and migrate the scans onto it

**Files:**
- Modify: `crates/game-core/src/state/investigator.rs:145–147` (`controlled_card_instances`)
- Modify: `crates/game-core/src/engine/evaluator.rs:2112–2140` (`sum_constant_modify`)
- Modify: `crates/game-core/src/engine/dispatch/reaction_windows.rs:222–288` (`scan_pending_triggers`) and `:362–424` (delete `scan_investigator_card_reactions`)
- Modify: wherever `CandidateSource::Investigator` is defined/matched (delete it)

**Interfaces:**
- Produces: `controlled_card_instances()` now yields the investigator card first, then `cards_in_play`, then `threat_area`. `sum_constant_modify` and the reaction scan iterate it. `scan_investigator_card_reactions` / `CandidateSource::Investigator` removed.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn a_seated_investigator_card_constant_modifier_is_summed() {
    // Register a test investigator card whose abilities() include a
    // Trigger::Constant Effect::Modify (+1 to some skill), seat it, and assert
    // sum_constant_modify picks it up WITHOUT injecting the card into
    // cards_in_play. (Use a bespoke test registry variant returning abilities
    // for a dedicated code.)
}
```

> If wiring a full constant-mod test is heavy, the equivalent observable is the existing Roland reaction / elder-sign integration tests in `crates/cards/tests/roland_banks_seated.rs` — Task 6 covers that path. Prefer a focused game-core test here on `controlled_card_instances()` ordering plus a `sum_constant_modify` unit test.

- [ ] **Step 2: Run it to verify it fails**

Expected: FAIL — the investigator card's ability is not summed (scan ignores it).

- [ ] **Step 3: Prepend the investigator card to the iterator**

```rust
    pub fn controlled_card_instances(&self) -> impl Iterator<Item = &CardInPlay> {
        std::iter::once(&self.investigator_card)
            .chain(self.cards_in_play.iter())
            .chain(self.threat_area.iter())
    }
```

- [ ] **Step 4: Migrate the bypassing consumers**

In `sum_constant_modify`, change the iteration from `inv.cards_in_play.iter()` to `inv.controlled_card_instances()`. In `scan_pending_triggers`, confirm it iterates `controlled_card_instances()` (it does, per the surface map) so the investigator card is now scanned; delete `scan_investigator_card_reactions` and its call site, and remove the `CandidateSource::Investigator` variant (replace its construction with the normal in-play-instance candidate carrying the investigator card's instance id).

> **Care point:** audit every *other* `cards_in_play.iter()` in the engine. Leave on `cards_in_play` any loop meaning "cards the player played" (asset queries, elimination drain, discard-all). Only ability/reaction/constant-modifier *scans* move to the unified iterator.

- [ ] **Step 5: Run the test + suite**

Expected: PASS; existing reaction/elder-sign tests still green.

- [ ] **Step 6: Full gauntlet + commit**

```bash
git add -A
git commit -m "engine: unify controlled_card_instances; scans include the investigator card (#448 cp3a)"
```

### Task 6: Retire the #118 bridge fields

**Files:**
- Modify: `crates/game-core/src/state/investigator.rs` (delete `card_code`, `ability_usage`, `is_usage_exhausted`, `bump_ability_usage`)
- Modify: `crates/game-core/src/engine/evaluator.rs:1935–1960` (`elder_sign_modifier` → `investigator_card.code`)
- Modify: `crates/game-core/src/engine/dispatch/reaction_windows.rs` (usage checks → `investigator_card.is_usage_exhausted` / `bump_ability_usage`)
- Modify: any remaining reader of `inv.card_code` / `inv.ability_usage`

**Interfaces:**
- Produces: identity exclusively via `inv.investigator_card.code`; per-period usage exclusively via `inv.investigator_card` (the `CardInPlay` methods). `Investigator.card_code` and `Investigator.ability_usage` no longer exist.

- [ ] **Step 1: Write the failing test (or reuse the seated-Roland integration test)**

The load-bearing behaviour is already covered by `crates/cards/tests/roland_banks_seated.rs` (elder-sign + seated reaction). Add a game-core unit assertion that `Investigator` has no `card_code` field by switching the seated-identity read:

```rust
#[test]
fn identity_is_read_from_the_investigator_card() {
    let inv = crate::test_support::test_investigator(1);
    assert_eq!(inv.investigator_card.code.as_str(), crate::test_support::TEST_INV);
}
```

- [ ] **Step 2: Run it / build to verify the bridge is still present**

Run: `cargo build -p game-core`
Expected: still compiles (bridge present). The "fail" here is the existence of the now-redundant fields; proceed to delete.

- [ ] **Step 3: Delete the bridge fields + wrappers**

Remove `card_code`, `ability_usage`, `is_usage_exhausted`, `bump_ability_usage` from `Investigator`. `cargo build --all` and migrate every flagged reader: `inv.card_code` → `inv.investigator_card.code`; `inv.ability_usage` / `inv.is_usage_exhausted(..)` / `inv.bump_ability_usage(..)` → `inv.investigator_card.ability_usage` / `inv.investigator_card.is_usage_exhausted(..)` / `inv.investigator_card.bump_ability_usage(..)`. In `elder_sign_modifier`, look abilities up via `investigator_card.code` (drop the empty-sentinel skip — a seated investigator always has a real code; an unseated test investigator simply has no elder-sign ability for `TEST_INV`).

- [ ] **Step 4: Update the #453 serde test**

The `omitting_any_required_field_is_rejected` investigator test lists `card_code` / `ability_usage` — remove those two from its field list (they no longer exist) and add `investigator_card`.

- [ ] **Step 5: Run the suite**

Run: `RUSTFLAGS="-D warnings" cargo test --all --all-features`
Expected: PASS, including `crates/cards/tests/roland_banks_seated.rs`.

- [ ] **Step 6: Full gauntlet + commit**

```bash
git add -A
git commit -m "engine: retire the #118 investigator-card bridge fields (#448 cp3b)"
```

---

## CHECKPOINT 4 — Delete the vestigial fields and finalize

### Task 7: Delete the four old fields; finalize web + fixtures

**Files:**
- Modify: `crates/game-core/src/state/investigator.rs` (delete `damage`, `horror`, `max_health`, `max_sanity`)
- Modify: `crates/game-core/src/test_support/fixtures.rs` (drop the old-field sets)
- Modify: `crates/game-core/src/engine/dispatch/phases.rs` (drop the old-field sets at seating)
- Modify: any remaining `Investigator { .. }` literal (compiler-guided)
- Verify: `crates/web/src/board.rs` already on accessors (Checkpoint 1)

**Interfaces:**
- Produces: `Investigator` with no `damage` / `horror` / `max_health` / `max_sanity` fields; all access via the accessors backed by `investigator_card`.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn defeat_reads_capacity_from_the_card_not_a_field() {
    crate::test_support::install_test_registry();
    let mut inv = crate::test_support::test_investigator(1);
    inv.investigator_card.accumulated_damage = 8; // == TEST_INV health
    assert!(inv.damage() >= inv.max_health(), "defeat threshold reached via accessors");
}
```

- [ ] **Step 2: Run it to verify it passes already (accessors)** then proceed to delete fields.

Run: `cargo test -p game-core defeat_reads_capacity_from_the_card_not_a_field`
Expected: PASS (accessors already back onto the card). This test guards the post-deletion state.

- [ ] **Step 3: Delete the four fields**

Remove `pub damage`, `pub horror`, `pub max_health`, `pub max_sanity` from `Investigator`. `cargo build --all` and remove every now-dangling field write/set the compiler flags (the `inv.damage = …` writes were all converted to `inv.investigator_card.accumulated_* = …` in Checkpoint 2, so remaining hits are construction literals — drop those keys).

- [ ] **Step 4: Update the #453 serde test field list**

Remove `damage`/`horror`/`max_health`/`max_sanity` from any JSON-literal field list that enumerated them; the `omitting_any_required_field_is_rejected` test should now include `investigator_card` among required fields.

- [ ] **Step 5: Run the full suite**

Run: `RUSTFLAGS="-D warnings" cargo test --all --all-features`
Expected: PASS.

- [ ] **Step 6: Full gauntlet + commit**

```bash
git add -A
git commit -m "engine: delete the four bespoke investigator harm/capacity fields (#448 cp4)"
```

---

## Finalization (after all checkpoints green)

- [ ] Update `docs/phases/phase-7-the-gathering.md`: mark #448 shipped, note the bridge retirement closes the #118 sunset and the #453 `card_code`-sentinel question. (Final commit, after CI is green on the opened PR — per the repo PR procedure.)
- [ ] Open the PR mapping commit → checkpoint, with a design-decisions paragraph linking the spec. `Closes #448.`

## Self-review notes (coverage against the spec)

- §1 target model → Tasks 1, 2, 7. §2 unified iteration → Task 5. §3 soak/defeat → Tasks 3, 4. §4 seating/suppression → Task 1 (Step 6). §5 bridge retirement → Task 6. §6 test strategy → Task 1 (Steps 4–5) + `install_test_registry`. §7 four checkpoints → the four-checkpoint structure.
- No `Option<CardCode>` redesign (out of scope, per spec).
- Open risk carried into execution: the §2 "care point" iterator-classification audit (Task 5, Step 4) is the highest-judgment step — every `cards_in_play.iter()` must be classified.
