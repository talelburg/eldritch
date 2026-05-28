# #70 — Upkeep phase content (design)

GitHub issue: [#70](https://github.com/talelburg/eldritch/issues/70) · Phase: [phase-4-scenario-plumbing](../../phases/phase-4-scenario-plumbing.md) · Depends on #69 (Mythos phase content — phase-driver pattern + `open_fast_window` helper, PR #136) and #62 (player decks) — both shipped.

## Context

The Upkeep phase is the last phase-content piece of Phase 4's set. Today `step_phase` ticks through Upkeep in zero events. #70 wires the real per-investigator end-of-round housekeeping from Rules Reference p.25 (IV. Upkeep phase): reset actions, ready every exhausted card, each investigator draws 1 and gains 1 resource, then the round ends and play proceeds to the next Mythos phase.

#69 (PR #136) established the **phase-driver pattern** (each phase has a driver owning `PhaseStarted` as step N.1 and an end helper owning `PhaseEnded`) and the **`open_fast_window`** helper for printed-rule player windows. #70 follows that pattern exactly, with one structural inversion: Mythos's player window sits at the *end* (post-1.4), so `mythos_phase` runs its content then opens the window and `mythos_phase_end` is the short continuation; Upkeep's player window sits at the *start* (post-4.1), so the driver opens the window immediately and **all the content is the continuation**.

## Rules Reference, verbatim (p.23 flowchart + p.25 detail)

> **IV. Upkeep phase**
> 4.1 Upkeep phase begins.
> [PLAYER WINDOW]
> 4.2 Reset actions.
> 4.3 Ready each exhausted card.
> 4.4 Each investigator draws 1 card and gains 1 resource.
> 4.5 Each investigator checks hand size.
> 4.6 Upkeep phase ends. Round ends.
> *Proceed to Mythos Phase of next game round.*

Step detail (p.25):

> **4.1 Upkeep phase begins.** This step formalizes the beginning of the upkeep phase.
> **4.2 Reset actions.** Flip each investigator's mini card back to its colored side. This indicates that the investigator's actions have been reset for his or her next turn.
> **4.3 Ready exhausted cards.** Simultaneously ready each exhausted card.
> **4.4 Each investigator draws 1 card and gains 1 resource.** In player order, each investigator draws 1 card. Once those cards have been drawn, each investigator gains 1 resource.
> **4.5 Each investigator checks hand size.** In player order, each investigator with more than 8 cards in hand chooses and discards cards from his or her hand until he or she has 8 cards remaining in hand.
> **4.6 Upkeep phase ends.** This step formalizes the end of the upkeep phase. As the upkeep phase is the final phase in the round, this step also formalizes the end of the round. Any active "until the end of the round" lasting effects expire at this time. After this step is complete, play proceeds to the beginning of the mythos phase of the next game round.

Deck-out rule for 4.4's draw (inherited, not re-derived — see below):

- p.10 (Encounter Deck, mirrored for player decks): *"If the … deck is empty, shuffle the … discard pile back into the … deck."*
- p.9: *"Any ability that would shuffle a discard pile of zero cards back into a deck does not shuffle the deck."*

#70 owns sub-steps 4.1, 4.2, 4.3, 4.4, and 4.6. Sub-step 4.5 (hand-size check) is **#111**'s domain and lands as a named call-site with a TODO body — it needs an `AwaitingInput` producer for the discard choice, which #111 carries.

### What the issue text gets wrong (this spec follows the Rules Reference)

The #70 issue body paraphrases the phase as "ready cards, draw 1, gain 1 resource" per investigator. The verified rules differ in four load-bearing ways, and this design follows the rules:

1. **The issue omits 4.2 Reset actions.** It's the canonical action-refresh point (see "action-refresh relocation" below).
2. **The issue says ready "cards they control"; the rules say "each exhausted card"** — every exhausted card in play regardless of controller, **including enemies**.
3. **The issue interleaves per-investigator (ready → draw → gain); the rules separate them**: 4.3 readies *everything simultaneously*, then 4.4 does *all draws in player order, then all resource-gains*.
4. **Eliminated investigators are skipped** for 4.4 (and have no actions to reset / cards to ready), per Rules Reference p.10 and the #69 `Status::Active` precedent.

### Stale phase-doc note to correct

The Phase-4 doc's `#70` row says it "Folds in `GameState.round: u32` incremented at Mythos start." That predates #69 — `state.round` already exists. #70 does **not** add the round counter; it **relocates the increment** from `step_phase`'s generic Mythos-entry bump into `mythos_phase` step 1.1 (see "round-counter relocation" below), so the rule's "round begins" point (Rules Reference p.24: *"As this is the first framework event of the round, it [1.1] also formalizes the beginning of a new game round"*) has explicit ownership in the driver. The phase-doc note gets corrected when #70's phase-doc update lands.

## Scope

- New `WindowKind::UpkeepBegins` variant for the post-4.1 player window (payload-less, mirrors `MythosAfterDraws`).
- New `Event::CardReadied { investigator, instance_id, code }` for readying an investigator's in-play card (mirrors `Event::CardExhausted`). Enemy readying reuses the existing `Event::EnemyReadied { enemy }`.
- Upkeep driver `upkeep_phase(state, events)` invoked from `step_phase` on the Enemy→Upkeep transition; emits `PhaseStarted(Upkeep)` (step 4.1) and opens the post-4.1 window.
- Upkeep continuation `upkeep_resume(state, events)` (the window continuation); runs 4.2 / 4.3 / 4.4 / 4.5 as explicit named call sites, then hands to `upkeep_phase_end`.
- Upkeep closing helper `upkeep_phase_end(state, events)` invoked at the tail of `upkeep_resume`; emits `PhaseEnded(Upkeep)` (step 4.6) and steps to Mythos (mirror of `mythos_phase_end`).
- Sub-step helpers `reset_actions`, `ready_exhausted_cards`, `upkeep_draw_and_resource`, `check_hand_size` (the last a `#111` TODO stub).
- **Action-refresh relocation:** move the `actions_remaining = ACTIONS_PER_TURN` refresh out of `rotate_to_active` and into `reset_actions` (step 4.2). `rotate_to_active` becomes set-active-only. `start_scenario` calls `reset_actions` to seed round-1 actions.
- Refactor: extract `draw_one_with_deckout(state, events, investigator)` from the `Draw` action body so the action and Upkeep 4.4 share one deck-out code path.
- Refactor: extract `grant_resources(state, events, investigator, amount)` (saturating-add + `ResourcesGained` emit) shared by the DSL `gain_resources` and Upkeep 4.4.
- `run_window_continuation` gains a `UpkeepBegins` arm (with the same in-flight-skill-test `unreachable!` guard as `MythosAfterDraws`).
- `step_phase`: extend `PhaseEnded` suppression to cover Upkeep; add the `Phase::Upkeep` driver-dispatch arm.
- **Round-counter relocation:** move the `state.round` increment out of `step_phase`'s Mythos-entry bump and into `mythos_phase`'s step 1.1 (the rules' "round begins" point). Pure refactor — no observable change to the round value at any read site.
- `end_turn`: drop the now-redundant third explicit `step_phase` call (Upkeep→Mythos moves into `upkeep_phase_end`).
- Engine unit tests + integration tests in `crates/scenarios/tests/upkeep_phase.rs`.

## Out of scope

- **Hand-size check (4.5).** `#111` — needs an `AwaitingInput` discard-choice producer. Lands as a named TODO call-site.
- **"Skip your upkeep step" card effects** (rare; deferred per the issue body).
- **Card-level "until end of round" lasting-effect expiry** (4.6 mentions it; no lasting-effect machinery exists yet — no in-scope consumer).
- **An explicit `TurnStarted` event.** The action-refresh relocation removes `ActionsRemainingChanged`'s incidental "turn started" signal; if a consumer later needs an explicit turn-start marker, that's a separate event/PR. None in scope.
- **`RoundStarted` / `RoundEnded` events.** The rules' 4.6 "round ends" / 1.1 "round begins" language could warrant explicit events, but there's no consumer yet (likely first ones: card effects keying on "at the start of the round" or 4.6's "until the end of the round" lasting-effect expiry — Phase 9/10). Deferred until a card forces them; the relocated round-bump in `mythos_phase` step 1.1 (and `start_scenario` for round 1) is the natural future emit site. Not filing an issue now.
- **Removing the dead `WindowKind::BetweenPhases` variant.** It's never opened in production (test-fixture + serde-roundtrip only; the "phase machine opens it at every transition" doc is stale — each phase got a specific variant). Rationalizing it belongs to **#140** (collapse marker `WindowKind` variants), which would also migrate the Fast-play timing tests that inject it. Out of scope for #70.

## Engine — new event

File: `crates/game-core/src/event.rs`.

```rust
enum Event {
    // ... existing ...
    /// An investigator's in-play card was readied (flipped from
    /// exhausted to ready) — e.g. during Upkeep step 4.3. Mirror of
    /// [`Event::CardExhausted`]. Enemies readying emit
    /// [`Event::EnemyReadied`] instead.
    CardReadied {
        /// The card's controller.
        investigator: InvestigatorId,
        /// The readied in-play instance.
        instance_id: CardInstanceId,
        /// The card code (for log readability; redundant with state).
        code: CardCode,
    },
}
```

`Event::EnemyReadied { enemy }` already exists (its doc even reads "e.g. during the Upkeep phase").

## Engine — new window kind

File: `crates/game-core/src/state/game_state.rs` (the `WindowKind` enum).

```rust
#[non_exhaustive]
enum WindowKind {
    AfterEnemyDefeated { enemy: EnemyId, by: Option<InvestigatorId> },
    BetweenPhases { from: Phase, to: Phase },
    MythosAfterDraws,
    /// The player window between Rules Reference p.25 step 4.1 (upkeep
    /// phase begins) and step 4.2 (reset actions). Carries no payload —
    /// no `EventPattern` matches against it specifically today; the
    /// variant exists so the rule's printed timing point is addressable
    /// when a future card binds to it. Mirror of `MythosAfterDraws`.
    UpkeepBegins,
}
```

Adding the variant is non-breaking (`WindowKind` is `#[non_exhaustive]`). `scan_pending_triggers` returns empty for it (no `EventPattern` matches), so with no Fast content the window auto-skips — identical to `MythosAfterDraws`.

## Engine — Upkeep driver

File: `crates/game-core/src/engine/dispatch.rs`.

```rust
/// Entered by `step_phase` on the Enemy→Upkeep transition. Owns the
/// `PhaseStarted(Upkeep)` emit (step 4.1) and opens the post-4.1
/// player window. Everything from 4.2 onward runs as the window's
/// continuation (`upkeep_resume`).
///
/// Mirror of the Mythos driver, inverted: Mythos's window sits at the
/// END (post-1.4), so `mythos_phase` runs 1.1–1.4 then opens and
/// `mythos_phase_end` is the short continuation. Upkeep's window sits
/// at the START (post-4.1), so the driver opens immediately and the
/// content (4.2–4.6) is the continuation.
fn upkeep_phase(state: &mut GameState, events: &mut Vec<Event>) {
    // 4.1 Upkeep phase begins. Rules Reference p.25: "This step
    //     formalizes the beginning of the upkeep phase."
    events.push(Event::PhaseStarted { phase: Phase::Upkeep });

    // PLAYER WINDOW (post-4.1). open_fast_window auto-skips inline
    // when nothing is Fast-eligible (the Phase-4 synthetic path),
    // running `upkeep_resume` immediately via run_window_continuation;
    // otherwise the window stays open and the cascade pauses here.
    open_fast_window(state, events, WindowKind::UpkeepBegins);
}

/// The post-4.1 window continuation. Lays out steps 4.2–4.5 as
/// explicit named call sites (so the rule structure is grep-able and
/// #111 fills 4.5's TODO body without changing the driver shape),
/// then hands to `upkeep_phase_end` for 4.6 + the transition.
///
/// Invoked from `run_window_continuation` — inline via
/// `open_fast_window`'s auto-skip path, or via `close_reaction_window_at`
/// when the player closes the window with `ResolveInput::Skip`.
fn upkeep_resume(state: &mut GameState, events: &mut Vec<Event>) {
    reset_actions(state, events);            // 4.2
    ready_exhausted_cards(state, events);    // 4.3
    upkeep_draw_and_resource(state, events); // 4.4
    check_hand_size(state, events);          // 4.5 (TODO #111)
    upkeep_phase_end(state, events);         // 4.6 + transition
}

/// Owns step 4.6's `PhaseEnded(Upkeep)` emit, then transitions to
/// Mythos. Exact analog of `mythos_phase_end`. `step_phase` suppresses
/// its `PhaseEnded(Upkeep)` fallback when `from == Upkeep`.
fn upkeep_phase_end(state: &mut GameState, events: &mut Vec<Event>) {
    // 4.6 Upkeep phase ends. Round ends. Rules Reference p.25: "this
    //     step also formalizes the end of the round."
    events.push(Event::PhaseEnded { phase: Phase::Upkeep });
    step_phase(state, events); // Upkeep → Mythos; calls mythos_phase
}
```

## Engine — sub-step helpers

```rust
/// 4.2 Reset actions. Rules Reference p.25: "Flip each investigator's
/// mini card back to its colored side. This indicates that the
/// investigator's actions have been reset for his or her next turn."
///
/// This is the **canonical action-refresh site.** Sets
/// `actions_remaining = ACTIONS_PER_TURN` for each Active investigator
/// and emits `Event::ActionsRemainingChanged` when the value changes.
/// (`rotate_to_active` no longer refreshes actions — step 2.2 is just
/// "the turn begins". `start_scenario` calls this to seed round-1
/// actions, since round 1 skips Mythos and has no preceding Upkeep.)
///
/// Eliminated investigators (Killed / Insane / Resigned) are skipped
/// per Rules Reference p.10 — they take no turns and have no actions
/// to reset.
fn reset_actions(state: &mut GameState, events: &mut Vec<Event>) {
    for id in active_investigators_in_turn_order(state) {
        let inv = state.investigators.get_mut(&id).expect("from turn_order");
        if inv.actions_remaining != ACTIONS_PER_TURN {
            inv.actions_remaining = ACTIONS_PER_TURN;
            events.push(Event::ActionsRemainingChanged {
                investigator: id,
                new_count: ACTIONS_PER_TURN,
            });
        }
    }
}

/// 4.3 Ready exhausted cards. Rules Reference p.25: "Simultaneously
/// ready each exhausted card." "Each exhausted card" is every
/// exhausted card in play regardless of controller — investigator
/// in-play cards AND enemies. The readies are simultaneous, so
/// iteration order is immaterial; we iterate deterministically
/// (investigator id, then in-play order; then enemy id) for
/// reproducible event streams.
///
/// - Investigator in-play card with `exhausted == true` → flip to
///   `false`, emit `Event::CardReadied { investigator, instance_id, code }`.
/// - Enemy with `exhausted == true` → flip to `false`, emit
///   `Event::EnemyReadied { enemy }`.
///
/// Already-ready cards emit nothing.
fn ready_exhausted_cards(state: &mut GameState, events: &mut Vec<Event>) { /* see doc */ }

/// 4.4 Each investigator draws 1 card and gains 1 resource. Rules
/// Reference p.25: "In player order, each investigator draws 1 card.
/// Once those cards have been drawn, each investigator gains 1
/// resource."
///
/// Two passes over the Active investigators in turn order to honor the
/// rule's "once those cards have been drawn, [then] each … gains":
///   pass 1 — `draw_one_with_deckout` for each;
///   pass 2 — `grant_resources(.., 1)` for each.
/// (Observable only in multiplayer — for one investigator the passes
/// collapse — but free to model correctly.)
fn upkeep_draw_and_resource(state: &mut GameState, events: &mut Vec<Event>) {
    let ids = active_investigators_in_turn_order(state);
    for &id in &ids {
        draw_one_with_deckout(state, events, id);
    }
    for &id in &ids {
        grant_resources(state, events, id, 1);
    }
}

/// 4.5 Each investigator checks hand size.
fn check_hand_size(_state: &mut GameState, _events: &mut Vec<Event>) {
    // TODO(#111): in player order, each investigator with more than 8
    //   cards in hand discards down to 8 (Rules Reference p.25 step
    //   4.5). Needs an AwaitingInput producer — which cards to discard
    //   is a player decision — so it lands in #111. The call site
    //   exists so the rule step is grep-able and #111 plugs in here
    //   without changing the driver shape. (Mirror of the #73 stubs
    //   place_doom_on_agenda / check_doom_threshold.)
}
```

`active_investigators_in_turn_order(state) -> Vec<InvestigatorId>` is a small shared filter (`turn_order` entries whose `Status == Active`), reused by `reset_actions` and `upkeep_draw_and_resource`. It mirrors the `Status::Active` filtering #69 introduced for Mythos draws / Investigation rotation.

## Engine — action-refresh relocation

File: `crates/game-core/src/engine/dispatch.rs`.

Today `rotate_to_active` (`dispatch.rs:944`) does two jobs: set `active_investigator`, **and** refresh `actions_remaining = ACTIONS_PER_TURN` + emit `ActionsRemainingChanged`. The rules grant actions at Upkeep 4.2, not at turn-start (step 2.2 is just "the turn begins"). So the refresh moves to `reset_actions`:

```rust
/// Set `active_investigator` to `id`. Does NOT refresh actions —
/// actions are reset at Upkeep step 4.2 (`reset_actions`) for the
/// whole next round, and seeded for round 1 by `start_scenario`. By
/// the time an investigator becomes active their `actions_remaining`
/// already holds this round's allotment.
fn rotate_to_active(state: &mut GameState, _events: &mut Vec<Event>, id: InvestigatorId) {
    debug_assert!(state.investigators.contains_key(&id), /* state-corruption invariant */);
    state.active_investigator = Some(id);
}
```

`start_scenario` seeds round-1 actions by calling `reset_actions` once, just before `investigation_phase`:

```rust
// ... shuffle decks, deal opening hands, set mulligan_window = true ...
reset_actions(state, events); // round-1 seed: every Active investigator → ACTIONS_PER_TURN
investigation_phase(state, events); // emits PhaseStarted(Investigation), rotates to lead
```

### Consequence: `ActionsRemainingChanged` timing shifts

This is the rules-faithful model, but it changes the event stream: `ActionsRemainingChanged(ACTIONS_PER_TURN)` now fires **once per round at Upkeep 4.2** (plus the round-1 seed in `start_scenario`) rather than at each turn-start in `rotate_to_active`. Within a round, each investigator takes exactly one turn (Rules Reference 2.2.2 loop) and `end_turn` drains their actions to 0, so the steady-state "3 actions at each turn" is preserved — the reset point just moves to the rules-correct beat. A client can no longer treat `ActionsRemainingChanged` as the "your turn started" marker; the turn-start signal is the `active_investigator` state change (there is no `TurnStarted` event today, and #70 doesn't add one — out of scope).

Existing tests that assert `ActionsRemainingChanged` at rotate-time get updated to the new timing:
- `crates/game-core/src/engine/mod.rs::start_scenario_advances_to_investigation_with_round_one` — the lead's `ActionsRemainingChanged(3)` now comes from the `reset_actions` round-1 seed (still emitted; assertion holds, but the source moves).
- `dispatch.rs::investigation_phase_emits_phase_started_and_rotates_to_lead` (and peers) — `rotate_to_active` no longer emits `ActionsRemainingChanged`, so the rotate-time assertion is dropped / moved.
- Any test asserting an exact `ActionsRemainingChanged` count at scenario start updates (multi-investigator setups now see one emit per Active investigator from the round-1 seed).

## Engine — shared helpers (refactors)

File: `crates/game-core/src/engine/dispatch.rs`.

```rust
/// Draw one card for `investigator`, applying the empty-deck rule:
/// reshuffle the discard into the deck if the deck is empty, draw,
/// and take 1 horror on any would-draw-from-empty. Extracted verbatim
/// from the `Draw` action body (`dispatch.rs:2905–2926`) so the action
/// and Upkeep 4.4 share one code path.
///
/// The deck-out reading (horror on would-draw-from-empty; no reshuffle
/// of a zero-card discard per Rules Reference p.9) is **inherited
/// unchanged** from the already-reviewed `draw` handler — #70 does not
/// re-litigate it.
///
/// Caller guarantees `investigator` exists in `state.investigators`.
fn draw_one_with_deckout(
    state: &mut GameState,
    events: &mut Vec<Event>,
    investigator: InvestigatorId,
) {
    let inv = state.investigators.get(&investigator).expect("caller guarantees existence");
    let deck_empty = inv.deck.is_empty();
    let discard_empty = inv.discard.is_empty();
    if deck_empty {
        if !discard_empty {
            reshuffle_discard_into_deck(state, events, investigator);
        }
        draw_cards(state, events, investigator, 1);
        take_horror(state, events, investigator, 1);
    } else {
        draw_cards(state, events, investigator, 1);
    }
}
```

The `Draw` action (`dispatch.rs:2861`) shrinks to: validate-first → `spend_one_action` → `draw_one_with_deckout`. No behavior change; its existing tests stay green and become the parity check for the extraction.

```rust
/// Grant `amount` resources to `investigator`: saturating-add to the
/// wallet and emit `Event::ResourcesGained`. The resource-grant core
/// shared by the DSL `gain_resources` (called after target resolution)
/// and Upkeep 4.4. No-op (no event) when `amount == 0`, matching the
/// existing `gain_resources` zero-amount behavior.
///
/// Caller guarantees `investigator` exists in `state.investigators`.
fn grant_resources(
    state: &mut GameState,
    events: &mut Vec<Event>,
    investigator: InvestigatorId,
    amount: u8,
) { /* saturating_add + ResourcesGained emit */ }
```

The DSL `gain_resources` (`evaluator.rs:289`) keeps its target-resolution + existence-check prefix and calls `grant_resources` for the mutation+emit. Both live in `game-core::engine`, so sharing crosses no crate boundary.

## Engine — `run_window_continuation` change

File: `crates/game-core/src/engine/dispatch.rs`.

```rust
fn run_window_continuation(state: &mut GameState, events: &mut Vec<Event>, kind: WindowKind) {
    match kind {
        WindowKind::MythosAfterDraws => { /* existing skill-test guard + mythos_phase_end */ }
        WindowKind::UpkeepBegins => {
            // Phase-transitioning continuation (runs 4.2–4.6 then
            // Upkeep→Mythos): cannot run while a skill test is in
            // flight — the transition would strand the test in the
            // wrong phase. Phase 4 has no Upkeep-phase skill-test
            // source, so this is structurally unreachable today.
            // Same guard as MythosAfterDraws.
            if let Some(in_flight) = state.in_flight_skill_test.as_ref() {
                unreachable!(
                    "UpkeepBegins window closed while a skill test is in flight \
                     (continuation={:?}). Phase 4 has no Upkeep-phase skill-test \
                     sources; a future PR adding one needs the window-close + \
                     phase-transition ordering redesigned before this fires.",
                    in_flight.continuation,
                );
            }
            upkeep_resume(state, events);
        }
        WindowKind::AfterEnemyDefeated { .. } | WindowKind::BetweenPhases { .. } => {}
    }
}
```

`close_reaction_window_at` already calls `run_window_continuation` generically after emitting `WindowClosed` — no change needed there.

## Engine — `step_phase` change

File: `crates/game-core/src/engine/dispatch.rs`.

```rust
fn step_phase(state: &mut GameState, events: &mut Vec<Event>) {
    let from = state.phase;
    let to = from.next();

    // PhaseEnded: suppressed when the from-phase's *_end helper owns
    // the emit. Phase 4: Mythos (mythos_phase_end) and now Upkeep
    // (upkeep_phase_end).
    if from != Phase::Mythos && from != Phase::Upkeep {
        events.push(Event::PhaseEnded { phase: from });
    }

    state.phase = to;
    // The round-counter bump previously here moves into `mythos_phase`
    // (step 1.1) — see "round-counter relocation" below. `step_phase`
    // no longer touches `state.round`.

    match to {
        Phase::Mythos if from != Phase::Mythos => mythos_phase(state, events),
        Phase::Investigation if from != Phase::Investigation => investigation_phase(state, events),
        Phase::Upkeep if from != Phase::Upkeep => upkeep_phase(state, events),
        _ => events.push(Event::PhaseStarted { phase: to }), // Enemy (until #71)
    }
}
```

After #70, Enemy is the only phase still using the `_` fallback for both its boundary emits — #71 lands its driver and shifts them the same way.

## Engine — round-counter relocation

File: `crates/game-core/src/engine/dispatch.rs`, `mythos_phase` (shipped in #69; #70 edits its step-1.1 block).

The round-counter increment moves from `step_phase`'s Mythos-entry bump into `mythos_phase`'s step 1.1, so the rule's "round begins" point owns it explicitly — the same "phase-driver owns its steps" reasoning that put `PhaseStarted(Mythos)` in the driver rather than `step_phase`.

```rust
fn mythos_phase(state: &mut GameState, events: &mut Vec<Event>) {
    // 1.1 Round begins. Mythos phase begins.
    //     Rules Reference p.24: "As this is the first framework event
    //     of the round, it [step 1.1] also formalizes the beginning of
    //     a new game round." The round-counter increment lives HERE —
    //     not in step_phase's generic Mythos-entry transition — so the
    //     rule's round-begin point has explicit driver ownership,
    //     mirroring PhaseStarted(Mythos)'s ownership. This is also the
    //     natural future home for a `RoundStarted` event emit (paired
    //     with start_scenario for round 1) when a consumer needs it.
    state.round = state.round.saturating_add(1);
    events.push(Event::PhaseStarted { phase: Phase::Mythos });

    // 1.2 / 1.3 / 1.4 unchanged from #69.
    // ...
}
```

`mythos_phase` is the **sole** entry into `Phase::Mythos` (`step_phase`'s `Phase::Mythos` arm; `start_scenario` skips Mythos in round 1 and sets `round = 1` directly), so relocating the bump is safe — every Mythos entry runs `mythos_phase`. No read site observes a different round value: with the bump at the top of `mythos_phase`, `state.round` is updated before the driver's body (doom/draws) runs and before control returns to `end_turn`, exactly as when it lived in `step_phase`. The existing `mythos_phase` 1.1 comment claiming "step_phase has already … bumped the round counter" is updated to reflect the new ownership.

## Engine — `end_turn` change

File: `crates/game-core/src/engine/dispatch.rs`, `end_turn` (the no-next-investigator branch).

Currently the branch makes three explicit `step_phase` calls (Inv→Enemy, Enemy→Upkeep, Upkeep→Mythos) then checks `mythos_draw_pending`. With #70 the Upkeep→Mythos step moves into `upkeep_phase_end`, so the **third explicit call is dropped**:

```rust
state.active_investigator = None;
step_phase(state, events); // Investigation → Enemy (empty until #71)
step_phase(state, events); // Enemy → Upkeep
// upkeep_phase opens the post-4.1 window. With no Fast play eligible
// (the synthetic Phase-4 path) it auto-skips and the continuation
// cascades 4.2–4.6 → step_phase(Upkeep→Mythos) → mythos_phase, which
// seeds mythos_draw_pending. If a Fast play IS eligible, the window
// stays open and the cascade pauses in Upkeep (open_windows non-empty,
// mythos_draw_pending None). The degenerate all-eliminated path
// cascades all the way to Investigation.
//
// Every path returns Done with the wait (if any) signalled on state:
//   - Upkeep Fast window: a non-empty open_windows entry; the player
//     plays Fast cards or sends ResolveInput::Skip (which closes the
//     window → upkeep_resume → cascade to Mythos).
//   - Mythos draws: mythos_draw_pending Some(lead); next apply is the
//     first DrawEncounterCard.
EngineOutcome::Done
```

The old `if state.mythos_draw_pending.is_some() { return Done }` early-return existed only to guard the now-removed third `step_phase`; the branch falls through to `Done` unconditionally. The `if let Some(next_id) = next` mid-round rotate path is unchanged.

## Tests

### Engine unit tests (`engine/dispatch.rs::upkeep_phase_tests`)

- `upkeep_phase_emits_phase_started_and_auto_skips_inline` — no Fast-eligible cards / no reactions: assert the event subsequence `PhaseStarted(Upkeep)` → `WindowOpened(UpkeepBegins)` → `WindowClosed(UpkeepBegins)` → … → `PhaseEnded(Upkeep)` → `PhaseStarted(Mythos)`, and that `UpkeepBegins` never lands persistently on `state.open_windows`.
- `reset_actions_sets_active_to_per_turn_and_emits` — investigators with `actions_remaining == 0` → set to `ACTIONS_PER_TURN`, one `ActionsRemainingChanged` each.
- `reset_actions_skips_eliminated_investigators` — a Killed/Insane/Resigned investigator's actions are untouched and no event fires for them.
- `reset_actions_emits_nothing_for_already_full` — investigator already at `ACTIONS_PER_TURN` → no event.
- `rotate_to_active_does_not_refresh_actions` — build an investigator with `actions_remaining == 1`, `rotate_to_active` to them; assert `active_investigator` set, `actions_remaining` still 1, no `ActionsRemainingChanged` emitted.
- `ready_exhausted_cards_readies_investigator_cards_and_enemies` — an exhausted in-play card + an exhausted enemy → both flip to ready; assert `CardReadied { .. }` and `EnemyReadied { .. }`.
- `ready_exhausted_cards_leaves_ready_cards_untouched` — already-ready card / enemy → no events.
- `upkeep_draw_and_resource_draws_one_and_grants_one_per_active` — two Active investigators (non-empty decks) → each hand +1, each resources +1; an eliminated third is skipped.
- `upkeep_draw_and_resource_two_pass_ordering` — two investigators: assert both `CardsDrawn` events precede both `ResourcesGained` events (the rule's "once those cards have been drawn, [then] … gains").
- `upkeep_draw_and_resource_deckout_reshuffles_and_takes_horror` — empty deck, non-empty discard → `DeckShuffled` + `CardsDrawn` + `HorrorTaken { amount: 1 }`.
- `upkeep_draw_and_resource_both_empty_takes_horror_no_card` — deck and discard both empty → `HorrorTaken { amount: 1 }`, hand unchanged (parity with the `draw` handler's documented reading).
- `start_scenario_seeds_round_one_actions` — after `StartScenario`, every Active investigator has `actions_remaining == ACTIONS_PER_TURN` and an `ActionsRemainingChanged` fired for each; `state.round == 1`.
- `step_phase_enemy_to_upkeep_invokes_upkeep_driver` — `PhaseStarted(Upkeep)` comes from the driver; `step_phase` does not emit a bare `PhaseEnded(Upkeep)` on the subsequent Upkeep→Mythos step (it's owned by `upkeep_phase_end`).
- `end_turn_cascades_through_upkeep_to_mythos_draw_pending` — single investigator with an exhausted card + non-empty deck: after the last `EndTurn`, the card is readied, hand +1, resources +1, `state.phase == Mythos`, `state.round` incremented (bump now owned by `mythos_phase` step 1.1), `mythos_draw_pending == Some(inv)`, `active_investigator == None`, outcome `Done`.
- `round_increments_on_mythos_entry_via_driver` — starting from an Upkeep-phase state at round N, driving the Upkeep→Mythos transition lands `state.round == N + 1`. Confirms the relocated bump fires on every Mythos entry (any pre-existing `step_phase` round-increment assertions move here / stay green since the observable behavior is unchanged).
- *No test for `check_hand_size` itself* — its body is an intentional `#111` TODO; the future hand-size PR tests it alongside the real body. (Same rationale as #69's `peril_check`.)

### Integration tests (`crates/scenarios/tests/upkeep_phase.rs`)

New file (separate cargo binary → installs the test registry without colliding).

- `upkeep_full_round_readies_draws_and_grants` — synthetic 1-investigator scenario. Seed an exhausted enemy (spawned during a prior Mythos, or placed directly) and a stocked player deck. Drive `StartScenario` → an Investigation action or two → `EndTurn`. Assert: the enemy readied (`EnemyReadied` + `enemy.exhausted == false`), the investigator drew 1 (`CardsDrawn`, hand grew), gained 1 resource (`ResourcesGained`), and the cascade landed at Mythos with `mythos_draw_pending` set (round 2 about to draw).
- `upkeep_deckout_takes_horror` — synthetic scenario where the investigator's deck is empty and discard non-empty at upkeep → `DeckShuffled` + `CardsDrawn` + `HorrorTaken { amount: 1 }`.
- `upkeep_replay_is_deterministic` — drive a full round through Upkeep, snapshot state, replay the action log from initial state, assert bit-for-bit identical state (Phase-4 "done" criteria; the full setup→resolution demo is the slot-11 PR).

## Edge cases handled by design

- **Auto-skip vs. pause at the post-4.1 window.** No Fast content → `open_fast_window` auto-skips, `upkeep_resume` runs inline, the phase completes synchronously (the Phase-4 synthetic path). Fast content eligible → the window stays open and the cascade pauses in Upkeep; the player plays Fast cards or sends `ResolveInput::Skip`. `UpkeepBegins` has empty `pending_triggers`, so `top_reaction_window()` (which skips empty-pending windows) returns `None` and the `apply_player_action` guard doesn't block Fast plays — identical handling to `MythosAfterDraws`.
- **Eliminated investigators.** Skipped for 4.2 (no actions to reset), 4.4 (no draw / resource). Their in-play cards, if any remain, are still subject to 4.3 readying via the deterministic in-play scan — but eliminated investigators' cards normally leave play, and the synthetic fixture keeps everyone Active, so this is theoretical for Phase 4.
- **Deck-out during 4.4.** Reuses the verified `draw_one_with_deckout` path: reshuffle if discard non-empty, draw, horror on would-draw-from-empty. Both-empty → horror, no card (the `draw` handler's documented reading, inherited).
- **Degenerate empty turn order.** `active_investigators_in_turn_order` is empty → 4.2/4.4 are no-ops, 4.3 readies any exhausted enemies (enemies aren't turn-order-bound), `upkeep_phase_end` still emits `PhaseEnded(Upkeep)` and steps to Mythos. `mythos_phase` then handles its own empty-turn-order degenerate path (#69).
- **Round-bump relocated to `mythos_phase` step 1.1.** `upkeep_phase_end`'s `step_phase(Upkeep→Mythos)` enters `mythos_phase`, which bumps `state.round` as its first action (step 1.1, "round begins"). Pure refactor from #69's `step_phase`-resident bump; round 1 remains special-cased in `start_scenario`. #70 adds no round-counter *logic* beyond moving the increment's home.

## Open questions (settled enough to start)

- **Naming of the continuation helper.** `upkeep_resume` was chosen to read as "resume after the post-4.1 window." Alternatives: `upkeep_after_window`, or folding 4.2–4.5 directly into `upkeep_phase_end` (rejected — keeps `upkeep_phase_end` as the exact `mythos_phase_end` analog: PhaseEnded + transition only). Final name can settle in review.
- **`reset_actions` emit-on-change vs. unconditional.** This spec emits `ActionsRemainingChanged` only when the value actually changes (avoids spurious `3→3` noise). `rotate_to_active` previously emitted unconditionally. Either is defensible; emit-on-change is the recommendation. Settle in review.
- **`UpkeepBegins` as a distinct `WindowKind` vs. a generic marker.** A distinct payload-less variant matches the current per-timing-point pattern. The eventual collapse into a generic `PlayerWindow { phase, step }` is tracked in **#140** (don't collapse until ≥3 phase-content PRs land — #70 is one of the data points).

## Follow-up issues

- **#111** (enforce maximum hand size at end-of-turn upkeep) is the consumer for 4.5's `check_hand_size` stub — already filed, unmilestoned.
- **#141** (Resource basic action) reuses the `grant_resources` helper this PR introduces — already filed, Phase 7.
- No new follow-ups required by #70.

## Phase-doc entries

Once the PR merges (and **only** then), in `docs/phases/phase-4-scenario-plumbing.md`:

- Move `#70` from the Open/Issues table to the Closed table; bump closed/open counts in Status.
- Flip Ordering / Arc row #7 (`#70`) to `✅ PR #N`.
- Correct the stale `#70` row note that says it "folds in `GameState.round`" — the round counter already exists (pre-#69); #70 adds no round-counter logic.
- Add Decisions-made entries (PR-numbered) for:
  - **Upkeep driver mirrors the Mythos driver, inverted around the window position.** `upkeep_phase` (4.1 + open post-4.1 window), `upkeep_resume` (4.2–4.5 continuation), `upkeep_phase_end` (4.6 + Upkeep→Mythos, the `mythos_phase_end` analog). Future phase drivers with a *leading* player window follow this shape; drivers with a *trailing* window follow Mythos's.
  - **Action refresh relocated from `rotate_to_active` to Upkeep 4.2 (`reset_actions`).** `rotate_to_active` is now set-active-only; `start_scenario` seeds round-1 actions. `ActionsRemainingChanged` now fires once per round at Upkeep (+ round-1 seed), not at each turn-start — load-bearing for any future turn-start consumer (which would need a new `TurnStarted` event, not this signal).
  - **Round-counter increment relocated from `step_phase` to `mythos_phase` step 1.1.** The rules' "round begins" is step 1.1 (Rules Reference p.24), so the driver owns the bump rather than `step_phase`'s generic Mythos-entry transition — same reasoning that put `PhaseStarted(Mythos)` in the driver. Round 1 stays special-cased in `start_scenario`. This is also the future home for a `RoundStarted` event when a consumer lands (none yet). Pure refactor; no read site sees a different round value.
  - **"Ready each exhausted card" (4.3) includes enemies.** New `Event::CardReadied` for in-play assets; existing `Event::EnemyReadied` for enemies. Divergence from the issue's "cards they control" paraphrase, following Rules Reference p.25 verbatim.
  - **4.4 is two-pass (all draws, then all resource-gains) in player order**, per "once those cards have been drawn, each investigator gains 1 resource."
  - **Shared helpers `draw_one_with_deckout` and `grant_resources`** de-duplicate the deck-out draw (with the `Draw` action) and the resource grant (with the DSL `gain_resources` / future #141 Resource action).
- No phase-doc Open questions are settled by #70 (the remaining ones cover #128 hunter movement and #131 resolution idempotency). Leave them.
