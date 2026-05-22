# #126 — Revelation DSL + on-draw resolution (design)

GitHub issue: [#126](https://github.com/talelburg/eldritch/issues/126) · Phase: [phase-4-scenario-plumbing](../../phases/phase-4-scenario-plumbing.md) · Depends on #72 (encounter deck state, must have shipped) · Followed by #127 (enemy spawn rules).

## Context
Phase-4 scenario plumbing needs a way for treachery cards to do anything when they're drawn. This PR lands the DSL surface (`Trigger::Revelation`, `EventPattern::CardRevealed`), the engine path that runs Revelation effects when an encounter card is revealed, and a synthetic treachery proving the wiring end-to-end.

We're in the middle of a three-PR sequence: #72 (encounter deck state) shipped first; this PR establishes the on-draw dispatch with the treachery arm complete and the enemy arm rejecting loudly; #127 replaces the enemy stub with the spawn handler.

## Scope
- DSL: `Trigger::Revelation`, `EventPattern::CardRevealed { card_type }`, builder `pub fn revelation(...)`.
- Engine: `Event::CardRevealed`, `EngineRecord::EncounterCardRevealed`, dispatch handler with full treachery arm and stub enemy arm.
- Test fixture: synthetic treachery card + `TEST_REGISTRY` exposed from `scenarios::test_fixtures` so integration tests can install it instead of `cards::REGISTRY`.
- Integration test: end-to-end reveal-treachery flow.
- Scenario setup wiring: synthetic fixture's `setup()` now populates `encounter_deck` with the synthetic treachery so the integration test exercises the real draw path.

## DSL additions
File: `crates/card-dsl/src/dsl.rs`.

```rust
enum Trigger {
    // ... existing variants ...
    /// Fires when the owning card is revealed from the encounter deck.
    /// First consumer: the synthetic treachery in Phase-4 test_fixtures.
    Revelation,
}

enum EventPattern {
    EnemyDefeated { by_controller: bool },           // unchanged
    /// An encounter card was revealed. `card_type` narrows the match:
    /// `None` matches any reveal; `Some(CardType::Treachery)` is the
    /// canonical Forewarned-style cancellation pattern.
    CardRevealed { card_type: Option<CardType> },
}

/// Construct a [`Trigger::Revelation`]-driven [`Ability`] wrapping the
/// given effect. Mirrors [`on_play`] / [`on_commit`].
pub fn revelation(effect: Effect) -> Ability {
    Ability { trigger: Trigger::Revelation, effect, ... }
}
```

**`CardRevealed` field choice:** `card_type: Option<CardType>` rather than `by_controller: bool`. Encounter draws are engine-driven, not card-controlled, so `by_controller` doesn't fit the semantics. Treachery-vs-enemy narrowing is the load-bearing distinction for hypothetical Forewarned-style listeners. `None` matches any reveal, `Some(card_type)` narrows.

## Engine — new event + engine-record action
Files: `crates/game-core/src/event.rs`, `crates/game-core/src/action.rs`.

```rust
// event.rs
enum Event {
    // ... existing ...
    CardRevealed {
        investigator: InvestigatorId,
        code: CardCode,
        card_type: CardType,
    },
}

// action.rs
enum EngineRecord {
    DeckShuffled { .. },
    EncounterDeckShuffled,
    /// The named investigator reveals the top of the encounter deck.
    /// Emitted by #69's Mythos draw loop; in #126's tests, issued
    /// directly to exercise the on-draw path.
    EncounterCardRevealed { investigator: InvestigatorId },
}
```

## Dispatch handler
File: `crates/game-core/src/engine/dispatch.rs`. Validate-first / mutate-second shape, with one documented exception (see "Validate-first contract" note below).

```rust
fn encounter_card_revealed(
    state: &mut GameState,
    events: &mut Vec<Event>,
    investigator: InvestigatorId,
) -> EngineOutcome {
    // validate
    let registry = match card_registry::current() {
        Some(r) => r,
        None => return Rejected { reason: "no card registry installed".into() },
    };
    let code = match draw_encounter_top(state, events) {
        Some(c) => c,
        None => return Rejected { reason: "encounter deck and discard both empty".into() },
    };
    let metadata = match (registry.metadata_for)(&code) {
        Some(m) => m,
        None => return Rejected { reason: format!("unknown card code: {code:?}").into() },
    };

    // emit reveal BEFORE Revelation resolves — Before-timing listeners
    // (none yet; structural for #52 reaction-window machinery) need it
    events.push(Event::CardRevealed {
        investigator,
        code: code.clone(),
        card_type: metadata.kind,
    });

    match metadata.kind {
        CardType::Treachery => {
            for ability in (registry.abilities_for)(&code).unwrap_or_default() {
                if matches!(ability.trigger, Trigger::Revelation) {
                    // call apply_effect with controller = investigator,
                    // source = the revealed card's instance (mint a transient
                    // CardInstanceId for the revelation source, or pass None
                    // — pick to match how OnPlay handles non-asset sources)
                    apply_effect(&ability.effect, state, events, investigator, ...)?;
                }
            }
            state.encounter_discard.push(code);
            Done
        }
        CardType::Enemy => Rejected {
            reason: "encounter enemy spawn lands in #127".into()
        },
        _ => Rejected {
            reason: format!("invalid encounter card type: {:?}", metadata.kind).into()
        },
    }
}
```

### Validate-first contract note
The handler emits `Event::CardRevealed` and mutates `encounter_deck` (via `draw_encounter_top`) before the enemy arm rejects. That's a transient violation of the project's validate-first / mutate-second convention. Two reasons it's acceptable:

1. The enemy arm is unreachable in #126's intended scope (the synthetic deck contains only the synthetic treachery). The reject exists to lock in a regression test, not as a real run-time path.
2. #127 replaces the enemy reject with the real spawn branch, after which the only "early emit" is `Event::CardRevealed` itself — which is intentional, because Before-timing listeners need the event to fire before Revelation resolves (rules-correct interposition point).

PR description should explicitly call this out, citing `play_card`'s documented mid-resolution caveat in CLAUDE.md as the precedent. Reviewers WILL flag the early emit; better to address it in the description than in review back-and-forth.

## Test fixture surface

### Synthetic test cards
New file: `crates/scenarios/src/test_fixtures/synth_cards.rs`. Gated behind the existing `test_fixtures` feature (default-on, per the existing pattern locked in by PR #130).

```rust
pub const SYNTH_TREACHERY_CODE: &str = "_synth_treachery";

// Underscore prefix: cannot collide with real ArkhamDB codes (which are
// digit-prefixed like "01001"). Future synthetic codes follow this convention.

pub static SYNTH_METADATA: Lazy<BTreeMap<CardCode, CardMetadata>> = Lazy::new(|| {
    let mut m = BTreeMap::new();
    m.insert(SYNTH_TREACHERY_CODE.into(), CardMetadata {
        kind: CardType::Treachery,
        // ... other required fields with trivial defaults ...
    });
    m
});

pub fn synth_metadata_for(code: &CardCode) -> Option<&'static CardMetadata> {
    SYNTH_METADATA.get(code).map(/* ... */)
}

pub fn synth_abilities_for(code: &CardCode) -> Option<Vec<Ability>> {
    match code.as_str() {
        SYNTH_TREACHERY_CODE => Some(vec![
            revelation(/* "lose 1 resource from active investigator" — see Effect-variant note below */),
        ]),
        _ => None,
    }
}

pub const TEST_REGISTRY: CardRegistry = CardRegistry {
    metadata_for: synth_metadata_for,
    abilities_for: synth_abilities_for,
};
```

**Effect-variant note:** the synthetic treachery's effect is "lose 1 resource from active investigator." Pick the matching DSL primitive at implementation time — `card-dsl/src/dsl.rs`'s `Effect` enum already has resource-changing primitives from Phase 3. If `Effect::GainResources { delta: i8 }` exists with signed delta, use `delta: -1`. If it's unsigned and there's no `LoseResources`, add one — but check first that the corpus doesn't already have a "you lose N resources" treachery whose impl forces the right shape.

### Updated synthetic scenario fixture
Update `crates/scenarios/src/test_fixtures/synthetic.rs`'s `setup()` to populate `encounter_deck` with one copy of `SYNTH_TREACHERY_CODE`. The existing resolution predicate (`phase == Investigation && round >= 1`) stays unchanged; the encounter deck just exists for the new integration tests.

## Test plan
1. **Unit: builder.** `revelation(effect)` produces `Ability { trigger: Trigger::Revelation, effect, .. }`. Asserts the trigger is `Revelation` and the effect round-trips.
2. **Unit: EventPattern serde.** `EventPattern::CardRevealed { card_type: Some(CardType::Treachery) }` and `{ card_type: None }` serialize and deserialize losslessly.
3. **Integration in `crates/scenarios/tests/encounter_reveal.rs`** — new file, follows the `crates/cards/tests/play_card.rs` pattern (own cargo binary, installs `TEST_REGISTRY` once at the top):
   - Build `GameState` with `SYNTH_TREACHERY_CODE` at top of `encounter_deck`, one investigator with starting resources.
   - Apply `EngineRecord::EncounterCardRevealed { investigator }`.
   - Assert `Event::CardRevealed { investigator, card_type: CardType::Treachery, .. }` fires.
   - Assert the Revelation effect resolved (active investigator's resources dropped by 1).
   - Assert `state.encounter_discard` contains the code, `encounter_deck` does not.
4. **Integration: enemy stub.** Build state with a test enemy code (synthetic enemy metadata with `kind: Enemy`) at top of deck. Apply `EncounterCardRevealed`. Assert `Rejected { reason: ... "lands in #127" ... }`. Locks the stub behavior for #127 to flip.
5. **Integration: empty.** Empty deck + empty discard → `EncounterCardRevealed` rejects with the "deck and discard both empty" reason.
6. **Integration: no registry.** Without installing a registry (skip the `install` call), apply → rejects cleanly with "no card registry installed."

## Phase-doc update (last commit of the PR)
File: `docs/phases/phase-4-scenario-plumbing.md`.

- Move `#126` from Open → Closed; bump counts.
- Flip Ordering row 4 to `✅ PR #N`.
- Add a Decision entry: **"`EventPattern::CardRevealed { card_type: Option<CardType> }` (`#126`, PR #N).** Chose `card_type` narrowing over the `EnemyDefeated`-mirror `by_controller: bool`. Encounter draws are engine-driven, not card-controlled, so `by_controller` doesn't fit the semantics; treachery-vs-enemy narrowing is the load-bearing distinction for hypothetical Forewarned-style listeners. The first real listener (Phase-7 or later) gets to confirm or extend." (Load-bearing because future listener cards will gate against this surface.)
- Add a Decision entry: **"`Event::CardRevealed` emits before Revelation resolves (`#126`, PR #N).** Intentional ordering: Before-timing reaction listeners (#52's machinery; not yet wired) need the event to fire first so they can interpose / cancel. Looks like a validate-first violation on the surface, but the only state-changing pre-emit op is the encounter-deck draw, which is itself the load-bearing 'reveal' moment per the Rules Reference." (Load-bearing because reviewers will flag the early emit; the decision entry documents *why* it's intentional.)
- Drop the "Window-stack invariants" open question only if this PR's reveal-window placement settles it; otherwise leave it.

## Out of scope (deferred)
- Enemy spawn handling — that's #127, which flips the stub.
- Mythos draw loop / Surge — that's #69.
- "Attach" mechanics for treacheries that persist after Revelation — no in-scope card.
- `Trigger::Revelation` on enemies driving multi-step encounter resolution — no Phase-4 card needs this.
- Real Phase-7 treachery cards using Revelation — picked up in Phase 7.

## Open items resolved at implementation time
- Exact `Effect` variant for "lose 1 resource" — see Effect-variant note above.
- `apply_effect`'s exact signature — match the existing call sites in `engine/evaluator.rs` (OnPlay path is the closest analogue: no in-play instance to pass as source, controller is the active/drawing investigator).
- Whether to mint a transient `CardInstanceId` for the revealed treachery as a Revelation source, or pass `None` — match whatever the OnPlay path does for events (which also have no in-play instance).
