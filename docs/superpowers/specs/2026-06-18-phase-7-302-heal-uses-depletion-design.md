# PR-3 (#302) — `Effect::Heal` + uses-depletion auto-discard

**Status:** design approved. PR-3 of the
[choice-cluster completion](2026-06-17-phase-7-choice-cluster-completion-decomposition-design.md),
on the merged keystone (#349/PR #351) and #301 (PR #352). Tracker issue
[#302](https://github.com/talelburg/eldritch/issues/302). First Aid's engine
prereqs; the First Aid *card* ships in PR-4 (#239), Medical Texts (a 2nd `Heal`
consumer) in PR-6 (#321). Independent of the choice axis except that `Heal`
reuses the keystone's `InvestigatorTarget::Chosen`.

## Why this exists

First Aid 01019 (verbatim, `data/arkhamdb-snapshot/pack/core/core.json`):
`Uses (3 supplies). If First Aid has no supplies, discard it.` /
`[action] Spend 1 supply: Heal 1 damage or horror from an investigator at your
location.` Two engine primitives are missing: the first **healing** effect, and
**uses-depletion auto-discard**.

## Component ① — `Effect::Heal`

The engine's first heal — the inverse of the existing `take_damage`/`take_horror`
(no defeat interaction; healing only reduces).

```rust
/// Which health track a heal/harm acts on. Shared by Effect::Heal now; the
/// DealDamage/DealHorror consolidation (separate follow-up, see below) adopts it.
enum HarmKind { Damage, Horror }

Effect::Heal { kind: HarmKind, target: InvestigatorTarget, count: u8 }
```

- Builder `heal(kind, target, count)`.
- Handler `heal_effect`: ground the target (add `Effect::Heal { target, .. }` to
  `ground_chosen_targets`' investigator-target match — reusing the keystone's
  `Chosen(At(Here))`), resolve the investigator, saturating-reduce its
  `damage`/`horror` by `count`, emit a new
  **`Event::Healed { investigator, kind, amount }`** (`amount` = the actually-
  reduced amount, `min(count, current)`). `count == 0` is a no-op.
- **Consumers:** First Aid — `ChooseOne([Heal{Damage,…}, Heal{Horror,…}])` over
  the keystone's already-shipped `Effect::ChooseOne`; Medical Texts (#321) heals
  on its skill-test success. Two consumers → a genuine shared primitive.

### Deferred (separate follow-up): consolidate `DealDamage` + `DealHorror`

`HarmKind` also fits the two damage effects, but consolidating them is a
cross-cutting refactor (**43 call sites** across cards/tests/engine) unrelated to
healing. #302 *introduces* `HarmKind` (for `Heal`); a focused follow-up PR folds
`Effect::{DealDamage, DealHorror}` into one kinded effect reusing it (#354).

## Component ② — uses-depletion auto-discard

`Uses` gains a printed-property flag; the engine discards a flagged asset when
its uses deplete.

- **`Uses.discard_when_empty: bool`** (card-dsl `card_data`), **pipeline-parsed**
  from the templated `If <name> has no <kind>, discard it` clause (a regex beside
  `parse_uses`; `true` for First Aid / Forbidden Knowledge 01058 / Grotesque
  Statue 01071, `false` for Flashlight / weapons — RR p.27: "If the card contains
  no such text, it remains in play even if out of uses").
- **Regenerate the corpus** (`cargo run -p card-data-pipeline`) — `Uses` gains a
  field, so `crates/cards/src/generated/cards.rs` regenerates (only the `Uses {…}`
  literals gain `discard_when_empty`). Update the hand-written mock `Uses {…}`
  literals in `crates/game-core/tests/{weapon_fight,discard_self}.rs`.
- **`discard_card_from_play(cx, investigator, instance_id)`** — extract the
  in-play-asset discard body from the `Cost::DiscardSelf` payment arm
  (abilities.rs) into a shared helper, so `DiscardSelf` *and* depletion both use
  it (2 consumers).
- **Check placement:** in `pay_activation_costs`' `Cost::SpendUses` arm, after the
  decrement: look up the source's metadata via `card_registry::current()`; if its
  `Uses.discard_when_empty` is set and the spent kind's runtime pool is now `0`,
  `discard_card_from_play`.

### Timing — a deliberate Slice-1 simplification (`TODO`)

RR p.27 frames the discard as a **condition** (the card is discarded *because* it
has no uses), with the initiating ability still resolving to completion. The
literal home is "after any uses change," because the three corpus cards deplete
differently: First Aid / Grotesque Statue via the **cost** (`Spend`), but
**Forbidden Knowledge via its effect** (`Move 1 secret … to your resource pool`).
A `SpendUses`-arm check is therefore First-Aid-correct but not general.

#302 ships the `SpendUses`-arm placement (First Aid is the only Slice-1 consumer,
and it depletes via cost; the discard is observationally equivalent — the heal
still resolves, nothing reacts to the ordering, and First Aid's damage-or-horror
`ChooseOne` always suspends, so a post-resolution hook would have to thread the
resume path for zero observable gain). A `TODO(#353)` records: rules-precise
timing is post-ability-resolution, and **effect-depletion cards (Forbidden
Knowledge 01058, Grotesque Statue 01071) require relocating the check** when
implemented. The metadata flag is already general — only the check's *placement*
is the minimal-now part.

## Testing

- **card-dsl:** serde round-trips for `Effect::Heal` and `Uses { discard_when_empty }`.
- **pipeline:** `parse` test — First Aid's text → `discard_when_empty: true`,
  Flashlight's → `false`.
- **engine unit:** `Heal` reduces `damage`/`horror` saturating at 0 + emits
  `Event::Healed`; `count: 0` no-op; the target-choice reuses `At(Here)`
  (auto-bind on one co-located investigator).
- **engine integration** (`crates/game-core/tests/`, mock registry): an asset with
  `Uses(1 …)` + `discard_when_empty: true` whose last `SpendUses` activation
  discards it (emits `CardDiscarded { InPlay }`); a sibling with the flag `false`
  stays in play at 0 uses.
- **regen sanity:** the `cards.rs` diff is *only* the new `Uses` field (First Aid
  → `true`, Flashlight → `false`).

## Out of scope (deferred)

- The First Aid / Medical Texts *cards* (PR-4 #239 / PR-6 #321).
- The `DealDamage`+`DealHorror` consolidation (#354).
- Rules-precise post-resolution discard timing + effect-depletion cards
  (Forbidden Knowledge, Grotesque Statue) — #353.

## Dependencies

- The keystone (#349): `InvestigatorTarget::Chosen` / `EntityScope::At` /
  `ChooseOne` for `Heal`'s targeting and First Aid's kind choice.
- `Cost::DiscardSelf` (#301) — the in-play-asset discard logic the
  `discard_card_from_play` helper is extracted from.
- The card-data pipeline (`parse_uses`, `uses_lit`) and the regen step.

## What "done" looks like

`Effect::Heal` reduces an investigator's damage/horror (saturating, `Event::Healed`)
with a reusable `HarmKind`; a `discard_when_empty` asset auto-discards when its
uses deplete via a spend; the corpus is regenerated with the new flag. First Aid's
content (PR-4) and Medical Texts' heal (PR-6) are then unblocked. Full strict
gauntlet green.
