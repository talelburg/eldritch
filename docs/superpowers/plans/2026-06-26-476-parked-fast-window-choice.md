# Parked Fast-Window Choice (#476) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** When a framework Fast window would park (the player holds a fast-playable card/ability), the engine emits a skippable `PickSingle` of the eligible fast plays instead of returning `Done`-idle — fixing the "Investigation round 2, no controls" strand and enabling fast-play-in-windows, with no client change.

**Architecture:** Refactor `any_fast_play_eligible` into `enumerate_fast_plays` (returns the eligible `TurnAction`s). Add a drive-loop arm for a top `FastWindow` that emits a skippable `PickSingle` of those plays (or closes the window when none remain). Route the resume: `PickSingle(OptionId)` dispatches the chosen fast play via the existing `dispatch_turn_action`, then the drive loop re-examines the still-top `FastWindow` and re-emits (play another) or closes — `Skip` keeps its existing "close + proceed" meaning.

**Tech Stack:** Rust, `game-core` engine (no I/O, wasm32-compatible kernel), the existing `Continuation::FastWindow` / `open_fast_window` machinery, `TurnAction` enumeration + `dispatch_turn_action`.

## Global Constraints

- **CI gauntlet (warnings-as-errors).** Before pushing, run all of: `RUSTFLAGS="-D warnings" cargo test --all --all-features`; `cargo clippy --all-targets --all-features -- -D warnings`; `cargo fmt --check`; `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`; `cargo build -p web --target wasm32-unknown-unknown`; `wasm-pack test --headless --firefox crates/web`; `cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings`.
- **Kernel purity.** `game-core` has no I/O and no presentation; this is a pure engine change. No client code changes (the existing skippable `PickSingle` rendering handles the new prompt).
- **Validate-first / mutate-second.** Resume handlers check preconditions and return `Rejected` (state/events unchanged) before mutating; out-of-range `OptionId` rejects.
- **`permits_fast` correctness.** `enumerate_fast_plays` MUST be called while the `FastWindow` is the top window (so `check_play_card`'s `top_window().permits_fast(investigator)` gate applies). Both call sites (drive loop, resume) satisfy this.
- **No behavior change when no fast play is eligible.** A window with no eligible fast play still auto-skips to `Done`/next prompt exactly as today.

---

### Task 1: `enumerate_fast_plays` — collect eligible fast plays

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/reaction_windows.rs` (`any_fast_play_eligible`, ~line 1219; imports near top)
- Test: `crates/game-core/src/engine/dispatch/reaction_windows.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Produces: `pub(super) fn enumerate_fast_plays(state: &GameState) -> Vec<crate::engine::enumerate::TurnAction>` — the eligible fast plays (Fast card plays + 0-cost activated abilities), in investigator/hand/ability order.
- `any_fast_play_eligible(state)` is reimplemented as `!enumerate_fast_plays(state).is_empty()`.

- [ ] **Step 1: Write the failing test**

`game-core`'s own test registry (`install_test_registry`) returns `None` abilities and only knows the `TEST_INV` investigator code, so there is **no in-crate fast-playable card** to enumerate. The in-crate unit test therefore covers only the **empty** case; the positive enumeration (a real Fast card → `PlayCard` candidate) is covered by the integration regression in **Task 5** (real `cards` registry + Magnifying Glass). In `crates/game-core/src/engine/dispatch/reaction_windows.rs`'s `#[cfg(test)] mod tests`, add:

```rust
    /// With no fast-playable card or 0-cost ability available, the enumeration is
    /// empty (the auto-skip path). The positive case — a real Fast card becoming
    /// a `PlayCard` candidate — is covered by the Task 5 integration regression,
    /// because game-core's test registry exposes no playable cards.
    #[test]
    fn enumerate_fast_plays_empty_when_nothing_eligible() {
        let inv = crate::state::InvestigatorId(1);
        let state = crate::test_support::GameStateBuilder::new()
            .with_phase(crate::state::Phase::Investigation)
            .with_active_investigator(inv)
            .with_investigator(crate::test_support::fixtures::test_investigator(1))
            .build();
        assert!(enumerate_fast_plays(&state).is_empty());
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p game-core --lib enumerate_fast_plays`
Expected: FAIL — `enumerate_fast_plays` not found.

- [ ] **Step 3: Implement `enumerate_fast_plays`; reimplement `any_fast_play_eligible` on top**

In `crates/game-core/src/engine/dispatch/reaction_windows.rs`, add `use crate::engine::enumerate::TurnAction;` to the imports if not present, then replace the body of `any_fast_play_eligible` (~line 1219) and add the new function. The new function mirrors the existing iteration exactly, but pushes `TurnAction`s instead of returning `true`:

```rust
/// Collect every fast play currently eligible across all investigators: Fast
/// cards in hand (`check_play_card` Ok + `is_fast`) and 0-action `Activated`
/// abilities on cards in play (`check_activate_ability` Ok). MUST be called with
/// the `FastWindow` on top of the stack so `check_play_card`'s `permits_fast`
/// gate applies to the right window (#476). Returns the plays as `TurnAction`s
/// in deterministic (investigator, hand-index / ability-index) order — the same
/// shape the open-turn menu dispatches via `dispatch_turn_action`.
pub(super) fn enumerate_fast_plays(state: &GameState) -> Vec<TurnAction> {
    let mut out = Vec::new();
    let Some(reg) = crate::card_registry::current() else {
        return out;
    };
    for (&inv_id, inv) in &state.investigators {
        // Fast events / Fast assets in hand.
        for hand_idx_usize in 0..inv.hand.len() {
            let Ok(hand_index) = u8::try_from(hand_idx_usize) else {
                break;
            };
            if let Ok(result) = check_play_card(state, inv_id, hand_index) {
                if result.is_fast {
                    out.push(TurnAction::PlayCard {
                        investigator: inv_id,
                        hand_index,
                    });
                }
            }
        }
        // 0-action Activated abilities on cards in play.
        for card in &inv.cards_in_play {
            let Some(abilities) = (reg.abilities_for)(&card.code) else {
                continue;
            };
            for (ab_idx, ability) in abilities.iter().enumerate() {
                let Trigger::Activated { action_cost: 0 } = ability.trigger else {
                    continue;
                };
                let Ok(ability_index) = u8::try_from(ab_idx) else {
                    break;
                };
                if check_activate_ability(state, inv_id, card.instance_id, ability_index).is_ok() {
                    out.push(TurnAction::ActivateAbility {
                        investigator: inv_id,
                        instance_id: card.instance_id,
                        ability_index,
                    });
                }
            }
        }
    }
    out
}

/// Whether any fast play is currently eligible (the boolean gate used by
/// `open_fast_window`'s park-vs-skip decision). Thin wrapper over
/// [`enumerate_fast_plays`].
pub(super) fn any_fast_play_eligible(state: &GameState) -> bool {
    !enumerate_fast_plays(state).is_empty()
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p game-core --lib enumerate_fast_plays any_fast_play`
Expected: PASS.

- [ ] **Step 5: Run the engine suite (no churn from the refactor)**

Run: `RUSTFLAGS="-D warnings" cargo test -p game-core`
Expected: PASS — `any_fast_play_eligible` keeps identical semantics.

- [ ] **Step 6: Commit**

```bash
git add crates/game-core/src/engine/dispatch/reaction_windows.rs
git commit -m "engine: enumerate_fast_plays — collect eligible fast plays (#476)"
```

---

### Task 2: Surface a parked Fast window as a skippable choice

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/reaction_windows.rs` (add `drive_fast_window`; imports)
- Modify: `crates/game-core/src/engine/dispatch/mod.rs` (drive loop, add a `FastWindow` arm after ~line 233)
- Test: covered by Task 5's integration regression (a parked window needs the real registry + a real fast card); add a focused engine assertion here only if a synthetic fast card is available (see Task 1 note).

**Interfaces:**
- Consumes: `enumerate_fast_plays` (Task 1); `close_reaction_window` (existing).
- Produces: `pub(super) fn drive_fast_window(cx: &mut Cx) -> EngineOutcome` — with a `FastWindow` on top: if `enumerate_fast_plays` is non-empty, return `AwaitingInput { PickSingle(plays), skippable }`; else `close_reaction_window(cx)` (pop + run continuation).

- [ ] **Step 1: Implement `drive_fast_window`**

In `crates/game-core/src/engine/dispatch/reaction_windows.rs`, add (ensure imports for `ChoiceOption`, `OptionId`, `InputRequest`, `ResumeToken`, `EngineOutcome`, `TurnAction` — most are already used in this file; add what's missing):

```rust
/// Drive a framework Fast window that is on top of the stack (#476): surface the
/// currently-eligible fast plays as a **skippable** `PickSingle`, or close the
/// window (running its continuation) when none remain. Called by the `drive`
/// loop's `FastWindow` arm — both when the window first parks and each time it is
/// re-exposed after a fast play resolves (the re-open loop). The window stays on
/// top across the prompt; `resume_window` dispatches the pick or closes on Skip.
pub(super) fn drive_fast_window(cx: &mut Cx) -> EngineOutcome {
    let plays = enumerate_fast_plays(cx.state);
    if plays.is_empty() {
        // Nothing (more) to play: close + run the window's continuation.
        return close_reaction_window(cx);
    }
    let options = plays
        .iter()
        .enumerate()
        .map(|(i, a)| ChoiceOption {
            id: OptionId(u32::try_from(i).unwrap_or(u32::MAX)),
            label: a.label(cx.state),
        })
        .collect::<Vec<_>>();
    EngineOutcome::AwaitingInput {
        request: InputRequest::pick_single(
            "Fast window — play a card or pass",
            options,
        )
        .skippable(),
        resume_token: ResumeToken(0),
    }
}
```

- [ ] **Step 2: Add the drive-loop `FastWindow` arm**

In `crates/game-core/src/engine/dispatch/mod.rs`, in the `drive` loop's `match top`, immediately AFTER the existing guarded window arm (the `TimingPointWindow | FastWindow` arm ending ~line 233) and BEFORE the `SkillTest` arm, add:

```rust
            // A framework Fast window on top (empty reaction candidates, so it
            // failed the guarded arm above): surface its eligible fast plays as a
            // skippable choice, or close it when none remain (#476). Re-examined
            // after each fast play resolves — the re-open loop — until the player
            // Skips or runs out of plays.
            Some(Continuation::FastWindow { .. }) => {
                match reaction_windows::drive_fast_window(cx) {
                    EngineOutcome::Done => {} // closed (no eligible plays); loop on
                    other => return other,    // the skippable prompt, or a continuation prompt
                }
            }
```

- [ ] **Step 3: Verify it compiles and the engine suite still passes**

Run: `RUSTFLAGS="-D warnings" cargo test -p game-core`
Expected: PASS to compile. Some tests that drive a sequence where a fast window now PROMPTS (previously idled to `Done`) may now fail by reaching an unexpected `AwaitingInput` — that is the expected blast radius. Note any failures; they are fixed in Task 4 (the no-commits driver) and by driving one more `Skip` where a test asserted `Done`. If a failure is a test that legitimately expects the old `Done`, update it to drive the `Skip` (or assert the new prompt) — record each in the commit message.

- [ ] **Step 4: Commit**

```bash
git add crates/game-core/src/engine/dispatch/reaction_windows.rs crates/game-core/src/engine/dispatch/mod.rs
git commit -m "engine: surface a parked Fast window as a skippable PickSingle (#476)"
```

---

### Task 3: Resume — play the chosen fast card, or pass

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/mod.rs` (`resume_window`, ~line 453)
- Test: covered by Task 5's integration regression (needs a real fast card to dispatch).

**Interfaces:**
- Consumes: `enumerate_fast_plays` (Task 1); `dispatch_turn_action` (existing, mod.rs); `close_reaction_window` (existing).
- Produces: `resume_window` handles a top framework `FastWindow` with `PickSingle(OptionId)` → dispatch the i-th fast play; `Skip` → close (existing).

- [ ] **Step 1: Extend `resume_window`**

In `crates/game-core/src/engine/dispatch/mod.rs`, replace the pure-Fast-gate tail of `resume_window` (the `if matches!(response, InputResponse::Skip) { … } else { Rejected }` block, ~line 467) with a `match` that also routes `PickSingle`:

```rust
    // A framework Fast window (no reaction candidates): the player either plays
    // an eligible fast card / ability (PickSingle into the re-enumerated list) or
    // passes (Skip). After a pick, dispatch the play and return — the `drive`
    // loop re-examines the still-top FastWindow and re-emits (play another) or
    // closes (#476).
    match response {
        InputResponse::Skip => reaction_windows::close_reaction_window(cx),
        InputResponse::PickSingle(OptionId(i)) => {
            let plays = reaction_windows::enumerate_fast_plays(cx.state);
            let Some(action) = plays.get(*i as usize).cloned() else {
                return EngineOutcome::Rejected {
                    reason: format!(
                        "ResolveInput: fast-window PickSingle({i}) out of range (0..{})",
                        plays.len(),
                    )
                    .into(),
                };
            };
            dispatch_turn_action(cx, &action)
        }
        other => EngineOutcome::Rejected {
            reason: format!(
                "ResolveInput: a Fast-play window is open; submit PickSingle(OptionId) to \
                 play, or Skip to pass, got {other:?}",
            )
            .into(),
        },
    }
```

(Keep the `has_candidates` reaction-window branch above this unchanged. Add `use crate::engine::OptionId;` to the file's imports if not already in scope — `OptionId` is used elsewhere in mod.rs, so it likely is.)

- [ ] **Step 2: Verify compile + engine suite**

Run: `RUSTFLAGS="-D warnings" cargo test -p game-core`
Expected: compiles; same blast-radius notes as Task 2 Step 3 (fixed in Task 4 + Task 5).

- [ ] **Step 3: Commit**

```bash
git add crates/game-core/src/engine/dispatch/mod.rs
git commit -m "engine: resume a Fast window — play the chosen fast card or pass (#476)"
```

---

### Task 4: Test-support — Skip skippable prompts in the no-commits driver

**Files:**
- Modify: `crates/game-core/src/test_support/resolver.rs` (`drive_to_terminal_no_commits`, the `let next = …` block)
- Test: `crates/game-core/src/test_support/resolver.rs` (`#[cfg(test)] mod tests`) — optional focused test; primarily exercised by Task 5.

**Interfaces:**
- Consumes: the new fast-window prompt (Tasks 2–3) is a `skippable` `AwaitingInput`.
- Produces: `drive_to_terminal_no_commits` answers any `skippable` `AwaitingInput` with `Skip`.

- [ ] **Step 1: Update the response selection**

In `crates/game-core/src/test_support/resolver.rs`, in `drive_to_terminal_no_commits`, change the `AwaitingInput` branch so a skippable prompt is declined with `Skip` (a no-commits drive declines every optional window). Replace the `let next = if let EngineOutcome::AwaitingInput { request, .. } = &outcome { … }` head:

```rust
        let next = if let EngineOutcome::AwaitingInput { request, .. } = &outcome {
            if request.skippable {
                // A skippable window (a #476 fast-window prompt, a reaction
                // window): the no-commits drive declines it.
                InputResponse::Skip
            } else {
                match request.kind {
                    InputKind::Confirm => InputResponse::Confirm,
                    _ => InputResponse::PickMultiple {
                        selected: Vec::new(),
                    },
                }
            }
        } else if matches!(outcome, EngineOutcome::Done) && !state.open_windows().is_empty() {
            InputResponse::Skip
        } else {
            return ApplyResult {
                state,
                events,
                outcome,
            };
        };
```

- [ ] **Step 2: Run the resolver + the previously-blast-radius tests**

Run: `RUSTFLAGS="-D warnings" cargo test -p game-core`
Expected: the no-commits-driver-mediated failures from Tasks 2–3 now PASS (the driver Skips the fast-window prompt). Any remaining failures are scripted-resolver tests that must add a `.skip()` / drive the prompt — fix each by driving the Skip or asserting the new prompt.

- [ ] **Step 3: Commit**

```bash
git add crates/game-core/src/test_support/resolver.rs
git commit -m "test: no-commits driver Skips skippable fast-window prompts (#476)"
```

---

### Task 5: Regression test — the real Gathering strand is fixed

**Files:**
- Create: `crates/scenarios/tests/issue_476_fast_window.rs` (a clean regression test; replaces the untracked debugging scratch `issue_476_repro.rs`, which is deleted/not committed)
- Modify: none.

**Interfaces:**
- Consumes: the full fix (Tasks 1–4) through the real `cards` + `scenarios` registries.

- [ ] **Step 1: Write the regression test**

Create `crates/scenarios/tests/issue_476_fast_window.rs`:

```rust
//! #476 regression: a framework Fast window no longer strands. Real registries,
//! Roland (01001), Rotting Remains (01163) on the encounter deck, a forced
//! Cultist bag, and Magnifying Glass (01030, a Fast asset) in hand. After the
//! Mythos draw + failed willpower test resolve, the InvestigatorTurnBegins Fast
//! window finds a fast play eligible and surfaces a SKIPPABLE PickSingle (not a
//! Done-idle strand). Skipping reaches the open turn; picking the option plays
//! the asset.

use game_core::action::RosterEntry;
use game_core::engine::{apply, seat_and_open, EngineOutcome};
use game_core::state::{CardCode, ChaosToken, GameState, InvestigatorId, Phase};
use game_core::test_support::take_turn_action;
use game_core::{Action, InputKind, InputResponse, OptionId, PlayerAction, TurnAction};
use scenarios::{the_gathering, REGISTRY};

#[ctor::ctor]
fn install_registries() {
    let _ = game_core::scenario_registry::install(REGISTRY);
    let _ = game_core::card_registry::install(cards::REGISTRY);
}

/// Drive to the post-Mythos-draw fast window: Roland holds Magnifying Glass,
/// fails the Rotting Remains willpower test (Cultist −1), and the
/// InvestigatorTurnBegins fast window prompts. Returns the state + outcome there.
fn to_fast_window() -> (GameState, EngineOutcome) {
    let roster = vec![RosterEntry {
        investigator: CardCode("01001".into()),
        deck: vec![],
    }];
    let mut state = seat_and_open(the_gathering::setup(), &roster).state;
    state = apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickMultiple { selected: vec![] },
        }),
    )
    .state;
    state.encounter_deck = vec![CardCode("01163".into())].into();
    state.encounter_discard.clear();
    state.chaos_bag.tokens = vec![ChaosToken::Cultist];
    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .unwrap()
        .hand
        .push(CardCode("01030".into()));

    let state = take_turn_action(state, &TurnAction::EndTurn).state;
    let state = apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::Confirm,
        }),
    )
    .state;
    let r = apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickMultiple { selected: vec![] },
        }),
    );
    (r.state, r.outcome)
}

#[test]
fn parked_fast_window_prompts_instead_of_stranding() {
    let (state, outcome) = to_fast_window();
    let EngineOutcome::AwaitingInput { request, .. } = &outcome else {
        panic!("expected a skippable fast-window prompt, got {outcome:?} (strand regression)");
    };
    assert_eq!(request.kind, InputKind::PickSingle, "{request:?}");
    assert!(request.skippable, "the fast window must be skippable (pass): {request:?}");
    assert!(
        !request.options.is_empty(),
        "the fast window lists the eligible fast plays: {request:?}"
    );
    assert_eq!(state.phase, Phase::Investigation);
    assert_eq!(state.round, 2);
}

#[test]
fn skipping_the_fast_window_reaches_the_open_turn() {
    let (state, _) = to_fast_window();
    let r = apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::Skip,
        }),
    );
    let EngineOutcome::AwaitingInput { request, .. } = &r.outcome else {
        panic!("Skip must reach the open turn, got {:?}", r.outcome);
    };
    assert_eq!(request.prompt, "Choose an action");
}

#[test]
fn playing_the_fast_card_puts_magnifying_glass_in_play() {
    let (state, _) = to_fast_window();
    // Option 0 is the only eligible fast play (Magnifying Glass).
    let r = apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(OptionId(0)),
        }),
    );
    assert!(
        r.state.investigators[&InvestigatorId(1)]
            .cards_in_play
            .iter()
            .any(|c| c.code == CardCode("01030".into())),
        "Magnifying Glass entered play after the fast-window pick"
    );
}
```

- [ ] **Step 2: Run the regression test**

Run: `cargo test -p scenarios --test issue_476_fast_window`
Expected: PASS (all three).

- [ ] **Step 3: Delete the debugging scratch file (if present)**

```bash
rm -f crates/scenarios/tests/issue_476_repro.rs
```

- [ ] **Step 4: Commit**

```bash
git add crates/scenarios/tests/issue_476_fast_window.rs
git rm --ignore-unmatch crates/scenarios/tests/issue_476_repro.rs
git commit -m "test: regression for the #476 parked-fast-window strand"
```

---

### Final: full CI gauntlet

- [ ] **Run the complete gauntlet** (all seven jobs, from Global Constraints). Fix any `fmt`/`clippy`/`doc` findings. Pay attention to the `test` job for any remaining blast-radius failures (a test that drove past a now-prompting fast window) and resolve each by driving the `Skip` (or asserting the new prompt) — never by suppressing the prompt.
- [ ] **No phase-doc update.** #476 is an unmilestoned `bug`/`engine`/`p1-next` issue not tracked in any `docs/phases/*` doc (consistent with its sibling bug/QoL fixes); do not invent a phase-doc entry.

## Notes for the implementer

- **Why no client change:** the engine now emits `AwaitingInput { PickSingle, skippable }` for a parked fast window; `AwaitingInputView`'s existing PickSingle arm renders the option buttons and the Skip control renders the pass. The strand was purely "client renders nothing for `Done`."
- **Re-open loop is automatic:** `resume_window`'s `PickSingle` arm dispatches the play and returns; `apply` then runs the `drive` loop, which re-examines the still-top `FastWindow` via the Task 2 arm and re-emits the prompt (play another) or closes (no more plays). No explicit loop code.
- **`permits_fast`:** never call `enumerate_fast_plays` without the `FastWindow` on top — both call sites (drive loop, resume) satisfy this; a future caller must too.
- Design doc: `docs/superpowers/specs/2026-06-26-476-parked-fast-window-choice-design.md`.
```
