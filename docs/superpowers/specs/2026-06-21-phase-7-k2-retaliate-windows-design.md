# Phase 7 keystone — K2: Retaliate opens cancel/soak windows (#379) — design

Tracking: **#379**, the **K2** sub-slice of the keystone attack-loop arc
(`2026-06-20-phase-7-keystone-mid-action-park-design.md`). K1 (#293, PR #413,
merged) shipped the mid-action park/resume mechanism: attacks of opportunity now
drive through `drive_attack_loop` (via `drive_aoo`) and open the `BeforeEnemyAttack`
cancel window (Dodge 01023) and `AfterEnemyAttackDamagedAsset` soak window (Guard
Dog 01021). K2 applies the **same mechanism to the one remaining direct-`enemy_attack`
caller**: the Retaliate attack fired from a failed Fight.

## Why this pass exists

`fire_retaliate_if_any` (`crates/game-core/src/engine/dispatch/skill_test.rs:854`)
fires a Retaliate attack after a *failed Fight* against a ready enemy with the
retaliate keyword. It calls `combat::enemy_attack` **directly**, so — exactly like
pre-K1 AoO — it opens no cancel/soak windows: Dodge cannot cancel a retaliate
strike and Guard Dog does not retaliate against one. Its own doc-comment names this
step "the future home of the 'after an enemy attacks' reaction window."

K1 left this site untouched on purpose (the master arc spec, §K2): retaliate fires
from a **different park point** than the basic actions. A basic action's AoO parks
on an `ActionResolution` frame above `InvestigatorTurn`; a retaliate fires from
*inside the Fight's skill-test follow-up*, after the test resolves — so its park
point is the already-existing `SkillTest` frame, and its resume must return to the
follow-up, not to the main `drive` loop. That asymmetry is the whole of K2's design.

### Rules (verified)

RR p.18 (quoted in the existing doc-comment): *"Each time an investigator fails a
skill test while attacking a ready enemy with the retaliate keyword, after applying
all results for that skill test, that enemy performs an attack against the attacking
investigator. An enemy does not exhaust after performing a retaliate attack."*

- **Trigger: failed Fight only.** "While attacking" = Fight; Evade is not an attack.
  K2 does **not** expand the trigger — `fire_retaliate_if_any` already gates on
  `SkillTestFollowUp::Fight` + `!succeeded` + `retaliate && !exhausted`.
- **A retaliate attack is an enemy attack**, so Dodge 01023 ("Cancel that attack")
  cancels it and Guard Dog 01021 (soak + deal 1 to the attacker) reacts to it —
  the same two windows K1 wired for AoO.
- **No exhaust** — already satisfied: K1 gated the exhaust step in
  `process_attacker_dealing` to `EnemyAttackSource::EnemyPhase` only, so any
  non-enemy-phase source is non-exhausting for free.

## The model

### The follow-up cursor is the park substrate (already exists)

The Fight follow-up runs as a `FinishContinuation` cursor carried **on the
`SkillTest` frame**, driven by the `drive_skill_test` loop
(`skill_test.rs`): `AwaitingCommit → PostFollowUp → PostRetaliate →
PostOnResolution → teardown`. Retaliate fires at the **`PostRetaliate`** stage.
Mid-test reaction windows already suspend and resume against this frame: a window
pushed *above* the `SkillTest` frame returns `AwaitingInput`, and on close the
window-continuation path re-enters `drive_skill_test`, which re-reads the cursor.
K2 reuses this substrate unchanged — it adds no new continuation frame.

### Stack shape

At `PostRetaliate`, with the `SkillTest` frame on the stack, routing the retaliate
through `drive_attack_loop` pushes the loop frame + window above it:

```
[ …, SkillTest{cursor: PostOnResolution}, AttackLoop{source: Retaliate}, Resolution(window) ]
```

The window is the player-facing prompt (Dodge? / resolve Guard Dog). `AwaitingInput`
returns. On close, `resume_enemy_attack` drains the loop, pops the `AttackLoop`, and
— for the `Retaliate` source — **re-enters `drive_skill_test`**, which reads the
cursor (`PostOnResolution`) and runs teardown. No retaliate-specific frame; the
`SkillTest` frame is the resume point.

### The pieces

1. **`EnemyAttackSource::Retaliate`** — a new variant. Reuses `drive_attack_loop`
   with a **single-element attacker list** (a retaliate is one enemy attacking once);
   the two sequential suspension points (BeforeAttack cancel, AfterSoak) are tracked
   by the existing `AttackLoopStage` cursor on the `AttackLoop` frame. Non-exhaust is
   inherited (exhaust is `EnemyPhase`-gated). A thin `drive_retaliate(cx, enemy,
   investigator)` helper mirrors `drive_aoo`: build the 1-element list, call
   `drive_attack_loop(…, Retaliate)`.

2. **`fire_retaliate_if_any` returns `EngineOutcome`** — instead of the `()` direct
   `enemy_attack`, it calls `drive_retaliate` and returns its outcome (`Done` when no
   window opens, `AwaitingInput` when one does). The no-retaliate early-returns
   become `EngineOutcome::Done`.

3. **`PostRetaliate` stage handles the suspension** — in `drive_skill_test`, the
   `PostRetaliate` arm advances the cursor to `PostOnResolution` **before** firing
   retaliate, then calls `fire_retaliate_if_any`; if it returns `AwaitingInput`,
   return that (the `SkillTest` frame is parked at `PostOnResolution` beneath the
   `AttackLoop` + window); otherwise fall through to the loop's next iteration as
   today. Setting the cursor first means resume continues cleanly at
   `PostOnResolution` regardless of whether a window opened.

4. **`resume_enemy_attack`'s `Retaliate` arm** — after draining the loop, re-enter
   `drive_skill_test(cx)` so the follow-up continues to teardown. (Contrast:
   `EnemyPhase → after_enemy_phase_attacks`; `AttackOfOpportunity → Done`.)

### Why this routing, not the alternatives

- **Reuse `drive_attack_loop` (chosen)** vs. opening the windows ad-hoc from
  `fire_retaliate_if_any`: the ad-hoc path would re-implement the two-stage
  BeforeAttack→AfterSoak cancel/soak tracking that `AttackLoopStage` already
  encodes. A 1-element loop is DRY and exercises the identical, already-tested code.
- **New `Retaliate` source** vs. reusing `AttackOfOpportunity`: the resume
  *destination* differs — AoO returns `Done` to the main `drive` loop (which then
  resumes an `ActionResolution` frame), whereas retaliate must re-enter
  `drive_skill_test`. The source enum is exactly the dispatch key
  `resume_enemy_attack` already switches on, so a third variant is the natural seam.

### Re-validation / mid-attack viability

Lighter than K1's gate. The retaliate fires *after* the Fight test fully resolves,
so there is no pending primary effect to re-validate — once the retaliate's
cancel/soak windows close, the only remaining work is the skill-test teardown
(`PostOnResolution`: discard committed cards, emit `SkillTestEnded`, drain
this-test modifiers, pop the `SkillTest` frame). That teardown reads only the
investigator's own committed-cards record, which a retaliate cannot disturb. The
existing `current_skill_test()` lookups in `drive_skill_test` keep their
`unreachable!`/`expect` guards (state-corruption invariants). No new re-validation
surface.

## Sub-PR decomposition

K2 is small; two behaviour-green steps (mirroring K1's cadence at smaller scale):

- **K2a — engine: route retaliate through the loop.** Add
  `EnemyAttackSource::Retaliate` + `drive_retaliate`; make `fire_retaliate_if_any`
  return `EngineOutcome`; handle the suspension in the `PostRetaliate` stage; add the
  `Retaliate` arm to `resume_enemy_attack` (re-enter `drive_skill_test`). Engine unit
  tests: retaliate still fires on a failed Fight, still doesn't exhaust, no-window
  path behaviour-preserving (the existing `failed_fight_*` tests stay green); a
  synthetic before/soak suspension is registry-gated, so the window-suspend proof
  lives in K2b.
- **K2b — integration: Dodge cancels + Guard Dog retaliates against a retaliate.**
  Registry-backed tests in `crates/cards/tests/`: a failed Fight against a ready
  retaliate enemy (a `retaliate` test enemy, or Ghoul Priest 01116 — The Gathering's
  boss carries `retaliate: true`) → the retaliate opens its windows → Dodge cancels
  it (no damage, the Fight teardown still completes); Guard Dog soaks it and deals 1
  back to the retaliating enemy, the attacker does not exhaust (RR p.18), and the
  skill test ends cleanly.

## Testing strategy

- **Behaviour-preserving:** the existing `failed_fight_against_ready_retaliate_enemy_
  triggers_attack`, `successful_fight_…does_not_trigger`, and
  `failed_fight_against_exhausted_retaliate_enemy_…` unit tests
  (`engine/mod.rs:1955+`) must stay green — K2a changes the *route*, not when
  retaliate fires or its damage.
- **Non-exhaust:** assert the retaliate attacker stays ready after the attack
  (RR p.18), through the new loop path.
- **Suspend/resume (K2b, registry-backed):** the real proof — the retaliate's
  window opens (`AwaitingInput`), a `ResolveInput` resolves it, and the Fight's
  skill-test teardown completes afterward (`SkillTestEnded` fires, the `SkillTest`
  frame is popped). Card text for Dodge 01023 / Guard Dog 01021 verified against
  ArkhamDB (incl. FAQ) before asserting.
- **Full native gauntlet** green (`fmt`, `test --all`, `clippy --all-targets
  --all-features`, `doc --workspace`); wasm-build + wasm-clippy (no web code
  changes).

## Open questions / deferrals

- **The "after an enemy attacks" reaction window (#64)** — the broader
  after-resolution skill-test reaction window (Roland's reaction; Ordering step 5 of
  #393) is **out of K2 scope**. K2 opens only the cancel/soak windows *on the
  retaliate attack itself*, matching K1.
- **Multi-attacker retaliate** — not a thing: a retaliate is one enemy attacking
  once. The 1-element loop is exact, not a simplification.

## What "done" looks like

A failed Fight against a ready retaliate enemy fires a retaliate attack that opens
its cancel (Dodge) and soak (Guard Dog) windows — Dodge cancels it, Guard Dog
retaliates against it — and the Fight's skill-test teardown completes after the
window closes, with the attacker non-exhausted (RR p.18). This closes #379 and
leaves K3 (#361/#378 — AoO from activated abilities + action-event play) as the next
sub-slice of the arc.
