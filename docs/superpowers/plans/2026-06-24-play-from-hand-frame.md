# PlayFromHand frame Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the two synchronous `apply_effect` hand-play sites (`complete_play`, `play_fast_event`) with a single-shot `Continuation::PlayFromHand` disposal frame, so OnPlay/OnEvent effects are pushed for the global `drive` loop and the type-disposal (event→discard, asset→enter-play) runs when the effect pops.

**Architecture:** A new `PlayFromHand { investigator, code, hand_index }` continuation, disposed by `cards::dispose_play_from_hand` (mirrors `dispose_encounter_card_if_top`): it pops itself, then flushes an event or enters an asset into play + emits `EnteredPlay` — letting the drive loop open any queued after-enters-play window. `PlayFromHand`'s disposal is the single event-flush site; the apply-loop and eager flushes are removed.

**Tech Stack:** Rust, the `game-core` engine crate; `cards` integration tests. No new dependencies.

## Global Constraints

- **Match CI's strict flags before declaring any task done** (copy verbatim):
  - `RUSTFLAGS="-D warnings" cargo test --all --all-features`
  - `cargo clippy --all-targets --all-features -- -D warnings`
  - `cargo fmt --check`
  - `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`
  - `cargo build -p web --target wasm32-unknown-unknown`
  - `cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings`
- **Validate-first / mutate-second** in every dispatch handler (engine convention).
- **Behaviour-preserving at the `apply` boundary**: `crates/cards/*` (card + integration suites) go through real `apply`/`drive` and MUST stay green **untouched** — the regression net. Load-bearing: Dynamite Blast 01024 (suspending OnPlay event + suspending Fast event), Research Librarian 01032 (after-enters-play window), Emergency Cache 01088 (non-Fast event), Machete 01020 / assets (enter play).
- **Each task is its own commit and keeps the full strict gauntlet green and bisectable.** One PR (continues #423 on `engine/effect-callsite-migration`).
- **Commit trailers** (every commit):
  ```
  Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_0174FjCFBQR8ZiSSTpHaUjyB
  ```
- Spec: `docs/superpowers/specs/2026-06-24-play-from-hand-frame-design.md`.

## File structure

| File | Responsibility | Tasks |
|---|---|---|
| `crates/game-core/src/state/game_state.rs` | `Continuation::PlayFromHand { investigator, code, hand_index }` | 1 |
| `crates/game-core/src/engine/dispatch/cards.rs` | `complete_play` → push frame; new `dispose_play_from_hand`; `apply_effect`→`push_effect` | 1 |
| `crates/game-core/src/engine/dispatch/mod.rs` | drive-loop `PlayFromHand` arm; `resolve_input` defensive arm | 1 |
| `crates/game-core/src/engine/mod.rs` | remove the apply-loop `flush_pending_played_event` call | 1 |
| `crates/game-core/src/engine/dispatch/reaction_windows.rs` | `play_fast_event` → push frame; drop eager flush + `apply_effect` import | 2 |
| `crates/cards/tests/play_card.rs` (+ a Fast-event test home) | new tests | 1, 2 |

---

## Task 1: `PlayFromHand` frame + normal-play migration

Introduce the frame + disposal, migrate `complete_play` (shared by the Fast inline path and the non-Fast post-AoO `resume_play_card` path), wire the drive-loop + `resolve_input` arms, and remove the now-redundant apply-loop flush.

**Files:**
- Modify: `crates/game-core/src/state/game_state.rs`
- Modify: `crates/game-core/src/engine/dispatch/cards.rs`
- Modify: `crates/game-core/src/engine/dispatch/mod.rs`
- Modify: `crates/game-core/src/engine/mod.rs`
- Test: `crates/cards/tests/play_card.rs`

**Interfaces:**
- Produces: `Continuation::PlayFromHand { investigator: InvestigatorId, code: CardCode, hand_index: u8 }`; `pub(super) fn dispose_play_from_hand(cx: &mut Cx) -> EngineOutcome`.
- Consumes: `push_effect` (evaluator), `resolve_play_target(&CardCode) -> Result<(PlayDestination, Vec<Ability>, bool, CardType), EngineOutcome>`, `threat_area::new_in_play_instance`, `emit::emit_event`, `flush_pending_played_event`.

- [ ] **Step 1: Write the failing test — normal event play discards exactly once**

In `crates/cards/tests/play_card.rs`, add a test that playing Emergency Cache 01088 (non-Fast event, `OnPlay GainResources(3)`) emits exactly one `CardDiscarded { from: Zone::Hand }` and leaves no card in hand. Model the harness (registry install, state builder, the no-AoO play drive) on the existing tests in this file — read the top of `play_card.rs` for the exact `install`/builder/`apply`-or-`drive` helpers it already uses, and reuse them.

```rust
#[test]
fn normal_event_play_discards_exactly_once() {
    // Play Emergency Cache 01088 (event, OnPlay GainResources 3) with no engaged
    // enemy (no AoO). Mirror this file's existing play harness.
    // Assert: outcome Done; resources gained; exactly one CardDiscarded(Hand);
    // 01088 no longer in hand; pending_played_event is None.
    assert_eq!(
        result
            .events
            .iter()
            .filter(|e| matches!(e, Event::CardDiscarded { from: Zone::Hand, .. }))
            .count(),
        1,
    );
    assert!(result.state.pending_played_event.is_none());
}
```

- [ ] **Step 2: Run it to verify it passes today (baseline) — then we keep it green**

Run: `cargo test -p cards --test play_card normal_event_play_discards_exactly_once`
Expected: PASS (today the apply-loop flush already gives one discard). This test is the **invariant guard** for the migration — it must stay green after the refactor, proving `PlayFromHand` flushes exactly once. (If it fails today, the harness mirror is wrong — fix the test setup before proceeding.)

- [ ] **Step 3: Add the `PlayFromHand` continuation variant**

In `crates/game-core/src/state/game_state.rs`, add to `enum Continuation` (next to `EncounterCard`):

```rust
    /// A card being played from hand, mid-resolution (Slice D #423). Pushed
    /// **below** the card's pushed `OnPlay`/`OnEvent` effect; when that effect
    /// pops, the drive loop's `PlayFromHand` arm runs [`dispose_play_from_hand`]
    /// (event → discard the stashed `pending_played_event`; asset → remove from
    /// hand at `hand_index`, enter play, emit `EnteredPlay`). Single-shot:
    /// `dispose_play_from_hand` pops the frame before emitting `EnteredPlay`, so
    /// the loop opens any after-enters-play window itself. Framework-internal;
    /// never awaits input (the catch-all `awaits_input`/`is_phase_anchor` arms
    /// cover it, as for `EncounterCard`).
    ///
    /// [`dispose_play_from_hand`]: crate::engine::dispatch::cards::dispose_play_from_hand
    PlayFromHand {
        /// The playing investigator.
        investigator: InvestigatorId,
        /// The played card's code (re-derives destination + asset metadata).
        code: CardCode,
        /// Hand slot of an **asset** still in hand (enters play at disposal).
        /// Ignored for an event — `begin_event_play` already removed it and
        /// stashed it in `pending_played_event`.
        hand_index: u8,
    },
```

No change to `is_phase_anchor` / `awaits_input` / `pending_candidates` — like `EncounterCard`, `PlayFromHand` falls into their catch-all arms and never sits on top at an action boundary.

- [ ] **Step 4: Add `dispose_play_from_hand`**

In `crates/game-core/src/engine/dispatch/cards.rs`, add (model on `encounter::dispose_encounter_card_if_top`):

```rust
/// Dispose of a [`PlayFromHand`](crate::state::Continuation::PlayFromHand) frame
/// once its pushed `OnPlay`/`OnEvent` effect has popped (Slice D #423). Pops the
/// frame first, then by destination: an **event** flushes its stashed
/// `pending_played_event` to discard; an **asset** is removed from hand at
/// `hand_index`, minted into play, and announced via `EnteredPlay`. Because the
/// frame is popped *before* `emit_event`, a reaction window the latter queues
/// (Research Librarian 01032) lands on top and the drive loop opens it — no
/// manual window open, no second stage. Returns `Done` (disposal never awaits
/// input); a missing-registry re-derive surfaces as `Rejected`.
pub(super) fn dispose_play_from_hand(cx: &mut Cx) -> EngineOutcome {
    let Some(crate::state::Continuation::PlayFromHand {
        investigator,
        code,
        hand_index,
    }) = cx.state.continuations.last().cloned()
    else {
        unreachable!("dispose_play_from_hand: top frame is not PlayFromHand");
    };
    cx.state.continuations.pop();

    let destination = match resolve_play_target(&code) {
        Ok((destination, _abilities, _is_fast, _card_type)) => destination,
        // Unreachable post-play (this code already resolved at play time); a
        // Rejected here would strand the played card, so surface it loudly.
        Err(outcome) => return outcome,
    };

    match destination {
        super::PlayDestination::Discard => {
            // Event: discard the stashed played event (RR Appendix I step 4),
            // exactly once — this is the sole flush site.
            flush_pending_played_event(cx);
        }
        super::PlayDestination::InPlay => {
            // Asset: remove from hand, mint + seed its in-play instance, push it
            // into play, then announce it. The drive loop opens the
            // after-enters-play reaction window (Research Librarian 01032) if
            // `emit_event` queued one — the frame is already popped.
            let played = cx
                .state
                .investigators
                .get_mut(&investigator)
                .expect("dispose_play_from_hand: investigator present")
                .hand
                .remove(usize::from(hand_index));
            let in_play = super::threat_area::new_in_play_instance(cx, played);
            let instance = in_play.instance_id;
            cx.state
                .investigators
                .get_mut(&investigator)
                .expect("dispose_play_from_hand: investigator present")
                .cards_in_play
                .push(in_play);
            let _ = super::emit::emit_event(
                cx,
                &super::emit::TimingEvent::EnteredPlay {
                    instance,
                    controller: investigator,
                },
            );
        }
    }
    EngineOutcome::Done
}
```

- [ ] **Step 5: Migrate `complete_play` to push the frame**

In `cards.rs`, replace `complete_play`'s body (the `apply_effect` OnPlay loop + the asset enter-play tail + the manual `open_queued_reaction_window`) with a push-and-return. Keep the doc comment's intent; the `TODO(#417)` note stays.

```rust
fn complete_play(
    cx: &mut Cx,
    investigator: InvestigatorId,
    hand_index: usize,
    code: &CardCode,
) -> EngineOutcome {
    let (_destination, abilities, _is_fast, _card_type) = match resolve_play_target(code) {
        Ok(v) => v,
        Err(outcome) => return outcome,
    };
    // Combine the OnPlay effects into one Seq and push it for the drive loop,
    // below a PlayFromHand frame that disposes the card (event → discard; asset
    // → enter play) once the effect pops. (Slice D #423 — replaces the
    // synchronous apply_effect + asset tail + manual window open.)
    cx.state
        .continuations
        .push(crate::state::Continuation::PlayFromHand {
            investigator,
            code: code.clone(),
            hand_index: u8::try_from(hand_index)
                .expect("hand index fits u8 (#111 hand-size cap)"),
        });
    let on_play: Vec<crate::dsl::Effect> = abilities
        .into_iter()
        .filter(|a| a.trigger == Trigger::OnPlay)
        .map(|a| a.effect)
        .collect();
    if !on_play.is_empty() {
        let eval_ctx = EvalContext::for_controller(investigator);
        push_effect(cx, &crate::dsl::Effect::Seq(on_play), eval_ctx);
    }
    EngineOutcome::Done
}
```

Switch the `cards.rs` import (line 11) from `use super::super::evaluator::{apply_effect, EvalContext};` to `{push_effect, EvalContext}` (confirm `apply_effect` has no other use in this file — it does not).

- [ ] **Step 6: Add the drive-loop arm**

In `crates/game-core/src/engine/dispatch/mod.rs`, in the `drive` loop, after the `EncounterCard` arm (≈`mod.rs:243`), add:

```rust
            Some(Continuation::PlayFromHand { .. }) => {
                match cards::dispose_play_from_hand(cx) {
                    EngineOutcome::Done => {}
                    other => return other,
                }
            }
```

- [ ] **Step 7: Add the `resolve_input` defensive arm**

In `mod.rs`'s `resolve_input`, after the `EncounterCard` reject arm (≈`mod.rs:504`), add:

```rust
        Some(Continuation::PlayFromHand { .. }) => EngineOutcome::Rejected {
            reason: "ResolveInput: no input prompt is outstanding (hand-play disposal is \
                     framework-internal)"
                .into(),
        },
```

- [ ] **Step 8: Remove the redundant apply-loop flush**

In `crates/game-core/src/engine/mod.rs` (≈line 167-175), delete the `if matches!(outcome, EngineOutcome::Done) { dispatch::cards::flush_pending_played_event(&mut cx); }` block and its comment — `PlayFromHand` disposal now flushes during the drive loop (before this point on a `Done` apply; on a suspending OnPlay it flushes on the resuming apply). Update `flush_pending_played_event`'s doc comment in `cards.rs` ("Called by the apply loop …" → "Called by `PlayFromHand` disposal …").

- [ ] **Step 9: Add the asset enter-play test**

In `crates/cards/tests/play_card.rs`, add a test that playing an asset (Machete 01020 — no OnPlay) lands the instance in `cards_in_play` and removes it from hand, driving through real `apply`/`drive`:

```rust
#[test]
fn asset_play_enters_play_through_the_frame() {
    // Play Machete 01020 (asset, no OnPlay), no engaged enemy. Mirror this
    // file's play harness.
    // Assert: outcome Done; 01020 in cards_in_play; not in hand;
    // an EnteredPlay-driven window only if a matching reaction is in play (none
    // here → no AwaitingInput).
}
```

- [ ] **Step 10: Run the new tests + the behaviour net**

Run:
```bash
cargo test -p cards --test play_card
cargo test -p cards   # Dynamite Blast, Research Librarian, revelation suites, etc.
```
Expected: PASS — the single-discard + asset-enter-play tests green, and **no assertion changes** in existing `crates/cards/*`. Research Librarian 01032's after-enters-play window still opens (now via the drive loop); Emergency Cache / Dynamite Blast (non-Fast) discard exactly once.

- [ ] **Step 11: Full strict gauntlet, then commit**

Run the six Global-Constraint commands. Then:
```bash
git add crates/game-core/src/state/game_state.rs crates/game-core/src/engine/dispatch/cards.rs crates/game-core/src/engine/dispatch/mod.rs crates/game-core/src/engine/mod.rs crates/cards/tests/play_card.rs
git commit -m "engine: PlayFromHand frame for normal hand-play disposal (Slice D, #423)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_0174FjCFBQR8ZiSSTpHaUjyB"
```

---

## Task 2: migrate `play_fast_event` to the frame

The last synchronous `apply_effect` production caller. Push the frame instead, dropping the eager flush (disposal owns it) and the now-unused `apply_effect` import.

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/reaction_windows.rs`
- Test: `crates/cards/tests/evidence.rs` (Evidence! 01022 — the canonical `play_fast_event` card: a Fast `reaction_on_event` after-defeat event)

**Interfaces:**
- Consumes: `Continuation::PlayFromHand` (Task 1), `push_effect`, `begin_event_play`.

- [ ] **Step 1: Write the failing test — Fast reaction event discards exactly once**

`play_fast_event` is the reaction-window path for Fast `OnEvent` events — Evidence! 01022 ("Fast. Play after you defeat an enemy: Discover 1 clue") is the canonical card; `crates/cards/tests/evidence.rs` already drives it end-to-end through the after-defeat window. Add there, reusing that file's harness (the defeat → window → Evidence!-play drive):

```rust
#[test]
fn evidence_fast_event_discards_exactly_once() {
    // Defeat an enemy to open the after-defeat window, then play Evidence!
    // 01022 from hand in it. Mirror this file's existing Evidence! harness.
    assert_eq!(
        result
            .events
            .iter()
            .filter(|e| matches!(e, Event::CardDiscarded { from: Zone::Hand, .. }))
            .count(),
        1,
    );
}
```

- [ ] **Step 2: Run it to verify it passes today (baseline)**

Run: `cargo test -p cards --test evidence evidence_fast_event_discards_exactly_once`
Expected: PASS today (the eager flush gives one discard). The invariant guard for this task — must stay green after migration.

- [ ] **Step 3: Migrate `play_fast_event`**

In `crates/game-core/src/engine/dispatch/reaction_windows.rs`, replace the `match apply_effect(cx, &effect, eval_ctx) { … }` block (the `Rejected`/`AwaitingInput`/`Done`+eager-flush arms) with a push-and-return — the `PlayFromHand` frame (above the live reaction window) owns the flush:

```rust
    // Push the event's disposal frame (above the window), then push its effect
    // for the drive loop. On the effect's completion, PlayFromHand disposal
    // flushes the event (RR Appendix I step 4) and the window beneath resumes
    // its candidate scan. `hand_idx` is moot for an event (begin_event_play
    // already removed + stashed it); pass it for the frame's shape. (Slice D #423.)
    cx.state
        .continuations
        .push(crate::state::Continuation::PlayFromHand {
            investigator: controller,
            code: candidate.code.clone(),
            hand_index: u8::try_from(hand_idx).unwrap_or(0),
        });
    push_effect(cx, &effect, eval_ctx);
    EngineOutcome::Done
```

Remove the now-unused `apply_effect` from the `use super::super::evaluator::{apply_effect, push_effect, EvalContext};` import (it becomes `{push_effect, EvalContext}` — `play_fast_event` was its last user here). Drop the `flush_pending_played_event` call (gone) and update the function doc to say the `PlayFromHand` frame discards the event on completion.

- [ ] **Step 4: Run the new test + the behaviour net**

Run:
```bash
cargo test -p cards   # Evidence! 01022, Dodge 01023 (Fast cancel reaction), Roland after-defeat, etc.
```
Expected: PASS — single `CardDiscarded` for the Fast reaction event; Dodge's cancel-window play and the after-defeat reaction suites untouched.

- [ ] **Step 5: Confirm no production `apply_effect` callers remain except Task-5 sites**

Run: `grep -rn "apply_effect" crates/game-core/src --include=*.rs | grep -v "fn apply_effect\|#\[cfg(test)\]\|drive_effect_to_base"`
Expected: only `choice.rs::resume_effect_walk` (via `drive_effect_to_base`) + the evaluator/choice **test** calls — the remaining Slice D **Task 5** work. `cards.rs` and `reaction_windows.rs` are now clean.

- [ ] **Step 6: Full strict gauntlet, then commit**

Run the six Global-Constraint commands. Then:
```bash
git add crates/game-core/src/engine/dispatch/reaction_windows.rs crates/cards/tests/evidence.rs
git commit -m "engine: PlayFromHand frame for Fast-event play (Slice D, #423)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_0174FjCFBQR8ZiSSTpHaUjyB"
```

---

## Self-Review notes

- **Spec coverage:** frame (Task 1 Step 3); `dispose_play_from_hand` pop-then-emit, drive loop opens window (Step 4, 6); `complete_play` migration (Step 5); single-flush ownership + apply-loop flush removal (Step 8); `play_fast_event` migration + eager-flush removal (Task 2 Step 3); `resolve_input` defensive arm (Step 7); single-`CardDiscarded` tests (1.1, 2.1) + asset enter-play (1.9). No `stage` enum / no `AfterEnterWindow` (the spec's simplification) — Step 3 has no stage field; Step 4 pops before `emit_event`.
- **Type consistency:** `Continuation::PlayFromHand { investigator: InvestigatorId, code: CardCode, hand_index: u8 }` and `dispose_play_from_hand(cx) -> EngineOutcome` are used identically in `game_state.rs`, `cards.rs`, and the two `mod.rs` arms. `resolve_play_target` returns `(PlayDestination, Vec<Ability>, bool, CardType)` as consumed.
- **Behaviour preservation:** `crates/cards/*` untouched except the two new tests; the new tests are baseline-green today (invariant guards), so a regression in the migration shows as a count change. Research Librarian's window now opens via the drive loop (Step 4) — its existing test asserts the window/effect, not the open mechanism.
- **`hand_index` u8:** `complete_play` takes `hand_index: usize`; the frame stores `u8` (hand-size cap #111). The `u8::try_from(...).expect(...)` mirrors `validate_commit_indices`' documented invariant.
