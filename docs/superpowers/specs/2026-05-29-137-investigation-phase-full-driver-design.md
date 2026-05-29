# #137 — Investigation phase full driver (design)

GitHub issue: [#137](https://github.com/talelburg/eldritch/issues/137) · Phase: [phase-4-scenario-plumbing](../../phases/phase-4-scenario-plumbing.md) (ordering slot 9) · Depends on #69 (phase-driver pattern + `open_fast_window`, PR #136) and #71 (shared cursor helpers + bare-window shape, PR #145) — both shipped.

## Goal

Complete the **phase-driver-owns-its-boundary-emits** pattern for the
Investigation phase, so it matches Mythos / Upkeep / Enemy: a driver that owns
`PhaseStarted` [2.1], named step functions for the printed sub-steps, player
windows at the printed timing points, and an `investigation_phase_end` helper
that owns `PhaseEnded` [2.3]. Once Investigation owns its `PhaseEnded`, all four
phases own their boundary emits and `step_phase`'s conditional `PhaseEnded` emit
is fully dead and is deleted.

The phase's **action-taking middle stays player-driven** (`Investigate` / `Move`
/ `Fight` / `Evade` / `PlayCard` / `Draw` / `ActivateAbility` / `EndTurn` as
today). This PR adds the structural skeleton and boundary windows; it does **not**
add multiplayer turn-order choice or per-action Fast-window re-opens (see
Follow-ups).

## Rules grounding (Rules Reference, Fantasy Flight `ahc01`)

Investigation phase, p.24:

- **2.1 Investigation phase begins.** *"This step formalizes the beginning of the
  investigation phase."*
- **2.2 Next investigator's turn begins.** *"The investigators may take their
  turns in any order. The investigators choose among themselves who … will take
  this turn, and making this choice begins that investigator's turn. … Once an
  investigator begins a turn, that investigator must complete the turn before
  another investigator may take his or her turn. Each investigator takes one turn
  each round."*
- **2.2.1 Investigator takes an action, if able.** *"After an investigator takes
  an action, return to the previous player window. An investigator may end his or
  her turn early … If the investigator does not or cannot take an action, proceed
  to 2.2.2."*
- **2.2.2 Investigator's turn ends.** *"If there is an investigator who has not
  yet taken a turn this round, return to 2.2. If each investigator has taken a
  turn this round, proceed to 2.3."*
- **2.3 Investigation phase ends.** *"This step formalizes the end of the
  investigation phase."*

Setup / mulligan timing:

- Setup step 8, p.27: *"Draw opening hands. Each player draws 5 cards. Each
  player, in player order, may mulligan once at this time."*
- p.27: *"There are no action windows during setup. Players may only trigger
  player card abilities or play cards from hand during setup if the card or
  ability's specific triggering condition is met."*
- The first game round begins **after** setup; per p.24 round 1 skips the Mythos
  phase, so the first phase that begins is Investigation 2.1.

The load-bearing consequence: the Investigation phase (and its post-2.1 player
window) must begin **after** the mulligan window closes — never during setup.

## Current state (before this PR)

- `investigation_phase` (dispatch.rs:831) emits `PhaseStarted(Investigation)` [2.1]
  and immediately rotates to the first `Status::Active` investigator [2.2,
  lead-first]. No player windows.
- `step_phase` (dispatch.rs:941) emits `PhaseEnded(Investigation)` [2.3] via its
  fallback `if from != Mythos && from != Upkeep && from != Enemy` — Investigation
  is the one phase not yet in the suppression set.
- `end_turn` (dispatch.rs:755) drains actions + emits `TurnEnded` [2.2.2], then
  either rotates to the next `turn_order` entry or sets `active = None` and calls
  `step_phase` directly (→ `enemy_phase`).
- Two entry paths into the driver: `start_scenario` (dispatch.rs:750, round 1)
  calls `investigation_phase` **during setup, with the mulligan window open**; and
  `mythos_phase_end → step_phase` (round ≥2).
- All four `step_phase` call sites (814 `end_turn`, 3003 `enemy_phase_end`, 3730
  `mythos_phase_end`, 3769 `upkeep_phase_end`) are `*_end` helpers; after this PR
  every one emits `PhaseEnded(from)` itself.
- Fast-play gate (`check_play_card`, dispatch.rs:3355): the **active** investigator
  may play Fast cards during Investigation purely via
  `active_during_investigation` — no open window required. An open window's
  `permissive_window` only matters for **non-active** investigators (multiplayer)
  and for phase-boundary timing.

## Design

### Approach selected

**A1 — full driver + boundary windows, turn-order *choice* deferred.** Land the
named-step driver, `investigation_phase_end`, and the two boundary windows so the
phase's structure mirrors the printed rules. Defer the multiplayer-only turn-order
choice machinery and the 2.2.1 between-action window re-opens to a Phase-8
follow-up, because the active investigator can already Fast-play during their turn
without a window — the deferred pieces have no in-scope (single-investigator)
consumer.

Approaches considered and rejected:

- *Boundary-only (no windows).* Rejected: the user wants the phase structure to
  follow the printed rules, including the player windows at 2.1 / 2.2, even ahead
  of a Fast consumer.
- *Full rules fidelity in one PR.* Rejected: the 2.2.1 "return to the previous
  player window after each action" touches all 8 action handlers and only matters
  for non-active investigators' Fast plays and post-action reaction triggers —
  neither has a Phase-4 consumer.

### Phase-begins-after-setup (mulligan kickoff)

`start_scenario` no longer emits `PhaseStarted(Investigation)` or rotates. It
keeps setting `state.phase = Phase::Investigation` (the existing position-field
convention; there is no `Setup` phase variant), deals hands, opens the mulligan
window, and seeds round-1 actions — but the Investigation phase does not *begin*
yet.

The round-1 kickoff moves to the mulligan-completion site in `apply_player_action`
(currently dispatch.rs:165–170, right after `state.mulligan_window = false`). When
the last mulligan completes — "the game begins" — call `investigation_phase(state,
events)`. This fires exactly once per scenario (the mulligan window only ever
opens once, in `start_scenario`).

Effects:

- `PhaseStarted(Investigation)` now appears in the final `Mulligan` action's event
  stream, not in `StartScenario`'s. Integration tests that asserted it under
  `StartScenario` are updated.
- `active_investigator` is `None` throughout setup (rotation happens in the
  post-2.1 window continuation, after the phase begins). The `Mulligan` action
  carries its own `investigator` payload, so it does not depend on
  `active_investigator`.
- No player window exists during setup → the
  setup-has-no-action-windows rule holds structurally, and the
  window-during-mulligan deadlock that an earlier draft worried about cannot
  occur. **No `any_fast_play_eligible` guard is added.**

Round ≥2 entry (`mythos_phase_end → step_phase` → `investigation_phase`) is
unchanged in trigger; it picks up the same driver.

### Named-step driver structure

Mirrors how `mythos_phase` / `upkeep_resume` decompose into named step functions.
Reuses the existing shared cursor helpers `first_active_investigator` (#69) and
`next_active_investigator_after` (#71), exactly as Mythos and Enemy do, so the
eliminated-investigator skip (Rules Reference p.10) is shared.

```
investigation_phase(state, events)                 // 2.1 Investigation phase begins
    emit PhaseStarted(Investigation)
    open_fast_window(InvestigationBegins)           //    post-2.1 player window
    // entered from: mulligan-completion (round 1) AND step_phase (round >= 2)

// run_window_continuation arms:
InvestigationBegins =>                              // post-2.1 window closed -> start first turn
    match first_active_investigator(state):
        Some(id) => begin_investigator_turn(state, events, id)
        None     => { /* PARK: no active investigator can take a turn.
                         Do NOT call investigation_phase_end. See the
                         "All-eliminated / no-active-investigator handling"
                         section below for the full rationale and the
                         TODO(#144) wording. */ }

begin_investigator_turn(state, events, who)         // 2.2 Next investigator's turn begins
    rotate_to_active(who)
    open_fast_window(InvestigatorTurnBegins)        //    post-2.2 player window
    // called from: InvestigationBegins continuation AND end_turn

InvestigatorTurnBegins => { /* no-op */ }           // 2.2.1 active investigator takes actions
                                                    //    (player-driven via Investigate/Move/...;
                                                    //    this window IS "the previous player window";
                                                    //    documented marker, no fn body)

end_turn(state, events)                             // 2.2.2 Investigator's turn ends (PlayerAction)
    validate: phase == Investigation, active == Some(_)
    drain actions; emit TurnEnded
    match next_active_investigator_after(state, current):
        Some(next) => begin_investigator_turn(state, events, next)   // "return to 2.2"
        None       => investigation_phase_end(state, events)         // -> 2.3

investigation_phase_end(state, events)              // 2.3 Investigation phase ends
    emit PhaseEnded(Investigation)
    step_phase(state, events)                       //    -> Enemy (enemy_phase)
```

Notes:

- **Rotation moves out of `investigation_phase`'s body into the
  `InvestigationBegins` continuation**, because the printed order is 2.1 → *window*
  → 2.2 (rotate). In the common single-investigator case nothing is Fast-eligible,
  so `open_fast_window` auto-skips inline and the continuation runs in the same
  `apply()` — rotation still lands in the same call, plus a `WindowOpened` /
  `WindowClosed` pair.
- `end_turn` is **not** split into a separate `end_investigator_turn`; the
  validate-then-act indirection was redundant. One fn owns step 2.2.2.
- **No trailing post-2.3 window.** The next player window after 2.3 is the Enemy
  phase's `BeforeInvestigatorAttacked`; the issue lists only post-2.1 / post-2.2.
- **2.2.1 has no function body.** It is the player-driven action loop realized by
  the existing action handlers (analogous to how Mythos 1.4 is realized by the
  `DrawEncounterCard` handler, not a step fn). It is represented as a documented
  marker at the `InvestigatorTurnBegins` continuation.

### All-eliminated / no-active-investigator handling (and why #137 stays first)

The `InvestigationBegins` continuation parks (does nothing) when
`first_active_investigator` returns `None`. This case is reachable in real
single-investigator play: the lone investigator ends their last turn → an enemy
attack defeats them during the Enemy phase → Upkeep and Mythos both auto-skip
(they already handle all-eliminated, per #70 / #71) → the cascade arrives at
Investigation with zero `Status::Active` investigators.

Parking — rather than calling `investigation_phase_end` — is **load-bearing**:
Investigation is the round cascade's only natural pause point. Every other phase
auto-skips its windows when no investigator is active, so auto-advancing out of
Investigation would loop `Investigation → Enemy → Upkeep → Mythos →
Investigation` forever. Parking matches today's behavior exactly (the current
`investigation_phase` emits `PhaseStarted`, fails to rotate, and returns without
advancing).

What the rules actually prescribe here is a framework scenario-end, **not** a
phase advance — Rules Reference p.10 (Elimination, step 6): *"If there are no
remaining players, the scenario ends. Refer to 'no resolution was reached' entry
for that scenario in the campaign guide."* And p.22: *"Should the scenario end
with no resolution being reached (for example, if all investigators have been
eliminated or have resigned), instructions … can be found in the … campaign
guide."* The trigger is universal (every scenario); the outcome is per-scenario.

Engine reality today: `check_all_defeated` (dispatch.rs:2753) emits
`Event::AllInvestigatorsDefeated` when no `Active` investigator remains, but it
does **not** end the scenario or fire a `Resolution` — the phase cascade is not
halted. So step 6 is event-only ("partial", as #144's body notes); the
scenario-end consequence is unwired.

**Sequencing decision (chosen: keep #137 first).** We considered landing the
elimination / scenario-end work first so this branch becomes unreachable:

- *Full chain `#128 → #144 → #137`* — #144 in full is blocked on #128 (only step
  3, multi-investigator re-engagement, needs the prey resolver). Heavy: a
  three-issue reorder to remove a one-line no-op.
- *Surgical precursor* — wire just `AllInvestigatorsDefeated → scenario ends
  (Lost / "no resolution reached")` (independent of #128) before #137. Overlaps
  resolution machinery owned by #73 (`ScenarioWon`/`ScenarioLost`) and #74
  (`Resolution`); ownership unsettled.
- *Keep #137 first (chosen)* — the park is correct and safe today; #144 (after
  #128) later adds the scenario-end check that makes this branch unreachable.

So #137 keeps its ordering slot. The park branch carries:
`// TODO(#144): Rules Reference p.10 step 6 — no remaining players → scenario`
`// ends. check_all_defeated already emits AllInvestigatorsDefeated; the`
`// scenario-end consequence is unwired. Until it lands, park here (prior`
`// behavior). Auto-advancing would loop the round forever.`
When the scenario-end check lands, revisit whether this becomes `unreachable!`.

### New `WindowKind` variants

Two bare variants (no payload), mirroring #71's `BeforeInvestigatorAttacked` /
`AfterAllInvestigatorsAttacked` shape, added to the `#[non_exhaustive]`
`WindowKind` enum (`state/game_state.rs`):

- `InvestigationBegins` — the post-2.1 player window. Continuation: select the
  first turn's investigator (`first_active_investigator`) and call
  `begin_investigator_turn`, or `investigation_phase_end` when there are no active
  investigators.
- `InvestigatorTurnBegins` — the post-2.2 player window, opened once per turn.
  Continuation: no-op (the engine waits for the active investigator's action
  inputs — step 2.2.1).

Both opened via the existing `open_fast_window` helper (`fast_actors: Any`, the
default for printed-rule windows). Both carry round-trip serde tests like the
existing variants.

### `run_window_continuation` skill-test-in-flight guards

The phase-transitioning continuations (`MythosAfterDraws`, `UpkeepBegins`, …)
carry an `unreachable!` guard asserting no skill test is in flight when the window
closes. For the two new variants:

- `InvestigationBegins` rotates + opens another window; it does not transition
  phase, but it runs at phase start where no skill test can be in flight. Decision:
  no guard needed; document why it cannot happen.
- `InvestigatorTurnBegins` is a no-op; no guard needed.

(Final guard wording is an implementation detail for the plan; the design
position is "no guard, documented rationale.")

### `step_phase` cleanup

After `investigation_phase_end` owns `PhaseEnded(Investigation)`, every
`step_phase` caller is a `*_end` helper that already emitted `PhaseEnded(from)`.
The conditional emit at dispatch.rs:946–948 becomes fully dead and is **deleted**
(not just extended to suppress Investigation). The doc-comment "PhaseEnded
suppression invariant" is rewritten to state the simpler invariant: every phase's
`*_end` helper owns its own `PhaseEnded`; `step_phase` emits none.

## Testing

Engine unit tests in `dispatch.rs` `#[cfg(test)]`, using the `TestGame` builder
and event-assertion macros:

- **Round-1 kickoff:** `StartScenario` does **not** emit `PhaseStarted(Investigation)`
  and leaves `active_investigator == None`; the final `Mulligan` emits
  `PhaseStarted(Investigation)`, auto-skips `InvestigationBegins` →
  `InvestigatorTurnBegins`, and leaves the lead active. (Update the existing
  `investigation_phase_*` tests around dispatch.rs:5051+ to the window-driven
  shape.)
- **Window auto-skip ordering (single investigator):** entering Investigation
  emits `PhaseStarted` then `WindowOpened(InvestigationBegins)` /
  `WindowClosed` / `WindowOpened(InvestigatorTurnBegins)` / `WindowClosed`, and
  rotates to the lead.
- **`investigation_phase_end` owns 2.3:** `EndTurn` for the last (only) investigator
  emits `TurnEnded` then `PhaseEnded(Investigation)` (from
  `investigation_phase_end`, not `step_phase`), then cascades into `enemy_phase`.
- **Two-investigator rotation:** `EndTurn` for the first investigator rotates to
  the second and re-opens `InvestigatorTurnBegins` (testable without
  `ChooseFirstActor` — strict `turn_order` rotation); `EndTurn` for the second
  ends the phase via `investigation_phase_end`.
- **All-eliminated / empty `turn_order`:** `InvestigationBegins` continuation with
  no active investigator **parks** — `active_investigator` stays `None`, no
  `PhaseEnded(Investigation)` is emitted, the phase does not advance. (Locks in the
  cascade-breaker behavior; auto-advancing would loop the round forever.)
- **`step_phase` emits no `PhaseEnded`:** a direct/structural check that the four
  transitions' `PhaseEnded` events come from the `*_end` helpers (existing
  `step_phase_from_enemy_does_not_emit_phase_ended_enemy`-style tests cover the
  pattern; add the Investigation analog).
- **Full round cascade + replay:** existing `end_turn_cascades_through_upkeep_to_mythos_draw_pending`
  and round-cycle tests updated for the new window events; action-log replay
  reproduces the stream.

## Follow-ups (filed)

1. **Phase 8 — Investigation turn-order choice (#146).** `PlayerAction::ChooseFirstActor`
   (and per-turn actor choice): the rules let investigators choose who takes each
   turn (2.2, and "return to 2.2" at 2.2.2); this PR uses lead-first / strict
   `turn_order` as a documented table convention. Also the 2.2.1 "return to the
   previous player window after each action" between-action window re-opens
   (touches all 8 action handlers; needed for non-active investigators' Fast plays
   and post-action reaction triggers). All are multiplayer-facing with no
   single-investigator consumer; land them together with a real consumer in
   Phase 8.

2. **Phase 4 — mulligan player-order cursor (#147).** Remodel mulligan to mirror Mythos
   1.4's `mythos_draw_pending`: a `mulligan_pending: Option<InvestigatorId>` cursor
   advanced in player order, with `Mulligan { investigator }` valid only when
   `pending == Some(investigator)`. Rules require mulligan "in player order"
   (p.16, p.27 step 8); the current `mulligan_window: bool` + "all `mulligan_used`"
   model is order-insensitive. The cursor collapses `mulligan_window` and the
   "all-used" completion scan (completion = cursor reaches `None`) and very likely
   removes the per-investigator `mulligan_used` flag; the setup gate in
   `apply_player_action` stays but keys off `mulligan_pending.is_some()`. Overlaps
   #137 only at the kickoff trigger (~5 lines): #137 kicks off Investigation when
   `mulligan_window` flips false; this follow-up swaps that to `mulligan_pending →
   None`. No hard blocker; recommend landing #137 first.

## Out of scope / non-goals

- Multiplayer turn-order choice and between-action windows (Follow-up 1).
- Mulligan cursor remodel (Follow-up 2).
- Any change to the action handlers (`Investigate` / `Move` / …) themselves.
- `#73` agenda/doom and `#128` hunter movement (independent Phase-4 issues).
