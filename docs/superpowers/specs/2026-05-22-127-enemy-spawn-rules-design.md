# #127 — enemy spawn rules (design)

GitHub issue: [#127](https://github.com/talelburg/eldritch/issues/127) · Phase: [phase-4-scenario-plumbing](../../phases/phase-4-scenario-plumbing.md) · Depends on #72 (encounter deck state) and #126 (Revelation DSL + on-draw resolution), both must have shipped.

## Context
Phase-4 scenario plumbing's third PR in the encounter trio. #72 landed the encounter deck state. #126 landed the on-draw resolution dispatch with the treachery arm complete and the enemy arm rejecting loudly with `reason: "encounter enemy spawn lands in #127"`. This PR replaces that reject with the real spawn handler, lands the `Spawn` keyword surface, and adds a synthetic spawn-bearing enemy proving the wiring.

## Scope
- Card-data: `Spawn` struct + `SpawnLocation::Specific` on `CardMetadata`.
- DSL: `EventPattern::EnemySpawned` (bare; narrowing later).
- Engine: `Event::EnemySpawned`, `spawn_enemy` handler, wiring into the existing `encounter_card_revealed` dispatch by replacing the enemy-arm reject.
- Test fixture: synthetic spawn-bearing enemy in `synth_cards.rs` from #126; integration test.

## Card-data additions
File: `crates/card-dsl/src/card_data.rs`.

```rust
enum SpawnLocation {
    /// Fixed-location spawn — the named location code.
    Specific(LocationCode),
}

struct Spawn {
    pub location: SpawnLocation,
}

struct CardMetadata {
    // ... existing fields ...
    /// Spawn rule for encounter-deck enemies. `None` for enemies that
    /// don't spawn from the encounter deck (placed at scenario setup
    /// directly) and for all non-enemy card types.
    pub spawn: Option<Spawn>,
}
```

**Why `Spawn` is a struct, not flat fields:** so it can grow without churning every enemy declaration — e.g. `engagement: EngagementOnSpawn` for Aloof / "Spawn unengaged" cards, or `also_spawn_doom_at: ...` for the rarer multi-effect spawn cards. Only `SpawnLocation::Specific` lands now; further variants (`LeadInvestigator`, `LowestSanityInvestigator`, etc.) land with Phase-7 when concrete cards force them.

**Pipeline:** `crates/card-data-pipeline/` leaves `spawn: None` for all generated enemies in this PR. Structured parsing of upstream spawn text is deferred to the first Phase-7 PR that consumes it. Regenerate `crates/cards/src/generated/cards.rs` after the field is added so every existing enemy entry gets `spawn: None`.

## DSL additions
File: `crates/card-dsl/src/dsl.rs`.

```rust
enum EventPattern {
    EnemyDefeated { by_controller: bool },
    CardRevealed { card_type: Option<CardType> },    // from #126
    /// An enemy spawned at a location.
    EnemySpawned,
}
```

`EnemySpawned` is intentionally bare. YAGNI on narrowing fields until a real listener forces the shape — the project's concrete-consumer-first principle. The Decision entry on the phase doc should note this so a future Phase-7 PR knows it owns the extension.

## Engine — new event
File: `crates/game-core/src/event.rs`.

```rust
enum Event {
    // ... existing ...
    EnemySpawned {
        enemy: EnemyId,
        code: CardCode,
        location: LocationId,
        /// If the spawn engaged an investigator, who. None if the
        /// enemy spawned at a location with no investigators.
        engaged_with: Option<InvestigatorId>,
    },
}
```

## Spawn handler
File: `crates/game-core/src/engine/dispatch.rs`. Called from the existing `encounter_card_revealed` handler (after we replace the enemy-arm reject).

```rust
fn spawn_enemy(
    state: &mut GameState,
    events: &mut Vec<Event>,
    investigator: InvestigatorId,
    code: CardCode,
    metadata: &CardMetadata,
) -> EngineOutcome {
    // Resolve spawn location.
    //
    // Rules Reference page 24 (1.4 Each investigator draws 1 encounter card):
    //   "If the encountered enemy has no spawn instruction, the enemy
    //    spawns engaged with the investigator encountering the card and
    //    is placed in that investigator's threat area."
    //
    // We model the threat-area placement as: enemy.location = drawing
    // investigator's location, engaged_with = drawing investigator (set
    // below by the engagement step).
    let location_id = match &metadata.spawn {
        Some(Spawn { location: SpawnLocation::Specific(loc_code) }) => {
            // find by code
            state.locations.iter()
                .find(|(_, loc)| &loc.code == loc_code)
                .map(|(id, _)| *id)
                .ok_or_else(|| /* reject: spawn location not in play */)?
        }
        None => {
            state.investigators[&investigator].location
                .ok_or_else(|| /* reject: drawing investigator has no location */)?
        }
    };

    // Resolve engagement.
    //
    // Rules Reference page 10 (Enemy Engagement):
    //   "Any time a ready unengaged enemy is at the same location as an
    //    investigator, it engages that investigator, and is placed in
    //    that investigator's threat area. If there are multiple
    //    investigators at the same location as a ready unengaged enemy,
    //    follow the enemy's prey instructions to determine which
    //    investigator is engaged."
    let investigators_at_loc: Vec<_> = state.investigators.iter()
        .filter(|(_, inv)| inv.location == Some(location_id))
        .map(|(id, _)| *id)
        .collect();
    let engaged_with = match investigators_at_loc.as_slice() {
        [] => None,
        [single] => Some(*single),
        _ => return Rejected {
            reason: "multi-investigator engagement-on-spawn requires Prey (lands in #128)".into()
        },
    };

    // Mint and place.
    let enemy_id = mint_enemy_id(state);   // pattern: state.next_card_instance_id, mirroring existing enemy minting
    let mut enemy = Enemy::from_metadata(enemy_id, code.clone(), metadata);
    enemy.location = location_id;
    enemy.engaged_with = engaged_with;
    state.enemies.insert(enemy_id, enemy);

    events.push(Event::EnemySpawned {
        enemy: enemy_id,
        code,
        location: location_id,
        engaged_with,
    });
    Done
}
```

The exact field names on `Enemy` (`location` / `engaged_with`) should match what `crates/game-core/src/state/enemy.rs` already exposes from the Phase-3 enemy state work (#67). Adjust if those landed under different names.

**Multi-investigator engagement-on-spawn rejects** with a reason naming #128. The phase doc's #128 row (hunter movement) picks up the work because hunter movement and prey resolution share the `Prey` shape. Acceptable for Phase-4 because the synthetic fixture has one investigator. The Decision entry on the phase doc should document this so #128's author knows they inherit it.

PR description must cite the verbatim Rules Reference clauses (page 10 + page 24) inline. Engine doc-comments on `spawn_enemy` should embed both quotes.

## Dispatch wiring — extend `encounter_card_revealed`
Replace the `CardType::Enemy => Rejected { ... }` arm landed in #126:

```rust
CardType::Enemy => {
    // Revelation on enemies is rare but possible (some encounter
    // enemies have "Revelation — ..." printed). Run first, mirroring
    // the treachery path.
    for ability in (registry.abilities_for)(&code).unwrap_or_default() {
        if matches!(ability.trigger, Trigger::Revelation) {
            apply_effect(&ability.effect, state, events, investigator, ...)?;
        }
    }
    spawn_enemy(state, events, investigator, code, metadata)
}
```

Enemies are NOT discarded — they stay in `state.enemies` (in play). Treachery and enemy paths diverge at this final step: treachery → discard; enemy → stays. The integration test asserts the absence of the enemy code in `encounter_discard`.

After this PR, the only intentional pre-validation emit in `encounter_card_revealed` is `Event::CardRevealed` itself, which is rules-correct (Before-timing listener interposition point). The "transient validate-first quirk" the #126 PR description called out is fully resolved.

## Synthetic spawn-bearing enemy
Extend `crates/scenarios/src/test_fixtures/synth_cards.rs` (from #126):

```rust
pub const SYNTH_ENEMY_CODE: &str = "_synth_enemy";

// In SYNTH_METADATA:
m.insert(SYNTH_ENEMY_CODE.into(), CardMetadata {
    kind: CardType::Enemy,
    spawn: Some(Spawn { location: SpawnLocation::Specific(SYNTH_LOC_CODE) }),
    // ... trivial stat defaults: 1 health, 0 damage, 0 horror, 1 fight, 1 evade ...
});

// synth_abilities_for returns None for SYNTH_ENEMY_CODE (no Revelation, no on-play triggers).
```

`SYNTH_LOC_CODE` is the test-fixture location's code already present in the `crates/scenarios/src/test_fixtures/synthetic.rs` setup. If the location doesn't carry a `code` field today, add one (verify against `crates/game-core/src/state/location.rs` at implementation time).

## Test plan
1. **Unit: serde.** `Spawn` + `SpawnLocation::Specific(...)` round-trip through serde losslessly.
2. **Unit: spawn handler edges.**
   - Non-existent location code in `SpawnLocation::Specific` → reject with the expected reason.
   - `spawn: None` with investigator missing location → reject.
3. **Integration in `crates/scenarios/tests/encounter_spawn.rs`** (new file, separate cargo binary):
   - Setup: deck has `SYNTH_ENEMY_CODE` at top, single investigator at `SYNTH_LOC`.
   - Apply `EngineRecord::EncounterCardRevealed { investigator }`.
   - Assert events in order: `CardRevealed { card_type: Enemy, .. }`, then `EnemySpawned { location: SYNTH_LOC, engaged_with: Some(active), .. }`.
   - Assert `state.enemies` contains the new enemy, at `SYNTH_LOC`, engaged with the active investigator.
   - Assert `state.encounter_discard` does NOT contain `SYNTH_ENEMY_CODE`.
4. **Integration: default spawn.** A test enemy with `spawn: None` (separate code, e.g. `_synth_enemy_no_spawn`) → spawns at the drawing investigator's location, engages them.
5. **Integration: multi-investigator reject.** Two investigators at `SYNTH_LOC`, spawn the Specific-location enemy → reject with the #128-pointing reason. Locks the deferral until #128 ships.
6. **Integration: location not in play.** Synthetic enemy targeting a location code that's not in the scenario → reject cleanly.

## Phase-doc update (last commit of the PR)
File: `docs/phases/phase-4-scenario-plumbing.md`.

- Move `#127` from Open → Closed; bump counts.
- Flip Ordering row 5 to `✅ PR #N`.
- Add a Decision entry: **"Multi-investigator engagement-on-spawn defers to #128 (`#127`, PR #N).** Single-investigator and zero-investigator cases handled per Rules Reference p.10 + p.24. The multi-investigator path requires Prey resolution, which shares its shape with hunter-movement target selection in #128 — rather than build a single-use prey resolver here, the engagement-on-spawn rejects with a reason pointing at #128. #128's author inherits the work of unifying the prey resolver across spawn and hunter-movement." (Load-bearing because #128's design has to account for this consumer.)
- Add a Decision entry: **"Default spawn (`spawn: None`) goes to drawing investigator's location (`#127`, PR #N).** Per Rules Reference p.24: 'If the encountered enemy has no spawn instruction, the enemy spawns engaged with the investigator encountering the card and is placed in that investigator's threat area.' We model threat-area placement as `enemy.location = drawing investigator's location` + `engaged_with = drawing investigator` (via the same engagement resolution as Specific spawns)." (Load-bearing because future spawn-keyword expansions need to know the no-instruction fallback.)
- Drop any settled Open question from the section above (none expected — the multi-investigator case stays open via #128).

## Out of scope (deferred)
- `SpawnLocation::LeadInvestigator` / `LowestSanityInvestigator` / `NearestUnexplored` etc. — Phase-7 picks these up.
- Aloof keyword affecting engagement-on-spawn — no Phase-4 card needs it.
- `EnemySpawned` pattern narrowing fields — added when a real listener lands.
- Pipeline structured-parsing of upstream spawn text — Phase-7.
- Multi-investigator engagement-on-spawn — #128 (Prey).
- "Discard X cards from the encounter deck" effects mid-spawn — irrelevant to this PR; lands with the card that needs it.

## Open items resolved at implementation time
- Exact `Enemy` field names (`location`, `engaged_with`) — verify against `crates/game-core/src/state/enemy.rs`.
- `mint_enemy_id` pattern — mirror existing enemy minting (likely `state.next_card_instance_id` increment, or a dedicated counter).
- Whether `LocationCode` already exists as a distinct type from `CardCode` — verify against `crates/card-dsl/src/card_data.rs` / `crates/game-core/src/state/location.rs`. If not, add it (locations and cards shouldn't share a code namespace).
- Trivial stat defaults for the synthetic enemy — pick anything that doesn't conflict with existing test fixtures.
