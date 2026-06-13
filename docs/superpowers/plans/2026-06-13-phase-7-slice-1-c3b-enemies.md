# C3b — The Gathering enemies Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the six Gathering encounter enemies spawn with their correct printed stats and keywords by parsing keyword/spawn/per-investigator-health data into the corpus and wiring `spawn_enemy` to read it.

**Architecture:** Combat stats already live in `CardKind::Enemy`. Add three keyword fields (`hunter`, `retaliate`, `prey`) and replace `health: Option<u8>` with `Option<HealthValue>` (mirroring the location `ClueValue` pattern). The pipeline parses these from card `text` and regenerates the corpus; `spawn_enemy` reads all of it, scaling per-investigator health by the in-game investigator count.

**Tech Stack:** Rust workspace — `card-dsl` (types), `card-data-pipeline` (codegen), `game-core` (engine), `cards` (corpus + integration tests).

**Spec:** `docs/superpowers/specs/2026-06-13-phase-7-slice-1-c3b-enemies-design.md`

**Verified stats** (from `data/arkhamdb-snapshot/pack/core/core_encounter.json`):

| Enemy | code | fight | evade | dmg | horror | health | victory | qty | keywords |
|---|---|---|---|---|---|---|---|---|---|
| Ghoul Priest | 01116 | 4 | 4 | 2 | 2 | 5 *(per-inv)* | 2 | 1 | Prey-Highest [combat], Hunter, Retaliate |
| Flesh-Eater | 01118 | 4 | 1 | 1 | 2 | 4 | 1 | 1 | Spawn-Attic |
| Icy Ghoul | 01119 | 3 | 4 | 2 | 1 | 4 | 1 | 1 | Spawn-Cellar |
| Ghoul Minion | 01160 | 2 | 2 | 1 | 1 | 2 | — | 3 | — |
| Ravenous Ghoul | 01161 | 3 | 3 | 1 | 1 | 3 | — | 1 | Prey-Lowest remaining health |
| Swarm of Rats | 01159 | 1 | 3 | 1 | 0 | 1 | — | 3 | Hunter |

Attic = `01113`, Cellar = `01114`.

**Pre-flight (run once before Task 1):**
```bash
git checkout -b card/c3b-gathering-enemies
```

---

## Task 1: Pipeline keyword/spawn parsers (pure helpers)

Pure functions + a pipeline-local `PreyParse` enum, TDD'd in isolation. They compile and pass before any struct change because they only produce `String`s / enums.

**Files:**
- Modify: `crates/card-data-pipeline/src/main.rs` (add helpers near `clue_value_lit` ~line 506; add tests in the existing `#[cfg(test)] mod tests` ~line 553)

- [ ] **Step 1: Write failing tests**

Add to the `#[cfg(test)] mod tests` block (the one importing `clue_value_lit, emit_card, …`). First extend its `use super::{…}` line to also import `health_value_opt_lit, parse_prey, parse_spawn_name, prey_lit, strip_html_bold, has_keyword, PreyParse`.

```rust
#[test]
fn strip_html_bold_removes_bold_tags() {
    assert_eq!(strip_html_bold("<b>Prey</b> - Highest [combat]."), "Prey - Highest [combat].");
}

#[test]
fn has_keyword_matches_standalone_token() {
    assert!(has_keyword("Hunter. Retaliate.", "Hunter"));
    assert!(has_keyword("Hunter. Retaliate.", "Retaliate"));
    assert!(!has_keyword("<b>Spawn</b> - Attic.", "Hunter"));
}

#[test]
fn parse_prey_recognizes_highest_combat() {
    assert_eq!(
        parse_prey("<b>Prey</b> - Highest [combat].\nHunter. Retaliate."),
        PreyParse::HighestSkill("Combat"),
    );
}

#[test]
fn parse_prey_recognizes_lowest_remaining_health() {
    assert_eq!(
        parse_prey("<b>Prey</b> - Lowest remaining health."),
        PreyParse::LowestRemainingHealth,
    );
}

#[test]
fn parse_prey_none_when_no_prey_line() {
    assert_eq!(parse_prey("Hunter."), PreyParse::None);
}

#[test]
fn parse_prey_unrecognized_keeps_clause() {
    assert_eq!(
        parse_prey("<b>Prey</b> - Most clues."),
        PreyParse::Unrecognized("Most clues".to_owned()),
    );
}

#[test]
fn prey_lit_emits_expected_literals() {
    assert_eq!(prey_lit(&PreyParse::None), "Prey::Default");
    assert_eq!(prey_lit(&PreyParse::Unrecognized("Most clues".to_owned())), "Prey::Default");
    assert_eq!(
        prey_lit(&PreyParse::HighestSkill("Combat")),
        "Prey::Ranked { direction: PreyDirection::Highest, measure: PreyMeasure::Skill(SkillKind::Combat) }",
    );
    assert_eq!(
        prey_lit(&PreyParse::LowestRemainingHealth),
        "Prey::Ranked { direction: PreyDirection::Lowest, measure: PreyMeasure::RemainingHealth }",
    );
}

#[test]
fn parse_spawn_name_extracts_location_name() {
    assert_eq!(parse_spawn_name("<b>Spawn</b> - Attic."), Some("Attic".to_owned()));
    assert_eq!(parse_spawn_name("<b>Spawn</b> - Cellar."), Some("Cellar".to_owned()));
    assert_eq!(parse_spawn_name("Hunter."), None);
}

#[test]
fn health_value_opt_lit_mirrors_clue_value() {
    assert_eq!(health_value_opt_lit(None, false), "None");
    assert_eq!(health_value_opt_lit(Some(4), false), "Some(HealthValue::Fixed(4))");
    assert_eq!(health_value_opt_lit(Some(5), true), "Some(HealthValue::PerInvestigator(5))");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p card-data-pipeline`
Expected: FAIL — `cannot find function/type` for the new helpers.

- [ ] **Step 3: Implement the helpers**

Add near `clue_value_lit` (~line 506):

```rust
/// Strip ArkhamDB bold markup so keyword lines can be matched plainly.
fn strip_html_bold(s: &str) -> String {
    s.replace("<b>", "").replace("</b>", "")
}

/// True if `text` contains the standalone keyword token `"<kw>."`
/// (e.g. `"Hunter."`). Bold tags are stripped first.
fn has_keyword(text: &str, keyword: &str) -> bool {
    strip_html_bold(text).contains(&format!("{keyword}."))
}

/// Parsed Prey line. `None` = no printed prey (emit `Prey::Default`);
/// `Unrecognized` = a "Prey - …" form we don't model yet (emit
/// `Prey::Default` + warn, matching `surge`/`peril`'s default-stub).
#[derive(Debug, PartialEq, Eq)]
enum PreyParse {
    None,
    /// `Prey - Highest [<skill>]`; payload is the `SkillKind` ident.
    HighestSkill(&'static str),
    /// `Prey - Lowest remaining health`.
    LowestRemainingHealth,
    /// A "Prey - …" clause not yet modeled (the clause text, trimmed).
    Unrecognized(String),
}

/// Parse an enemy's Prey line out of `text`.
fn parse_prey(text: &str) -> PreyParse {
    let stripped = strip_html_bold(text);
    let Some(rest) = stripped.split("Prey - ").nth(1) else {
        return PreyParse::None;
    };
    // The clause runs to the next period.
    let clause = rest.split('.').next().unwrap_or("").trim();
    match clause {
        "Highest [willpower]" => PreyParse::HighestSkill("Willpower"),
        "Highest [intellect]" => PreyParse::HighestSkill("Intellect"),
        "Highest [combat]" => PreyParse::HighestSkill("Combat"),
        "Highest [agility]" => PreyParse::HighestSkill("Agility"),
        "Lowest remaining health" => PreyParse::LowestRemainingHealth,
        other => PreyParse::Unrecognized(other.to_owned()),
    }
}

/// Emit the `Prey` literal for a parsed prey line.
fn prey_lit(p: &PreyParse) -> String {
    match p {
        PreyParse::None | PreyParse::Unrecognized(_) => "Prey::Default".to_owned(),
        PreyParse::HighestSkill(skill) => format!(
            "Prey::Ranked {{ direction: PreyDirection::Highest, \
             measure: PreyMeasure::Skill(SkillKind::{skill}) }}"
        ),
        PreyParse::LowestRemainingHealth => "Prey::Ranked { direction: PreyDirection::Lowest, \
             measure: PreyMeasure::RemainingHealth }"
            .to_owned(),
    }
}

/// Parse the location name from a `Spawn - <name>.` line, if present.
fn parse_spawn_name(text: &str) -> Option<String> {
    let stripped = strip_html_bold(text);
    let rest = stripped.split("Spawn - ").nth(1)?;
    Some(rest.split('.').next().unwrap_or("").trim().to_owned())
}

/// Emit the `Option<HealthValue>` literal for an enemy's health, mirroring
/// `clue_value_lit`. Polarity is the opposite of clues: per-investigator
/// only when ArkhamDB's `health_per_investigator` is set.
fn health_value_opt_lit(health: Option<u8>, per_investigator: bool) -> String {
    match health {
        None => "None".to_owned(),
        Some(n) if per_investigator => format!("Some(HealthValue::PerInvestigator({n}))"),
        Some(n) => format!("Some(HealthValue::Fixed({n}))"),
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p card-data-pipeline`
Expected: PASS (new tests green; helpers are dead-code until Task 2 — that is fine, `cargo test` does not deny warnings).

- [ ] **Step 5: Commit**

```bash
git add crates/card-data-pipeline/src/main.rs
git commit -m "$(cat <<'EOF'
pipeline: enemy keyword/spawn/health parse helpers (C3b)

Pure parsers for Hunter/Retaliate/Prey/Spawn-location and a
health_value_opt_lit mirroring clue_value_lit. Wired into the corpus
in the next commit.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: Extend `CardKind::Enemy`, wire the pipeline, regenerate the corpus

Atomic by necessity: adding fields to `CardKind::Enemy` breaks the generated corpus, so the struct change, the pipeline emit change, the corpus regen, and the hand-written literal fixes all land together.

**Files:**
- Modify: `crates/card-dsl/src/card_data.rs` (add `HealthValue` enum ~after `ClueValue` line 254; extend `CardKind::Enemy` ~line 325; fix serde test literal ~line 683)
- Modify: `crates/card-dsl/src/lib.rs` (export `HealthValue` alongside `ClueValue` ~line 34)
- Modify: `crates/card-data-pipeline/src/main.rs` (raw + normalized `health_per_investigator`; new `PreyParse`/spawn fields on `NormalizedCard`; wire `normalize`; spawn-name→code resolution pass in `run`; extend `render_kind` Enemy arm + generated `use` line; fix the `CardKind::Enemy { fight: 4, evade: 4,` assertion ~line 1020)
- Modify: `crates/scenarios/src/test_fixtures/synth_cards.rs:92` (literal)
- Modify: `crates/game-core/src/engine/dispatch/encounter.rs:968` (test-mock literal)
- Regenerate: `crates/cards/src/generated/cards.rs`

- [ ] **Step 1: Add the `HealthValue` enum**

In `crates/card-dsl/src/card_data.rs`, after the `ClueValue` enum (~line 254):

```rust
/// An enemy's printed health. Mirrors [`ClueValue`]: `PerInvestigator(n)`
/// scales by the number of investigators in the game (Rules Reference
/// p.12); `Fixed(n)` is a flat value. Distinguishes ArkhamDB's
/// `health_per_investigator` (absent/false → fixed; `true` →
/// per-investigator). Note the polarity is the opposite of `ClueValue`,
/// whose clues default to per-investigator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HealthValue {
    /// Exactly `value`, regardless of investigator count.
    Fixed(u8),
    /// `value × #investigators` at spawn time.
    PerInvestigator(u8),
}
```

- [ ] **Step 2: Extend `CardKind::Enemy`**

In the same file, in the `Enemy { … }` variant (~line 325): change the `health` field type and add three keyword fields (place the keyword cluster after `peril`, before `quantity`):

```rust
        /// Maximum health (per-investigator or fixed).
        health: Option<HealthValue>,
```
and after the `peril: bool,` field:
```rust
        /// Hunter keyword (Rules Reference p.12).
        hunter: bool,
        /// Retaliate keyword (Rules Reference p.18).
        retaliate: bool,
        /// Prey instruction (Rules Reference p.17); `Prey::Default` when
        /// the card prints no prey line.
        prey: Prey,
```

- [ ] **Step 3: Export `HealthValue`**

In `crates/card-dsl/src/lib.rs`, add `HealthValue` to the `pub use card_data::{…}` re-export list (alphabetically near `ClueValue`).

- [ ] **Step 4: Fix the `card-dsl` serde-test literal**

In `crates/card-dsl/src/card_data.rs` ~line 683 (`card_metadata_serde_roundtrip_preserves_spawn_specific`), update the enemy literal to the new shape:

```rust
            kind: CardKind::Enemy {
                fight: 3,
                evade: 2,
                damage: 1,
                horror: 1,
                health: Some(HealthValue::Fixed(1)),
                victory: None,
                spawn: Some(Spawn {
                    location: SpawnLocation::Specific("_synth_loc".into()),
                }),
                surge: false,
                peril: false,
                hunter: false,
                retaliate: false,
                prey: Prey::Default,
                quantity: 1,
            },
```
Ensure `Prey` and `HealthValue` are in scope in that test module (add to its `use` if needed).

- [ ] **Step 5: Add a `HealthValue` serde roundtrip assertion**

In the card-dsl tests, add:

```rust
#[test]
fn health_value_serde_roundtrip() {
    for hv in [HealthValue::Fixed(4), HealthValue::PerInvestigator(5)] {
        let json = serde_json::to_string(&hv).expect("serialize");
        assert_eq!(serde_json::from_str::<HealthValue>(&json).expect("deserialize"), hv);
    }
}
```

- [ ] **Step 6: Add pipeline raw + normalized `health_per_investigator` and the parsed fields**

In `crates/card-data-pipeline/src/main.rs`:

In `RawCard` (~line 174, next to `enemy_horror`):
```rust
    health_per_investigator: Option<bool>,
```
In `NormalizedCard` (~line 211, next to `enemy_horror`):
```rust
    enemy_horror: Option<u8>,
    health_per_investigator: bool,
    hunter: bool,
    retaliate: bool,
    prey: PreyParse,
    /// Location name parsed from a `Spawn - <name>.` line (pre-resolution).
    spawn_name: Option<String>,
    /// Spawn location code, resolved from `spawn_name` after all cards load.
    spawn_code: Option<String>,
```

- [ ] **Step 7: Populate the new fields in `normalize`**

In `normalize` (~line 230, in the `Ok(NormalizedCard { … })`), after `enemy_horror: raw.enemy_horror,`:
```rust
        health_per_investigator: raw.health_per_investigator.unwrap_or(false),
        hunter: raw.text.as_deref().is_some_and(|t| has_keyword(t, "Hunter")),
        retaliate: raw.text.as_deref().is_some_and(|t| has_keyword(t, "Retaliate")),
        prey: raw.text.as_deref().map_or(PreyParse::None, parse_prey),
        spawn_name: raw.text.as_deref().and_then(parse_spawn_name),
        spawn_code: None,
```
Note: `raw.text` is moved into the struct's `text` field — read these *before* that move, or clone. Simplest: compute them into `let` bindings at the top of `normalize` (alongside `is_fast`, which already reads `raw.text.as_deref()`), then move the bindings in. Mirror the existing `is_fast` pattern.

- [ ] **Step 8: Resolve `spawn_name` → `spawn_code` after all cards load**

In `run()` (~line 86, after the `for rel in PACK_FILES` loop, before `render(&all)`):
```rust
    // Resolve enemy Spawn-location names to location codes now that every
    // card is loaded. Unresolved names (out-of-scope forms like
    // "Engaged with Prey") stay None and warn — a loud stub, not silent.
    let loc_index: BTreeMap<String, String> = all
        .values()
        .filter(|c| c.card_type == "Location")
        .map(|c| (c.name.clone(), c.code.clone()))
        .collect();
    let mut resolutions: Vec<(String, Option<String>)> = Vec::new();
    for c in all.values() {
        if let Some(name) = &c.spawn_name {
            match loc_index.get(name) {
                Some(code) => resolutions.push((c.code.clone(), Some(code.clone()))),
                None => {
                    eprintln!(
                        "card-data-pipeline: enemy {} ({}): unresolved Spawn location {name:?} \
                         — emitting spawn: None",
                        c.code, c.name
                    );
                    resolutions.push((c.code.clone(), None));
                }
            }
        }
    }
    for (code, spawn_code) in resolutions {
        if let Some(c) = all.get_mut(&code) {
            c.spawn_code = spawn_code;
        }
    }
    // Warn on Prey lines we parsed but do not model yet.
    for c in all.values() {
        if let PreyParse::Unrecognized(clause) = &c.prey {
            eprintln!(
                "card-data-pipeline: enemy {} ({}): unmodeled Prey clause {clause:?} \
                 — emitting Prey::Default",
                c.code, c.name
            );
        }
    }
```
(The two-pass collect-then-set avoids a simultaneous `&` + `&mut` borrow of `all`.)

- [ ] **Step 9: Emit the new fields in `render_kind` + extend the generated `use` line**

In `render_kind`, replace the `"Enemy" =>` arm:
```rust
        "Enemy" => format!(
            "CardKind::Enemy {{ fight: {}, evade: {}, damage: {}, horror: {}, \
             health: {}, victory: {}, spawn: {}, surge: false, peril: false, \
             hunter: {}, retaliate: {}, prey: {}, quantity: {} }}",
            c.enemy_fight.unwrap_or(0),
            c.enemy_evade.unwrap_or(0),
            c.enemy_damage.unwrap_or(0),
            c.enemy_horror.unwrap_or(0),
            health_value_opt_lit(c.health, c.health_per_investigator),
            opt_u8(c.victory),
            spawn_lit(c.spawn_code.as_deref()),
            c.hunter,
            c.retaliate,
            prey_lit(&c.prey),
            c.quantity,
        ),
```
Add the `spawn_lit` helper near `health_value_opt_lit`:
```rust
/// Emit the `Option<Spawn>` literal for an enemy's resolved spawn code.
fn spawn_lit(spawn_code: Option<&str>) -> String {
    match spawn_code {
        Some(code) => format!(
            "Some(Spawn {{ location: SpawnLocation::Specific({}.to_owned()) }})",
            str_lit(code)
        ),
        None => "None".to_owned(),
    }
}
```
Extend the generated `use` line in `render` (~line 347) to:
```rust
        "use card_dsl::card_data::{CardKind, CardMetadata, Class, ClueValue, HealthValue, Prey, PreyDirection, PreyMeasure, SkillIcons, SkillKind, Skills, Slot, Spawn, SpawnLocation};\n\n\
```
Update the doc-comment on `render_kind` (~line 388) to drop the now-stale "`spawn`… emit not-yet-parsed defaults" sentence for enemies (keep the note for `surge`/`peril`).

- [ ] **Step 10: Fix the pipeline test assertion that pins the Enemy literal prefix**

In `crates/card-data-pipeline/src/main.rs` ~line 1020, the test mock sets `c.enemy_fight = Some(4)` and asserts `buf.contains("CardKind::Enemy { fight: 4, evade: 4,")`. That prefix is unchanged by our edits, so it still holds — but that mock `NormalizedCard` now needs the new fields to construct. Update every `NormalizedCard { … }` test literal in this file (there are several skeleton mocks ~lines 585, 621, 958, 1061) to add:
```rust
            health_per_investigator: false,
            hunter: false,
            retaliate: false,
            prey: PreyParse::None,
            spawn_name: None,
            spawn_code: None,
```
and add `health_per_investigator: None,` to any `RawCard { … }` test literal.

- [ ] **Step 11: Fix remaining hand-written `CardKind::Enemy` literals**

`crates/scenarios/src/test_fixtures/synth_cards.rs:92` and `crates/game-core/src/engine/dispatch/encounter.rs:968` (`synth_enemy_metadata`): change `health: Some(1)` → `health: Some(HealthValue::Fixed(1))` and add `hunter: false, retaliate: false, prey: Prey::Default,`. Add `HealthValue` / `Prey` to each file's imports (`encounter.rs` test module already imports from `card_dsl::card_data` at line 959 — extend it; `synth_cards.rs` imports near its top).

- [ ] **Step 12: Regenerate the corpus**

Run: `cargo run -p card-data-pipeline`
Expected: prints `wrote N cards`, plus warnings for out-of-scope unmodeled forms (e.g. Masked Hunter `Most clues` / `Engaged with Prey`). Confirm the six in-scope enemies look right:

Run: `grep -E 'code: "011(16|18|19|59|60|61)"' -A2 crates/cards/src/generated/cards.rs | grep -E 'CardKind::Enemy'`
Expected (spot-check): Ghoul Priest has `health: Some(HealthValue::PerInvestigator(5))`, `hunter: true`, `retaliate: true`, `prey: Prey::Ranked { direction: PreyDirection::Highest, measure: PreyMeasure::Skill(SkillKind::Combat) }`; Flesh-Eater has `spawn: Some(Spawn { location: SpawnLocation::Specific("01113".to_owned()) })`; Ravenous Ghoul has the `Lowest`/`RemainingHealth` prey.

- [ ] **Step 13: Build the workspace**

Run: `RUSTFLAGS="-D warnings" cargo build --all --all-features`
Expected: clean build (generated corpus + all literal sites compile against the new struct).

- [ ] **Step 14: Run the affected test suites**

Run: `cargo test -p card-dsl -p card-data-pipeline`
Expected: PASS.

- [ ] **Step 15: Commit**

```bash
git add crates/card-dsl crates/card-data-pipeline crates/scenarios crates/game-core/src/engine/dispatch/encounter.rs crates/cards/src/generated/cards.rs
git commit -m "$(cat <<'EOF'
card: enemy keywords + per-investigator health in the corpus (C3b)

Add hunter/retaliate/prey to CardKind::Enemy and replace
health: Option<u8> with Option<HealthValue> (mirrors ClueValue). The
pipeline parses these from card text and resolves Spawn-location names
to codes; regenerated corpus carries them for the six Gathering enemies.
Out-of-scope keyword forms default + warn.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: `spawn_enemy` reads stats + keywords from the corpus

**Files:**
- Modify: `crates/game-core/src/engine/dispatch/encounter.rs` (`spawn_enemy` ~line 245; `synth_enemy_metadata` + tests in `mod spawn_enemy_tests` ~line 954)

- [ ] **Step 1: Write failing tests**

In `mod spawn_enemy_tests`, first give the mock helper parameters for the new data. Replace `synth_enemy_metadata` with:

```rust
fn synth_enemy_metadata(spawn: Option<Spawn>) -> CardMetadata {
    enemy_metadata(spawn, HealthValue::Fixed(1), false, false, Prey::Default, 1, 1, 0, 0)
}

#[allow(clippy::too_many_arguments)]
fn enemy_metadata(
    spawn: Option<Spawn>,
    health: HealthValue,
    hunter: bool,
    retaliate: bool,
    prey: Prey,
    fight: u8,
    evade: u8,
    damage: u8,
    horror: u8,
) -> CardMetadata {
    CardMetadata {
        code: "_synth_enemy".into(),
        name: "Synth Enemy".into(),
        text: None,
        traits: Vec::new(),
        pack_code: "_synth".into(),
        kind: CardKind::Enemy {
            fight, evade, damage, horror,
            health: Some(health),
            victory: None,
            spawn,
            surge: false,
            peril: false,
            hunter,
            retaliate,
            prey,
            quantity: 1,
        },
    }
}
```
Extend the module's imports (line 959) to `use card_dsl::card_data::{CardKind, CardMetadata, HealthValue, Prey, Spawn, SpawnLocation};`.

Then add tests:

```rust
#[test]
fn spawn_enemy_reads_combat_stats_and_keywords_from_metadata() {
    let mut state = GameStateBuilder::new()
        .with_investigator(test_investigator(1))
        .with_location({ let mut l = test_location(10, "Loc"); l.code = CardCode("_l".into()); l })
        .with_turn_order([InvestigatorId(1)])
        .build();
    state.investigators.get_mut(&InvestigatorId(1)).unwrap().current_location = Some(LocationId(10));

    let metadata = enemy_metadata(None, HealthValue::Fixed(5), true, true, Prey::Default, 4, 4, 2, 2);
    let mut events = Vec::new();
    spawn_enemy(&mut Cx { state: &mut state, events: &mut events }, InvestigatorId(1), CardCode("_synth_enemy".into()), &metadata);

    let enemy = state.enemies.values().next().expect("enemy spawned");
    assert_eq!(enemy.fight, 4);
    assert_eq!(enemy.evade, 4);
    assert_eq!(enemy.attack_damage, 2);
    assert_eq!(enemy.attack_horror, 2);
    assert_eq!(enemy.max_health, 5);
    assert!(enemy.hunter);
    assert!(enemy.retaliate);
}

#[test]
fn spawn_enemy_scales_per_investigator_health_by_investigator_count() {
    let mut state = GameStateBuilder::new()
        .with_investigator(test_investigator(1))
        .with_investigator(test_investigator(2))
        .with_location({ let mut l = test_location(10, "Loc"); l.code = CardCode("_l".into()); l })
        .with_turn_order([InvestigatorId(1), InvestigatorId(2)])
        .build();
    for id in [1, 2] {
        state.investigators.get_mut(&InvestigatorId(id)).unwrap().current_location = Some(LocationId(10));
    }

    let metadata = enemy_metadata(None, HealthValue::PerInvestigator(5), false, false, Prey::Default, 4, 4, 2, 2);
    let mut events = Vec::new();
    spawn_enemy(&mut Cx { state: &mut state, events: &mut events }, InvestigatorId(1), CardCode("_synth_enemy".into()), &metadata);

    let enemy = state.enemies.values().next().expect("enemy spawned");
    assert_eq!(enemy.max_health, 10, "5 health × 2 investigators");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p game-core spawn_enemy`
Expected: FAIL — `enemy.fight` is `1` (hardcoded), `max_health` is `5` not `10`, `hunter`/`retaliate` are `false`.

- [ ] **Step 3: Implement — read stats from metadata in `spawn_enemy`**

In `spawn_enemy` (~line 253), widen the destructure and compute health:
```rust
    let CardKind::Enemy {
        spawn, health, fight, evade, damage, horror, hunter, retaliate, prey, ..
    } = &metadata.kind
    else {
        return EngineOutcome::Rejected {
            reason: format!("spawn_enemy: card {code} is not an enemy").into(),
        };
    };
```
Compute `max_health` from `HealthValue` using the same investigator-count source as per-investigator clues (`reveal.rs:20`):
```rust
    // Resolve health. PerInvestigator scales by the number of
    // investigators in the game (Rules Reference p.12); matches the
    // per-investigator clue path in reveal.rs (its future
    // started-count caveat applies equally here).
    let max_health = match health {
        Some(HealthValue::Fixed(n)) => *n,
        Some(HealthValue::PerInvestigator(n)) => {
            let count = u8::try_from(cx.state.investigators.len()).unwrap_or(u8::MAX);
            n.saturating_mul(count)
        }
        None => 1,
    };
    let prey = *prey;
```
Then in the `Enemy { … }` literal (~line 309) replace the hardcoded fields:
```rust
        fight: i8::try_from(*fight).unwrap_or(i8::MAX),
        evade: i8::try_from(*evade).unwrap_or(i8::MAX),
        max_health,
        damage: 0,
        attack_damage: *damage,
        attack_horror: *horror,
        ...
        hunter: *hunter,
        prey,
        retaliate: *retaliate,
```
(`fight`/`evade` on `Enemy` are `i8`; metadata is `u8` — convert. `damage` here is the *current* damage suffered, stays `0`; the printed attack damage goes to `attack_damage`.)

- [ ] **Step 4: Use the enemy's prey for spawn-engagement narrowing**

The `resolve_prey` call (~line 329) currently passes `crate::card_data::Prey::Default`. Change it to the enemy's `prey`:
```rust
    match super::hunters::resolve_prey(cx.state, prey, &candidates) {
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p game-core spawn_enemy`
Expected: PASS (new tests + the pre-existing spawn tests).

- [ ] **Step 6: Commit**

```bash
git add crates/game-core/src/engine/dispatch/encounter.rs
git commit -m "$(cat <<'EOF'
engine: spawn_enemy reads stats + keywords from the corpus (C3b)

Replace the hardcoded fight/evade/attack/health/keyword placeholders
with the CardKind::Enemy values; scale PerInvestigator health by the
in-game investigator count; engage on spawn using the enemy's prey.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: Integration test — the six enemies against the real corpus

Realizes the issue's "card test per enemy" acceptance: with `cards::REGISTRY` installed, assert each enemy's parsed corpus metadata. (Full in-engine spawn-to-defeat is the C7b end-to-end test, #245.)

**Files:**
- Create: `crates/cards/tests/enemies.rs`

- [ ] **Step 1: Write the test**

```rust
//! C3b — the six Gathering encounter enemies carry their printed stats,
//! keywords, and spawn-location in the corpus.

use card_dsl::card_data::{
    CardKind, HealthValue, Prey, PreyDirection, PreyMeasure, SkillKind, Spawn, SpawnLocation,
};

fn enemy(code: &str) -> CardKind {
    cards::by_code(code)
        .unwrap_or_else(|| panic!("enemy {code} in corpus"))
        .kind
        .clone()
}

#[test]
fn ghoul_priest_full_profile() {
    let CardKind::Enemy { fight, evade, damage, horror, health, victory, hunter, retaliate, prey, .. } = enemy("01116")
    else { panic!("01116 is an enemy") };
    assert_eq!((fight, evade, damage, horror), (4, 4, 2, 2));
    assert_eq!(health, Some(HealthValue::PerInvestigator(5)));
    assert_eq!(victory, Some(2));
    assert!(hunter);
    assert!(retaliate);
    assert_eq!(prey, Prey::Ranked { direction: PreyDirection::Highest, measure: PreyMeasure::Skill(SkillKind::Combat) });
}

#[test]
fn flesh_eater_spawns_at_attic() {
    let CardKind::Enemy { fight, evade, damage, horror, health, spawn, .. } = enemy("01118")
    else { panic!("enemy") };
    assert_eq!((fight, evade, damage, horror), (4, 1, 1, 2));
    assert_eq!(health, Some(HealthValue::Fixed(4)));
    assert_eq!(spawn, Some(Spawn { location: SpawnLocation::Specific("01113".to_owned()) }));
}

#[test]
fn icy_ghoul_spawns_at_cellar() {
    let CardKind::Enemy { fight, evade, damage, horror, spawn, .. } = enemy("01119")
    else { panic!("enemy") };
    assert_eq!((fight, evade, damage, horror), (3, 4, 2, 1));
    assert_eq!(spawn, Some(Spawn { location: SpawnLocation::Specific("01114".to_owned()) }));
}

#[test]
fn ghoul_minion_is_plain() {
    let CardKind::Enemy { fight, evade, damage, horror, health, hunter, retaliate, prey, quantity, .. } = enemy("01160")
    else { panic!("enemy") };
    assert_eq!((fight, evade, damage, horror), (2, 2, 1, 1));
    assert_eq!(health, Some(HealthValue::Fixed(2)));
    assert!(!hunter);
    assert!(!retaliate);
    assert_eq!(prey, Prey::Default);
    assert_eq!(quantity, 3);
}

#[test]
fn ravenous_ghoul_prey_lowest_remaining_health() {
    let CardKind::Enemy { fight, evade, damage, horror, prey, .. } = enemy("01161")
    else { panic!("enemy") };
    assert_eq!((fight, evade, damage, horror), (3, 3, 1, 1));
    assert_eq!(prey, Prey::Ranked { direction: PreyDirection::Lowest, measure: PreyMeasure::RemainingHealth });
}

#[test]
fn swarm_of_rats_is_a_hunter() {
    let CardKind::Enemy { fight, evade, damage, horror, hunter, quantity, .. } = enemy("01159")
    else { panic!("enemy") };
    assert_eq!((fight, evade, damage, horror), (1, 3, 1, 0));
    assert!(hunter);
    assert_eq!(quantity, 3);
}
```

Note: confirm the corpus accessor name — if `cards::by_code` does not exist, use the one the other integration tests use (check `crates/cards/tests/play_card.rs` for the metadata accessor and `REGISTRY` install pattern, and mirror it). If a `REGISTRY` install is required for the accessor, add it; if `by_code` reads the static corpus directly, no install is needed.

- [ ] **Step 2: Run the test to verify it passes**

Run: `cargo test -p cards --test enemies`
Expected: PASS (the corpus was regenerated in Task 2).

- [ ] **Step 3: Commit**

```bash
git add crates/cards/tests/enemies.rs
git commit -m "$(cat <<'EOF'
test: corpus profile checks for the six Gathering enemies (C3b)

Assert stats, keywords, per-investigator health, and spawn-location for
Ghoul Priest, Flesh-Eater, Icy Ghoul, Ghoul Minion, Ravenous Ghoul, and
Swarm of Rats against the regenerated corpus.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: Full CI gauntlet + PR

- [ ] **Step 1: Run the full local gauntlet** (from CLAUDE.md)

```bash
RUSTFLAGS="-D warnings"     cargo test --all --all-features
                            cargo clippy --all-targets --all-features -- -D warnings
                            cargo fmt --check
RUSTDOCFLAGS="-D warnings"  cargo doc --workspace --no-deps --all-features
                            cargo build -p web --target wasm32-unknown-unknown
                            cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings
```
Expected: all green. Fix any clippy/doc issues before pushing (e.g. `too_many_arguments` on the test helper is already `#[allow]`'d; check the generated corpus has no unused imports — every added `use` type must appear in an emitted literal).

- [ ] **Step 2: Push and open the PR**

```bash
git push -u origin card/c3b-gathering-enemies
gh pr create --fill
```
PR body: design-decisions paragraph (keyword/spawn/health now parsed in the pipeline into the corpus; `HealthValue` mirrors `ClueValue`; out-of-scope forms default + warn), and `Closes #231.`

- [ ] **Step 3: Watch CI**

Run: `gh pr checks <PR#> --watch`
Fix failures with follow-up commits to the same branch.

- [ ] **Step 4: Update the phase doc — ONLY after CI is green**

Per `docs/phases/README.md`: in `docs/phases/phase-7-the-gathering.md`, flip the C3b row (#231) to `✅ PR #<n>`, update the Status "Shipped:" line, and add a **Decisions made** entry only if load-bearing for future PRs (e.g. "enemy keywords/spawn/per-inv-health are parsed in the pipeline into `CardKind::Enemy`; `HealthValue` mirrors `ClueValue`; future enemies need no hand-written impl"). Commit as the final commit on the branch. Do **not** merge — stop for user approval.

---

## Self-review notes

- **Spec coverage:** card-dsl fields (T2) · pipeline parsing + corpus regen (T1, T2) · per-investigator `HealthValue` mirroring `ClueValue` (T1, T2) · `spawn_enemy` reads stats/keywords + scales health + uses prey (T3) · out-of-scope default+warn (T2 step 8) · pipeline/spawn/integration tests (T1, T3, T4) · all six enemies verified (T4). Covered.
- **i8/u8:** `Enemy.fight/evade` are `i8`; metadata `u8` — converted in T3 step 3.
- **Atomicity:** the `CardKind::Enemy` change forces a corpus regen; T2 bundles struct + pipeline + regen + literal fixes so every commit compiles.
- **Accessor name (`cards::by_code`)** is verified against `crates/cards/tests/play_card.rs` in T4 step 1 before relying on it.
