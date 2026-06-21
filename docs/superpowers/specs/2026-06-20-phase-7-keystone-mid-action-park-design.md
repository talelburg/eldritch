# Phase 7 keystone — mid-action park/resume + the attack-loop arc — design

Tracking: the **keystone** of the unified control-flow model
(`2026-06-20-unified-control-flow-model-design.md`, **#393**), §D. This is
**Ordering step 4** of that spec and **the riskiest slice in Phase 7** — it
collapses six issues into one attack-loop arc:

- **#293** — attacks of opportunity open no soak/cancel window (Guard Dog, Dodge).
- **#379** — Retaliate opens no soak/cancel window (Guard Dog, Dodge).
- **#361** — activated abilities don't provoke AoO (First Aid, Medical Texts, Flashlight).
- **#378** — action-event play doesn't provoke AoO (Dynamite Blast, Emergency Cache).
- **#143** — player picks attack order with 2+ engaged enemies.
- **#44** — player chooses damage/horror distribution across soakers + self.

Plus the refactor pull-in **#119** (unify damage/horror/clue tokens onto
`CardInPlay`), which #44's soak distribution needs for symmetric
investigator/asset token storage.

This spec designs the whole arc as **one coherent mechanism** with an explicit
**sub-PR decomposition (K1→K5)**, each behaviour-preserving-or-additive and
independently green (mirroring the slice-1/2 cadence).

## Why this pass exists

Today every action that provokes an attack of opportunity does so
**synchronously and window-droppingly**. Each basic-action handler runs:

```
validate → spend_one_action → fire_attacks_of_opportunity → if-survived → primary effect
```

and `fire_attacks_of_opportunity` (combat.rs) — together with
`fire_retaliate_if_any` (skill_test.rs) — calls `enemy_attack` **directly**,
bypassing `drive_attack_loop`. It deliberately **drops the damaged-soaker
survivor list** `enemy_attack` returns, so no `AfterEnemyAttackDamagedAsset`
soak window and no `BeforeEnemyAttack` cancel window ever opens. The result:
Guard Dog 01021 does not retaliate against an AoO, and Dodge 01023 cannot cancel
one. combat.rs documents this as the `TODO(#293)` gap.

The fix is structural: **a synchronous mid-handler call can't suspend**, so AoO
can never open a window mid-action. The action must run as a *frame* with a
resume point, so the AoO loop can suspend on a window and the action's primary
effect resumes after the window closes. This is why the keystone is inseparable
from the `InvestigatorTurn` frame (slice 2a-i) and the unified `drive` loop
(slice 1b): the substrate already exists; this slice is its **first and hardest
consumer**.

### The actual AoO-firing sites today (corrected)

The #393 spec §D listed "5 sites — cards.rs:258 (play card), actions.rs:73/121/205/334".
On inspection **cards.rs:258 is the `draw` handler, not `play_card`** — `play_card`
fires **no** AoO today (nor does `activate_ability`). The real AoO-firing sites
are the **five basic actions**:

| Action | Site | AoO call |
|---|---|---|
| Investigate | actions.rs:73 | `fire_attacks_of_opportunity` |
| Resource | actions.rs:121 | `fire_attacks_of_opportunity` |
| Engage | actions.rs:205 | `fire_attacks_of_opportunity` |
| Move | actions.rs:334 | `fire_attacks_of_opportunity` |
| Draw | cards.rs:258 | `fire_attacks_of_opportunity` |

This cleanly separates **K1** (the five basic actions get the frame + windows)
from **K3** (*add* AoO firing to non-fast play-card #378 + action-cost activated
abilities #361 — sites that fire none today). Fight and Evade are AoO-exempt
(RR p.5) and never enter the AoO path at all; **Retaliate** (K2 / #379) fires
from the Fight follow-up (skill_test.rs:443), a different park point — see K2.

## The model

### Stack shape

During an open turn the continuation stack idles as:

```
[ …, InvestigationPhase{TurnBegins}, InvestigatorTurn{who} ]
```

`InvestigatorTurn` is on top (idle in 2a — it accepts typed `PlayerAction`s; it
emits `OptionId`s in 2b). A typed action arrives; its handler validates, spends
the action, and pushes an **`ActionResolution`** frame **above**
`InvestigatorTurn`, then drives the AoO loop. If an engaged ready enemy attacks
and a cancel/soak reaction is available, the loop pushes `AttackLoop` then the
reaction window above it:

```
[ …, InvestigationPhase{TurnBegins}, InvestigatorTurn{who}, ActionResolution{Move}, AttackLoop, Resolution(window) ]
                                                            └─ the action ──────────┘└─ its AoO child ──────────────┘
```

The window is the player-facing top prompt ("play Dodge?"); `AwaitingInput`
returns. When it closes, `resume_enemy_attack` drains the loop and pops
`AttackLoop`; `ActionResolution` is now top; the uniform `drive` loop resumes it;
it re-validates, pops itself, and runs the Move primary effect; `Done` →
`drive` sees `InvestigatorTurn` again → idle. Turn continues.

**Key invariants:**

- `ActionResolution` sits **above** `InvestigatorTurn` (the action belongs to that
  turn); `AttackLoop` is **its child**; windows are the loop's children.
- The frame is **transient**: pushed when an action arrives, popped the moment its
  primary effect runs. Between actions only `InvestigatorTurn` is on top.
- It persists **across an `apply()` boundary only when a window suspends**. In the
  no-window case it is pushed and popped within one `apply()` call, so every
  observable boundary (between player actions) is unchanged — **behaviour-preserving**.

### The frame variant

`ActionResolution` is a **generic frame carrying a resume enum**, not one variant
per action:

```rust
Continuation::ActionResolution {
    investigator: InvestigatorId,
    resume: ActionResume,
}

enum ActionResume {
    Move { destination: LocationId },
    Investigate,
    Resource,
    Engage { enemy: EnemyId },
    Draw,
    // K3 adds:
    // PlayCard { hand_index: u8 },
    // ActivateAbility { instance_id: CardInstanceId, ability_index: u8 },
}
```

**Why generic, not per-action variants** (contrast the per-phase anchors, which
*are* one variant each): the per-phase split exists to make illegal
phase/boundary pairings (e.g. `Mythos` + `AfterAttackLoop`) *unrepresentable*.
There is no analogous illegal action/stage cross-product here — the resume runs
to completion or aborts; it carries no orthogonal stage axis. So the generic
form wins on less enum surface with no unrepresentable-illegal-state payoff
forgone.

**Why the resume re-derives board state rather than snapshotting it.** Each
resume carries only the *parameters of the action* (the destination, the target
enemy), not derived values like the Investigate difficulty. On resume it re-reads
board-dependent values live (location shroud, enemy presence), so a mid-action
board change is reflected — this is the same liveness the re-validation gate
needs, and it keeps the frame minimal and serializable.

### Pop-self-then-run-primary (no stage cursor)

When the AoO loop pops, `ActionResolution`'s resume **pops itself first, then runs
the primary effect**. The primary effect's *own* suspensions (Investigate's skill
test pushes `SkillTest`; a played card's on-play `ChooseOne` pushes `Choice`) are
independent frames that resume through their existing paths. So the action frame
needs **no internal stage cursor** — it bridges exactly the AoO gap and is gone
before the primary effect's sub-resolution begins. The chunk boundary sits
exactly where the synchronous `fire_attacks_of_opportunity` call sits today, so
AoO-vs-primary-effect ordering is preserved by construction.

### The `drive` extension

`drive` (dispatch/mod.rs) today advances only `*Phase` anchors via
`anchor_on_child_pop`. K1 extends its "handle the top frame" rule to
`ActionResolution`: when an `ActionResolution` is on top (whether because the
handler just pushed it with no window, or because the AoO `AttackLoop` just
popped), `drive` runs its resume. `resume_enemy_attack`'s reserved
`EnemyAttackSource::AttackOfOpportunity` arm (currently the unreachable `Done`
stub) becomes reachable: it pops the `AttackLoop` and returns `Done`, unwinding to
`drive`, which then sees `ActionResolution` and resumes it. One code path for
window and no-window cases.

## The re-validation gate (§D "mid-action viability")

Mid-action suspension means the world can change underneath an action: an AoO can
defeat the actor, a retaliate can defeat an enemy, a soak can exhaust a source.
So `ActionResolution`'s resume **re-validates before completing**:

1. **Actor still `Status::Active`** — today's `if inv_after_aoo.status != Active
   { return Done }` check, relocated onto the frame.
2. **The primary effect's own target precondition** — re-run the relevant
   `check_*` predicate (Investigate: location still revealed; Engage: enemy still
   present + not-already-engaged; Move: destination still connected). These are
   the **same shared predicates the 2a-ii `legal_actions` enumerator extracted** —
   the enumerator predicates *are* the re-validation predicates, so no new surface.

On failure the resume **aborts cleanly**: pop self, suppress the primary effect,
return `Done`. The spent action and the AoO/window effects (damage dealt, Dodge
played, retaliate dealt) **persist** — they really happened. This is **not** a
clean whole-action rollback: mid-action abort suppresses only the *primary
effect*, not the AoO side effects. (The apply loop's snapshot-and-rollback on
`Rejected` is a separate, whole-action safety net; the gate is the in-scope
mechanism for *partial* abort.)

**In-scope reachability.** In 1-player Gathering scope the only mid-AoO change
that actually fires is **actor defeat** (no in-scope AoO/retaliate can change a
Move/Investigate/Engage *target* other than by defeating the actor). The full
target re-check is therefore a **structural hook** that is correct-by-construction
today and ready when a card forces a richer invalidation — chosen (over
actor-Active-only) so the gate is uniform and the predicate reuse is total.

## Sub-PR decomposition (K1→K5)

Each step is independently green; behaviour-preserving except where it adds the
documented new player agency.

### K1 — foundation: mid-action park/resume + AoO windows (#293)

The load-bearing, riskiest PR. Scope:

- Add `Continuation::ActionResolution { investigator, resume: ActionResume }` and
  `ActionResume` (the five K1 variants).
- Restructure the five basic-action handlers (Move/Investigate/Resource/Engage/
  Draw) to `validate → spend action → push ActionResolution → drive_attack_loop(
  …, AttackOfOpportunity)`. The post-AoO chunk (the primary effect) moves into the
  frame's resume.
- Make AoO **drive the loop** instead of `fire_attacks_of_opportunity`'s direct
  `enemy_attack`: AoO now queues the cancel/soak windows (stops dropping the
  survivor list). Preserve the **RR p.7 non-exhaust rule** for AoO (the
  `EnemyAttackSource` already distinguishes it; enemy-phase exhausts, AoO does not).
- Wire `resume_enemy_attack`'s `AttackOfOpportunity` arm + extend `drive` to
  resume `ActionResolution` (re-validation gate).
- Update the `resolve_input` defensive arm: an `ActionResolution` frame, like
  `AttackLoop`/`EncounterCard`, never awaits input (it is only ever momentarily
  top inside `drive`), so a `ResolveInput` arriving against it rejects defensively.

**Delivers:** Dodge cancels an AoO; Guard Dog retaliates against an AoO.
**Carries the §D test matrix** (below).

`fire_attacks_of_opportunity` becomes a thin "collect engaged ready attackers →
`drive_attack_loop(AttackOfOpportunity)`" — converging with
`resolve_attacks_for_investigator`'s shape (both snapshot attackers in `EnemyId`
order, then delegate to `drive_attack_loop`), differing only by `source`.

### K2 — Retaliate windows (#379)

Retaliate fires from the **Fight follow-up** (skill_test.rs:443
`fire_retaliate_if_any`), *after* the Fight skill test resolves — **not** at
action declaration. So its park point differs from K1's action frame: the
`AttackLoop{source: Retaliate-or-AoO}` is pushed from inside the skill-test
follow-up resolution, suspending it on the retaliate's soak/cancel window and
resuming the follow-up's tail when the window closes. This needs either a new
`EnemyAttackSource::Retaliate` (to route `resume_enemy_attack` back to the
follow-up) or reuse of the existing AoO source with a distinct resume site — to
be settled in K2's own thin design. Behaviour added: Guard Dog retaliates / Dodge
cancels against a retaliate strike.

### K3 — new AoO sites: action-event play (#378) + activated abilities (#361)

Wire AoO firing into `play_card` (non-fast) and `activate_ability`
(`action_cost > 0`), gated so **fast plays remain exempt** (RR p.11 — fast
events/abilities are not actions). Both reuse K1's `ActionResolution` frame with
the two new `ActionResume` variants. The chunk boundary mirrors the basic
actions: spend cost → fire AoO (suspendable) → on resume, run the card's on-play
effects / the ability's effect. **K3 must first confirm where the non-fast
play-card action cost is charged** — `play_card` does not call `spend_one_action`
in its body today (the cost handling is not visible there); the AoO must fire
after the action cost is paid, so this is a prerequisite to verify, not assume.

### K4 — player attack-order (#143) + enemy-phase frame extension

When an investigator is engaged with **2+ ready enemies**, offer an order choice
(a `Choice`/`OptionId` prompt) instead of the deterministic `EnemyId` order — for
**both** the AoO loop and the enemy-phase loop (`resolve_attacks_for_investigator`).

This is where the **slice-3 carry-over** lands: today the enemy-phase `AttackLoop`
frame exists only *while parked on a window* (Shape A). Holding a player-chosen
order requires the frame to **span the whole per-investigator step 3.3** (pushed
at the start of that investigator's attacks, carrying the chosen order), and
forces a decision on **attacker-snapshot timing** (the list is currently
snapshotted when `BeforeInvestigatorAttacked` closes, after Fast plays). Both are
deferred to here deliberately — the order-choice is the first consumer that needs
the frame to span the action, so the extension is paid for by a real need (the
#393 promotion rule), and changing the snapshot timing is a behaviour change best
made where it is exercised.

### K5 — player damage/soak distribution (#44) + token unification (#119)

Replace the fill-to-capacity `assign_attack` default (which today auto-soaks onto
the lowest-`CardInstanceId` asset first) with a **player-choice `AwaitingInput`**
distributing each point of damage/horror across eligible soakers + self. **Do
with #119** (unify damage/horror/clue tokens onto `CardInPlay`) so the investigator
and asset token storage is symmetric, rather than special-casing the investigator
side. This also makes the multi-soak-window case (today guarded as
unconstructible in `park_on_soak_window`) reachable, so K5 must also lift that
single-window-per-attack guard into a real multi-window drain (coordinating with
simultaneous-trigger ordering, #213).

## Testing strategy

- **K1 — the §D keystone matrix** (`game-core` engine tests + `crates/cards/tests`
  for registry-backed cards):
  - AoO that **defeats the actor mid-Move** → primary effect suppressed, spent
    action + AoO events persist (re-validation abort path).
  - AoO **cancelled by Dodge** → no damage, action completes.
  - AoO **soaked onto Guard Dog → retaliate** fires against the AoO.
  - **multi-attacker** AoO (two engaged ready enemies).
  - the **re-validation-gate abort** path explicitly.
- **Behaviour-preserving** for the no-AoO path (no engaged enemy) and the
  no-window path (AoO with no available cancel/soak reaction): the existing engine
  + integration suite stays green through K1.
- **Per-slice green**: K2–K5 each add their own coverage (retaliate matrix;
  fast-exempt AoO gating; 2+-enemy order choice; multi-soak distribution +
  multi-window drain) and keep the prior slices green.

## Open questions / deferrals

- **No-op fast path.** When the actor has no engaged ready enemies, the AoO loop
  is a no-op and no window can open — the `ActionResolution` frame is pushed and
  immediately popped within one `apply()` call. Skipping the frame entirely in
  that case is an available optimization, **deferred (YAGNI)** in favour of one
  uniform code path; revisit only if profiling demands.
- **K2 retaliate source routing** — new `EnemyAttackSource::Retaliate` vs. reuse;
  settled in K2's thin design.
- **K3 play-card action-cost site** — verify where the non-fast play cost is
  charged before wiring AoO (prerequisite, above).
- **K4 attacker-snapshot timing** — when the per-investigator attacker list is
  fixed relative to Fast plays in the `BeforeInvestigatorAttacked` window;
  settled in K4 alongside the frame extension.

## What "done" looks like (the arc)

Attacks of opportunity and retaliate resolve with **full player agency**: cancel
(Dodge) and soak (Guard Dog retaliate) windows open against them; activated
abilities and action-event plays provoke them; the player picks attack order with
2+ engaged enemies and distributes damage/horror across soakers. The
`ActionResolution` frame + re-validation gate are the engine substrate; the §D
keystone matrix passes; the Gathering plays end-to-end through the new path. This
closes Ordering step 4 of #393, leaving step 5 (#347 token emission) to finish the
C checkpoint.
