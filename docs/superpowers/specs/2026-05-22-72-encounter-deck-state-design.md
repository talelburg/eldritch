# #72 — encounter deck state (design)

GitHub issue: [#72](https://github.com/talelburg/eldritch/issues/72) · Phase: [phase-4-scenario-plumbing](../../phases/phase-4-scenario-plumbing.md) · Sibling PRs (sequential, in order): this → #126 → #127.

## Context
Phase-4 scenario plumbing needs the shared encounter deck — the pile of treacheries and enemies Mythos draws from. This is the first of three sequential PRs that together unlock #69's Mythos phase content. #126 (Revelation DSL + on-draw resolution) and #127 (enemy spawn rules) both depend on the state and primitive ops landed here.

We chose **sequential** over parallel for these three: the on-draw dispatch in #126/#127 is a shared function and would collide on simultaneous edits, plus the `EventPattern` enum and `test_fixtures` module would both see additive churn. Sequential keeps per-PR review surface small.

## Scope
Add the encounter deck + discard on `GameState`, the primitive draw/shuffle/reshuffle helpers in dispatch, and the engine-record + event variants logging shuffles for replay determinism. No DSL changes. No on-draw path yet (that's #126).

## State additions on `GameState`
File: `crates/game-core/src/state/game_state.rs`.

```rust
/// Shared encounter deck (top = front). Built at scenario setup from
/// encounter-set codes; drawn from during Mythos. When empty, the
/// discard reshuffles back in.
pub encounter_deck: VecDeque<CardCode>,
/// Encounter discard pile. Treacheries land here after Revelation
/// resolves; defeated enemies land here in later issues.
pub encounter_discard: Vec<CardCode>,
```

`VecDeque` for the deck because top-draw (`pop_front`) is the dominant op. Indexed access for "look at top three" effects (Drawn to the Flame style) is rare and stays cheap. Discard is `Vec` — order matters and the ops are push + drain-on-reshuffle.

## Engine surface — additive sibling variants
File: `crates/game-core/src/action.rs` and `crates/game-core/src/event.rs`.

```rust
// action.rs
enum EngineRecord {
    DeckShuffled { investigator: InvestigatorId },   // unchanged, player-deck-only
    EncounterDeckShuffled,                           // new
}

// event.rs
enum Event {
    DeckShuffled { investigator: InvestigatorId },   // unchanged
    EncounterDeckShuffled,                           // new
}
```

**Additive sibling pattern.** Existing `DeckShuffled` stays player-deck-only — no caller churn. The mild asymmetry (one variant says "player" implicitly, the other says "encounter" explicitly) is acceptable; a future tagged-deck refactor is reachable if/when act/agenda decks land.

## Dispatch internals (private helpers)
File: `crates/game-core/src/engine/dispatch.rs`. Mirrors the existing `shuffle_player_deck` shape.

```rust
pub(super) fn shuffle_encounter_deck(state: &mut GameState, events: &mut Vec<Event>);
pub(super) fn draw_encounter_top(state: &mut GameState, events: &mut Vec<Event>) -> Option<CardCode>;
pub(super) fn reshuffle_encounter_discard(state: &mut GameState, events: &mut Vec<Event>);
```

- `shuffle_encounter_deck`: shuffles `state.encounter_deck` in place via the deterministic RNG path, emits `Event::EncounterDeckShuffled` iff the deck had ≥ 2 cards (mirrors `shuffle_player_deck`'s "skip emit on trivial shuffle" rule).
- `draw_encounter_top`: returns `Some(code)` from the front; on empty deck, transparently calls `reshuffle_encounter_discard` and retries. If both deck and discard are empty, returns `None` and the caller decides what to do (#69's Mythos loop will treat this as a scenario condition, not an engine error).
- `reshuffle_encounter_discard`: drains `state.encounter_discard` into `state.encounter_deck` and calls `shuffle_encounter_deck`. Does NOT push `EngineRecord::EncounterDeckShuffled` to the action log — mirrors the existing player-deck pattern where mid-handler reshuffles rely on RNG determinism for replay rather than log entries. The variant is reserved for explicit shuffle actions (see next section).

### Handler for the explicit `EngineRecord::EncounterDeckShuffled`
Add to the dispatch match in `engine/mod.rs`. Analogous to the existing `deck_shuffled` handler — calls `shuffle_encounter_deck`. The variant exists so future non-action-triggered shuffles (e.g., a "shuffle X into the encounter deck" effect) have an explicit log entry; #72 itself adds the handler but no path in this PR pushes the variant. First real consumer lands when a card effect forces an explicit encounter-deck shuffle.

## Setup integration
Scenario setup currently lives in `PlayerAction::StartScenario`'s handler. #72 doesn't wire real encounter-set composition through `ScenarioModule` yet — the synthetic fixture (`crates/scenarios/src/test_fixtures/synthetic.rs`) leaves `encounter_deck` empty (`VecDeque::new()`) and the new `Default::default()` for `GameState` populates both fields as empty. Tests directly mutate `state.encounter_deck` to exercise draw/shuffle ops.

Real setup wiring (populated encounter sets via `ScenarioModule::setup`) lands incidentally in #126 — the synthetic fixture gains a populated encounter deck containing the synthetic treachery.

## Test plan
1. **State serde roundtrip.** `GameState` with non-empty `encounter_deck` + `encounter_discard` serializes and deserializes losslessly.
2. **Drain to empty.** Build a known deck of N codes via direct mutation, call `draw_encounter_top` N times → each returns `Some(code)` in order, the (N+1)th returns `None`.
3. **Empty-deck reshuffle.** Build deck of 0, push K codes to discard, call `draw_encounter_top` → asserts `Event::EncounterDeckShuffled` fires, returns one of the K codes, the remaining (K−1) stay in the deck. The mid-handler reshuffle does NOT push an `EngineRecord` (per the design above) — the helper does not maintain the action log directly, the caller does.
4. **Empty-on-both.** Empty deck + empty discard → `draw_encounter_top` returns `None`, no shuffle event emitted.
5. **Determinism.** Two identical `GameState`s with the same `RngState` seed and the same K-card discard reshuffle → produce identical post-shuffle order. Confirms reshuffle uses the seeded RNG path.

## Phase-doc update (last commit of the PR)
File: `docs/phases/phase-4-scenario-plumbing.md`.

- Move `#72` from the Open table → Closed table; bump counts.
- Flip Ordering row 3 to `✅ PR #N`.
- Add a Decision entry: **"Additive sibling for `DeckShuffled` (`#72`, PR #N).** Encounter deck shuffles ride a new `EngineRecord::EncounterDeckShuffled` / `Event::EncounterDeckShuffled` rather than renaming or tagging the existing `DeckShuffled` (which stays player-deck-only). Trade-off: one variant says 'player' implicitly, the other says 'encounter' explicitly. Worth re-examining if act/agenda decks join the family — at that point a tagged `DeckKind` refactor becomes load-bearing." (Load-bearing because #126/#127 callers will follow this pattern.)

## Out of scope (deferred)
- Encounter-set type wiring through `ScenarioModule::setup` — picked up in #126.
- "Discard X cards from the encounter deck" effects (Drawn to the Flame–style) — lands with that card.
- Act / agenda deck siblings — separate issue when content forces them.
