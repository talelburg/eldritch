# Phase 7 keystone — K4: player picks engaged-enemy attack order (#143) — design

Tracking: **#143**, the **K4** sub-slice of the keystone attack-loop arc
(`2026-06-20-phase-7-keystone-mid-action-park-design.md`). K1 (#293, PR #413), K2
(#379, PR #414), and K3 (#361 PR #415 / #378 PR #416) shipped the mid-action
park/resume mechanism: all three attack sources — enemy phase, attacks of
opportunity, and retaliate — now drive through `drive_attack_loop`, opening the
`BeforeEnemyAttack` cancel window (Dodge 01023) and `AfterEnemyAttackDamagedAsset`
soak window (Guard Dog 01021). K4 adds the **player's choice of attack order** when
an investigator is engaged with **2+ ready enemies**, replacing today's
deterministic `EnemyId` order at both the enemy-phase and AoO sites.

## Why this pass exists

`resolve_attacks_for_investigator` (enemy phase 3.3) and `drive_aoo` (attacks of
opportunity) each snapshot their ready engaged attackers in **deterministic
`EnemyId` order** (`BTreeMap` iteration is key-sorted) and hand that list to
`drive_attack_loop`, which resolves it head-first. Both carry a `TODO(#143)` citing
the rule they violate.

RR p.25 step 3.3, verbatim: *"If an investigator is engaged with multiple enemies,
resolve their attacks in the order of the attacked investigator's choosing."*

In 1-player Standard scope this is reachable both ways: a lone investigator can be
engaged with 2+ enemies in the Enemy phase, and can provoke an AoO from 2+ engaged
enemies by taking a provoking action. No in-scope card *cares* which engaged enemy
strikes first (the issue notes this), so the deterministic order is not
*incorrect-by-outcome* today — but the player agency is a rules-correctness gap the
solo gate closes, and it is the first consumer that forces the **enemy-phase
`AttackLoop` frame to span the whole per-investigator step 3.3** (the slice-3 Shape-A
carry-over, below).

## The model

### One pick point, at the top of `drive_attack_loop`

All three sources funnel through `drive_attack_loop(cx, investigator, attackers,
source)`. The order choice lives **at the top of its per-attacker loop**: before
resolving the head attacker, if **2+ attackers remain**, suspend on a `PickSingle`
over the remaining enemies ("which attacks next?"). On resume, the chosen enemy is
moved to the head and **that one attack** resolves through the existing
before-cancel → deal → soak sequence; then the loop comes around and re-prompts if
2+ still remain.

This is **interleaved** picking: the player chooses the next attacker *seeing the
result of the previous one*, matching RR p.25 ("the order of the attacked
investigator's choosing" — chosen as play proceeds). N attackers → **N−1 picks** (2
enemies = 1 pick, 3 = 2 picks); the final attacker is forced. A single-attacker list
(every Retaliate; an AoO/enemy-phase with one engaged enemy) **never prompts** —
the `len() >= 2` gate is the whole switch.

Putting the pick in the shared driver means **one change covers both #143 sites**
(`resolve_attacks_for_investigator` and `drive_aoo`) and the Retaliate site for
free, with no per-source branching.

### Stack shape

The order pick makes the `AttackLoop` frame itself the **top frame awaiting input**
— unlike the cancel/soak case, where a `Resolution` window sits *above* a parked
loop. With 2+ ready engaged enemies at enemy-phase step 3.3:

```
[ …, EnemyPhase{BeforeInvestigatorAttacked, attacking: Who}, AttackLoop{remaining, EnemyPhase, PickOrder} ]
                                                              └─ top frame; its PickSingle is the prompt ─┘
```

`AwaitingInput` returns with a `PickSingle` over `remaining`. On `ResolveInput`,
`resolve_input`'s `AttackLoop` arm routes the `PickOrder` stage to the new
`resume_attack_order_pick`, which reorders, resolves the chosen head (which may
itself suspend on *its* before-cancel/soak window, re-parking as
`BeforeAttack`/`AfterSoak`), then continues the loop.

### The `AttackLoopStage::PickOrder` variant

```rust
pub enum AttackLoopStage {
    BeforeAttack,
    AfterSoak,
    /// Suspended on the order `PickSingle` (#143), with 2+ attackers remaining
    /// and none yet dealt this iteration. The `AttackLoop` frame is the top
    /// frame (no window above it) and IS the prompt; resume reorders
    /// `remaining_attackers` to put the picked enemy at the head, deals it,
    /// then continues. Distinct from the window stages: those park *beneath* a
    /// reaction window and resume on window-close via `resume_enemy_attack`;
    /// this one resumes on `ResolveInput` via `resume_attack_order_pick`.
    PickOrder,
}
```

### Two resume entries for `AttackLoop` (by trigger, not by accident)

- **Window-close → `resume_enemy_attack`** (stages `BeforeAttack` / `AfterSoak`):
  unchanged. Called from `run_window_continuation` when the cancel/soak window pops.
- **`ResolveInput` PickSingle → `resume_attack_order_pick`** (stage `PickOrder`):
  new. The order pick is not a reaction window, so it routes through the normal
  `ResolveInput` path, like HunterMove / SpawnEngage.

`resolve_input`'s `AttackLoop` arm — today a blanket defensive reject ("a parked
attack loop is top, no prompt outstanding") — branches on stage: `PickOrder` →
`resume_attack_order_pick`; `BeforeAttack`/`AfterSoak` keep the defensive reject
(those frames *are* parked beneath a window and never legitimately top-await input).

### Code shape: extract the per-attacker body

To share the "resolve the head attacker" sequence between `drive_attack_loop` (the
single-/post-pick path) and `resume_attack_order_pick` (the just-picked head), the
body currently inline in `drive_attack_loop`'s `while` — the `BeforeEnemyAttack`
emit + park-on-before, else `deal_head_and_maybe_park` — extracts into a helper:

```rust
/// Resolve the head attacker: open its before-cancel window (park as
/// `BeforeAttack` and suspend if a cancel reaction is available), else deal it +
/// maybe park on its soak window (`AfterSoak`). `Some(outcome)` = suspended;
/// `None` = continue to the next attacker.
fn process_head_attacker(
    cx: &mut Cx,
    investigator: InvestigatorId,
    attackers: &mut Vec<EnemyId>,
    source: EnemyAttackSource,
) -> Option<EngineOutcome>;
```

Then:

```rust
fn drive_attack_loop(cx, investigator, mut attackers, source) -> EngineOutcome {
    while let Some(&_head) = attackers.first() {
        if !actor_active(cx, investigator) { break; }            // early-break (unchanged)
        if attackers.len() >= 2 {
            return suspend_order_pick(cx, investigator, attackers, source); // NEW
        }
        if let Some(out) = process_head_attacker(cx, investigator, &mut attackers, source) {
            return out;
        }
    }
    EngineOutcome::Done
}
```

`suspend_order_pick` parks `AttackLoop{remaining: attackers, source, stage:
PickOrder}` as the top frame and returns `AwaitingInput { request:
InputRequest::choice(prompt, candidate_options(&attackers)), resume_token:
ResumeToken(0) }` — the HunterMove construction (`candidate_options` is already
`pub(super)` in `hunters.rs`; `OptionId(i)` indexes `remaining_attackers`).

`resume_attack_order_pick`:

```rust
fn resume_attack_order_pick(cx, response) -> EngineOutcome {
    // pop AttackLoop{ investigator, mut remaining_attackers, source, PickOrder }
    // PickSingle(OptionId(i)); validate i < remaining.len() (else Rejected, leave frame)
    // swap the picked enemy to remaining[0]
    if let Some(out) = process_head_attacker(cx, investigator, &mut remaining, source) {
        return out;                                   // chosen head suspended on its own window
    }
    let out = drive_attack_loop(cx, investigator, remaining, source); // rest (re-prompts if 2+)
    if matches!(out, AwaitingInput {..}) { return out; }
    finish_attack_loop(cx, source, investigator)      // shared source-keyed tail (below)
}
```

`finish_attack_loop(cx, source, investigator)` is the **source-keyed post-loop
tail** extracted from `resume_enemy_attack`'s existing match (lines 927–936):
`EnemyPhase → after_enemy_phase_attacks` · `AttackOfOpportunity → Done` · `Retaliate
→ drive_skill_test`. Both resumes share it, so the dispatch stays single-sourced.

### Why interleaved, not upfront ordering

Upfront ordering (collect the full permutation before any attack resolves) needs a
*partial-order* field on the frame separate from `remaining_attackers`, fixes the
order before the player sees any outcome, and buys nothing — the loop already
consumes `remaining_attackers` head-first, so storing the chosen-next at the head
*is* the order. Interleaved reuses `remaining_attackers` as-is, adds no frame state
beyond the stage variant, and is strictly more expressive. A bespoke
single-shot `InputResponse::PickAttackOrder` (the issue floats it) was rejected: it
cuts against the continuation-stack-cleanup's normalized `PickSingle` /
`PickMultiple` / `Confirm` / `Skip` channel and #393's OptionId direction.

## The slice-3 frame extension (Shape A → spanning), settled here

The keystone spec (§K4) scoped two carry-overs to this slice:

**1. The enemy-phase `AttackLoop` frame spans step 3.3.** Slice 3 (#411) left the
enemy-phase `AttackLoop` frame existing *only while parked on a window* (Shape A).
The order pick is the first consumer that needs the frame **at the start** of the
per-investigator sequence: with 2+ engaged enemies, `drive_attack_loop` now parks
the frame on its first iteration (the `PickOrder` suspension) — *before* any attack
— so it spans the whole of that investigator's step 3.3. This is the **#393 promotion
rule paying for itself**: the frame extends exactly because a real suspension
(the order choice) now occurs at the step's start. The single-engaged-enemy case
keeps Shape A (the frame is pushed only if a cancel/soak window suspends) — correct,
since with one attacker there is no order to choose and nothing to span.

**2. Attacker-snapshot timing — confirmed frozen at loop entry, no Fast-play
re-scan.** The concern was *when* the per-investigator attacker list is fixed
relative to Fast plays in the `BeforeInvestigatorAttacked` window. Resolution: it is
already frozen at loop entry and stays so. `resolve_attacks_for_investigator`
snapshots the ready engaged enemies *once*, after the `BeforeInvestigatorAttacked`
window has closed (so post-Fast-play), and `drive_attack_loop` thereafter operates
on the stored `remaining_attackers` — the order pick draws from that stored list,
**never re-scanning** `state.enemies`. So a mid-sequence board change (an enemy
defeated by a soak retaliate, say) does not add or remove a chosen-but-unresolved
attacker; the early-break-on-actor-defeat (loop step 1) is the only membership
change, and it stops the loop entirely. K4 introduces **no** re-scan and **no**
snapshot-timing change — it documents the existing freeze point and removes the
`TODO(#143)`.

## Sub-PR shape

**Single PR.** K4 is one coherent mechanism (the `PickOrder` suspension in the
shared `drive_attack_loop`) and is fully exercisable with synthetic multi-engagement
`game-core` engine tests — the attack *order* needs no specific card, only 2+
engaged ready test enemies. (Contrast K2, split because the retaliate window-suspend
proof required registry-backed Dodge/Guard Dog.) The cancel/soak interaction *under*
a chosen order is already covered by the K1/K2 registry tests and is behaviour-
preserving here; one integration test layering a Dodge cancel onto a 2-enemy chosen
order can ride along in `crates/cards/tests/` if cheap, but is not load-bearing.

## Testing strategy

- **Order pick, enemy phase (2 enemies):** investigator engaged with two ready
  enemies → `resolve_attacks_for_investigator` returns `AwaitingInput` with a
  `PickSingle` offering both; `ResolveInput(PickSingle(second))` resolves the chosen
  enemy first, then the other; both deal damage and exhaust (RR p.25).
- **Order pick, AoO (2 enemies):** a provoking basic action with two engaged ready
  enemies → `drive_aoo` returns the same `PickSingle`; the chosen AoO resolves first;
  neither exhausts (RR p.7); the parked action resumes after.
- **3-enemy partial ordering:** three engaged ready enemies → pick #1, resolve, pick
  #2 (two-option prompt), resolve, the third is forced — two prompts, three attacks
  in the chosen order.
- **Single enemy never prompts (behaviour-preserving):** one engaged ready enemy →
  no `AwaitingInput` for order; resolves inline exactly as today. The existing
  `resolve_attacks_for_investigator_*` and `drive_aoo_*` unit tests stay green.
- **Retaliate never prompts:** the 1-element retaliate list short-circuits the
  `len() >= 2` gate; the K2 `failed_fight_*` tests stay green.
- **Chosen order × cancel/soak window:** the chosen head attacker opening its own
  before-cancel (Dodge) or soak (Guard Dog) window re-parks correctly (`BeforeAttack`
  / `AfterSoak`) and resumes; after that attack, the loop re-prompts for the next if
  2+ remain (interleaving holds across a mid-attack window).
- **Invalid pick rejects, frame retained:** an out-of-range or wrong-variant
  `ResolveInput` against the `PickOrder` frame rejects and leaves the frame on the
  stack for retry (mirrors `resume_hunter_choice`).
- **Frame-spanning:** with 2+ engaged enemies, the `AttackLoop` frame is present on
  the stack from the first order pick through the last attack (assert it is on the
  stack while `AwaitingInput` for the order, beneath the enemy-phase anchor).
- **Full native gauntlet** green (`fmt`, `test --all --all-features`, `clippy
  --all-targets --all-features`, `doc --workspace`); wasm-build + wasm-clippy (no web
  code changes — engine-only).

## Open questions / deferrals

- **K5 multi-soak distribution (#44 + #119)** stays out of scope: K4 chooses *which
  enemy* attacks next, not *how its damage/horror is distributed*. The
  `park_on_soak_window` single-window-per-attack `debug_assert` is untouched.
- **Order re-confirmation after a defeat mid-sequence.** If a chosen-but-unresolved
  attacker is somehow removed mid-sequence (no in-scope source does this short of
  defeating the actor, which early-breaks the whole loop), `resume_attack_order_pick`
  validates the picked index against the *current* `remaining_attackers` and the next
  prompt re-derives from the stored list — so a stale list cannot strand. A richer
  invalidation (an enemy leaving engagement mid-sequence) is the same structural hook
  K1's re-validation gate is, correct-by-construction in scope.
- **Human-readable option labels** remain the `format!("{enemy:?}")` debug repr
  (`candidate_options`), promoted to real labels by #205 at the browser capstone —
  same deferral as every other `PickSingle` prompt today.

## What "done" looks like

An investigator engaged with 2+ ready enemies — in the Enemy phase or facing an
attack of opportunity — is prompted to pick the order their attacks resolve, one at
a time, seeing each result before choosing the next (RR p.25 step 3.3). Single-enemy
and Retaliate paths never prompt and are behaviour-preserving. The enemy-phase
`AttackLoop` frame spans the whole per-investigator step 3.3 in the multi-enemy
case, resolving slice 3's Shape-A carry-over, and the attacker snapshot is confirmed
frozen at loop entry. Both `TODO(#143)`s are gone. This closes #143 and leaves **K5**
(#44 / #119 — player damage/horror distribution + token unification) as the final
sub-slice of the keystone arc.
