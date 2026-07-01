# Board interactivity pass — design

**Date:** 2026-07-01
**Status:** approved (brainstorm), pending implementation plan
**Phase:** 7 (web iteration); the interactivity pass that follows the display-only card-rendering rework
**Umbrella issue:** #206 (`[ui] Intuitive legality affordances`)

This is an **umbrella** design covering the whole interactivity model. It is
sliced into independently-shippable PRs (S0–S6 below); each slice gets its own
finer implementation plan (and, where useful, its own spec) at plan time,
referencing this document.

## Goal

The display-only rework (#519–#533) rendered every board zone as a card. Player
input, however, still flows through a **flat sticky action bar** — the open-turn
action menu and every framework prompt render as a list of buttons detached from
the board. This pass **retires the bar** and moves interaction onto the board:
actionable entities glow and open a context menu of their legal actions; the
board becomes the input surface.

## The central constraint (why this touches the engine)

The engine already advertises the legal action set every open turn:
`enumerate::legal_actions` builds a rich `TurnAction` enum, each variant carrying
its target (`LocationId` / `EnemyId` / `hand_index` / `CardInstanceId`). But at
the wire boundary (`dispatch::mod::turn_menu`) that gets flattened to
`ChoiceOption { id, label: String }` — the target is **thrown away**, leaving only
a display string. So the client cannot map an option back to the board entity it
acts on. That is the one thing blocking board-attached interaction.

The fix is to **stop discarding the target**: enrich each wire `ChoiceOption` with
a structured anchor. Every downstream decision (which cards glow, what a card's
menu contains) derives from that anchor. This is the "richer per-option metadata
beyond `label`" that #205 explicitly deferred, and it is the engine-authoritative
answer to #206 (the board can never offer an action the server would reject,
because every button *is* an option the engine enumerated as legal — no
client-side legality re-computation, no drift).

### Approaches weighed

- **A — engine enriches `ChoiceOption` with a structured anchor (chosen).** Single
  source of truth; the engine already holds the target. Cost: a struct field +
  every option-builder supplies an anchor.
- **B — client re-derives targets from labels** (`"Fight Ghoul"` → enemy).
  String-coupled, blind to engine-internal targets (windows, effect choices),
  violates the no-approximation discipline. Rejected.
- **C — client re-computes `legal_actions`.** Duplicates engine legality → the
  exact drift risk #206 warns against. Rejected.

## The interaction model

- **Actionable entity → glow.** Any card/location/enemy whose anchor has ≥1 live
  option gets an "actionable" highlight. This alone satisfies #206's "communicate
  what's legal intuitively" — the player sees what they can act on before clicking.
- **Click → context menu.** Clicking an actionable entity opens a small popover
  listing that anchor's options (as `label`s). Uniform: **even a single option
  opens the menu** (player agency — always see and confirm what you commit to). No
  auto-execute branch.
- **Multi-select → click the cards to select.** Mulligan / skill-test commit /
  hand-size discard are a distinct "selection mode": click the actual hand cards to
  toggle a selected ring (no per-card buttons, no menu). Selecting **zero** is a
  legal, meaningful choice, so finalization needs an explicit control — the
  **Confirm (and Pass, when `skippable`) lives in the prompt banner**, not a
  bespoke widget bolted to the hand.
- **Prompt banner.** One slim, ephemeral surface (only while a prompt is live)
  carrying `request.prompt` text for prompts that mean something (windows, choices,
  soak) plus any Confirm/Pass. The open-turn menu's `"Choose an action"` is
  suppressed (noise). Not a control cluster — it absorbs only the finalize/decline
  controls that have no board home.
- **No persistent floating bar.** The `.action-bar` is deleted (S6). Controls are
  either on entities (menus), on the hand (selection), or in the transient banner.

The client never distinguishes "open turn" from "reaction window" — it routes
every option by anchor and renders banners/finalize controls off
`kind` / `skippable` / a `Global` anchor. Open-turn actions, windows, effect
choices, and soak all share one code path.

## Section 1 — engine + protocol foundation

```rust
// beside ChoiceOption (game_core::engine::outcome)
#[non_exhaustive]
pub enum OptionTarget {
    Global,                                                    // no board anchor
    Location(LocationId),
    Enemy(EnemyId),
    HandCard { investigator: InvestigatorId, hand_index: u8 },
    CardInstance(CardInstanceId),                              // in-play / threat / investigator / soak card
    Act,
}

pub struct ChoiceOption {
    pub id: OptionId,
    pub label: String,        // unchanged: full, unambiguous, engine-authored ("Fight Ghoul")
    pub target: OptionTarget, // NEW
}
```

- `turn_menu` derives `target` from each `TurnAction`: Move/Investigate →
  `Location`; Fight/Evade/Engage → `Enemy`; PlayCard → `HandCard`; ActivateAbility
  → `CardInstance`; AdvanceAct → `Act`; EndTurn/Resource/Draw → `Global`.
- `label` stays the **full** string (do **not** shorten it engine-side). The client
  shortens for display if it wants; keeping the full label leaves the transitional
  flat bar unambiguous while slices land, and decouples label text from render
  context.
- Every *other* option-builder (reaction windows, `choice.rs`, soak
  `DamageAssignment`, attack-order) supplies `OptionTarget::Global` in S0; later
  slices replace `Global` with real anchors as each prompt dissolves.
- `ChoiceOption` rides `EngineOutcome` through `protocol` untouched — recompile
  only. The new field is **required** on the wire, following the #453 precedent (a
  stale payload errors rather than silently degrading).

## Section 2 — web routing model

- **Index.** A pure fn `route_options(&InputRequest) -> OptionIndex` groups
  `request.options` by `target` into an `anchor → Vec<ChoiceOption>` map.
  Native-testable, no DOM.
- **Context provision.** Provide two things via leptos context (as `OutboundTx`
  already is), avoiding prop-drilling through `board → map → node` and
  `board → panel → card`:
  - the `OptionIndex` (a derived signal off the store's `outcome`), and
  - a `submit(InputResponse)` closure wrapping the existing
    `tx.unbounded_send(ClientMessage::Submit{…})` + `pending_label` bookkeeping.
- **Shared `ContextMenu` component.** Reads the active-anchor signal + the index +
  submit; renders one popover of the anchor's options; a menu item submits
  `ResolveInput(PickSingle(id))` and closes. Dismiss on outside-click / Escape.
  Positioning anchors to the clicked node (absolutely-positioned map node vs.
  flex-row hand card) is the one piece needing care.
- **Per-entity seam.** Each display component (`Card`, `EnemyCard`, `location_map`
  node, act card) gains: (a) glow when its anchor has ≥1 option, (b) a click
  handler setting the active anchor (opening the menu). Components stay pure over
  `(entity, options-for-this-anchor)` — still native-testable. Option-rendering is
  centralized in `ContextMenu`, not smeared across components.

## Section 3 — framework prompts finding homes

Every prompt dissolves via the same `anchor → surface` mechanism; three
*ephemeral* control types survive (only while a prompt is live):

| Prompt | Anchor / home | Control shape |
|---|---|---|
| Open-turn menu | entity cards | glow + context menu |
| Mulligan / skill-test commit / hand-size discard (`PickMultiple`, hand-indexed) | the hand cards — click to toggle a selected ring | banner Confirm (+ Pass if skippable) + prompt text |
| Encounter draw (`Confirm`) | a new minimal **encounter-deck** element (renders `encounter_deck` count) | a Draw button on the deck |
| Reaction / Fast window (`PickSingle` + Skip) | the source card (`CardInstance`) → "Trigger …" menu item | window-global Pass in the banner |
| Interactive soak (`PickSingle`) | the soak cards (`CardInstance`) → click to assign | banner shows remaining damage/horror |
| Effect `ChooseOne` (`PickSingle`) | the chosen enemies / locations (`Enemy`/`Location`) | banner with the choice prompt |

**Genuinely-global open-turn actions** (End turn, Gain resource, Draw) have no
board entity: End turn → a distinct always-present control on the active
investigator's panel; Gain resource → a small affordance by the resource counter;
Draw → a button on a minimal **player-deck** element. This is the most
design-per-action for the least mechanical gain, so it lands last (S6).

## Slicing (S0–S6)

Mirrors the display-only cadence — each slice its own issue / plan / PR. The
engine advertises anchors from S0 while the flat bar keeps reading `label`, so
every web slice lands independently with the bar as a working fallback until S6
retires it. **No slice leaves the client unusable.**

- **S0 — engine + protocol foundation.** `OptionTarget` on `ChoiceOption`;
  `turn_menu` derives anchors; other builders emit `Global`. No web behavior
  change. `engine`.
- **S1 — web plumbing + locations.** `route_options`, context provision,
  `ContextMenu`, prompt banner, glow seam — proven on locations (Move/Investigate).
  Flat bar coexists. `ui`.
- **S2 — enemies** (Fight/Evade/Engage menu). `ui`.
- **S3 — hand** (Play menu) + mulligan/commit/hand-size-discard as click-to-select
  on hand cards + banner Confirm. `ui`.
- **S4 — in-play / threat** (Activate + reaction/Fast-window "Trigger" menu;
  window Pass in banner). `ui`.
- **S5 — act + soak + effect choices** (Advance Act; click-to-assign soak;
  `ChooseOne` anchoring). Touches adjacent #492 (single-option auto-binds surfaced
  as choices). `ui`, `engine`.
- **S6 — globals + bar retirement (the closer).** Homes for End turn / Gain
  resource / Draw (investigator-panel controls + player-deck element);
  encounter-deck element for the draw Confirm; **delete `.action-bar`**, folding
  picker + skill-test-result into their own surfaces. `ui`.

## Testing

- **Engine** — anchor derivation covered in `enumerate`/`turn_menu` tests; extend
  `every_enumerated_action_is_accepted_by_its_handler` to also assert each
  `TurnAction` → expected `OptionTarget`. Pure, native.
- **Protocol** — a serde round-trip test for the new field.
- **Web pure fns** — `route_options` (anchor grouping) and menu-item derivation are
  native-testable `#[cfg(test)]`, no DOM.
- **Web rendering** — wasm-pack headless: glow appears iff the anchor has options;
  click opens the menu; a menu item submits the correct `ResolveInput`.
  Registry-dependent rendering goes in its own test binary (the `location_card.rs`
  first-wins-registry precedent).

## What "done" looks like (whole pass, at S6)

- No `.action-bar`. Actionable board entities glow; clicking one opens a context
  menu of its legal actions; selecting an item performs it.
- Multi-select prompts are click-to-select on the hand with a banner Confirm.
- Windows / soak / effect choices resolve on their source cards; the banner carries
  prompt text + Pass/Confirm only.
- End turn / Gain resource / Draw / encounter-draw have board homes.
- The board can never surface an action the server rejects.
- Full 7-job CI gauntlet green at every slice.

## Out of scope

- Any change to engine legality *rules* — this pass only surfaces the already-
  enumerated legal set.
- Multiplayer input routing (whose-turn, delegated tests) — Phase 8.
- Animation / drag-and-drop / richer per-option metadata beyond the anchor.
- The ArkhamDB icon font (still deferred).
