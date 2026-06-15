# Phase 7 Slice 1 — C5b: enemy-attack damage soak + Guard Dog reaction

**Issue:** [#237](https://github.com/talelburg/eldritch/issues/237) (C5b).
**Phase doc:** `docs/phases/phase-7-the-gathering.md`.
**Predecessors:** A2 reaction machinery (#219), C5a interrupt/suspension
precedents (#236, PR #291).
**Related:** #44 (DSL horror-soak / damage-redirect primitive) — this PR
implements the *core* of #44 and shrinks its remaining scope to the
interactive distribution choice (see "Closing task").

## Problem

Guard Dog 01021's verbatim text is:

> **[reaction]** When an enemy attack deals damage to Guard Dog: Deal 1
> damage to the attacking enemy.

Issue #237 was scoped on a misremembered trigger ("after an enemy deals
damage to **you**"). The real trigger fires only when an enemy attack's
damage is **assigned to the Guard Dog asset** — i.e. the damage-soak /
damage-assignment mechanic, which is unimplemented (tracked in #44).
Today `enemy_attack` → `apply_damage_numeric` dumps all attack damage
straight onto `inv.damage`; nothing can ever land on an ally, so a
faithful Guard Dog can never fire.

`CardInPlay` already carries unused `accumulated_damage` /
`accumulated_horror` fields, added in anticipation of this mechanic.

**Decision (settled in brainstorming):** implement the faithful card by
pulling the soak mechanic into C5b, rather than re-pointing the window to
"damage to you" (which would diverge from card text) or shipping a
window that can never fire.

## Scope

Three inseparable deliverables:

1. **Core soak mechanic** — enemy-attack damage/horror is *assigned*
   across the defending investigator + their soak-bearing assets,
   *placed* simultaneously, then *defeat-checked* (RR p.7).
2. **A new reaction window** — "an enemy attack dealt damage to a
   controlled asset."
3. **Guard Dog 01021's card impl** — the window's sole consumer. The
   other five Guardian L0 assets (.45 Automatic, Physical Training, Beat
   Cop, First Aid, Machete) listed under C5d (#239) stay there; only
   Guard Dog moves into C5b, because it is the sole consumer of the new
   window and is trivial to implement and test alongside it. **This
   overlap with C5d is intentional** — note it when picking up C5d.

Both attack sites are in scope: enemy-phase attacks
(`resolve_attacks_for_investigator`) and attacks of opportunity
(`fire_attacks_of_opportunity`). The suspend/resume helper is shared, so
the marginal cost of the second site is small.

## RR p.7 grounding

> *"Any assigned damage/horror that has not been prevented is now placed
> on each card to which it has been assigned, simultaneously. … After
> applying damage/horror, if an investigator has damage equal to or
> higher than his or her health or horror equal to or higher than his or
> her sanity, he or she is defeated."*

Assignment → simultaneous placement → defeat check is three distinct
steps. The reaction window opens after the damage is *placed* on the
asset.

## Architecture

The attack pipeline gains one swappable step (assignment); everything
downstream is written once and does not change when interactivity lands:

```
enemy_attack(enemy, investigator)
  ├─ assign_attack()       ← deterministic now (soak-first); TODO(#44) interactive
  ├─ place simultaneously  (investigator.damage/horror + asset.accumulated_*)
  ├─ defeat checks         (investigator unchanged; NEW: asset defeat on overflow)
  └─ for each asset that took damage: queue reaction window
```

### 1. Assignment (deterministic now)

`assign_attack(state, enemy, investigator) -> Assignment`, where
`Assignment` is a `{target → (damage, horror)}` map (`target` is the
investigator or a controlled `CardInstanceId`).

**Soak-first ordering:** fill eligible soak-bearing assets (ordered by
`CardInstanceId`, matching the codebase's other simultaneous loops) up to
each one's remaining capacity, then the investigator absorbs the
remainder.

- **Damage eligibility:** controlled assets whose printed metadata is
  `CardKind::Asset { health: Some(h), .. }` with `accumulated_damage < h`.
- **Horror eligibility:** symmetric on `sanity` / `accumulated_horror`.

**Caveat (accepted):** soak-first is the *only* deterministic default
that makes Guard Dog observable — investigator-first would render the
reaction dead code. It means the engine auto-feeds every attack to allies
until they die, which is not always what a human would choose; acceptable
as a deterministic stand-in, consistent with the `ChooseOne` stub and
agenda 01105's deterministic branch.

`TODO(#44)`: replace the body with a parked window that surfaces eligible
sources and accepts the whole `{target → points}` distribution in one new
`InputResponse` variant, validated for totals + per-target capacity,
feeding the identical placement path below.

### 2. Placement + defeat

Place all assigned damage and horror **simultaneously** (both the
investigator's stats and each asset's `accumulated_*` update before any
defeat check), then check defeats:

- **Investigator defeat** — unchanged (`apply_investigator_defeat`).
- **Asset defeat (new)** — an asset with `accumulated_damage >= printed
  health` (or `accumulated_horror >= printed sanity`) is defeated:
  removed from `cards_in_play` and discarded, emitting `CardDiscarded`.

### 3. Reaction window + Guard Dog

- **`WindowKind::AfterEnemyAttackDamagedAsset { asset: CardInstanceId,
  enemy: EnemyId, controller: InvestigatorId }`** — queued via the
  existing `queue_reaction_window` (mirror of `AfterEnemyDefeated`), once
  per asset that took damage in the attack.
- **`EventPattern::EnemyAttackDamagedSelf`** — new, **bare** (no
  narrowing fields). The engine binds *self* = the soaked asset, the way
  `EnteredLocation` / `EndOfTurn` bind their context. Window-only:
  `trigger_matches` returns `false` in the general reaction pipeline (like
  `WouldDiscoverClues` / the forced-only patterns).
- **`EvalContext.attacking_enemy: Option<EnemyId>`** — new, set only
  while resolving this reaction (mirror of `failed_by` /
  `clue_discovery_count`).
- **Guard Dog impl** — a single `reaction(EnemyAttackDamagedSelf)`
  ability whose effect is `Effect::Native("01021:retaliate")`, calling
  `combat::damage_enemy(attacking_enemy, 1, Some(controller))`. Native
  (not a new `DealDamage`-to-enemy DSL primitive) because Guard Dog is the
  first card to deal damage to a specific enemy from a reaction — per the
  "don't add DSL primitives speculatively" rule. A shared variant lands if
  a second card wants the pattern.

The reaction is optional (a `[reaction]`): the window suspends with
`AwaitingInput`, and the player chooses whether to fire.

### 4. Resumable attack loop

This is the suspension `combat.rs` already predicted:

> *"The first PR that adds a reaction `EventPattern` matching events
> emitted inside this loop … must persist the remaining-attackers list on
> `GameState` … so resume-after-pause re-enters the right iteration
> point."*

When the reaction window suspends, park
`pending_enemy_attack { investigator, remaining_attackers: Vec<EnemyId>,
source: EnemyAttackSource }` on `GameState` (same shape as
`pending_end_turn` / `spawn_engage_pending`). After the window closes, a
`resume_enemy_attack` continuation re-enters the loop at the next
attacker. `EnemyAttackSource` distinguishes the two call sites
(`EnemyPhase` vs `AttackOfOpportunity`) so resume returns to the right
driver. Resolved through `resolve_input`, routed alongside the existing
pending-window resumes.

## State additions (summary)

| Field / variant | Where | Purpose |
|---|---|---|
| `WindowKind::AfterEnemyAttackDamagedAsset { asset, enemy, controller }` | `state` | the reaction window |
| `EventPattern::EnemyAttackDamagedSelf` | `card-dsl` | the trigger (bare, window-only) |
| `EvalContext.attacking_enemy: Option<EnemyId>` | engine | binds "the attacking enemy" for the Native effect |
| `GameState.pending_enemy_attack: Option<PendingEnemyAttack>` | `state` | suspend/resume the attack loop |
| `accumulated_damage` / `accumulated_horror` | `CardInPlay` | already exist; now read/written |

## Testing

1. **Engine unit tests** (`combat.rs` / reaction-window modules):
   - assignment soak-first fills an eligible asset before the
     investigator, ordered by `CardInstanceId`;
   - damage + horror place simultaneously (both land before defeat);
   - asset defeat on overflow → `CardDiscarded`, removed from
     `cards_in_play`;
   - the window opens once per damaged asset;
   - suspend mid-loop and resume re-enters at the correct next attacker
     (multi-attacker case), for both `EnemyPhase` and AoO sources.
2. **Guard Dog card test** (`crates/cards/src/impls/guard_dog.rs`): the
   reaction deals 1 damage to the attacking enemy.
3. **Integration test** (`crates/cards/tests/`): end-to-end — an enemy
   attacks an investigator controlling Guard Dog, damage soaks onto Guard
   Dog, the window fires, the attacking enemy takes 1 damage.

## Closing task (post-merge)

Reframe #44: its remaining scope collapses to the **interactive
distribution choice** (the `TODO(#44)` in assignment — surfacing eligible
soak sources and accepting a player-chosen `{target → points}`
distribution). The placement, asset-defeat, and symmetric horror-soak
mechanics ship here.

## Decisions made (for the phase doc, on merge)

- Faithful soak pulled into C5b rather than re-pointing the window to
  "damage to you"; soak-first deterministic assignment now, interactive
  distribution deferred to a reframed #44.
- Symmetric damage + horror soak (the assign/place/defeat logic is
  identical parameterized by `(stat, accumulator)`; asymmetric
  special-casing would be more code, not less).
- Guard Dog's "deal 1 damage to the attacking enemy" ships as
  `Effect::Native`, not a new DSL primitive (first consumer).
- Guard Dog's card impl ships in C5b despite C5d (#239) listing it — it
  is the new window's sole consumer.
