# Resource + Engage basic actions (+ Draw AoO fix)

Phase 7 — solo correctness gate, Tier-1 item A (missing basic actions).
Closes #141 (Resource) and #77 (Engage's basic-action half; Parley defers to
#258). Folds in a fix to the pre-existing Draw attack-of-opportunity gap found
during design.

## Background

The Rules Reference (vendored PDF, p.4/p.5) lists the **basic actions**:
*Draw, Resource, Move, Investigate, Fight, Engage, Evade*. The engine's
`PlayerAction` enum has Draw, Move, Investigate, Fight, Evade — but **no
Resource and no Engage**, so a solo player cannot take either. This adds them.

**Attack-of-opportunity rule (RR p.5, verbatim):** "Each time an investigator
is engaged with one or more ready enemies and takes an action other than to
fight, to evade, or to activate a parley or resign ability, each of those
enemies makes an attack of opportunity against the investigator, in the order
of the investigator's choosing." So the exempt actions are **fight / evade /
parley / resign only** — Draw, Resource, Move, Investigate, and **Engage** all
provoke an AoO. Today Move and Investigate fire `fire_attacks_of_opportunity`;
Draw does **not** (a latent gap); a comment in `actions.rs` wrongly lists Engage
as exempt.

## Scope

In: the Resource and Engage actions, both firing AoO; the Draw AoO fix; the
stale exempt-list comment. Out: the AoO *reaction windows* (soak/cancel) — the
keystone cluster (#293/#379/#361/#378) upgrades **all** AoO sites uniformly
later; this work uses the existing window-less `fire_attacks_of_opportunity` and
stays consistent with Move/Investigate. Also out: Engage's multiplayer
"engage an enemy engaged with another investigator" clause (single
`Enemy.engaged_with` field; latent in 1p) and Parley/Resign (card/location-
granted action types, not basic actions → #258).

## Design

Both handlers live in `engine/dispatch/actions.rs` and follow the established
**validate-first / mutate-second** basic-action shape (cf. `move_action`,
`investigate`), reusing `spend_one_action` and `combat::fire_attacks_of_opportunity`.

### Resource action (#141)

`PlayerAction::Resource { investigator }`.

- **Validate** (all preconditions before any mutation): phase is
  `Investigation`; `investigator` is the active investigator; status is
  `Active`; `actions_remaining >= 1`.
- **Mutate**: `spend_one_action` → `fire_attacks_of_opportunity(cx, investigator)`
  (Resource is not exempt) → **if the investigator is still `Active`** (AoO can
  defeat/eliminate them), `resources = resources.saturating_add(1)` and emit
  `Event::ResourcesGained { investigator, amount: 1 }`. If AoO eliminated them,
  the resource is not gained — the spent action + AoO events stay (mirrors the
  Move/Investigate "primary effect suppressed by AoO" guard).

### Engage action (#77)

`PlayerAction::Engage { investigator, enemy }`.

- **Validate**: phase is `Investigation`; `investigator` is active; status is
  `Active`; `actions_remaining >= 1`; `enemy` exists in state; the enemy is at
  the investigator's `current_location`; the investigator is **not already
  engaged** with that enemy (`enemy.engaged_with != Some(investigator)` — RR:
  "cannot engage an enemy he or she is already engaged with").
- **Mutate**: `spend_one_action` →
  `fire_attacks_of_opportunity(cx, investigator)` (from *other* already-engaged
  ready enemies; the target is not engaged yet, so it is correctly not in the
  AoO set) → **if still `Active`**, set `enemy.engaged_with = Some(investigator)`
  and emit `Event::EnemyEngaged { … }`. AoO-elimination suppresses the
  engagement, same guard as Resource.

### Draw AoO fix (folded in)

Add `fire_attacks_of_opportunity(cx, investigator)` to the `Draw` handler
(`engine/dispatch/cards.rs`), after `spend_one_action` and before the card is
drawn, with the same "if still `Active`" guard around the draw. Draw is not
AoO-exempt; this brings it in line with Move/Investigate and the two new
actions.

### Comment fix

Correct the `actions.rs` exempt-list comment (currently "only Fight, Evade,
Parley, Engage, Resign are [exempt]") to the rules-accurate set: **fight /
evade / parley / resign**.

## Dispatch wiring

Add the two variants to `PlayerAction` (`action.rs`, with doc-comments stating
the validation + AoO behavior), and two arms to the `apply_player_action`
match (`engine/dispatch/mod.rs`) delegating to the new `actions.rs` handlers.
`#[non_exhaustive]` is already on the enum.

## Testing

Engine unit tests (`actions.rs` `#[cfg(test)]`) using the `TestGame` builder +
event-assertion macros:

- **Resource**: happy path (action spent, +1 resource, `ResourcesGained`
  emitted); rejections — wrong phase, not active investigator, not `Active`
  status, `actions_remaining == 0`; AoO path — engaged with a ready enemy →
  AoO fires (damage dealt) and the resource is still gained when the
  investigator survives.
- **Engage**: happy path (action spent, target `engaged_with` set,
  `EnemyEngaged` emitted); rejections — wrong phase / not active / not `Active`
  / no actions / enemy not in state / enemy not at the investigator's location /
  already engaged with that enemy; AoO path — a *second* already-engaged ready
  enemy makes an AoO when the first is engaged (2-enemy fixture), and the target
  enemy does **not** AoO (it wasn't engaged yet).
- **Draw**: extend the existing Draw tests with an AoO case (engaged ready enemy
  → AoO fires on Draw).

No integration test needed (no card data); these are pure engine actions.

## Out-of-scope follow-ups (already tracked / noted)

- AoO **reaction windows** (Guard Dog soak, Dodge cancel) on these sites —
  keystone #293/#379; this work uses the window-less mechanism, consistent with
  the existing AoO callers.
- **Engage attack-order** when multiple other enemies are engaged — RR "in the
  order of the investigator's choosing" is #143 (player-picked AoO order);
  today `fire_attacks_of_opportunity` uses a fixed deterministic order.
- Engage's **multiplayer** clause (engage an enemy engaged with another
  investigator) — needs multi-investigator engagement, deferred.
