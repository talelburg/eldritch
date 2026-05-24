# #69 — Mythos phase content (design)

GitHub issue: [#69](https://github.com/talelburg/eldritch/issues/69) · Phase: [phase-4-scenario-plumbing](../../phases/phase-4-scenario-plumbing.md) · Depends on #72 (encounter deck state), #126 (Revelation DSL + on-draw resolution path), #127 (enemy spawn rules), #103 (unified window stack) — all shipped.

## Context

The Mythos phase is the second-to-last piece of Phase 4's phase-content set. Today `step_phase` ticks through Mythos in zero events. #69 wires the real per-investigator encounter draw loop, the per-card 5-step resolution sequence (peril check, Revelation, enemy spawn, surge re-draw) from Rules Reference p.24, and the post-1.4 player window.

The earlier Phase-4 PRs (#72/#126/#127) landed each primitive in isolation with the synthetic fixture proving the wiring; #69 composes them into the actual rules-faithful Mythos driver.

## Rules Reference, verbatim (p.24)

> **I. Mythos phase** (skip during first round of game)
> 1.1 Round begins. Mythos phase begins.
> 1.2 Place 1 doom on the current agenda.
> 1.3 Check doom threshold.
> 1.4 Each investigator draws 1 encounter card.
> [PLAYER WINDOW]
> 1.5 Mythos phase ends.

And the per-card sub-sequence inside 1.4:

> 1. Draw the card from the encounter deck.
> 2. Check for the peril keyword on the drawn card. (If the card has the peril keyword, the investigator who drew the card cannot confer with the other players. Those other players cannot play cards, trigger abilities, or commit cards to that investigator's skill test(s) while the peril encounter is resolving.)
> 3. Resolve the revelation ability on the drawn card.
> 4. If the card is an enemy, spawn it following any spawn instruction the card bears.
> 5. If the drawn card has the surge keyword, the investigator must draw another card from the encounter deck.

Plus:

- p.18 (Treachery): *"When a treachery card is drawn by an investigator, that investigator must resolve its effects. Then, place the card in its discard pile unless otherwise instructed by the ability."*
- p.18 (Revelation): *"…in the case of a treachery card, before it is placed in the discard pile."*
- p.19 (Surge): *"After drawing and resolving an encounter with the surge keyword, an investigator must draw another card from the encounter deck."*
- p.10 (Encounter Deck): *"If the encounter deck is empty, shuffle the encounter discard pile back into the encounter deck."*
- p.9: *"Any ability that would shuffle a discard pile of zero cards back into a deck does not shuffle the deck."*

#69 owns sub-steps 1.1, 1.4 (per-investigator loop + per-card 5-step shape including surge), the post-1.4 player window, and 1.5. Sub-steps 1.2 / 1.3 are #73's domain and land as named function call-sites with TODO bodies. Per-card step 2 (peril enforcement) lands as a named call-site with TODO body — no machinery for "confer" / cross-investigator commit blocking exists yet.

## Scope

- New player action `PlayerAction::DrawEncounterCard` + dispatch handler.
- New `GameState.mythos_draw_pending: Option<InvestigatorId>` cursor.
- New `WindowKind::MythosAfterDraws` variant for the post-1.4 player window.
- New `CardMetadata` fields `surge: bool` and `peril: bool` (default `false`; pipeline emits `false` for everything in this PR).
- New `Event::WindowOpened { kind: WindowKind }` for observability symmetry with `Event::WindowClosed`.
- Mythos driver `mythos_phase(state, events)` invoked from `step_phase` on the Upkeep→Mythos transition; runs 1.1 / 1.2 / 1.3 inline (1.2 / 1.3 are TODO stubs for #73) and seeds `mythos_draw_pending`.
- Mythos closing helper `mythos_phase_end(state, events)` invoked from the post-1.4 window's close path; emits `PhaseEnded(Mythos)` as step 1.5 and steps to Investigation (whose driver handles rotation).
- Investigation skeleton driver `investigation_phase(state, events)` invoked by `step_phase` on any-to-Investigation transition; emits `PhaseStarted(Investigation)` as step 2.1 and rotates to lead as step 2.2. Future Investigation-content PRs flesh out the body (e.g. opening the post-2.1 player window before rotation).
- Per-card resolution helper `mythos_draw_for(state, events, investigator)` walking the 5-step sequence with surge re-draw loop.
- Refactor: extract `check_play_card` and `check_activate_ability` as pure-validation peer helpers from `play_card` and `activate_ability` (no behavior change at call sites).
- New helper `any_fast_play_eligible(state) -> bool` using the extracted validators to short-circuit the post-1.4 window when nobody can play anything Fast.
- New helper `open_fast_window(state, events, kind)` that always emits `Event::WindowOpened` and either (a) pushes the window onto `state.open_windows` if pending triggers or any Fast play is eligible — the apply loop's existing "pending reactions → AwaitingInput" path then surfaces the wait, or (b) immediately emits `Event::WindowClosed` and runs the kind's continuation inline.
- Synthetic surge-bearing treachery in `test_fixtures/synth_cards.rs`.
- Integration tests in `crates/scenarios/tests/mythos_phase.rs`.

## Out of scope

- Per-card stat fields beyond what `spawn_enemy` already hardcodes (already deferred by #127).
- Doom +1 and threshold check (#73).
- Peril conferral-restriction enforcement (future PR; no machinery exists yet for confer / cross-investigator commit blocking).
- Pipeline parsing of surge/peril keywords from upstream JSON (deferred until the first real surge/peril card in Phase 7+ lands; emits `false` for everything until then).
- Generalizing `WindowKind` to a single `PlayerWindow` variant (separate follow-up issue — see Open Questions).

## Card-data additions

File: `crates/card-dsl/src/card_data.rs`.

```rust
struct CardMetadata {
    // ... existing fields ...
    /// Surge keyword (Rules Reference p.19). When `true`, after the
    /// card is drawn and resolved during a Mythos encounter draw, the
    /// drawing investigator immediately draws another encounter card.
    pub surge: bool,
    /// Peril keyword (Rules Reference p.18, referenced in p.24 1.4
    /// step 2). When `true`, the drawing investigator cannot confer
    /// and other players cannot play cards / trigger abilities /
    /// commit to that investigator's skill tests during resolution.
    /// Enforcement is not yet wired — no machinery exists for
    /// cross-investigator commit blocking. The field exists so cards
    /// can carry the keyword and the step-2 call site can become
    /// load-bearing when the enforcement PR lands.
    pub peril: bool,
}
```

Both default to `false`. The pipeline (`crates/card-data-pipeline/`) emits `false` for every generated card in this PR. Structured parsing of `surge` / `peril` from upstream JSON is deferred until the first Phase-7+ card needs it. Regenerate `crates/cards/src/generated/cards.rs` so every existing entry gets `surge: false, peril: false`.

## DSL additions

None. Surge and peril are metadata-level keywords, not DSL primitives; the engine reads the booleans directly. No `Trigger::Surge` or `EventPattern::Peril` until concrete consumers force the shape.

## Engine — new state

File: `crates/game-core/src/state/game_state.rs`.

```rust
struct GameState {
    // ... existing fields ...
    /// The investigator whose Mythos-phase encounter draw is pending,
    /// during Rules-Reference p.24 step 1.4. `Some(id)` between
    /// `mythos_phase` entry and the last drawer's completion; `None`
    /// otherwise. Advanced after each `PlayerAction::DrawEncounterCard`
    /// completes its chain (including any surge re-draws). `None`
    /// once all investigators have drawn — at which point the
    /// `MythosAfterDraws` window opens.
    pub mythos_draw_pending: Option<InvestigatorId>,
}
```

## Engine — new events

File: `crates/game-core/src/event.rs`.

```rust
enum Event {
    // ... existing ...
    /// A reaction / Fast / phase-boundary player window opened.
    /// Symmetric with [`Event::WindowClosed`]. Emitted by every
    /// path that pushes onto `state.open_windows`, including the
    /// `open_fast_window` helper. Order with `WindowClosed`:
    /// `WindowOpened { kind: K }` always precedes the matching
    /// `WindowClosed { kind: K }` for the same window instance.
    WindowOpened {
        kind: WindowKind,
    },
}
```

Note: this lands `Event::WindowOpened` for the first time. Existing call sites that push to `state.open_windows` (the `queue_reaction_window` helper) gain a `WindowOpened` emit too — small but observable change to the AfterEnemyDefeated event stream. Existing tests that assert on full event sequences in those areas will need a small update; tests using `assert_event!` and friends (order-insensitive) are unaffected.

## Engine — new window kind

File: `crates/game-core/src/state/game_state.rs` (the `WindowKind` enum, currently in this file).

```rust
#[non_exhaustive]
enum WindowKind {
    AfterEnemyDefeated { enemy: EnemyId, by: Option<InvestigatorId> },
    BetweenPhases { from: Phase, to: Phase },
    /// The player window between Rules Reference p.24 step 1.4 (each
    /// investigator draws an encounter card) and step 1.5 (Mythos
    /// phase ends). Carries no payload — there is no
    /// `EventPattern` today that matches against this specifically;
    /// the variant exists so the rule's printed timing point is
    /// addressable when a future card binds to it.
    MythosAfterDraws,
}
```

Adding the variant is non-breaking (`WindowKind` is `#[non_exhaustive]`).

## Engine — Mythos driver

File: `crates/game-core/src/engine/dispatch.rs`.

```rust
/// Entered by `step_phase` on the Upkeep→Mythos transition (and not
/// on the `start_scenario` Mythos→Investigation skip, per Rules
/// Reference p.24's "skip during first round of game" exception).
///
/// Lays out the printed sub-steps as discrete named call sites so the
/// rule structure is grep-able and #73 / future-peril-PR fills in
/// the TODO bodies without changing the driver shape.
fn mythos_phase(state: &mut GameState, events: &mut Vec<Event>) {
    // 1.1 Round begins. Mythos phase begins.
    //     `step_phase` has already emitted `Event::PhaseEnded(Upkeep)`,
    //     updated `state.phase` to Mythos, and bumped the round
    //     counter. The `PhaseStarted(Mythos)` emit lives HERE rather
    //     than in `step_phase` so step 1.1 has explicit ownership in
    //     the driver — Rules Reference p.24: "This step formalizes
    //     the beginning of the mythos phase."
    events.push(Event::PhaseStarted { phase: Phase::Mythos });

    // 1.2 Place 1 doom on the current agenda.
    place_doom_on_agenda(state, events);

    // 1.3 Check doom threshold.
    check_doom_threshold(state, events);

    // 1.4 Each investigator draws 1 encounter card.
    //     Seed the cursor; the actual draws are player-driven via
    //     `PlayerAction::DrawEncounterCard`. apply returns Done here;
    //     the next apply is the first drawer's action.
    state.mythos_draw_pending = state.turn_order.first().copied();
    if state.mythos_draw_pending.is_none() {
        // No investigators — degenerate state, but the chain
        // naturally proceeds: no draws happen, post-1.4 window
        // opens immediately on a subsequent apply that detects the
        // empty-turn-order condition. For #69 we keep the simpler
        // shape: if there's no one to draw, close the phase right
        // now.
        open_fast_window(state, events, WindowKind::MythosAfterDraws);
    }
}

fn place_doom_on_agenda(_state: &mut GameState, _events: &mut Vec<Event>) {
    // TODO(#73): place 1 doom on the current agenda per Rules
    //            Reference p.24 step 1.2. Currently no agenda state
    //            exists; #73 lands the agenda struct + doom counter
    //            + this body.
}

fn check_doom_threshold(_state: &mut GameState, _events: &mut Vec<Event>) {
    // TODO(#73): compare total doom in play to current agenda's
    //            threshold; advance if met. Rules Reference p.24
    //            step 1.3. Same reason as above: no agenda state
    //            yet.
}

/// Called after the post-1.4 window closes. Emits 1.5's
/// `PhaseEnded(Mythos)` marker, then transitions the engine to
/// Investigation phase. Rotation is owned by `investigation_phase`
/// (step 2.2), not by `mythos_phase_end`. Invoked from
/// `close_reaction_window_at`'s kind-aware tail when a
/// `MythosAfterDraws` window pops.
fn mythos_phase_end(state: &mut GameState, events: &mut Vec<Event>) {
    // 1.5 Mythos phase ends.
    //     The `PhaseEnded(Mythos)` emit lives HERE rather than in
    //     `step_phase` so step 1.5 has explicit ownership in the
    //     driver — mirror of step 1.1's PhaseStarted ownership in
    //     `mythos_phase`. Rules Reference p.24: "This step
    //     formalizes the end of the mythos phase."
    events.push(Event::PhaseEnded { phase: Phase::Mythos });
    step_phase(state, events); // Mythos → Investigation; calls investigation_phase
}
```

## Engine — Investigation phase skeleton driver

File: `crates/game-core/src/engine/dispatch.rs`. Establishes the phase-driver pattern for Investigation; #69 only fills in steps 2.1 and 2.2 (rotation). Future Investigation-content PRs add the post-2.1 player window, the 2.2 player-pick action (replacing lead-first default), the 2.2.1 action-taking sub-loop, etc.

```rust
/// Entered by `step_phase` on any-to-Investigation transition.
/// Owns the `PhaseStarted(Investigation)` emit (step 2.1) and the
/// initial rotation to the active investigator (step 2.2).
///
/// **Rotation policy (Phase 4):** lead-first by default. Rules
/// Reference p.24 step 2.2: "The investigators may take their turns
/// in any order. The investigators choose among themselves who…will
/// take this turn." Phase 4 hardcodes lead-first as the table
/// convention; a future PR adds a `PlayerAction::ChooseFirstActor`
/// (or similar) to land the explicit pick within an opened post-2.1
/// player window.
fn investigation_phase(state: &mut GameState, events: &mut Vec<Event>) {
    // 2.1 Investigation phase begins.
    events.push(Event::PhaseStarted { phase: Phase::Investigation });

    // [Player window between 2.1 and 2.2 — not opened in #69 because
    //  the post-2.1 window has no in-scope listener and adding it
    //  here would pull #70/#71 plumbing forward. Future PR adds the
    //  `open_fast_window(state, events, WindowKind::AfterInvestigationStart)`
    //  call site at this exact line. The window's continuation
    //  would be a second helper that does step 2.2.]

    // 2.2 Next investigator's turn begins. (First turn of the phase.)
    if let Some(&first) = state.turn_order.first() {
        rotate_to_active(state, events, first);
    }
}
```

`mythos_phase_end`'s `step_phase(Mythos→Investigation)` call now invokes `investigation_phase` (via `step_phase`'s phase-driver dispatch — see "Engine — step_phase changes" below). The rotation that previously lived in `mythos_phase_end` lives here.

**Callers that previously did their own rotate-to-lead after a phase transition simplify:**

- `start_scenario` (dispatch.rs:680-683): the trailing block that calls `step_phase(state, events)` (Mythos→Investigation, the first-round skip) followed by `rotate_to_active(state, events, first)` simplifies — the `step_phase` call now invokes `investigation_phase`, which handles the rotate. The explicit `rotate_to_active` call after `step_phase` is dropped.
- `end_turn` (dispatch.rs:738-742): same simplification at the chain tail. The explicit `rotate_to_active` after the final `step_phase(Mythos→Investigation)` is dropped (now handled by `investigation_phase`). The mid-Investigation rotate path (when a next investigator exists at line 732) is unchanged — that's the per-EndTurn step-2.2 instance, not phase-entry.

`rotate_to_active` itself stays as a low-level helper; only its callers shrink.

## Engine — per-card resolution

File: `crates/game-core/src/engine/dispatch.rs`.

```rust
/// Hard cap on a single Mythos draw chain. Real scenarios surge ≤2
/// in a chain; the cap exists purely to guarantee termination on
/// malformed encounter decks (e.g. a deck small enough for surge to
/// loop via the p.10 reshuffle). `unreachable!`-class — never
/// reached in legitimate play.
const MAX_SURGE_CHAIN: usize = 64;

/// Resolves one investigator's full Mythos encounter draw — the
/// per-card 5-step sub-sequence from Rules Reference p.24, with
/// surge re-draws looping until the chain ends.
///
/// Called from `PlayerAction::DrawEncounterCard`'s handler with the
/// pending-drawer's id. Returns Done on success (chain completed,
/// `mythos_draw_pending` advanced).
fn mythos_draw_for(
    state: &mut GameState,
    events: &mut Vec<Event>,
    investigator: InvestigatorId,
) -> EngineOutcome {
    let Some(reg) = card_registry::current() else {
        return EngineOutcome::Rejected {
            reason: "DrawEncounterCard: no card registry installed".into(),
        };
    };

    let mut chain_count: usize = 0;
    loop {
        chain_count += 1;
        if chain_count > MAX_SURGE_CHAIN {
            unreachable!(
                "Mythos draw chain exceeded MAX_SURGE_CHAIN ({MAX_SURGE_CHAIN}) for \
                 investigator {investigator:?}. Indicates either an infinite reshuffle \
                 loop (Rules Reference p.18: treachery discard precedes surge re-draw, \
                 so a surging treachery in a too-small deck cycles via the p.10 \
                 reshuffle path) or a malformed scenario encounter deck. Real \
                 scenarios don't surge >{MAX_SURGE_CHAIN} cards in one chain.",
            );
        }

        // Step 1: Draw the card from the encounter deck.
        let Some(code) = draw_encounter_top(state, events) else {
            if chain_count == 1 {
                // Genuine "deck and discard both empty" on the initial
                // draw — treated as a Rejected action, leaving the
                // pending cursor untouched (player can retry once the
                // scenario adds cards, or the scenario marks itself
                // ended). validate-first preserved because nothing
                // has mutated yet on the first iteration.
                return EngineOutcome::Rejected {
                    reason: "DrawEncounterCard: encounter deck and discard both empty".into(),
                };
            }
            // Mid-chain empty-deck-and-discard: only reachable when
            // surging enemies have exhausted the encounter universe
            // within one chain (enemies spawn to play, not discard,
            // so p.10 reshuffle has nothing to pull). Scenario-data
            // malformation, not legitimate play.
            unreachable!(
                "Mythos draw chain hit empty encounter deck AND empty discard for \
                 investigator {investigator:?} at chain position {chain_count}. \
                 Indicates a malformed scenario where surging enemies exhausted the \
                 encounter universe within one chain.",
            );
        };

        let Some(metadata) = (reg.metadata_for)(&code) else {
            return EngineOutcome::Rejected {
                reason: format!("DrawEncounterCard: unknown card code: {code:?}").into(),
            };
        };

        // Step 2: Check for the peril keyword on the drawn card.
        peril_check(state, events, &code, investigator, metadata.peril);

        // Step 3 + 4: Resolve revelation, then enemy-spawn if applicable.
        //   Delegates to the shared helper extracted from
        //   `encounter_card_revealed` (the existing
        //   `EngineRecord::EncounterCardRevealed` path).
        let outcome = resolve_encounter_card(state, events, investigator, code.clone(), metadata);
        if !matches!(outcome, EngineOutcome::Done) {
            return outcome;
        }

        // Step 5: If the drawn card has the surge keyword, loop.
        if !metadata.surge {
            break;
        }
    }

    // Chain complete — advance the cursor.
    advance_mythos_draw_pending(state, events);
    EngineOutcome::Done
}

fn peril_check(
    _state: &mut GameState,
    _events: &mut Vec<Event>,
    _code: &CardCode,
    _investigator: InvestigatorId,
    _is_peril: bool,
) {
    // TODO(future-peril-PR): if `is_peril`, install a temporary
    //   restriction on `state` such that other investigators cannot
    //   (a) play cards, (b) trigger abilities, or (c) commit to the
    //   drawing investigator's skill tests until this card's
    //   resolution completes. Rules Reference p.24 step 1.4.2. No
    //   machinery exists for cross-investigator commit blocking
    //   yet — Phase 4 is single-investigator-focused. The function
    //   call site exists so the rule step is grep-able and the
    //   restriction enforcement plugs in here without changing the
    //   driver shape.
}

/// Shared helper: resolves an already-drawn encounter card (steps
/// 3+4 of the per-card sub-sequence). Called by both `mythos_draw_for`
/// and the existing `EngineRecord::EncounterCardRevealed` path.
///
/// The body is the existing resolution prefix from
/// `encounter_card_revealed` at `dispatch.rs:257` (the block that
/// runs after `draw_encounter_top` returns Some and the metadata
/// lookup succeeds): emits `Event::CardRevealed`, then dispatches on
/// CardType — treachery → run Revelation effects → push card to
/// `encounter_discard` → emit `Event::CardDiscarded { from:
/// Zone::EncounterDeck }`; enemy → call `spawn_enemy`; any other
/// type → return `Rejected`. #69 lifts that block verbatim into this
/// helper and shrinks `encounter_card_revealed` to:
///
/// ```rust
/// fn encounter_card_revealed(state, events, investigator) -> EngineOutcome {
///     // registry check, draw_encounter_top, metadata lookup unchanged
///     resolve_encounter_card(state, events, investigator, code, metadata)
/// }
/// ```
fn resolve_encounter_card(
    state: &mut GameState,
    events: &mut Vec<Event>,
    investigator: InvestigatorId,
    code: CardCode,
    metadata: &CardMetadata,
) -> EngineOutcome { /* lifted body, see doc-comment */ }

/// Advance `state.mythos_draw_pending` after a completed chain.
/// If a next investigator exists in turn order, set to that id.
/// Otherwise set to None and open the post-1.4 window.
fn advance_mythos_draw_pending(state: &mut GameState, events: &mut Vec<Event>) {
    let current = state.mythos_draw_pending.expect("called only after a successful chain");
    let next = state
        .turn_order
        .iter()
        .position(|id| *id == current)
        .and_then(|idx| state.turn_order.get(idx + 1).copied());

    state.mythos_draw_pending = next;
    if next.is_none() {
        open_fast_window(state, events, WindowKind::MythosAfterDraws);
    }
}
```

## Engine — new player action

File: `crates/game-core/src/action.rs`.

```rust
enum PlayerAction {
    // ... existing ...
    /// Resolve one Mythos-phase encounter draw for the acting
    /// investigator. Valid only during `Phase::Mythos` when
    /// `state.mythos_draw_pending == Some(acting_investigator)`.
    /// Resolves the per-card 5-step sub-sequence from Rules
    /// Reference p.24 step 1.4 inline (including surge re-draws);
    /// advances `mythos_draw_pending` to the next-in-turn-order
    /// drawer, or opens the `MythosAfterDraws` window if this was
    /// the last drawer.
    DrawEncounterCard,
}
```

Dispatch handler:

```rust
fn draw_encounter_card(
    state: &mut GameState,
    events: &mut Vec<Event>,
    investigator: InvestigatorId,
) -> EngineOutcome {
    if state.phase != Phase::Mythos {
        return EngineOutcome::Rejected {
            reason: format!(
                "DrawEncounterCard: only valid during Mythos phase, got {:?}",
                state.phase,
            ).into(),
        };
    }
    match state.mythos_draw_pending {
        None => EngineOutcome::Rejected {
            reason: "DrawEncounterCard: no draw pending (all investigators have drawn)".into(),
        },
        Some(expected) if expected != investigator => EngineOutcome::Rejected {
            reason: format!(
                "DrawEncounterCard: out of order; expected {expected:?}, got {investigator:?}",
            ).into(),
        },
        Some(_) => mythos_draw_for(state, events, investigator),
    }
}
```

## Engine — Fast-window helpers

File: `crates/game-core/src/engine/dispatch.rs`. Two pieces:

**1. Extract pure-validation helpers (no behavior change).**

```rust
struct PlayCheckResult {
    destination: PlayDestination,
    abilities: Vec<Ability>,
    is_fast: bool,
    card_type: CardType,
}

/// Pure-validation peer to `play_card`'s mutation path. Returns
/// `Ok(result)` if the named card is currently playable by
/// `investigator`, `Err(reason)` if not. The check is exactly the
/// existing `play_card` validation block, lifted unchanged.
fn check_play_card(
    state: &GameState,
    investigator: InvestigatorId,
    hand_index: u8,
) -> Result<PlayCheckResult, Cow<'static, str>>;

struct ActivateCheckResult {
    /// Index of the card instance in the investigator's `cards_in_play`
    /// (resolved from `instance_id` during validation; cached so the
    /// mutation step doesn't re-search).
    in_play_pos: usize,
    /// The ability being activated (cloned from the registry during
    /// validation), passed forward to the mutation step.
    ability: Ability,
    /// Whether the source card was exhausted at validation time —
    /// load-bearing for activated abilities whose payment includes
    /// `Cost::Exhaust`.
    source_exhausted: bool,
}

/// Pure-validation peer to `activate_ability`. Same shape as
/// `check_play_card`.
fn check_activate_ability(
    state: &GameState,
    investigator: InvestigatorId,
    instance_id: CardInstanceId,
    ability_index: u8,
) -> Result<ActivateCheckResult, Cow<'static, str>>;
```

`play_card` and `activate_ability` become thin wrappers: call the check, then run the existing mutation block on the `Ok` payload. No behavior change at either call site.

**2. New Fast-eligibility scan + Fast-window opener.**

```rust
/// Returns `true` if any investigator has at least one playable Fast
/// option in the current state — either a Fast card in hand or a
/// non-exhausted 0-action Activated ability on a card in play.
/// Used by `open_fast_window` to short-circuit windows where nobody
/// can act. Uses the extracted `check_play_card` /
/// `check_activate_ability` helpers so the eligibility check is
/// exactly the existing play/activate gate — no parallel
/// implementation, no drift.
fn any_fast_play_eligible(state: &GameState) -> bool {
    let Some(reg) = card_registry::current() else { return false; };
    for (&inv_id, inv) in &state.investigators {
        // Fast cards in hand.
        for hand_idx in 0..u8::try_from(inv.hand.len()).unwrap_or(u8::MAX) {
            if let Ok(result) = check_play_card(state, inv_id, hand_idx) {
                if result.is_fast {
                    return true;
                }
            }
        }
        // Fast activated abilities (action_cost == 0) on cards in play.
        for card in &inv.cards_in_play {
            let Some(abilities) = (reg.abilities_for)(&card.code) else { continue; };
            for (ab_idx, ability) in abilities.iter().enumerate() {
                let Trigger::Activated { action_cost: 0, .. } = ability.trigger else { continue; };
                let ab_idx_u8 = u8::try_from(ab_idx).expect("abilities vec exceeds u8::MAX");
                if check_activate_ability(state, inv_id, card.instance_id, ab_idx_u8).is_ok() {
                    return true;
                }
            }
        }
    }
    false
}

/// Open a Fast-play window of the given kind. Always emits
/// `Event::WindowOpened { kind }` for observability. Then:
/// - Scans for pending reaction triggers (`scan_pending_triggers`).
/// - Scans for Fast play eligibility (`any_fast_play_eligible`).
/// - If neither has anything: immediately emits
///   `Event::WindowClosed { kind }` and runs the kind's
///   continuation inline. The window never lands on
///   `state.open_windows`.
/// - Otherwise: pushes the `OpenWindow` onto `state.open_windows`
///   with the scanned pending triggers and `fast_actors: Any`. The
///   apply returns `Done` here; the caller's dispatch path is
///   expected to translate this into an `AwaitingInput` at its
///   tail when appropriate.
fn open_fast_window(state: &mut GameState, events: &mut Vec<Event>, kind: WindowKind) {
    events.push(Event::WindowOpened { kind });

    let pending_triggers = scan_pending_triggers(state, kind);
    let has_fast_eligible = any_fast_play_eligible(state);

    if pending_triggers.is_empty() && !has_fast_eligible {
        events.push(Event::WindowClosed { kind });
        run_window_continuation(state, events, kind);
        return;
    }

    state.open_windows.push(OpenWindow {
        kind,
        pending_triggers,
        fast_actors: FastActorScope::Any,
    });
}

/// Kind-aware continuation called when a window closes (whether
/// inline via `open_fast_window`'s auto-skip path or via the
/// `close_reaction_window_at` pop path). For
/// `WindowKind::MythosAfterDraws`, runs `mythos_phase_end`.
/// Other window kinds: no continuation (preserves existing
/// `close_reaction_window_at` behavior).
fn run_window_continuation(state: &mut GameState, events: &mut Vec<Event>, kind: WindowKind) {
    match kind {
        WindowKind::MythosAfterDraws => mythos_phase_end(state, events),
        WindowKind::AfterEnemyDefeated { .. } | WindowKind::BetweenPhases { .. } => {}
    }
}
```

`close_reaction_window_at` gains a tail call to `run_window_continuation` after `Event::WindowClosed` emits — see "Close-path changes" below.

## Engine — close-path changes

File: `crates/game-core/src/engine/dispatch.rs`, `close_reaction_window_at`.

After the existing `events.push(Event::WindowClosed { kind })` and before the in-flight skill-test resume path:

```rust
run_window_continuation(state, events, kind);
```

`MythosAfterDraws` is the only kind that uses this today. The skill-test resume logic at the function tail is unaffected; the continuation runs first, then if a skill test is still mid-resolution and not `AwaitingCommit`, the existing `drive_skill_test` path takes over.

## Engine — step_phase changes

File: `crates/game-core/src/engine/dispatch.rs`, `step_phase`.

`step_phase` becomes the central dispatcher between phase boundaries. It owns:
- The `state.phase` update
- The round-counter bump on Mythos entry (existing invariant)
- Invoking the destination phase's driver function if one exists, otherwise emitting `PhaseStarted` as a fallback for phases without drivers yet (Enemy, Upkeep in current scope)

`step_phase` no longer emits boundary events for phases whose drivers own them:
- **`PhaseEnded(Mythos)`** is emitted by `mythos_phase_end` as step 1.5. `step_phase` suppresses it when `from == Mythos`.
- **`PhaseStarted(Mythos)`** is emitted by `mythos_phase` as step 1.1. `step_phase` suppresses it when `to == Mythos && from != Mythos`.
- **`PhaseStarted(Investigation)`** is emitted by `investigation_phase` as step 2.1. `step_phase` suppresses it when `to == Investigation && from != Investigation`.
- `PhaseEnded(Investigation)`, `PhaseEnded(Enemy)`, `PhaseEnded(Upkeep)`, `PhaseStarted(Enemy)`, `PhaseStarted(Upkeep)` — still emitted by `step_phase` directly. Future #70/#71 PRs land the matching enemy_phase / upkeep_phase drivers + their end helpers and shift these emits the same way.

```rust
fn step_phase(state: &mut GameState, events: &mut Vec<Event>) {
    let from = state.phase;
    let to = from.next();

    // PhaseEnded: suppressed when the from-phase's *_end helper owns
    // the emit. Phase 4: only Mythos has an end helper.
    if from != Phase::Mythos {
        events.push(Event::PhaseEnded { phase: from });
    }

    state.phase = to;
    // Round-bump invariant: bump when entering Mythos. Unchanged.
    if to == Phase::Mythos {
        state.round = state.round.saturating_add(1);
        // (existing round-start emit, if any, stays unchanged)
    }

    // Dispatch to phase driver if one exists; otherwise emit
    // PhaseStarted directly.
    match to {
        Phase::Mythos if from != Phase::Mythos => mythos_phase(state, events),
        Phase::Investigation if from != Phase::Investigation => investigation_phase(state, events),
        _ => events.push(Event::PhaseStarted { phase: to }),
    }
}
```

The `from != to` guards on each driver-dispatch arm preserve correctness if `step_phase` is ever called with the same source and destination (defensive — the existing `Phase::next()` always returns a different phase, but the guard documents intent and survives future hand-mutations).

**`start_scenario` drops its explicit `PhaseStarted(Mythos)` emit entirely** (currently at `dispatch.rs:658`). Per Rules Reference p.24 ("During the first round of the game, skip the mythos phase"), the entire Mythos phase — including step 1.1's "Mythos phase begins" formalization — is skipped in round 1. No `PhaseStarted(Mythos)` and no `PhaseEnded(Mythos)` should fire. The `dispatch.rs:652` comment about emitting Mythos start explicitly is removed alongside the emit. `start_scenario` jumps straight from `ScenarioStarted` to Investigation:

```rust
// Round 1: scenario starts directly in Investigation phase — Mythos is
// skipped entirely per Rules Reference p.24 "During the first round of
// the game, skip the mythos phase." No PhaseStarted(Mythos) / PhaseEnded(
// Mythos) fire — the phase doesn't happen.
state.round = 1;
state.phase = Phase::Investigation;
events.push(Event::ScenarioStarted);
// ... shuffle decks, deal opening hands, set mulligan_window = true ...
investigation_phase(state, events); // emits PhaseStarted(Investigation), rotates to lead
```

The `ScenarioStarted` event serves as the round-1 marker; future PRs can add an `Event::RoundStarted { round }` emit at the Mythos round-bump site (paired with a parallel emit here in `start_scenario`) if a card or reaction needs the explicit round-began signal — none in scope yet, so not adding it now.

The explicit `rotate_to_active` call at the original `dispatch.rs:681-683` is dropped (handled by `investigation_phase`).

The auto-chain in `end_turn` (Investigation→Enemy→Upkeep→Mythos→Investigation) now invokes `mythos_phase` when it reaches the Upkeep→Mythos `step_phase` call. That sets `mythos_draw_pending = Some(lead)` and returns; the next `step_phase` (Mythos→Investigation) in the chain would normally fire next, but **with `mythos_draw_pending` set, `end_turn` must stop chaining** at that point — the round's Mythos draws are now pending the player's actions.

```rust
// In end_turn, after Upkeep is reached:
step_phase(state, events); // Upkeep → Mythos (round bumps + mythos_phase runs)
if state.mythos_draw_pending.is_some() {
    // Chain pauses here. `Done` outcome; next apply is the first
    // drawer's `PlayerAction::DrawEncounterCard`. mythos_phase_end
    // (triggered later via close_reaction_window_at) will emit
    // PhaseEnded(Mythos), call step_phase(Mythos→Investigation),
    // which invokes investigation_phase, which rotates.
    return EngineOutcome::Done;
}
// Otherwise (no investigators in turn order — degenerate state):
// the MythosAfterDraws window already opened and closed inline via
// `mythos_phase`, which fired mythos_phase_end as the auto-skip
// continuation; that emitted PhaseEnded(Mythos) and stepped to
// Investigation via investigation_phase. Nothing left to do.
```

The explicit `step_phase` + `rotate_to_active` after the Upkeep→Mythos boundary is dropped — `mythos_phase_end` handles both whether the chain auto-skipped or paused for draws.

## Test fixture additions

File: `crates/scenarios/src/test_fixtures/synth_cards.rs`.

```rust
/// Code for the synthetic surge-bearing treachery. Its Revelation
/// is the same trivial "gain 1 resource" as `_synth_treachery`; the
/// load-bearing difference is `surge: true` on the metadata, which
/// drives the surge re-draw path in the per-card sub-sequence.
pub const SYNTH_SURGE_TREACHERY_CODE: &str = "_synth_surge_treachery";

fn synth_surge_treachery_metadata() -> CardMetadata {
    CardMetadata {
        code: SYNTH_SURGE_TREACHERY_CODE.to_owned(),
        // ... mirror synth_treachery_metadata, then ...
        surge: true,
        peril: false,
    }
}
```

Also: the existing `synth_treachery_metadata` and `synth_enemy_metadata` gain explicit `surge: false, peril: false` to compile with the new fields.

File: `crates/scenarios/src/test_fixtures/synthetic.rs`. Add a helper to seed a specific encounter-deck composition for tests:

```rust
/// Seed the encounter deck of the synthetic scenario state with the
/// given card codes (in draw order, top = index 0). Used by Phase-4
/// integration tests that want to drive Mythos through deterministic
/// card sequences.
pub fn with_encounter_deck(state: &mut GameState, codes: Vec<CardCode>) {
    state.encounter_deck = codes.into();
}
```

## Tests

### Engine unit tests (`engine/dispatch.rs::mythos_phase_tests`)

- `mythos_phase_seeds_draw_pending_on_entry` — after `step_phase(Upkeep→Mythos)`, `state.mythos_draw_pending == Some(lead)`.
- `draw_encounter_card_rejects_outside_mythos` — Investigation phase → `Rejected`.
- `draw_encounter_card_rejects_when_no_pending` — `mythos_draw_pending == None` → `Rejected`.
- `draw_encounter_card_rejects_out_of_order` — `mythos_draw_pending == Some(inv1)`, inv2 submits → `Rejected`.
- `draw_encounter_card_rejects_on_empty_deck_and_discard` — initial draw with both empty → `Rejected` (not panic — this is the legitimate-but-rejectable case from `chain_count == 1`).
- `draw_encounter_card_advances_pending_in_turn_order` — 2-investigator setup, inv1 draws, `mythos_draw_pending` becomes `Some(inv2)`.
- `last_drawer_opens_mythos_after_draws_window` — single investigator, no Fast-eligible cards, no reactions: assert event sequence `WindowOpened(MythosAfterDraws)` → `WindowClosed(MythosAfterDraws)` → `PhaseEnded(Mythos)` → `PhaseStarted(Investigation)`. Window never lands on `state.open_windows`.
- `mythos_phase_skipped_on_first_round_via_start_scenario` — `StartScenario` skips Mythos entirely. Assert: NO `PhaseStarted(Mythos)` event in the emitted events; NO `PhaseEnded(Mythos)` event either. Subsequence assertion (shuffle/deal events from opening hand setup interleave): `ScenarioStarted` → `PhaseStarted(Investigation)` (investigation_phase) → `TurnStarted` for lead (investigation_phase's rotate). At end: `state.phase == Investigation`, `state.active_investigator == Some(lead)`, `state.round == 1`. Update any existing scenario-start tests in `crates/game-core/src/engine/mod.rs` that asserted on the prior phantom Mythos emits.
- `investigation_phase_entry_rotates_to_lead` — call `step_phase(Mythos→Investigation)` (or invoke `investigation_phase` directly) in a 2-investigator setup; assert `state.active_investigator == Some(lead)` and `Event::PhaseStarted(Investigation)` precedes the `Event::TurnStarted` for lead in the events.
- `investigation_phase_entry_no_op_when_turn_order_empty` — invoke `investigation_phase` with `state.turn_order` empty; assert `PhaseStarted(Investigation)` emitted and `state.active_investigator == None` (the rotation is skipped because `turn_order.first()` is `None`).
- `end_turn_pauses_at_mythos_draw_pending` — after the last investigator's `EndTurn`, state is in Mythos phase with `mythos_draw_pending == Some(lead)`, `active_investigator == None`, outcome was `Done` (not `AwaitingInput`).
- *No test for `peril_check` itself.* The function body is intentionally a TODO; there is no behavior to assert. The function's existence is verified by the type checker (the call site in `mythos_draw_for` won't compile without it). When the future peril-enforcement PR lands, the test for the conferral restriction lives in that PR alongside the real body.

### Engine unit tests for Fast-eligibility scan

- `any_fast_play_eligible_returns_false_on_empty_hands` — no investigators have any cards → `false`.
- `any_fast_play_eligible_finds_fast_event_in_hand` — one investigator has a Fast event in hand; the timing context permits Fast play → `true`.
- `any_fast_play_eligible_skips_non_fast_card` — only non-Fast cards in hand → `false`.
- `any_fast_play_eligible_finds_zero_cost_activated_ability` — investigator has an in-play card with a `Trigger::Activated { action_cost: 0, .. }` ability → `true`.
- `any_fast_play_eligible_skips_exhausted_zero_cost_activated_ability` — same as above but the card is exhausted / usage-limit-exhausted → `false`.

### Engine unit tests for the validator extraction

- `check_play_card_matches_play_card_rejection` — for a known-rejecting setup, the extracted `check_play_card` returns `Err` with the same reason that `play_card` returns in `EngineOutcome::Rejected`. (Sanity check that the extraction didn't drift.)
- Similar for `check_activate_ability`.

### Integration tests (`crates/scenarios/tests/mythos_phase.rs`)

New file. Separate cargo binary so it can `install(TEST_REGISTRY)` without colliding with other test runs.

- `mythos_phase_resolves_single_treachery` — 1 investigator, encounter deck `[synth_treachery]`. Drive: `StartScenario` → `EndTurn` (chains to first Mythos draw pending) → `DrawEncounterCard(inv1)`. Assert: encounter deck empty, discard has `[synth_treachery]`, events show `CardRevealed { code: synth_treachery, .. }` + Revelation gain-resources events, `WindowOpened(MythosAfterDraws)` + `WindowClosed(MythosAfterDraws)` back-to-back, `PhaseEnded(Mythos)` + `PhaseStarted(Investigation)`, `state.phase == Investigation`, `state.active_investigator == Some(inv1)`.
- `mythos_phase_resolves_single_spawn_enemy` — 1 investigator, encounter deck `[synth_enemy]`. Assert enemy spawned + engagement-on-spawn.
- `mythos_phase_surge_chains_into_next_card` — 1 investigator, encounter deck `[synth_surge_treachery, synth_treachery]`. Single `DrawEncounterCard` action: events show 2× `CardRevealed` in order (surge first, then base treachery), both Revelations fired, encounter deck empty, both treacheries in discard.
- `mythos_phase_multi_investigator_player_order` — 2 investigators, encounter deck `[A, B]` (two distinct treacheries). Drive `StartScenario` → ... → inv1 draws A, `mythos_draw_pending` advances to inv2 → inv2 draws B → window opens-and-closes → Investigation begins. Events show A's `CardRevealed` before B's.
- `mythos_phase_full_round_chain` — drives `StartScenario` → Investigation actions for inv1 (1 investigator setup) → `EndTurn` → assert chain went through Enemy/Upkeep/Mythos 1.1/1.2/1.3, paused at draw-pending, completed via `DrawEncounterCard`, ended at Investigation phase round 2.

## Edge cases handled by design

- **`StartScenario` first-round skip preserved (and tightened).** `start_scenario` sets `state.phase = Investigation` directly (rather than transiently Mythos), emits `ScenarioStarted` + opening shuffles/deals/mulligan setup, then calls `investigation_phase`. No Mythos boundary events fire at all — the phase doesn't happen. This is a small behavior change from the existing engine (which currently emits a phantom `PhaseStarted(Mythos)` + `PhaseEnded(Mythos)` pair on scenario start); the new behavior matches Rules Reference p.24 literally ("During the first round of the game, skip the mythos phase"). Any existing test that asserted on the phantom Mythos emits at scenario-start needs updating.
- **Surge with truly-empty deck-and-discard.** Two `unreachable!()` sites in `mythos_draw_for` catch malformed scenario data: (a) `MAX_SURGE_CHAIN = 64` cap on chain length (catches infinite reshuffle loops with surging treacheries — Rules Reference p.18 places treachery discard before surge re-draw, enabling p.10 reshuffles to cycle); (b) mid-chain `draw_encounter_top` returning `None` (catches surging-enemy chains exhausting the universe — enemies spawn to play, not discard). Both are scenario-data-malformation class issues, consistent with the codebase's existing `unreachable!()` pattern for state-corruption invariants.
- **Initial draw with empty deck+discard.** Distinguished from mid-chain via `chain_count == 1`: legitimate `Rejected` outcome (the action is valid but can't proceed; player can retry once cards exist). Doesn't panic.
- **`DrawEncounterCard` during the post-1.4 window.** `mythos_draw_pending == None` at that point, so the action rejects.
- **Same investigator re-submitting `DrawEncounterCard` after their chain.** Out-of-order rejection (they're no longer in `mythos_draw_pending`).
- **Degenerate empty turn order.** `mythos_phase` opens the `MythosAfterDraws` window directly (no draws to wait for); `open_fast_window`'s auto-skip path fires because there are no investigators with Fast-eligible cards either, so the window closes inline and `mythos_phase_end` runs immediately. `end_turn`'s chain continues to Investigation.
- **Investigator without a location during their Mythos draw.** If the drawn card is an enemy with default-spawn, `spawn_enemy` already rejects per #135. The rejection bubbles through `resolve_encounter_card` → `mythos_draw_for` → `draw_encounter_card`. State is left mid-chain (events for the prior chain steps emitted before the failure). Per the existing `play_card` caveat in CLAUDE.md, this matches the "mid-resolution rejection" pattern; the apply loop's `events.clear()` on `Rejected` wipes the event stream. Acceptable for Phase 4 — synthetic fixture ensures investigators always have a location.

## Open questions (settled enough to start)

- **Naming of the Fast-window opener.** `open_fast_window` was chosen over alternatives like `push_player_window`, `queue_player_window` to (a) emphasize the auto-skip behavior when nothing is eligible, and (b) avoid collision with the existing `queue_reaction_window` semantics. Final name decision can settle during implementation review.
- **Whether `peril_check` is worth keeping as a function vs. an inline comment.** The body is a TODO and the call only carries the rule-step marker. Keeping the function makes it grep-able by `peril_check` and ensures the future enforcement PR has a single landing site; the function's existence is the recommendation but the implementer may collapse to a `// 1.4 step 2: peril check — TODO(future-peril-PR)` comment if the empty function feels gratuitous.
- **Continuation registration shape.** `run_window_continuation` is a `match` on `WindowKind` in `dispatch.rs`. Cleaner long-term might be a continuation registered with the window itself (closure / id), but per the project's no-speculative-machinery rule, the match is appropriate while only one window kind has a continuation.

## Follow-up issues to file

1. **Investigation phase full driver: sub-steps 2.1 / 2.2 / 2.2.1 / 2.2.2 / 2.3 per Rules Reference p.24.** #69 ships only a minimal Investigation skeleton (`PhaseStarted(Investigation)` + lead-first rotate). The full driver lands the explicit per-step structure parallel to what #69 does for Mythos: the post-2.1 player window, the 2.2 player-pick action (replacing the lead-first default with explicit selection inside the post-2.1 window), the 2.2.1 action-taking sub-loop (currently lives implicitly across Investigate / Move / PlayCard / etc. handlers), 2.2.2 turn-end formalization, and 2.3 phase-end. This is the Investigation-phase parallel to #69 (Mythos), #70 (Upkeep), #71/#128 (Enemy) — file it as a peer Phase-4-or-later issue so the same rules-faithful step structure applies to every phase. Should also land the `investigation_phase_end(state, events)` helper that owns the `PhaseEnded(Investigation)` emit (mirror of `mythos_phase_end`), letting `step_phase` suppress its `PhaseEnded(Investigation)` fallback.
2. **Pipeline: surge / peril keyword parsing.** File when the first Phase-7+ scenario includes a real surge or peril card. The pipeline currently emits `false` for everything; the fix is small (regex against the card's text field or, preferably, structured parse of an upstream `keywords` JSON field if present). Out of scope here because no in-scope card forces it.
2. **Generalize `WindowKind` toward a single `PlayerWindow` variant.** The current per-timing-point variants (`BetweenPhases`, `MythosAfterDraws`, future siblings) serve as routing keys for reaction triggers — but in practice today, no card text keys off most of them specifically (only `AfterEnemyDefeated` carries routing-load-bearing data via its `enemy` / `by` fields). Once enough phase-content PRs land and we have a clearer picture of which variants carry real routing data, consider collapsing the rest into a generic `PlayerWindow` (with `phase: Phase` + `marker: PhaseStep` for the few that need timing-point disambiguation). Not blocking #69.
3. **Peril enforcement.** The conferral restriction needs machinery for cross-investigator commit blocking + Fast-card play gating from non-drawing investigators. Not in scope until the first peril-bearing card is in a planned PR.

## Phase-doc entries

Once the PR merges:

- Move `#69` from the Open table to the Closed table in `docs/phases/phase-4-scenario-plumbing.md`.
- Flip Ordering / Arc row #6 to `✅ PR #N`.
- Add Decisions made entries for:
  - **Phase-driver pattern: each phase has a driver function (owns `PhaseStarted` as step N.1) and an end helper (owns `PhaseEnded`).** #69 lands `mythos_phase` + `mythos_phase_end` (full) and `investigation_phase` (skeleton: emit + rotate to lead per Rules Reference p.24 step 2.2). #70 / #71 / the future full Investigation driver land their own peers and replace the remaining direct boundary emits in `step_phase`. `start_scenario`'s round-1 path bypasses both the Mythos driver and end helper entirely — no Mythos boundary events fire — because per Rules Reference p.24 the phase is skipped, not just empty.
  - **Player-initiated phase actions are peers to action-phase actions, not `AwaitingInput` sub-choices.** `PlayerAction::DrawEncounterCard` sits alongside `Investigate` / `Move` / `PlayCard`. Future per-investigator phase content (Upkeep choices, Enemy responses) follows the same shape unless it's genuinely a sub-choice within a resolving effect.
  - **No "end round" / "end phase" actions; `EndTurn` auto-chains across phase boundaries.** The chain pauses only when player input is genuinely required (Mythos 1.4 draws, future printed Fast windows that don't auto-skip). UI gets discrete pauses for free at the natural beats.
  - **`open_fast_window` helper for printed-rule Fast windows.** Always emits `WindowOpened`; auto-skips (emits `WindowClosed` + runs continuation inline) when no reactions queue AND `any_fast_play_eligible` returns false. Eligibility uses the extracted `check_play_card` / `check_activate_ability` validators so the real PlayCard / ActivateAbility gates back it, not a parallel weak filter. #70 / #71 / future Investigation-driver PRs use this helper for their printed player windows.
- No Open questions in the phase doc are settled by this PR (the remaining ones cover hunter movement #128 and resolution-fired idempotency #131, neither of which #69 touches). Leave them.
