# #69 — Mythos phase content — implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Land the Mythos-phase driver per Rules Reference p.24 sub-steps 1.1–1.5, with player-driven encounter draws (`PlayerAction::DrawEncounterCard`), surge re-draw chain, post-1.4 player window, and a skeleton Investigation phase driver that owns the rotation to lead (step 2.2). Ship the phase-driver-owns-its-boundary-emits pattern + `open_fast_window` helper that #70 / #71 / the future full Investigation driver will reuse.

**Architecture:** Each phase has a driver function (owns `PhaseStarted` as step N.1) and an end helper (owns `PhaseEnded`). `step_phase` dispatches to drivers and suppresses boundary emits the drivers own. `start_scenario` skips Mythos entirely in round 1 (no Mythos boundary events fire — the rules say "skip the mythos phase," not "run it empty"). The Mythos sub-steps 1.4 and the per-card 5-step sequence (peril check, Revelation, enemy spawn, surge re-draw) land as named call sites with TODO bodies for the steps owned by other PRs (#73 doom + threshold; future-peril-PR conferral enforcement). `PlayerAction::DrawEncounterCard` is a top-level player action peer to `Investigate` / `Move` / `PlayCard`, not an `AwaitingInput` sub-choice. The new `open_fast_window` helper short-circuits printed Fast windows when no reactions queue AND no investigator has any Fast-playable option — eligibility is computed via the extracted `check_play_card` / `check_activate_ability` validators so it's the real PlayCard / ActivateAbility gate, not a parallel weak filter.

**Tech Stack:** Rust 2021, `serde`, `card-dsl` (pure data), `game-core` (kernel), `scenarios` (synthetic fixtures), `card-data-pipeline` (corpus regeneration).

**Spec:** `docs/superpowers/specs/2026-05-24-69-mythos-phase-content-design.md` is the authoritative design — re-read it when starting and when in doubt about a decision.

**Branch name:** `engine/mythos-phase-content`.

**PR procedure:** CLAUDE.md's 7-step PR procedure applies. This plan covers steps 1 (local CI gauntlet), 2 (commits on a feature branch), 6 (phase-doc update as last commit), and the PR-open hand-off. CI watch + addressing CI failures and the user-approved merge are driven by the human after the PR opens.

---

## Design decisions locked in before coding

These resolve the spec's "Open questions" section and a few additional choices made while reading current code. If implementation surfaces a reason to revisit any of these, raise it before pressing on.

1. **Helper name: `open_fast_window`.** The spec flagged this as final-name-decidable at implementation time. Picked over `push_player_window` / `queue_player_window` because (a) it emphasizes the auto-skip-on-no-eligibility behavior that's the whole point of the helper, and (b) it doesn't collide with the existing `queue_reaction_window` semantics. The helper is `pub(super) fn open_fast_window(state, events, kind)` and lives in `engine/dispatch.rs` alongside `queue_reaction_window`.
2. **`peril_check` is a real function, not an inline comment.** Spec was ambivalent. Picked the function because (a) it's grep-able by name, (b) the future peril-enforcement PR has a single body to fill in without changing the driver shape, and (c) the function signature documents what data the enforcement will need (state, events, card code, drawing investigator, peril flag). The empty body is fine — a 4-line `_ = state; _ = events; _ = code; _ = investigator; _ = is_peril;` to silence unused-param warnings plus a TODO comment.
3. **`run_window_continuation` is a `match` on `WindowKind` in `dispatch.rs`.** Spec mentioned a closure / id alternative; picked the match because it's the smallest possible shape while only one window kind has a continuation. When the second kind needs one, revisit.
4. **`state.mythos_draw_pending: Option<InvestigatorId>` defaults to `None` in `GameState::default()`.** The cursor is `Some(_)` only between `mythos_phase` entry and the last drawer's completion. Default-`None` matches the "scenario hasn't started yet" semantic.
5. **`MAX_SURGE_CHAIN = 64` lives as a `const` directly in `engine/dispatch.rs` next to `mythos_draw_for`.** Not a public engine config; not exposed via builder; not a feature flag.
6. **No explicit `#[should_panic]` test for the surge `unreachable!` sites.** Doc-comments at the call site cite the rules and explain the trigger condition; that's the contract. Adding panic tests would couple the test to the message text and force `catch_unwind` plumbing for negligible value.
7. **`investigation_phase` rotates to lead unconditionally (lead-first default).** Spec frames the explicit player-pick as a future PR concern. For #69's scope, the lead-first default is correct and matches the existing `start_scenario` / `end_turn` rotate-to-first behavior — no behavior change for callers, the rotation just moves to the driver function.
8. **The `Event::WindowOpened` retrofit emits at every existing call site that pushes to `state.open_windows`.** That's currently only `queue_reaction_window` in `engine/dispatch.rs`. Existing tests in `engine/mod.rs` and `engine/dispatch.rs` that assert on exact event sequences in the `AfterEnemyDefeated` flow get updated to expect the new event in the sequence.
9. **`with_encounter_deck` lives in `crates/scenarios/src/test_fixtures/synthetic.rs` as a `pub` helper.** Mirrors the existing module pattern; integration tests in `crates/scenarios/tests/*.rs` import it.
10. **`PlayerAction::DrawEncounterCard` carries no payload.** The acting investigator is plumbed in via the existing dispatch wrapper that passes `investigator` to handlers (see `apply_player_action` in `engine/mod.rs`). No need for an investigator field on the variant — symmetric with `PlayerAction::EndTurn`.

---

## File map

- **Create:**
  - `crates/scenarios/tests/mythos_phase.rs` — integration test binary (own process; installs `TEST_REGISTRY`).
- **Modify:**
  - `crates/card-dsl/src/card_data.rs` — `surge: bool`, `peril: bool` on `CardMetadata`; unit tests.
  - `crates/card-data-pipeline/src/main.rs` — emit `surge: false, peril: false` for every card.
  - `crates/cards/src/generated/cards.rs` — regenerated; every entry gets `surge: false, peril: false`.
  - `crates/game-core/src/state/game_state.rs` — add `mythos_draw_pending: Option<InvestigatorId>` field on `GameState`; add `WindowKind::MythosAfterDraws` variant; serde tests.
  - `crates/game-core/src/event.rs` — add `Event::WindowOpened { kind: WindowKind }`; serde test.
  - `crates/game-core/src/action.rs` — add `PlayerAction::DrawEncounterCard`.
  - `crates/game-core/src/engine/dispatch.rs` — extract `check_play_card` + `check_activate_ability` validators; add `any_fast_play_eligible`, `open_fast_window`, `run_window_continuation`; add `investigation_phase`, `mythos_phase`, `mythos_phase_end`, `peril_check`, `mythos_draw_for`, `draw_encounter_card`; refactor `step_phase`, `start_scenario`, `end_turn`, `encounter_card_revealed`; retrofit `queue_reaction_window` to emit `Event::WindowOpened`; wire `close_reaction_window_at` to call `run_window_continuation` after `WindowClosed`; tests for new behavior.
  - `crates/game-core/src/engine/mod.rs` — `apply_player_action` dispatch arm for `DrawEncounterCard`; update any existing tests that asserted on the prior phantom Mythos boundary emits at scenario start.
  - `crates/game-core/src/test_support/builder.rs` — update any `with_phase` / fixture wiring that names the dropped phantom emits in its docs.
  - `crates/scenarios/src/test_fixtures/synth_cards.rs` — add `SYNTH_SURGE_TREACHERY_CODE` const + metadata + abilities; extend `metadata_for` and `abilities_for` lookups.
  - `crates/scenarios/src/test_fixtures/synthetic.rs` — add `with_encounter_deck` helper.
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
- Add: `docs/superpowers/plans/2026-05-24-69-mythos-phase-content.md` (this file).
- Add: `docs/superpowers/specs/2026-05-24-69-mythos-phase-content-design.md` (spec, already written).

- [ ] **Step 1: Create the feature branch from main**

```bash
git checkout main
git pull
git checkout -b engine/mythos-phase-content
```

- [ ] **Step 2: Commit the spec + plan**

`docs/superpowers/` is tracked in git (PR #132 + #133 set the convention). Add both as the branch's first commit:

```bash
git add docs/superpowers/specs/2026-05-24-69-mythos-phase-content-design.md \
        docs/superpowers/plans/2026-05-24-69-mythos-phase-content.md
git commit -m "$(cat <<'EOF'
docs: spec + implementation plan for #69

Refs #69.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: Add `surge: bool` + `peril: bool` to `CardMetadata` and regenerate the corpus

**Files:**
- Modify: `crates/card-dsl/src/card_data.rs`
- Modify: `crates/card-data-pipeline/src/main.rs`
- Modify (generated): `crates/cards/src/generated/cards.rs`

`CardMetadata` is **not** `#[non_exhaustive]`; adding fields requires every struct literal in tests + fixtures to add the new fields. The corpus regeneration handles ~600 generated entries in one pass.

- [ ] **Step 1: Write failing serde tests for the new fields**

Append to the `#[cfg(test)] mod is_fast_tests` block (or a new sibling mod) in `crates/card-dsl/src/card_data.rs`:

```rust
#[test]
fn card_metadata_serde_roundtrip_preserves_surge_and_peril() {
    let original = CardMetadata {
        code: "_synth_surge_treachery".into(),
        name: "Synth Surge Treachery".into(),
        class: Class::Mythos,
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
        deck_limit: 1,
        quantity: 1,
        pack_code: "_synth".into(),
        position: 1,
        is_fast: false,
        spawn: None,
        surge: true,
        peril: false,
    };
    let json = serde_json::to_string(&original).expect("serialize");
    let back: CardMetadata = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back, original);
    assert!(back.surge);
    assert!(!back.peril);
}
```

- [ ] **Step 2: Confirm compile failure**

```bash
cargo test -p card-dsl card_metadata_serde_roundtrip_preserves_surge_and_peril 2>&1 | head -20
```

Expected: errors like `no field surge on type CardMetadata` and `no field peril on type CardMetadata`.

- [ ] **Step 3: Add `surge` and `peril` fields to `CardMetadata`**

In `crates/card-dsl/src/card_data.rs`, inside `pub struct CardMetadata`, append after the existing `pub spawn: Option<Spawn>` field:

```rust
    /// Surge keyword (Rules Reference p.19). When `true`, after the
    /// card is drawn and resolved during a Mythos encounter draw, the
    /// drawing investigator immediately draws another encounter card.
    /// The pipeline emits `false` for every card until the first
    /// Phase-7+ scenario with a real surge-bearing card forces the
    /// pipeline-update work; the synthetic fixture sets `true`
    /// on its surge-bearing treachery to exercise the engine path.
    pub surge: bool,
    /// Peril keyword (Rules Reference p.18, referenced in p.24 1.4
    /// step 2). When `true`, the drawing investigator cannot confer
    /// and other players cannot play cards / trigger abilities /
    /// commit to that investigator's skill tests during resolution.
    /// Enforcement is not yet wired — no machinery exists for
    /// cross-investigator commit blocking. The field exists so cards
    /// can carry the keyword and the engine's step-2 call site can
    /// become load-bearing when the enforcement PR lands.
    pub peril: bool,
```

- [ ] **Step 4: Update existing `CardMetadata` struct-literal sites in `card-dsl` tests**

Other tests in `crates/card-dsl/src/card_data.rs` (e.g. `is_fast_tests::metadata_serde_roundtrip_preserves_is_fast`, `spawn_tests::card_metadata_serde_roundtrip_preserves_spawn_*`) all construct `CardMetadata` with explicit field values. Append `surge: false, peril: false,` to each existing struct literal — exhaustive struct construction without the new fields will fail to compile.

`grep -n "CardMetadata {" crates/card-dsl/src/card_data.rs` lists each call site; update them all.

- [ ] **Step 5: Verify `card-dsl` compiles and tests pass**

```bash
cargo test -p card-dsl --all-features 2>&1 | tail -20
```

Expected: all card-dsl tests pass, including the new surge/peril roundtrip and the updated existing literals.

- [ ] **Step 6: Update the pipeline emitter to write `surge: false, peril: false` for every card**

In `crates/card-data-pipeline/src/main.rs`, find the function that writes generated card metadata (search for `is_fast` to locate the existing emit block). Add `surge: false,` and `peril: false,` after the existing `is_fast` emit line, mirroring the format:

```rust
writeln!(out, "        is_fast: {},", card.is_fast)?;
writeln!(out, "        spawn: None,")?;
writeln!(out, "        surge: false,")?;
writeln!(out, "        peril: false,")?;
```

(Exact `writeln!` shape will match the file's existing style — preserve indentation + trailing comma conventions.)

- [ ] **Step 7: Run the pipeline to regenerate the corpus**

```bash
cargo run -p card-data-pipeline
```

This rewrites `crates/cards/src/generated/cards.rs`. Inspect `git diff crates/cards/src/generated/cards.rs | head -40` to confirm only `surge: false,` + `peril: false,` lines were added (one of each per card entry) — no other changes.

- [ ] **Step 8: Verify `cards` compiles + corpus tests pass**

```bash
cargo build -p cards 2>&1 | tail -10
cargo test -p cards --all-features 2>&1 | tail -10
```

Expected: cards crate compiles; existing per-card tests still pass.

- [ ] **Step 9: Update fixture struct literals in `scenarios` test fixtures**

`crates/scenarios/src/test_fixtures/synth_cards.rs` constructs `CardMetadata` for `_synth_treachery` and `_synth_enemy`. Both need `surge: false, peril: false,` appended.

```bash
grep -n "CardMetadata {" crates/scenarios/src/test_fixtures/synth_cards.rs
```

For every match, add the two new fields at the end of the struct literal.

- [ ] **Step 10: Full workspace compile**

```bash
RUSTFLAGS="-D warnings" cargo test --workspace --all-features 2>&1 | tail -20
cargo clippy --all-targets --all-features -- -D warnings 2>&1 | tail -10
```

Expected: all green.

- [ ] **Step 11: Commit**

```bash
git add crates/card-dsl/src/card_data.rs \
        crates/card-data-pipeline/src/main.rs \
        crates/cards/src/generated/cards.rs \
        crates/scenarios/src/test_fixtures/synth_cards.rs
git commit -m "$(cat <<'EOF'
infra: add surge + peril fields on CardMetadata

Adds two new boolean keyword fields to CardMetadata. The pipeline
emits `false` for every card in this PR; the first Phase-7+ scenario
with a real surge or peril card forces the structured-keyword-parsing
work. Synthetic fixtures populate `false` explicitly; #69's per-card
Mythos sub-sequence (5 step / surge re-draw / peril check call site)
reads these fields.

Refs #69.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: Extract `check_play_card` validator from `play_card`

**Files:**
- Modify: `crates/game-core/src/engine/dispatch.rs`

Pure refactor: lift `play_card`'s validation block (`dispatch.rs:2930-3007`) into a separate `check_play_card` function returning `Result<PlayCheckResult, Cow<'static, str>>`. `play_card`'s mutation block calls the validator and unpacks the `Ok` payload. No behavior change at the call site.

- [ ] **Step 1: Read the existing `play_card` body to lock in the lift boundary**

Open `crates/game-core/src/engine/dispatch.rs` and locate `fn play_card` (around line 2924). Identify:

- Validation prefix: from `let Some(inv) = state.investigators.get(&investigator)` through the `if !allowed { return Rejected; }` block.
- Mutation suffix: from `events.push(Event::CardPlayed { ... })` onward.

The marker is the existing `// Mutate.` comment line. Everything above it goes into the validator; everything below stays in `play_card`.

- [ ] **Step 2: Write failing tests for the validator's API shape**

Add a `#[cfg(test)] mod check_play_card_tests` block at the bottom of `engine/dispatch.rs` (or in the appropriate existing test module):

```rust
#[cfg(test)]
mod check_play_card_tests {
    use super::*;
    use crate::test_support::{TestGame, test_investigator};

    #[test]
    fn check_play_card_returns_err_for_unknown_hand_index() {
        let state = TestGame::default()
            .with_investigator(test_investigator(1))
            .with_active_investigator(InvestigatorId(1))
            .build();
        let err = check_play_card(&state, InvestigatorId(1), 0)
            .expect_err("empty hand should reject");
        assert!(err.contains("hand_index"));
    }

    #[test]
    fn check_play_card_returns_err_when_investigator_missing() {
        let state = TestGame::default().build();
        let err = check_play_card(&state, InvestigatorId(99), 0)
            .expect_err("missing investigator should reject");
        assert!(err.contains("not in state"));
    }
}
```

(Imports may need adjustment based on existing test conventions in `dispatch.rs`.)

- [ ] **Step 3: Confirm compile failure**

```bash
cargo test -p game-core check_play_card_tests 2>&1 | head -20
```

Expected: `cannot find function check_play_card`.

- [ ] **Step 4: Add the `PlayCheckResult` struct + `check_play_card` function**

In `crates/game-core/src/engine/dispatch.rs`, add immediately above the existing `fn play_card`:

```rust
/// Validated payload returned by [`check_play_card`] on success.
/// Carries the data `play_card`'s mutation step needs without
/// re-running the validation.
pub(super) struct PlayCheckResult {
    pub destination: PlayDestination,
    pub abilities: Vec<Ability>,
    pub is_fast: bool,
    pub card_type: CardType,
}

/// Pure-validation peer to [`play_card`]. Returns `Ok` if the named
/// card is currently playable by `investigator`, `Err(reason)` if
/// not. The check is the existing `play_card` validation block lifted
/// verbatim — no behavior change at `play_card`'s call site.
///
/// Used by [`play_card`] (which then runs the mutation block on the
/// `Ok` payload) and by [`any_fast_play_eligible`] (which only
/// inspects `Ok` vs `Err`).
pub(super) fn check_play_card(
    state: &GameState,
    investigator: InvestigatorId,
    hand_index: u8,
) -> Result<PlayCheckResult, Cow<'static, str>> {
    let Some(inv) = state.investigators.get(&investigator) else {
        return Err(format!("PlayCard: investigator {investigator:?} is not in state").into());
    };
    if inv.status != Status::Active {
        return Err(format!(
            "PlayCard: {investigator:?} is not Active (status {:?})",
            inv.status,
        )
        .into());
    }
    let idx = usize::from(hand_index);
    if idx >= inv.hand.len() {
        return Err(format!(
            "PlayCard: hand_index {hand_index} out of bounds (hand size {})",
            inv.hand.len(),
        )
        .into());
    }
    let code: CardCode = inv.hand[idx].clone();
    let (destination, abilities, is_fast, card_type) = match resolve_play_target(&code) {
        Ok(v) => v,
        // resolve_play_target returns EngineOutcome::Rejected; lift the reason out for the
        // Result shape.
        Err(EngineOutcome::Rejected { reason }) => return Err(reason),
        Err(other) => unreachable!(
            "resolve_play_target returned non-Rejected error: {other:?}"
        ),
    };
    let active_during_investigation =
        state.phase == Phase::Investigation && state.active_investigator == Some(investigator);
    let owner_is_active = state.active_investigator == Some(investigator);
    let permissive_window = state
        .open_windows
        .last()
        .is_some_and(|w| w.fast_actors.permits(investigator));
    let allowed = if is_fast {
        match card_type {
            CardType::Event => active_during_investigation || permissive_window,
            CardType::Asset => {
                active_during_investigation || (owner_is_active && permissive_window)
            }
            _ => active_during_investigation,
        }
    } else {
        active_during_investigation
    };
    if !allowed {
        return Err(format!(
            "PlayCard: card not playable in this timing window. \
             Rules Reference p. 11: non-Fast cards require Investigation + active \
             investigator; Fast events require active investigator or a window whose \
             fast_actors permits the actor; Fast assets additionally require the OWNER \
             (active investigator) to act. \
             Got is_fast={is_fast}, card_type={card_type:?}, phase={phase:?}, \
             active={active:?}, actor={investigator:?}, owner_is_active={owner_is_active}, \
             permissive_window={permissive_window}.",
            phase = state.phase,
            active = state.active_investigator,
        )
        .into());
    }
    Ok(PlayCheckResult {
        destination,
        abilities,
        is_fast,
        card_type,
    })
}
```

Adjust the `resolve_play_target` error mapping if its current return shape differs (likely returns `Result<(...), EngineOutcome>` per the existing call site — confirm at the source line and match).

- [ ] **Step 5: Replace `play_card`'s validation block with a `check_play_card` call**

The original `play_card` validation block is everything before the `// Mutate.` comment. Replace it with:

```rust
fn play_card(
    state: &mut GameState,
    events: &mut Vec<Event>,
    investigator: InvestigatorId,
    hand_index: u8,
) -> EngineOutcome {
    let PlayCheckResult {
        destination,
        abilities,
        is_fast: _,
        card_type: _,
    } = match check_play_card(state, investigator, hand_index) {
        Ok(r) => r,
        Err(reason) => return EngineOutcome::Rejected { reason },
    };
    let code: CardCode = state
        .investigators
        .get(&investigator)
        .expect("checked in validator")
        .hand[usize::from(hand_index)]
        .clone();

    // Mutate.
    events.push(Event::CardPlayed {
        investigator,
        code: code.clone(),
    });
    // ... rest of the original mutation block unchanged ...
}
```

(`is_fast` and `card_type` are discarded — they were only computed for the timing gate. The Mutation block doesn't need them.)

- [ ] **Step 6: Run the existing `play_card` test suite to verify no behavior change**

```bash
RUSTFLAGS="-D warnings" cargo test -p game-core play_card 2>&1 | tail -20
RUSTFLAGS="-D warnings" cargo test -p cards 2>&1 | tail -10
```

Expected: every existing `play_card` test passes; cards integration tests pass.

- [ ] **Step 7: Run the new validator unit tests**

```bash
cargo test -p game-core check_play_card_tests 2>&1 | tail -10
```

Expected: PASS.

- [ ] **Step 8: Full game-core gauntlet**

```bash
RUSTFLAGS="-D warnings" cargo test -p game-core --all-features 2>&1 | tail -10
cargo clippy -p game-core --all-targets --all-features -- -D warnings 2>&1 | tail -10
RUSTDOCFLAGS="-D warnings" cargo doc -p game-core --no-deps --all-features 2>&1 | tail -10
```

- [ ] **Step 9: Commit**

```bash
git add crates/game-core/src/engine/dispatch.rs
git commit -m "$(cat <<'EOF'
engine: extract check_play_card validator from play_card

Pure refactor: lifts play_card's validation block into a new
pub(super) check_play_card function returning Result<PlayCheckResult,
Cow<'static, str>>. play_card calls the validator and unpacks the Ok
payload. No behavior change at the call site.

The validator becomes reusable by any_fast_play_eligible (next task),
which needs to ask "would PlayCard accept this?" without mutating
state.

Refs #69.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: Extract `check_activate_ability` validator from `activate_ability`

**Files:**
- Modify: `crates/game-core/src/engine/dispatch.rs`

Same shape as Task 3 for `activate_ability`. Locate the existing validation block at `dispatch.rs:3088`-ish (everything before the cost-payment / `Event::AbilityActivated` emit).

- [ ] **Step 1: Identify the lift boundary**

Open `fn activate_ability` and find the boundary between validation (investigator-exists, status, instance lookup, ability index, trigger-is-Activated, timing gate, cost-payability check) and mutation (cost payment + event emit + effect dispatch).

- [ ] **Step 2: Write failing validator tests**

In the same test module as `check_play_card_tests` (or a sibling `check_activate_ability_tests`):

```rust
#[test]
fn check_activate_ability_returns_err_for_missing_instance() {
    let state = TestGame::default()
        .with_investigator(test_investigator(1))
        .with_active_investigator(InvestigatorId(1))
        .build();
    let err = check_activate_ability(&state, InvestigatorId(1), CardInstanceId(999), 0)
        .expect_err("missing instance should reject");
    assert!(err.contains("no in-play instance"));
}

#[test]
fn check_activate_ability_returns_err_when_investigator_missing() {
    let state = TestGame::default().build();
    let err = check_activate_ability(&state, InvestigatorId(99), CardInstanceId(1), 0)
        .expect_err("missing investigator should reject");
    assert!(err.contains("not in state"));
}
```

- [ ] **Step 3: Confirm compile failure**

```bash
cargo test -p game-core check_activate_ability_tests 2>&1 | head -10
```

Expected: `cannot find function check_activate_ability`.

- [ ] **Step 4: Add the `ActivateCheckResult` struct + `check_activate_ability` function**

```rust
/// Validated payload returned by [`check_activate_ability`] on success.
pub(super) struct ActivateCheckResult {
    /// Position of the source card in the investigator's `cards_in_play`.
    pub in_play_pos: usize,
    /// The ability being activated (cloned from the registry during
    /// validation), passed forward to the mutation step.
    pub ability: Ability,
    /// Whether the source card was exhausted at validation time —
    /// load-bearing for activated abilities whose payment includes
    /// `Cost::Exhaust`.
    pub source_exhausted: bool,
}

/// Pure-validation peer to [`activate_ability`]. Mirrors
/// [`check_play_card`]: validation block lifted verbatim, no behavior
/// change at the call site.
pub(super) fn check_activate_ability(
    state: &GameState,
    investigator: InvestigatorId,
    instance_id: CardInstanceId,
    ability_index: u8,
) -> Result<ActivateCheckResult, Cow<'static, str>> {
    // (Body is the existing activate_ability validation block —
    //  investigator-exists, status, instance lookup, ability
    //  resolution from the registry, trigger-is-Activated check,
    //  timing gate, cost-payability check — adjusted to return
    //  Err(reason) instead of EngineOutcome::Rejected { reason }.)
    //
    //  Implementation note: the existing handler computes
    //  `source_exhausted` and `in_play_pos` mid-validation; capture
    //  them in the result struct to avoid re-searching during the
    //  mutation step.
    todo!("lift the validation block from activate_ability, returning ActivateCheckResult on success")
}
```

The implementer fills in the `todo!()` by lifting the existing validation block from `activate_ability` (everything before the cost-payment phase). Mirror the `check_play_card` extraction's shape.

- [ ] **Step 5: Replace `activate_ability`'s validation block with the validator call**

```rust
fn activate_ability(
    state: &mut GameState,
    events: &mut Vec<Event>,
    investigator: InvestigatorId,
    instance_id: CardInstanceId,
    ability_index: u8,
) -> EngineOutcome {
    let ActivateCheckResult {
        in_play_pos,
        ability,
        source_exhausted,
    } = match check_activate_ability(state, investigator, instance_id, ability_index) {
        Ok(r) => r,
        Err(reason) => return EngineOutcome::Rejected { reason },
    };
    // ... existing mutation block (pay costs, emit AbilityActivated, dispatch effect) ...
}
```

- [ ] **Step 6: Verify all activate_ability tests pass + new validator tests pass**

```bash
RUSTFLAGS="-D warnings" cargo test -p game-core activate 2>&1 | tail -20
RUSTFLAGS="-D warnings" cargo test -p game-core check_activate_ability_tests 2>&1 | tail -10
```

- [ ] **Step 7: Full game-core gauntlet**

```bash
RUSTFLAGS="-D warnings" cargo test -p game-core --all-features 2>&1 | tail -10
cargo clippy -p game-core --all-targets --all-features -- -D warnings 2>&1 | tail -10
RUSTDOCFLAGS="-D warnings" cargo doc -p game-core --no-deps --all-features 2>&1 | tail -10
```

- [ ] **Step 8: Commit**

```bash
git add crates/game-core/src/engine/dispatch.rs
git commit -m "$(cat <<'EOF'
engine: extract check_activate_ability validator from activate_ability

Mirror of the check_play_card extraction. Pure refactor.

Refs #69.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: Add `any_fast_play_eligible` helper

**Files:**
- Modify: `crates/game-core/src/engine/dispatch.rs`

Walks every investigator's hand + cards-in-play looking for a Fast-play option the existing PlayCard / ActivateAbility gate would accept. Uses the extracted validators from Tasks 3-4 so the eligibility check is the real gate, not a parallel filter.

- [ ] **Step 1: Write failing tests for the scan**

In `engine/dispatch.rs`'s test section:

```rust
#[cfg(test)]
mod any_fast_play_eligible_tests {
    use super::*;
    use crate::test_support::{TestGame, test_investigator};

    #[test]
    fn returns_false_when_no_investigators() {
        let state = TestGame::default().build();
        assert!(!any_fast_play_eligible(&state));
    }

    #[test]
    fn returns_false_when_hands_and_in_play_empty() {
        let state = TestGame::default()
            .with_investigator(test_investigator(1))
            .build();
        assert!(!any_fast_play_eligible(&state));
    }

    // Positive Fast-eligible tests rely on the card registry being
    // installed with cards that have is_fast / Activated{action_cost:
    // 0} abilities. Those tests land in the integration test file
    // (Task 14) where the test registry is installed.
}
```

The positive paths are covered by the Task 14 integration tests where a registry with the synth surge treachery + future Fast cards exists.

- [ ] **Step 2: Confirm compile failure**

```bash
cargo test -p game-core any_fast_play_eligible_tests 2>&1 | head -10
```

Expected: `cannot find function any_fast_play_eligible`.

- [ ] **Step 3: Implement `any_fast_play_eligible`**

In `crates/game-core/src/engine/dispatch.rs`:

```rust
/// Returns `true` if any investigator has at least one playable Fast
/// option in the current state — either a Fast card in hand or a
/// non-exhausted 0-action Activated ability on a card in play.
/// Used by [`open_fast_window`] to short-circuit windows where
/// nobody can act.
///
/// Eligibility uses the extracted [`check_play_card`] /
/// [`check_activate_ability`] validators so the gate is exactly the
/// existing PlayCard / ActivateAbility gate — no parallel
/// implementation, no drift.
///
/// Returns `false` when the card registry isn't installed (tests
/// that don't touch card data) — same fallback as
/// [`scan_pending_triggers`].
pub(super) fn any_fast_play_eligible(state: &GameState) -> bool {
    let Some(reg) = crate::card_registry::current() else {
        return false;
    };
    for (&inv_id, inv) in &state.investigators {
        // Fast events / Fast assets in hand.
        for hand_idx_usize in 0..inv.hand.len() {
            let Ok(hand_idx) = u8::try_from(hand_idx_usize) else {
                break;
            };
            if let Ok(result) = check_play_card(state, inv_id, hand_idx) {
                if result.is_fast {
                    return true;
                }
            }
        }
        // 0-action Activated abilities on cards in play.
        for card in &inv.cards_in_play {
            let Some(abilities) = (reg.abilities_for)(&card.code) else {
                continue;
            };
            for (ab_idx, ability) in abilities.iter().enumerate() {
                let Trigger::Activated { action_cost: 0, .. } = ability.trigger else {
                    continue;
                };
                let Ok(ab_idx_u8) = u8::try_from(ab_idx) else {
                    break;
                };
                if check_activate_ability(state, inv_id, card.instance_id, ab_idx_u8).is_ok() {
                    return true;
                }
            }
        }
    }
    false
}
```

- [ ] **Step 4: Verify negative tests pass**

```bash
cargo test -p game-core any_fast_play_eligible_tests 2>&1 | tail -10
```

Expected: 2 tests pass.

- [ ] **Step 5: Full game-core gauntlet**

```bash
RUSTFLAGS="-D warnings" cargo test -p game-core --all-features 2>&1 | tail -10
cargo clippy -p game-core --all-targets --all-features -- -D warnings 2>&1 | tail -10
RUSTDOCFLAGS="-D warnings" cargo doc -p game-core --no-deps --all-features 2>&1 | tail -10
```

- [ ] **Step 6: Commit**

```bash
git add crates/game-core/src/engine/dispatch.rs
git commit -m "$(cat <<'EOF'
engine: add any_fast_play_eligible scan helper

Walks every investigator's hand + cards_in_play, using the extracted
check_play_card / check_activate_ability validators to determine
whether any Fast event or 0-action Activated ability would be
accepted by PlayCard / ActivateAbility right now. Returns false when
no card registry is installed (matches scan_pending_triggers fallback).

Used in the next task by open_fast_window to auto-skip printed Fast
windows where nobody can act.

Refs #69.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: State-shape additions — `WindowOpened` event, `MythosAfterDraws` window kind, `mythos_draw_pending` cursor

**Files:**
- Modify: `crates/game-core/src/event.rs`
- Modify: `crates/game-core/src/state/game_state.rs`

All three additions are small struct/enum changes that the rest of the work depends on. Bundling them keeps the workspace compiling between commits.

- [ ] **Step 1: Write failing serde test for `Event::WindowOpened`**

In `crates/game-core/src/event.rs`, append to the existing test module:

```rust
#[test]
fn window_opened_serde_roundtrip() {
    let ev = Event::WindowOpened {
        kind: WindowKind::BetweenPhases {
            from: Phase::Mythos,
            to: Phase::Investigation,
        },
    };
    let json = serde_json::to_string(&ev).expect("serialize");
    let back: Event = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back, ev);
}
```

- [ ] **Step 2: Write failing serde test for `WindowKind::MythosAfterDraws`**

In `crates/game-core/src/state/game_state.rs`'s existing `open_window_tests` module:

```rust
#[test]
fn mythos_after_draws_window_kind_serde_roundtrip() {
    let kind = WindowKind::MythosAfterDraws;
    let json = serde_json::to_string(&kind).expect("serialize");
    let back: WindowKind = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back, kind);
}
```

- [ ] **Step 3: Write failing test for the `mythos_draw_pending` default**

In `crates/game-core/src/state/game_state.rs`'s test module:

```rust
#[test]
fn game_state_default_has_no_mythos_draw_pending() {
    let state = GameState::default();
    assert_eq!(state.mythos_draw_pending, None);
}
```

- [ ] **Step 4: Confirm all three compile-fail**

```bash
cargo test -p game-core window_opened_serde_roundtrip mythos_after_draws_window_kind_serde_roundtrip game_state_default_has_no_mythos_draw_pending 2>&1 | head -20
```

Expected: compile errors naming `Event::WindowOpened`, `WindowKind::MythosAfterDraws`, and `mythos_draw_pending` as missing.

- [ ] **Step 5: Add `Event::WindowOpened` variant**

In `crates/game-core/src/event.rs`, add (mirroring `Event::WindowClosed`'s shape):

```rust
    /// A player window opened. Symmetric with [`Event::WindowClosed`].
    /// Emitted by every path that pushes onto `state.open_windows`
    /// (today: [`queue_reaction_window`] for after-event reaction
    /// windows; [`open_fast_window`] for printed Fast windows,
    /// landing in #69). Order with `WindowClosed`: `WindowOpened {
    /// kind: K }` always precedes the matching `WindowClosed { kind:
    /// K }` for the same window instance.
    WindowOpened {
        kind: WindowKind,
    },
```

(Path references in the doc comment use the existing module path; adjust if `queue_reaction_window` lives somewhere different from where the doc-comment expects.)

- [ ] **Step 6: Add `WindowKind::MythosAfterDraws` variant**

In `crates/game-core/src/state/game_state.rs`, inside the existing `pub enum WindowKind` block (which is `#[non_exhaustive]`):

```rust
    /// The player window between Rules Reference p.24 step 1.4
    /// (each investigator draws an encounter card) and step 1.5
    /// (Mythos phase ends). Carries no payload — there is no
    /// `EventPattern` today that matches against this specifically;
    /// the variant exists so the rule's printed timing point is
    /// addressable when a future card binds to it (see the
    /// "Generalize WindowKind to PlayerWindow" follow-up issue for
    /// the consideration to collapse this with `BetweenPhases` once
    /// routing-load-bearing data is clearer).
    MythosAfterDraws,
```

- [ ] **Step 7: Add `mythos_draw_pending: Option<InvestigatorId>` field on `GameState`**

In `crates/game-core/src/state/game_state.rs`, inside `pub struct GameState`, append after the existing field most aligned with phase progression (e.g. near `phase: Phase` and `round: u32`):

```rust
    /// The investigator whose Mythos-phase encounter draw is pending,
    /// during Rules-Reference p.24 step 1.4. `Some(id)` between
    /// `mythos_phase` entry and the last drawer's completion; `None`
    /// otherwise. Advanced after each `PlayerAction::DrawEncounterCard`
    /// completes its chain (including any surge re-draws). `None`
    /// once all investigators have drawn — at which point the
    /// `MythosAfterDraws` window opens.
    pub mythos_draw_pending: Option<InvestigatorId>,
```

- [ ] **Step 8: Update `GameState::default()` to initialize `mythos_draw_pending: None`**

Find the `impl Default for GameState` block (or the constructor pattern the existing code uses) and add `mythos_draw_pending: None,` to the field initialization.

- [ ] **Step 9: Update `TestGame::build()` if it constructs `GameState` directly**

In `crates/game-core/src/test_support/builder.rs`, locate the `build` method that produces `GameState`. If it spells out every field explicitly (as opposed to spreading from `Default`), add `mythos_draw_pending: None,` there too.

- [ ] **Step 10: Verify the new tests pass**

```bash
RUSTFLAGS="-D warnings" cargo test -p game-core window_opened_serde_roundtrip mythos_after_draws_window_kind_serde_roundtrip game_state_default_has_no_mythos_draw_pending 2>&1 | tail -10
```

Expected: 3 PASS.

- [ ] **Step 11: Full workspace compile**

```bash
RUSTFLAGS="-D warnings" cargo test --workspace --all-features 2>&1 | tail -20
cargo clippy --all-targets --all-features -- -D warnings 2>&1 | tail -10
```

Expected: all green. `GameState` consumers (server, web, scenarios) should compile cleanly because of the `Default`-initialized new field.

- [ ] **Step 12: Commit**

```bash
git add crates/game-core/src/event.rs \
        crates/game-core/src/state/game_state.rs \
        crates/game-core/src/test_support/builder.rs
git commit -m "$(cat <<'EOF'
engine: add WindowOpened event + MythosAfterDraws window kind + mythos_draw_pending cursor

Three small additions the #69 Mythos driver depends on:

- Event::WindowOpened — symmetric with WindowClosed. Future
  open_fast_window helper and the existing queue_reaction_window
  emit it.
- WindowKind::MythosAfterDraws — the post-1.4 printed player window
  per Rules Reference p.24.
- GameState.mythos_draw_pending: Option<InvestigatorId> — cursor for
  the 1.4 per-investigator draw loop.

No behavior change yet; later tasks wire these in.

Refs #69.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 7: Add `open_fast_window` + `run_window_continuation`; retrofit `queue_reaction_window` to emit `WindowOpened`; wire `close_reaction_window_at`

**Files:**
- Modify: `crates/game-core/src/engine/dispatch.rs`

The helper that printed Fast windows go through, plus the kind-aware continuation dispatch that fires after a window closes (used by `MythosAfterDraws` to chain into `mythos_phase_end`). Also retrofits `queue_reaction_window` to emit `WindowOpened` so the event stream stays consistent.

- [ ] **Step 1: Update existing tests asserting full `AfterEnemyDefeated` event sequences**

`queue_reaction_window` will start emitting `Event::WindowOpened` before pushing onto `state.open_windows`. Any existing test in `engine/dispatch.rs` or `engine/mod.rs` that asserts on the contiguous event sequence around `AfterEnemyDefeated` needs `Event::WindowOpened { kind: WindowKind::AfterEnemyDefeated { .. } }` inserted at the right position.

Grep for affected tests:

```bash
grep -rn "AfterEnemyDefeated" crates/game-core/src/ | grep -v "//" | head -20
```

For each test that uses `assert_eq!` on a `&events[..]` slice (or `assert_event_sequence!`), add the new `WindowOpened` event between the event that triggers the reaction window and the previously-asserted `WindowClosed` (or trigger-fire) events. Tests using `assert_event!` / `assert_no_event!` (order-insensitive) need the new event added to whichever positive assertions list "all events I expect to see."

- [ ] **Step 2: Write failing tests for `open_fast_window`**

```rust
#[cfg(test)]
mod open_fast_window_tests {
    use super::*;
    use crate::test_support::{TestGame, test_investigator};

    #[test]
    fn open_fast_window_with_no_eligibility_emits_open_then_close_inline() {
        let mut state = TestGame::default()
            .with_investigator(test_investigator(1))
            .build();
        let mut events = Vec::new();
        open_fast_window(&mut state, &mut events, WindowKind::MythosAfterDraws);
        // No reactions, no Fast-eligible cards → auto-skip.
        assert!(state.open_windows.is_empty());
        // Events: WindowOpened followed by WindowClosed for the
        // same kind.
        assert!(matches!(
            events.first(),
            Some(Event::WindowOpened { kind: WindowKind::MythosAfterDraws })
        ));
        assert!(matches!(
            events.iter().find(|e| matches!(e, Event::WindowClosed { .. })),
            Some(Event::WindowClosed { kind: WindowKind::MythosAfterDraws })
        ));
    }

    #[test]
    fn run_window_continuation_for_unknown_kind_does_nothing() {
        let mut state = TestGame::default().build();
        let mut events = Vec::new();
        // AfterEnemyDefeated has no continuation. Calling it should
        // be a no-op (no events, no state change).
        run_window_continuation(
            &mut state,
            &mut events,
            WindowKind::AfterEnemyDefeated {
                enemy: EnemyId(1),
                by: None,
            },
        );
        assert!(events.is_empty());
    }
}
```

- [ ] **Step 3: Confirm compile failure**

```bash
cargo test -p game-core open_fast_window_tests 2>&1 | head -10
```

Expected: `cannot find function open_fast_window`, `cannot find function run_window_continuation`.

- [ ] **Step 4: Add the `run_window_continuation` helper**

In `crates/game-core/src/engine/dispatch.rs`:

```rust
/// Kind-aware continuation called when a window closes (whether
/// inline via [`open_fast_window`]'s auto-skip path or via the
/// [`close_reaction_window_at`] pop path). For
/// [`WindowKind::MythosAfterDraws`], runs [`mythos_phase_end`].
/// Other window kinds: no continuation (preserves existing
/// [`close_reaction_window_at`] behavior pre-#69).
///
/// Stub for the not-yet-implemented `mythos_phase_end` lands in
/// Task 9; this function dispatches to it from #69 onward.
fn run_window_continuation(state: &mut GameState, events: &mut Vec<Event>, kind: WindowKind) {
    match kind {
        WindowKind::MythosAfterDraws => mythos_phase_end(state, events),
        WindowKind::AfterEnemyDefeated { .. } | WindowKind::BetweenPhases { .. } => {}
    }
}
```

The `mythos_phase_end` call won't have a real body yet — that lands in Task 9. To keep this commit compilable AND behavior-preserving (in case future test code calls `run_window_continuation` directly for `MythosAfterDraws`), add a **functional no-op stub** at the bottom of the file:

```rust
/// Stub — real body lands in Task 9. Until then, behave like the
/// vanilla phase transition step_phase performs for non-driver
/// phases: emit PhaseEnded(Mythos), then call step_phase to advance
/// to Investigation. No tests currently reach this code path
/// (nothing pushes MythosAfterDraws onto the window stack until
/// Task 9 wires mythos_phase to do it), so the stub's only job is
/// to preserve correct semantics if anyone calls it.
fn mythos_phase_end(state: &mut GameState, events: &mut Vec<Event>) {
    events.push(Event::PhaseEnded { phase: Phase::Mythos });
    step_phase(state, events);
}
```

(This stub mirrors what `step_phase`'s pre-#69 behavior would have done. Task 9 replaces it with the real body that doesn't call `step_phase` directly because investigation_phase handles the transition.)

- [ ] **Step 5: Add the `open_fast_window` helper**

```rust
/// Open a printed Fast-play window of the given kind. Always emits
/// `Event::WindowOpened { kind }` for observability. Then either:
/// - Pushes the [`OpenWindow`] onto `state.open_windows` if pending
///   reaction triggers or any Fast play is eligible. The apply loop's
///   existing "pending reactions → AwaitingInput" path then surfaces
///   the wait at the dispatch tail.
/// - Or emits `Event::WindowClosed { kind }` immediately and runs
///   [`run_window_continuation`] inline. The window never lands on
///   `state.open_windows`.
///
/// Auto-skip is the common case for the synthetic fixture (no Fast
/// cards in any hand, no 0-cost Activated abilities on any in-play
/// card) and saves a UI round-trip when nobody can act.
pub(super) fn open_fast_window(
    state: &mut GameState,
    events: &mut Vec<Event>,
    kind: WindowKind,
) {
    events.push(Event::WindowOpened { kind });

    let pending_triggers = scan_pending_triggers(state, kind);
    let has_fast_eligible = any_fast_play_eligible(state);

    if pending_triggers.is_empty() && !has_fast_eligible {
        events.push(Event::WindowClosed { kind });
        run_window_continuation(state, events, kind);
        return;
    }

    state.open_windows.push(OpenWindow {
        kind,
        pending_triggers,
        fast_actors: FastActorScope::Any,
    });
}
```

- [ ] **Step 6: Retrofit `queue_reaction_window` to emit `Event::WindowOpened`**

In `crates/game-core/src/engine/dispatch.rs`, locate `fn queue_reaction_window` (around dispatch.rs:1425). Currently:

```rust
fn queue_reaction_window(state: &mut GameState, kind: WindowKind) {
    let pending_triggers = scan_pending_triggers(state, kind);
    if pending_triggers.is_empty() {
        return;
    }
    state.open_windows.push(OpenWindow {
        kind,
        pending_triggers,
        fast_actors: FastActorScope::Any,
    });
}
```

The signature takes only `(state, kind)` — no events buffer. Update both signature and callers to pass `events`:

```rust
fn queue_reaction_window(state: &mut GameState, events: &mut Vec<Event>, kind: WindowKind) {
    let pending_triggers = scan_pending_triggers(state, kind);
    if pending_triggers.is_empty() {
        return;
    }
    events.push(Event::WindowOpened { kind });
    state.open_windows.push(OpenWindow {
        kind,
        pending_triggers,
        fast_actors: FastActorScope::Any,
    });
}
```

Grep for callers and add `events` to each call site:

```bash
grep -n "queue_reaction_window(" crates/game-core/src/engine/dispatch.rs
```

(The existing call site at line ~2333 takes a state but no events — the handler that calls it (`damage_enemy` or similar) has an `events` parameter already, so threading `events` through is mechanical.)

- [ ] **Step 7: Wire `close_reaction_window_at` to call `run_window_continuation`**

In `close_reaction_window_at` (dispatch.rs:1808-ish), after the existing `events.push(Event::WindowClosed { kind })` line and before the existing skill-test-resume block, add:

```rust
    events.push(Event::WindowClosed { kind });
    run_window_continuation(state, events, kind);

    // ... existing skill-test resume logic ...
```

(The skill-test resume runs after the continuation. For `MythosAfterDraws`, `run_window_continuation` calls `mythos_phase_end` which calls `step_phase` which calls `investigation_phase` — none of which interact with `in_flight_skill_test`. So the ordering is safe.)

- [ ] **Step 8: Verify the new and updated tests pass**

```bash
RUSTFLAGS="-D warnings" cargo test -p game-core open_fast_window_tests 2>&1 | tail -10
RUSTFLAGS="-D warnings" cargo test -p game-core AfterEnemyDefeated 2>&1 | tail -10
RUSTFLAGS="-D warnings" cargo test -p game-core --all-features 2>&1 | tail -20
```

Expected: all green. If any `AfterEnemyDefeated` test fails because of the new `Event::WindowOpened` in the sequence, update the test's expected event list.

- [ ] **Step 9: Full game-core gauntlet**

```bash
cargo clippy -p game-core --all-targets --all-features -- -D warnings 2>&1 | tail -10
RUSTDOCFLAGS="-D warnings" cargo doc -p game-core --no-deps --all-features 2>&1 | tail -10
```

- [ ] **Step 10: Commit**

```bash
git add crates/game-core/src/engine/dispatch.rs
git commit -m "$(cat <<'EOF'
engine: add open_fast_window + run_window_continuation; retrofit reaction-window emit

Three coordinated changes:

- open_fast_window: always emits WindowOpened, auto-skips by emitting
  WindowClosed + running the kind continuation inline when no
  reactions queue AND no Fast play is eligible. Pushes onto
  state.open_windows otherwise.
- run_window_continuation: kind-aware match called from
  close_reaction_window_at (and from open_fast_window's auto-skip
  path) after WindowClosed emits. Currently dispatches MythosAfterDraws
  to mythos_phase_end (stubbed; real body lands in a follow-up task).
- queue_reaction_window: now emits Event::WindowOpened before pushing
  to state.open_windows so reaction-window observability is symmetric
  with Fast-window observability. Existing AfterEnemyDefeated tests
  updated to include the new event in their sequences.

Refs #69.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 8: Add `investigation_phase` skeleton driver + refactor `step_phase`, `start_scenario`, `end_turn`

**Files:**
- Modify: `crates/game-core/src/engine/dispatch.rs`

The phase-driver pattern lands here. `step_phase` becomes the dispatcher; `investigation_phase` owns step 2.1's PhaseStarted emit + step 2.2's rotate; `start_scenario` skips Mythos entirely in round 1 (no Mythos boundary events); `end_turn` drops the trailing rotate.

- [ ] **Step 1: Update existing scenario-start tests for the dropped phantom Mythos emits**

The existing `start_scenario` emits `Event::PhaseStarted { phase: Phase::Mythos }` (at dispatch.rs:658) and the subsequent `step_phase(Mythos→Investigation)` emits `Event::PhaseEnded { phase: Phase::Mythos }`. After this task, neither fires for round-1 scenario start.

Grep for affected tests:

```bash
grep -rn "PhaseStarted.*Mythos\|PhaseEnded.*Mythos" crates/game-core/src/engine/ | head -20
```

Inspect each match. Any test that asserts a `Phase::Mythos`-bearing PhaseStarted / PhaseEnded event near `StartScenario` needs the assertion removed or updated. Tests asserting the post-StartScenario state (`state.phase == Phase::Investigation`, `state.active_investigator == Some(lead)`, `state.round == 1`) stay unchanged.

- [ ] **Step 2: Write failing tests for `investigation_phase` and the new step_phase shape**

In `engine/dispatch.rs`'s test section:

```rust
#[cfg(test)]
mod investigation_phase_tests {
    use super::*;
    use crate::test_support::{TestGame, test_investigator};

    #[test]
    fn investigation_phase_emits_phase_started_and_rotates_to_lead() {
        let mut state = TestGame::default()
            .with_investigator(test_investigator(1))
            .with_investigator(test_investigator(2))
            .with_phase(Phase::Mythos)
            .build();
        // Set turn_order so we can verify "lead first."
        state.turn_order = vec![InvestigatorId(1), InvestigatorId(2)];
        state.active_investigator = None;

        let mut events = Vec::new();
        investigation_phase(&mut state, &mut events);

        assert_eq!(state.active_investigator, Some(InvestigatorId(1)));
        let phase_started_idx = events
            .iter()
            .position(|e| matches!(e, Event::PhaseStarted { phase: Phase::Investigation }))
            .expect("PhaseStarted(Investigation) emitted");
        let turn_started_idx = events
            .iter()
            .position(|e| matches!(e, Event::TurnStarted { .. }))
            .expect("TurnStarted emitted");
        assert!(phase_started_idx < turn_started_idx, "PhaseStarted precedes TurnStarted");
    }

    #[test]
    fn investigation_phase_with_empty_turn_order_is_noop_rotate() {
        let mut state = TestGame::default().with_phase(Phase::Mythos).build();
        // No investigators; no turn order.
        state.turn_order.clear();
        state.active_investigator = None;

        let mut events = Vec::new();
        investigation_phase(&mut state, &mut events);

        assert_eq!(state.active_investigator, None);
        assert!(events.iter().any(|e| matches!(e, Event::PhaseStarted { phase: Phase::Investigation })));
        assert!(!events.iter().any(|e| matches!(e, Event::TurnStarted { .. })));
    }
}
```

- [ ] **Step 3: Confirm compile failure**

```bash
cargo test -p game-core investigation_phase_tests 2>&1 | head -10
```

Expected: `cannot find function investigation_phase`.

- [ ] **Step 4: Add the `investigation_phase` driver**

In `crates/game-core/src/engine/dispatch.rs`:

```rust
/// Entered by [`step_phase`] on any-to-Investigation transition.
/// Owns the `PhaseStarted(Investigation)` emit (Rules Reference
/// p.24 step 2.1) and the initial rotation to the active
/// investigator (step 2.2).
///
/// **Rotation policy (Phase 4):** lead-first by default.
/// Rules Reference p.24 step 2.2: "The investigators may take their
/// turns in any order. The investigators choose among themselves
/// who…will take this turn." Phase 4 hardcodes lead-first as the
/// table convention; the future full Investigation driver PR adds
/// a player-pick action within an opened post-2.1 window.
fn investigation_phase(state: &mut GameState, events: &mut Vec<Event>) {
    // 2.1 Investigation phase begins.
    events.push(Event::PhaseStarted {
        phase: Phase::Investigation,
    });

    // [Post-2.1 player window not opened in #69 — the future
    //  Investigation full driver PR adds open_fast_window here.]

    // 2.2 Next investigator's turn begins. (First turn of the phase.)
    if let Some(&first) = state.turn_order.first() {
        rotate_to_active(state, events, first);
    }
}
```

- [ ] **Step 5: Refactor `step_phase` to dispatch into drivers + suppress driver-owned emits**

Replace the existing `fn step_phase` body. The new shape:

```rust
fn step_phase(state: &mut GameState, events: &mut Vec<Event>) {
    let from = state.phase;
    let to = from.next();

    // PhaseEnded: suppressed when the from-phase's *_end helper owns
    // the emit. Phase 4: only Mythos has an end helper.
    if from != Phase::Mythos {
        events.push(Event::PhaseEnded { phase: from });
    }

    state.phase = to;
    // Round-bump invariant: bump when entering Mythos. Unchanged.
    if to == Phase::Mythos {
        state.round = state.round.saturating_add(1);
    }

    // Dispatch to phase driver if one exists; otherwise emit
    // PhaseStarted directly.
    match to {
        Phase::Mythos if from != Phase::Mythos => mythos_phase(state, events),
        Phase::Investigation if from != Phase::Investigation => investigation_phase(state, events),
        _ => events.push(Event::PhaseStarted { phase: to }),
    }
}
```

The `mythos_phase` call won't have its full body yet — that lands in Task 9. To keep this commit compilable AND behavior-preserving (existing `end_turn` tests drive the chain through Mythos and assert on the events that fired), add a **functional no-op stub** at the bottom of the file that emits exactly what `step_phase` used to emit for the Mythos arm:

```rust
/// Stub — real body lands in Task 9. Until then, emit
/// PhaseStarted(Mythos) only (preserving step_phase's pre-#69
/// behavior for the Mythos transition). The full body — 1.1 marker
/// + 1.2/1.3 TODO stubs + 1.4 cursor seed + degenerate
/// open_fast_window fallback — lands in Task 9 along with the
/// end_turn refactor that pauses on mythos_draw_pending.
fn mythos_phase(_state: &mut GameState, events: &mut Vec<Event>) {
    events.push(Event::PhaseStarted { phase: Phase::Mythos });
}
```

- [ ] **Step 6: Refactor `start_scenario` to skip Mythos entirely**

In `start_scenario` (dispatch.rs:641-685), replace the existing structure with:

```rust
fn start_scenario(state: &mut GameState, events: &mut Vec<Event>) -> EngineOutcome {
    if state.round != 0 {
        return EngineOutcome::Rejected {
            reason: "StartScenario applied to a state that is already in progress".into(),
        };
    }
    // Round 1: scenario starts directly in Investigation phase —
    // Mythos is skipped entirely per Rules Reference p.24 "During
    // the first round of the game, skip the mythos phase." No
    // PhaseStarted(Mythos) / PhaseEnded(Mythos) fire — the phase
    // doesn't happen.
    state.round = 1;
    state.phase = Phase::Investigation;
    events.push(Event::ScenarioStarted);

    // For each investigator (sorted by id for determinism), shuffle
    // their deck and deal an initial hand of up to 5.
    let inv_ids: Vec<InvestigatorId> = state.investigators.keys().copied().collect();
    for inv_id in inv_ids {
        shuffle_player_deck(state, events, inv_id);
        draw_cards(state, events, inv_id, INITIAL_HAND_SIZE);
    }

    // Open the mulligan window. Each investigator may now submit a
    // single `PlayerAction::Mulligan` to redraw a subset of their
    // starting hand.
    state.mulligan_window = true;

    // investigation_phase emits PhaseStarted(Investigation) + rotates to lead.
    investigation_phase(state, events);

    EngineOutcome::Done
}
```

- [ ] **Step 7: Refactor `end_turn` to drop the trailing rotate (but keep the chain)**

In `end_turn` (dispatch.rs:687-746), the final block currently is:

```rust
        state.active_investigator = None;
        step_phase(state, events); // Investigation → Enemy
        step_phase(state, events); // Enemy → Upkeep
        step_phase(state, events); // Upkeep → Mythos (round bumps)
        step_phase(state, events); // Mythos → Investigation
        if let Some(&first) = state.turn_order.first() {
            rotate_to_active(state, events, first);
        }
```

Replace ONLY the trailing rotate (Task 8 doesn't yet touch the chain itself — the `mythos_draw_pending` pause logic depends on the real `mythos_phase` body landing in Task 9):

```rust
        state.active_investigator = None;
        step_phase(state, events); // Investigation → Enemy
        step_phase(state, events); // Enemy → Upkeep
        step_phase(state, events); // Upkeep → Mythos (round bumps; mythos_phase stub fires)
        step_phase(state, events); // Mythos → Investigation (investigation_phase rotates)
        // Trailing rotate_to_active removed — investigation_phase
        // owns step 2.2 rotation. Task 9 replaces the
        // `step_phase(Mythos→Investigation)` line with a pause-on-
        // mythos_draw_pending check once the real mythos_phase body
        // sets the cursor.
```

- [ ] **Step 8: Verify new + updated tests pass**

```bash
RUSTFLAGS="-D warnings" cargo test -p game-core investigation_phase_tests start_scenario end_turn 2>&1 | tail -20
```

Expected: all green. The investigation_phase_tests pass; start_scenario tests pass with the new no-Mythos-emits behavior; end_turn tests pass.

- [ ] **Step 9: Full workspace gauntlet**

```bash
RUSTFLAGS="-D warnings" cargo test --workspace --all-features 2>&1 | tail -20
cargo clippy --all-targets --all-features -- -D warnings 2>&1 | tail -10
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features 2>&1 | tail -10
```

- [ ] **Step 10: Commit**

```bash
git add crates/game-core/src/engine/dispatch.rs
git commit -m "$(cat <<'EOF'
engine: phase-driver pattern + investigation_phase skeleton

Each phase has a driver function (owns PhaseStarted as step N.1).
step_phase dispatches into drivers and suppresses driver-owned
emits. #69 lands Mythos (driver + end helper, in following task)
and Investigation (skeleton: emit + rotate-to-lead per Rules
Reference p.24 step 2.2). #70 / #71 / future full Investigation
driver follow the same pattern.

start_scenario skips Mythos entirely in round 1 — no Mythos boundary
events fire, because per Rules Reference p.24 the phase is skipped
(not run empty). Existing tests asserting on the phantom emits
updated.

end_turn's trailing rotate_to_active is dropped (investigation_phase
owns step 2.2 rotation now). The chain itself still ticks fully
through Mythos→Investigation in one apply because mythos_phase is a
behavior-preserving stub (emits PhaseStarted only); Task 9 lands the
real mythos_phase body + the end_turn pause-on-mythos_draw_pending
check that makes the chain stop for player draws.

mythos_phase and mythos_phase_end exist as functional no-op stubs at
this point (preserve old behavior, do not panic); Task 9 swaps them
for the real bodies.

Refs #69.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 9: Add `mythos_phase` driver + `mythos_phase_end` helper (1.1 / 1.5 markers + TODO stubs for 1.2 / 1.3)

**Files:**
- Modify: `crates/game-core/src/engine/dispatch.rs`

The Mythos phase driver with its sub-step structure: 1.1 PhaseStarted emit, 1.2 / 1.3 as TODO call sites for #73, 1.4 seeds the draw cursor. `mythos_phase_end` emits PhaseEnded(Mythos) as 1.5 and transitions to Investigation.

- [ ] **Step 1: Write failing tests for the driver shape**

```rust
#[cfg(test)]
mod mythos_phase_tests {
    use super::*;
    use crate::test_support::{TestGame, test_investigator};

    #[test]
    fn mythos_phase_emits_phase_started_and_seeds_draw_pending() {
        let mut state = TestGame::default()
            .with_investigator(test_investigator(1))
            .with_investigator(test_investigator(2))
            .with_phase(Phase::Mythos)
            .build();
        state.turn_order = vec![InvestigatorId(1), InvestigatorId(2)];
        state.mythos_draw_pending = None;
        let mut events = Vec::new();

        mythos_phase(&mut state, &mut events);

        assert_eq!(state.mythos_draw_pending, Some(InvestigatorId(1)));
        assert!(events.iter().any(|e| matches!(
            e,
            Event::PhaseStarted { phase: Phase::Mythos }
        )));
    }

    #[test]
    fn mythos_phase_with_empty_turn_order_opens_after_draws_window_inline() {
        let mut state = TestGame::default().with_phase(Phase::Mythos).build();
        state.turn_order.clear();
        state.mythos_draw_pending = None;
        let mut events = Vec::new();

        mythos_phase(&mut state, &mut events);

        // No drawers → open_fast_window runs for MythosAfterDraws,
        // which auto-skips (no Fast eligibility), runs continuation
        // (mythos_phase_end), which steps into Investigation.
        assert_eq!(state.mythos_draw_pending, None);
        assert_eq!(state.phase, Phase::Investigation);
        assert!(events.iter().any(|e| matches!(
            e,
            Event::WindowOpened { kind: WindowKind::MythosAfterDraws }
        )));
        assert!(events.iter().any(|e| matches!(
            e,
            Event::WindowClosed { kind: WindowKind::MythosAfterDraws }
        )));
        assert!(events.iter().any(|e| matches!(
            e,
            Event::PhaseEnded { phase: Phase::Mythos }
        )));
        assert!(events.iter().any(|e| matches!(
            e,
            Event::PhaseStarted { phase: Phase::Investigation }
        )));
    }

    #[test]
    fn mythos_phase_end_emits_phase_ended_and_steps_to_investigation() {
        let mut state = TestGame::default()
            .with_investigator(test_investigator(1))
            .with_phase(Phase::Mythos)
            .build();
        state.turn_order = vec![InvestigatorId(1)];
        let mut events = Vec::new();

        mythos_phase_end(&mut state, &mut events);

        assert_eq!(state.phase, Phase::Investigation);
        assert!(events.iter().any(|e| matches!(
            e,
            Event::PhaseEnded { phase: Phase::Mythos }
        )));
        assert!(events.iter().any(|e| matches!(
            e,
            Event::PhaseStarted { phase: Phase::Investigation }
        )));
    }
}
```

- [ ] **Step 2: Replace the `mythos_phase` and `mythos_phase_end` stubs with real bodies**

Replace the temporary stubs added in earlier tasks:

```rust
/// Entered by [`step_phase`] on the Upkeep→Mythos transition. Lays
/// out the Rules Reference p.24 sub-steps as discrete named call
/// sites so the rule structure is grep-able and #73 / future-peril-PR
/// fills in TODO bodies without changing the driver shape.
fn mythos_phase(state: &mut GameState, events: &mut Vec<Event>) {
    // 1.1 Round begins. Mythos phase begins.
    //     `step_phase` has already emitted PhaseEnded(Upkeep),
    //     updated state.phase to Mythos, and bumped the round
    //     counter. The PhaseStarted(Mythos) emit lives HERE rather
    //     than in step_phase so step 1.1 has explicit ownership in
    //     the driver — Rules Reference p.24: "This step formalizes
    //     the beginning of the mythos phase."
    events.push(Event::PhaseStarted {
        phase: Phase::Mythos,
    });

    // 1.2 Place 1 doom on the current agenda.
    place_doom_on_agenda(state, events);

    // 1.3 Check doom threshold.
    check_doom_threshold(state, events);

    // 1.4 Each investigator draws 1 encounter card.
    //     Seed the cursor; the actual draws are player-driven via
    //     PlayerAction::DrawEncounterCard. The dispatch handler
    //     advances the cursor after each chain.
    state.mythos_draw_pending = state.turn_order.first().copied();
    if state.mythos_draw_pending.is_none() {
        // Degenerate state — no investigators to draw. Open the
        // post-1.4 window immediately; open_fast_window's auto-skip
        // path triggers because nothing is eligible, runs the
        // MythosAfterDraws continuation (mythos_phase_end), which
        // transitions to Investigation. All in this same apply.
        open_fast_window(state, events, WindowKind::MythosAfterDraws);
    }
}

fn place_doom_on_agenda(_state: &mut GameState, _events: &mut Vec<Event>) {
    // TODO(#73): place 1 doom on the current agenda per Rules
    //            Reference p.24 step 1.2. Currently no agenda state
    //            exists; #73 lands the agenda struct + doom counter
    //            + this body.
}

fn check_doom_threshold(_state: &mut GameState, _events: &mut Vec<Event>) {
    // TODO(#73): compare total doom in play to current agenda's
    //            threshold; advance if met. Rules Reference p.24
    //            step 1.3. Same reason as above: no agenda state
    //            yet.
}

/// Called after the post-1.4 window closes. Emits 1.5's
/// `PhaseEnded(Mythos)` marker, then transitions to Investigation.
/// Rotation is owned by `investigation_phase` (step 2.2), not by
/// `mythos_phase_end`. Invoked from `close_reaction_window_at`'s
/// kind-aware tail when a `MythosAfterDraws` window pops, and from
/// `open_fast_window`'s auto-skip path inline.
fn mythos_phase_end(state: &mut GameState, events: &mut Vec<Event>) {
    // 1.5 Mythos phase ends.
    //     The PhaseEnded(Mythos) emit lives HERE rather than in
    //     step_phase so step 1.5 has explicit ownership in the
    //     driver — mirror of step 1.1's PhaseStarted ownership in
    //     mythos_phase. Rules Reference p.24: "This step formalizes
    //     the end of the mythos phase."
    events.push(Event::PhaseEnded {
        phase: Phase::Mythos,
    });
    step_phase(state, events); // Mythos → Investigation; calls investigation_phase
}
```

- [ ] **Step 3: Update `end_turn` to pause on `mythos_draw_pending`**

In `end_turn`, the chain (post-Task 8) currently is:

```rust
        step_phase(state, events); // Investigation → Enemy
        step_phase(state, events); // Enemy → Upkeep
        step_phase(state, events); // Upkeep → Mythos
        step_phase(state, events); // Mythos → Investigation
```

Replace the last two lines so the chain pauses when the real `mythos_phase` body has seeded `mythos_draw_pending`:

```rust
        step_phase(state, events); // Investigation → Enemy
        step_phase(state, events); // Enemy → Upkeep
        step_phase(state, events); // Upkeep → Mythos (round bumps + mythos_phase runs)
        if state.mythos_draw_pending.is_some() {
            // Chain pauses here; the player's DrawEncounterCard
            // actions advance Mythos. mythos_phase_end (triggered
            // later via close_reaction_window_at's continuation
            // dispatch) handles the transition into Investigation.
            return EngineOutcome::Done;
        }
        // Degenerate state (no investigators in turn_order):
        // mythos_phase opened+closed MythosAfterDraws inline, which
        // fired mythos_phase_end as the continuation; that emitted
        // PhaseEnded(Mythos) and stepped into Investigation via
        // investigation_phase. Nothing left to do.
```

- [ ] **Step 4: Verify the new tests pass**

```bash
RUSTFLAGS="-D warnings" cargo test -p game-core mythos_phase_tests 2>&1 | tail -10
```

Expected: 3 PASS.

- [ ] **Step 5: Verify existing `end_turn` tests pass**

The chain now pauses at Mythos when investigators are in turn_order. Tests that assumed the chain auto-continues through to Investigation may need the player to drive `DrawEncounterCard` for the chain to complete. Inspect:

```bash
RUSTFLAGS="-D warnings" cargo test -p game-core end_turn 2>&1 | tail -30
```

For any failing test, either: (a) update its assertions to expect `state.phase == Mythos` + `state.mythos_draw_pending == Some(lead)` after `end_turn`, or (b) extend the test to also drive `DrawEncounterCard` to complete the round. Pick (a) where the test is about end-of-turn semantics; pick (b) where it's about round-cycle semantics.

- [ ] **Step 6: Full workspace gauntlet**

```bash
RUSTFLAGS="-D warnings" cargo test --workspace --all-features 2>&1 | tail -20
cargo clippy --all-targets --all-features -- -D warnings 2>&1 | tail -10
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features 2>&1 | tail -10
```

- [ ] **Step 7: Commit**

```bash
git add crates/game-core/src/engine/dispatch.rs
git commit -m "$(cat <<'EOF'
engine: mythos_phase + mythos_phase_end drivers with sub-step structure

Lands the Mythos phase driver per Rules Reference p.24:

- 1.1: PhaseStarted(Mythos) emit owned by mythos_phase.
- 1.2 / 1.3: place_doom_on_agenda / check_doom_threshold called as
  named-but-empty functions with TODO(#73) bodies, so the rule
  structure is grep-able and #73 fills in the bodies without
  changing the driver shape.
- 1.4: seed state.mythos_draw_pending with the first turn-order
  investigator. Player-driven via PlayerAction::DrawEncounterCard
  (lands in a later task). Degenerate empty-turn-order case opens
  MythosAfterDraws inline, which auto-skips and runs
  mythos_phase_end as the continuation.
- 1.5: PhaseEnded(Mythos) emit owned by mythos_phase_end, then
  step_phase transitions to Investigation (whose driver rotates).

mythos_phase is invoked by step_phase on Upkeep→Mythos.
mythos_phase_end is invoked by close_reaction_window_at's
kind-aware tail when MythosAfterDraws pops, and by open_fast_window's
auto-skip path inline.

Refs #69.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 10: Extract `resolve_encounter_card` shared helper from `encounter_card_revealed`

**Files:**
- Modify: `crates/game-core/src/engine/dispatch.rs`

Lift the post-draw resolution prefix (emit `Event::CardRevealed`, dispatch on CardType: treachery / enemy / reject) from `encounter_card_revealed` into a shared helper. Both `encounter_card_revealed` (the existing `EngineRecord::EncounterCardRevealed` path) and `mythos_draw_for` (landing in the next task) call it.

- [ ] **Step 1: Identify the lift boundary**

Open `fn encounter_card_revealed` (dispatch.rs:257). Identify:

- Prefix that stays in `encounter_card_revealed`: registry check, `draw_encounter_top` call, metadata lookup.
- Suffix that moves to `resolve_encounter_card`: everything from `Event::CardRevealed` emit onward (the existing match on `metadata.card_type` for Treachery / Enemy / other).

- [ ] **Step 2: Write a test confirming behavior preservation**

The simplest behavior-preservation test is the existing `encounter_card_revealed_tests` module — if it stays green after the refactor, the lift didn't drift. No new test needed for this step; the existing ones serve as the regression net.

- [ ] **Step 3: Add the `resolve_encounter_card` helper**

In `crates/game-core/src/engine/dispatch.rs`:

```rust
/// Shared post-draw resolution helper. Resolves the per-card 5-step
/// sub-sequence's steps 3 (Revelation) and 4 (enemy spawn) for an
/// already-drawn encounter card. Called by `encounter_card_revealed`
/// (the EngineRecord::EncounterCardRevealed path) and by
/// `mythos_draw_for` (Mythos 1.4 player-driven draws).
///
/// Body: emits Event::CardRevealed, then dispatches on
/// metadata.card_type — treachery → run Revelation abilities →
/// push card to encounter_discard + emit Event::CardDiscarded;
/// enemy → call spawn_enemy; any other type → return Rejected.
///
/// **Mid-resolution caveat:** Event::CardRevealed emits before
/// Revelation runs (Before-timing reactions need that ordering,
/// per #126's design decision). The apply loop's events.clear() on
/// Rejected still wipes the event stream on rejection.
fn resolve_encounter_card(
    state: &mut GameState,
    events: &mut Vec<Event>,
    investigator: InvestigatorId,
    code: CardCode,
    metadata: &CardMetadata,
) -> EngineOutcome {
    // (Body is the existing post-draw block from encounter_card_revealed,
    //  lifted verbatim. Implementer: copy lines from the existing
    //  encounter_card_revealed body starting at the Event::CardRevealed
    //  emit through the final type-match arms; replace any
    //  return-from-outer-function paths with returns from this helper.)
    todo!("lift body from encounter_card_revealed")
}
```

Implementer fills in the `todo!()` by copying the existing block verbatim.

- [ ] **Step 4: Replace the prefix in `encounter_card_revealed` to call the helper**

```rust
fn encounter_card_revealed(
    state: &mut GameState,
    events: &mut Vec<Event>,
    investigator: InvestigatorId,
) -> EngineOutcome {
    let Some(reg) = card_registry::current() else {
        return EngineOutcome::Rejected {
            reason: "EncounterCardRevealed: no card registry installed".into(),
        };
    };
    let Some(code) = draw_encounter_top(state, events) else {
        return EngineOutcome::Rejected {
            reason: "EncounterCardRevealed: encounter deck and discard both empty".into(),
        };
    };
    let Some(metadata) = (reg.metadata_for)(&code) else {
        return EngineOutcome::Rejected {
            reason: format!("EncounterCardRevealed: unknown card code: {code:?}").into(),
        };
    };
    resolve_encounter_card(state, events, investigator, code, metadata)
}
```

- [ ] **Step 5: Verify existing `encounter_card_revealed` tests still pass**

```bash
RUSTFLAGS="-D warnings" cargo test -p game-core encounter_card_revealed 2>&1 | tail -10
RUSTFLAGS="-p cards" cargo test -p cards 2>&1 | tail -10
```

(Adjust the second command — the intent is to run integration tests in `crates/cards/tests/` that depend on encounter resolution.)

Expected: all PASS.

- [ ] **Step 6: Full workspace gauntlet**

```bash
RUSTFLAGS="-D warnings" cargo test --workspace --all-features 2>&1 | tail -20
cargo clippy --all-targets --all-features -- -D warnings 2>&1 | tail -10
```

- [ ] **Step 7: Commit**

```bash
git add crates/game-core/src/engine/dispatch.rs
git commit -m "$(cat <<'EOF'
engine: extract resolve_encounter_card shared helper

Lifts the post-draw resolution prefix (Event::CardRevealed emit +
type-based dispatch to Revelation / spawn / reject) from
encounter_card_revealed into a separate resolve_encounter_card
helper. Pure refactor.

Reusable from the upcoming mythos_draw_for handler so the Mythos
draw loop and the existing EngineRecord::EncounterCardRevealed
path share resolution logic — they only differ in how the card
is drawn (player action vs. scenario effect).

Refs #69.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 11: Add `peril_check` stub + `mythos_draw_for` per-card 5-step + surge loop + `MAX_SURGE_CHAIN`

**Files:**
- Modify: `crates/game-core/src/engine/dispatch.rs`

The per-investigator Mythos draw chain. Walks the 5-step sub-sequence per card; loops on surge; advances `mythos_draw_pending` to the next-in-order drawer when the chain ends; opens `MythosAfterDraws` after the last drawer.

- [ ] **Step 1: Add the `MAX_SURGE_CHAIN` constant + `peril_check` stub**

In `crates/game-core/src/engine/dispatch.rs`:

```rust
/// Hard cap on a single Mythos draw chain. Real scenarios surge ≤2
/// in a chain; the cap exists purely to guarantee termination on
/// malformed encounter decks (e.g. a deck small enough for surge to
/// loop via the Rules Reference p.10 reshuffle). `unreachable!`-class
/// — never reached in legitimate play.
const MAX_SURGE_CHAIN: usize = 64;

/// Per-card 5-step sub-sequence's step 2 (Rules Reference p.24 1.4
/// step 2): peril keyword check. When `is_peril` is true, the
/// drawing investigator's conferral and other players' interactions
/// (playing cards, triggering abilities, committing to skill tests)
/// are restricted during resolution. **Enforcement not yet wired**
/// — no machinery exists for cross-investigator commit blocking,
/// and Phase 4 is single-investigator-focused. The function call
/// site exists so the rule step is grep-able and the future
/// peril-enforcement PR plugs in here without changing the driver
/// shape.
fn peril_check(
    _state: &mut GameState,
    _events: &mut Vec<Event>,
    _code: &CardCode,
    _investigator: InvestigatorId,
    _is_peril: bool,
) {
    // TODO(future-peril-PR): if `is_peril`, install a temporary
    //   restriction on `state` such that other investigators cannot
    //   (a) play cards, (b) trigger abilities, or (c) commit to the
    //   drawing investigator's skill tests until this card's
    //   resolution completes.
}
```

- [ ] **Step 2: Write failing tests for `mythos_draw_for`**

```rust
#[cfg(test)]
mod mythos_draw_for_tests {
    use super::*;
    use crate::test_support::{TestGame, test_investigator};

    #[test]
    fn rejects_when_registry_not_installed() {
        let mut state = TestGame::default()
            .with_investigator(test_investigator(1))
            .with_phase(Phase::Mythos)
            .build();
        state.turn_order = vec![InvestigatorId(1)];
        state.mythos_draw_pending = Some(InvestigatorId(1));
        let mut events = Vec::new();
        let outcome = mythos_draw_for(&mut state, &mut events, InvestigatorId(1));
        assert!(matches!(
            outcome,
            EngineOutcome::Rejected { reason } if reason.contains("registry")
        ));
    }

    // The positive paths (treachery resolves, enemy spawns, surge
    // chains) are exercised in the integration tests in
    // crates/scenarios/tests/mythos_phase.rs where TEST_REGISTRY is
    // installed.
}
```

- [ ] **Step 3: Confirm compile failure**

```bash
cargo test -p game-core mythos_draw_for_tests 2>&1 | head -10
```

Expected: `cannot find function mythos_draw_for`.

- [ ] **Step 4: Implement `mythos_draw_for`**

In `crates/game-core/src/engine/dispatch.rs`:

```rust
/// Resolves one investigator's full Mythos encounter draw — the
/// per-card 5-step sub-sequence from Rules Reference p.24, with
/// surge re-draws looping until the chain ends.
///
/// Called by the `PlayerAction::DrawEncounterCard` handler with the
/// pending-drawer's id. Returns Done on success (chain completed,
/// `mythos_draw_pending` advanced).
fn mythos_draw_for(
    state: &mut GameState,
    events: &mut Vec<Event>,
    investigator: InvestigatorId,
) -> EngineOutcome {
    let Some(reg) = crate::card_registry::current() else {
        return EngineOutcome::Rejected {
            reason: "DrawEncounterCard: no card registry installed".into(),
        };
    };

    let mut chain_count: usize = 0;
    loop {
        chain_count += 1;
        if chain_count > MAX_SURGE_CHAIN {
            unreachable!(
                "Mythos draw chain exceeded MAX_SURGE_CHAIN ({}) for \
                 investigator {:?}. Indicates either an infinite reshuffle \
                 loop (Rules Reference p.18: treachery discard precedes surge \
                 re-draw, so a surging treachery in a too-small deck cycles \
                 via the p.10 reshuffle path) or a malformed scenario encounter \
                 deck. Real scenarios don't surge >{} cards in one chain.",
                MAX_SURGE_CHAIN, investigator, MAX_SURGE_CHAIN,
            );
        }

        // Step 1: Draw the card from the encounter deck.
        let Some(code) = draw_encounter_top(state, events) else {
            if chain_count == 1 {
                return EngineOutcome::Rejected {
                    reason: "DrawEncounterCard: encounter deck and discard both empty".into(),
                };
            }
            unreachable!(
                "Mythos draw chain hit empty encounter deck AND empty discard for \
                 investigator {:?} at chain position {}. Indicates a malformed \
                 scenario where surging enemies exhausted the encounter universe \
                 within one chain (enemies spawn to play, not discard, so p.10 \
                 reshuffle has nothing to pull).",
                investigator, chain_count,
            );
        };

        let Some(metadata) = (reg.metadata_for)(&code) else {
            return EngineOutcome::Rejected {
                reason: format!("DrawEncounterCard: unknown card code: {code:?}").into(),
            };
        };

        // Step 2: Check for the peril keyword on the drawn card.
        peril_check(state, events, &code, investigator, metadata.peril);

        // Step 3 + 4: Resolve revelation, then enemy-spawn if applicable.
        let outcome = resolve_encounter_card(state, events, investigator, code.clone(), metadata);
        if !matches!(outcome, EngineOutcome::Done) {
            return outcome;
        }

        // Step 5: If the drawn card has the surge keyword, loop.
        if !metadata.surge {
            break;
        }
    }

    // Chain complete — advance the cursor.
    advance_mythos_draw_pending(state, events);
    EngineOutcome::Done
}

/// Advance `state.mythos_draw_pending` after a completed chain. If
/// a next investigator exists in turn order, set to that id.
/// Otherwise set to None and open the post-1.4 window.
fn advance_mythos_draw_pending(state: &mut GameState, events: &mut Vec<Event>) {
    let current = state
        .mythos_draw_pending
        .expect("advance_mythos_draw_pending called only after a successful chain");
    let next = state
        .turn_order
        .iter()
        .position(|id| *id == current)
        .and_then(|idx| state.turn_order.get(idx + 1).copied());

    state.mythos_draw_pending = next;
    if next.is_none() {
        open_fast_window(state, events, WindowKind::MythosAfterDraws);
    }
}
```

- [ ] **Step 5: Verify negative test passes**

```bash
cargo test -p game-core mythos_draw_for_tests 2>&1 | tail -10
```

Expected: 1 PASS.

- [ ] **Step 6: Full game-core gauntlet**

```bash
RUSTFLAGS="-D warnings" cargo test -p game-core --all-features 2>&1 | tail -10
cargo clippy -p game-core --all-targets --all-features -- -D warnings 2>&1 | tail -10
RUSTDOCFLAGS="-D warnings" cargo doc -p game-core --no-deps --all-features 2>&1 | tail -10
```

- [ ] **Step 7: Commit**

```bash
git add crates/game-core/src/engine/dispatch.rs
git commit -m "$(cat <<'EOF'
engine: mythos_draw_for per-card 5-step + surge loop

Lands the per-investigator Mythos draw chain. Walks the Rules
Reference p.24 1.4 sub-sequence (draw, peril check, Revelation,
enemy spawn, surge re-draw) for each card in the chain; loops on
surge until the chain ends. Advances mythos_draw_pending to the
next-in-turn-order drawer or opens MythosAfterDraws after the last
drawer.

MAX_SURGE_CHAIN = 64 cap + two unreachable! sites guard against
scenario-data malformation (infinite reshuffle loop from surging
treacheries in a too-small deck; mid-chain empty-deck-and-discard
from surging enemies exhausting the encounter universe). Both are
scenario-build-time bugs, never reached in legitimate play.

peril_check is a TODO stub — the conferral restriction requires
cross-investigator commit blocking machinery that doesn't exist
yet.

Refs #69.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 12: Add `PlayerAction::DrawEncounterCard` + dispatch handler

**Files:**
- Modify: `crates/game-core/src/action.rs`
- Modify: `crates/game-core/src/engine/dispatch.rs`
- Modify: `crates/game-core/src/engine/mod.rs`

The top-level player action that exposes the Mythos draw chain. Validates phase + cursor; delegates to `mythos_draw_for`.

- [ ] **Step 1: Write failing tests for the handler**

```rust
#[cfg(test)]
mod draw_encounter_card_tests {
    use super::*;
    use crate::test_support::{TestGame, test_investigator};

    #[test]
    fn rejects_outside_mythos_phase() {
        let mut state = TestGame::default()
            .with_investigator(test_investigator(1))
            .with_phase(Phase::Investigation)
            .build();
        state.mythos_draw_pending = Some(InvestigatorId(1));
        let mut events = Vec::new();
        let outcome = draw_encounter_card(&mut state, &mut events, InvestigatorId(1));
        assert!(matches!(
            outcome,
            EngineOutcome::Rejected { reason } if reason.contains("only valid during Mythos")
        ));
    }

    #[test]
    fn rejects_when_no_draw_pending() {
        let mut state = TestGame::default()
            .with_investigator(test_investigator(1))
            .with_phase(Phase::Mythos)
            .build();
        state.mythos_draw_pending = None;
        let mut events = Vec::new();
        let outcome = draw_encounter_card(&mut state, &mut events, InvestigatorId(1));
        assert!(matches!(
            outcome,
            EngineOutcome::Rejected { reason } if reason.contains("no draw pending")
        ));
    }

    #[test]
    fn rejects_when_out_of_order() {
        let mut state = TestGame::default()
            .with_investigator(test_investigator(1))
            .with_investigator(test_investigator(2))
            .with_phase(Phase::Mythos)
            .build();
        state.mythos_draw_pending = Some(InvestigatorId(1));
        let mut events = Vec::new();
        // Inv2 attempts to draw when inv1 is expected.
        let outcome = draw_encounter_card(&mut state, &mut events, InvestigatorId(2));
        assert!(matches!(
            outcome,
            EngineOutcome::Rejected { reason } if reason.contains("out of order")
        ));
    }
}
```

- [ ] **Step 2: Confirm compile failure**

```bash
cargo test -p game-core draw_encounter_card_tests 2>&1 | head -10
```

Expected: `cannot find function draw_encounter_card`.

- [ ] **Step 3: Add the `PlayerAction::DrawEncounterCard` variant**

In `crates/game-core/src/action.rs`, inside `pub enum PlayerAction`:

```rust
    /// Resolve one Mythos-phase encounter draw for the acting
    /// investigator. Valid only during `Phase::Mythos` when
    /// `state.mythos_draw_pending == Some(acting_investigator)`.
    /// Resolves the per-card 5-step sub-sequence from Rules
    /// Reference p.24 step 1.4 inline (including surge re-draws);
    /// advances `mythos_draw_pending` to the next-in-turn-order
    /// drawer, or opens the `MythosAfterDraws` window if this was
    /// the last drawer.
    DrawEncounterCard,
```

- [ ] **Step 4: Add the dispatch handler**

In `crates/game-core/src/engine/dispatch.rs`:

```rust
/// Handler for [`PlayerAction::DrawEncounterCard`]. Validates phase
/// + cursor; delegates to [`mythos_draw_for`] on success.
pub(super) fn draw_encounter_card(
    state: &mut GameState,
    events: &mut Vec<Event>,
    investigator: InvestigatorId,
) -> EngineOutcome {
    if state.phase != Phase::Mythos {
        return EngineOutcome::Rejected {
            reason: format!(
                "DrawEncounterCard: only valid during Mythos phase, got {:?}",
                state.phase,
            )
            .into(),
        };
    }
    match state.mythos_draw_pending {
        None => EngineOutcome::Rejected {
            reason: "DrawEncounterCard: no draw pending (all investigators have drawn)".into(),
        },
        Some(expected) if expected != investigator => EngineOutcome::Rejected {
            reason: format!(
                "DrawEncounterCard: out of order; expected {expected:?}, got {investigator:?}",
            )
            .into(),
        },
        Some(_) => mythos_draw_for(state, events, investigator),
    }
}
```

- [ ] **Step 5: Wire the handler into `apply_player_action`**

In `crates/game-core/src/engine/mod.rs`, locate the `apply_player_action` (or equivalent) dispatch match arm. Add the new variant:

```rust
        PlayerAction::DrawEncounterCard => {
            crate::engine::dispatch::draw_encounter_card(state, events, investigator)
        }
```

- [ ] **Step 6: Verify all the new tests pass**

```bash
RUSTFLAGS="-D warnings" cargo test -p game-core draw_encounter_card_tests 2>&1 | tail -10
```

Expected: 3 PASS.

- [ ] **Step 7: Full workspace gauntlet**

```bash
RUSTFLAGS="-D warnings" cargo test --workspace --all-features 2>&1 | tail -20
cargo clippy --all-targets --all-features -- -D warnings 2>&1 | tail -10
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features 2>&1 | tail -10
```

- [ ] **Step 8: Commit**

```bash
git add crates/game-core/src/action.rs \
        crates/game-core/src/engine/dispatch.rs \
        crates/game-core/src/engine/mod.rs
git commit -m "$(cat <<'EOF'
engine: PlayerAction::DrawEncounterCard handler

Top-level player action exposing the Mythos draw chain. Peer to
Investigate / Move / PlayCard / EndTurn, not an AwaitingInput
sub-choice. The handler validates phase + cursor + ordering, then
delegates to mythos_draw_for.

UI shape: one click per investigator per Mythos phase; surge
re-draws are forced by rule and resolve within the same apply.

Refs #69.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 13: Add `_synth_surge_treachery` fixture + `with_encounter_deck` helper

**Files:**
- Modify: `crates/scenarios/src/test_fixtures/synth_cards.rs`
- Modify: `crates/scenarios/src/test_fixtures/synthetic.rs`

A surge-bearing synthetic treachery so integration tests can exercise the surge chain path, plus a helper for seeding encounter-deck compositions.

- [ ] **Step 1: Add `SYNTH_SURGE_TREACHERY_CODE` + metadata + abilities**

In `crates/scenarios/src/test_fixtures/synth_cards.rs`:

```rust
/// Code for the synthetic surge-bearing treachery. Its Revelation
/// is the same trivial "gain 1 resource" as `_synth_treachery`; the
/// load-bearing difference is `surge: true` on the metadata, which
/// drives the surge re-draw path in the per-card sub-sequence
/// (Rules Reference p.19, p.24 1.4 step 5).
pub const SYNTH_SURGE_TREACHERY_CODE: &str = "_synth_surge_treachery";

fn synth_surge_treachery_metadata() -> CardMetadata {
    CardMetadata {
        code: SYNTH_SURGE_TREACHERY_CODE.to_owned(),
        name: "Synthetic Surge Treachery".to_owned(),
        class: Class::Mythos,
        card_type: CardType::Treachery,
        cost: None,
        xp: None,
        text: Some(
            "Revelation - You gain 1 resource. Surge. \
             (Synthetic; not a printed card.)"
                .to_owned(),
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
        health: None,
        sanity: None,
        deck_limit: 1,
        quantity: 1,
        pack_code: "_synth".to_owned(),
        position: 2,
        is_fast: false,
        spawn: None,
        surge: true,
        peril: false,
    }
}

fn synth_surge_treachery_metadata_static() -> &'static CardMetadata {
    static M: OnceLock<CardMetadata> = OnceLock::new();
    M.get_or_init(synth_surge_treachery_metadata)
}

fn synth_surge_treachery_abilities() -> Vec<Ability> {
    // Same trivial Revelation as _synth_treachery — gain 1 resource.
    // (Mirror the existing synth_treachery_abilities body.)
    vec![Ability {
        trigger: Trigger::Revelation,
        effect: gain_resources(1, InvestigatorTarget::Drawer),
        usage_limit: None,
    }]
}
```

- [ ] **Step 2: Extend `metadata_for` / `abilities_for` lookups**

In the same file, find the `metadata_for` and `abilities_for` functions (or the `CardRegistry` const wiring) that the test registry uses. Add match arms for `SYNTH_SURGE_TREACHERY_CODE`:

```rust
fn metadata_for(code: &CardCode) -> Option<&'static CardMetadata> {
    match code.0.as_str() {
        SYNTH_TREACHERY_CODE => Some(synth_treachery_metadata_static()),
        SYNTH_ENEMY_CODE => Some(synth_enemy_metadata_static()),
        SYNTH_SURGE_TREACHERY_CODE => Some(synth_surge_treachery_metadata_static()),
        _ => None,
    }
}

fn abilities_for(code: &CardCode) -> Option<Vec<Ability>> {
    match code.0.as_str() {
        SYNTH_TREACHERY_CODE => Some(synth_treachery_abilities()),
        SYNTH_ENEMY_CODE => Some(synth_enemy_abilities()),
        SYNTH_SURGE_TREACHERY_CODE => Some(synth_surge_treachery_abilities()),
        _ => None,
    }
}
```

(Field names / function shapes may differ slightly from current code — match the existing pattern.)

- [ ] **Step 3: Add the `with_encounter_deck` helper**

In `crates/scenarios/src/test_fixtures/synthetic.rs`:

```rust
/// Seed the encounter deck of the synthetic scenario state with the
/// given card codes (in draw order, top = index 0). Used by Phase-4
/// integration tests that want to drive Mythos through deterministic
/// card sequences.
pub fn with_encounter_deck(state: &mut GameState, codes: Vec<CardCode>) {
    state.encounter_deck = codes.into();
}
```

(`state.encounter_deck` is a `VecDeque<CardCode>` per #132; `Vec<CardCode>::into()` yields `VecDeque`.)

- [ ] **Step 4: Verify the fixture compiles + existing tests pass**

```bash
RUSTFLAGS="-D warnings" cargo test -p scenarios --all-features 2>&1 | tail -20
cargo clippy -p scenarios --all-targets --all-features -- -D warnings 2>&1 | tail -10
RUSTDOCFLAGS="-D warnings" cargo doc -p scenarios --no-deps --all-features 2>&1 | tail -10
```

Expected: all green; the existing synthetic encounter / scenario tests still pass.

- [ ] **Step 5: Commit**

```bash
git add crates/scenarios/src/test_fixtures/synth_cards.rs \
        crates/scenarios/src/test_fixtures/synthetic.rs
git commit -m "$(cat <<'EOF'
scenario: synth surge treachery + with_encounter_deck helper

Adds a synthetic surge-bearing treachery (_synth_surge_treachery)
with Revelation = "gain 1 resource" and surge: true. Mirrors the
existing _synth_treachery — the load-bearing difference is the
surge flag, exercised by the next task's integration tests for the
Mythos draw chain.

with_encounter_deck helper lets tests seed the encounter deck with
deterministic card sequences.

Refs #69.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 14: Integration tests in `crates/scenarios/tests/mythos_phase.rs`

**Files:**
- Create: `crates/scenarios/tests/mythos_phase.rs`

End-to-end tests driving `StartScenario` → `EndTurn` → `DrawEncounterCard(...)` through the new Mythos machinery. Separate cargo binary so it owns `TEST_REGISTRY` installation without colliding with other tests.

- [ ] **Step 1: Create the test file with the registry install fixture**

`crates/scenarios/tests/mythos_phase.rs`:

```rust
//! Integration tests for #69 Mythos phase content.
//!
//! Drives full apply cycles through StartScenario → Investigation
//! actions → EndTurn → (pause at Mythos draws) → DrawEncounterCard →
//! Investigation, verifying the per-card 5-step sub-sequence, surge
//! chain, and post-1.4 window behavior end-to-end.

use std::sync::Once;

use game_core::action::{Action, PlayerAction};
use game_core::card_registry;
use game_core::engine::apply;
use game_core::state::{CardCode, GameState, InvestigatorId, Phase, WindowKind};
use game_core::Event;
use scenarios::test_fixtures::{
    synth_cards::{
        SYNTH_ENEMY_CODE, SYNTH_SURGE_TREACHERY_CODE, SYNTH_TREACHERY_CODE, TEST_REGISTRY,
    },
    synthetic::{setup, with_encounter_deck},
};

static REGISTRY_INIT: Once = Once::new();

fn install_registry() {
    REGISTRY_INIT.call_once(|| {
        let _ = card_registry::install(TEST_REGISTRY);
    });
}

// ... tests below ...
```

(Module / import paths may differ — match the existing `crates/scenarios/tests/encounter_spawn.rs` import pattern.)

- [ ] **Step 2: Add the single-treachery happy-path test**

In the same file:

```rust
#[test]
fn mythos_phase_resolves_single_treachery() {
    install_registry();
    let mut state = setup(/* 1 investigator */);
    with_encounter_deck(&mut state, vec![CardCode(SYNTH_TREACHERY_CODE.into())]);

    // Drive StartScenario → EndTurn (auto-chains to Mythos draw pending) → DrawEncounterCard.
    let inv1 = state.turn_order[0];
    let (state, _events) = drive(&mut state, vec![
        Action::PlayerAction { investigator: inv1, action: PlayerAction::EndTurn },
        Action::PlayerAction { investigator: inv1, action: PlayerAction::DrawEncounterCard },
    ]);

    // Assertions:
    assert_eq!(state.phase, Phase::Investigation, "Mythos phase ended, transitioned to Investigation");
    assert!(state.encounter_deck.is_empty(), "deck drained");
    assert_eq!(state.encounter_discard.len(), 1, "treachery placed in encounter discard");
    assert_eq!(state.mythos_draw_pending, None, "cursor cleared after last drawer");
    // Active investigator rotated to lead for the new Investigation phase.
    assert_eq!(state.active_investigator, Some(inv1));
}
```

The implementer supplies a `drive` helper that applies a series of actions and returns the final state + collected events. Mirror the pattern from `encounter_spawn.rs` if one exists; otherwise write a small local helper:

```rust
fn drive(state: &mut GameState, actions: Vec<Action>) -> (GameState, Vec<Event>) {
    let mut all_events = Vec::new();
    for action in actions {
        let result = apply(state.clone(), action);
        // ... per existing apply pattern ...
        all_events.extend(result.events);
        *state = result.state;
    }
    (state.clone(), all_events)
}
```

- [ ] **Step 3: Add the surge-chain test**

```rust
#[test]
fn mythos_phase_surge_chains_into_next_card() {
    install_registry();
    let mut state = setup(/* 1 investigator */);
    with_encounter_deck(
        &mut state,
        vec![
            CardCode(SYNTH_SURGE_TREACHERY_CODE.into()),
            CardCode(SYNTH_TREACHERY_CODE.into()),
        ],
    );

    let inv1 = state.turn_order[0];
    let (state, events) = drive(&mut state, vec![
        Action::PlayerAction { investigator: inv1, action: PlayerAction::EndTurn },
        Action::PlayerAction { investigator: inv1, action: PlayerAction::DrawEncounterCard },
    ]);

    // Both treacheries drawn + resolved in the single chain.
    assert!(state.encounter_deck.is_empty());
    assert_eq!(state.encounter_discard.len(), 2);

    // Both Revelations fired (resource gains visible in events).
    let card_revealed_count = events
        .iter()
        .filter(|e| matches!(e, Event::CardRevealed { .. }))
        .count();
    assert_eq!(card_revealed_count, 2, "surge re-draw triggered second CardRevealed");

    // First card revealed is the surge treachery, then the regular treachery.
    let surge_idx = events.iter().position(|e| matches!(
        e,
        Event::CardRevealed { code, .. } if code.0 == SYNTH_SURGE_TREACHERY_CODE
    )).expect("surge treachery revealed");
    let regular_idx = events.iter().position(|e| matches!(
        e,
        Event::CardRevealed { code, .. } if code.0 == SYNTH_TREACHERY_CODE
    )).expect("regular treachery revealed");
    assert!(surge_idx < regular_idx, "surge treachery revealed first");
}
```

- [ ] **Step 4: Add the enemy-spawn-during-Mythos test**

```rust
#[test]
fn mythos_phase_resolves_single_spawn_enemy() {
    install_registry();
    let mut state = setup(/* 1 investigator */);
    with_encounter_deck(&mut state, vec![CardCode(SYNTH_ENEMY_CODE.into())]);

    let inv1 = state.turn_order[0];
    let (state, events) = drive(&mut state, vec![
        Action::PlayerAction { investigator: inv1, action: PlayerAction::EndTurn },
        Action::PlayerAction { investigator: inv1, action: PlayerAction::DrawEncounterCard },
    ]);

    assert_eq!(state.enemies.len(), 1, "enemy spawned into play");
    // Engagement was on-spawn; assert via the EnemySpawned event.
    let spawned = events.iter().find_map(|e| match e {
        Event::EnemySpawned { code, engaged_with, .. } if code.0 == SYNTH_ENEMY_CODE => {
            Some((code.clone(), *engaged_with))
        }
        _ => None,
    }).expect("EnemySpawned emitted");
    assert_eq!(spawned.1, Some(inv1), "enemy engaged with drawing investigator on spawn");
}
```

- [ ] **Step 5: Add the multi-investigator player-order test**

```rust
#[test]
fn mythos_phase_multi_investigator_player_order() {
    install_registry();
    let mut state = setup(/* 2 investigators — adjust the setup helper or
                           construct manually if `setup` is hardcoded to 1 */);
    let inv1 = state.turn_order[0];
    let inv2 = state.turn_order[1];

    // Encounter deck: 2 distinct treacheries (need a second non-surge code or
    // synthesize one — for this test, reuse SYNTH_TREACHERY_CODE for both
    // and verify ordering via the events' contents-by-position).
    with_encounter_deck(
        &mut state,
        vec![
            CardCode(SYNTH_TREACHERY_CODE.into()),
            CardCode(SYNTH_TREACHERY_CODE.into()),
        ],
    );

    // Drive: each investigator's EndTurn brings us closer to Mythos;
    // after the last investigator's EndTurn, the chain pauses at
    // mythos_draw_pending == Some(inv1).
    let (state, _events) = drive(&mut state, vec![
        Action::PlayerAction { investigator: inv1, action: PlayerAction::EndTurn },
        Action::PlayerAction { investigator: inv2, action: PlayerAction::EndTurn },
        Action::PlayerAction { investigator: inv1, action: PlayerAction::DrawEncounterCard },
        Action::PlayerAction { investigator: inv2, action: PlayerAction::DrawEncounterCard },
    ]);

    assert!(state.encounter_deck.is_empty(), "both drew");
    assert_eq!(state.encounter_discard.len(), 2);
    assert_eq!(state.mythos_draw_pending, None);
    assert_eq!(state.phase, Phase::Investigation);
}
```

(If `setup` only supports 1 investigator today, either extend it or construct the multi-investigator state manually in this test. Match what the existing scenarios tests do.)

- [ ] **Step 6: Add the full-round-chain test**

```rust
#[test]
fn mythos_phase_full_round_chain() {
    install_registry();
    let mut state = setup(/* 1 investigator */);
    with_encounter_deck(&mut state, vec![CardCode(SYNTH_TREACHERY_CODE.into())]);

    let inv1 = state.turn_order[0];
    let initial_round = state.round;
    let (state, _events) = drive(&mut state, vec![
        Action::PlayerAction { investigator: inv1, action: PlayerAction::EndTurn },
        Action::PlayerAction { investigator: inv1, action: PlayerAction::DrawEncounterCard },
    ]);

    assert_eq!(state.round, initial_round + 1, "round bumped by step_phase entering Mythos");
    assert_eq!(state.phase, Phase::Investigation);
    assert_eq!(state.active_investigator, Some(inv1));
}
```

- [ ] **Step 7: Add the initial-empty-deck rejection test**

```rust
#[test]
fn mythos_draw_rejects_when_initial_deck_and_discard_both_empty() {
    install_registry();
    let mut state = setup(/* 1 investigator */);
    // Don't seed encounter deck — it's empty.

    let inv1 = state.turn_order[0];
    // Drive to Mythos draw pending.
    let state_after_endturn = apply(state.clone(), Action::PlayerAction {
        investigator: inv1,
        action: PlayerAction::EndTurn,
    });
    assert_eq!(state_after_endturn.state.mythos_draw_pending, Some(inv1));

    // Try to draw with empty deck + empty discard.
    let result = apply(state_after_endturn.state, Action::PlayerAction {
        investigator: inv1,
        action: PlayerAction::DrawEncounterCard,
    });
    assert!(matches!(
        result.outcome,
        game_core::engine::EngineOutcome::Rejected { reason }
            if reason.contains("encounter deck and discard both empty")
    ));
}
```

- [ ] **Step 8: Run the integration test suite**

```bash
RUSTFLAGS="-D warnings" cargo test -p scenarios --test mythos_phase 2>&1 | tail -30
```

Expected: all tests PASS.

- [ ] **Step 9: Full workspace gauntlet**

```bash
RUSTFLAGS="-D warnings" cargo test --workspace --all-features 2>&1 | tail -20
cargo clippy --all-targets --all-features -- -D warnings 2>&1 | tail -10
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features 2>&1 | tail -10
cargo build -p web --target wasm32-unknown-unknown 2>&1 | tail -10
```

Expected: every job green. This is the full CI-equivalent gauntlet from CLAUDE.md — must pass before merging.

- [ ] **Step 10: Commit**

```bash
git add crates/scenarios/tests/mythos_phase.rs
git commit -m "$(cat <<'EOF'
scenario: integration tests for #69 Mythos phase

End-to-end tests driving StartScenario → EndTurn → DrawEncounterCard
through the new Mythos machinery:

- Single-treachery happy path
- Surge chain (2 cards in one DrawEncounterCard apply)
- Single-spawn-enemy via Mythos
- Multi-investigator player order
- Full round chain (round-counter bump + transition back to
  Investigation)
- Initial-empty-deck rejection

Separate cargo binary so the test owns TEST_REGISTRY installation
without colliding with other Phase-4 integration tests.

Refs #69.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 15: Update the Phase-4 doc as the final commit

**Files:**
- Modify: `docs/phases/phase-4-scenario-plumbing.md`

Per CLAUDE.md's PR procedure step 6, the phase doc gets touched exactly once per PR, as the final commit before merge. By this point the PR number is known (open the PR first via Task 16, then come back here to fill in `#NN` — or finalize after CI is green and the PR is approved).

- [ ] **Step 1: Move #69 from Open to Closed**

In `docs/phases/phase-4-scenario-plumbing.md`, edit:

- Remove the `#69` row from the `Issues (10 — ...)` Open table at the top.
- Add a row to the `### Closed` table with the format used by prior entries (`#127` is the most recent template):

```markdown
| `#69` | Mythos phase content (draw + resolve + Surge) | #NN | Brief summary: player-driven `PlayerAction::DrawEncounterCard` (peer to Investigate / Move / PlayCard), per-card 5-step sub-sequence with surge re-draw loop (MAX_SURGE_CHAIN = 64 cap), post-1.4 `MythosAfterDraws` window via new `open_fast_window` helper, Mythos and skeleton Investigation phase drivers (phase-driver-owns-its-boundary-emits pattern; #70 / #71 / future full Investigation driver follow). `start_scenario` skips Mythos entirely in round 1 (no phantom boundary events). Extracted `check_play_card` / `check_activate_ability` validators back the Fast-eligibility scan. Synthetic surge treachery added to the test fixture. |
```

- [ ] **Step 2: Update the Status section header**

The line currently reads (post-#127): `🟡 In progress. ... First five PRs merged: ... Remaining: #69, #70, #71, #128, #73.`

Update to reflect #69 merging:

```markdown
🟡 In progress. ... First six PRs merged: ... `#69` Mythos phase content as PR #NN. Remaining: `#70`, `#71`, `#128`, `#73`.
```

(Mirror the exact wording style of the prior "PR #N" callouts.)

- [ ] **Step 3: Flip the Ordering / Arc table row**

In the `## Ordering (Shape B)` table, row 6 currently reads:

```markdown
| 6 | `#69` Mythos phase content | Composes 3 + 4 + 5. |
```

Update to:

```markdown
| 6 | `#69` Mythos phase content | ✅ PR #NN. Composes 3 + 4 + 5. |
```

- [ ] **Step 4: Add Decisions made entries** (load-bearing only — verbatim from the spec)

Append to the `## Decisions made (design pass 2026-05-21)` section, after the existing #127 entries:

```markdown

- **Phase-driver pattern: each phase has a driver function (owns `PhaseStarted` as step N.1) and an end helper (owns `PhaseEnded`) (`#69`, PR #NN).** Lands `mythos_phase` + `mythos_phase_end` (full) and `investigation_phase` (skeleton: emit + rotate to lead per Rules Reference p.24 step 2.2). #70 / #71 / the future full Investigation driver land their own peers and replace the remaining direct boundary emits in `step_phase`. `start_scenario`'s round-1 path bypasses both the Mythos driver and end helper entirely — no Mythos boundary events fire — because per Rules Reference p.24 the phase is skipped, not just empty.
- **Player-initiated phase actions are peers to action-phase actions, not `AwaitingInput` sub-choices (`#69`, PR #NN).** `PlayerAction::DrawEncounterCard` sits alongside `Investigate` / `Move` / `PlayCard`. Future per-investigator phase content (Upkeep choices, Enemy responses) follows the same shape unless it's genuinely a sub-choice within a resolving effect.
- **No "end round" / "end phase" actions; `EndTurn` auto-chains across phase boundaries (`#69`, PR #NN).** The chain pauses only when player input is genuinely required (Mythos 1.4 draws, future printed Fast windows that don't auto-skip). UI gets discrete pauses for free at the natural beats.
- **`open_fast_window` helper for printed-rule Fast windows (`#69`, PR #NN).** Always emits `WindowOpened`; auto-skips (emits `WindowClosed` + runs continuation inline) when no reactions queue AND `any_fast_play_eligible` returns false. Eligibility uses the extracted `check_play_card` / `check_activate_ability` validators so the real PlayCard / ActivateAbility gates back it, not a parallel weak filter. #70 / #71 / future Investigation-driver PRs use this helper for their printed player windows.
```

- [ ] **Step 5: Verify no Open question needs removal**

The current Open questions section covers hunter-movement (#128), resolution idempotency latch (#131), and the `AwaitingInput`-skip contract test gap. None are settled by #69. Leave the section unchanged.

- [ ] **Step 6: Commit**

```bash
git add docs/phases/phase-4-scenario-plumbing.md
git commit -m "$(cat <<'EOF'
docs: phase-4 doc update for #69 merge

Refs #69.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

(Substitute the real PR number `#NN` everywhere in the doc once the PR is open.)

---

## Task 16: Open the PR

**Files:** none (gh CLI).

- [ ] **Step 1: Push the branch**

```bash
git push -u origin engine/mythos-phase-content
```

- [ ] **Step 2: Open the PR**

Use the repo's PR template; include a brief design-decisions paragraph if the spec touches material the issue body doesn't already cover.

```bash
gh pr create --title "engine: Mythos phase content (#69)" --body "$(cat <<'EOF'
## Summary
- Lands the Mythos phase driver per Rules Reference p.24 sub-steps 1.1–1.5, with player-driven encounter draws (`PlayerAction::DrawEncounterCard`), surge re-draw chain (`MAX_SURGE_CHAIN = 64` cap), post-1.4 `MythosAfterDraws` player window via new `open_fast_window` helper.
- Adds a skeleton `investigation_phase` driver that owns rotation to lead (step 2.2). Establishes the "phase-driver owns its boundary emits" pattern that #70 / #71 / the future full Investigation driver will follow.
- `start_scenario` skips Mythos entirely in round 1 (no phantom `PhaseStarted(Mythos)` / `PhaseEnded(Mythos)` boundary events — the rules say "skip the mythos phase," not "run it empty").
- Extracts `check_play_card` / `check_activate_ability` pure-validation helpers from `play_card` / `activate_ability` (no behavior change at call sites); reused by `any_fast_play_eligible` so the Fast-window auto-skip uses the real PlayCard / ActivateAbility gates.

Spec: `docs/superpowers/specs/2026-05-24-69-mythos-phase-content-design.md`.
Plan: `docs/superpowers/plans/2026-05-24-69-mythos-phase-content.md`.

Closes #69.

## Test plan
- [ ] `RUSTFLAGS="-D warnings" cargo test --workspace --all-features`
- [ ] `cargo clippy --all-targets --all-features -- -D warnings`
- [ ] `cargo fmt --check`
- [ ] `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`
- [ ] `cargo build -p web --target wasm32-unknown-unknown`
- [ ] Integration tests in `crates/scenarios/tests/mythos_phase.rs` cover single-treachery / single-enemy / surge-chain / multi-investigator / full-round / empty-deck-rejection paths.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

- [ ] **Step 3: Watch CI in the background**

```bash
gh pr checks $(gh pr view --json number -q .number) --watch &
```

Per CLAUDE.md, since this PR was prepared via the brainstorming + writing-plans flow with no separate pre-push review pass, a post-push `review-agent` is reasonable. Per the user's preference, this is a layered-review project — defer the post-push review decision to the user.

- [ ] **Step 4: Update the phase doc PR-number references**

After the PR opens, run `gh pr view --json number -q .number` to get the assigned number, then edit `docs/phases/phase-4-scenario-plumbing.md` to substitute `#NN` with the real number throughout the entries added in Task 15. Amend or add a follow-up commit on the same branch.

---

## Summary

15 implementation tasks + 1 PR-opening task. Each task ships in its own commit (Task 2 ships 1 commit; some tasks ship more), every commit compiles cleanly under the full CI gauntlet. The work is structured to keep behavior changes scoped per commit:

- Tasks 1, 15, 16 are procedural (branch / phase doc / PR).
- Task 2 is a field addition + corpus regen with no engine behavior change.
- Tasks 3, 4, 5, 10 are behavior-preserving refactors (validator extractions, shared helper extraction).
- Tasks 6, 7 add state + helpers without surfacing them in user-facing flows yet.
- Tasks 8, 9 land the phase-driver pattern (the biggest behavior change — flips the Mythos / Investigation phase shape).
- Tasks 11, 12, 13 ship the user-visible Mythos draw mechanics.
- Task 14 verifies the assembly end-to-end.

## Rules-reference citations

The spec and this plan cite Rules Reference p.18 (Treachery, Revelation), p.19 (Surge), p.24 (I. Mythos phase 1.1–1.5; II. Investigation phase 2.1–2.2), p.10 (Encounter Deck reshuffle), p.9 (empty-discard reshuffle no-op). Quote the load-bearing clauses verbatim in doc-comments where the rule shapes engine behavior (Mythos phase driver, surge loop, `unreachable!` site rationales).

## Test plan

Per-task tests live with the code they cover (engine unit tests in `engine/dispatch.rs` / `engine/mod.rs`, fixture tests in the fixture file, integration tests in `crates/scenarios/tests/mythos_phase.rs`). The Task 14 integration suite is the end-to-end verification; if all those pass plus the workspace gauntlet, the PR is mergeable.

## Self-review checklist (run before saving)

- [x] **Spec coverage:** Each spec section maps to a task or sub-step in this plan. Key sections: card-data additions → Task 2; engine state additions → Task 6; new player action → Task 12; Mythos driver → Tasks 9, 11; Investigation skeleton → Task 8; Fast-window helpers → Tasks 5, 7; validator extractions → Tasks 3, 4; close-path changes → Task 7; step_phase / start_scenario / end_turn changes → Task 8; fixture additions → Task 13; tests → Task 14; follow-up entries + phase doc → Task 15.
- [x] **Placeholder scan:** No "TBD"-style placeholders. The `todo!()` instances in Tasks 4, 7, 9, 10 are intentional sequencing markers where one task adds a stub and the next task replaces it with the real body (called out explicitly at each occurrence).
- [x] **Type consistency:** Type names (`PlayCheckResult`, `ActivateCheckResult`, `WindowKind::MythosAfterDraws`, `Event::WindowOpened`, `PlayerAction::DrawEncounterCard`, `state.mythos_draw_pending`) and function names (`check_play_card`, `check_activate_ability`, `any_fast_play_eligible`, `open_fast_window`, `run_window_continuation`, `mythos_phase`, `mythos_phase_end`, `investigation_phase`, `peril_check`, `mythos_draw_for`, `advance_mythos_draw_pending`, `draw_encounter_card`, `resolve_encounter_card`, `MAX_SURGE_CHAIN`) match between earlier and later tasks. The `with_encounter_deck` helper signature is consistent between Tasks 13 (definition) and 14 (usage).
