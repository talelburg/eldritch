# AttackLoop frame — lifting the enemy-attack cursors onto the continuation stack

**Issue:** [#411](https://github.com/talelburg/eldritch/issues/411) — step 3 of
the #393 unified control-flow C-checkpoint (spec §D Part 1,
`2026-06-20-unified-control-flow-model-design.md`).

## Goal

Remove the last two framework cursors the #393 model targets for migration —
`GameState::enemy_attack_pending` and `GameState::pending_enemy_attack` — by
moving the parked enemy-attack loop onto a `Continuation::AttackLoop` frame and
the per-investigator cursor onto the `EnemyPhase` anchor's resume key.

This is a **behaviour-preserving cursor lift.** Enemy-phase attacks — multi-
attacker order, the before-attack cancel window, the after-soak window, the
no-active-investigator path — must resolve exactly as today. It is *not* the
keystone (step 4): opening attack-of-opportunity / Retaliate windows mid-action
is built on top of the frame this slice introduces.

## Background: the two pieces of state

The enemy phase (Rules Reference p.25, step 3.3) attacks each engaged
investigator in turn. Two distinct pieces of state drive this today:

- **`enemy_attack_pending: Option<InvestigatorId>`** — the per-investigator
  cursor. Seeded by `enemy_attack_kickoff` to the first active investigator,
  advanced by `after_enemy_phase_attacks` to the next, read by the anchor's
  `BeforeInvestigatorAttacked` arm to know whose attacks to resolve. It lives
  across the *entire* per-investigator sub-step, including across the
  `BeforeInvestigatorAttacked` framework window that opens *before* the attack
  loop computes its attacker list.

- **`pending_enemy_attack: Option<PendingEnemyAttack>`** — the parked mid-loop
  suspension (`investigator`, `remaining_attackers`, `source`, `stage`). Set in
  `drive_attack_loop` / `park_on_soak_window` when an attack opens a reaction
  window; `take()`n by `resume_enemy_attack` when that window closes. It lives
  *only* while suspended on a cancel/soak window.

They have different lifetimes and roles, so they lift differently.

## Design

### 1. Rename `AttackLoopPhase` → `AttackLoopStage`

"Phase" is a load-bearing game concept (Mythos / Investigation / Enemy /
Upkeep). The existing `AttackLoopPhase` enum (`BeforeAttack` / `AfterSoak`) and
its `phase` field are renamed to `AttackLoopStage` / `stage`. (`Stage`, not
`Step` — `PhaseStep` is already the rules-step concept, so `Step` would trade
one collision for another.)

### 2. The parked suspension → `Continuation::AttackLoop`

`PendingEnemyAttack { investigator, remaining_attackers, source, stage }` becomes
a `Continuation::AttackLoop { … }` variant carrying the same fields verbatim. The
`PendingEnemyAttack` struct and the `GameState::pending_enemy_attack` field are
removed.

- **Push** at the two spots that today set `pending_enemy_attack = Some(...)` —
  `drive_attack_loop`'s before-cancel park and `park_on_soak_window` — pushed
  *immediately beneath* the reaction window that `open_queued_reaction_window`
  then pushes above it.
- **Pop** in `resume_enemy_attack`, replacing the `.take()`. The existing
  `run_window_continuation` arm for `BeforeEnemyAttack` /
  `AfterEnemyAttackDamagedAsset` keeps calling `resume_enemy_attack` — minimal,
  behaviour-preserving routing.

Stack shape while suspended:
`[…, EnemyPhase anchor, AttackLoop, Resolution(window)]` — the windows are
children pushed *above* the loop frame, as the #393 spec describes.

`Continuation::awaits_input()` returns `false` for `AttackLoop`: it is an
internal sequencing frame, never a player-facing prompt. The window *above* it is
what the player resolves; the loop frame is only ever momentarily on top inside
`resume_enemy_attack` (between the window pop and the loop pop), never at a
suspension or action boundary. `drive` never advances it (it is not a phase
anchor); a `debug_assert` documents that an `AttackLoop` left on top at a drive
boundary is a state-corruption invariant violation.

### 3. The cursor → an `attacking` field on the `EnemyPhase` anchor

`enemy_attack_pending` becomes a field on the anchor itself:
`Continuation::EnemyPhase { resume: EnemyResume, attacking: Option<InvestigatorId> }`.
The anchor carries which investigator is currently being attacked.

A field (not an `EnemyResume::AttacksFor(InvestigatorId)` payload) because the
anchor exists *before* an investigator is selected: `enemy_phase` pushes it ahead
of hunter movement (step 3.2), so a lead-investigator tie suspends above an
already-present anchor; `enemy_attack_kickoff` only picks the first attacked
investigator *after* hunter movement resolves. `AttacksFor` would need a fake
placeholder id during that window; `Option` is honest.

- `enemy_phase` pushes `{ resume: BeforeInvestigatorAttacked, attacking: None }`
  (the resume is a placeholder overwritten by kickoff, as today).
- `enemy_attack_kickoff` sets `attacking: Some(first_active)` (or
  `attacking: None`, `resume: AfterAllAttacked` when there is no active
  investigator).
- `after_enemy_phase_attacks` sets `attacking: Some(next_active)` or
  `attacking: None` + `resume: AfterAllAttacked`.
- the anchor's `BeforeInvestigatorAttacked` arm reads `attacking` (expect-`Some`,
  matching today's cursor expect-`Some`) instead of the state field.

The single mutator `set_enemy_anchor(cx, resume, attacking)` replaces
`set_enemy_anchor_resume` (which today replaces the whole variant), setting both
fields together so neither is dropped. The `GameState::enemy_attack_pending`
field is removed; no separate end-of-phase clear is needed (the final advance
sets `attacking: None`, then the anchor is popped at phase end).

## Why Shape A (the parked-only lift), not the spec end-state

The #393 spec §D Part 1 phrases the cursor migration as "the anchor pushes one
`AttackLoop` per active investigator" — an end-state where the `AttackLoop` frame
spans the *whole* per-investigator step 3.3, the `BeforeInvestigatorAttacked`
window becomes a child above it, and `after_enemy_phase_attacks` pops-and-pushes
the next investigator's frame.

This slice deliberately stops short of that. **Deferred to the keystone (step 4):**
extending the `AttackLoop` frame's lifetime to span the whole sub-step. The
reason: the full-span shape forces a decision on *when* the attacker list is
computed. Today it is snapshotted when the `BeforeInvestigatorAttacked` window
closes — *after* any Fast plays made in that window, which can change engagement.
Pushing the frame at kickoff with an eager attacker list would move that snapshot
earlier and change Fast-play timing — a behaviour change, in a slice meant to
have none.

The keystone is where `AttackLoop` becomes a real mid-action consumer (parking
the *triggering action*, opening AoO/Retaliate windows, resuming), so it is the
natural place to decide the frame's full lifetime and the attacker-snapshot
timing together. **The phase-7 doc records this deferral** so the keystone slice
accounts for it (see "Phase doc" below).

## Touchpoints

- `crates/game-core/src/state/game_state.rs` — rename `AttackLoopPhase`/`phase`;
  remove `PendingEnemyAttack` struct + the two `GameState` fields; add the
  `Continuation::AttackLoop` variant and wire it into `awaits_input` /
  `as_resolution` / `as_resolution_mut`; add the `attacking` field to
  `Continuation::EnemyPhase`.
- `crates/game-core/src/engine/dispatch/combat.rs` — push `AttackLoop` at the two
  park sites; pop it in `resume_enemy_attack`.
- `crates/game-core/src/engine/dispatch/phases.rs` — `enemy_attack_kickoff`,
  `set_enemy_anchor` (replacing `set_enemy_anchor_resume`), the anchor
  `BeforeInvestigatorAttacked` arm reads `attacking`; the `enemy_phase` /
  transition pushes carry `attacking: None`.
- `crates/game-core/src/engine/dispatch/reaction_windows.rs` —
  `after_enemy_phase_attacks` sets the anchor resume instead of the cursor field.
- `crates/game-core/src/state/builder.rs` — drop the two field initialisers.
- `crates/game-core/src/engine/dispatch/cursor.rs` — doc-comment reference.

## Testing

Pure refactor: the existing combat / enemy-phase integration and unit tests are
the regression net and stay green untouched —

- multi-attacker resolution order,
- Guard Dog (01021) after-soak window,
- Dodge (01023) before-attack cancel window,
- the no-active-investigator straight-to-`AfterAllAttacked` path,
- the Enemy → Upkeep cascade.

The serde round-trip tests for the removed fields are repointed: replace the
`enemy_attack_pending` / `pending_enemy_attack` round-trips with an `AttackLoop`
frame round-trip and an `EnemyPhase { attacking: Some(_) }` anchor round-trip.

## Phase doc

Per project convention the phase-7 doc update lands in this PR's final commit
(after CI is green). It records:

- #411 closed (move to the Closed table, bump counts);
- the Shape-A **deferral**: the `AttackLoop` frame's full per-investigator span
  (and the attacker-snapshot-timing decision) is deferred to the keystone slice
  (step 4), which must account for it when it extends the frame to park the
  triggering action.

## Out of scope

- Mid-action park/resume; opening AoO / Retaliate reaction windows — the keystone
  (step 4).
- #347 token emission (step 5).
- Any change to attack resolution behaviour.
