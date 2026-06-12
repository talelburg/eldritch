# Encounter-card Ingestion Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ingest the in-scope encounter cards (locations/acts/agendas/enemies/treacheries/story-assets) into the corpus with their printed stats, and de-hardcode `the_gathering::setup()` to read them.

**Architecture:** Add `Location`/`Act`/`Agenda` `CardKind` variants + `Enemy` combat stats (card-dsl), then teach the pipeline to ingest the 8 encounter files and emit the new shape (skipping `scenario`-type cards), regenerate the corpus, and migrate `the_gathering::setup()` to read stats via `cards::by_code`. The variant + Enemy-field change is atomic (breaks the corpus + Enemy mocks until regen), so the bulk lands in one green commit.

**Tech Stack:** Rust workspace (`card-dsl` ← `game-core` ← `cards`/`scenarios`; `card-data-pipeline` generates `cards/src/generated/cards.rs`).

**Spec:** `docs/superpowers/specs/2026-06-12-encounter-card-ingestion-design.md`
**Issue:** #252

---

## File Structure

- `crates/card-dsl/src/card_data.rs` — `CardKind` gains `Location`/`Act`/`Agenda`; `Enemy` gains `fight/evade/damage/horror/victory`; `card_type()`/`class()` arms.
- `crates/card-data-pipeline/src/main.rs` — `PACK_FILES` + `RawCard`/`NormalizedCard` fields + `process_raw` scenario-skip + `render_kind` arms + tests.
- `crates/cards/src/generated/cards.rs` — regenerated.
- `crates/scenarios/src/test_fixtures/synth_cards.rs`, `crates/game-core/src/engine/dispatch/encounter.rs` — Enemy mock literals gain the new fields.
- `crates/scenarios/src/the_gathering.rs` — read Study/act/agenda stats via `cards::by_code`.

---

## Task 1: `CardKind` — new variants + `Enemy` combat stats (card-dsl)

**Files:** `crates/card-dsl/src/card_data.rs`

- [ ] **Step 1: Extend `Enemy` and add the three variants**

In the `pub enum CardKind`, replace the `Enemy` variant and add three variants after `Treachery`:

```rust
    /// Enemy — an encounter (or weakness) creature.
    Enemy {
        /// Fight (combat difficulty).
        fight: u8,
        /// Evade difficulty.
        evade: u8,
        /// Damage dealt to an investigator on attack.
        damage: u8,
        /// Horror dealt to an investigator on attack.
        horror: u8,
        /// Maximum health.
        health: Option<u8>,
        /// Victory points when defeated (in the victory display).
        victory: Option<u8>,
        /// Spawn rule (`None` = default: engaged with the drawing investigator).
        spawn: Option<Spawn>,
        /// Surge keyword.
        surge: bool,
        /// Peril keyword.
        peril: bool,
        /// Copies in the encounter deck (build multiplicity).
        quantity: u8,
    },
    /// Location — a place investigators move between and investigate.
    Location {
        /// Shroud (investigate difficulty).
        shroud: u8,
        /// Clues placed at scenario setup.
        clues: u8,
        /// Victory points when in the victory display.
        victory: Option<u8>,
    },
    /// Act — the investigators' side of the act/agenda deck.
    Act {
        /// Clues the group spends to advance, or `None` for acts that
        /// advance on a non-clue objective.
        clue_threshold: Option<u8>,
        /// Victory points, if any.
        victory: Option<u8>,
    },
    /// Agenda — the doom side of the act/agenda deck.
    Agenda {
        /// Doom in play required to advance.
        doom_threshold: u8,
    },
```

- [ ] **Step 2: Add `card_type()` / `class()` arms**

In `card_type()` add:

```rust
            CardKind::Location { .. } => CardType::Location,
            CardKind::Act { .. } => CardType::Act,
            CardKind::Agenda { .. } => CardType::Agenda,
```

In `class()`, extend the encounter-`None` arm:

```rust
            CardKind::Enemy { .. }
            | CardKind::Treachery { .. }
            | CardKind::Location { .. }
            | CardKind::Act { .. }
            | CardKind::Agenda { .. } => None,
```

- [ ] **Step 3: Fix the existing `Enemy` mock in card_data.rs tests**

The `card_metadata_serde_roundtrip_preserves_spawn_specific` test builds a `CardKind::Enemy { health, spawn, surge, peril, quantity }` — add the new fields:

```rust
            kind: CardKind::Enemy {
                fight: 3,
                evade: 2,
                damage: 1,
                horror: 1,
                health: Some(1),
                victory: None,
                spawn: Some(Spawn {
                    location: SpawnLocation::Specific("_synth_loc".into()),
                }),
                surge: false,
                peril: false,
                quantity: 1,
            },
```

Add an accessor test:

```rust
#[test]
fn new_encounter_kinds_have_no_class_and_right_type() {
    let loc = CardMetadata {
        code: "01111".into(), name: "Study".into(), traits: vec![], text: None,
        pack_code: "core".into(),
        kind: CardKind::Location { shroud: 2, clues: 2, victory: None },
    };
    assert_eq!(loc.card_type(), CardType::Location);
    assert_eq!(loc.class(), None);
}
```

(Put it in the `is_fast_tests` module, which has `use super::*`.)

- [ ] **Step 4: Verify card-dsl compiles + tests pass in isolation**

Run: `cargo test -p card-dsl`
Expected: PASS. (The workspace does NOT yet compile — the generated corpus + Enemy mocks still use the old `Enemy` shape; that's fixed in Task 2.)

- [ ] **Step 5: Commit**

```bash
git add crates/card-dsl/src/card_data.rs
git commit -m "card-dsl: add Location/Act/Agenda CardKind variants + Enemy combat stats"
# end with: Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
```

---

## Task 2: Pipeline ingestion + corpus regen + Enemy mocks (atomic green)

**Files:** `crates/card-data-pipeline/src/main.rs`, regenerated `cards.rs`, the two Enemy mock sites.

- [ ] **Step 1: Add the encounter files to `PACK_FILES`**

Append to the `PACK_FILES` array (after the existing player files):

```rust
    "pack/core/core_encounter.json",
    "pack/dwl/dwl_encounter.json",
    "pack/dwl/tmm_encounter.json",
    "pack/dwl/tece_encounter.json",
    "pack/dwl/bota_encounter.json",
    "pack/dwl/uau_encounter.json",
    "pack/dwl/wda_encounter.json",
    "pack/dwl/litas_encounter.json",
```

Update the `PACK_FILES` doc comment and the module-level doc (lines ~22-24) — they currently say encounter companions are skipped; note they're now ingested (encounter `scenario`-type cards excepted).

- [ ] **Step 2: Add the raw + normalized stat fields**

In `struct RawCard` add (all `Option`):

```rust
    shroud: Option<u8>,
    clues: Option<u8>,
    victory: Option<u8>,
    doom: Option<u8>,
    enemy_fight: Option<u8>,
    enemy_evade: Option<u8>,
    enemy_damage: Option<u8>,
    enemy_horror: Option<u8>,
```

In `struct NormalizedCard` add the same eight `Option<u8>` fields. In `normalize`'s `Ok(NormalizedCard { … })`, pass each straight through (`shroud: raw.shroud,` … `enemy_horror: raw.enemy_horror,`).

- [ ] **Step 3: Skip `scenario`/`story` cards in `process_raw`**

In `process_raw`, after the skeleton-skip, add:

```rust
    // Encounter `scenario` / `story` cards have no CardKind variant
    // (e.g. the Gathering reference card 01104 — its symbol effects live
    // in an abilities() impl, not metadata). Skip them like skeletons.
    if matches!(raw.type_code.as_deref(), Some("scenario" | "story")) {
        return Ok(());
    }
```

- [ ] **Step 4: Add the `render_kind` arms**

In `render_kind`'s `match c.card_type`, add before the `other =>` arm, and replace the existing `"Enemy"` arm:

```rust
        "Enemy" => format!(
            "CardKind::Enemy {{ fight: {}, evade: {}, damage: {}, horror: {}, \
             health: {}, victory: {}, spawn: None, surge: false, peril: false, quantity: {} }}",
            c.enemy_fight.unwrap_or(0),
            c.enemy_evade.unwrap_or(0),
            c.enemy_damage.unwrap_or(0),
            c.enemy_horror.unwrap_or(0),
            opt_u8(c.health),
            opt_u8(c.victory),
            c.quantity,
        ),
        "Location" => format!(
            "CardKind::Location {{ shroud: {}, clues: {}, victory: {} }}",
            c.shroud.unwrap_or(0),
            c.clues.unwrap_or(0),
            opt_u8(c.victory),
        ),
        "Act" => format!(
            "CardKind::Act {{ clue_threshold: {}, victory: {} }}",
            opt_u8(c.clues),
            opt_u8(c.victory),
        ),
        "Agenda" => format!(
            "CardKind::Agenda {{ doom_threshold: {} }}",
            c.doom.unwrap_or(0),
        ),
```

Update the `other =>` panic message to drop the "land in #252" note (now: "no CardKind variant — `scenario`/`story` are skipped upstream").

- [ ] **Step 5: Update the pipeline test literals for the new fields**

`RawCard` is built in `raw_card` (helper) and two fast-detection tests; `NormalizedCard` in `emitted_treachery_renders_treachery_kind`. Add the eight new `Option<u8>` fields, all `None`, to each literal. Then add two pipeline tests:

```rust
#[test]
fn emitted_location_renders_location_kind() {
    let mut c = normalized("01111", "Study", "Location");
    c.shroud = Some(2);
    c.clues = Some(2);
    let mut buf = String::new();
    emit_card(&mut buf, &c);
    assert!(buf.contains("CardKind::Location { shroud: 2, clues: 2"), "got:\n{buf}");
}

#[test]
fn scenario_type_card_is_skipped() {
    let mut raw = raw_card("01104");
    raw.type_code = Some("scenario".to_owned());
    let mut all = BTreeMap::new();
    process_raw(raw, &mut all, Path::new("fixture.json")).expect("scenario skip is not an error");
    assert!(all.is_empty(), "scenario-type cards are skipped");
}
```

Add a `normalized(code, name, card_type)` test helper next to `raw_card` that builds a `NormalizedCard` with all-default fields (zeros / `None` / empty), so the new test and any future ones don't repeat the full literal:

```rust
fn normalized(code: &str, name: &str, card_type: &'static str) -> NormalizedCard {
    NormalizedCard {
        code: code.into(), name: name.into(), class: "Mythos", card_type,
        cost: None, xp: None, text: None, traits: Vec::new(), slots: Vec::new(),
        skill_willpower: 0, skill_intellect: 0, skill_combat: 0, skill_agility: 0, skill_wild: 0,
        health: None, sanity: None, deck_limit: 0, quantity: 1, pack_code: "core".into(),
        is_fast: false,
        shroud: None, clues: None, victory: None, doom: None,
        enemy_fight: None, enemy_evade: None, enemy_damage: None, enemy_horror: None,
    }
}
```

Refactor `emitted_treachery_renders_treachery_kind` to use `normalized(...)` too (DRY).

Run: `cargo test -p card-data-pipeline`
Expected: PASS.

- [ ] **Step 6: Regenerate the corpus**

Run: `cargo run -p card-data-pipeline`
Expected: prints a card count ~500 (was 216 + ~273 ingested non-scenario encounter cards). If it errors with `duplicate card code: …`, a code appears in both a player and an encounter file — STOP and report the code (unexpected; needs a decision).

Run: `cargo build -p cards`
Expected: the regenerated corpus compiles against the new variants.

- [ ] **Step 7: Fix the two `Enemy` mock literals**

`crates/scenarios/src/test_fixtures/synth_cards.rs` (`synth_enemy_metadata`) and `crates/game-core/src/engine/dispatch/encounter.rs` (test `synth_enemy_metadata`) build `CardKind::Enemy { health, spawn, surge, peril, quantity }`. Add the new fields to each:

```rust
        kind: CardKind::Enemy {
            fight: 1,
            evade: 1,
            damage: 0,
            horror: 0,
            health: Some(1),
            victory: None,
            spawn: /* existing */,
            surge: false,
            peril: false,
            quantity: 1,
        },
```

(Keep each mock's existing `health`/`spawn` values; the encounter.rs one takes `spawn` as a param.)

- [ ] **Step 8: Full strict gauntlet**

```bash
RUSTFLAGS="-D warnings" cargo test --all --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --all && cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
cargo build -p web --target wasm32-unknown-unknown
cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings
```

Expected: green. (Existing engine behavior unchanged — the 3 player-file enemies now carry real combat stats from metadata, but nothing reads fight/evade from metadata yet; spawn still emits `None`.)

- [ ] **Step 9: Commit**

```bash
git add -A
git commit -m "card-data: ingest encounter cards (locations/acts/agendas/enemies/treacheries)"
# body: 8 encounter files into PACK_FILES; scenario/story skipped; corpus regenerated.
```

---

## Task 3: `the_gathering::setup()` reads stats from the corpus

**Files:** `crates/scenarios/src/the_gathering.rs`

- [ ] **Step 1: Add corpus-reading helpers + migrate setup()**

At the top of `the_gathering.rs` add `use game_core::card_data::CardKind;` (and keep the existing imports). Add these helpers (module-private):

```rust
/// Read a location's printed `(shroud, clues)` from the corpus. The code
/// is a build-time invariant of the generated corpus, so a miss is a bug.
fn location_stats(code: &str) -> (u8, u8) {
    match cards::by_code(code).expect("location code in corpus").kind {
        CardKind::Location { shroud, clues, .. } => (shroud, clues),
        ref k => panic!("{code} is not a Location ({k:?})"),
    }
}

/// Read an agenda's printed doom threshold from the corpus.
fn agenda_doom(code: &str) -> u8 {
    match cards::by_code(code).expect("agenda code in corpus").kind {
        CardKind::Agenda { doom_threshold } => doom_threshold,
        ref k => panic!("{code} is not an Agenda ({k:?})"),
    }
}

/// Read an act's printed clue threshold from the corpus, falling back to
/// `placeholder` for acts that advance on a non-clue objective (`01110`,
/// "Ghoul Priest defeated" — C1b owns that).
fn act_clue_threshold(code: &str, placeholder: u8) -> u8 {
    match cards::by_code(code).expect("act code in corpus").kind {
        CardKind::Act { clue_threshold, .. } => clue_threshold.unwrap_or(placeholder),
        ref k => panic!("{code} is not an Act ({k:?})"),
    }
}
```

In `setup()`, replace the hardcoded Study build:

```rust
    let (study_shroud, study_clues) = location_stats("01111");
    let study = Location::new(STUDY_ID, CardCode("01111".into()), "Study", study_shroud, study_clues);
```

Replace the `act_deck` thresholds: `clue_threshold: act_clue_threshold("01108", 0)`, `act_clue_threshold("01109", 0)`, and for `01110` `act_clue_threshold("01110", 2)` (the `2` is the C1b placeholder, since `01110`'s metadata threshold is `None`). Replace the `agenda_deck` doom thresholds with `doom_threshold: agenda_doom("01105")`, `agenda_doom("01106")`, `agenda_doom("01107")`. Drop the now-inaccurate "real printed values" comments that hardcoded the numbers; keep the note that `01110` is the C1b placeholder.

- [ ] **Step 2: Add a test that the reads match the snapshot**

In `the_gathering::tests`, add:

```rust
#[test]
fn setup_reads_card_stats_from_corpus() {
    let s = setup();
    let study = s.locations.get(&STUDY_ID).unwrap();
    assert_eq!((study.shroud, study.clues), (2, 2), "Study 01111 stats");
    assert_eq!(
        s.agenda_deck.iter().map(|a| a.doom_threshold).collect::<Vec<_>>(),
        [3, 7, 10],
        "agenda doom thresholds from corpus",
    );
    assert_eq!(s.act_deck[0].clue_threshold, 2, "act 01108 from corpus");
    assert_eq!(s.act_deck[1].clue_threshold, 3, "act 01109 from corpus");
}
```

The existing `setup_*` tests keep their literal assertions (2/3, 3/7/10) — they now double as a check that the corpus values match. The C1a `tests/the_gathering.rs` integration tests must still pass unchanged.

- [ ] **Step 3: Run the scenario tests**

Run: `cargo test -p scenarios the_gathering`
Expected: PASS (the corpus values equal the previously-hardcoded ones).

- [ ] **Step 4: Commit**

```bash
git add crates/scenarios/src/the_gathering.rs
git commit -m "scenario: the-gathering reads Study/act/agenda stats from the corpus"
# body: Closes #252.
```

---

## Task 4: Gauntlet + PR

- [ ] **Step 1:** Re-run the full strict gauntlet (Task 2 Step 8 commands) — green.
- [ ] **Step 2:** Push `infra/encounter-ingestion`; open the PR (`Closes #252`); watch `gh pr checks <PR#> --watch`.
- [ ] **Step 3:** No phase-doc update — `#252` is unmilestoned `[infra]`, not a phase issue.

---

## Self-Review

**Spec coverage:**
- Location/Act/Agenda variants + Enemy combat stats → Task 1. ✓
- 8 encounter files into PACK_FILES; skip `scenario` → Task 2 Steps 1, 3. ✓
- Parse new fields + render arms → Task 2 Steps 2, 4. ✓
- Corpus regen → Task 2 Step 6. ✓
- the_gathering reads via `cards::by_code` (01111 shroud/clues, acts, agendas; 01110 placeholder) → Task 3. ✓
- Deferred (encounter_code, deck filter) → not touched. ✓
- Tests (pipeline render arms, scenario skip, by_code spot checks, the_gathering reads) → Tasks 2, 3. ✓

**Placeholder scan:** No "TBD". The `01110` placeholder `2` is an explicit, commented C1b stand-in (its metadata threshold is genuinely `None`).

**Type consistency:** `CardKind::{Enemy,Location,Act,Agenda}` field names match between the variant defs (Task 1), `render_kind` (Task 2 Step 4), the mocks (Task 2 Step 7), and the_gathering matches (Task 3). `clues` is a single `Option<u8>` on `NormalizedCard` serving both Location (`clues`) and Act (`clue_threshold`). `opt_u8` is the existing helper for `Option<u8>`.

**Atomicity note:** Task 1 leaves the workspace non-compiling (corpus + Enemy mocks); it goes green at Task 2 Step 8. Same shape as the #254 remodel. Task 3 is an independent consumer migration.
