# #71 — Enemy phase: engagement attacks (design)

GitHub issue: [#71](https://github.com/talelburg/eldritch/issues/71) · Phase: [phase-4-scenario-plumbing](../../phases/phase-4-scenario-plumbing.md) · Depends on #69 (Mythos phase content — phase-driver pattern + `open_fast_window` helper, PR #136), #70 (Upkeep phase content, PR #142), #67 (enemy state — shipped), and #103 (unified window stack, PR #129) — all shipped.

## Context

The Enemy phase is the third phase-driver piece in Phase 4's set. Today `step_phase` ticks through Enemy in zero events (it has no driver — only the `_` fallback emits `PhaseStarted(Enemy)`). #71 wires the engagement-attacks half of Rules Reference p.25 (III. Enemy phase): each Active investigator's engaged ready enemies attack, then exhaust. Hunter movement (step 3.2) is deferred to **#128**.

#71 follows the **phase-driver pattern** established by #69 (PR #136) and #70 (PR #142): a driver function `enemy_phase` owns `PhaseStarted(Enemy)` (step 3.1) and an end helper `enemy_phase_end` owns `PhaseEnded(Enemy)` (step 3.4). The shape is closest to **Mythos's**: per-investigator iteration driven by a state-side cursor (analog of `mythos_draw_pending`), with the per-investigator loop body executing in the window-close continuation rather than inline in the driver.

## Rules Reference, verbatim (p.23 flowchart + p.25 detail)

> **III. Enemy phase**
> 3.1 Enemy phase begins.
> 3.2 Hunter enemies move.
> 3.3 Next investigator resolves engaged enemy attacks.
> 3.4 Enemy phase ends.

Step detail (p.25):

> **3.1 Enemy phase begins.** This step formalizes the beginning of the enemy phase.
> **3.2 Hunter enemies move.** Resolve the hunter keyword for each ready, unengaged enemy that has the hunter keyword (see "Hunter" on page 12).
> **3.3 Next investigator resolves engaged enemy attacks.** Resolve engaged enemy attacks in player order, with each player resolving all of his or her engaged enemies before advancing to the next player. Each ready, engaged enemy makes an attack against the investigator to which it is engaged. When an enemy attacks, deal its attack (both its damage and its horror, simultaneously) to the engaged investigator. Upon completion of dealing the attack (and all abilities triggered by the attack), exhaust the enemy. If an investigator is engaged with multiple enemies, resolve their attacks in the order of the attacked investigator's choosing. After an investigator has resolved the attacks of the enemies he or she is engaged with, return to the previous player window. After the final investigator resolves enemy attacks, proceed to the next player window.
> **3.4 Enemy phase ends.** This step formalizes the end of the enemy phase.

Supporting clauses inherited by reference (not re-derived):

- p.7 Apply Damage/Horror: *"Any assigned damage/horror that has not been prevented is now placed on each card to which it has been assigned, simultaneously. … After applying damage/horror, if an investigator has damage equal to or higher than his or her health or horror equal to or higher than his or her sanity, he or she is defeated."* — damage and horror place simultaneously; the defeat check fires *after* placement.
- p.10 Elimination: *"Any time a player is eliminated: … 3. All enemies engaged with that player are placed at the location the investigator was at when he or she was eliminated, unengaged but otherwise maintaining their current game state."*
- p.10 Enemy Engagement: *"Any time a ready unengaged enemy is at the same location as an investigator, it engages that investigator … If there are multiple investigators at the same location as a ready unengaged enemy, follow the enemy's prey instructions to determine which investigator is engaged."*

#71 owns sub-steps 3.1, 3.3, and 3.4. Sub-step 3.2 (hunter movement) is **#128**'s domain and lands as a named call site with a TODO body — it needs the `Prey` enum + BFS + `PickLocation` ambiguity resolution that #128 carries.

## Scope

- Two new `WindowKind` variants (bare, no payload): `BeforeInvestigatorAttacked` for the per-investigator window opened before each Active investigator's engaged enemies attack, and `AfterAllInvestigatorsAttacked` for the final window after all investigators have resolved.
- New `GameState` field `enemy_attack_pending: Option<InvestigatorId>` (mirror of `mythos_draw_pending`) carrying the per-investigator cursor across `apply` calls.
- Enemy driver `enemy_phase(state, events)` invoked from `step_phase` on the Investigation→Enemy transition; emits `PhaseStarted(Enemy)` (step 3.1), calls `hunter_movement_step` (3.2 stub), seeds `enemy_attack_pending` to the first Active investigator in `turn_order`, and opens the first `BeforeInvestigatorAttacked` window (or `AfterAllInvestigatorsAttacked` directly if no Active investigator exists).
- Enemy closing helper `enemy_phase_end(state, events)` invoked from `run_window_continuation`'s `AfterAllInvestigatorsAttacked` arm; emits `PhaseEnded(Enemy)` (step 3.4) and steps to Upkeep (mirror of `mythos_phase_end` / `upkeep_phase_end`).
- Sub-step helpers `hunter_movement_step` (#128 TODO stub) and `resolve_attacks_for_investigator` (the per-investigator attack loop).
- **Extract two shared cursor helpers** from `mythos_phase` (seed) and `advance_mythos_draw_pending` (advance) so Mythos and Enemy share one code path: `first_active_investigator(state) -> Option<InvestigatorId>` and `next_active_investigator_after(state, current) -> Option<InvestigatorId>`. Both filter on `Status::Active` (Rules p.10 Elimination). Each cursor-using site shrinks to a one-liner.
- `run_window_continuation` gains two arms: `BeforeInvestigatorAttacked` (run `resolve_attacks_for_investigator`, advance cursor, open next window) and `AfterAllInvestigatorsAttacked` (run `enemy_phase_end`). Both carry the same in-flight-skill-test `unreachable!()` guard as the existing `MythosAfterDraws` / `UpkeepBegins` arms.
- `step_phase`: extend `PhaseEnded` suppression to cover Enemy; add the `Phase::Enemy` driver-dispatch arm; **replace the `_ =>` fallback with `unreachable!`** — once Enemy is driver-dispatched, all four `Phase::next()` outputs are matched and `from == to` cannot occur, so the arm is structurally unreachable.
- Engine unit tests in `dispatch.rs`'s `#[cfg(test)]` block (~12 tests covering driver shape, attack resolution, pause/resume, and `step_phase` wiring).

## Out of scope

- **Hunter movement (3.2).** `#128` — needs `Prey` enum + BFS + `PickLocation` ambiguity. Lands as a named TODO call site (`hunter_movement_step` with an empty body).
- **Player-pick attack order when 2+ engaged enemies on one investigator.** Rules p.25: *"If an investigator is engaged with multiple enemies, resolve their attacks in the order of the attacked investigator's choosing."* Deterministic `EnemyId` order with a `TODO(#143)` citing this clause, mirroring `fire_attacks_of_opportunity`'s current shape. **#143** (unmilestoned) covers both call sites (Enemy-phase 3.3 + AoO) — pulled in when a multi-engagement + multi-investigator scenario forces it (Phase 7+).
- **`Event::EnemyAttacked`.** No corpus consumer today. `DamageTaken` / `HorrorTaken` / `InvestigatorDefeated` provide enough signal for non-reaction observability. Concrete-consumer-first; defer until a Survival-class card needs the Before/After attack reaction window.
- **Full investigator-elimination flow (Rules p.10 steps 1–5).** `apply_investigator_defeat` today only flips `Status` + emits `InvestigatorDefeated`. The full flow (remove controlled cards from game; clues at location; **disengage + re-engage enemies**; threat-area discard; lead transfer) lands in **#144** (Phase-4 milestone, `blocked` on #128 so multi-investigator re-engagement can use prey logic directly — no "rejects pointing at #128" stub). #71's loop uses an early-break on `Status != Active` as the rules-correct minimal interpretation in the interim.
- **Per-step-3.2 reaction window.** Rules don't print one between 3.2 and 3.3. If hunter movement (#128) needs a pre-3.2 reaction window for hunter-keyword triggers, #128 adds it.
- **Restricting `fast_actors` to the to-be-attacked investigator.** `FastActorScope::Any` matches Mythos/Upkeep. When a "Fast: before you're attacked" card lands, the card's own gate enforces — or we narrow `fast_actors` at the open site.
- **Engine accessor exposing the cursor to cards.** Bare `WindowKind` variants force this by design; no corpus consumer asks for it today. The consolidation-into-single-variant follow-up may expose a generic "current actor" accessor.

## Engine — new types

### `WindowKind` (additive)

File: `crates/game-core/src/state/game_state.rs`. `WindowKind` is `#[non_exhaustive]`; both new variants are additive.

```rust
/// The player window opened before an investigator's engaged enemies
/// resolve their attacks (Rules Reference p.25 step 3.3, the
/// "previous player window" investigators "return to" between
/// resolutions). The investigator to be attacked next is carried on
/// [`GameState::enemy_attack_pending`], not in the variant — mirror
/// of [`MythosAfterDraws`] + [`GameState::mythos_draw_pending`].
///
/// Continuation (in [`run_window_continuation`]): read the cursor,
/// resolve the pending investigator's engaged ready enemies in
/// [`EnemyId`] order, exhaust each, advance the cursor to the next
/// Active investigator in [`turn_order`] (or `None`), open the next
/// window (`BeforeInvestigatorAttacked` if Some,
/// `AfterAllInvestigatorsAttacked` if None).
///
/// One window per Active investigator in `turn_order`.
BeforeInvestigatorAttacked,

/// The player window after all investigators have resolved their
/// engaged enemies' attacks (Rules Reference p.25 step 3.3, the
/// "next player window" entered after the final investigator).
/// Continuation runs [`enemy_phase_end`] (step 3.4 + transition).
/// Mirror of [`MythosAfterDraws`]'s end-of-step shape.
AfterAllInvestigatorsAttacked,
```

### `GameState` (additive field)

File: `crates/game-core/src/state/game_state.rs`. Mirror of `mythos_draw_pending`.

```rust
/// The next investigator due to resolve engaged-enemy attacks during
/// Enemy phase step 3.3. Mirrors [`mythos_draw_pending`]'s contract:
///
/// - Set to the first [`Status::Active`] investigator in
///   [`turn_order`] when [`enemy_phase`] runs step 3.3's loop kickoff.
/// - Advanced by [`run_window_continuation`] after each per-investigator
///   attack resolution closes, to the next Active investigator in
///   [`turn_order`] (or `None` when the loop is done).
/// - Stays `None` during all phases other than Enemy.
///
/// Eliminated investigators ([`Status::Killed`] / [`Status::Insane`]
/// / [`Status::Resigned`]) are skipped during advance, mirroring the
/// `mythos_draw_pending` semantics established in #69.
///
/// [`mythos_draw_pending`]: GameState::mythos_draw_pending
/// [`turn_order`]: GameState::turn_order
pub enemy_attack_pending: Option<InvestigatorId>,
```

`Default::default()` returns `None`. Serde derives flow through `Option<InvestigatorId>`; no custom serialization.

### No new `Event` variants

`Event::EnemyExhausted` already exists at `event.rs:208`. The existing `WindowOpened` / `WindowClosed` / `PhaseStarted` / `PhaseEnded` / `DamageTaken` / `HorrorTaken` / `InvestigatorDefeated` cover everything else #71 needs to emit.

### No new `EngineRecord` variants

No new randomness — no chaos token draws, deck shuffles, or encounter draws. Replay determinism is automatic.

## Engine — shared cursor helpers (extracted from Mythos)

File: `crates/game-core/src/engine/dispatch.rs`. The `mythos_draw_pending` seed and advance use identical "first Active in turn_order" and "next Active after `current` in turn_order" logic. #71 needs the same for `enemy_attack_pending`. Extracted once, used by both.

```rust
/// First investigator in [`turn_order`] whose status is
/// [`Status::Active`]. Eliminated investigators
/// ([`Status::Killed`] / [`Status::Insane`] / [`Status::Resigned`])
/// are skipped per Rules Reference p.10 (Elimination).
///
/// Used by per-investigator phase loops to seed their cursor:
/// Mythos 1.4 draws ([`mythos_phase`] seeds `mythos_draw_pending`),
/// Enemy 3.3 attacks ([`enemy_phase`] seeds `enemy_attack_pending`).
///
/// [`turn_order`]: GameState::turn_order
fn first_active_investigator(state: &GameState) -> Option<InvestigatorId> {
    state.turn_order.iter().copied().find(|id| {
        state.investigators.get(id)
            .is_some_and(|inv| inv.status == Status::Active)
    })
}

/// First investigator in [`turn_order`] whose status is
/// [`Status::Active`], positioned strictly after `current`. Returns
/// `None` when no Active investigator follows `current` in
/// `turn_order`, or when `current` is not in `turn_order` at all.
///
/// Eliminated investigators are skipped per Rules Reference p.10
/// (same predicate as [`first_active_investigator`]).
///
/// Used by per-investigator phase loops to advance their cursor:
/// `advance_mythos_draw_pending` after a draw chain completes,
/// `run_window_continuation`'s `BeforeInvestigatorAttacked` arm
/// after one investigator's attacks resolve.
///
/// Notable: `current` may itself be non-Active (defeated mid-loop in
/// Enemy phase) — using `turn_order` as the index basis (rather than
/// the filtered-Active list) makes this case the same single-pass
/// lookup.
///
/// [`turn_order`]: GameState::turn_order
fn next_active_investigator_after(
    state: &GameState,
    current: InvestigatorId,
) -> Option<InvestigatorId> {
    state.turn_order.iter()
        .position(|id| *id == current)
        .and_then(|idx| {
            state.turn_order.iter().skip(idx + 1).copied().find(|id| {
                state.investigators.get(id)
                    .is_some_and(|inv| inv.status == Status::Active)
            })
        })
}
```

Call-site changes:
- `mythos_phase` line 886–891 collapses to `state.mythos_draw_pending = first_active_investigator(state);`
- `advance_mythos_draw_pending` line 5013–5024 collapses to `let next = next_active_investigator_after(state, current); state.mythos_draw_pending = next; if next.is_none() { open_fast_window(state, events, WindowKind::MythosAfterDraws); }` — the window-open-on-None remains in-place.
- `enemy_phase` seed and the `BeforeInvestigatorAttacked` continuation advance use both helpers (skeleton below).

Tests:
- A small unit test pair for the two helpers (`first_active_investigator_*`, `next_active_investigator_after_*`) — covers empty `turn_order`, all-eliminated, mixed Active/non-Active middle entries, `current` not in `turn_order`, `current` is the last entry. The existing Mythos cursor tests (line 5258, 5304, 5322) continue to pass without modification; they exercise the same logic via the public Mythos paths.

## Engine — driver shape

File: `crates/game-core/src/engine/dispatch.rs`.

### `enemy_phase`

Entered by `step_phase` on the Investigation→Enemy transition.

```rust
/// Entered by [`step_phase`] on the Investigation→Enemy transition.
/// Owns the `PhaseStarted(Enemy)` emit (Rules Reference p.25 step 3.1)
/// and kicks off the per-investigator attack loop (step 3.3) by
/// seeding [`GameState::enemy_attack_pending`] and opening the first
/// [`WindowKind::BeforeInvestigatorAttacked`] window. The loop body
/// runs in [`run_window_continuation`]'s arms; this driver returns
/// after the kickoff.
///
/// Hunter movement (step 3.2) is a named TODO stub
/// ([`hunter_movement_step`]) deferred to #128.
fn enemy_phase(state: &mut GameState, events: &mut Vec<Event>) {
    // 3.1 Enemy phase begins.
    events.push(Event::PhaseStarted { phase: Phase::Enemy });

    // 3.2 Hunter enemies move. TODO(#128).
    hunter_movement_step(state, events);

    // 3.3 Kick off the per-investigator attack loop. Seed the cursor
    //     to the first Active investigator in turn_order. Eliminated
    //     investigators (Killed / Insane / Resigned) are skipped per
    //     Rules Reference p.10 (Elimination); `first_active_investigator`
    //     is the shared helper used by Mythos 1.4 (#69) for the same
    //     semantics.
    state.enemy_attack_pending = first_active_investigator(state);

    if state.enemy_attack_pending.is_some() {
        open_fast_window(state, events, WindowKind::BeforeInvestigatorAttacked);
    } else {
        // No Active investigators (turn_order empty or all eliminated).
        // Skip straight to the final window (mirror of mythos_phase's
        // no-drawer path at dispatch.rs:892-900).
        open_fast_window(state, events, WindowKind::AfterAllInvestigatorsAttacked);
    }
}
```

### `hunter_movement_step` (TODO stub)

```rust
/// 3.2 Hunter enemies move. Rules Reference p.25: "Resolve the hunter
/// keyword for each ready, unengaged enemy that has the hunter
/// keyword."
fn hunter_movement_step(_state: &mut GameState, _events: &mut Vec<Event>) {
    // TODO(#128): iterate ready unengaged enemies with the Hunter
    //             keyword; BFS over location-connection graph; move +
    //             engage-on-arrival. Ambiguous shortest paths prompt
    //             the active investigator via AwaitingInput +
    //             InputResponse::PickLocation. Currently no Hunter
    //             keyword exists on CardMetadata; #128 lands it
    //             alongside this body.
}
```

Precedent: `place_doom_on_agenda` / `check_doom_threshold` (TODO #73) and `check_hand_size` (TODO #111) are the same named-stub pattern.

### `resolve_attacks_for_investigator`

The per-investigator attack loop. Runs in `run_window_continuation`'s `BeforeInvestigatorAttacked` arm.

```rust
/// Resolve all of one investigator's engaged ready enemies' attacks
/// (Rules Reference p.25 step 3.3 inner body). Snapshot the attacker
/// list in [`EnemyId`] order, then for each attacker:
///
/// 1. Early-break if `investigator` is no longer [`Status::Active`]
///    (defeated by an earlier attack in the same loop). Remaining
///    attackers do not attack and do not exhaust, per Rules
///    Reference p.10 Elimination step 3 ("All enemies engaged with
///    that player are placed at the location … unengaged but
///    otherwise maintaining their current game state") and p.25
///    ("Each ready, engaged enemy makes an attack" — a disengaged
///    enemy is not "engaged").
///
///    Today `apply_investigator_defeat` only flips `Status`; the full
///    disengage + re-engage flow lands in a follow-up
///    (`TODO(#144)`). The early-break here is the
///    rules-correct minimal interpretation: no incorrect events
///    fire, no behavior anomaly visible. After the elimination-flow
///    PR lands, the `enemy.engaged_with` field is properly cleared
///    on defeat too; this early-break stays as the simpler form.
///
/// 2. Call [`enemy_attack`] (places damage + horror simultaneously,
///    fires defeat if either crosses).
///
/// 3. Set `enemy.exhausted = true`, emit
///    [`Event::EnemyExhausted { enemy }`]. Per Rules Reference p.25,
///    exhaustion happens "Upon completion of dealing the attack (and
///    all abilities triggered by the attack)" — there is no
///    carve-out for "the attack defeated the target," so an attack
///    that lands and defeats its target still exhausts the attacker.
///    "All abilities triggered by the attack" is satisfied vacuously
///    today (no `EventPattern` matches `DamageTaken` / `HorrorTaken`
///    / `EnemyDefeated`-from-attack); the first PR adding such a
///    pattern must revisit this loop's atomicity (see the doc-comment
///    block below).
///
/// **Atomicity invariant:** the snapshot + loop run as a block
/// within [`run_window_continuation`]'s arm — no Fast plays or
/// reactions interpose mid-loop. The first PR that adds a reaction
/// `EventPattern` matching events emitted inside this loop
/// (`DamageTaken` / `HorrorTaken` / `EnemyExhausted` /
/// `EnemyDefeated`-from-attack) must persist the remaining-attackers
/// list on `GameState` (analogous to `enemy_attack_pending`) so
/// resume-after-pause re-enters the right iteration point.
///
/// **Attack order:** deterministic by [`EnemyId`]. Rules Reference
/// p.25 prescribes "the order of the attacked investigator's
/// choosing" when an investigator is engaged with multiple enemies;
/// `TODO(#143)` covers that. Mirrors the same limitation in
/// [`fire_attacks_of_opportunity`].
fn resolve_attacks_for_investigator(
    state: &mut GameState,
    events: &mut Vec<Event>,
    investigator: InvestigatorId,
) {
    // Snapshot ready engaged attackers in deterministic EnemyId
    // order. BTreeMap iteration is already sorted by key.
    let attackers: Vec<EnemyId> = state
        .enemies
        .iter()
        .filter(|(_, e)| e.engaged_with == Some(investigator) && !e.exhausted)
        .map(|(id, _)| *id)
        .collect();

    for enemy_id in attackers {
        // Early-break on defeat (rules-correct stub until the
        // elimination-flow follow-up lands; see fn doc above).
        let active = state.investigators.get(&investigator)
            .is_some_and(|inv| inv.status == Status::Active);
        if !active {
            break;
        }

        // Damage + horror placement (simultaneous per p.7) + defeat.
        enemy_attack(state, events, enemy_id, investigator);

        // Exhaust the attacker post-resolution.
        let enemy = state.enemies.get_mut(&enemy_id).unwrap_or_else(|| {
            unreachable!(
                "resolve_attacks_for_investigator: snapshotted enemy \
                 {enemy_id:?} is gone from state.enemies; this is a \
                 state-corruption invariant violation"
            )
        });
        enemy.exhausted = true;
        events.push(Event::EnemyExhausted { enemy: enemy_id });
    }
}
```

### `enemy_phase_end`

```rust
/// Called from [`run_window_continuation`]'s
/// [`WindowKind::AfterAllInvestigatorsAttacked`] arm. Emits step
/// 3.4's `PhaseEnded(Enemy)` marker, then transitions to Upkeep.
/// Exact analog of [`mythos_phase_end`] / [`upkeep_phase_end`].
fn enemy_phase_end(state: &mut GameState, events: &mut Vec<Event>) {
    // 3.4 Enemy phase ends.
    events.push(Event::PhaseEnded { phase: Phase::Enemy });
    step_phase(state, events); // Enemy → Upkeep; calls upkeep_phase
}
```

## Engine — `step_phase` edits

File: `crates/game-core/src/engine/dispatch.rs:937`.

```rust
fn step_phase(state: &mut GameState, events: &mut Vec<Event>) {
    let from = state.phase;
    let to = from.next();

    // PhaseEnded suppression — driver-owned phases handle their own emit.
    if from != Phase::Mythos && from != Phase::Upkeep && from != Phase::Enemy {
        events.push(Event::PhaseEnded { phase: from });
    }

    state.phase = to;

    match to {
        Phase::Mythos if from != Phase::Mythos => mythos_phase(state, events),
        Phase::Investigation if from != Phase::Investigation => investigation_phase(state, events),
        Phase::Enemy if from != Phase::Enemy => enemy_phase(state, events),
        Phase::Upkeep if from != Phase::Upkeep => upkeep_phase(state, events),
        _ => unreachable!(
            "step_phase: from == to (from={from:?}, to={to:?}); Phase::next \
             never returns the same phase, so this branch is structurally \
             unreachable. If it ever fires, something has corrupted \
             state.phase between the read and the dispatch."
        ),
    }
}
```

The `_ =>` arm is structurally unreachable once all four phases are driver-dispatched: `Phase::next()` cycles `Mythos → Investigation → Enemy → Upkeep → Mythos` and never returns its input, so `from != to` always holds and one of the four guarded arms always matches. If this branch ever fires, it's a state-corruption invariant violation, not a normal fallback — make it loud.

The `PhaseEnded(Mythos)` suppression doc-comment block at lines 922–936 grows to mention Enemy alongside Mythos and Upkeep.

The Investigation skeleton driver (line 828) is untouched — its `PhaseEnded(Investigation)` is still emitted by `step_phase`'s now-shorter suppression-set. #137 will land the full Investigation driver and add `from != Phase::Investigation` to the suppression set.

The `end_turn` doc-comment at line 804 (`step_phase(state, events); // Investigation → Enemy (empty until #71)`) updates to remove the "empty until #71" qualifier.

## Engine — `run_window_continuation` edits

File: `crates/game-core/src/engine/dispatch.rs:3712`. Two new arms.

```rust
fn run_window_continuation(state: &mut GameState, events: &mut Vec<Event>, kind: WindowKind) {
    match kind {
        WindowKind::MythosAfterDraws => { /* … existing … */ }
        WindowKind::UpkeepBegins => { /* … existing … */ }

        WindowKind::BeforeInvestigatorAttacked => {
            // Phase-transitioning continuation (advances to next window
            // and ultimately to Upkeep) — cannot run while a skill test
            // is in flight (would strand it). Phase 4 has no Enemy-phase
            // skill-test source, so this branch is structurally
            // unreachable today. A future PR adding one (e.g. a
            // treachery-style "make an Agility test or take damage"
            // attack ability) must redesign the window-close +
            // phase-transition ordering before this assertion fires.
            if let Some(in_flight) = state.in_flight_skill_test.as_ref() {
                unreachable!(
                    "BeforeInvestigatorAttacked window closed while a \
                     skill test is in flight (continuation={:?}). Phase \
                     transition would strand the skill test in the wrong \
                     phase. Phase 4 has no Enemy-phase skill test \
                     sources; if a future PR adds one, the window-close \
                     + phase-transition ordering needs redesign before \
                     this assertion can be relaxed.",
                    in_flight.continuation,
                );
            }

            // Cursor expect-Some: BeforeInvestigatorAttacked is only
            // ever opened after `enemy_attack_pending` is set to
            // Some(_) in `enemy_phase` or in the advance below. A
            // None cursor here is a state-corruption invariant
            // violation, not a normal rejection path.
            let investigator = state.enemy_attack_pending.unwrap_or_else(|| {
                unreachable!(
                    "BeforeInvestigatorAttacked closed with \
                     enemy_attack_pending == None; this is a \
                     state-corruption invariant violation"
                )
            });

            resolve_attacks_for_investigator(state, events, investigator);

            // Advance cursor: next Active investigator AFTER
            // `investigator` in turn_order. The shared helper uses
            // turn_order (not the filtered-Active list) as the index
            // basis so `investigator` itself can have been defeated
            // mid-loop and we still find the right successor.
            state.enemy_attack_pending = next_active_investigator_after(state, investigator);

            if state.enemy_attack_pending.is_some() {
                open_fast_window(state, events, WindowKind::BeforeInvestigatorAttacked);
            } else {
                open_fast_window(state, events, WindowKind::AfterAllInvestigatorsAttacked);
            }
        }

        WindowKind::AfterAllInvestigatorsAttacked => {
            if let Some(in_flight) = state.in_flight_skill_test.as_ref() {
                unreachable!(
                    "AfterAllInvestigatorsAttacked window closed while a \
                     skill test is in flight (continuation={:?}). Phase 4 \
                     has no Enemy-phase skill-test sources; a future PR \
                     adding one needs the window-close + phase-transition \
                     ordering redesigned before this fires.",
                    in_flight.continuation,
                );
            }
            enemy_phase_end(state, events);
        }

        WindowKind::AfterEnemyDefeated { .. } | WindowKind::BetweenPhases { .. } => {}
    }
}
```

## Data flow

### Happy-path cascade — 1 Active investigator with 1 engaged ready enemy (Phase 4 synthetic fixture)

```
end_turn (final Investigation turn)
 └─ step_phase  Investigation→Enemy
     ├─ PhaseEnded(Investigation)          ← step_phase emits (no Investigation _end yet; #137)
     └─ enemy_phase
         ├─ PhaseStarted(Enemy)            ← driver-owned (step 3.1)
         ├─ hunter_movement_step           ← no-op TODO(#128)
         ├─ enemy_attack_pending = Some(inv1)
         └─ open_fast_window(BeforeInvestigatorAttacked)
             ├─ WindowOpened(BeforeInvestigatorAttacked)
             ├─ scan + eligibility both empty → auto-skip
             ├─ WindowClosed(BeforeInvestigatorAttacked)
             └─ run_window_continuation(BeforeInvestigatorAttacked)
                 ├─ skill-test-in-flight assertion (None today)
                 ├─ resolve_attacks_for_investigator(inv1):
                 │   ├─ snapshot attackers = [E]
                 │   └─ for E in snapshot:
                 │       ├─ active check passes
                 │       ├─ enemy_attack(E, inv1) → DamageTaken { amount } [/ HorrorTaken / InvestigatorDefeated]
                 │       ├─ enemy.exhausted = true
                 │       └─ EnemyExhausted { enemy: E }
                 ├─ enemy_attack_pending = None (no next Active investigator)
                 └─ open_fast_window(AfterAllInvestigatorsAttacked)
                     ├─ WindowOpened(AfterAllInvestigatorsAttacked)
                     ├─ auto-skip
                     ├─ WindowClosed(AfterAllInvestigatorsAttacked)
                     └─ run_window_continuation(AfterAllInvestigatorsAttacked)
                         ├─ skill-test-in-flight assertion (None today)
                         └─ enemy_phase_end
                             ├─ PhaseEnded(Enemy)        ← driver-owned (step 3.4)
                             └─ step_phase  Enemy→Upkeep → upkeep_phase → (Upkeep cascade per #70)
```

All inside one `apply()` call returning `EngineOutcome::Done`.

### Pause/resume — Fast play eligible during a per-investigator window

If a Fast event is eligible when `BeforeInvestigatorAttacked` opens (the investigator owns a Fast event and `check_play_card`'s timing gate accepts the current `open_windows.last()`), the window stays on the stack and `apply` returns `AwaitingInput`. State at the pause point:

- `state.phase == Phase::Enemy`
- `state.enemy_attack_pending == Some(X)` — preserved across `apply` calls
- `state.open_windows.last() == BeforeInvestigatorAttacked`
- `state.active_investigator == None` (set by `end_turn`; Enemy phase doesn't rotate)

Resume comes via `PlayerAction::PlayCard` (Fast — leaves window open, repeat eligibility scan) or `PlayerAction::ResolveInput(InputResponse::Skip)` (pops the window → `close_reaction_window_at` → `run_window_continuation` → loop continues from cursor).

### Edge cases

- **Investigator X defeated mid-attack-resolution.** Attack N fires and defeats X via `apply_investigator_defeat`. E_N still exhausts (defeat check is *after* damage/horror placement; "completion of dealing the attack" is satisfied). Iteration N+1's top-of-loop active check fails → break. Attacks N+1…M do not fire and do not exhaust. Their `enemy.engaged_with == Some(X)` field stays stale until #144 lands; flagged via `TODO(#144)` on the driver doc-comment.
- **`turn_order` empty or all eliminated.** `enemy_phase` finds no first-Active → `enemy_attack_pending = None` → opens `AfterAllInvestigatorsAttacked` directly. Mirror of `mythos_phase`'s no-drawer path.
- **Investigator with no engaged ready enemies.** `resolve_attacks_for_investigator`'s snapshot is empty, loop is a no-op, cursor advances. `WindowOpened` / `WindowClosed` emit pair still fires (printed timing point is per-investigator regardless of whether attacks happen).
- **Exhausted engaged enemy.** Filtered out by the snapshot's `!e.exhausted` predicate. No attack, no exhaust (already exhausted).
- **Unengaged ready enemy at same location.** Filtered out by the `engaged_with == Some(investigator)` predicate. Re-engagement timing is governed by Rules p.10 Enemy Engagement (post-defeat re-engagement is in the elimination follow-up; spawn-time engagement is #127).

### Replay determinism

No new randomness. No new `EngineRecord` variants. Action-log replay is bit-for-bit identical without further plumbing. `enemy_attack_pending: Option<InvestigatorId>` serializes via standard serde derives.

## Tests

All in `crates/game-core/src/engine/dispatch.rs`'s `#[cfg(test)]` block (engine unit layer). No `crates/cards/tests/` work (no card content); no scenario-fixture changes (the synthetic fixture stays 1-investigator).

**Driver shape (5):**

1. `enemy_phase_emits_phase_started_and_cascades_to_upkeep` — 1 Active investigator, no engaged enemies. Full event sequence asserted positionally: `PhaseStarted(Enemy)` < `WindowOpened(BeforeInvestigatorAttacked)` < `WindowClosed(BeforeInvestigatorAttacked)` < `WindowOpened(AfterAllInvestigatorsAttacked)` < `WindowClosed(AfterAllInvestigatorsAttacked)` < `PhaseEnded(Enemy)`. State lands in Mythos.
2. `enemy_phase_with_two_investigators_iterates_in_turn_order` — 2 Active, no engaged enemies. Verify two `BeforeInvestigatorAttacked` windows + one `AfterAllInvestigatorsAttacked` window between `PhaseStarted(Enemy)` and `PhaseEnded(Enemy)`. Verify `enemy_attack_pending == None` at the end.
3. `enemy_phase_skips_eliminated_investigator_in_advance` — 3 turn_order entries, middle has `Status::Insane`. Verify only two `BeforeInvestigatorAttacked` windows (first + third), plus the final `AfterAllInvestigatorsAttacked`. Cursor never lands on the eliminated investigator.
4. `enemy_phase_with_all_investigators_eliminated_opens_after_all_directly` — turn_order entries all non-Active (Killed/Insane/Resigned). Verify no `BeforeInvestigatorAttacked` window fires; only `AfterAllInvestigatorsAttacked`. Mirror of `mythos_phase_with_all_investigators_eliminated_opens_after_draws_window`.
5. `enemy_phase_with_empty_turn_order_cascades_to_upkeep` — empty `turn_order`. Cascade lands in Mythos via auto-skip.

**Attack resolution (4):**

6. `resolve_attacks_for_investigator_fires_engaged_ready_enemy_attack_and_exhausts` — 1 investigator + 1 engaged ready enemy, `attack_damage = 1`, `attack_horror = 0`. Verify ordered: `DamageTaken { amount: 1 }` < `EnemyExhausted`. State: `enemy.exhausted == true`.
7. `resolve_attacks_for_investigator_excludes_exhausted_and_unengaged_enemies` — one already-exhausted engaged enemy + one ready unengaged enemy + one ready engaged enemy. Verify only the ready engaged enemy's `DamageTaken` + `EnemyExhausted` fire.
8. `resolve_attacks_for_investigator_iterates_multiple_attackers_in_enemy_id_order` — 2 engaged ready enemies with distinct `attack_damage` values; verify the `DamageTaken` sequence matches `EnemyId` order.
9. `resolve_attacks_for_investigator_early_breaks_on_defeat` — investigator with `max_health = 1` engaged with 2 enemies (`EnemyId(1)`, `EnemyId(2)`), both `attack_damage = 1`. Verify: `EnemyId(1)` fires attack + exhaust (defeats investigator) → `EnemyId(2)` does **not** emit `DamageTaken` or `EnemyExhausted`; `state.enemies[&EnemyId(2)].exhausted == false`.

**Pause/resume (2):**

10. `enemy_phase_pauses_when_fast_play_eligible` — investigator with a Fast event in hand + resources. `step_phase` Investigation→Enemy. Verify `apply` returns `AwaitingInput`; `state.open_windows.last() == BeforeInvestigatorAttacked`; `state.enemy_attack_pending == Some(inv)`; `state.phase == Phase::Enemy`.
11. `enemy_phase_resumes_via_skip_input` — from (10)'s state, submit `PlayerAction::ResolveInput(InputResponse::Skip)`. Verify the window closes, attacks resolve (if any), cursor advances or `AfterAllInvestigatorsAttacked` opens, cascade continues.

**`step_phase` wiring (1):**

12. `step_phase_from_enemy_suppresses_phase_ended_emit` — assert directly that `step_phase` does not emit `PhaseEnded(Enemy)` (because `enemy_phase_end` owns it). Mirror of the implicit suppression test pattern for Mythos/Upkeep.

**Shared cursor helpers (2):**

13. `first_active_investigator_finds_first_active_skipping_eliminated` — `turn_order = [eliminated, eliminated, active, active]`; expect `Some(active_1)`. Covers empty `turn_order` → `None` and all-eliminated → `None` via sub-cases.
14. `next_active_investigator_after_skips_eliminated_and_returns_none_past_end` — `turn_order = [a, b_eliminated, c, d]`; `next_after(a) == Some(c)`; `next_after(c) == Some(d)`; `next_after(d) == None`. Sub-case: `next_after(id_not_in_turn_order) == None`.

**Total: 14 tests.**

Use `assert_event!` / `assert_no_event!` / `assert_event_sequence!` macros per CLAUDE.md guidance. Use `assert_eq!` on the events slice only for tests 1 and 5 (cascade ordering where exact contiguous order matters).

## Open questions

- *Multi-investigator Fast-play scope.* `fast_actors: FastActorScope::Any` matches Mythos/Upkeep. A future "Fast: before you're attacked" card may want to restrict to just the to-be-attacked investigator. Resolved per-card at card time or by narrowing `fast_actors` at the open site when a consumer lands.
- *Engine accessor exposing the cursor to cards.* Bare `WindowKind` variants force this. The consolidation-into-single-variant follow-up may expose a generic "current actor" accessor; out of scope for #71.

## Decisions made (for the phase-doc Decisions table after PR merge)

- **Per-investigator + final windows, bare variants, cursor on `enemy_attack_pending`.** Mirror of `mythos_draw_pending` rather than a payload-carrying `WindowKind`. Reasoning: the consolidation-into-single-variant follow-up's option space is preserved; the per-investigator window subsumes the rules' "return to the previous player window" reading without inventing a separate inter-step window. Phase-4-doc-relevant.
- **Early-break on `Status != Active` inside `resolve_attacks_for_investigator`** is the rules-correct minimal interpretation until **#144** lands. The early-break stays as the simpler form even after that PR (when `enemy.engaged_with` is also properly cleared on defeat).
- **Deterministic `EnemyId` attack order** at both call sites (Enemy-phase 3.3 + AoO). Player-pick deferred to **#143** (unmilestoned), shared by both sites.
- **No `Event::EnemyAttacked`** — concrete-consumer-first.
- **`hunter_movement_step` is a named TODO stub** for #128; matches `place_doom_on_agenda` / `check_doom_threshold` / `check_hand_size` precedent.
- **#144 scope** is all of Rules p.10 steps 1–5 (formalize the defeat flow everywhere `apply_investigator_defeat` is called), scheduled **after #128** so multi-investigator re-engagement uses prey logic directly. Phase-4 milestone, `blocked` label.
- **Shared cursor helpers extracted from Mythos.** `first_active_investigator` + `next_active_investigator_after` replace duplicated inline lookups at Mythos's seed and advance sites; Enemy reuses both. Surfacing the shared semantics in one place means the eliminated-skip predicate (Rules p.10) has a single canonical site.
- **`step_phase`'s `_` arm is `unreachable!`, not a defensive emit.** Once Enemy is driver-dispatched, all four `Phase::next()` outputs are matched and `from == to` cannot occur. Reachability changes from "defensive fallback we don't actually need" to "state-corruption invariant violation if it fires."

## Follow-ups filed alongside this spec

- **#143** (unmilestoned, `engine` + `p2-later`): "Player picks engaged-enemy attack order (Enemy phase 3.3 + attacks of opportunity)." Both call sites switch from deterministic `EnemyId` order to an `AwaitingInput` / `InputResponse` flow. Pulled in when a multi-engagement + multi-investigator scenario forces it (Phase 7+).
- **#144** (Phase-4 milestone, `engine` + `p2-later` + `blocked`): "Formalize investigator elimination flow (Rules Reference p.10 Elimination steps 1–5)." Extends `apply_investigator_defeat` to apply all five steps: cards removed from game; clues at location + resources returned; disengage + single-investigator re-engagement (multi-investigator uses #128's prey logic directly); threat-area discard; lead-investigator transfer. Step 6 (scenario end) consolidates with existing `check_all_defeated`. Blocked on #128.
