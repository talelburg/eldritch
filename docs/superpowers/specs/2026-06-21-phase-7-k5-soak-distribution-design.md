# Phase 7 keystone — K5: player damage/horror soak distribution (#44) — design

Tracking: **#44**, the **K5** (final) sub-slice of the keystone attack-loop arc
(`2026-06-20-phase-7-keystone-mid-action-park-design.md`). K1–K4 shipped the
attack-loop park/resume substrate and the player's attack-order pick. K5 gives
the **defending player the choice of how to distribute** an attack's (and a
treachery's) damage/horror across their soak-bearing assets and themselves
(RR p.7), replacing today's deterministic soak-first stand-in, and **routes
non-attack damage/horror through the same soak pipeline** so treachery
damage/horror soaks too.

## Scope (verified 2026-06-21)

The keystone spec lumped three things into K5; on inspection only two are in the
1-player Gathering gate.

**IN — two pieces:**

1. **Interactive distribution.** `assign_attack` is **soak-first deterministic**
   (`TODO(#44)`): it auto-fills soakers before the investigator. The rules grant
   the *defending player* the choice (RR p.7: damage/horror is *assigned* by the
   player, then *placed simultaneously*, then defeat is checked). Soak-first can
   force a bad assignment — e.g. spending Guard Dog's last point of health when
   the player would rather take the hit. Replace it with a player choice.

2. **Non-attack damage/horror soak.** Only `enemy_attack` routes through the soak
   pipeline (`build_soakers → assign_attack → place_assignment`). Direct
   damage/horror from card/treachery effects routes through
   `elimination::take_damage` / `take_horror` → `apply_damage_numeric` /
   `apply_horror_numeric` **straight to the investigator, no soak**. **Grasping
   Hands (damage) and Rotting Remains (horror) are implemented Gathering
   treacheries** on that path, so a Roland holding Guard Dog / Beat Cop silently
   doesn't soak their harm — a real 1p-Gathering correctness gap.

**DEFERRED — the multi-soak-window drain.** The keystone spec assumed interactive
distribution makes the multi-window-per-attack case reachable. **It does not.**
Guard Dog 01021 is the **only** card with the `EnemyAttackDamagedSelf` soak
reaction, and it is an **Ally** — the single Ally slot forbids two copies. So an
attack opens **≤1 soak reaction window regardless of distribution**: spreading
damage onto Guard Dog *and* Beat Cop still yields one window (only Guard Dog
reacts). A second window needs a second *reactor*, not a second damaged soaker.
The `park_on_soak_window` `debug_assert` (single-window-per-attack) therefore
**stays**; the multi-window drain is genuinely unconstructible in scope and is
deferred (its avenues remain #294: a second reactor, or Charisma + a second
reactor). This spec updates the keystone spec's K5 note to record this.

## Decomposition: K5a (shared entry) → K5b (interactive)

Two behaviour-green sub-PRs (the K1–K4 cadence).

### K5a — route all damage/horror through one soak entry (additive)

Extract the soak pipeline into a single entry both paths call:

```rust
/// Distribute `damage` + `horror` to `investigator` across eligible soakers
/// then self (soak-first, RR p.7), place simultaneously, defeat overflow.
/// Returns the damaged surviving soaker assets (for the caller to queue soak
/// reaction windows). Non-attack callers pass one of `damage`/`horror` as 0 and
/// ignore the return (treachery harm opens no soak reaction window — Guard Dog
/// retaliates only to enemy *attacks*).
fn soak_and_place(cx, investigator, damage, horror) -> Vec<CardInstanceId>;
//   = build_soakers + assign_attack + place_assignment   (today's enemy_attack body)
```

- **`enemy_attack`** becomes a thin wrapper: read `attack_damage`/`attack_horror`,
  call `soak_and_place`, return survivors (caller queues windows — unchanged).
- **`take_damage(cx, inv, n)`** → `soak_and_place(cx, inv, n, 0)`, ignore survivors.
- **`take_horror(cx, inv, n)`** → `soak_and_place(cx, inv, 0, n)`, ignore survivors.

`place_assignment` already applies investigator defeat (`dmg_lethal || hor_lethal
→ apply_investigator_defeat`, cause `Damage` else `Horror`) and asset defeat, so
the `take_*` defeat behaviour is preserved exactly (damage-only → cause `Damage`
iff lethal; horror-only → cause `Horror` iff lethal). **Behaviour change is purely
additive:** non-attack harm now soaks onto eligible assets via the soak-first
default; attacks are byte-identical; the no-soaker case (no registry / no
soak-bearing asset) drops everything on the investigator exactly as before. **No
suspension in K5a** — soak-first stays deterministic, so `take_*` keep their `()`
return and signatures. This closes the non-attack-soak gap on its own.

All `take_damage` / `take_horror` callers (the evaluator's `deal_effect`, Crypt
Chill's native no-asset branch via the `game_core::take_damage` re-export,
`cards.rs`' direct `take_horror`) pick up soak for free.

### K5b — the player chooses the distribution

Replace the soak-first body of `soak_and_place` with an interactive per-point
assignment (Approach A), surfaced as `PickSingle` prompts — the faithful RR p.7
model and the same substrate K4 used.

#### When to prompt (the gate)

Prompt **only when there is a genuine choice**: `build_soakers` returns ≥1 soaker
with remaining capacity for a harm type that has ≥1 point to place. Otherwise
(no soaker, or all soakers full) every point has exactly one destination — the
investigator — so assign deterministically with **no** `AwaitingInput`
(behaviour-preserving for the overwhelmingly common no-soaker case, mirroring
K4's "single attacker never prompts").

#### Per-point assignment

Each point is one `PickSingle` over the eligible targets *for that harm type*:

- a **damage** point: `{ investigator } ∪ { health-soakers with remaining health }`;
- a **horror** point: `{ investigator } ∪ { sanity-soakers with remaining sanity }`.

The investigator is **always** eligible (a player may always choose to take harm
themselves, up to and including lethal — defeat is checked at *placement*, not
*assignment*). A soaker drops out of later prompts once its remaining capacity
hits 0. Damage points are assigned first, then horror points (deterministic
order; placement is simultaneous regardless). N damage + M horror → N+M prompts
when soakers are present (1–2 in scope).

#### Simultaneity (RR p.7)

The picks **only build the `Assignment`** — nothing is placed until every point
is assigned. When the last point lands, run the existing **`place_assignment`
once** (simultaneous placement → investigator defeat → asset defeat), exactly as
today. So per-point *assignment* with a single end-of-distribution *placement*
preserves the "placed simultaneously, then defeat" semantics. `place_assignment`
itself is unchanged.

#### The suspension frame

A new `Continuation::DamageAssignment` holds the in-progress distribution:

```rust
DamageAssignment {
    investigator: InvestigatorId,
    remaining_damage: u8,        // damage points still to assign
    remaining_horror: u8,        // horror points still to assign
    assignment: Assignment,      // accumulates picks; placed when both reach 0
    source: DamageSource,        // how to resume after placement
}

enum DamageSource {
    /// An enemy attack (K1–K4 attack loop). The `AttackLoop` frame holding the
    /// loop context is parked *beneath* this one; on placement, queue soak
    /// windows for survivors and continue the attack loop.
    EnemyAttack { enemy: EnemyId },
    /// A card/treachery effect (the evaluator's `deal_effect`). No window; on
    /// placement, return `Done` so the effect walk continues.
    Effect,
}
```

The frame carries the current soaker capacities **derived live** from
`assignment` + a fresh `build_soakers` on each prompt (no stored capacity
snapshot — re-reading keeps it consistent with any mid-distribution state change,
and the eligible set is small). The `PickSingle` options are `candidate_options`
over `[investigator-as-target, soaker₁, soaker₂, …]` (reusing K4's helper); the
resume reads the picked `OptionId`, credits one point to that target in
`assignment`, decrements `remaining_*`, and either re-prompts or — when both
counters hit 0 — places and resumes by `source`. Invalid pick (out-of-range /
wrong variant) rejects and **leaves the frame** for retry (the K4 / HunterMove
contract). `resolve_input` routes `DamageAssignment` to `resume_damage_assignment`.

#### Resume routing (the two sources)

This is the one structural subtlety — the distribution suspends from two
contexts, so it resumes to two places (the `EnemyAttackSource` pattern):

- **`Effect`** — `deal_effect` called `soak_and_place`, which suspended. On
  completion the resume places and returns `Done`; the evaluator's effect walk
  continues (the next `Seq` step, etc.) through its existing suspend/resume.
  `deal_effect` already returns `EngineOutcome`, so threading `AwaitingInput`
  through it is the existing effect-suspension path — no new evaluator surface.

- **`EnemyAttack`** — the distribution suspends from inside the attack-dealing
  step (`process_attacker_dealing` → `enemy_attack` → `soak_and_place`). The
  **`AttackLoop` frame is parked beneath** the `DamageAssignment` frame (carrying
  remaining attackers / source / a new `AttackLoopStage::AwaitingAssignment`), so
  the loop context survives. On the distribution's completion the resume:
  (1) runs `place_assignment` → survivors; (2) queues a soak reaction window per
  survivor (today's `process_attacker_dealing` tail); (3) hands back to
  `resume_enemy_attack` — if a window opened, the loop is now parked on it
  (`AfterSoak`) and `AwaitingInput` returns; else the loop drains the rest. The
  "place + queue windows" tail that runs synchronously in `process_attacker_dealing`
  today **moves into this resume** for the suspending case (and stays inline for
  the no-prompt case). This mirrors how K4's order pick parks the `AttackLoop`
  beneath the prompt and resumes the per-attacker body.

#### Signature change

`soak_and_place` becomes suspendable: it returns `EngineOutcome` (or an
internal "suspended vs. survivors" result), not `Vec<survivors>` directly,
because survivors aren't known until placement, which may be post-resume.
Consequently in K5b the `take_*` wrappers also return `EngineOutcome`
(propagating a possible `AwaitingInput`), and **every `take_*` caller must sit in
a suspendable context**: `deal_effect` already returns `EngineOutcome` (its
`HarmKind` arms just forward `take_*`'s outcome); the two card-local callers
(Crypt Chill's native no-asset branch via the `game_core::take_damage`
re-export, and `cards.rs`' direct `take_horror`) must be confirmed to propagate —
a prerequisite to verify at implementation (mirrors K3's "confirm the play-card
charge site" check), and in the common no-soaker case they return `Done` so
nothing changes for them. `enemy_attack`'s caller moves its window-queuing into
the shared resume tail (above). This is the K5b blast radius — contained to
`combat.rs` (the attack tail) + `elimination.rs` (the `take_*` wrappers) +
`evaluator.rs` (forwarding `deal_effect`'s outcome) + `dispatch/mod.rs`
(`resolve_input` routing) + a new `resume_damage_assignment`.

## Testing strategy

**K5a (additive soak entry):**
- Non-attack damage soaks: a `take_damage` with a health-soaker in play accrues
  on the asset, not the investigator (registry-backed; Guard Dog / Beat Cop).
- Non-attack horror soaks: a `take_horror` with a sanity-soaker (Beat Cop) accrues
  on the asset.
- Defeat preserved: `take_damage` ≥ max_health still defeats with cause `Damage`;
  `take_horror` ≥ max_sanity with cause `Horror` (existing elimination tests stay
  green).
- Attack path byte-identical: existing `enemy_attack` / Guard Dog / soak tests
  unchanged.
- A real treachery: Grasping Hands damage / Rotting Remains horror soaks onto a
  controlled asset (registry-backed integration test).

**K5b (interactive distribution):**
- No-soaker / soakers-full → no prompt, all to investigator (behaviour-preserving;
  existing tests green).
- 2-damage attack with Guard Dog (3 health): two `PickSingle` prompts; choosing
  `{Guard Dog, investigator}` places 1 + 1; choosing both on the investigator
  leaves Guard Dog untouched (proves the player may decline to soak).
- Capacity decrement: a soaker with 1 remaining health drops out of the second
  prompt once filled.
- Simultaneity / defeat: a distribution that brings the investigator to lethal
  defeats *after* all points place (one `InvestigatorDefeated`).
- Soak reaction still fires: damage assigned onto Guard Dog opens its retaliate
  window after placement (attack source); the same assignment via a *treachery*
  opens **no** window (Effect source).
- Invalid pick rejects + retains the `DamageAssignment` frame.
- The attack-order (K4) × distribution (K5) interaction: a 2-enemy attack picks
  order, then each attacker's damage distributes — both suspensions sequence
  cleanly.
- Full native gauntlet + wasm jobs green.

## Open questions / deferrals

- **Multi-soak-window drain — deferred** (see Scope). The `park_on_soak_window`
  single-window `debug_assert` stays; reachable only via #294's avenues.
- **A single effect dealing both damage and horror** (two `take_*` calls in a
  `Seq`) would yield two separate distribution prompts. No in-scope card does this
  (treacheries deal one or the other; enemy attacks deal both via the *attack*
  path, one combined distribution). Deferred — note it; revisit if a card needs
  combined non-attack damage+horror placed as one dealing.
- **Trauma choice on simultaneous damage+horror defeat** (RR p.6 "chooses which
  type of trauma") stays the deterministic `DefeatCause::Damage` placeholder
  (out of scope, tracked on `enemy_attack`'s doc since #83).
- **#119 helper consolidation** (`deal_damage`/`deal_horror` dispatchers) is a
  separate post-K5 cleanup — `soak_and_place` is the natural place those land, but
  K5 doesn't need them.

## What "done" looks like

A defending player distributes each point of an attack's — or a treachery's —
damage/horror across their soak-bearing assets and themselves (RR p.7), one point
at a time, declining to soak if they choose; the assignment places simultaneously
and defeat is checked after; treachery damage/horror soaks where enemy attacks
already did; and the soak reaction (Guard Dog) still fires on attack-source harm.
This closes #44 and **completes the keystone arc (K1→K5)** — leaving the
multi-window drain (#294) and the #119 helper cleanup as deferred follow-ups.
