# Asset slot limits + discard-to-make-room (#498)

## Problem

`PlayCard` does not enforce asset **slot** limits. An investigator with Beat
Cop (an Ally) in play can play Guard Dog (also an Ally) and end up with two
Allies in play, despite having only one Ally slot. Found in a live-browser
playtest while verifying #466 / PR #493.

Slots **are** modeled in card metadata (`CardKind::Asset { slots: Vec<Slot> }`,
populated by the pipeline — the corpus carries `Slot::Ally`, `Slot::Hand`,
two-handed `vec![Slot::Hand, Slot::Hand]`, etc.). They are simply never consulted
on play.

## Rules (authoritative)

Rules Reference p.19, "Slots" (verbatim, from the vendored
`data/rules-reference/ahc01_rules_reference_web.pdf`):

> "The slots normally available to an investigator are: 1 accessory slot · 1 body
> slot · 1 ally slot · 2 hand slots · 2 arcane slots"

> "If an asset has no slot symbols on it, it does not take up any of the above
> slots. There is no limit to the number of slot-less assets an investigator can
> have in play."

> "If an investigator is at his or her slot limit for a type of asset and wishes
> to play or gain control of a different asset that would use that slot, the
> investigator must choose and discard other assets under his or her control
> simultaneously with the new asset entering the slot."

The load-bearing consequence: a full slot does **not** block the play. The
player **must discard** occupying asset(s) to make room, and that discard is
**simultaneous** with the new asset entering. So the correct fix is *implement
slots*, not *add a missing reject*. (The issue flagged this: "scope may be
'implement slots' rather than 'add a missing check'.")

`Tarot` is not in the original Core Rules Reference (a later-product slot); no
Core/Dunwich card uses it. We default its capacity to 1 and document it as
unreachable in scope.

## Corpus bounds

Distinct asset slot shapes in the generated corpus
(`crates/cards/src/generated/cards.rs`):

| shape | count |
|---|---|
| `Vec::new()` (slot-less) | 40 |
| `[Ally]` | 28 |
| `[Hand]` | 24 |
| `[Accessory]` | 11 |
| `[Arcane]` | 8 |
| `[Hand, Hand]` | 6 |
| `[Body]` | 4 |

No asset uses two *different* slot types; no asset uses `Arcane×2` or `Tarot`.
The only multi-count shape is the two-handed weapon (`Hand×2`). The algorithm is
written generally (a per-type multiset), but the only *genuine multi-option*
discard choice reachable today is: a slot type with capacity 2 (Hand or Arcane)
already holding two single-slot assets, when playing a third single-slot asset of
that type — the player picks which one of the two to discard.

## Approach

Mirror the existing interactive soak distribution
(`Continuation::DamageAssignment` in `dispatch/combat.rs`): an in-flight frame
holds the pending state, the player resolves **one `PickSingle` at a time**, and
the driver re-prompts until the deficit is satisfied. Auto-resolve (no prompt)
when there is a single candidate.

Considered and rejected: a batch `PickMultiple` of the whole discard set. It has
no existing analog to copy, makes auto-bind awkward, and turns validation into a
knapsack-style "does this set free enough" check. The iterative `PickSingle`
shape is already proven in the soak code.

## Where the choice happens

An asset enters play in `dispatch/cards.rs::dispose_play_from_hand` (the
`PlayDestination::InPlay` branch), which runs after the non-fast play's
attack-of-opportunity loop and the card's `OnPlay` effect. That is the RR
"entering the slot" moment, so the make-room discard belongs there — not at
`check_play_card` (validation, pre-cost) and not before the AoO loop.

## Components

### 1. Slot capacity — `default_slot_capacity(Slot) -> u8` (game-core)

A free function returning the RR p.19 defaults:

```
Accessory => 1, Body => 1, Ally => 1, Hand => 2, Arcane => 2, Tarot => 1
```

Doc-comment notes: `Tarot` is unreachable in the Core/Dunwich corpus, and
slot-modifying cards (none in scope) are a future concern — when one lands, this
becomes a per-investigator query. Lives in game-core (a rules concept), not
card-dsl (pure data). Likely home: a new `dispatch/slots.rs` module.

### 2. Occupancy / deficit — `dispatch/slots.rs`

- `card_slot_need(code) -> SlotCounts` — the asset's `slots` multiset from the
  registry (empty for a slot-less asset or a missing/non-asset code).
- `occupied_slots(state, investigator) -> SlotCounts` — sum of `slots` over the
  investigator's `cards_in_play` **assets**. The investigator card is
  deliberately *not* in `cards_in_play`, so it is correctly excluded; slot-less
  and non-asset in-play cards contribute nothing.
- `slot_deficit(state, investigator, code) -> SlotCounts` — for each slot type
  `T`: `max(0, occupied[T] + need[T] - cap[T])`.

`SlotCounts` is a small per-type tally (a `BTreeMap<Slot, u8>` or a fixed struct;
implementer's choice — keep it `Copy`/cheap and deterministic).

### 3. Validation gate — `check_play_card` (`dispatch/reaction_windows.rs`)

After resolving the card is an asset, reject only when `need[T] > cap[T]` for
some `T` — the play is *unsatisfiable* even after discarding every occupying
asset. A merely-full slot is **not** rejected (RR allows the play via
make-room). This reject is **unreachable in the current corpus** (max need is
`Hand×2` = cap 2); it exists for no-silent-approximation and guards a future
card that needs more of a slot type than the investigator has.

Consequence for the turn menu: because slot-full is not a rejection,
`enumerate.rs::legal_actions` keeps offering the play, so the interactive flow is
"player picks *play Guard Dog* → engine prompts *discard which asset to free the
Ally slot?* → player picks Beat Cop."

### 4. Enter-play with make-room — `dispose_play_from_hand`

Extract the current InPlay tail (remove from hand at `hand_index` → mint via
`threat_area::new_in_play_instance` → push to `cards_in_play` → `emit_event`
`EnteredPlay`) into `enter_asset_into_play(cx, investigator, hand_index)`. Then
the InPlay branch becomes:

- `deficit` all-zero → `enter_asset_into_play` (today's behavior, unchanged).
- `deficit > 0`, exactly one candidate asset → auto-discard it, then
  `enter_asset_into_play`.
- `deficit > 0`, 2+ candidates → push `Continuation::SlotDiscard { investigator,
  code, hand_index }` and return `AwaitingInput` with a **non-skippable**
  `PickSingle` over the co-controlled assets occupying a still-deficit slot type.

A "candidate" is an in-play asset under the investigator's control that occupies
≥1 slot of a slot type currently in deficit. The pending asset stays in hand
during the suspend (nothing mutates the hand between `AwaitingInput` and its
resume), and is removed by `enter_asset_into_play` at finish. Discarded occupiers
are in `cards_in_play`, so removing them never shifts `hand_index`.

The discard is mandatory (RR "must choose and discard"): there is no decline /
skip. A player who does not want to discard simply does not play the card.

### 5. `Continuation::SlotDiscard` frame + resume

New `Continuation` variant:

```rust
SlotDiscard {
    investigator: InvestigatorId,
    code: CardCode,   // the pending asset (still in hand at hand_index)
    hand_index: u8,
}
```

- **`drive` loop:** like `EncounterCard` / `PlayFromHand`, the `SlotDiscard` arm
  is framework-internal *while suspended* — but unlike those, it only makes
  progress via `resolve_input`. The frame, once pushed, is immediately
  accompanied by the `AwaitingInput` returned from `dispose_play_from_hand`, so
  the catch-all idle arm covers it (a suspension on top). It is added to the
  `awaits_input` / `is_phase_anchor` classification arms as a non-anchor that
  awaits input (mirroring `DamageAssignment`).
- **`resolve_input` routing:** add a `Some(Continuation::SlotDiscard { .. }) =>
  slots::resume_slot_discard(cx, response)` arm.
- **`resume_slot_discard`:** validate the `PickSingle` indexes a current
  candidate (invalid → `Rejected`, keep the frame — the `HunterMove` /
  `DamageAssignment` contract). On valid: discard the chosen in-play asset,
  recompute `slot_deficit`; if still in deficit, re-prompt (re-derive candidates,
  keep the frame); else pop the frame and `enter_asset_into_play`. Because the
  occupiers are discarded before the new asset enters, the observable event order
  is the discards' `CardDiscarded { from: Zone::InPlay }` then the new asset's
  `EnteredPlay` — RR "simultaneously" rendered as discard-then-enter.

Candidate enumeration must be **deterministic** (e.g. `cards_in_play` order /
`CardInstanceId` order) and identical between the prompt and the resume, so the
`OptionId` index is stable across the round-trip — the same discipline as
`prompt_current_point` / `resume_damage_assignment`.

### 6. Discard helper extraction (folds in #119)

The "remove a `CardInPlay` → push its code to the owner's `discard` → emit
`CardDiscarded { from: Zone::InPlay }`" sequence is currently duplicated in
`dispatch/combat.rs::defeat_overflowed_assets` and `dispatch/abilities.rs:300`.
Extract one helper:

```rust
fn discard_in_play_asset(cx: &mut Cx, investigator: InvestigatorId,
                         instance: CardInstanceId) -> Option<CardCode>
```

and reuse it at all three sites (the two existing + the new
`resume_slot_discard`). This nudges #119 (consolidate damage/discard mutation
helpers) along; it is in-scope because this PR performs exactly that operation.
Keep the helper behavior-preserving for the two existing callers (same events,
same order).

## Edge cases

- **Slot-less asset** (`Vec::new()`): `need` all-zero → `deficit` all-zero →
  enters directly, never prompts. (40 corpus assets.)
- **Two-handed weapon over-frees:** playing `Hand×2` with two single-`Hand`
  items in play → deficit `Hand: 2`; both candidates must go (each frees 1) →
  two iterative auto/explicit discards. Playing a single-`Hand` item when a
  `Hand×2` weapon occupies both → deficit `Hand: 1`; the only candidate is the
  weapon, discarding it frees 2 (over-frees by 1) → single auto-discard.
- **Capacity-1 slots (Ally/Body/Accessory):** always a single occupant →
  auto-discard, no prompt. (Beat Cop → Guard Dog is this case.)
- **Genuine multi-option:** Hand or Arcane (cap 2) holding two single-slot
  assets, playing a third → `PickSingle` over the two occupants.
- **Mid-play defeat:** if the actor is defeated during the play's AoO, the
  existing suppress path in `resume_action_resolution` runs and the asset never
  reaches `dispose_play_from_hand` — no slot logic runs, no frame pushed.
  Unchanged.
- **Registry-free engine unit tests:** with no registry installed, `card_slot_need`
  / `occupied_slots` see no metadata and return empty tallies → deficit zero →
  the slot path is a no-op, preserving registry-free behavior (same discipline
  as `is_weakness_code`, `pay_play_cost`).

## Testing

In the project's order of importance:

1. **Card tests** (`crates/cards/src/impls/<name>.rs`): Beat Cop in play + play
   Guard Dog → the Ally slot is freed (Beat Cop discarded, `CardDiscarded {
   from: Zone::InPlay }`), Guard Dog enters (`EnteredPlay`), exactly one Ally in
   `cards_in_play`. (Capacity-1 auto-discard path.)
2. **Engine unit tests** (`dispatch/slots.rs` `#[cfg(test)]` + `engine/mod.rs`):
   - `default_slot_capacity` returns the RR defaults.
   - `slot_deficit` math: slot-less → zero; cap-1 full → 1; `Hand×2` cases.
   - auto-bind when single candidate (no `AwaitingInput`).
   - `need > cap` → `check_play_card` rejects (construct a synthetic
     over-capacity asset via the test registry, since no corpus card hits it).
   - the genuine 2-of-3 Hand choice → `AwaitingInput` `PickSingle` with two
     options; resume with a valid pick discards the chosen one and the new asset
     enters; an out-of-range pick → `Rejected`, frame retained.
   - candidate ordering stable between prompt and resume.
3. **Integration test** (`crates/cards/tests/`): full `seat_and_open` → play to a
   slot conflict → resolve the discard prompt → assert final board, against real
   card metadata + abilities.

Run the full CI gauntlet (fmt, clippy host + wasm, test, doc, wasm-build) before
pushing.

## Out of scope / deferred

- **Slot-modifying cards** (grant/remove slots): none in Core/Dunwich; the
  capacity fn gets a `TODO` for a per-investigator query when one lands.
- **Surfacing the auto-discard (single-candidate) as an interactive prompt**
  when in `interactive_acknowledge` mode — ties into #492 (surface single-option
  auto-binds as choices); not part of this fix. The auto-bind here follows the
  existing `resolve_choice_count` 1-⇒-auto-bind discipline.
- **"Gain control of" an asset** (not via `PlayCard`) also triggers make-room per
  RR; no such effect exists in scope. The helper structure leaves room for it.
```
