# Player-draw weakness Revelation — design

**Date:** 2026-06-29
**Issue:** [#509](https://github.com/talelburg/eldritch/issues/509) (Part 3 of [#494](https://github.com/talelburg/eldritch/issues/494))
**Deferred follow-up:** [#514](https://github.com/talelburg/eldritch/issues/514)
**Status:** approved (brainstorm) — pending implementation plan

## Goal

When a weakness is drawn from the **player deck during play**, resolve its
Revelation immediately (RR Weakness keyword) instead of leaving it as a normal
hand card. For Cover Up (01007) this puts it into the controller's threat area
with 3 clues — the behavior that makes the #494 arc complete (after #508
reshuffles it out of the opening hand, drawing it later sets it up).

## Scope (settled in brainstorm)

**In scope:** drawn **persistent treachery weaknesses** (Cover Up is the only
one in the corpus). Minimal, YAGNI — no other weakness type is reachable in a
player-draw path today.

**Deferred to #514** (left in hand on draw — no regression, none reachable in
the corpus): non-persistent treachery weaknesses (need a post-Revelation
discard frame), weakness enemies (spawn on draw), weakness assets (enter play).

## Load-bearing facts (from investigation)

- **No synchronous effect-apply.** Effects resolve via `push_effect` + the
  `drive` loop. The encounter path (`resolve_encounter_card`,
  `crates/game-core/src/engine/dispatch/encounter.rs`) is the reference: it
  emits `Event::CardRevealed`, collects the `Trigger::Revelation` effects, and
  `push_effect`s them as one `Effect::Seq` with
  `EvalContext::for_controller(investigator)`; the loop steps them (handling
  suspension for free).
- **`Effect::PutIntoThreatArea` spawns a fresh instance _by code_**
  (`evaluator.rs:479` → `place_in_threat_area`). It does **not** move the drawn
  card. So the drawn copy must be removed from hand *before* the Revelation is
  pushed, or Cover Up duplicates (one in hand + one in the threat area).
- **`treachery_is_persistent(&[Ability])`** already exists in `encounter.rs`
  (private). It returns `true` when a treachery has a non-Revelation ability
  (Cover Up has reaction + forced). It is the existing "stays in play vs.
  auto-discard" signal and is reused here (made `pub(crate)`).
- **Setup vs. play distinction.** The setup opening-hand draw (#508) *sets
  weaknesses aside* (RR step 8), it does NOT resolve them. So the reveal hook
  must live on the in-play draw entries, never on the shared low-level
  `draw_cards` (which setup and mulligan call directly).

## Architecture / components

### 1. `resolve_drawn_weaknesses(cx, investigator)` — `crates/game-core/src/engine/dispatch/cards.rs`

The new helper. After a play-time draw lands cards in hand:

1. If no registry is installed, no-op (registry-free engine unit tests
   unchanged).
2. Scan the hand for codes that are **weakness** (`CardMetadata::is_weakness()`)
   **and** treachery (`card_type() == CardType::Treachery`) **and** persistent
   (`treachery_is_persistent(&abilities_for(code))`). Collect `(index, code)`.
3. Remove those from hand high-index-to-low (indices stay valid), preserving
   draw order for event emission.
4. For each removed weakness, in draw order: emit
   `Event::CardRevealed { investigator, code, card_type: Treachery }`; collect
   its `Trigger::Revelation` effects; if non-empty,
   `push_effect(cx, &Effect::Seq(effects), EvalContext::for_controller(investigator))`.
5. Weaknesses that don't match (non-persistent / non-treachery) are **left in
   hand untouched** — the #514 deferral.

Removal (step 3) is synchronous; the pushed Revelation (step 4) resolves later
on the drive loop — which is exactly why removal must precede the push.

`treachery_is_persistent` becomes `pub(crate)` and is reused (not duplicated).

### 2. Hook points

- **`draw_one_with_deckout`** (`cards.rs`): call `resolve_drawn_weaknesses`
  after the draw. This single site covers both the basic **Draw action**
  (`draw_primary_effect`) and the **Upkeep 4.4** draw
  (`upkeep_draw_and_resource`), which both route through it.
- **`draw_cards_effect`** (`evaluator.rs`): call
  `cards::resolve_drawn_weaknesses(cx, target_id)` after `draw_cards(...)`,
  before returning `Done` — covers DSL / card draw effects.
- **Never** `draw_cards` itself, nor `replace_opening_hand_weaknesses` — setup
  and mulligan keep *setting aside* (#508).

### Why no disposition frame

The general drawn-weakness machinery (a player-weakness continuation frame
mirroring `EncounterCard`, handling persistent vs. discard disposition) was
considered and declined as YAGNI: the only in-scope card (Cover Up) is
persistent and self-places via its own Revelation, so no post-Revelation
disposal step is needed. The non-persistent path (which *would* need such a
frame) is deferred to #514.

## Data flow

```
play-time draw (Draw action / Upkeep / DSL draw effect)
  → draw lands card(s) in hand
  → resolve_drawn_weaknesses: persistent treachery weakness?
      → remove from hand → CardRevealed → push_effect(Seq(revelation))
  → drive loop resolves the Revelation (Cover Up: PutIntoThreatArea(3))
  → Cover Up in the controller's threat area with 3 clues; not in hand
```

## Testing

- **Cover Up drawn via the Draw action** (integration, with `cards::REGISTRY`):
  deck with 01007 on top; after the Draw action resolves, Cover Up is NOT in
  hand, IS in the controller's threat area with 3 clues, and a
  `CardRevealed { code: "01007" }` event fired.
- **DSL / Upkeep draw of Cover Up** reaches the same end state (cover whichever
  of the upkeep / `DrawCards`-effect paths is cleanly drivable in a focused
  test; both route through the two hook points).
- **Non-weakness draw** is untouched: stays in hand, no `CardRevealed`, nothing
  in the threat area.
- **Regression:** the #508 opening-hand tests
  (`crates/scenarios/tests/opening_hand_weaknesses.rs`) still pass — setup
  *sets aside*, does NOT resolve.
- **Gauntlet:** host test/clippy/fmt/doc + wasm build/clippy (no web change, but
  the workspace must stay green).

## Out of scope (YAGNI / deferred)

- Non-persistent treachery weaknesses, weakness enemies, weakness assets on
  draw → #514.
- A suspending player-weakness Revelation: handled for free by the drive loop if
  one ever exists, but none does; no special work.
- No change to setup / mulligan (#508) or encounter-deck resolution.

## Open questions

None blocking.
