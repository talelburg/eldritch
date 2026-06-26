# Trigger-level Eligibility Predicate (#368) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a per-ability native eligibility predicate, evaluated at reaction-scan time, that suppresses a reaction when its effect can't change game state — replacing Cover Up 01007's hardcoded `card.clues == 0` stand-in and adding The Barrier 01109's missing Hallway-affordability gate (closing #470).

**Architecture:** A `Ability.eligibility: Option<String>` tag → a registry `native_eligibility_for(tag) -> Option<EligibilityFn>` (where `EligibilityFn = fn(&GameState, &EvalContext) -> bool`) → a shared `ability_eligible(...)` helper consulted at both reaction-scan sites. Cards supply tiny Rust predicates; no declarative DSL vocabulary is added.

**Tech Stack:** Rust workspace (`card-dsl`, `game-core`, `cards`), serde.

## Global Constraints

- **YAGNI:** no declarative `Condition` eligibility vocabulary; no fast-window wiring (no in-scope fast consumer); #368 item 2 is **out** (tracked in #471).
- **Ordering keeps every task green:** the scan helper is wired *before* any hardcode is removed; Cover Up's tag+predicate land in the *same* task that deletes its `card.clues == 0` hardcode (no regression window).
- **DRY:** The Barrier's affordability check is one helper (`round_end_advance_affordable`) shared by the offer predicate and the resolve handler, so they can't drift.
- **CI gauntlet before push** (all seven jobs, warnings-as-errors):
  - `RUSTFLAGS="-D warnings" cargo test --all --all-features`
  - `cargo clippy --all-targets --all-features -- -D warnings`
  - `cargo fmt --check`
  - `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`
  - `cargo build -p web --target wasm32-unknown-unknown`
  - `wasm-pack test --headless --firefox crates/web`
  - `cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings`
- **Branch:** `engine/trigger-eligibility` (already created; spec committed). One branch, follow-up commits, no force-push.
- Spec of record: `docs/superpowers/specs/2026-06-26-trigger-eligibility-design.md`.

---

### Task 1: Eligibility plumbing — DSL field + registry hook (inert)

The whole mechanism scaffold: the `Ability` field + builder (`card-dsl`), and the registry function-pointer + `cards` wiring (`game-core`, `cards`). Inert until consumers declare a tag.

**Files:**
- Modify: `crates/card-dsl/src/dsl.rs` (struct field, builder, all `Ability { … }` literals)
- Modify: `crates/game-core/src/card_registry.rs` (`EligibilityFn` type, struct field, fake registry)
- Modify: `crates/cards/src/lib.rs` (REGISTRY const + adapter)
- Modify: `crates/cards/src/impls/mod.rs` (empty `native_eligibility_for` dispatch)

**Interfaces:**
- Produces: `Ability { …, eligibility: Option<String> }`; `Ability::with_eligibility(self, tag) -> Self`; `game_core::card_registry::EligibilityFn = fn(&GameState, &EvalContext) -> bool`; `CardRegistry.native_eligibility_for: fn(&str) -> Option<EligibilityFn>`; `cards::impls::native_eligibility_for(tag) -> Option<EligibilityFn>` (returns `None` for now).

- [ ] **Step 1: Write the failing DSL test**

In `crates/card-dsl/src/dsl.rs`, in its `#[cfg(test)] mod tests` (or add one if absent — search for `mod tests` in the file and append):

```rust
#[test]
fn with_eligibility_sets_the_tag_and_default_is_none() {
    use super::{reaction_on_event, EventPattern, EventTiming};
    let bare = reaction_on_event(EventPattern::RoundEnded, EventTiming::When, super::Effect::Cancel);
    assert_eq!(bare.eligibility, None);
    let gated = reaction_on_event(EventPattern::RoundEnded, EventTiming::When, super::Effect::Cancel)
        .with_eligibility("01109:can_advance");
    assert_eq!(gated.eligibility.as_deref(), Some("01109:can_advance"));
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p card-dsl with_eligibility_sets_the_tag`
Expected: FAIL to compile — no field `eligibility`, no method `with_eligibility`.

- [ ] **Step 3: Add the field + builder**

In `crates/card-dsl/src/dsl.rs`, add the field to `struct Ability` (after `usage_limit`):
```rust
    pub usage_limit: Option<UsageLimit>,
    /// Native eligibility-predicate tag (`native_eligibility_for`). When set,
    /// the reaction/fast scan suppresses this ability unless the predicate
    /// holds (RR p.2: an ability can't initiate if its effect won't change
    /// game state). `None` for the common no-gate case; stays implicitly
    /// optional on the wire (#453 per-field carve-out, like `usage_limit`).
    pub eligibility: Option<String>,
```
Add the builder in `impl Ability` (next to `with_usage_limit`):
```rust
    /// Attach a native eligibility-predicate tag (see [`Self::eligibility`]).
    #[must_use]
    pub fn with_eligibility(mut self, tag: impl Into<String>) -> Self {
        self.eligibility = Some(tag.into());
        self
    }
```
Then add `eligibility: None,` to **every** `Ability { … }` literal in the file — the constructor functions `on_event` (~1264), `constant` (~1206), `on_play` (~1220), `on_commit` (~1232), `on_skill_test_resolution` (~1246), the `~1264` activated-ability path, `revelation` (~1297), `activated` (~1316), `elder_sign` (~1336). The compiler lists each missing-field site; fix until `cargo build -p card-dsl` is clean.

- [ ] **Step 4: Run the DSL test + serde round-trip**

Run: `cargo test -p card-dsl with_eligibility_sets_the_tag`
Expected: PASS.
Run: `cargo build -p card-dsl` — clean (all literals updated).

- [ ] **Step 5: Add the registry hook (game-core)**

In `crates/game-core/src/card_registry.rs`: extend the imports to include `GameState`:
```rust
use crate::state::{CardCode, GameState};
```
Add the type alias near `NativeEffectFn`:
```rust
/// A card-local read-only eligibility predicate: returns whether a reaction
/// ability whose [`Ability::eligibility`] names this tag may be offered.
/// Receives the same [`EvalContext`] native effects do (controller + source).
pub type EligibilityFn = fn(&GameState, &EvalContext) -> bool;
```
Add the field to `struct CardRegistry` (after `native_effect_for`):
```rust
    /// Look up a card-local eligibility predicate by its
    /// [`Ability::eligibility`] tag. `None` for unregistered tags.
    pub native_eligibility_for: fn(&str) -> Option<EligibilityFn>,
```
Update `fake_registry()` in the test module (and any other `CardRegistry { … }` literal — the compiler flags them) to add:
```rust
            native_eligibility_for: |_| None,
```

- [ ] **Step 6: Wire the `cards` registry**

In `crates/cards/src/impls/mod.rs`, add (near `native_effect_for`):
```rust
/// Dispatch a native eligibility-predicate tag to its card-local handler.
/// (Per-card branches are added by their consumer tasks.)
#[must_use]
pub fn native_eligibility_for(_tag: &str) -> Option<game_core::card_registry::EligibilityFn> {
    None
}
```
In `crates/cards/src/lib.rs`, add the adapter (near `registry_native_effect_for`):
```rust
/// Adapter from an eligibility tag to its card-local predicate.
fn registry_native_eligibility_for(tag: &str) -> Option<game_core::card_registry::EligibilityFn> {
    impls::native_eligibility_for(tag)
}
```
and add the field to the `REGISTRY` const:
```rust
pub const REGISTRY: CardRegistry = CardRegistry {
    metadata_for: registry_metadata_for,
    abilities_for: registry_abilities_for,
    native_effect_for: registry_native_effect_for,
    native_eligibility_for: registry_native_eligibility_for,
};
```

- [ ] **Step 7: Build the workspace + commit**

Run: `RUSTFLAGS="-D warnings" cargo build --all --all-features`
Expected: clean (all `Ability {}` / `CardRegistry {}` literals updated workspace-wide).
Run: `cargo test -p card-dsl && cargo test -p game-core --lib card_registry`
Expected: PASS.

```bash
git add crates/card-dsl/src/dsl.rs crates/game-core/src/card_registry.rs crates/cards/src/lib.rs crates/cards/src/impls/mod.rs
git commit -m "engine: Ability.eligibility tag + native_eligibility_for registry hook (inert)

Adds the per-ability eligibility-predicate plumbing: an Ability.eligibility
tag, a with_eligibility builder, and a CardRegistry.native_eligibility_for
function pointer (EligibilityFn = fn(&GameState, &EvalContext) -> bool). No
consumer yet; wired into the scan and used by Cover Up / The Barrier next.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 2: `ability_eligible` scan helper, wired into both reaction scans (inert)

Adds the evaluator helper and consults it at both reaction-scan sites. Still inert (no ability declares a tag; Cover Up's `card.clues == 0` hardcode stays in place this task, so behaviour is unchanged).

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs` (`CandidateSource::instance()` helper)
- Modify: `crates/game-core/src/engine/dispatch/reaction_windows.rs` (`ability_eligible`; call at both scans)

**Interfaces:**
- Consumes: `CardRegistry.native_eligibility_for`, `EvalContext::for_controller_with_optional_source`.
- Produces: `CandidateSource::instance(self) -> Option<CardInstanceId>`; `ability_eligible(state, ability, source, controller) -> bool` (module-private).

- [ ] **Step 1: Write the failing helper test**

In `crates/game-core/src/state/game_state.rs` test module (search `mod tests` near `CandidateSource`, or add one), add:
```rust
#[test]
fn candidate_source_instance_projects_inplay_only() {
    use super::{CandidateSource, CardInstanceId};
    assert_eq!(CandidateSource::InPlay(CardInstanceId(4)).instance(), Some(CardInstanceId(4)));
    assert_eq!(CandidateSource::Board.instance(), None);
    assert_eq!(CandidateSource::Hand.instance(), None);
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p game-core --lib candidate_source_instance_projects_inplay_only`
Expected: FAIL to compile — no method `instance`.

- [ ] **Step 3: Add `CandidateSource::instance()`**

In `crates/game-core/src/state/game_state.rs`, add an `impl CandidateSource` (after the enum):
```rust
impl CandidateSource {
    /// The in-play instance backing this candidate, if any. `Board` /
    /// `Hand` candidates have no instance.
    #[must_use]
    pub fn instance(self) -> Option<CardInstanceId> {
        match self {
            CandidateSource::InPlay(id) => Some(id),
            CandidateSource::Board | CandidateSource::Hand => None,
        }
    }
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p game-core --lib candidate_source_instance_projects_inplay_only`
Expected: PASS.

- [ ] **Step 5: Add `ability_eligible` and wire both scans**

In `crates/game-core/src/engine/dispatch/reaction_windows.rs`, add the helper (place it near `scan_pending_triggers`):
```rust
/// Whether a reaction `ability` may be offered, per its
/// [`Ability::eligibility`] tag (RR p.2: an ability can't initiate if its
/// effect won't change game state). No tag → eligible. A tag with no
/// resolvable predicate (registry absent / unknown tag) → suppressed, never
/// offered on a half-installed host.
fn ability_eligible(
    state: &GameState,
    ability: &Ability,
    source: CandidateSource,
    controller: InvestigatorId,
) -> bool {
    let Some(tag) = ability.eligibility.as_deref() else {
        return true;
    };
    let Some(reg) = card_registry::current() else {
        return false;
    };
    let Some(pred) = (reg.native_eligibility_for)(tag) else {
        return false;
    };
    let ctx = crate::engine::evaluator::EvalContext::for_controller_with_optional_source(
        controller,
        source.instance(),
    );
    pred(state, &ctx)
}
```
In `scan_pending_triggers`, after the existing trigger match guard (`if !trigger_matches(event, pattern, *timing, id) { continue }`), add:
```rust
                if !ability_eligible(state, ability, CandidateSource::InPlay(card.instance_id), id) {
                    continue;
                }
```
In `scan_act_agenda_reactions`, after its `if !trigger_matches(event, pattern, *timing, lead) { … continue }` guard and before pushing the `ResolutionCandidate`, add:
```rust
            if !ability_eligible(state, ability, CandidateSource::Board, lead) {
                continue;
            }
```
(`EvalContext` may need importing at the top of the file if not already in scope; the compiler will say so.)

- [ ] **Step 6: Build + run the reaction-window tests (behaviour unchanged)**

Run: `RUSTFLAGS="-D warnings" cargo test -p game-core --lib dispatch::reaction_windows`
Expected: PASS — nothing declares an `eligibility` tag yet, so `ability_eligible` returns `true` everywhere; Cover Up's `card.clues == 0` hardcode is still present.
Run: `cargo clippy -p game-core --all-targets --all-features -- -D warnings` — clean.

- [ ] **Step 7: Commit**

```bash
git add crates/game-core/src/state/game_state.rs crates/game-core/src/engine/dispatch/reaction_windows.rs
git commit -m "engine: consult ability_eligible in both reaction scans (inert)

Adds CandidateSource::instance() and an ability_eligible helper that evaluates
an ability's eligibility tag via the registry, and calls it in
scan_pending_triggers and scan_act_agenda_reactions. No ability declares a tag
yet, so behaviour is unchanged; consumers migrate next.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 3: Cover Up 01007 — declare eligibility, drop the hardcode

**Files:**
- Modify: `crates/cards/src/impls/cover_up.rs` (tag + predicate + dispatch)
- Modify: `crates/cards/src/impls/mod.rs` (chain Cover Up into `native_eligibility_for`)
- Modify: `crates/game-core/src/engine/dispatch/reaction_windows.rs` (delete `card.clues == 0` hardcode)

**Interfaces:**
- Consumes: `EligibilityFn`, `ability_eligible` (already wired), `Ability::with_eligibility`.
- Produces: `cover_up::native_eligibility_for(tag)`; tag `"01007:has_clues"`.

- [ ] **Step 1: Write the failing card test**

In `crates/cards/src/impls/cover_up.rs` test module, add:
```rust
#[test]
fn has_clues_predicate_gates_on_source_instance_clues() {
    use card_dsl::dsl::EventPattern;
    use game_core::engine::EvalContext;
    use game_core::state::{CardInPlay, CardInstanceId, GameStateBuilder, InvestigatorId};

    // The reaction ability now carries the eligibility tag.
    let abilities = super::abilities();
    let reaction = abilities
        .iter()
        .find(|a| matches!(&a.trigger, card_dsl::dsl::Trigger::OnEvent { pattern: EventPattern::WouldDiscoverClues, .. }))
        .expect("Cover Up has a WouldDiscoverClues reaction");
    assert_eq!(reaction.eligibility.as_deref(), Some("01007:has_clues"));

    // Predicate: true when the source instance holds clues, false at 0.
    let pred = super::native_eligibility_for("01007:has_clues").expect("registered");
    let mut inv = game_core::test_support::test_investigator(1);
    let mut card = CardInPlay::enter_play(game_core::state::CardCode::new("01007"), CardInstanceId(0));
    card.clues = 3;
    inv.threat_area.push(card);
    let state = GameStateBuilder::new().with_investigator(inv).build();
    let ctx3 = EvalContext::for_controller_with_source(InvestigatorId(1), CardInstanceId(0));
    assert!(pred(&state, &ctx3));

    // Drain to 0 → ineligible.
    let mut state0 = state.clone();
    state0.investigators.get_mut(&InvestigatorId(1)).unwrap().threat_area[0].clues = 0;
    assert!(!pred(&state0, &ctx3));
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p cards has_clues_predicate_gates_on_source_instance_clues`
Expected: FAIL — `native_eligibility_for` missing on `cover_up`, and the ability has no eligibility tag.

- [ ] **Step 3: Add the tag + predicate + dispatch (cover_up.rs)**

In `crates/cards/src/impls/cover_up.rs`, add the tag constant near the others:
```rust
/// Eligibility tag: Cover Up may replace a discovery only while it still
/// holds clues to discard (RR p.2 potential gate).
const HAS_CLUES_TAG: &str = "01007:has_clues";
```
Chain `.with_eligibility(HAS_CLUES_TAG)` onto the `reaction_on_event(WouldDiscoverClues, …)` ability in `abilities()`:
```rust
        reaction_on_event(
            EventPattern::WouldDiscoverClues,
            EventTiming::When,
            Effect::Seq(vec![native(DISCARD_TAG), Effect::Cancel]),
        )
        .with_eligibility(HAS_CLUES_TAG),
```
Add the predicate + dispatch (import `EligibilityFn`, `EvalContext`, `GameState`):
```rust
use game_core::card_registry::EligibilityFn;
use game_core::{state::GameState, EvalContext};

/// True while the Cover Up instance still holds clues to discard.
fn has_clues(state: &GameState, ctx: &EvalContext) -> bool {
    let Some(source) = ctx.source else { return false };
    state.investigators.get(&ctx.controller).is_some_and(|inv| {
        inv.threat_area
            .iter()
            .chain(inv.cards_in_play.iter())
            .any(|c| c.instance_id == source && c.clues > 0)
    })
}

/// Resolve Cover Up's eligibility tag.
pub(crate) fn native_eligibility_for(tag: &str) -> Option<EligibilityFn> {
    match tag {
        HAS_CLUES_TAG => Some(has_clues as EligibilityFn),
        _ => None,
    }
}
```

- [ ] **Step 4: Chain Cover Up into the dispatch (impls/mod.rs)**

In `crates/cards/src/impls/mod.rs`, replace the empty `native_eligibility_for` body with:
```rust
#[must_use]
pub fn native_eligibility_for(tag: &str) -> Option<game_core::card_registry::EligibilityFn> {
    cover_up::native_eligibility_for(tag)
}
```

- [ ] **Step 5: Delete the hardcoded stand-in (reaction_windows.rs)**

In `crates/game-core/src/engine/dispatch/reaction_windows.rs`, **delete** the block:
```rust
            // Potential-gate stand-in for Cover Up (RR p.2 "an ability cannot
            // initiate if its effect won't change the game state"; TODO(#368)):
            // only a source still holding clues to discard can replace the
            // discovery — an emptied Cover Up would otherwise prompt forever.
            if matches!(event, TimingEvent::WouldDiscoverClues { .. }) && card.clues == 0 {
                continue;
            }
```
(Its job is now done by `ability_eligible` + Cover Up's tag.)

- [ ] **Step 6: Run card + engine tests**

Run: `cargo test -p cards has_clues_predicate_gates_on_source_instance_clues` — PASS.
Run: `cargo test -p cards --test '*' 2>/dev/null; cargo test -p cards` — Cover Up's existing tests stay green.
Run: `RUSTFLAGS="-D warnings" cargo test -p game-core --lib dispatch::reaction_windows` — green (the inline hardcode test, if any, was for the engine path; an integration test in `cards` now owns Cover Up's gating). If a game-core unit test asserted the old hardcode directly, port its intent to the `cover_up` predicate test above and delete the now-obsolete engine-level assertion.

- [ ] **Step 7: Commit**

```bash
git add crates/cards/src/impls/cover_up.rs crates/cards/src/impls/mod.rs crates/game-core/src/engine/dispatch/reaction_windows.rs
git commit -m "cards: Cover Up 01007 declares 01007:has_clues eligibility; drop engine hardcode

Cover Up's discover-replacement reaction now carries an eligibility tag whose
predicate gates on the source instance still holding clues, replacing the
hardcoded card.clues==0 branch in scan_pending_triggers.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 4: The Barrier 01109 — affordability gate (closes #470)

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/act_agenda.rs` (`round_end_advance_affordable`; reuse in `round_end_advance`)
- Modify: `crates/game-core/src/lib.rs` (re-export `round_end_advance_affordable`)
- Modify: `crates/cards/src/impls/the_barrier.rs` (tag + predicate + dispatch; doc fix)
- Modify: `crates/cards/src/impls/mod.rs` (chain The Barrier)

**Interfaces:**
- Consumes: `EligibilityFn`, `ability_eligible`, `Ability::with_eligibility`, the `investigators_at` / `clues_held` helpers.
- Produces: `game_core::round_end_advance_affordable(&GameState, &str) -> bool`; tag `"01109:can_advance"`.

- [ ] **Step 1: Write the failing affordability test**

In `crates/game-core/src/engine/dispatch/act_agenda.rs`, add to the test module that already imports `Act`/`CardCode` (the `advance_act_tests` mod near the bottom; add `test_location, LocationId` to its imports):
```rust
#[test]
fn round_end_advance_affordable_tracks_hallway_clues_vs_threshold() {
    use crate::state::{Act, CardCode, InvestigatorId, LocationId};
    use crate::test_support::{test_investigator, test_location, GameStateBuilder};

    // A location coded "HALL"; the investigator stands on it.
    let mut hall = test_location(1, "Hallway");
    hall.code = CardCode("HALL".into());
    let mut investigator = test_investigator(1);
    investigator.current_location = Some(LocationId(1));
    let mut state = GameStateBuilder::new()
        .with_location(hall)
        .with_investigator(investigator)
        .build();
    state.act_deck = vec![Act { code: CardCode("_act".into()), clue_threshold: 3, resolution: None }];
    state.act_index = 0;

    state.investigators.get_mut(&InvestigatorId(1)).unwrap().clues = 2;
    assert!(!super::round_end_advance_affordable(&state, "HALL"), "2 < 3 → not affordable");
    state.investigators.get_mut(&InvestigatorId(1)).unwrap().clues = 3;
    assert!(super::round_end_advance_affordable(&state, "HALL"), "3 >= 3 → affordable");
}
```
(`test_location(id, name)` builds a revealed `Location` with `code = "_test_loc_{id}"`; we overwrite `.code` to `"HALL"` so `location_id_by_code` resolves it.)

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p game-core --lib round_end_advance_affordable_tracks`
Expected: FAIL to compile — `round_end_advance_affordable` doesn't exist.

- [ ] **Step 3: Add `round_end_advance_affordable` and reuse it in the resolver**

In `crates/game-core/src/engine/dispatch/act_agenda.rs`, add (above `round_end_advance`):
```rust
/// Whether the current act's round-end group clue-spend advance is affordable:
/// investigators at `contributor_location_code` hold at least the current act's
/// `clue_threshold`. Shared by the offer-side eligibility predicate (01109's
/// `with_eligibility`) and the resolve-side [`round_end_advance`], so the two
/// can't drift. Returns `false` if there is no current act or the location is
/// not in play (the "no investigators in the Hallway" case is subsumed — 0
/// contributors ⇒ 0 clues ⇒ not affordable).
#[must_use]
pub fn round_end_advance_affordable(state: &GameState, contributor_location_code: &str) -> bool {
    let Some(act) = state.act_deck.get(state.act_index) else {
        return false;
    };
    let threshold = act.clue_threshold;
    let Some(loc) = crate::engine::location_id_by_code(state, contributor_location_code) else {
        return false;
    };
    clues_held(state, &investigators_at(state, loc)) >= u32::from(threshold)
}
```
Refactor `round_end_advance`'s affordability branch to delegate — replace the inline `let threshold = …; let contributors = …; if clues_held(...) < threshold { reject }` portion so the gate reads:
```rust
    if !round_end_advance_affordable(cx.state, contributor_location_code) {
        return EngineOutcome::Rejected {
            reason: "round_end_advance: contributors no longer hold enough clues".into(),
        };
    }
    // re-resolve loc + contributors for the spend (location is in play — affordable implies present)
    let threshold = cx.state.act_deck[cx.state.act_index].clue_threshold;
    let loc = crate::engine::location_id_by_code(cx.state, contributor_location_code)
        .expect("affordable ⇒ contributor location in play");
    let contributors = investigators_at(cx.state, loc);
    spend_clues_from(cx.state, &contributors, threshold);
    advance_act(cx);
    EngineOutcome::Done
```

In `crates/game-core/src/lib.rs`, add `round_end_advance_affordable` to the `pub use engine::{ … }` re-export list (next to `round_end_advance`).

- [ ] **Step 4: Run the affordability test**

Run: `cargo test -p game-core --lib round_end_advance_affordable_tracks` — PASS.
Run: `cargo test -p game-core --lib act_agenda` — existing `round_end_advance` tests stay green.

- [ ] **Step 5: Declare the tag + predicate (the_barrier.rs) + fix docs**

In `crates/cards/src/impls/the_barrier.rs`, add the tag constant:
```rust
/// Eligibility tag: the round-end advance may be offered only when the Hallway
/// group can afford the act's clue threshold (RR p.2 potential gate).
const CAN_ADVANCE_TAG: &str = "01109:can_advance";
```
Chain `.with_eligibility(CAN_ADVANCE_TAG)` onto the `reaction_on_event(RoundEnded, …)` ability in `abilities()`:
```rust
        reaction_on_event(
            EventPattern::RoundEnded,
            EventTiming::When,
            native(ROUND_END_ADVANCE),
        )
        .with_eligibility(CAN_ADVANCE_TAG),
```
Add the predicate + dispatch (import `EligibilityFn`, `EvalContext`, `GameState`, `round_end_advance_affordable`):
```rust
use game_core::card_registry::EligibilityFn;
use game_core::{round_end_advance_affordable, state::GameState, EvalContext};

/// True when the Hallway group can afford the act's clue threshold.
fn can_advance(state: &GameState, _ctx: &EvalContext) -> bool {
    round_end_advance_affordable(state, HALLWAY)
}

/// Resolve The Barrier's eligibility tag.
pub(crate) fn native_eligibility_for(tag: &str) -> Option<EligibilityFn> {
    match tag {
        CAN_ADVANCE_TAG => Some(can_advance as EligibilityFn),
        _ => None,
    }
}
```
Correct the stale module/`advance_via_clue_spend` doc comments: replace "Affordability is gated in the reaction scan" / "the candidate is offered only when affordable" with an accurate description — affordability is gated by the `01109:can_advance` eligibility predicate (shared with the resolve-side `round_end_advance_affordable`); the resolve-side check is now a defensive backstop.

- [ ] **Step 6: Chain The Barrier into the dispatch (impls/mod.rs)**

In `crates/cards/src/impls/mod.rs`, extend the dispatch:
```rust
#[must_use]
pub fn native_eligibility_for(tag: &str) -> Option<game_core::card_registry::EligibilityFn> {
    cover_up::native_eligibility_for(tag).or_else(|| the_barrier::native_eligibility_for(tag))
}
```

- [ ] **Step 7: Write the #470 regression integration test**

In `crates/cards/tests/`, add `the_barrier_eligibility.rs` (new file; integration tests can `install(cards::REGISTRY)` without colliding). This drives the **decision point #470 hinges on** — the Barrier's eligibility predicate, resolved through the *installed* registry (the exact lookup `scan_act_agenda_reactions` performs via `ability_eligible`):
```rust
//! #470: The Barrier's round-end advance is gated by an eligibility predicate
//! that must reject when the Hallway group can't afford the act's clue
//! threshold. Exercises the predicate through the installed cards::REGISTRY —
//! the same `native_eligibility_for("01109:can_advance")` lookup the reaction
//! scan performs.
use game_core::card_registry;
use game_core::state::{Act, CardCode, GameStateBuilder, InvestigatorId, LocationId};
use game_core::test_support::{test_investigator, test_location};
use game_core::EvalContext;

#[test]
fn barrier_advance_eligibility_gates_on_hallway_affordability() {
    let _ = card_registry::install(cards::REGISTRY);
    let pred = card_registry::current()
        .expect("registry installed")
        .native_eligibility_for("01109:can_advance")
        .expect("01109:can_advance is registered by The Barrier");

    // The Barrier's contributor location is the Hallway (01112).
    let mut hall = test_location(1, "Hallway");
    hall.code = CardCode("01112".into());
    let mut inv = test_investigator(1);
    inv.current_location = Some(LocationId(1));
    let mut state = GameStateBuilder::new()
        .with_location(hall)
        .with_investigator(inv)
        .build();
    state.act_deck = vec![Act { code: CardCode("01109".into()), clue_threshold: 3, resolution: None }];
    state.act_index = 0;

    let ctx = EvalContext::for_controller(InvestigatorId(1));
    state.investigators.get_mut(&InvestigatorId(1)).unwrap().clues = 0;
    assert!(!pred(&state, &ctx), "#470: not offered at 0/3 clues (nobody can afford)");
    state.investigators.get_mut(&InvestigatorId(1)).unwrap().clues = 3;
    assert!(pred(&state, &ctx), "offered at 3/3 clues");
}
```
This proves the tag is wired into `cards::REGISTRY` and the predicate gates correctly. The scan-side suppression itself is mechanical (`scan_act_agenda_reactions` → `ability_eligible` → this exact predicate, wired inert in Task 2), so this predicate-through-registry test is the #470 regression guard; a full round-end window drive is unnecessary belt-and-suspenders.

- [ ] **Step 8: Run card + integration tests**

Run: `cargo test -p cards --test the_barrier_eligibility` — PASS.
Run: `cargo test -p cards` — The Barrier's existing tests stay green.

- [ ] **Step 9: Commit**

```bash
git add crates/game-core/src/engine/dispatch/act_agenda.rs crates/game-core/src/lib.rs crates/cards/src/impls/the_barrier.rs crates/cards/src/impls/mod.rs crates/cards/tests/the_barrier_eligibility.rs
git commit -m "cards: The Barrier 01109 round-end advance eligibility gate (closes #470)

Adds a shared round_end_advance_affordable helper (used by both the offer-side
eligibility predicate and the resolve-side handler) and tags 01109's round-end
reaction with 01109:can_advance, so the advance is no longer offered when the
Hallway group can't afford the clue threshold. Corrects the stale 'gated in the
scan' doc comments.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 5: Gauntlet, push, PR, phase doc

- [ ] **Step 1: Full local gauntlet**

Run each (all green):
```bash
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
cargo build -p web --target wasm32-unknown-unknown
wasm-pack test --headless --firefox crates/web
cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings
```
Fix `cargo fmt` diffs by running `cargo fmt` and folding into the relevant commit.

- [ ] **Step 2: Push and open the PR**

```bash
git push -u origin engine/trigger-eligibility
gh pr create --fill
```
PR body: design-decisions paragraph (native eligibility predicate hook, not declarative Condition — both consumers single-consumer + heterogeneous; shared `round_end_advance_affordable` so offer/resolve can't drift; item 2 split to #471). Ensure the body has `Closes #368.` and `Closes #470.`

- [ ] **Step 3: Watch CI**

Run: `gh pr checks <PR#> --watch`. Fix failures with follow-up commits (no force-push).

- [ ] **Step 4: Phase-7 doc (after CI green, final commit)**

In `docs/phases/phase-7-the-gathering.md`, update **Remaining gate work** item 2 (the `#368` entry): mark #368 ✅ shipped (PR #N) and #470 ✅ shipped; note the eligibility predicate replaced both hardcoded stand-ins and that item 2 moved to #471. Add a **Decisions made** entry only if it passes the README test (e.g. "eligibility is a native predicate hook, not declarative Condition — promote to a Condition when a predicate recurs"). Commit:
```bash
git add docs/phases/phase-7-the-gathering.md
git commit -m "docs: phase-7 — #368 eligibility predicate + #470 shipped"
git push
```

- [ ] **Step 5: Merge only after explicit user approval**

Do not merge autonomously. On approval:
```bash
gh pr merge <PR#> --squash --delete-branch
```
Confirm #368 and #470 auto-closed; `git pull` on `main`.

## Self-Review

**Spec coverage:**
- `Ability.eligibility` + builder → Task 1. ✓
- `EligibilityFn` + `native_eligibility_for` registry + cards wiring → Task 1. ✓
- `CandidateSource::instance()` + `ability_eligible` + both scan sites → Task 2. ✓
- Cover Up consumer + hardcode removal → Task 3. ✓
- The Barrier consumer + shared `round_end_advance_affordable` + #470 fix + doc correction → Task 4. ✓
- Out-of-scope (item 2, fast-window, declarative vocab) → Global Constraints; no task adds them. ✓
- Tests: DSL/registry units, Cover Up gating, Barrier affordability + #470 integration → Tasks 1–4. ✓
- Closes #368 + #470; phase doc → Task 5. ✓

**Placeholder scan:** Clean — every code step carries complete, concrete code (Task 4's affordability unit test and the #470 integration test both use real fixtures: `test_location(id, name)` with an overwritten `.code`, `GameStateBuilder::with_location`/`with_investigator`, and the real tags/codes `"01109"`/`"01112"`). The `ability_eligible` no-tag→`true` path needs no dedicated test — every existing reaction-window test has no-tag abilities and stays green in Task 2 Step 6, which exercises exactly that path. No "TBD"/"handle errors"/"similar to Task N".

**Type consistency:** `eligibility: Option<String>`, `EligibilityFn = fn(&GameState, &EvalContext) -> bool`, `native_eligibility_for`, `ability_eligible(state, ability, source, controller)`, `CandidateSource::instance()`, `round_end_advance_affordable(&GameState, &str)`, tags `"01007:has_clues"` / `"01109:can_advance"` — all used consistently across tasks. The ordering (helper wired Task 2 before hardcode removed Task 3; Cover Up tag + hardcode-removal same task) preserves green between tasks. ✓
