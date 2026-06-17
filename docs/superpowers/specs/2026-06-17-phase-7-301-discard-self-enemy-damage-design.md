# PR-2 (#301) — `Cost::DiscardSelf` + enemy choice + `Effect::DealDamageToEnemy`

**Status:** design approved. PR-2 of the
[choice-cluster completion](2026-06-17-phase-7-choice-cluster-completion-decomposition-design.md),
built on the merged keystone
([#349 / PR #351](2026-06-17-phase-7-choice-keystone-design.md)). Tracker issue
[#301](https://github.com/talelburg/eldritch/issues/301). These are Beat Cop's
engine prereqs; the Beat Cop *card* ships in PR-4 (#239), Knife reuses
`Cost::DiscardSelf` in PR-5 (#312).

## Why this exists

Beat Cop 01018 (verbatim, `data/arkhamdb-snapshot/pack/core/core.json`):
`You get +1 [combat].` / `[fast] Discard Beat Cop: Deal 1 damage to an enemy at
your location.` The constant half is already expressible; the fast ability needs
two engine primitives the keystone's choice surface doesn't yet cover: a
discard-the-source-asset **cost**, and a typed **deal-damage-to-a-chosen-enemy**
effect. It is also the first shipped consumer of the keystone's **enemy** variety.

## Components

### ① `Cost::DiscardSelf`

Discards the source in-play asset as an activation cost (distinct from
`Effect::DiscardSelf`, which removes a treachery from a threat area / location).

- **`check_cost_payable`** (abilities.rs): trivially payable — the source is in
  play by the activation precondition (`check_activate_ability` already located
  it in `cards_in_play`). Returns `Ok`.
- **`pay_activation_costs`** (abilities.rs): remove the source from
  `cards_in_play` by `instance_id`, push its `code` to the owner's `discard`, emit
  `Event::CardDiscarded { from: Zone::InPlay }` — the exact pattern
  `combat::defeat_overflowed_assets` uses (combat.rs:245).
- **Ordering constraint (loud guard):** `DiscardSelf` removes the source and thus
  invalidates `in_play_pos` / any later source-referencing cost. It is the **sole**
  source-referencing cost permitted on an ability and is paid **last**. An ability
  pairing it with `Exhaust` or `SpendUses` is rejected in `check_activate_ability`
  (`TODO`: lift if a card ever needs the combo). Both in-scope consumers (Beat
  Cop, Knife) list only `DiscardSelf`.

Consumers: Beat Cop (#239), Knife (#312).

### ② Enemy variety

The keystone's third entity variety, shipped here with its first effect consumer.

- **DSL (`card-dsl`):** `enum EnemyTarget { Chosen(Choose<EntityScope>) }` —
  symmetric with `InvestigatorTarget` / `LocationTarget` (one variant now; a
  non-chosen form like `Engaged` lands with its first consumer). Reuses the
  keystone's `Choose<EntityScope>` / `EntityScope::At(LocationSet)` verbatim.
- **`EvalContext.chosen_enemy: Option<EnemyId>`** — the enemy counterpart of
  `chosen_investigator` / `chosen_location`, bound by target-grounding, `None`
  outside a grounded-choice evaluation.
- **`combat::enemies_in_scope(state, controller, EntityScope) -> Vec<EnemyId>`** —
  the shared enumerator (sorted `BTreeMap`/id order for deterministic replay),
  reachable by both the evaluator (`ground_chosen_targets`) and the activation
  pre-check (abilities.rs). For `At(Here)`: enemies whose `current_location ==`
  the controller's `current_location` (RR "at your location"; *not* engagement —
  `engaged_with` is a separate axis Fight already covers). Empty when the
  controller is between locations.
- **Resolver:** `ground_chosen_targets` grounds the enemy carried by
  `Effect::DealDamageToEnemy`; `resolve_enemy_target` reads `chosen_enemy`,
  mirroring `resolve_investigator_target` / `resolve_location_target`.

### ③ `Effect::DealDamageToEnemy { target: EnemyTarget, amount: u8 }`

Typed/inspectable direct (non-test) damage to a resolved enemy.

- **Handler:** ground the enemy (binds `chosen_enemy`), then
  `combat::deal_damage_to_enemy(cx, enemy, amount, Some(controller))` — the
  existing public entry (Guard Dog #237), so the defeat cascade
  (`EnemyDefeated`, victory, Roland's after-defeat reaction) fires identically to
  a Fight kill. `amount == 0` is a no-op.
- **Pre-cost target check** (the reason it's typed, not `Native`): in
  `check_activate_ability`, mirroring the existing `effect_initiates_fight`
  guard — if the effect is `DealDamageToEnemy` and `enemies_in_scope(...)` is
  **empty**, reject **before** any cost is paid (you cannot discard Beat Cop for
  no legal target). `≥1` proceeds; `2+` suspends via the `Choose` resolver (the
  controller picks). Unlike Fight's "exactly one engaged" auto-target,
  `DealDamageToEnemy` *can* choose among co-located enemies, so the gate is
  "≥1", not "==1".

## Testing

- **`card-dsl`:** serde round-trip for `Cost::DiscardSelf`, `EnemyTarget`,
  `Effect::DealDamageToEnemy`.
- **`game-core` unit (evaluator/abilities/combat):**
  - `DiscardSelf` payment removes the source from `cards_in_play` + emits
    `CardDiscarded { InPlay }`; the illegal `DiscardSelf + Exhaust` combo rejects.
  - `enemies_in_scope` / enemy choice: `At(Here)` filters to co-located enemies;
    auto-binds on 1, suspends on 2+, the pre-check rejects on 0.
  - `DealDamageToEnemy` damages the chosen enemy, attributed to the controller,
    and runs the defeat cascade when it kills.
- **Integration** (`game-core` with `synth_cards::TEST_REGISTRY`, or
  `crates/cards/tests/`): a synthetic `[fast] DiscardSelf → DealDamageToEnemy`
  ability end-to-end — activation rejects with no enemy present, discards the
  source and damages the enemy when one is co-located. (The Beat Cop *card* and
  its own test ship in PR-4.)

## Out of scope (deferred)

- The Beat Cop / Knife *cards* (PR-4 #239 / PR-5 #312).
- `EnemyTarget::Engaged` and other non-chosen enemy forms (no consumer yet —
  Fight uses `single_engaged_enemy`).
- `DiscardSelf` co-existing with other source-referencing costs (loud reject;
  `TODO` until a card needs it).
- `LocationSet::YourOrConnecting` (PR-8 #306).

## Dependencies

- The merged keystone (#349): `Choose<S>`, `LocationSet`, `EntityScope::At`,
  `ground_chosen_targets`, the resolve convention + `Choice` frame.
- `combat::deal_damage_to_enemy` (#237) — the public damage entry.
- The activated-ability cost path (`check_cost_payable` / `pay_activation_costs`
  / `check_activate_ability`) and the `effect_initiates_fight` pre-check
  precedent.

## What "done" looks like

`Cost::DiscardSelf`, `EnemyTarget::Chosen`, and `Effect::DealDamageToEnemy` exist
and are exercised: an activated ability can reject (no enemy in scope, before
paying), discard its source, and deal damage to a co-located enemy (auto-target
on 1, suspend on 2+), with the defeat cascade intact. Beat Cop's content is then
unblocked (PR-4), and Knife's discard-self cost is available (PR-5). Full strict
gauntlet green.
