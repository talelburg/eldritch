# #468 + #469 — PickMultiple prompt UX

**Date:** 2026-06-26
**Issues:** #468 (hand-size discard renders no cards — functional), #469 (PickMultiple prompts leak developer-facing text + always say "Commit" — cosmetic).

Both touch the same surface (the `PickMultiple` commit/mulligan/discard UI), so they ship together.

## Problem

1. **#468 (functional).** During the upkeep hand-size discard, `AwaitingInputView`'s `PickMultiple` branch renders **no card buttons** — only the action button — so the player can't pick what to discard. `active_hand` (`crates/web/src/input.rs`) derives the hand from `active_investigator → current_mulligan()`, but the hand-size discard runs in **upkeep**, where `active_investigator` is `None` and `current_mulligan()` is `None`. So `active_hand` returns empty.

2. **#469 (cosmetic).** The three `PickMultiple` engine prompts surface **wire/protocol text** to the player — e.g. `submit InputResponse::PickMultiple with the hand indices (as option ids)` — and the investigator via `{:?}` Debug (`InvestigatorId(1)`). The web client's `PickMultiple` button is a hardcoded `"Commit"` for all three contexts (mulligan / skill-commit / discard), so the mulligan and discard prompts read "Commit", which is wrong/odd.

## Solution

### #468 — render the prompted hand

- **game-core** (`crates/game-core/src/state/game_state.rs`): add an accessor mirroring `current_mulligan()`:
  ```rust
  /// The investigator currently prompted to discard down to the hand-size
  /// limit, if an upkeep hand-size discard is in progress; `None` otherwise.
  /// Reads the top `Continuation::HandSizeDiscard` frame's `remaining[0]`
  /// (the frame is only the top while the discard is pending, so `.last()`
  /// is correct — mirrors `current_mulligan()`).
  #[must_use]
  pub fn current_hand_size_discard(&self) -> Option<InvestigatorId> {
      match self.continuations.last() {
          Some(Continuation::HandSizeDiscard(h)) => h.remaining.first().copied(),
          _ => None,
      }
  }
  ```
- **web** (`crates/web/src/input.rs`): extend `active_hand`'s fallback chain:
  ```rust
  game.active_investigator
      .or_else(|| game.current_mulligan())
      .or_else(|| game.current_hand_size_discard())   // NEW
  ```

### #469 — player-facing copy + neutral button

- **Rewrite the three engine prompts** (drop the `submit InputResponse::PickMultiple with the hand indices (as option ids)` clause and the `{investigator:?}` Debug):
  - **Mulligan** (`dispatch/cards.rs`): `"Mulligan: choose cards to redraw (an empty selection keeps your hand)."`
  - **Skill-test commit** (`dispatch/skill_test.rs`): `"Commit cards to the {skill:?} test (difficulty {difficulty})."` (`{skill:?}` renders `Intellect`/`Combat`/… — player-readable; difficulty is a number.)
  - **Hand-size discard** (`dispatch/phases.rs`): `"You have more than {HAND_SIZE_LIMIT} cards in hand — choose cards to discard down to {HAND_SIZE_LIMIT}."`
- **web** (`crates/web/src/input.rs`): the `PickMultiple` button `"Commit"` → `"Confirm"`. The (now player-facing) prompt carries the per-context meaning.

## Scope notes (YAGNI)

- **Solo-scope:** the rewritten prompts drop the investigator reference (the prompted player is the active/sole one). Per-investigator naming via `inv.name` is deferred to multiplayer.
- **No per-context button verbs** (Mulligan/Commit/Discard). That needs a `confirm_label`-style field on `InputRequest` — a step toward #205's richer per-prompt metadata we deliberately deferred. A single neutral `"Confirm"` + the contextual prompt is the chosen minimal.
- `InputKind`/`skippable` and the rest of the structured-input contract are untouched.

## Testing

- **#468 (wasm, `crates/web/tests/`):** feed an `AwaitingInput` `PickMultiple` outcome with a `Continuation::HandSizeDiscard` frame and `active_investigator = None`; assert the prompted investigator's hand cards render (a `.hand-card` per card). This currently fails (empty list).
- **#469 (game-core unit):** assert each rewritten prompt contains none of `InputResponse`, `option ids`, `InvestigatorId(`; update any existing test that matched the old prompt substrings. **wasm:** assert the `PickMultiple` button text is `"Confirm"`.
- Full CI gauntlet (touches `game-core` + `web`, so the wasm jobs matter).

## Done criteria

- The hand-size discard prompt renders the investigator's hand so cards are selectable.
- The mulligan / commit / discard prompts read as player copy (no `InputResponse`/`option ids`/`InvestigatorId(N)`), and the button reads `"Confirm"`.
- All seven CI jobs green.
