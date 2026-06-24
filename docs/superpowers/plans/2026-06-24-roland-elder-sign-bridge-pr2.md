# Roland's Elder-Sign + Investigator-Card Bridge (PR 2) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Roland Banks's `[elder_sign]` symbol ("+1 for each clue on your location", 01001) actually add its bonus when the elder-sign chaos token resolves during his skill test, AND make his `[reaction]` fire from a *roster-seated* investigator (today it only fires when a test hand-injects his card into `cards_in_play`). This is **Section 2** of the IntExpr cluster spec (`docs/superpowers/specs/2026-06-24-intexpr-dynamic-value-cluster-design.md`); Sections 1 & 3 already shipped (PR #450 / #452 on `main`).

**Architecture:** The elder-sign is the *investigator's* own symbol token — same resolution pipeline as scenario symbol tokens (`resolve_symbol_token` → `TokenResolution`), but sourced from the **investigator card** instead of the scenario bag. The bonus flows through the existing `Modifier`-total path; there is **no** `Effect::ModifySkillTestTotal`. The investigator's own card code is currently dropped at seating, so investigator-card abilities (elder-sign *and* reaction) don't fire in a seated game. #118 adds a deliberately small bridge: `Investigator.card_code` (direct lookup for the elder-sign), `Investigator.ability_usage` (a usage-tracking home for once-per-round reactions), and a `scan_investigator_card_reactions` reaction-scan source. The investigator card stays **out of `cards_in_play`** (no phantom-soaker hazard).

**Tech Stack:** Rust workspace. `card-dsl` (pure data: the DSL enums), `game-core` (the kernel: state + apply loop + evaluator), `cards` (content: hand-written `Ability` declarations + corpus). Cross-crate card data flows through `game_core::card_registry` (function pointers `metadata_for` / `abilities_for`).

**Deferrals (do NOT add tasks for these — note them in code doc-comments only):**
- Elder-signs that also run an *effect* beyond the modifier — Daisy's per-Tome draw, Agnes's optional damage. The inline path handles only the modifier. When the first lands, consider building a full `SymbolOutcome` from the investigator card for uniformity with the scenario path. Roland is pure-modifier, so none of this is needed now.
- Substitute-test / reveal-another-token elder-signs.
- **#448** (investigator-card-as-permanent) later unifies the investigator card as a real `CardInPlay` (health/sanity/soak too), retiring `card_code` + `ability_usage` + the bespoke scan source into the uniform path. #118's bridge is deliberately small and **sunset-by-#448**; say so in the doc-comments on the new fields/scan.

## Global Constraints
- CI warnings-as-errors; before each commit run `RUSTFLAGS="-D warnings" cargo test --all --all-features`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo fmt --check`, `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`, and the wasm jobs `cargo build -p web --target wasm32-unknown-unknown` + `cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings`.
- `card-dsl` is pure data; engine logic lives in `game-core`.
- Behaviour-preserving for every existing investigator/card (no elder-sign ability ⇒ identical resolution; the elder-sign arm returns `0` bonus when the controller's card has no `Trigger::ElderSign` ability).
- **`IntExpr` already exists** (shipped PR1) with `Lit(i8)` / `Cond { when, then, otherwise }` / `Count(Quantity)`, and `Quantity::CluesAtControllerLocation` / `EngagedEnemies` / `SkillTestFailedBy`. `eval_int_expr` / `eval_quantity` already live in `crates/game-core/src/engine/evaluator.rs` (both currently private `fn`). Roland's elder-sign uses `IntExpr::Count(Quantity::CluesAtControllerLocation)` — no new `Quantity` term.
- **Card text is authoritative** (`data/arkhamdb-snapshot/pack/core/core.json`, 01001): `[elder_sign] effect: +1 for each clue on your location.` Confirmed.

---

## Task 1 — `Trigger::ElderSign { modifier: IntExpr }`

Add the config-on-trigger variant to the `Trigger` enum, mirroring `Trigger::Activated { action_cost }` / `Trigger::OnSkillTestResolution { outcome }` (same derives — `Trigger` derives `Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize`).

**Files:**
- `crates/card-dsl/src/dsl.rs` — add the variant + a doc-comment; add a `#[cfg(test)]` round-trip test.

**Interfaces:**
- Consumes: `IntExpr` (already in this file, `dsl.rs:1127`).
- Produces: `Trigger::ElderSign { modifier: IntExpr }`.

### Steps

- [ ] **1.1 — Write the failing test.** Append to the `#[cfg(test)] mod tests` block in `crates/card-dsl/src/dsl.rs` (the same module that holds `on_event_carries_trigger_kind`, ~line 2040):

```rust
    /// `Trigger::ElderSign` is a config-on-trigger variant (like
    /// `Activated { action_cost }`): it carries the elder-sign's printed
    /// modifier as an `IntExpr` and round-trips through serde. Roland's
    /// "+1 for each clue on your location" is `Count(CluesAtControllerLocation)`.
    #[test]
    fn elder_sign_trigger_carries_int_expr_and_round_trips() {
        let t = Trigger::ElderSign {
            modifier: IntExpr::Count(Quantity::CluesAtControllerLocation),
        };
        let json = serde_json::to_string(&t).expect("serialize");
        let back: Trigger = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(t, back);
        // Distinct from a literal-modifier elder-sign and from other triggers.
        assert_ne!(
            t,
            Trigger::ElderSign {
                modifier: IntExpr::Lit(1),
            },
        );
        assert_ne!(t, Trigger::Constant);
    }
```

  `IntExpr` and `Quantity` are defined in this same module, so they're already in scope for the test (`use super::*;` is at the top of the tests module — verify; if not, the existing tests reference `IntExpr` directly so it resolves).

- [ ] **1.2 — Run it; confirm it fails to compile** (`Trigger::ElderSign` does not exist):

```sh
cargo test -p card-dsl elder_sign_trigger_carries_int_expr_and_round_trips
```

  Expected: compile error `no variant or associated item named ElderSign found for enum Trigger`.

- [ ] **1.3 — Add the variant.** In `crates/card-dsl/src/dsl.rs`, inside `pub enum Trigger { … }` (after the `OnEvent { … }` arm, before the closing `}` at ~line 193), add:

```rust
    /// Fires when the investigator's **elder-sign** chaos token (`[O]`)
    /// is revealed during a skill test they are taking. The elder-sign is
    /// the investigator's *own* symbol token: its effect is sourced from
    /// the investigator card rather than the scenario bag.
    ///
    /// `modifier` is the printed skill-test modifier the elder-sign grants,
    /// as an [`IntExpr`] so board-state-dependent values (Roland Banks's
    /// "+1 for each clue on your location" → `Count(CluesAtControllerLocation)`)
    /// resolve at draw time. The engine adds this to the test total through
    /// the existing `Modifier` path (`skill_test.rs`), keeping the
    /// `ElderSign` resolution label for observability.
    ///
    /// Config-on-trigger, like [`Activated`](Self::Activated) /
    /// [`OnSkillTestResolution`](Self::OnSkillTestResolution).
    ///
    /// **Scope (#118):** only pure-*modifier* elder-signs are handled. Signs
    /// that also run an effect (Daisy's per-Tome draw, Agnes's optional
    /// damage) or substitute/reveal another token are deferred — the first
    /// such card should build a full `SymbolOutcome` from the investigator
    /// card for uniformity with the scenario symbol path.
    ElderSign {
        /// The printed skill-test modifier the elder-sign grants.
        modifier: IntExpr,
    },
```

- [ ] **1.4 — Run it; confirm it passes:**

```sh
cargo test -p card-dsl elder_sign_trigger_carries_int_expr_and_round_trips
```

  Expected: `test result: ok. 1 passed`.

- [ ] **1.5 — Commit.**

```sh
cargo fmt && git add -A && git commit -m "card-dsl: Trigger::ElderSign { modifier: IntExpr }"
```

---

## Task 2 — `Investigator.card_code: CardCode` threaded at seating

Add the field to `struct Investigator`, set it at roster seating from `RosterEntry.investigator`, and default it to an empty-string sentinel `CardCode::new("")` for the pre-seated `test_investigator` / builder path.

**Files:**
- `crates/game-core/src/state/investigator.rs` — add the `card_code` field (with `#[serde(default)]` for backward-compat) + a doc-comment.
- `crates/game-core/src/engine/dispatch/phases.rs` — widen the `resolved` tuple to carry the code; set `card_code` in the `Investigator { … }` construction.
- `crates/game-core/src/test_support/fixtures.rs` — set `card_code: CardCode::new("")` in `test_investigator`.

**Interfaces:**
- Consumes: `RosterEntry.investigator: CardCode` (already passed into `start_scenario`).
- Produces: `Investigator.card_code: CardCode`.

### Steps

- [ ] **2.1 — Write the failing test.** In `crates/cards/tests/roster_seating.rs` (installs the real registry already), append to the file:

```rust
#[test]
fn seated_investigator_carries_its_card_code() {
    install_registry();
    let roster = vec![RosterEntry {
        investigator: CardCode::new("01001"),
        deck: vec![],
    }];
    let state = GameStateBuilder::new().build();
    let result = apply(
        state,
        Action::Player(PlayerAction::StartScenario { roster }),
    );
    let inv = result
        .state
        .investigators
        .get(&InvestigatorId(1))
        .expect("Roland seated at id 1");
    assert_eq!(inv.card_code, CardCode::new("01001"));
}
```

  (`RosterEntry`, `apply`, `CardCode`, `InvestigatorId`, `GameStateBuilder`, `PlayerAction`, `Action` are already imported at the top of `roster_seating.rs`.)

- [ ] **2.2 — Run it; confirm it fails to compile** (`Investigator` has no field `card_code`):

```sh
cargo test -p cards --test roster_seating seated_investigator_carries_its_card_code
```

  Expected: compile error `no field card_code on type &Investigator`.

- [ ] **2.3 — Add the field.** In `crates/game-core/src/state/investigator.rs`, inside `pub struct Investigator { … }`, add after `pub id: InvestigatorId,` (before `pub name: String,`):

```rust
    /// The investigator's own `ArkhamDB` card code (01001 for Roland
    /// Banks). Set at roster seating from `RosterEntry.investigator`;
    /// the elder-sign firing path and the seated-reaction scan look the
    /// investigator card's abilities up by this code
    /// (`abilities_for(card_code)`). An empty sentinel (`CardCode::new("")`)
    /// marks the pre-seated `test_support` / builder path — codepaths skip
    /// empty codes, so those investigators carry no investigator-card
    /// abilities. Defaults to empty for backward-compatible deserialization.
    ///
    /// **Bridge (#118), sunset by #448:** when the investigator card
    /// becomes a real `CardInPlay` (health/sanity/soak), this field and
    /// [`ability_usage`](Self::ability_usage) fold into the uniform path.
    #[serde(default)]
    pub card_code: CardCode,
```

  `CardCode` is already imported at the top of `investigator.rs` (`use super::card::{CardCode, CardInPlay, CardInstanceId};`). `CardCode` derives `Default` via `String`? — verify: `CardCode(pub String)` does **not** auto-derive `Default`. So `#[serde(default)]` needs `CardCode: Default`. **Add a `Default` derive to `CardCode`** in `crates/game-core/src/state/card.rs` (line 17): change `#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]` to include `Default`:

```rust
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CardCode(pub String);
```

  (`Default` for the newtype yields `CardCode(String::new())` — the empty sentinel.)

- [ ] **2.4 — Set it at seating.** In `crates/game-core/src/engine/dispatch/phases.rs`:

  (a) Widen the `resolved` tuple type (~line 38) to carry the code:

```rust
    let mut resolved: Vec<(Skills, u8, u8, String, Vec<CardCode>, CardCode)> =
        Vec::with_capacity(roster.len());
```

  (b) Push the code (~line 63), adding `entry.investigator.clone()` as the trailing element:

```rust
        resolved.push((
            skills,
            health,
            sanity,
            meta.name.clone(),
            entry.deck.clone(),
            entry.investigator.clone(),
        ));
```

  (c) Bind it in the seating loop (~line 96) and set it on the `Investigator`:

```rust
    for (idx, (skills, health, sanity, name, deck, card_code)) in resolved.into_iter().enumerate() {
        let id = InvestigatorId(u32::try_from(idx).unwrap_or(0) + 1);
        cx.state.investigators.insert(
            id,
            Investigator {
                id,
                card_code,
                name,
                current_location: start,
                skills,
                max_health: health,
                damage: 0,
                max_sanity: sanity,
                horror: 0,
                clues: 0,
                resources: 5,
                actions_remaining: 0,
                status: Status::Active,
                deck,
                hand: Vec::new(),
                discard: Vec::new(),
                cards_in_play: Vec::new(),
                threat_area: Vec::new(),
                removed_from_game: Vec::new(),
                ability_usage: std::collections::BTreeMap::new(),
                action_surcharge_spent_this_round: std::collections::BTreeSet::new(),
            },
        );
        cx.state.turn_order.push(id);
    }
```

  > NOTE: the `ability_usage:` line above is added in **Task 3** — when doing Task 2 alone, omit it (the struct won't have the field yet). The plan keeps the full construction here for the final shape; if executing strictly task-by-task, add only `card_code,` in Task 2 and `ability_usage: …` in Task 3.

- [ ] **2.5 — Fix the `test_investigator` fixture.** In `crates/game-core/src/test_support/fixtures.rs`, inside `test_investigator` (`Investigator { … }`, ~line 36), add after `id: InvestigatorId(id),`:

```rust
        card_code: CardCode::new(""),
```

  Verify `CardCode` is imported in `fixtures.rs`; if not, add it to the existing `use` of `crate::state::{…}`. (Search the file head — `test_location` uses `CardCode`, so it is already imported.)

- [ ] **2.6 — Run the new test + the fixture/serde sanity tests; confirm green:**

```sh
cargo test -p cards --test roster_seating seated_investigator_carries_its_card_code
cargo test -p game-core --lib state::investigator
```

  Expected: both pass. (The existing `deserializes_when_threat_area_field_absent` / `deserializes_when_field_absent` JSON-literal tests in `investigator.rs` omit `card_code` and still deserialize because of `#[serde(default)]` — they should stay green.)

- [ ] **2.7 — Commit.**

```sh
cargo fmt && git add -A && git commit -m "engine: thread Investigator.card_code at roster seating"
```

---

## Task 3 — `Investigator.ability_usage` + shared usage helper

Add `ability_usage: BTreeMap<u8, AbilityUsageRecord>` to `Investigator` (mirroring `CardInPlay.ability_usage`), and extract the per-period exhaustion check into a free function over `&BTreeMap<u8, AbilityUsageRecord>` reused by both `CardInPlay` and `Investigator` (DRY).

**Files:**
- `crates/game-core/src/state/card.rs` — extract `usage_exhausted(usage: &BTreeMap<…>, ability_index, limit, current_round) -> bool` and `bump_usage(usage: &mut BTreeMap<…>, ability_index, current_round)` free functions; re-point `CardInPlay::is_usage_exhausted` / `bump_ability_usage` at them.
- `crates/game-core/src/state/investigator.rs` — add the `ability_usage` field + `is_usage_exhausted` / `bump_ability_usage` methods delegating to the free functions; add a `#[cfg(test)]` test.

**Interfaces:**
- Consumes: `AbilityUsageRecord`, `UsageLimit`, `UsagePeriod` (all in `card.rs` / `dsl`).
- Produces: free fns `state::card::usage_exhausted` / `state::card::bump_usage`; `Investigator { ability_usage, is_usage_exhausted, bump_ability_usage }`.

### Steps

- [ ] **3.1 — Write the failing test.** Append a new `#[cfg(test)]` module to `crates/game-core/src/state/investigator.rs`:

```rust
#[cfg(test)]
mod ability_usage_tests {
    use super::*;
    use crate::dsl::{UsageLimit, UsagePeriod};
    use crate::state::AbilityUsageRecord;

    #[test]
    fn new_investigator_has_empty_ability_usage() {
        let inv = crate::test_support::test_investigator(1);
        assert!(inv.ability_usage.is_empty());
    }

    #[test]
    fn usage_exhausts_after_limit_within_a_round_and_resets_across_rounds() {
        let mut inv = crate::test_support::test_investigator(1);
        let limit = Some(UsageLimit {
            count: 1,
            period: UsagePeriod::Round,
        });
        // Ability 0, round 5: not yet fired → not exhausted.
        assert!(!inv.is_usage_exhausted(0, limit, 5));
        // Fire once in round 5 → now exhausted in round 5.
        inv.bump_ability_usage(0, 5);
        assert!(inv.is_usage_exhausted(0, limit, 5));
        assert_eq!(inv.ability_usage.get(&0), Some(&AbilityUsageRecord::new(5, 1)));
        // Round 6: lazy reset → not exhausted (stored record is stale).
        assert!(!inv.is_usage_exhausted(0, limit, 6));
        // No limit (None) is never exhausted.
        assert!(!inv.is_usage_exhausted(0, None, 5));
    }
}
```

- [ ] **3.2 — Run it; confirm it fails to compile** (`Investigator` has no `ability_usage` / `is_usage_exhausted` / `bump_ability_usage`):

```sh
cargo test -p game-core --lib state::investigator::ability_usage_tests
```

  Expected: compile errors `no field ability_usage` / `no method named is_usage_exhausted`.

- [ ] **3.3 — Extract the shared free functions.** In `crates/game-core/src/state/card.rs`, add two free functions (place them just above `impl CardInPlay`, ~line 190). The bodies are lifted verbatim from `CardInPlay::is_usage_exhausted` (`card.rs:217`) and `CardInPlay::bump_ability_usage` (`card.rs:243`):

```rust
/// Whether the ability at `ability_index` has reached its
/// [`UsageLimit::count`] for the current period, reading the firing
/// record out of `usage`. Shared by [`CardInPlay`] and
/// [`Investigator`](crate::state::Investigator) so both usage-bearing
/// sources apply the same lazy-reset semantics (see the field docs on
/// [`CardInPlay::ability_usage`]). `None` limit ⇒ no cap ⇒ `false`.
#[must_use]
pub fn usage_exhausted(
    usage: &BTreeMap<u8, AbilityUsageRecord>,
    ability_index: u8,
    limit: Option<UsageLimit>,
    current_round: u32,
) -> bool {
    let Some(limit) = limit else {
        return false;
    };
    match limit.period {
        UsagePeriod::Round => {
            let Some(record) = usage.get(&ability_index) else {
                return false;
            };
            if record.round != current_round {
                return false;
            }
            record.count >= limit.count
        }
    }
}

/// Record one firing of the ability at `ability_index` against the
/// current period, into `usage` (lazy reset when the stored record is
/// for a stale period). Shared by [`CardInPlay`] and
/// [`Investigator`](crate::state::Investigator).
pub fn bump_usage(
    usage: &mut BTreeMap<u8, AbilityUsageRecord>,
    ability_index: u8,
    current_round: u32,
) {
    let record = usage.entry(ability_index).or_insert(AbilityUsageRecord {
        round: current_round,
        count: 0,
    });
    if record.round != current_round {
        record.round = current_round;
        record.count = 0;
    }
    record.count = record.count.saturating_add(1);
}
```

  Then re-point the existing `CardInPlay` methods at them — replace the bodies of `CardInPlay::is_usage_exhausted` (~line 217) and `CardInPlay::bump_ability_usage` (~line 243):

```rust
    #[must_use]
    pub fn is_usage_exhausted(
        &self,
        ability_index: u8,
        limit: Option<UsageLimit>,
        current_round: u32,
    ) -> bool {
        usage_exhausted(&self.ability_usage, ability_index, limit, current_round)
    }

    pub fn bump_ability_usage(&mut self, ability_index: u8, current_round: u32) {
        bump_usage(&mut self.ability_usage, ability_index, current_round);
    }
```

  (Keep both methods' existing doc-comments; only the bodies change.)

- [ ] **3.4 — Add the field + delegating methods to `Investigator`.** In `crates/game-core/src/state/investigator.rs`:

  (a) Extend the `use super::card::{…}` import to bring in the helpers + `AbilityUsageRecord`. Change line 5 to:

```rust
use super::card::{
    bump_usage, usage_exhausted, AbilityUsageRecord, CardCode, CardInPlay, CardInstanceId,
};
use std::collections::BTreeMap;
```

  (b) Add the field to `struct Investigator { … }` (after `action_surcharge_spent_this_round`, or grouped near `cards_in_play` — place it just before `action_surcharge_spent_this_round` for readability):

```rust
    /// Per-ability "Limit X per \[period\]" usage records for this
    /// investigator's **own card** abilities (Roland Banks's once-per-round
    /// `[reaction]`). Mirrors [`CardInPlay::ability_usage`] — the investigator
    /// card is not a `CardInPlay`, so it needs its own usage home. Keyed by
    /// ability index within the investigator card's `abilities()`. Lazy
    /// reset: a stale-round record reads as 0 (see [`CardInPlay::ability_usage`]
    /// docs). Defaults to empty for backward-compatible deserialization.
    ///
    /// **Bridge (#118), sunset by #448:** retired when the investigator card
    /// becomes a real `CardInPlay`.
    ///
    /// [`CardInPlay::ability_usage`]: crate::state::CardInPlay::ability_usage
    #[serde(default)]
    pub ability_usage: BTreeMap<u8, AbilityUsageRecord>,
```

  (c) Add the delegating methods to the existing `impl Investigator` block (after `controlled_card_instances`, ~line 122):

```rust
    /// Whether this investigator's own-card ability at `ability_index` has
    /// reached its per-period [`UsageLimit`](crate::dsl::UsageLimit). Mirrors
    /// [`CardInPlay::is_usage_exhausted`] over [`ability_usage`](Self::ability_usage).
    #[must_use]
    pub fn is_usage_exhausted(
        &self,
        ability_index: u8,
        limit: Option<crate::dsl::UsageLimit>,
        current_round: u32,
    ) -> bool {
        usage_exhausted(&self.ability_usage, ability_index, limit, current_round)
    }

    /// Record one firing of this investigator's own-card ability at
    /// `ability_index` against the current period. Mirrors
    /// [`CardInPlay::bump_ability_usage`].
    pub fn bump_ability_usage(&mut self, ability_index: u8, current_round: u32) {
        bump_usage(&mut self.ability_usage, ability_index, current_round);
    }
```

- [ ] **3.5 — Initialize the field at the two construction sites.**
  - `crates/game-core/src/engine/dispatch/phases.rs` — add `ability_usage: std::collections::BTreeMap::new(),` to the `Investigator { … }` in the seating loop (see Task 2.4(c) final shape).
  - `crates/game-core/src/test_support/fixtures.rs` — add `ability_usage: std::collections::BTreeMap::new(),` to `test_investigator`'s `Investigator { … }`.

- [ ] **3.6 — Run the new test + the `CardInPlay` usage tests (regression):**

```sh
cargo test -p game-core --lib state::investigator::ability_usage_tests
cargo test -p game-core --lib state::card
```

  Expected: both pass; the refactored `CardInPlay` methods behave identically.

- [ ] **3.7 — Commit.**

```sh
cargo fmt && git add -A && git commit -m "engine: Investigator.ability_usage + shared usage helpers"
```

---

## Task 4 — `scan_investigator_card_reactions` + `CandidateSource::Investigator` routing

Add a reaction-scan source that finds reaction abilities on each investigator's **own card** (by `card_code`, skipping empty sentinels), and a `CandidateSource::Investigator(InvestigatorId)` variant so the fire/close path checks + bumps usage against `Investigator.ability_usage`. Wire the new scan into `scan_pending_triggers` (alongside `scan_act_agenda_reactions`).

**Files:**
- `crates/game-core/src/state/game_state.rs` — add `CandidateSource::Investigator(InvestigatorId)`; handle it in `CandidateSource::instance` (→ `None`).
- `crates/game-core/src/engine/dispatch/reaction_windows.rs` — add `scan_investigator_card_reactions`; extend `scan_pending_triggers`; teach `bump_usage_counter` the new source.
- `crates/cards/tests/roland_banks_seated.rs` — new integration test (seated Roland, no manual `cards_in_play` injection).

**Interfaces:**
- Consumes: `Investigator.card_code`, `Investigator.ability_usage`, `Investigator.is_usage_exhausted` / `bump_ability_usage` (Tasks 2 & 3), `reg.abilities_for`.
- Produces: `CandidateSource::Investigator(InvestigatorId)`; `fn scan_investigator_card_reactions(state, event, bucket) -> Vec<ResolutionCandidate>`.

### Steps

- [ ] **4.1 — Add the `CandidateSource` variant.** In `crates/game-core/src/state/game_state.rs`, inside `pub enum CandidateSource { … }` (after `Hand,`, ~line 1521):

```rust
    /// An ability on an investigator's **own card** (Roland Banks's seated
    /// `[reaction]`). Carries the controller id — the investigator card is
    /// not a `CardInPlay`, so it has no `CardInstanceId`; usage-limit checks
    /// and bumps point at `Investigator.ability_usage` instead. Fires by
    /// `code` like `Board`. **Bridge (#118), sunset by #448.**
    Investigator(InvestigatorId),
```

  And in `impl CandidateSource::instance` (~line 1530), extend the `None` arm:

```rust
    pub fn instance(self) -> Option<CardInstanceId> {
        match self {
            CandidateSource::InPlay(id) => Some(id),
            CandidateSource::Board | CandidateSource::Hand | CandidateSource::Investigator(_) => {
                None
            }
        }
    }
```

  > `InvestigatorId` is already in scope in `game_state.rs` (it's used throughout, e.g. `ResolutionCandidate.controller`). The new arm in `instance` returns `None` — the elder-sign / reaction effects don't self-reference a card instance.

- [ ] **4.2 — Write the failing integration test.** Create `crates/cards/tests/roland_banks_seated.rs`. This is the spec's keystone for Task 4: Roland's reaction fires from a **roster-seated** investigator with **no manual `cards_in_play` injection**, and is capped once per round. Model the seating + fight sequence on `roster_seating.rs` (StartScenario) and `roland_banks.rs` (the Fight defeat). Because seating opens a mulligan prompt and places Roland at the scenario starting location (which a bare `GameStateBuilder` has none of), this test drives the **engine reaction scan directly** against a hand-built *seated-shaped* state (real `card_code`, empty `cards_in_play`) — the point under test is that the scan source reads `card_code`, not the full StartScenario flow:

```rust
//! Roland Banks (01001) reacts from a **roster-seated** investigator — his
//! `[reaction]` fires with NO manual `cards_in_play` injection, sourced from
//! `Investigator.card_code` via the new `scan_investigator_card_reactions`.
//! Caps once per round through `Investigator.ability_usage`.
//!
//! Card text (`data/arkhamdb-snapshot/pack/core/core.json`, 01001):
//! > [reaction] After you defeat an enemy: Discover 1 clue at your
//! > location. (Limit once per round.)
//!
//! Integration test so it can install `cards::REGISTRY` in its own process.

use std::sync::Once;

use game_core::engine::{EngineOutcome, OptionId};
use game_core::event::Event;
use game_core::state::{
    AbilityUsageRecord, ChaosBag, ChaosToken, EnemyId, InvestigatorId, LocationId, Phase,
    TokenModifiers,
};
use game_core::test_support::{
    apply_no_commits, drive, test_enemy, test_investigator, test_location, GameStateBuilder,
    ScriptedResolver,
};
use game_core::{assert_event, assert_no_event, Action, PlayerAction};

const ROLAND: &str = "01001";

fn install_registry() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let _ = game_core::card_registry::install(cards::REGISTRY);
    });
}

/// Roland engaged with a 1-HP enemy, his investigator card represented ONLY by
/// `card_code` (the seated shape) — `cards_in_play` is empty, proving the
/// reaction is found by the new investigator-card scan, not the in-play scan.
fn seated_roland_with_enemy(
    round: u32,
) -> (InvestigatorId, EnemyId, LocationId, game_core::GameState) {
    install_registry();
    let inv_id = InvestigatorId(1);
    let enemy_id = EnemyId(100);
    let loc_id = LocationId(10);

    let mut inv = test_investigator(1);
    inv.card_code = game_core::state::CardCode::new(ROLAND);
    inv.current_location = Some(loc_id);
    inv.skills.combat = 4;
    assert!(inv.cards_in_play.is_empty(), "seated shape: no in-play injection");

    let mut enemy = test_enemy(100, "Mock Ghoul");
    enemy.fight = 1;
    enemy.max_health = 1;
    enemy.damage = 0;
    enemy.engaged_with = Some(inv_id);
    enemy.current_location = Some(loc_id);

    let mut loc = test_location(10, "Study");
    loc.clues = 2;

    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_round(round)
        .with_active_investigator(inv_id)
        .with_turn_order([inv_id])
        .with_investigator(inv)
        .with_enemy(enemy)
        .with_location(loc)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .build();
    (inv_id, enemy_id, loc_id, state)
}

fn fight_action(inv: InvestigatorId, enemy: EnemyId) -> Action {
    Action::Player(PlayerAction::Fight {
        investigator: inv,
        enemy,
    })
}

#[test]
fn seated_roland_reaction_fires_with_no_in_play_injection() {
    let (inv_id, enemy_id, loc_id, state) = seated_roland_with_enemy(0);

    let mut resolver = ScriptedResolver::new();
    resolver.commit_cards(&[]).pick_single(OptionId(0));
    let result = drive(state, fight_action(inv_id, enemy_id), resolver);

    assert_eq!(result.outcome, EngineOutcome::Done);
    assert_event!(
        result.events,
        Event::EnemyDefeated { enemy: e, by: Some(by) } if *e == enemy_id && *by == inv_id
    );
    assert_event!(
        result.events,
        Event::CluePlaced { investigator, count: 1 } if *investigator == inv_id
    );
    assert_eq!(result.state.locations[&loc_id].clues, 1);
    assert_eq!(result.state.investigators[&inv_id].clues, 1);

    // Usage bumped on the INVESTIGATOR (not a CardInPlay): ability index 0, round 0.
    let inv = &result.state.investigators[&inv_id];
    assert_eq!(
        inv.ability_usage.get(&0),
        Some(&AbilityUsageRecord::new(0, 1)),
        "seated Roland's reaction recorded one fire on Investigator.ability_usage",
    );
}

#[test]
fn seated_roland_reaction_capped_once_per_round() {
    let (inv_id, enemy_id, loc_id, mut state) = seated_roland_with_enemy(0);
    // Pretend Roland already reacted this round.
    state
        .investigators
        .get_mut(&inv_id)
        .unwrap()
        .bump_ability_usage(0, 0);

    let result = apply_no_commits(state, fight_action(inv_id, enemy_id));

    assert_eq!(result.outcome, EngineOutcome::Done);
    assert_event!(
        result.events,
        Event::EnemyDefeated { enemy: e, by: Some(by) } if *e == enemy_id && *by == inv_id
    );
    // Limit exhausted → no second reaction → no clue moved.
    assert_no_event!(result.events, Event::CluePlaced { .. });
    assert_eq!(result.state.locations[&loc_id].clues, 2);
}
```

- [ ] **4.3 — Run it; confirm it fails** (the seated reaction is not yet found — no `CluePlaced`, and `pick_single(OptionId(0))` finds no window):

```sh
cargo test -p cards --test roland_banks_seated
```

  Expected: failure on `seated_roland_reaction_fires_with_no_in_play_injection` (`assert_event!` for `CluePlaced` fails, or the resolver's scripted `pick_single` is unconsumed).

- [ ] **4.4 — Add `scan_investigator_card_reactions`.** In `crates/game-core/src/engine/dispatch/reaction_windows.rs`, add the function (model it on `scan_act_agenda_reactions`, ~line 302), placed right after it. Per-investigator, keyed by `inv.card_code`, skipping empty codes, with the usage check pointed at `Investigator.is_usage_exhausted`:

```rust
/// Scan every investigator's **own card** (by `Investigator.card_code`) for
/// `Trigger::OnEvent` reaction abilities matching `event` at `bucket` — makes
/// Roland Banks's `[reaction]` fire from a *seated* investigator, whose card is
/// not in any `cards_in_play` zone (so `scan_pending_triggers`' per-instance
/// loop can't reach it). Mirrors [`scan_act_agenda_reactions`]: candidate keyed
/// by `code`, `CandidateSource::Investigator(id)`, the per-period usage check
/// pointed at `Investigator.ability_usage`. Skips empty sentinel codes (the
/// pre-seated `test_support` path) and uninstalled registry / non-matching
/// abilities. **Bridge (#118), sunset by #448.**
fn scan_investigator_card_reactions(
    state: &GameState,
    event: &TimingEvent,
    bucket: EventTiming,
) -> Vec<ResolutionCandidate> {
    let Some(reg) = card_registry::current() else {
        return Vec::new();
    };
    // Active-investigator-first / turn-order order, matching `scan_pending_triggers`.
    let mut order: Vec<InvestigatorId> = Vec::with_capacity(state.turn_order.len());
    if let Some(active) = state.active_investigator {
        order.push(active);
    }
    for id in &state.turn_order {
        if Some(*id) != state.active_investigator {
            order.push(*id);
        }
    }

    let mut hits = Vec::new();
    for id in order {
        let Some(inv) = state.investigators.get(&id) else {
            continue;
        };
        // Empty sentinel ⇒ pre-seated path, no investigator-card abilities.
        if inv.card_code.as_str().is_empty() {
            continue;
        }
        let Some(abilities) = (reg.abilities_for)(&inv.card_code) else {
            continue;
        };
        for (idx, ability) in abilities.iter().enumerate() {
            let Trigger::OnEvent {
                pattern,
                timing,
                kind,
            } = &ability.trigger
            else {
                continue;
            };
            if *kind != TriggerKind::Reaction
                || *timing != bucket
                || !trigger_matches(event, pattern, *timing, id)
            {
                continue;
            }
            let ability_index = u8::try_from(idx)
                .expect("abilities vec exceeds u8::MAX — card-impl bug, abilities are tiny");
            // "Limit X per [period]" — skip if the investigator-card counter has
            // hit the cap this round (Roland's reaction is Limit 1 per round).
            if inv.is_usage_exhausted(ability_index, ability.usage_limit, state.round) {
                continue;
            }
            hits.push(ResolutionCandidate {
                code: inv.card_code.clone(),
                controller: id,
                ability_index,
                source: CandidateSource::Investigator(id),
            });
        }
    }
    hits
}
```

  Verify `Trigger`, `TriggerKind`, `EventTiming`, `TimingEvent`, `CandidateSource`, `ResolutionCandidate`, `InvestigatorId`, `card_registry`, `trigger_matches` are all already imported/in-module in `reaction_windows.rs` — they are (all used by `scan_act_agenda_reactions` / `scan_pending_triggers` above).

- [ ] **4.5 — Wire it into `scan_pending_triggers`.** In `crates/game-core/src/engine/dispatch/reaction_windows.rs`, at the tail of `scan_pending_triggers` (~line 290), add the investigator-card scan alongside the act/agenda scan:

```rust
    pending.extend(scan_act_agenda_reactions(state, event, bucket));
    pending.extend(scan_investigator_card_reactions(state, event, bucket));
    pending
```

  > This flows into both `queue_reaction_window` (single-bucket events, e.g. the after-defeat window) and `scan_reactions_at` (the coordinator), since both call `scan_pending_triggers`.

- [ ] **4.6 — Teach `bump_usage_counter` the new source.** In `crates/game-core/src/engine/dispatch/reaction_windows.rs`, `bump_usage_counter` (~line 909) currently `unreachable!`s on any non-`InPlay` source. Replace its body to branch on the source so an `Investigator` candidate bumps `Investigator.ability_usage`:

```rust
fn bump_usage_counter(state: &mut GameState, trigger: &ResolutionCandidate) {
    let current_round = state.round;
    match trigger.source {
        CandidateSource::InPlay(instance_id) => {
            let inv = state
                .investigators
                .get_mut(&trigger.controller)
                .unwrap_or_else(|| {
                    unreachable!(
                        "bump_usage_counter: controller {ctl:?} vanished while reaction window \
                         was open; state-corruption invariant violation",
                        ctl = trigger.controller,
                    )
                });
            let card = inv
                .cards_in_play
                .iter_mut()
                .chain(inv.threat_area.iter_mut())
                .find(|c| c.instance_id == instance_id)
                .unwrap_or_else(|| {
                    unreachable!(
                        "bump_usage_counter: instance {instance_id:?} vanished from controller \
                         {ctl:?}'s cards_in_play / threat area while reaction window was open; \
                         state-corruption invariant violation",
                        ctl = trigger.controller,
                    )
                });
            card.bump_ability_usage(trigger.ability_index, current_round);
        }
        CandidateSource::Investigator(id) => {
            let inv = state.investigators.get_mut(&id).unwrap_or_else(|| {
                unreachable!(
                    "bump_usage_counter: investigator {id:?} vanished while reaction window \
                     was open; state-corruption invariant violation"
                )
            });
            inv.bump_ability_usage(trigger.ability_index, current_round);
        }
        CandidateSource::Board | CandidateSource::Hand => unreachable!(
            "bump_usage_counter: a usage-limited candidate must be an in-play instance or an \
             investigator card (board / hand candidates carry no usage limits); candidate {trigger:?}"
        ),
    }
}
```

  Update the function's doc-comment lead-in (the "for in-play instances" wording) to mention investigator-card candidates too. The early-bump call site in `fire_pending_trigger` (`reaction_windows.rs:777`, `if usage_limit.is_some() { bump_usage_counter(...) }`) is unchanged — it already gates on `usage_limit.is_some()`, which is exactly Roland's `Some(Limit 1/round)`.

- [ ] **4.7 — Run the integration test; confirm green:**

```sh
cargo test -p cards --test roland_banks_seated
```

  Expected: both `seated_roland_reaction_fires_with_no_in_play_injection` and `seated_roland_reaction_capped_once_per_round` pass.

- [ ] **4.8 — Regression: the existing in-play Roland tests still pass** (the in-play injection path is untouched; the new scan adds candidates only for investigators whose `card_code` is set — `test_investigator`'s sentinel is empty, so `evidence.rs` / `roland_banks.rs` see no duplicate candidate):

```sh
cargo test -p cards --test roland_banks
cargo test -p cards --test evidence
cargo test -p game-core --lib engine::dispatch::reaction_windows
```

  Expected: all green. (Note: `roland_banks.rs` builds its investigator via `test_investigator(1)` whose `card_code` is the empty sentinel and injects the card into `cards_in_play`, so only the in-play scan fires there — no double window.)

- [ ] **4.9 — Commit.**

```sh
cargo fmt && git add -A && git commit -m "engine: scan_investigator_card_reactions + CandidateSource::Investigator (seated reactions)"
```

---

## Task 5 — Elder-sign ST.4 firing path

Add `elder_sign_modifier(state, registry, controller) -> i8` to the evaluator (looks up the controller's investigator card by `card_code`, reads its `Trigger::ElderSign { modifier }`, returns `eval_int_expr(modifier)`, `0` if none), and change the `TokenResolution::ElderSign` arm in `skill_test.rs` to add the bonus.

**Files:**
- `crates/game-core/src/engine/evaluator.rs` — add `pub(crate) fn elder_sign_modifier`.
- `crates/game-core/src/engine/dispatch/skill_test.rs` — import it; change the `ElderSign` arm in `run_resolution`.

**Interfaces:**
- Consumes: `Investigator.card_code`, `reg.abilities_for`, `Trigger::ElderSign`, `eval_int_expr` (private in `evaluator.rs`), `EvalContext::for_controller`.
- Produces: `pub(crate) fn elder_sign_modifier(state: &GameState, registry: &CardRegistry, controller: InvestigatorId) -> i8`.

### Steps

- [ ] **5.1 — Write the failing test.** Add a unit test to `crates/game-core/src/engine/evaluator.rs`'s `#[cfg(test)]` module (find it; the file has a tests module exercising `eval_quantity` / `constant_skill_modifier`). The elder-sign lookup needs a mock registry returning an `ElderSign` ability for one code. Use the existing test-registry pattern in that module (search for `install` / a `CardRegistry { metadata_for, abilities_for }` literal). Write:

```rust
    /// `elder_sign_modifier` reads the controller's investigator card's
    /// `Trigger::ElderSign { modifier }` and evaluates it. Roland's
    /// `Count(CluesAtControllerLocation)` returns the clue count at his
    /// location; an investigator with no elder-sign ability returns 0.
    #[test]
    fn elder_sign_modifier_reads_controller_card_clue_count() {
        use crate::dsl::{Ability, Effect, IntExpr, Quantity, Trigger};
        use crate::state::CardCode;

        // Mock registry: code "ES" carries a Count(CluesAtControllerLocation)
        // elder-sign; everything else has no abilities.
        fn abilities_for(code: &CardCode) -> Option<Vec<Ability>> {
            if code.as_str() == "ES" {
                Some(vec![Ability {
                    trigger: Trigger::ElderSign {
                        modifier: IntExpr::Count(Quantity::CluesAtControllerLocation),
                    },
                    costs: Vec::new(),
                    effect: Effect::Seq(Vec::new()),
                    usage_limit: None,
                }])
            } else {
                None
            }
        }
        fn metadata_for(_: &CardCode) -> Option<&'static crate::card_data::CardMetadata> {
            None
        }
        let registry = CardRegistry {
            metadata_for,
            abilities_for,
        };

        let inv_id = InvestigatorId(1);
        let loc_id = crate::state::LocationId(10);
        let mut inv = crate::test_support::test_investigator(1);
        inv.card_code = CardCode::new("ES");
        inv.current_location = Some(loc_id);
        let mut loc = crate::test_support::test_location(10, "Study");
        loc.clues = 2;
        let state = crate::state::GameStateBuilder::new()
            .with_investigator(inv)
            .with_location(loc)
            .build();

        assert_eq!(elder_sign_modifier(&state, &registry, inv_id), 2);

        // An investigator whose card has no elder-sign ability → 0.
        let inv_id2 = InvestigatorId(2);
        let mut inv2 = crate::test_support::test_investigator(2);
        inv2.card_code = CardCode::new("PLAIN");
        let state2 = crate::state::GameStateBuilder::new()
            .with_investigator(inv2)
            .build();
        assert_eq!(elder_sign_modifier(&state2, &registry, inv_id2), 0);
    }
```

  > Confirm the exact field layout of `Effect::Seq` (a no-op placeholder effect for the mock) by checking `dsl.rs` — `Effect::Seq(Vec<Effect>)` is a tuple variant; an empty vec is the inert choice. If the tests module needs `GameStateBuilder` / `CardRegistry` / `InvestigatorId` imports, add them (the module's other tests use `GameStateBuilder` — verify imports at the module head and extend as needed).

- [ ] **5.2 — Run it; confirm it fails to compile** (`elder_sign_modifier` does not exist):

```sh
cargo test -p game-core --lib engine::evaluator::tests::elder_sign_modifier_reads_controller_card_clue_count
```

  Expected: `cannot find function elder_sign_modifier`.

- [ ] **5.3 — Add `elder_sign_modifier`.** In `crates/game-core/src/engine/evaluator.rs`, add near the other `pub fn` query helpers (after `constant_skill_modifier`, ~line 1913). It mirrors `constant_skill_modifier`'s `(state, registry, controller)` shape and reuses the private `eval_int_expr`:

```rust
/// The controller's **elder-sign** skill-test modifier: the
/// `IntExpr` on their investigator card's `Trigger::ElderSign { modifier }`
/// ability, evaluated for the controller. Returns `0` when the controller has
/// no investigator card (empty sentinel `card_code`), the card isn't in the
/// registry, or it carries no elder-sign ability — so every investigator
/// without an elder-sign resolves exactly as before.
///
/// Called from the skill-test resolution's `TokenResolution::ElderSign` arm
/// (`skill_test.rs`); the bonus flows through the existing `Modifier` total.
///
/// **Scope (#118), sunset by #448:** handles only pure-modifier elder-signs.
/// Signs that also run an effect (Daisy / Agnes) are deferred — see
/// [`Trigger::ElderSign`](crate::dsl::Trigger::ElderSign).
#[must_use]
pub(crate) fn elder_sign_modifier(
    state: &GameState,
    registry: &CardRegistry,
    controller: InvestigatorId,
) -> i8 {
    let Some(inv) = state.investigators.get(&controller) else {
        return 0;
    };
    if inv.card_code.as_str().is_empty() {
        return 0;
    }
    let Some(abilities) = (registry.abilities_for)(&inv.card_code) else {
        return 0;
    };
    let ctx = EvalContext::for_controller(controller);
    for ability in &abilities {
        if let Trigger::ElderSign { modifier } = &ability.trigger {
            // A malformed elder-sign IntExpr (unexpressible Condition) yields
            // Err; treat it as no bonus rather than panicking mid-test — the
            // only in-scope IntExpr is Count(CluesAtControllerLocation), which
            // is always Ok.
            return eval_int_expr(state, &ctx, modifier).unwrap_or(0);
        }
    }
    0
}
```

  > `EvalContext::for_controller` is defined in this file (`evaluator.rs:159`), `eval_int_expr` is the private `fn` at `evaluator.rs:1119`, `Trigger` / `IntExpr` are already imported (`dsl.rs` use at the top). `CardRegistry` is imported (`evaluator.rs:61`).

- [ ] **5.4 — Run the evaluator test; confirm green:**

```sh
cargo test -p game-core --lib engine::evaluator::tests::elder_sign_modifier_reads_controller_card_clue_count
```

  Expected: pass.

- [ ] **5.5 — Write the failing skill-test arm test.** Add a unit test in `crates/game-core/src/engine/dispatch/skill_test.rs`'s `#[cfg(test)]` module. It resolves the **ElderSign** chaos token and asserts the total gained N (observed via the `SkillTestSucceeded { margin }` event, where `margin = total - difficulty`). This needs a mock registry installed — but `skill_test.rs` tests run in the `game-core` lib process where the `OnceLock` registry may already be set/unset. Use a **table over clue counts** and assert the margin moves by the clue count. Because the lib-test registry is a process-global `OnceLock`, prefer asserting this end-to-end in the **integration test** (Task 6) where `cards::REGISTRY` is installed; here, write a focused arm test that does NOT depend on the registry by exercising `elder_sign_modifier`'s `0`-bonus default:

```rust
    /// With no elder-sign ability on the controller's card (empty sentinel
    /// card_code), the ElderSign token resolves exactly as before: total =
    /// clamped skill value, bonus 0. Locks the behaviour-preserving default.
    #[test]
    fn elder_sign_token_adds_zero_without_an_elder_sign_ability() {
        use crate::state::ChaosToken;

        let inv = InvestigatorId(1);
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1)) // card_code = "" sentinel
            .with_active_investigator(inv)
            .build();
        // Willpower 3, difficulty 2, ElderSign token. Bonus 0 → total 3 → succeed by 1.
        state.chaos_bag.tokens = vec![ChaosToken::ElderSign];
        let mut events = Vec::new();
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };
        let out = start_skill_test(
            &mut cx,
            inv,
            SkillKind::Willpower,
            SkillTestKind::Plain,
            2,
            SkillTestFollowUp::None,
            None,
            None,
            None,
            0,
        );
        assert!(matches!(out, EngineOutcome::AwaitingInput { .. }));
        let out = finish_skill_test(&mut cx, &[]);
        let out = super::super::drive(&mut cx, out);
        assert_eq!(out, EngineOutcome::Done);
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::SkillTestSucceeded { margin, .. } if *margin == 1
            )),
            "ElderSign with no elder-sign ability → bonus 0 → succeed by 1: {events:?}",
        );
    }
```

  > The clue-scaling assertion (0/1/2 clues → +0/+1/+2) belongs in the **integration test** (Task 6), which installs the real `cards::REGISTRY` so `elder_sign_modifier` finds Roland's ability. This lib-level test pins the behaviour-preserving `0`-bonus path. Verify `ChaosToken::ElderSign` is the correct variant name (`crates/game-core/src/state/chaos_bag.rs`).

- [ ] **5.6 — Run it; confirm it fails** (today the arm is `(skill_value.max(0), …)` with no bonus — but with bonus 0 this would *pass* already!). **This test is a guard, not a driver** — it pins the default. To get a genuinely-failing driver before the arm change, also assert the arm *calls* `elder_sign_modifier`: temporarily skip — instead make the **arm change** the unit under test via the Task 6 integration test (the real driver). Run 5.5's guard to confirm it passes against the unmodified arm:

```sh
cargo test -p game-core --lib engine::dispatch::skill_test::tests::elder_sign_token_adds_zero_without_an_elder_sign_ability
```

  Expected: passes against the current arm (bonus path is 0). Keep it as the regression guard.

- [ ] **5.7 — Change the `ElderSign` arm.** In `crates/game-core/src/engine/dispatch/skill_test.rs`, `run_resolution` (~line 304). First compute the bonus just before the `match resolution` (after `let skill_value = …`, line 304):

```rust
    let skill_value = sum_skill_value(cx.state, investigator, skill, kind, indices_u8);
    let elder_sign_bonus = card_registry::current()
        .map_or(0, |reg| elder_sign_modifier(cx.state, reg, investigator));
    let (total, fail_reason) = match resolution {
        TokenResolution::Modifier(n) => {
            (skill_value.saturating_add(n).max(0), FailureReason::Total)
        }
        TokenResolution::ElderSign => (
            skill_value.saturating_add(elder_sign_bonus).max(0),
            FailureReason::Total,
        ),
        TokenResolution::AutoFail => (0, FailureReason::AutoFail),
    };
```

  And add `elder_sign_modifier` to the evaluator import at the top of `skill_test.rs` (line 18):

```rust
use super::super::evaluator::{
    constant_skill_modifier, elder_sign_modifier, pending_skill_modifier, push_effect, EvalContext,
};
```

  `card_registry` is already imported (`skill_test.rs:10`).

- [ ] **5.8 — Run the guard test + the broader skill-test suite; confirm green:**

```sh
cargo test -p game-core --lib engine::dispatch::skill_test
```

  Expected: all pass (the `0`-bonus arm is behaviour-preserving; the guard still succeeds-by-1).

- [ ] **5.9 — Commit.**

```sh
cargo fmt && git add -A && git commit -m "engine: elder_sign_modifier + ElderSign ST.4 firing"
```

---

## Task 6 — Roland's elder-sign ability + end-to-end integration

Add the `Trigger::ElderSign` ability to Roland's `abilities()`, update its card-level doc + unit test, and add an integration test that drives a seated-shaped Roland through a skill test resolving the elder-sign token at 0/1/2 clues.

**Files:**
- `crates/cards/src/impls/roland_banks.rs` — add the elder-sign ability + a unit test; update the module doc.
- `crates/cards/tests/roland_elder_sign.rs` — new integration test.

**Interfaces:**
- Consumes: `Trigger::ElderSign`, `IntExpr::Count`, `Quantity::CluesAtControllerLocation` (from `card_dsl::dsl`); `elder_sign_modifier` firing path (Task 5).
- Produces: a second `Ability` in Roland's `abilities()`.

### Steps

- [ ] **6.1 — Write the failing card unit test.** In `crates/cards/src/impls/roland_banks.rs`, the existing test `abilities_are_one_reaction_with_once_per_round_limit` asserts `abilities.len() == 1`. Update it to expect 2 and add a new test for the elder-sign shape. Replace the `assert_eq!(abilities.len(), 1);` with `assert_eq!(abilities.len(), 2);` in that test, and add:

```rust
    #[test]
    fn abilities_include_elder_sign_clue_count_modifier() {
        use card_dsl::dsl::{IntExpr, Quantity, Trigger};
        let abilities = super::abilities();
        assert_eq!(abilities.len(), 2);
        // The elder-sign half: +1 for each clue on your location.
        assert_eq!(
            abilities[1].trigger,
            Trigger::ElderSign {
                modifier: IntExpr::Count(Quantity::CluesAtControllerLocation),
            },
        );
        assert!(abilities[1].usage_limit.is_none());
    }
```

  And in `abilities_are_one_reaction_with_once_per_round_limit`, keep the existing reaction assertions but change them to index `abilities[0]` (already the case) and bump the length to 2. (Rename that test to `first_ability_is_the_reaction_with_once_per_round_limit` for accuracy if desired — optional.)

- [ ] **6.2 — Run it; confirm it fails** (`abilities()` returns 1, no `ElderSign`):

```sh
cargo test -p cards --lib impls::roland_banks
```

  Expected: `abilities_include_elder_sign_clue_count_modifier` fails on the length / index.

- [ ] **6.3 — Add the ability.** In `crates/cards/src/impls/roland_banks.rs`:

  (a) Extend the `use card_dsl::dsl::{…}` import (line 35) to add the elder-sign pieces:

```rust
use card_dsl::dsl::{
    discover_clue, reaction_on_event, Ability, EventPattern, EventTiming, IntExpr, LocationTarget,
    Quantity, Trigger, UsageLimit, UsagePeriod,
};
```

  (b) Add the second ability to the `vec![…]` in `abilities()` (after the reaction, before the closing `]`):

```rust
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![
        reaction_on_event(
            EventPattern::EnemyDefeated {
                by_controller: true,
                code: None,
            },
            EventTiming::After,
            discover_clue(LocationTarget::YourLocation, 1),
        )
        .with_usage_limit(UsageLimit {
            count: 1,
            period: UsagePeriod::Round,
        }),
        // [elder_sign] effect: +1 for each clue on your location.
        // (data/arkhamdb-snapshot/pack/core/core.json, 01001.) No builder —
        // ElderSign is config-on-trigger; effect is inert (the engine reads the
        // trigger's `modifier`, not an Effect), so use an empty Seq.
        Ability {
            trigger: Trigger::ElderSign {
                modifier: IntExpr::Count(Quantity::CluesAtControllerLocation),
            },
            costs: Vec::new(),
            effect: card_dsl::dsl::Effect::Seq(Vec::new()),
            usage_limit: None,
        },
    ]
}
```

  > `Ability` is `#[non_exhaustive]` *within other crates* — but this struct literal is in `cards`, a *different* crate, so `#[non_exhaustive]` blocks struct-literal construction! **Check:** the existing `reaction_on_event(...).with_usage_limit(...)` goes through builders precisely because `cards` can't name all fields. **Resolution:** add a builder `pub fn elder_sign(modifier: IntExpr) -> Ability` to `crates/card-dsl/src/dsl.rs` (alongside `on_skill_test_resolution`, ~line 1224) and call it here instead:

  In `crates/card-dsl/src/dsl.rs`:

```rust
/// Construct a [`Trigger::ElderSign`] ability carrying the elder-sign's
/// printed skill-test `modifier`. Costs are empty and the effect is an inert
/// empty `Seq` — the engine reads the modifier off the trigger, not the
/// effect (the bonus flows through the skill-test `Modifier` total).
#[must_use]
pub fn elder_sign(modifier: IntExpr) -> Ability {
    Ability {
        trigger: Trigger::ElderSign { modifier },
        costs: Vec::new(),
        effect: Effect::Seq(Vec::new()),
        usage_limit: None,
    }
}
```

  Then in `roland_banks.rs`, import `elder_sign` and use it:

```rust
use card_dsl::dsl::{
    discover_clue, elder_sign, reaction_on_event, Ability, EventPattern, EventTiming, IntExpr,
    LocationTarget, Quantity, Trigger, UsageLimit, UsagePeriod,
};
```

```rust
        // [elder_sign] effect: +1 for each clue on your location. (01001.)
        elder_sign(IntExpr::Count(Quantity::CluesAtControllerLocation)),
```

  Add a builder unit test to `card-dsl`'s tests module (alongside Task 1's):

```rust
    #[test]
    fn elder_sign_builder_constructs_the_trigger() {
        let a = elder_sign(IntExpr::Count(Quantity::CluesAtControllerLocation));
        assert_eq!(
            a.trigger,
            Trigger::ElderSign {
                modifier: IntExpr::Count(Quantity::CluesAtControllerLocation),
            },
        );
        assert!(a.costs.is_empty());
        assert!(a.usage_limit.is_none());
        assert!(matches!(a.effect, Effect::Seq(ref v) if v.is_empty()));
    }
```

  (Use `elder_sign` / `Effect` from the test module's imports; the module imports `super::*` or names types directly — match the surrounding style.)

  (c) Update the module doc-comment at the top of `roland_banks.rs` — it currently says `abilities() ships only the [reaction] half. The [elder_sign] half stays as the engine-wide +0 placeholder`. Replace the **Scope** paragraph (lines 14–19) with:

```rust
//! # Scope
//!
//! `abilities()` ships both halves: the `[reaction]` and the
//! `[elder_sign]`. The elder-sign is a [`Trigger::ElderSign`] carrying
//! `IntExpr::Count(Quantity::CluesAtControllerLocation)` — "+1 for each clue
//! on your location" — which the skill-test resolution adds to the total when
//! Roland's elder-sign token is drawn (#118). Reached via the investigator-card
//! bridge (`Investigator.card_code`); sunset by #448.
```

  And update the doc-link line at the bottom (`[`Trigger::ElderSign`]: card_dsl::dsl::Trigger::ElderSign`) — add it to the intra-doc-link block.

- [ ] **6.4 — Run the card + builder unit tests; confirm green:**

```sh
cargo test -p cards --lib impls::roland_banks
cargo test -p card-dsl elder_sign
```

  Expected: all pass.

- [ ] **6.5 — Write the failing integration test.** Create `crates/cards/tests/roland_elder_sign.rs` — seated-shaped Roland, drive a Willpower test resolving the **ElderSign** token at 0/1/2 clues at his location, assert the total gained the clue count (via `SkillTestSucceeded.margin` / `SkillTestFailed.by`). Difficulty is chosen so the clue count is observable in the margin:

```rust
//! End-to-end: seated Roland Banks (01001) draws his `[elder_sign]` token
//! during a skill test → "+1 for each clue on your location" adds his
//! location's clue count to the total (0 / 1 / 2 clues).
//!
//! Card text (`data/arkhamdb-snapshot/pack/core/core.json`, 01001):
//! > [elder_sign] effect: +1 for each clue on your location.
//!
//! Integration test so it installs the real `cards::REGISTRY` (which carries
//! Roland's `Trigger::ElderSign` ability) in its own process.

use std::sync::Once;

use game_core::engine::{apply, drive, EngineOutcome};
use game_core::event::Event;
use game_core::state::{
    CardCode, ChaosBag, ChaosToken, InvestigatorId, LocationId, Phase, SkillKind, TokenModifiers,
};
use game_core::test_support::{test_investigator, test_location, GameStateBuilder, ScriptedResolver};
use game_core::{Action, PlayerAction};

const ROLAND: &str = "01001";

fn install_registry() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let _ = game_core::card_registry::install(cards::REGISTRY);
    });
}

/// Drive a Willpower-3 test at difficulty 3 with the ElderSign token, Roland
/// seated (card_code set, NOT in cards_in_play) at a location holding
/// `clues`. Returns the resolved events for outcome assertions.
fn run_elder_sign_test(clues: u8) -> Vec<Event> {
    install_registry();
    let inv_id = InvestigatorId(1);
    let loc_id = LocationId(10);

    let mut inv = test_investigator(1);
    inv.card_code = CardCode::new(ROLAND);
    inv.current_location = Some(loc_id);
    inv.skills.willpower = 3; // base 3

    let mut loc = test_location(10, "Study");
    loc.clues = clues;

    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_active_investigator(inv_id)
        .with_turn_order([inv_id])
        .with_investigator(inv)
        .with_location(loc)
        .with_chaos_bag(ChaosBag::new([ChaosToken::ElderSign]))
        .with_token_modifiers(TokenModifiers::default())
        .build();

    // Bare PerformSkillTest: Willpower vs difficulty 3. ElderSign bonus = clues.
    // total = 3 + clues; succeed iff total >= 3 (always, here) by margin = clues.
    let action = Action::Player(PlayerAction::PerformSkillTest {
        investigator: inv_id,
        skill: SkillKind::Willpower,
        difficulty: 3,
    });
    let mut resolver = ScriptedResolver::new();
    resolver.commit_cards(&[]);
    let result = drive(state, action, resolver);
    assert_eq!(result.outcome, EngineOutcome::Done);
    result.events
}

#[test]
fn elder_sign_adds_zero_clues() {
    let events = run_elder_sign_test(0);
    assert!(
        events.iter().any(|e| matches!(
            e,
            Event::SkillTestSucceeded { margin, .. } if *margin == 0
        )),
        "0 clues → +0 → succeed by 0: {events:?}",
    );
}

#[test]
fn elder_sign_adds_one_clue() {
    let events = run_elder_sign_test(1);
    assert!(
        events.iter().any(|e| matches!(
            e,
            Event::SkillTestSucceeded { margin, .. } if *margin == 1
        )),
        "1 clue → +1 → succeed by 1: {events:?}",
    );
}

#[test]
fn elder_sign_adds_two_clues() {
    let events = run_elder_sign_test(2);
    assert!(
        events.iter().any(|e| matches!(
            e,
            Event::SkillTestSucceeded { margin, .. } if *margin == 2
        )),
        "2 clues → +2 → succeed by 2: {events:?}",
    );
}
```

  > Verify the exact `PlayerAction::PerformSkillTest` field names against `crates/game-core/src/action.rs` (the variant exists — `perform_skill_test` is its dispatch wrapper at `skill_test.rs:1260`). If `drive` isn't re-exported from `game_core::engine`, use the `game_core::test_support::drive` helper (the seated reaction tests import `drive` from `test_support`) — match whichever the other integration tests use. Verify `ChaosBag::new` / `ScriptedResolver` / `apply` imports; drop `apply` if unused.

- [ ] **6.6 — Run it; confirm it fails for the 1-clue / 2-clue cases pre-arm** — wait: Task 5 already shipped the arm, so by the time Task 6 runs, the integration test should PASS once the ability (6.3) is in `abilities()`. To see a genuine red→green, run it **after 6.3 but check that without the ability it'd be 0**: the cleanest sequencing is to write the integration test here (6.5), run it (6.6) and confirm it passes (the arm from Task 5 + the ability from 6.3 together produce the bonus). If you want a red first, momentarily comment out the `elder_sign(...)` line in `abilities()`, run, see all-but-zero fail, then restore:

```sh
cargo test -p cards --test roland_elder_sign
```

  Expected (with the ability present): all three pass. (With the ability commented out, `elder_sign_adds_one_clue` / `_two_clues` fail with margin 0 — proving the ability is load-bearing.)

- [ ] **6.7 — Commit.**

```sh
cargo fmt && git add -A && git commit -m "card: Roland Banks elder-sign (+1 per clue at location) + integration (#118)"
```

---

## Final gauntlet (before opening the PR)

- [ ] **F.1 — Run the full CI gauntlet locally** (all warnings-as-errors):

```sh
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
cargo build -p web --target wasm32-unknown-unknown
cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings
```

  Expected: every command exits 0. (The wasm jobs matter — `game-core` compiles to `wasm32`; `card_code` / `ability_usage` / the new scan are all pure data + logic, no `std::time` / I/O, so they're wasm-clean.)

- [ ] **F.2 — Phase doc** — update `docs/phases/phase-N-<slug>.md` (find the phase owning #118) as the **final** commit once CI is green on the opened PR: move #118 to the Closed table, flip its Arc/Ordering row to `✅ PR #N`, drop any settled Open question, add a Decisions-made entry only if load-bearing for a future PR (e.g. "investigator-card bridge is `card_code` + `ability_usage` + `scan_investigator_card_reactions`, sunset by #448" passes the test). Follow `docs/phases/README.md`.

---

## Self-Review

**Spec coverage (Section 2):**
- ✅ `Trigger::ElderSign { modifier: IntExpr }` — Task 1 (config-on-trigger, same derives, serde round-trip).
- ✅ Bonus flows through the existing `Modifier` total, no `Effect::ModifySkillTestTotal` — Task 5 changes only the `TokenResolution::ElderSign` arm to `saturating_add(bonus).max(0)`, keeping the `ElderSign` label.
- ✅ ST.4 firing via `elder_sign_modifier(state, reg, controller)` returning `0` when none — Task 5; `eval_int_expr` exposed via a `pub(crate)` wrapper (`elder_sign_modifier`) rather than making `eval_int_expr` itself public (minimal surface).
- ✅ The bridge: `Investigator.card_code` (Task 2, set at seating from `RosterEntry.investigator`, empty sentinel for the pre-seated path), `Investigator.ability_usage` (Task 3, mirroring `CardInPlay`), `scan_investigator_card_reactions` (Task 4, mirrors `scan_act_agenda_reactions`, keyed by code, usage against `Investigator.ability_usage`).
- ✅ Investigator card stays out of `cards_in_play` — the scan reads `card_code`; no `CardInPlay` injection in the seated tests (asserted explicitly in Task 4.2).
- ✅ Roland's card (01001) gains the elder-sign ability — Task 6, text verified verbatim against `core.json`.
- ✅ Tests at every spec-named level: card unit (6.1), engine `elder_sign_modifier` (5.1), behaviour-preserving `0`-bonus arm (5.5), seated reaction fires + once-per-round cap (4.2), integration elder-sign 0/1/2 clues (6.5).
- ✅ Deferrals (Daisy/Agnes effect-bearing elder-signs, #448 sunset) noted in doc-comments only (Task 1, 3, 4, 5, 6 doc text) — no tasks added.

**Placeholder scan:** No "TBD" / "similar to" / prose-only steps. Every code block is concrete and transcribed from the real current source. Two **verify-before-typing** flags are called out explicitly (not placeholders): (a) `CandidateSource::instance` arm + `bump_usage_counter` match must stay exhaustive (Task 4.1, 4.6 give the full match); (b) `Ability` is `#[non_exhaustive]` so `cards` cannot struct-literal it — resolved by adding the `elder_sign` builder to `card-dsl` (Task 6.3), mirroring every other ability builder.

**Type consistency:**
- `elder_sign_modifier(state: &GameState, registry: &CardRegistry, controller: InvestigatorId) -> i8` matches `constant_skill_modifier`'s shape and the arm's `i8` add.
- `CardCode` gains `Default` (Task 2.3) so `#[serde(default)]` on `card_code` compiles; the empty-string sentinel is the documented "pre-seated" marker, and every new codepath (`scan_investigator_card_reactions`, `elder_sign_modifier`) skips `card_code.as_str().is_empty()`.
- `Investigator.ability_usage: BTreeMap<u8, AbilityUsageRecord>` matches `CardInPlay.ability_usage` exactly; the shared `usage_exhausted` / `bump_usage` free fns take `&BTreeMap<u8, AbilityUsageRecord>` / `&mut …` and are reused by both (DRY per spec).
- `CandidateSource::Investigator(InvestigatorId)` carries the controller (no `CardInstanceId`); `instance()` → `None`; `bump_usage_counter` routes it to `Investigator.bump_ability_usage`. The early-bump call site is unchanged (gated on `usage_limit.is_some()`).
- The `resolved` tuple widening in `phases.rs` (`(Skills, u8, u8, String, Vec<CardCode>)` → `+ CardCode`) is the single seating-path change; the two other `Investigator { … }` construction sites (`test_investigator`, no others — `combat.rs` hits are format strings, not constructors) get the new fields.

**Construction-site completeness:** `Investigator { … }` is built in exactly two places — `phases.rs:100` (seating) and `fixtures.rs:36` (`test_investigator`). Both updated for `card_code` (Task 2) and `ability_usage` (Task 3). The `GameStateBuilder` takes a pre-built `Investigator` (via `test_investigator`), so no builder change is needed.
