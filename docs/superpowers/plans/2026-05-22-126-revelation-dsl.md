# #126 — Revelation DSL + on-draw resolution — implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Land the DSL primitive `Trigger::Revelation` + `EventPattern::CardRevealed`, the engine's on-draw resolution path that runs Revelation effects on encounter draws, and a synthetic treachery that proves the wiring end-to-end.

**Architecture:** Mirror the existing `Trigger::OnPlay` shape in `card-dsl` and the `play_card` handler's evaluator-call pattern in `game-core`. New `Event::CardRevealed` + `EngineRecord::EncounterCardRevealed` follow the additive-sibling pattern PR #132 used for the encounter-deck shuffle event. Synthetic test cards live in a new file under `crates/scenarios/src/test_fixtures/`, gated by the existing `test_fixtures` feature; integration tests install a fresh `TEST_REGISTRY` value (their own process, no `OnceLock` contention).

**Tech Stack:** Rust 2021, `serde`, `card-dsl` (pure data), `game-core` (kernel), `scenarios` (synthetic fixtures).

**Spec:** `docs/superpowers/specs/2026-05-22-126-revelation-dsl-design.md` is the authoritative design — re-read it when starting and when in doubt about a decision.

**Branch name:** `engine/revelation-dsl`.

**PR procedure:** CLAUDE.md's 8-step PR procedure applies. This plan covers steps 1 (local CI gauntlet), 2 (commits on a feature branch), 7 (phase-doc update as last commit), and the PR-open hand-off. CI watch + addressing CI failures (steps 4–6) and the user-approved merge (step 8) are driven by the human after the PR opens.

---

## Design decisions locked in before coding

These are settled now to keep task-by-task execution mechanical. If implementation surfaces a reason to revisit, raise it before pressing on.

1. **Synthetic treachery's Revelation effect is `Effect::GainResources { target: Controller, amount: 1 }`.** The spec example ("lose 1 resource") would need a new DSL primitive — and the snapshot has no Phase-4-scope treachery with that exact shape, so the "two-consumers-before-DSL-grows" rule applies. The proof we need is *"a Revelation effect ran and mutated state observably,"* not anything thematic. Resources go up by 1 from the default-5 starting wallet — easy to assert. The Phase-7 implementer who lands the first real lose-resources treachery gets to add `LoseResources` (or signed-delta `GainResources`) with two consumers in hand.
2. **`InvestigatorTarget::Controller` not `Active`.** The drawing investigator passed to `apply_effect` is bound as `ctx.controller`. `Active` would require the integration test to set `state.active_investigator`, which is irrelevant noise for the Mythos draw path (Mythos has no active investigator in general).
3. **`Event::CardRevealed` emits BEFORE Revelation resolves.** This is the rules-correct interposition point for Before-timing reaction listeners (none wired today; structural for #52). The handler is a documented exception to the validate-first / mutate-second contract — the PR description must call this out explicitly, citing `play_card`'s in-CLAUDE.md caveat as precedent.
4. **No new `CardInstanceId` minted for the revealed treachery's evaluator source.** `EvalContext::for_controller(investigator)` with `source: None` matches the `play_card` path for events (which also have no in-play instance). The Revelation source isn't an in-play card.
5. **`SYNTH_TREACHERY_CODE = "_synth_treachery"`.** Underscore prefix can't collide with real ArkhamDB codes (which are digit-prefixed five-char strings). Future synthetic codes follow this convention.
6. **`TEST_REGISTRY` lives in the `synth_cards` module, re-exported through `scenarios::test_fixtures`.** Gated behind the existing `test_fixtures` feature (default-on per PR #130).
7. **`encounter_card_revealed` lives in `crates/game-core/src/engine/dispatch.rs` alongside the existing engine-record handlers.** Same file, same dispatch arm pattern as `encounter_deck_shuffled` (PR #132).

---

## File map

- **Create:**
  - `crates/scenarios/src/test_fixtures/synth_cards.rs` — synthetic treachery metadata + abilities + `TEST_REGISTRY`.
  - `crates/scenarios/tests/encounter_reveal.rs` — integration test binary (own process; installs `TEST_REGISTRY`).
- **Modify:**
  - `crates/card-dsl/src/dsl.rs` — `Trigger::Revelation`, `EventPattern::CardRevealed { card_type: Option<CardType> }`, `pub fn revelation()` builder, unit tests.
  - `crates/game-core/src/event.rs` — `Event::CardRevealed { investigator, code, card_type }` variant + serde test.
  - `crates/game-core/src/action.rs` — `EngineRecord::EncounterCardRevealed { investigator }` variant.
  - `crates/game-core/src/engine/dispatch.rs` — `encounter_card_revealed` handler, arm in `apply_engine_record`, drop the `#[allow(dead_code)]` from `draw_encounter_top` / `reshuffle_encounter_discard` (real caller has landed), unit tests using a local fake registry.
  - `crates/scenarios/src/test_fixtures/mod.rs` — declare the new `synth_cards` module + re-export.
  - `crates/scenarios/src/test_fixtures/synthetic.rs` — extend `setup()` to push `SYNTH_TREACHERY_CODE` onto `encounter_deck`.
  - `crates/scenarios/src/lib.rs` — leave `REGISTRY` alone; the integration test installs `TEST_REGISTRY` from `test_fixtures::synth_cards`. Touch only if compile errors force it.
  - `docs/phases/phase-4-scenario-plumbing.md` — LAST commit only; do not touch mid-PR.

The DSL crate's serde derives mean `CardType` already implements `Serialize`/`Deserialize`/`Eq`/`Hash`/`Copy`, so `EventPattern::CardRevealed { card_type: Option<CardType> }` should compile without further derive plumbing — verify in Task 3.

Every commit must compile cleanly with the full CI gauntlet:

```sh
RUSTFLAGS="-D warnings"    cargo test --all --all-features
                           cargo clippy --all-targets --all-features -- -D warnings
                           cargo fmt --check
RUSTDOCFLAGS="-D warnings"  cargo doc --workspace --no-deps --all-features
                           cargo build -p web --target wasm32-unknown-unknown
```

---

## Task 1: Set up the feature branch + commit the plan

**Files:**
- Add: `docs/superpowers/plans/2026-05-22-126-revelation-dsl.md` (this file).

- [ ] **Step 1: Create the feature branch from main**

```bash
git checkout main
git pull
git checkout -b engine/revelation-dsl
```

- [ ] **Step 2: Commit the plan file**

In this repo `docs/superpowers/` is tracked in git (PR #132 set the convention — the encounter-deck plan was committed alongside the code). Add the plan file as the branch's first commit:

```bash
git add docs/superpowers/plans/2026-05-22-126-revelation-dsl.md
git commit -m "$(cat <<'EOF'
docs: implementation plan for #126

Refs #126.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: Add `Trigger::Revelation` variant + `revelation()` builder

**Files:**
- Modify: `crates/card-dsl/src/dsl.rs`

- [ ] **Step 1: Write the failing builder test**

Append to the existing `#[cfg(test)] mod tests` block at the bottom of `crates/card-dsl/src/dsl.rs`:

```rust
/// The `revelation` builder produces the new Trigger variant with
/// the given effect. Distinct from OnPlay / OnCommit at the type
/// level so the compiler enforces the difference at every match site.
#[test]
fn revelation_builder_constructs_treachery_shape() {
    let ability = revelation(gain_resources(InvestigatorTarget::Controller, 1));
    assert_eq!(ability.trigger, Trigger::Revelation);
    assert!(matches!(
        ability.effect,
        Effect::GainResources {
            target: InvestigatorTarget::Controller,
            amount: 1,
        },
    ));
    assert!(ability.costs.is_empty());
    assert!(ability.usage_limit.is_none());
}

#[test]
fn revelation_distinct_from_other_triggers() {
    assert_ne!(Trigger::Revelation, Trigger::OnPlay);
    assert_ne!(Trigger::Revelation, Trigger::OnCommit);
    assert_ne!(Trigger::Revelation, Trigger::Constant);
}

#[test]
fn revelation_ability_round_trips_through_serde_json() {
    let original = revelation(gain_resources(InvestigatorTarget::Controller, 1));
    let json = serde_json::to_string(&original).expect("serialize");
    let recovered: Ability = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(original, recovered);
}
```

- [ ] **Step 2: Run the tests to confirm compile failure**

Run:
```bash
cargo test -p card-dsl revelation 2>&1 | head -30
```

Expected: compile errors — `no variant Revelation` on `Trigger`, `no function revelation` in scope.

- [ ] **Step 3: Add the `Trigger::Revelation` variant**

In `crates/card-dsl/src/dsl.rs`, inside the `enum Trigger { ... }` block, add the new variant immediately after `OnCommit` (keeping triggers grouped by when-they-fire reads roughly chronologically: enter-play → commit → reveal → activated/event/resolution):

```rust
    /// Fires when the owning card is revealed from the encounter deck.
    ///
    /// First consumer: the synthetic treachery in
    /// `scenarios::test_fixtures::synth_cards`. Real Phase-7+ treachery
    /// cards will replace the synthetic fixture's role as primary
    /// consumer.
    ///
    /// Distinct from [`OnPlay`](Self::OnPlay) — Revelation fires for
    /// engine-driven encounter draws (Mythos phase, scenario forced
    /// effects), not for cards played from a player's hand. Treacheries
    /// are never in a player's hand; they're encounter-bag content.
    ///
    /// The engine's on-draw resolution path
    /// ([`encounter_card_revealed`](https://docs.rs/game-core/0/game_core/engine/index.html))
    /// runs every `Trigger::Revelation` ability on the drawn card through
    /// the DSL evaluator, then discards the treachery (or hands off to
    /// the spawn handler for enemies — landing in #127).
    Revelation,
```

- [ ] **Step 4: Add the `revelation()` builder**

Find the other builder free functions (after `on_event`, before `activated`). Add the sibling:

```rust
/// Construct a [`Trigger::Revelation`]-driven [`Ability`] wrapping
/// the given effect. Mirrors [`on_play`] / [`on_commit`]; costs and
/// usage limits are empty (Revelation effects pay nothing and have
/// no per-period cap — the rules treat each draw as a fresh
/// occurrence).
#[must_use]
pub fn revelation(effect: Effect) -> Ability {
    Ability {
        trigger: Trigger::Revelation,
        costs: Vec::new(),
        effect,
        usage_limit: None,
    }
}
```

- [ ] **Step 5: Run the tests to verify pass**

Run:
```bash
cargo test -p card-dsl revelation
```

Expected: 3 tests pass.

- [ ] **Step 6: Local doc build, to catch broken intra-doc links**

Run:
```bash
RUSTDOCFLAGS="-D warnings" cargo doc -p card-dsl --no-deps --all-features
```

Expected: no warnings. (The doc-comment above references `encounter_card_revealed`; if rustdoc can't resolve the cross-crate link it will warn — drop the bracket form to a bare backtick if needed: `[`encounter_card_revealed`]` → `` `encounter_card_revealed` ``.)

- [ ] **Step 7: Commit**

```bash
git add crates/card-dsl/src/dsl.rs
git commit -m "$(cat <<'EOF'
infra: add Trigger::Revelation + revelation() builder

Adds the DSL primitive for treachery effects. First consumer is the
synthetic treachery landing later in this PR; real Phase-7+ treachery
cards will follow.

Distinct enum variant from OnPlay / OnCommit so the compiler enforces
the timing distinction at every match site.

Refs #126.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: Add `EventPattern::CardRevealed { card_type: Option<CardType> }`

**Files:**
- Modify: `crates/card-dsl/src/dsl.rs`

- [ ] **Step 1: Write the failing pattern test**

Append to the same `#[cfg(test)] mod tests` block in `crates/card-dsl/src/dsl.rs`:

```rust
/// `EventPattern::CardRevealed { card_type: Some(...) }` and
/// `{ card_type: None }` are distinct variants with serde
/// round-tripping. Locks the wire shape now so #52's persistence
/// doesn't surprise later.
#[test]
fn card_revealed_pattern_round_trips_through_serde_json() {
    use card_data::CardType;
    let any = EventPattern::CardRevealed { card_type: None };
    let treachery = EventPattern::CardRevealed {
        card_type: Some(CardType::Treachery),
    };
    for original in [any, treachery] {
        let json = serde_json::to_string(&original).expect("serialize");
        let recovered: EventPattern = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(original, recovered);
    }
}

#[test]
fn card_revealed_distinct_from_enemy_defeated() {
    use card_data::CardType;
    let revealed_treachery = EventPattern::CardRevealed {
        card_type: Some(CardType::Treachery),
    };
    let enemy_defeated = EventPattern::EnemyDefeated {
        by_controller: true,
    };
    assert_ne!(revealed_treachery, enemy_defeated);
}
```

If the import path `card_data::CardType` doesn't resolve inside this test module, switch it to `crate::card_data::CardType` (the existing `EnemyDefeated` test uses unqualified `EventPattern::EnemyDefeated` so `CardType` is likely not yet imported in this module).

- [ ] **Step 2: Confirm compile failure**

Run:
```bash
cargo test -p card-dsl card_revealed 2>&1 | head -20
```

Expected: `no variant CardRevealed found for enum EventPattern`.

- [ ] **Step 3: Add the `CardRevealed` variant**

In `crates/card-dsl/src/dsl.rs`, inside `enum EventPattern { ... }`, add the new variant after the existing `EnemyDefeated`:

```rust
    /// An encounter card was revealed (drawn from the encounter deck
    /// and announced via the engine's on-draw path). `card_type`
    /// narrows the match: `None` matches any reveal, `Some(card_type)`
    /// matches only reveals whose card type equals the given value.
    ///
    /// Canonical listener shape: a hypothetical Forewarned-style
    /// cancellation effect would set `card_type: Some(CardType::Treachery)`
    /// to react only to treachery reveals. No card uses this pattern in
    /// the Phase-4 scope; the DSL surface lands here, the engine's
    /// reaction-window machinery (#52) fires it.
    ///
    /// **Why `card_type` not `by_controller`:** encounter draws are
    /// engine-driven, not card-controlled. The `EnemyDefeated`-style
    /// `by_controller: bool` qualifier doesn't fit. Treachery-vs-enemy
    /// narrowing is the load-bearing distinction for hypothetical
    /// listener cards instead.
    CardRevealed {
        /// Narrow the match by card type. `None` = any reveal.
        card_type: Option<crate::card_data::CardType>,
    },
```

- [ ] **Step 4: Verify the existing test that pins `EnemyDefeated`-distinctness still passes**

Run:
```bash
cargo test -p card-dsl on_event_distinct_from_other_triggers_and_internally
```

Expected: pass. The existing test doesn't enumerate every `EventPattern` variant, so the new variant is non-breaking.

- [ ] **Step 5: Run the new tests to verify pass**

Run:
```bash
cargo test -p card-dsl card_revealed
```

Expected: 2 new tests pass.

- [ ] **Step 6: Run the full `card-dsl` test gauntlet**

Run:
```bash
RUSTFLAGS="-D warnings" cargo test -p card-dsl --all-features
cargo clippy -p card-dsl --all-targets --all-features -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc -p card-dsl --no-deps --all-features
```

Expected: all green, no warnings.

- [ ] **Step 7: Commit**

```bash
git add crates/card-dsl/src/dsl.rs
git commit -m "$(cat <<'EOF'
infra: add EventPattern::CardRevealed { card_type }

Adds the OnEvent pattern for listener cards that key off "an
encounter card has just been revealed" — Forewarned-style
cancellation effects and "after a treachery is revealed" reactions
will use this surface when they land.

Encounter draws are engine-driven, so the EnemyDefeated-style
by_controller qualifier doesn't fit; card_type: Option<CardType>
is the load-bearing narrowing instead.

Refs #126.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: Add `Event::CardRevealed` variant

**Files:**
- Modify: `crates/game-core/src/event.rs`

- [ ] **Step 1: Write the failing serde roundtrip test**

Append to the existing `#[cfg(test)] mod encounter_deck_event_tests` block at the bottom of `crates/game-core/src/event.rs` (or add a sibling `mod card_revealed_event_tests` — prefer the sibling for clarity):

```rust
#[cfg(test)]
mod card_revealed_event_tests {
    use super::*;
    use crate::state::CardCode;
    use card_dsl::card_data::CardType;

    #[test]
    fn card_revealed_event_serde_roundtrip() {
        let ev = Event::CardRevealed {
            investigator: InvestigatorId(1),
            code: CardCode("_synth_treachery".into()),
            card_type: CardType::Treachery,
        };
        let json = serde_json::to_string(&ev).expect("serialize");
        let back: Event = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, ev);
    }
}
```

- [ ] **Step 2: Confirm compile failure**

Run:
```bash
cargo test -p game-core card_revealed_event 2>&1 | head -20
```

Expected: `no variant CardRevealed found for enum Event`.

- [ ] **Step 3: Add the variant**

In `crates/game-core/src/event.rs`, find an appropriate spot near the other card-zone events (`CardPlayed`, `CardDiscarded`, `CardExhausted`). Add immediately after `CardExhausted`:

```rust
    /// An encounter card was revealed from the encounter deck. Fires
    /// before any [`Trigger::Revelation`](card_dsl::dsl::Trigger::Revelation)
    /// effects on the card resolve — the card has been drawn off the
    /// deck and identified, but its Revelation effect has not yet
    /// applied. Before-timing reaction listeners (#52's machinery, not
    /// wired in Phase 4) hook this point to interpose or cancel.
    ///
    /// Emitted by [`encounter_card_revealed`](crate::engine::dispatch)
    /// in response to [`EngineRecord::EncounterCardRevealed`](crate::action::EngineRecord::EncounterCardRevealed).
    CardRevealed {
        /// The investigator whose draw produced this reveal. For
        /// Phase-4 Mythos draws, this is the investigator taking their
        /// Mythos turn; for forced reveals (scenario effects), the
        /// scenario module names the controller.
        investigator: InvestigatorId,
        /// The revealed card's code.
        code: CardCode,
        /// The card's type, as resolved by the card registry at reveal
        /// time. Redundant with the metadata lookup but baked into the
        /// event so consumers don't need the registry to filter.
        card_type: CardType,
    },
```

Then add the import for `CardType` at the top of the file:

```rust
use card_dsl::card_data::CardType;
```

(`card_dsl` is already a workspace dep of `game-core` — see `crates/game-core/src/dsl.rs` re-exports.)

- [ ] **Step 4: Fix any exhaustive matches the compiler flags**

Run:
```bash
cargo check --all --all-features 2>&1 | grep -E "non-exhaustive|error" | head -30
```

`Event` is `#[non_exhaustive]`, so external matches with `_ => ...` won't break. In-crate matches that exhaustively enumerate `Event` variants need an arm for `CardRevealed`. Likely candidates: assertion-helper macros in `crates/game-core/src/engine/mod.rs` (look for `assert_event!` macro internals), and any debug-formatter helpers.

For each error, add a benign arm (most don't need special treatment — a `_` fallback is fine).

- [ ] **Step 5: Run the test, verify pass**

Run:
```bash
cargo test -p game-core card_revealed_event
```

Expected: pass.

- [ ] **Step 6: Full game-core gauntlet**

```bash
RUSTFLAGS="-D warnings" cargo test -p game-core --all-features
RUSTDOCFLAGS="-D warnings" cargo doc -p game-core --no-deps --all-features
```

Expected: green.

- [ ] **Step 7: Commit**

```bash
git add crates/game-core/src/event.rs
git commit -m "$(cat <<'EOF'
engine: add Event::CardRevealed for encounter draws

Announces an encounter card being revealed from the deck. Emitted
before any Revelation effect resolves so Before-timing reaction
listeners (#52, not yet wired) can interpose.

card_type is denormalized onto the event so consumers can filter
without a registry lookup.

Refs #126.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: Add `EngineRecord::EncounterCardRevealed`

**Files:**
- Modify: `crates/game-core/src/action.rs`

- [ ] **Step 1: Write the failing serde roundtrip test**

Append a new test module at the bottom of `crates/game-core/src/action.rs`:

```rust
#[cfg(test)]
mod encounter_card_revealed_action_tests {
    use super::*;

    #[test]
    fn encounter_card_revealed_engine_record_serde_roundtrip() {
        let rec = EngineRecord::EncounterCardRevealed {
            investigator: InvestigatorId(1),
        };
        let json = serde_json::to_string(&rec).expect("serialize");
        let back: EngineRecord = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, rec);
    }
}
```

- [ ] **Step 2: Confirm compile failure**

```bash
cargo test -p game-core encounter_card_revealed_action 2>&1 | head -20
```

Expected: `no variant EncounterCardRevealed found for enum EngineRecord`.

- [ ] **Step 3: Add the variant**

In `crates/game-core/src/action.rs`, inside `enum EngineRecord { ... }`, add the new variant after `EncounterDeckShuffled`:

```rust
    /// The named investigator reveals the top card of the encounter
    /// deck. Emitted by #69's Mythos draw loop when it lands; in
    /// #126's tests, issued directly to exercise the on-draw path.
    ///
    /// The reveal flow: the dispatch handler (#126's
    /// `encounter_card_revealed` in `engine::dispatch`) draws the top
    /// of the deck (transparently reshuffling discard if needed),
    /// emits [`Event::CardRevealed`](crate::Event::CardRevealed),
    /// runs any [`Trigger::Revelation`](card_dsl::dsl::Trigger::Revelation)
    /// abilities through the DSL evaluator, then routes by card type
    /// (treachery → discard; enemy → spawn handler from #127).
    EncounterCardRevealed {
        /// The investigator whose draw produced this reveal.
        investigator: InvestigatorId,
    },
```

- [ ] **Step 4: Fix exhaustive matches**

Run:
```bash
cargo check --all --all-features 2>&1 | grep -E "non-exhaustive|error" | head -20
```

The primary site is `apply_engine_record` in `dispatch.rs` — Task 6 fills its arm. Until then, add a temporary stub arm so the crate compiles:

In `crates/game-core/src/engine/dispatch.rs`, inside `apply_engine_record`'s match block (after the `EncounterDeckShuffled` arm):

```rust
        EngineRecord::EncounterCardRevealed { .. } => EngineOutcome::Rejected {
            reason: "EncounterCardRevealed handler lands in the next commit".into(),
        },
```

(Stubbing in this commit so the crate compiles for the test; Task 6 replaces this with the real handler.)

- [ ] **Step 5: Run the test, verify pass**

```bash
cargo test -p game-core encounter_card_revealed_action
```

Expected: pass.

- [ ] **Step 6: Commit**

```bash
git add crates/game-core/src/action.rs crates/game-core/src/engine/dispatch.rs
git commit -m "$(cat <<'EOF'
engine: add EngineRecord::EncounterCardRevealed action variant

Records an encounter-deck draw in the action log. The dispatch
handler lands in the next commit; this commit stubs the arm to
"handler lands next" so the crate compiles.

Replay determinism: the action carries only the drawing investigator;
the specific card revealed is determined by the deck state + seeded
RNG, so replay reproduces the same reveal sequence.

Refs #126.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: Implement the `encounter_card_revealed` dispatch handler

**Files:**
- Modify: `crates/game-core/src/engine/dispatch.rs`

This is the load-bearing task. The handler validates the registry installation, draws the top of the encounter deck, looks up metadata, emits `Event::CardRevealed`, then branches on card type:
- **Treachery:** run every `Trigger::Revelation` ability through `apply_effect`, then push the code onto `encounter_discard`.
- **Enemy:** reject with a "lands in #127" message (stub for the next PR to flip).
- **Other:** reject with "invalid encounter card type."

The handler is a documented exception to validate-first / mutate-second: `draw_encounter_top` mutates the deck and `Event::CardRevealed` emits before the enemy-arm reject. The PR description must call this out explicitly (see "PR description" task below).

- [ ] **Step 1: Write the failing tests using a process-local fake registry**

The `play_card` handler's tests can't help here — those run in integration tests with a real registry. For unit-level coverage, write a `#[cfg(test)] mod encounter_card_revealed_tests` block alongside the existing `encounter_deck_helper_tests` at the bottom of `dispatch.rs`.

**Caveat about the `OnceLock` registry:** `card_registry::install` is process-global, so multiple `game-core` unit tests can't each install their own registry. Two strategies:

1. *Don't install* — exercise the "no registry" reject path with no install, and exercise the happy path / enemy stub via an integration test in `crates/scenarios/tests/encounter_reveal.rs` (Task 9) where a process-isolated install is fine.
2. *Install once* — if another `game-core` unit test in the same binary has already installed the registry (it hasn't, as of #132), use `OnceLock::set`'s idempotent semantics: `let _ = install(my_registry);`.

Pick strategy 1 for the unit tests (no install), and rely on Task 9's integration test for the happy path + enemy stub. This keeps the unit-test surface narrow and matches how `play_card`'s real-registry behavior is exercised.

Add the test block:

```rust
#[cfg(test)]
mod encounter_card_revealed_tests {
    use super::*;
    use crate::state::CardCode;
    use crate::test_support::{test_investigator, TestGame};

    #[test]
    fn rejects_when_no_card_registry_installed() {
        // Note: this test relies on the absence of a card-registry
        // install in this binary. If a future `game-core` unit test
        // installs the registry, this test will need to move to its
        // own integration binary or use a fresh process.
        let mut state = TestGame::new()
            .with_investigator(test_investigator(1))
            .build();
        // Seed the encounter deck so we can prove the reject fires
        // *before* the draw (validate-first for the registry check).
        state.encounter_deck.push_back(CardCode("anything".into()));
        let pre_deck_len = state.encounter_deck.len();
        let mut events = Vec::new();

        let outcome = apply_engine_record(
            &mut state,
            &mut events,
            &EngineRecord::EncounterCardRevealed {
                investigator: InvestigatorId(1),
            },
        );

        match outcome {
            EngineOutcome::Rejected { reason } => {
                assert!(
                    reason.contains("no card registry installed"),
                    "unexpected reject reason: {reason:?}",
                );
            }
            other => panic!("expected Rejected, got {other:?}"),
        }
        assert_eq!(
            state.encounter_deck.len(),
            pre_deck_len,
            "deck must be untouched on registry-missing reject",
        );
        assert!(
            events.is_empty(),
            "no events should fire on registry-missing reject; got {events:?}",
        );
    }

    #[test]
    fn rejects_when_deck_and_discard_both_empty() {
        // This test exercises the empty-deck reject path. It must run
        // in a binary where the registry IS installed; otherwise the
        // no-registry reject fires first. Use a process-local fake
        // registry install — `OnceLock::set` returns Err on the second
        // call, so we just try-and-ignore. If the registry is already
        // installed from another test, that's fine for this assertion.
        //
        // ALTERNATIVE: skip this case here and exercise it from the
        // integration binary in Task 9, where we control the install
        // explicitly. The integration test is the authoritative home
        // for the empty-deck case — drop this stub if it's flaky.
        //
        // For now, write the stub as #[ignore] and revisit in Task 9.
    }
}
```

(The empty-deck case is covered authoritatively in Task 9's integration test where a fresh process gets a clean install. Don't burn time wrestling the `OnceLock` here.)

- [ ] **Step 2: Run the test, verify compile failure**

```bash
cargo test -p game-core encounter_card_revealed_tests 2>&1 | head -30
```

Expected: compile success (the test references existing surface), and the test FAILS at runtime because the stub from Task 5 returns "handler lands in the next commit" not "no card registry installed."

- [ ] **Step 3: Implement the handler**

In `crates/game-core/src/engine/dispatch.rs`:

(a) Replace the `EncounterCardRevealed { .. }` arm in `apply_engine_record` with a call to the new handler:

```rust
        EngineRecord::EncounterCardRevealed { investigator } => {
            encounter_card_revealed(state, events, *investigator)
        }
```

(b) Add the handler function. Place it near `encounter_deck_shuffled` (same crate-record family). Imports needed at the top of the file if not already present: `card_data::CardType` (already imported), `card_registry`, `dsl::Trigger`, and `super::evaluator::{apply_effect, EvalContext}`. Verify via `grep` before adding duplicate imports.

```rust
/// Handler for [`EngineRecord::EncounterCardRevealed`].
///
/// Drives the on-draw resolution path for one encounter card:
///
/// 1. Validate that a card registry is installed (reject with
///    `"no card registry installed"` if not).
/// 2. Draw the top of the encounter deck via [`draw_encounter_top`]
///    (transparently reshuffles discard back in if the deck is
///    empty). Reject with `"encounter deck and discard both empty"`
///    if both piles are exhausted.
/// 3. Look up the drawn card's metadata via the installed registry.
///    Reject with `"unknown card code: {code}"` if the registry
///    doesn't know the code.
/// 4. Emit [`Event::CardRevealed`] with the drawn code and the
///    metadata's `card_type`.
/// 5. Branch on `card_type`:
///    - [`CardType::Treachery`]: run every [`Trigger::Revelation`]
///      ability on the card through the DSL evaluator (controller
///      = drawing investigator, no source instance — matches the
///      `play_card` path for events), then push the code onto
///      `state.encounter_discard`.
///    - [`CardType::Enemy`]: reject with `"encounter enemy spawn
///      lands in #127"`. Stub for #127 to replace with the real
///      spawn handler.
///    - Any other type: reject with `"invalid encounter card type:
///      {kind:?}"`. Encounter decks should only contain treachery
///      and enemy cards per the Rules Reference.
///
/// # Validate-first contract caveat
///
/// `draw_encounter_top` mutates `state.encounter_deck` and
/// `state.encounter_discard`, and `Event::CardRevealed` emits, all
/// BEFORE the enemy / unknown-type rejects fire. This is a
/// documented exception to the project's validate-first /
/// mutate-second convention. Two reasons it's acceptable:
///
/// 1. The enemy arm is unreachable in #126's intended scope (the
///    synthetic fixture's deck contains only the synthetic
///    treachery). The reject exists as a regression test for #127
///    to flip, not as a real runtime path.
/// 2. #127 replaces the enemy reject with the real spawn branch,
///    after which the only "early emit" is `Event::CardRevealed`
///    itself — which is intentional, because Before-timing
///    reaction listeners (#52, not wired) need the event to fire
///    before Revelation resolves (rules-correct interposition
///    point).
///
/// Compare to `play_card`'s documented mid-resolution caveat in
/// CLAUDE.md: same shape, same rationale.
fn encounter_card_revealed(
    state: &mut GameState,
    events: &mut Vec<Event>,
    investigator: InvestigatorId,
) -> EngineOutcome {
    let Some(registry) = card_registry::current() else {
        return EngineOutcome::Rejected {
            reason: "EncounterCardRevealed: no card registry installed".into(),
        };
    };

    let Some(code) = draw_encounter_top(state, events) else {
        return EngineOutcome::Rejected {
            reason: "EncounterCardRevealed: encounter deck and discard both empty".into(),
        };
    };

    let Some(metadata) = (registry.metadata_for)(&code) else {
        return EngineOutcome::Rejected {
            reason: format!("EncounterCardRevealed: unknown card code: {code:?}").into(),
        };
    };
    let card_type = metadata.card_type;

    // Emit BEFORE Revelation resolves — see caveat above.
    events.push(Event::CardRevealed {
        investigator,
        code: code.clone(),
        card_type,
    });

    match card_type {
        CardType::Treachery => {
            let abilities = (registry.abilities_for)(&code).unwrap_or_default();
            let ctx = EvalContext::for_controller(investigator);
            for ability in abilities.iter().filter(|a| a.trigger == Trigger::Revelation) {
                let outcome = apply_effect(state, events, &ability.effect, ctx);
                if !matches!(outcome, EngineOutcome::Done) {
                    return outcome;
                }
            }
            state.encounter_discard.push(code);
            EngineOutcome::Done
        }
        CardType::Enemy => EngineOutcome::Rejected {
            reason: "EncounterCardRevealed: encounter enemy spawn lands in #127".into(),
        },
        other => EngineOutcome::Rejected {
            reason: format!(
                "EncounterCardRevealed: invalid encounter card type {other:?}; \
                 encounter decks contain only treachery and enemy cards",
            )
            .into(),
        },
    }
}
```

(c) Drop the `#[allow(dead_code)]` attributes from `reshuffle_encounter_discard` and `draw_encounter_top` (the real caller has landed). Find them at the comment lines `// Real (non-test) callers land in #126's on-draw resolution path.` and remove BOTH that comment and the `#[allow(dead_code)]` directly below it.

- [ ] **Step 4: Run the unit test**

```bash
cargo test -p game-core encounter_card_revealed_tests
```

Expected: the `rejects_when_no_card_registry_installed` test passes. (Ignored tests stay ignored — they're rehearsals; the integration test is authoritative.)

- [ ] **Step 5: Run full game-core test gauntlet**

```bash
RUSTFLAGS="-D warnings" cargo test -p game-core --all-features
cargo clippy -p game-core --all-targets --all-features -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc -p game-core --no-deps --all-features
```

Expected: all green. Watch for:
- Clippy flagging the `format!` calls in reject reasons — they're already in the codebase pattern; fine.
- Doc warnings on the cross-crate intra-doc links to `Trigger::Revelation` — if rustdoc can't resolve them, fall back to inline-code formatting (`` `Trigger::Revelation` ``) rather than `[`Trigger::Revelation`]`.

- [ ] **Step 6: Commit**

```bash
git add crates/game-core/src/engine/dispatch.rs
git commit -m "$(cat <<'EOF'
engine: on-draw resolution handler (encounter_card_revealed)

Drives the encounter-deck draw path:
- validates registry installation
- draws the top card (reshuffling discard if needed)
- emits Event::CardRevealed
- runs Trigger::Revelation abilities on treacheries through the
  DSL evaluator, then discards
- stubs the enemy arm with a "lands in #127" reject
- rejects other card types as invalid encounter content

Documented exception to validate-first/mutate-second: the deck
draw and Event::CardRevealed emission happen before the enemy /
invalid-type rejects fire. This is the rules-correct interposition
point for Before-timing reactions (#52); the enemy stub is
unreachable in #126's intended scope (synthetic deck contains only
the synthetic treachery). Precedent: play_card's documented
mid-resolution caveat in CLAUDE.md.

Refs #126.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 7: Add synthetic test cards module (`synth_cards.rs`)

**Files:**
- Create: `crates/scenarios/src/test_fixtures/synth_cards.rs`
- Modify: `crates/scenarios/src/test_fixtures/mod.rs`

- [ ] **Step 1: Write the new module file**

Create `crates/scenarios/src/test_fixtures/synth_cards.rs`:

```rust
//! Synthetic test cards used by Phase-4's integration tests.
//!
//! These don't exist in any printed pack — they're vehicles for
//! proving engine wiring end-to-end without depending on real corpus
//! cards. The card codes use an underscore prefix (`_synth_*`) to
//! guarantee no collision with ArkhamDB's digit-prefixed codes.
//!
//! Exposed alongside [`TEST_REGISTRY`] — integration tests install
//! this registry instead of `cards::REGISTRY` so they don't pull in
//! the full 5600-line corpus.

use std::sync::OnceLock;

use card_dsl::card_data::{CardMetadata, CardType, Class, SkillIcons};
use card_dsl::dsl::{gain_resources, revelation, Ability, InvestigatorTarget};
use game_core::card_registry::CardRegistry;
use game_core::state::CardCode;

/// Code for the synthetic treachery. Underscore prefix guarantees no
/// collision with ArkhamDB's digit-prefixed five-char codes.
pub const SYNTH_TREACHERY_CODE: &str = "_synth_treachery";

/// Static metadata for the synthetic treachery. Fields populated with
/// trivial defaults — only `code`, `name`, `card_type`, and
/// `deck_limit`/`quantity` carry meaning for the tests; the rest
/// satisfy `CardMetadata`'s non-`#[non_exhaustive]` struct shape.
fn synth_treachery_metadata() -> CardMetadata {
    CardMetadata {
        code: SYNTH_TREACHERY_CODE.to_owned(),
        name: "Synthetic Treachery".to_owned(),
        class: Class::Mythos,
        card_type: CardType::Treachery,
        cost: None,
        xp: None,
        text: Some("Revelation - You gain 1 resource. (Synthetic; not a printed card.)".to_owned()),
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
        health: None,
        sanity: None,
        deck_limit: 1,
        quantity: 1,
        pack_code: "_synth".to_owned(),
        position: 1,
        is_fast: false,
    }
}

fn synth_treachery_metadata_static() -> &'static CardMetadata {
    static M: OnceLock<CardMetadata> = OnceLock::new();
    M.get_or_init(synth_treachery_metadata)
}

/// `metadata_for` function pointer used by [`TEST_REGISTRY`].
fn metadata_for(code: &CardCode) -> Option<&'static CardMetadata> {
    if code.as_str() == SYNTH_TREACHERY_CODE {
        Some(synth_treachery_metadata_static())
    } else {
        None
    }
}

/// `abilities_for` function pointer used by [`TEST_REGISTRY`].
fn abilities_for(code: &CardCode) -> Option<Vec<Ability>> {
    match code.as_str() {
        SYNTH_TREACHERY_CODE => Some(vec![revelation(gain_resources(
            InvestigatorTarget::Controller,
            1,
        ))]),
        _ => None,
    }
}

/// Ready-made [`CardRegistry`] backed by this module's synthetic
/// cards. Integration tests install this via
/// [`game_core::card_registry::install`] instead of `cards::REGISTRY`
/// so they don't pull in the full corpus.
///
/// Process-isolated: each `cargo test --test` binary gets its own
/// process, so this install doesn't collide with `cards::REGISTRY`
/// installs in other test binaries.
pub const TEST_REGISTRY: CardRegistry = CardRegistry {
    metadata_for,
    abilities_for,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_for_resolves_synth_treachery() {
        let code = CardCode(SYNTH_TREACHERY_CODE.into());
        let meta = metadata_for(&code).expect("synth treachery must resolve");
        assert_eq!(meta.code, SYNTH_TREACHERY_CODE);
        assert_eq!(meta.card_type, CardType::Treachery);
    }

    #[test]
    fn metadata_for_returns_none_for_unknown_code() {
        let code = CardCode("not_in_synth_registry".into());
        assert!(metadata_for(&code).is_none());
    }

    #[test]
    fn abilities_for_returns_one_revelation_ability() {
        let code = CardCode(SYNTH_TREACHERY_CODE.into());
        let abilities = abilities_for(&code).expect("synth treachery must have abilities");
        assert_eq!(abilities.len(), 1);
        assert_eq!(
            abilities[0].trigger,
            card_dsl::dsl::Trigger::Revelation,
        );
    }

    #[test]
    fn test_registry_dispatches_to_module_functions() {
        let code = CardCode(SYNTH_TREACHERY_CODE.into());
        assert!((TEST_REGISTRY.metadata_for)(&code).is_some());
        assert!((TEST_REGISTRY.abilities_for)(&code).is_some());
    }
}
```

- [ ] **Step 2: Declare the new module**

In `crates/scenarios/src/test_fixtures/mod.rs`, add the new module declaration:

```rust
//! Synthetic / minimal scenario fixtures.
//!
//! These exist only to exercise the engine's scenario-module wiring;
//! they are *not* part of any shipped campaign. Gated behind
//! `cfg(any(test, feature = "test_fixtures"))` at the crate root so
//! they never ship in a release build.

pub mod synth_cards;
pub mod synthetic;
```

- [ ] **Step 3: Run the module's unit tests**

```bash
cargo test -p scenarios --features test_fixtures synth_cards
```

Expected: 4 tests pass.

- [ ] **Step 4: Full scenarios gauntlet**

```bash
RUSTFLAGS="-D warnings" cargo test -p scenarios --all-features
cargo clippy -p scenarios --all-targets --all-features -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc -p scenarios --no-deps --all-features
```

Expected: green.

- [ ] **Step 5: Commit**

```bash
git add crates/scenarios/src/test_fixtures/synth_cards.rs crates/scenarios/src/test_fixtures/mod.rs
git commit -m "$(cat <<'EOF'
test: synthetic test cards + TEST_REGISTRY for #126

Adds crates/scenarios/src/test_fixtures/synth_cards.rs with one
synthetic treachery (code "_synth_treachery") whose Revelation
effect gains the controller 1 resource. TEST_REGISTRY exposes the
synthetic metadata + abilities as a CardRegistry value so
integration tests can install it instead of the full
cards::REGISTRY corpus.

Underscore-prefixed codes guarantee no collision with ArkhamDB's
digit-prefixed real codes.

Refs #126.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 8: Seed the synthetic fixture's encounter deck

**Files:**
- Modify: `crates/scenarios/src/test_fixtures/synthetic.rs`

- [ ] **Step 1: Write the failing test**

Append to the bottom of `crates/scenarios/src/lib.rs`'s existing test module (or add a sibling test inside `synthetic.rs`):

```rust
#[cfg(test)]
mod setup_seeds_encounter_deck_tests {
    use super::test_fixtures::{synth_cards::SYNTH_TREACHERY_CODE, synthetic};
    use game_core::state::CardCode;

    #[test]
    fn synthetic_setup_seeds_encounter_deck_with_synth_treachery() {
        let state = synthetic::setup();
        assert_eq!(
            state.encounter_deck.len(),
            1,
            "synthetic fixture must seed exactly one encounter card",
        );
        assert_eq!(
            state.encounter_deck[0],
            CardCode(SYNTH_TREACHERY_CODE.into()),
        );
    }
}
```

- [ ] **Step 2: Confirm test fails**

```bash
cargo test -p scenarios --features test_fixtures synthetic_setup_seeds_encounter_deck 2>&1 | head -20
```

Expected: failure — `encounter_deck.len() == 0` (the current setup leaves it empty).

- [ ] **Step 3: Update `synthetic::setup()`**

In `crates/scenarios/src/test_fixtures/synthetic.rs`, modify `setup()` to push the synth treachery code onto the encounter deck. Adjust the imports at the top of the file accordingly:

```rust
use card_dsl::card_data::Class; // if needed; check existing imports
use game_core::event::Event;
use game_core::scenario::{Resolution, ScenarioId, ScenarioModule};
use game_core::state::{CardCode, GameState, InvestigatorId, Phase};
use game_core::test_support::{test_investigator, test_location, TestGame};

use super::synth_cards::SYNTH_TREACHERY_CODE;
```

(Verify which imports are actually needed by the diff — only add `CardCode` and the `SYNTH_TREACHERY_CODE` use if they're not present. Leave existing imports untouched.)

Modify the `setup()` body:

```rust
pub fn setup() -> GameState {
    let mut state = TestGame::new()
        .with_investigator(test_investigator(1))
        .with_location(test_location(10, "Demo Location"))
        .with_turn_order([InvestigatorId(1)])
        .with_scenario_id(ScenarioId::new(ID))
        .build();
    state
        .encounter_deck
        .push_back(CardCode(SYNTH_TREACHERY_CODE.into()));
    state
}
```

Update the doc comment on `setup()` to mention the encounter-deck seeding:

```rust
/// Build the initial [`GameState`] for this fixture: one
/// investigator, one location, `scenario_id` set, `turn_order`
/// populated, encounter deck seeded with one copy of
/// [`synth_cards::SYNTH_TREACHERY_CODE`]. Phase = Mythos, round =
/// 0 — ready for
/// [`PlayerAction::StartScenario`](game_core::PlayerAction::StartScenario).
///
/// The encounter-deck seeding gives #126's `encounter_reveal.rs`
/// integration test something to draw from when it exercises the
/// on-draw resolution path. The pre-existing `StartScenario` →
/// `Resolution::Won` flow (see `synthetic_resolution.rs`) is
/// unaffected because the auto-resolved demo path doesn't draw
/// encounter cards.
///
/// [`synth_cards::SYNTH_TREACHERY_CODE`]: super::synth_cards::SYNTH_TREACHERY_CODE
```

- [ ] **Step 4: Run the test, verify pass**

```bash
cargo test -p scenarios --features test_fixtures synthetic_setup_seeds_encounter_deck
```

Expected: pass.

- [ ] **Step 5: Verify the existing `synthetic_resolution.rs` integration test still passes**

The seeded encounter deck shouldn't affect the auto-resolved demo flow (which doesn't draw encounters), but verify:

```bash
cargo test -p scenarios --test synthetic_resolution
```

Expected: still passes — `StartScenario` advances to Investigation + round 1, `detect_resolution` fires, `Event::ScenarioResolved` emits. The seeded encounter card just sits in the deck.

- [ ] **Step 6: Full scenarios gauntlet**

```bash
RUSTFLAGS="-D warnings" cargo test -p scenarios --all-features
cargo clippy -p scenarios --all-targets --all-features -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc -p scenarios --no-deps --all-features
```

Expected: green.

- [ ] **Step 7: Commit**

```bash
git add crates/scenarios/src/test_fixtures/synthetic.rs crates/scenarios/src/lib.rs
git commit -m "$(cat <<'EOF'
test: seed synthetic fixture's encounter deck

Synthetic fixture's setup() now pushes SYNTH_TREACHERY_CODE onto
encounter_deck so the encounter_reveal.rs integration test has a
real card to draw. Existing synthetic_resolution.rs path is
unaffected — auto-resolved demo flow doesn't draw encounters.

Refs #126.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 9: Integration test (`encounter_reveal.rs`)

**Files:**
- Create: `crates/scenarios/tests/encounter_reveal.rs`

This file is its own cargo binary — it gets a fresh process and can install `TEST_REGISTRY` without colliding with other test binaries.

- [ ] **Step 1: Write the integration test**

Create `crates/scenarios/tests/encounter_reveal.rs`:

```rust
//! End-to-end test of the on-draw resolution path.
//!
//! Installs the synthetic `TEST_REGISTRY` (NOT the real
//! `cards::REGISTRY`) so this binary doesn't pull in the full
//! corpus. The test exercises:
//!
//! - Happy path: revealing the synthetic treachery emits
//!   `Event::CardRevealed`, resolves its Revelation effect
//!   (gain 1 resource), and discards the card.
//! - Empty-deck reject when both deck and discard are empty.
//!
//! Lives in `crates/scenarios/tests/` (not `game-core/src/engine/`)
//! because the `cards`-crate dependency direction prevents game-core
//! tests from constructing real card-shaped registries, and because
//! `card_registry::install` is process-global — an integration test
//! binary gets its own process, so this install doesn't collide
//! with `cards::REGISTRY` installs in other test binaries (e.g.
//! `crates/cards/tests/play_card.rs`).

use std::sync::Once;

use card_dsl::card_data::CardType;
use game_core::action::EngineRecord;
use game_core::engine::{apply, EngineOutcome};
use game_core::event::Event;
use game_core::state::{CardCode, InvestigatorId};
use game_core::{assert_event, Action};
use scenarios::test_fixtures::synth_cards::{SYNTH_TREACHERY_CODE, TEST_REGISTRY};
use scenarios::test_fixtures::synthetic;

static INSTALL: Once = Once::new();

fn install_test_registry() {
    INSTALL.call_once(|| {
        let _ = game_core::card_registry::install(TEST_REGISTRY);
    });
}

#[test]
fn revealing_synth_treachery_runs_revelation_and_discards() {
    install_test_registry();
    let state = synthetic::setup();
    let pre_resources = state.investigators[&InvestigatorId(1)].resources;
    let pre_deck_len = state.encounter_deck.len();
    assert!(pre_deck_len >= 1, "fixture must seed at least one card");

    let result = apply(
        state,
        Action::Engine(EngineRecord::EncounterCardRevealed {
            investigator: InvestigatorId(1),
        }),
    );

    assert_eq!(result.outcome, EngineOutcome::Done);

    // CardRevealed fires for the synthetic treachery.
    assert_event!(
        result.events,
        Event::CardRevealed { investigator, code, card_type }
            if *investigator == InvestigatorId(1)
                && *code == CardCode(SYNTH_TREACHERY_CODE.into())
                && *card_type == CardType::Treachery
    );

    // Revelation effect ran: controller gained 1 resource.
    let post_resources = result.state.investigators[&InvestigatorId(1)].resources;
    assert_eq!(
        post_resources,
        pre_resources + 1,
        "Revelation should grant 1 resource",
    );

    // Card moved deck → discard.
    assert_eq!(
        result.state.encounter_deck.len(),
        pre_deck_len - 1,
        "deck length should decrement by 1",
    );
    assert!(
        result.state.encounter_discard.contains(&CardCode(SYNTH_TREACHERY_CODE.into())),
        "synth treachery should be in discard after Revelation resolves",
    );
}

#[test]
fn rejects_when_encounter_deck_and_discard_both_empty() {
    install_test_registry();
    let mut state = synthetic::setup();
    // Drain the deck (and ensure discard stays empty).
    state.encounter_deck.clear();
    assert!(state.encounter_discard.is_empty());

    let result = apply(
        state,
        Action::Engine(EngineRecord::EncounterCardRevealed {
            investigator: InvestigatorId(1),
        }),
    );

    match result.outcome {
        EngineOutcome::Rejected { reason } => {
            assert!(
                reason.contains("encounter deck and discard both empty"),
                "unexpected reject reason: {reason:?}",
            );
        }
        other => panic!("expected Rejected, got {other:?}"),
    }
    assert!(
        result.events.is_empty(),
        "no events should fire on empty-deck reject; got {:?}",
        result.events,
    );
}
```

**On the enemy-stub case from the spec test plan:** the spec lists "Integration: enemy stub" as test 4. Implementing it requires adding a second synthetic card (a `_synth_enemy` with `card_type: CardType::Enemy`) to `synth_cards.rs`, plus a way to seed it onto the deck.

For #126's PR scope, prefer to **defer the enemy-stub integration test to #127**, which is going to flip the stub into the real spawn handler and naturally needs synthetic enemy cards. Document the deferral in the PR description ("enemy-stub integration test deferred to #127 where it'll naturally pair with the real spawn handler"). If reviewers push back, add the test here — but the enemy-arm reject is structurally simple enough to lock in via the in-crate unit test at Task 6 if needed.

**On the "no registry installed" case:** also deferred — `card_registry::install`'s `OnceLock` makes it impossible to test the no-install path from a binary where any other test installs. The unit test at Task 6 covers it within `game-core`.

- [ ] **Step 2: Run the integration test**

```bash
cargo test -p scenarios --features test_fixtures --test encounter_reveal
```

Expected: 2 tests pass.

- [ ] **Step 3: Full scenarios gauntlet**

```bash
RUSTFLAGS="-D warnings" cargo test -p scenarios --all-features
```

Expected: all `scenarios` tests pass (including the existing `synthetic_resolution.rs`, this new `encounter_reveal.rs`, and all unit tests).

- [ ] **Step 4: Commit**

```bash
git add crates/scenarios/tests/encounter_reveal.rs
git commit -m "$(cat <<'EOF'
test: end-to-end encounter-reveal integration test

Drives EngineRecord::EncounterCardRevealed against the synthetic
fixture and asserts:
- Event::CardRevealed fires with the synth treachery's code and
  card_type
- The Revelation effect resolves (controller's resources go up
  by 1)
- The card moves from encounter_deck to encounter_discard

Plus an empty-deck reject path.

Enemy-arm and no-registry-install integration tests deferred —
the enemy stub will get its real test in #127 alongside the spawn
handler, and the no-install path is covered by a game-core unit
test (the OnceLock registry can't be uninstalled within a binary).

Refs #126.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 10: Full local CI gauntlet

Before the phase-doc update, run the complete CI-equivalent locally to catch anything piecemeal task-by-task gauntlets missed. CLAUDE.md mandates this; the `doc` and `wasm-build` jobs in particular catch issues `cargo test` alone misses.

**Files:** none modified.

- [ ] **Step 1: Run all five CI jobs locally**

```bash
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
cargo build -p web --target wasm32-unknown-unknown
```

All five must exit 0. Each is independent — run them in parallel terminals or sequentially.

- [ ] **Step 2: If anything fails, fix and re-test before proceeding**

Common failure modes for this PR:
- **`cargo doc -D warnings`** — broken intra-doc links to cross-crate items (e.g. `Trigger::Revelation` referenced from `game-core` doc-comments where the link can't resolve across the crate boundary). Fix by switching to bare backticks: `` `Trigger::Revelation` `` instead of `[`Trigger::Revelation`]`.
- **`clippy -D warnings`** — `format!` in a hot path, `&str`-vs-`String` redundancies. Fix at the flagged site.
- **`wasm-build`** — `game-core` is `no_std`-friendly via wasm32; if a new dep accidentally pulls in `std`-only crates, wasm-build fails. Unlikely here (we're only adding variants + handlers), but check the build output.

DO NOT skip this step. CI will catch what local `cargo test` misses, and re-iterating after a CI failure is more expensive than fixing locally.

---

## Task 11: Phase-doc update (LAST commit before pushing)

**Files:**
- Modify: `docs/phases/phase-4-scenario-plumbing.md`

Per CLAUDE.md's PR procedure step 7: the phase-doc gets touched **exactly once per PR, as the final commit before push**, so it reflects the actually-shipping state.

- [ ] **Step 1: Update the Status line**

Read the current Status section in `docs/phases/phase-4-scenario-plumbing.md`:

```
🟡 In progress. Design pass complete 2026-05-21. First three PRs merged: `#103` unified window stack as PR #129, `#74` ScenarioModule skeleton as PR #130, and `#72` encounter deck state as PR #132. Remaining: `#126`, `#127`, `#69`, `#70`, `#71`, `#128`, `#73`.
```

Update to reflect #126 closed (PR number known when `gh pr create` runs — placeholder for now; final edit happens after PR creation):

```
🟡 In progress. Design pass complete 2026-05-21. First four PRs merged: `#103` unified window stack as PR #129, `#74` ScenarioModule skeleton as PR #130, `#72` encounter deck state as PR #132, and `#126` Revelation DSL + on-draw resolution as PR #<NN>. Remaining: `#127`, `#69`, `#70`, `#71`, `#128`, `#73`.
```

- [ ] **Step 2: Move `#126` from Open → Closed**

In the Open issues table, find the row:

```
| `#126` | DSL `Trigger::Revelation` + `EventPattern::CardRevealed` + on-draw resolution path | Split out of `#69`. First consumer is a synthetic treachery in the test fixture (e.g. "lose 1 resource"). |
```

Delete it from the Open table. Add to the Closed table after the `#72` row:

```
| `#126` | DSL `Trigger::Revelation` + `EventPattern::CardRevealed` + on-draw resolution | #<NN> | `Trigger::Revelation` and `EventPattern::CardRevealed { card_type: Option<CardType> }` land in `card-dsl`. Engine: `Event::CardRevealed` + `EngineRecord::EncounterCardRevealed` + `encounter_card_revealed` dispatch handler. Documented exception to validate-first / mutate-second contract (early `Event::CardRevealed` emission is the rules-correct interposition point for Before-timing reactions). Enemy arm stubbed for #127 to flip. First consumer: synthetic treachery in `crates/scenarios/src/test_fixtures/synth_cards.rs` with effect "gain 1 resource" (chose existing `Effect::GainResources` over a new lose-resources primitive — corpus has no in-scope two-consumer case). |
```

- [ ] **Step 3: Flip the Ordering row**

In the Ordering / Arc table, change row 4 from:

```
| 4 | `#126` DSL `Trigger::Revelation` + on-draw path | Lands the DSL primitive in isolation. First consumer is a synthetic treachery in the fixture. |
```

to:

```
| 4 | `#126` DSL `Trigger::Revelation` + on-draw path | ✅ PR #<NN>. Lands the DSL primitive in isolation. First consumer is a synthetic treachery in the fixture. |
```

- [ ] **Step 4: Add Decisions entries**

In the "Decisions made (design pass 2026-05-21)" section, add new entries near the bottom. Per CLAUDE.md, only include decisions that are **load-bearing for future PRs**.

Two entries earn their keep — both will shape future PRs' decisions:

```markdown
- **`EventPattern::CardRevealed { card_type: Option<CardType> }` (`#126`, PR #<NN>).** Chose `card_type` narrowing over the `EnemyDefeated`-mirror `by_controller: bool`. Encounter draws are engine-driven, not card-controlled, so `by_controller` doesn't fit the semantics; treachery-vs-enemy narrowing is the load-bearing distinction for hypothetical Forewarned-style listeners. The first real listener (Phase-7+) gets to confirm or extend.
- **`Event::CardRevealed` emits BEFORE Revelation resolves (`#126`, PR #<NN>).** Intentional ordering: Before-timing reaction listeners (#52's machinery; not yet wired) need the event to fire first so they can interpose / cancel. Documented exception to validate-first / mutate-second — the only state-changing pre-emit op is the encounter-deck draw, which is the load-bearing 'reveal' moment per the Rules Reference. Precedent: `play_card`'s mid-resolution caveat in CLAUDE.md. `#127`'s spawn-handler PR retains the same shape (the enemy arm replaces the stub reject, but the reveal-before-spawn ordering stays).
```

**Do NOT add an entry for:** the choice of `GainResources` over a new `LoseResources` primitive (it's an internal trade-off; future PRs don't need to know — when `LoseResources` lands it will be driven by a real consumer); the underscore-prefix code convention (visible from the file); the `TEST_REGISTRY` placement (a future test author will discover it by grep).

- [ ] **Step 5: Drop settled Open questions**

In the "Open questions" section, the entry on **Window-stack invariants** stays — #126 doesn't open any new windows. No entries are settled by this PR. (Tasks 4 + 6 don't push or pop on `state.open_windows`.)

- [ ] **Step 6: Commit (placeholder PR number)**

```bash
git add docs/phases/phase-4-scenario-plumbing.md
git commit -m "$(cat <<'EOF'
docs: phase-4 doc update for #126

- Status line: bumps merged count to 4.
- Move #126 from Open to Closed; flip Arc row 4 to checked.
- Add two Decisions entries: card_type-vs-by_controller narrowing
  choice, and the intentional Event::CardRevealed-before-Revelation
  ordering (documented validate-first exception).
- No Open questions settled by this PR.

PR number placeholder #<NN> — edited in a follow-up commit after
gh pr create returns the real number.

Refs #126.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

(Use a literal `#<NN>` placeholder in the doc — Task 12 patches the PR number in once `gh pr create` returns it.)

---

## Task 12: Push and open the PR

**Files:** none modified by the push itself. The follow-up doc edit modifies `docs/phases/phase-4-scenario-plumbing.md`.

- [ ] **Step 1: Push the branch**

```bash
git push -u origin engine/revelation-dsl
```

- [ ] **Step 2: Open the PR**

```bash
gh pr create --title "engine: Revelation DSL + on-draw resolution (#126)" --body "$(cat <<'EOF'
## Summary

Phase-4 issue #126. Three layers shipped together:

- **DSL** (`card-dsl`): `Trigger::Revelation` + `pub fn revelation()` builder; `EventPattern::CardRevealed { card_type: Option<CardType> }`.
- **Engine** (`game-core`): `Event::CardRevealed { investigator, code, card_type }`, `EngineRecord::EncounterCardRevealed { investigator }`, and the `encounter_card_revealed` dispatch handler that drives the on-draw resolution path.
- **Synthetic fixture** (`scenarios`): `crates/scenarios/src/test_fixtures/synth_cards.rs` with one synthetic treachery (code `"_synth_treachery"`) whose Revelation gains the controller 1 resource. `TEST_REGISTRY` exposes the fixture as a `CardRegistry` value for integration tests. The synthetic-scenario `setup()` now seeds the encounter deck with one copy.

Integration test: `crates/scenarios/tests/encounter_reveal.rs` — installs `TEST_REGISTRY`, draws the synth treachery via `EngineRecord::EncounterCardRevealed`, asserts the reveal event fires, the Revelation effect resolves, and the card lands in `encounter_discard`. Plus an empty-deck reject path.

## Design decisions

- **`EventPattern::CardRevealed` carries `card_type: Option<CardType>`** rather than `EnemyDefeated`'s `by_controller: bool`. Encounter draws are engine-driven, so `by_controller` doesn't fit the semantics; treachery-vs-enemy narrowing is the load-bearing knob for hypothetical Forewarned-style listeners.
- **`Event::CardRevealed` emits BEFORE the Revelation effect resolves.** This is the rules-correct interposition point for Before-timing reaction listeners (#52, not wired). Documented exception to the project's validate-first / mutate-second contract — the only state-changing pre-emit op is the encounter-deck draw, which is itself the reveal moment per the Rules Reference. Precedent: `play_card`'s mid-resolution caveat in CLAUDE.md. The handler's doc-comment expands on this.
- **Synthetic treachery effect is `Effect::GainResources { target: Controller, amount: 1 }`** — uses the existing DSL primitive rather than introducing a new `LoseResources` for one synthetic consumer. The corpus has no in-scope two-consumer case to justify a new primitive yet; "the Revelation ran and mutated state" is the proof we need, and sign doesn't change that. A future PR landing the first real lose-resources treachery picks up the new primitive with two consumers in hand.
- **Enemy-arm stub returns `Rejected { reason: "lands in #127" }`** — regression-test scaffolding for #127's spawn-handler PR to flip into the real branch.

## Out of scope (deferred)

- Mythos draw loop / Surge — #69.
- Enemy spawn rules — #127.
- "Attach" mechanics for treacheries that persist past Revelation — no in-scope card.
- Enemy-stub integration test — deferred to #127 where the spawn handler replaces the stub; the in-crate unit test surface is narrow enough.

## Test plan

- [x] `cargo test -p card-dsl` — DSL builder + serde round-trips for `Trigger::Revelation` and `EventPattern::CardRevealed`.
- [x] `cargo test -p game-core` — `Event::CardRevealed` + `EngineRecord::EncounterCardRevealed` serde; `encounter_card_revealed` handler unit tests (no-registry reject).
- [x] `cargo test -p scenarios --features test_fixtures` — synthetic fixture seeds encounter deck; `synth_cards` module asserts its own metadata + abilities surface.
- [x] `cargo test -p scenarios --test encounter_reveal` — end-to-end reveal flow + empty-deck reject path.
- [x] Full CI gauntlet: `cargo test --all --all-features` / `clippy -D warnings` / `cargo fmt --check` / `cargo doc -D warnings` / `cargo build -p web --target wasm32-unknown-unknown`.

Closes #126.
EOF
)"
```

- [ ] **Step 3: Capture the PR number**

`gh pr create` prints the PR URL — extract the trailing number. Example output: `https://github.com/talelburg/eldritch/pull/137` → PR #137.

- [ ] **Step 4: Patch the placeholder `#<NN>` references in the phase doc**

Open `docs/phases/phase-4-scenario-plumbing.md` and replace every `#<NN>` with the real PR number (3 occurrences from Task 11: the Status line, the Closed-table row, the Ordering row, the two Decisions entries — count the substitutions, don't miss one):

```bash
sed -i "s/#<NN>/#137/g" docs/phases/phase-4-scenario-plumbing.md  # replace 137 with actual
```

Verify:
```bash
grep -n "#<NN>" docs/phases/phase-4-scenario-plumbing.md
```

Expected: no matches.

- [ ] **Step 5: Commit and push the doc fix**

```bash
git add docs/phases/phase-4-scenario-plumbing.md
git commit -m "$(cat <<'EOF'
docs: patch phase-4 doc with real PR number

Replaces the #<NN> placeholders from the prior commit with the
actual PR number returned by gh pr create.

Refs #126.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
git push
```

- [ ] **Step 6: Hand off to CI watch + review**

Per CLAUDE.md PR procedure step 4: code review for routine PRs happens **before push** via the pre-push subagent flow. If this plan was executed with `superpowers:subagent-driven-development`, the post-push `review-agent` dispatch is redundant and should be skipped.

CI watch is the next human-driven step:

```bash
gh pr checks <PR#> --watch
```

This task is complete once the PR is open with all green local checks. Merge happens only with explicit user approval per CLAUDE.md PR procedure step 8.

---

## Self-review checklist (run before handing off)

Per `superpowers:writing-plans`'s "Self-Review" step. After saving this plan, sanity-check it against the spec:

- **Spec coverage:** Does every Acceptance criterion in the issue and every section in the spec map to a task?
  - `Trigger::Revelation` in DSL → Task 2.
  - `EventPattern::CardRevealed { card_type }` → Task 3.
  - On-draw resolution path → Tasks 4 (Event), 5 (EngineRecord), 6 (handler).
  - Synthetic treachery + first-consumer wiring → Tasks 7 + 8.
  - Tests proving the wiring → Tasks 7 (unit), 9 (integration).
- **Placeholder scan:** No "TBD," "TODO," "add appropriate handling," or "similar to Task N (code omitted)" — every code step has the actual code.
- **Type consistency:** Names referenced across tasks (`SYNTH_TREACHERY_CODE`, `TEST_REGISTRY`, `encounter_card_revealed`, `EncounterCardRevealed`, `Event::CardRevealed`, `Trigger::Revelation`, `EventPattern::CardRevealed`) match exactly between their definition task and their reference tasks.
- **Validate-first caveat documented:** Task 6's handler doc-comment + PR description (Task 12) both call out the early `Event::CardRevealed` emit explicitly. This is the most likely review pushback point — front-loading the explanation saves a round trip.
