# C5c — Roland's signature (.38 Special) + weakness (Cover Up)

Phase 7, Slice 1, Group C. Issues: **#295** (engine prereq), **#238** (C5c card content).

## Goal

Ship Roland Banks' signature/weakness pair for The Gathering:

- **Roland's .38 Special 01006** — `Roland Banks deck only. Uses (4 ammo). [action] Spend 1 ammo: Fight. You get +1 [combat] for this attack (if there are 1 or more clues on your location, you get +3 [combat], instead). This attack deals +1 damage.`
- **Cover Up 01007** — `Revelation - Put Cover Up into play in your threat area, with 3 clues on it. [reaction] When you would discover 1 or more clues at your location: Discard that many clues from Cover Up instead. Forced - When the game ends, if there are any clues on Cover Up: You suffer 1 mental trauma.`

(Card text verified against `data/arkhamdb-snapshot/pack/core/core.json` on 2026-06-15.)

The work splits along the engine/content seam, following the #276 / #286 prerequisite pattern.

## Decomposition

- **PR 1 — engine prereq (#295, `engine`/`infra`):** weapon support — ammo/uses + an inspectable `Effect::Fight`. No card content.
- **PR 2 — C5c card content (#238, `card`):** the two card impls + tests, plus the tiny `PutIntoThreatArea { clues }` extension Cover Up needs. Rides on PR 1.

## PR 1 — engine prereq (#295)

The full design lives in #295. Summary of the surface it adds:

1. **Ammo / Uses — pipeline-parsed.** `Uses (N <kind>)` parsed from asset text →
   `CardKind::Asset { uses: Option<Uses { kind: UsesKind, count: u8 }>, … }`.
   `CardInstance.uses_remaining: Option<u8>` initialized from metadata on
   asset enter-play. New `Cost::SpendUses(u8)` validated/paid in the
   activation flow, emitting `Event::UsesSpent`. Generalizes to every
   future uses-card.

2. **Inspectable `Effect::Fight { combat_modifier: IntExpr, extra_damage: u8 }`.**
   Resolves the modifier against current state, auto-targets the single
   engaged enemy, starts a Combat skill test reusing the existing
   suspend/resume commit-window path.

3. **`IntExpr` — `Lit(i8)` + `Cond { when: Condition, then: i8, otherwise: i8 }`.**
   A value-level conditional resolved at eval, so the modifier reads as
   `IntExpr::cond(LocationHasClues, 3, 1)` rather than an `Effect::If`
   duplicating the whole `fight(...)` node. Condition-agnostic — the
   minimum generalization this card forces.

4. **`Condition::LocationHasClues`** — already pre-named in the
   `Condition` doc comments; ≥1 clue on the controller's location.

5. **This-test modifier** — `InFlightSkillTest.test_modifier: i8`, applied
   at total computation; `0` for player-action tests.

6. **Parameterized Fight follow-up** — `SkillTestFollowUp::Fight { enemy,
   extra_damage: u8 }` deals `1 + extra_damage`; stays `Copy`.

7. **Validate-first target check** — `check_activate_ability` rejects, before
   charging action or ammo, when an `Effect::Fight` ability is fired without
   exactly one engaged enemy (`0` → illegal; `2+` → TODO, deferred with the
   #212/#213 interactive-choice cluster).

Base `PlayerAction::Fight` is unchanged (modifier 0, deals 1) — covered by a
regression test.

## PR 2 — C5c card content (#238)

### Roland's .38 Special 01006 (`crates/cards/src/impls/roland_38_special.rs`)

(Player-card impls are named by card name — `holy_rosary`, `magnifying_glass`,
`guard_dog` — not code; only treacheries use the `treachery_NNNNN` form.)

A single activated ability; ammo (4) comes from the corpus via PR 1's
pipeline parse, so it is **not** hand-typed here:

```rust
activated(
    1,                                  // [action]
    vec![Cost::SpendUses(1)],           // Spend 1 ammo
    fight(
        IntExpr::cond(Condition::LocationHasClues, 3, 1),  // +1, or +3 instead if clues
        1,                                                 // +1 damage
    ),
)
```

Card tests:
- **+3 path** — a clue on the investigator's location: the in-flight test's
  `test_modifier` is 3; a Combat total that beats fight by the right margin;
  the Fight follow-up deals 2 damage.
- **+1 path** — no clues: modifier 1.
- **ammo** — `uses_remaining` decrements per activation; activation rejects
  when ammo is 0 (`Cost::SpendUses` unpayable).

### Cover Up 01007 (`crates/cards/src/impls/treachery_01007.rs`)

A port of the synthetic Cover Up that C5a (#236) already built and proved
(`crates/scenarios/src/test_fixtures/synth_cards.rs`): its
`WouldDiscoverClues` before-timing interrupt, the `clue_interrupt_pending`
suspension, `clue_discovery_count` threading, the `GameEnd` forced point, and
`Event::TraumaSuffered` all exist. C5c moves the two native effects into the
real card module and adds the placement Revelation:

```rust
vec![
    revelation(put_into_threat_area_with_clues(CODE, 3)),
    on_event(WouldDiscoverClues, Before, native("01007:discard_clues")),
    on_event(GameEnd, After, native("01007:trauma")),
]
```

- **`PutIntoThreatArea { code, clues }` extension** (engine, but tiny and
  Cover-Up-specific, so it lands here not in #295): the effect mints the
  threat-area instance with `clues` already on it. Existing callers
  (Dissonant Voices 01165) pass `0` — the `put_into_threat_area(code)`
  builder keeps that arity; a new `put_into_threat_area_with_clues(code, n)`
  builder serves Cover Up.
- **Native effects** — `01007:discard_clues` (discard the threaded count from
  the source instance instead of discovering) and `01007:trauma` (suffer 1
  mental trauma at game end iff the source still holds clues), registered via
  `treachery_01007::native_effect_for` in `impls/mod.rs`. Direct ports of the
  synth bodies, keyed to the real instance via `EvalContext::source`.

Cover Up is a **persistent treachery** (it has non-Revelation abilities), so
`resolve_encounter_card` does not auto-discard it after revelation — the C4c
disposition rule already handles this; no routing change.

Card tests:
- Revelation puts Cover Up in the threat area with 3 clues.
- Reaction: a clue discovery at the investigator's location discards that many
  clues from Cover Up instead of discovering them.
- Game-end forced: trauma fires iff clues remain (3 clues → trauma; 0 → no
  trauma).

## Testing

- **Engine (#295):** unit tests for ammo (pay/reject), `Effect::Fight`
  (modifier into total, bonus damage, auto-target, 2+-engaged rejection),
  `IntExpr`/`Condition::LocationHasClues` eval, and a `Uses (N kind)` pipeline
  parse test. Base-Fight regression.
- **C5c (#238):** the per-card tests above, plus an integration test in
  `crates/cards/tests/` driving `.38 Special` end-to-end through the real
  registry (ammo → activate → commit → damage). Cover Up's interrupt is
  already integration-shaped from C5a.

## Out of scope / deferred

- **Multi-enemy weapon target selection** — auto-targets the single engaged
  enemy; 2+ engaged rejects with a TODO, landing with the #212/#213
  interactive-choice cluster.
- **A Fight reachable only inside a `Seq`/`If` branch** — `.38 Special` fights
  unconditionally in both `IntExpr` branches; the nested-Fight validate case
  is a documented TODO.
- **Trauma persistence** (campaign log / max-stat reduction) — Phase 9. C5c
  only emits `Event::TraumaSuffered` (as C5a established).

## Open questions

None blocking. The `UsesKind` enum starts with just `Ammo` (the only kind a
Slice-1 card prints); other kinds (Supplies, Charges, Secrets) are added by
the pipeline as cards that print them land.
