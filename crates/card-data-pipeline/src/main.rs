//! Card-data ingestion CLI.
//!
//! Reads the pinned `ArkhamDB` snapshot at `data/arkhamdb-snapshot/`,
//! normalizes it into Eldritch's card-metadata shape, and emits Rust
//! source at `crates/cards/src/generated/cards.rs`.
//!
//! # Determinism
//!
//! Same snapshot input must produce byte-identical output every run.
//! That means: cards are sorted by code; field order in generated
//! source is fixed; no timestamps or other run-dependent data leaks
//! into the output.
//!
//! # Run
//!
//! ```sh
//! cargo run -p card-data-pipeline
//! ```
//!
//! Inputs (relative to repo root): the Core + Dunwich player files and
//! their `*_encounter.json` companions (see [`PACK_FILES`]). Encounter
//! `scenario`/`story`-type cards are skipped (no `CardKind` variant).
//!
//! Output:
//! - `crates/cards/src/generated/cards.rs`

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use serde::Deserialize;

const SNAPSHOT_DIR: &str = "data/arkhamdb-snapshot";
const OUTPUT_PATH: &str = "crates/cards/src/generated/cards.rs";

/// Pack files we read: the player files plus their `*_encounter.json`
/// companions (the in-scope Core + Dunwich snapshot). Encounter
/// `scenario`/`story`-type cards have no `CardKind` variant and are
/// skipped in `process_raw`; everything else (locations/acts/agendas/
/// enemies/treacheries/story-assets) is ingested.
const PACK_FILES: &[&str] = &[
    "pack/core/core.json",
    "pack/dwl/dwl.json",
    "pack/dwl/tmm.json",
    "pack/dwl/tece.json",
    "pack/dwl/bota.json",
    "pack/dwl/uau.json",
    "pack/dwl/wda.json",
    "pack/dwl/litas.json",
    "pack/core/core_encounter.json",
    "pack/dwl/dwl_encounter.json",
    "pack/dwl/tmm_encounter.json",
    "pack/dwl/tece_encounter.json",
    "pack/dwl/bota_encounter.json",
    "pack/dwl/uau_encounter.json",
    "pack/dwl/wda_encounter.json",
    "pack/dwl/litas_encounter.json",
];

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("card-data-pipeline: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let repo_root = repo_root()?;
    let snapshot = repo_root.join(SNAPSHOT_DIR);
    let mut all: BTreeMap<String, NormalizedCard> = BTreeMap::new();

    for rel in PACK_FILES {
        let path = snapshot.join(rel);
        let raw = std::fs::read_to_string(&path)
            .map_err(|e| format!("reading {}: {e}", path.display()))?;
        let cards: Vec<RawCard> =
            serde_json::from_str(&raw).map_err(|e| format!("parsing {}: {e}", path.display()))?;
        for raw in cards {
            process_raw(raw, &mut all, &path)?;
        }
    }

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
            if let Some(code) = loc_index.get(name) {
                resolutions.push((c.code.clone(), Some(code.clone())));
            } else {
                eprintln!(
                    "card-data-pipeline: enemy {} ({}): unresolved Spawn location {name:?} \
                     — emitting spawn: None",
                    c.code, c.name
                );
                resolutions.push((c.code.clone(), None));
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

    let output = render(&all);
    let out_path = repo_root.join(OUTPUT_PATH);
    std::fs::write(&out_path, output)
        .map_err(|e| format!("writing {}: {e}", out_path.display()))?;

    eprintln!(
        "card-data-pipeline: wrote {} cards to {}",
        all.len(),
        out_path.display()
    );
    Ok(())
}

/// Per-card pipeline step: skip skeleton entries (no `name`),
/// normalize, then insert deduplicated into the accumulator. The
/// `path` is purely for error-context formatting on `normalize`
/// failures.
fn process_raw(
    raw: RawCard,
    all: &mut BTreeMap<String, NormalizedCard>,
    path: &Path,
) -> Result<(), String> {
    // Skip skeleton entries (codes reserved upstream but no populated
    // card data — name is the cheap presence check).
    if raw.name.is_none() {
        return Ok(());
    }
    // Encounter `scenario` / `story` cards have no `CardKind` variant
    // (e.g. the Gathering reference card 01104 — its symbol effects live
    // in an abilities() impl, not metadata). Skip them like skeletons.
    if matches!(raw.type_code.as_deref(), Some("scenario" | "story")) {
        return Ok(());
    }
    let normalized =
        normalize(raw).map_err(|e| format!("normalizing card in {}: {e}", path.display()))?;
    if let Some(prev) = all.insert(normalized.code.clone(), normalized) {
        return Err(format!("duplicate card code: {}", prev.code));
    }
    Ok(())
}

fn repo_root() -> Result<PathBuf, String> {
    // The pipeline is meant to be run via `cargo run -p
    // card-data-pipeline` from the workspace root. We trust
    // `CARGO_MANIFEST_DIR` to point at the binary's own crate
    // directory; the repo root is two levels up from there.
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .map_err(|_| "CARGO_MANIFEST_DIR is not set; run via `cargo run -p card-data-pipeline`")?;
    Path::new(&manifest_dir)
        .ancestors()
        .nth(2)
        .map(Path::to_path_buf)
        .ok_or_else(|| "could not derive repo root from CARGO_MANIFEST_DIR".into())
}

// ---- upstream JSON schema (only the fields we consume) ------------

#[derive(Debug, Deserialize)]
struct RawCard {
    code: String,
    name: Option<String>,
    text: Option<String>,
    traits: Option<String>,
    slot: Option<String>,
    cost: Option<i32>,
    xp: Option<i32>,
    health: Option<u8>,
    sanity: Option<u8>,
    deck_limit: Option<u8>,
    quantity: Option<u8>,
    pack_code: String,
    faction_code: Option<String>,
    type_code: Option<String>,
    skill_willpower: Option<u8>,
    skill_intellect: Option<u8>,
    skill_combat: Option<u8>,
    skill_agility: Option<u8>,
    skill_wild: Option<u8>,
    // Encounter-card stats (locations / acts / agendas / enemies).
    shroud: Option<u8>,
    clues: Option<u8>,
    clues_fixed: Option<bool>,
    victory: Option<u8>,
    doom: Option<u8>,
    enemy_fight: Option<u8>,
    enemy_evade: Option<u8>,
    enemy_damage: Option<u8>,
    enemy_horror: Option<u8>,
    health_per_investigator: Option<bool>,
}

// ---- normalized shape we emit -----------------------------------

#[derive(Debug)]
#[allow(clippy::struct_excessive_bools)] // mirrors ArkhamDB's flag fields
struct NormalizedCard {
    code: String,
    name: String,
    class: &'static str,
    card_type: &'static str,
    cost: Option<i8>,
    xp: Option<u8>,
    text: Option<String>,
    traits: Vec<String>,
    slots: Vec<&'static str>,
    skill_willpower: u8,
    skill_intellect: u8,
    skill_combat: u8,
    skill_agility: u8,
    skill_wild: u8,
    health: Option<u8>,
    sanity: Option<u8>,
    deck_limit: u8,
    quantity: u8,
    pack_code: String,
    is_fast: bool,
    /// Limited-use tokens parsed from a `Uses (N <kind>)` clause, as
    /// `(count, UsesKind-variant-name, discard_when_empty)`. `None` when absent
    /// or unmodeled.
    uses: Option<(u8, &'static str, bool)>,
    /// "Max N committed per skill test" cap, parsed from text. `None` when
    /// uncapped. Skill-only in scope.
    commit_limit: Option<u8>,
    // Encounter-card stats. `clues` is a location's starting clues AND an
    // act's advance threshold (same JSON field); consumer interprets by kind.
    shroud: Option<u8>,
    clues: Option<u8>,
    clues_fixed: bool,
    victory: Option<u8>,
    doom: Option<u8>,
    enemy_fight: Option<u8>,
    enemy_evade: Option<u8>,
    enemy_damage: Option<u8>,
    enemy_horror: Option<u8>,
    health_per_investigator: bool,
    hunter: bool,
    retaliate: bool,
    prey: PreyParse,
    /// Location name parsed from a `Spawn - <name>.` line (pre-resolution).
    spawn_name: Option<String>,
    /// Spawn location code, resolved from `spawn_name` after all cards load.
    spawn_code: Option<String>,
}

fn normalize(raw: RawCard) -> Result<NormalizedCard, String> {
    let name = raw.name.ok_or("missing name")?;
    let class = map_class(raw.faction_code.as_deref(), &raw.code)?;
    let card_type = map_card_type(raw.type_code.as_deref(), &raw.code)?;
    let cost = raw
        .cost
        .map(|n| {
            i8::try_from(n).map_err(|_| format!("cost {n} on card {} doesn't fit in i8", raw.code))
        })
        .transpose()?;

    let is_fast = raw
        .text
        .as_deref()
        .is_some_and(|t| t.starts_with("Fast.") || t.starts_with("Fast "));

    // Enemy keyword/prey/spawn data parsed from card text (read before
    // `raw.text` is moved into the struct, mirroring `is_fast`).
    let hunter = raw
        .text
        .as_deref()
        .is_some_and(|t| has_keyword(t, "Hunter"));
    let retaliate = raw
        .text
        .as_deref()
        .is_some_and(|t| has_keyword(t, "Retaliate"));
    let prey = raw.text.as_deref().map_or(PreyParse::None, parse_prey);
    let spawn_name = raw.text.as_deref().and_then(parse_spawn_name);
    let uses = raw.text.as_deref().and_then(parse_uses);
    let commit_limit = raw.text.as_deref().and_then(parse_commit_limit);

    Ok(NormalizedCard {
        code: raw.code,
        name,
        class,
        card_type,
        cost,
        xp: raw.xp.and_then(|n| u8::try_from(n).ok()),
        text: raw.text,
        traits: parse_traits(raw.traits.as_deref()),
        slots: parse_slots(raw.slot.as_deref()),
        skill_willpower: raw.skill_willpower.unwrap_or(0),
        skill_intellect: raw.skill_intellect.unwrap_or(0),
        skill_combat: raw.skill_combat.unwrap_or(0),
        skill_agility: raw.skill_agility.unwrap_or(0),
        skill_wild: raw.skill_wild.unwrap_or(0),
        health: raw.health,
        sanity: raw.sanity,
        deck_limit: raw.deck_limit.unwrap_or(0),
        quantity: raw.quantity.unwrap_or(1),
        pack_code: raw.pack_code,
        is_fast,
        uses,
        commit_limit,
        shroud: raw.shroud,
        clues: raw.clues,
        clues_fixed: raw.clues_fixed.unwrap_or(false),
        victory: raw.victory,
        doom: raw.doom,
        enemy_fight: raw.enemy_fight,
        enemy_evade: raw.enemy_evade,
        enemy_damage: raw.enemy_damage,
        enemy_horror: raw.enemy_horror,
        health_per_investigator: raw.health_per_investigator.unwrap_or(false),
        hunter,
        retaliate,
        prey,
        spawn_name,
        spawn_code: None,
    })
}

fn map_class(faction: Option<&str>, code: &str) -> Result<&'static str, String> {
    match faction {
        Some("guardian") => Ok("Guardian"),
        Some("seeker") => Ok("Seeker"),
        Some("rogue") => Ok("Rogue"),
        Some("mystic") => Ok("Mystic"),
        Some("survivor") => Ok("Survivor"),
        Some("neutral") => Ok("Neutral"),
        Some("mythos") => Ok("Mythos"),
        Some(other) => Err(format!("unknown faction_code {other:?} on card {code}")),
        None => Err(format!("missing faction_code on card {code}")),
    }
}

fn map_card_type(type_code: Option<&str>, code: &str) -> Result<&'static str, String> {
    match type_code {
        Some("investigator") => Ok("Investigator"),
        Some("asset") => Ok("Asset"),
        Some("event") => Ok("Event"),
        Some("skill") => Ok("Skill"),
        Some("treachery") => Ok("Treachery"),
        Some("enemy") => Ok("Enemy"),
        Some("location") => Ok("Location"),
        Some("agenda") => Ok("Agenda"),
        Some("act") => Ok("Act"),
        Some("scenario") => Ok("Scenario"),
        Some("story") => Ok("Story"),
        Some(other) => Err(format!("unknown type_code {other:?} on card {code}")),
        None => Err(format!("missing type_code on card {code}")),
    }
}

fn parse_traits(raw: Option<&str>) -> Vec<String> {
    let Some(raw) = raw else { return Vec::new() };
    raw.split('.')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Parse upstream's `slot` string into a vec of slot variants.
///
/// Upstream notation in Core + Dunwich is one of:
/// - bare slot name: `"Hand"`, `"Accessory"`, `"Ally"`, `"Arcane"`, `"Body"`
/// - multi-slot count: `"Hand x2"` (Shotgun, Lightning Gun, etc.)
///
/// `"Foo xN"` expands to `N` copies of `Foo`. Unknown slot names are
/// dropped silently — Chapter-2 introduced `"Head"` which we don't
/// ingest yet (pipeline reads original Core, not `core_2026`); add a
/// `Head` variant to `card_dsl::card_data::Slot` when widening coverage.
fn parse_slots(raw: Option<&str>) -> Vec<&'static str> {
    let Some(raw) = raw else { return Vec::new() };
    let raw = raw.trim();
    if raw.is_empty() {
        return Vec::new();
    }

    let (name, count) = if let Some((name, suffix)) = raw.rsplit_once(" x") {
        let count = suffix.trim().parse::<usize>().unwrap_or(1);
        (name.trim(), count)
    } else {
        (raw, 1)
    };

    let slot = match name {
        "Hand" => "Hand",
        "Accessory" => "Accessory",
        "Ally" => "Ally",
        "Arcane" => "Arcane",
        "Body" => "Body",
        "Tarot" => "Tarot",
        _ => return Vec::new(),
    };

    vec![slot; count]
}

// ---- code generation --------------------------------------------

fn render(all: &BTreeMap<String, NormalizedCard>) -> String {
    let mut out = String::new();
    out.push_str(GENERATED_HEADER);
    out.push_str(
        "use card_dsl::card_data::{\n    \
         CardKind, CardMetadata, Class, ClueValue, HealthValue, Prey, PreyDirection, PreyMeasure,\n    \
         SkillIcons, SkillKind, Skills, Slot, Spawn, SpawnLocation, UseKind, Uses,\n};\n\n\
         /// Every card from the pinned snapshot, sorted by code.\n\
         #[must_use]\n\
         pub fn all_cards() -> Vec<CardMetadata> {\n    vec![\n",
    );
    for card in all.values() {
        render_card(&mut out, card);
    }
    out.push_str("    ]\n}\n");
    out
}

/// Write a single [`CardMetadata`] literal into `out`.
///
/// Split out from [`render_card`] so the snapshot test can call it
/// against a synthetic [`NormalizedCard`] without running the full
/// pipeline.
#[cfg(test)]
fn emit_card(out: &mut String, c: &NormalizedCard) {
    render_card(out, c);
}

fn render_card(out: &mut String, c: &NormalizedCard) {
    let _ = writeln!(out, "        CardMetadata {{");
    let _ = writeln!(out, "            code: {}.to_owned(),", str_lit(&c.code));
    let _ = writeln!(out, "            name: {}.to_owned(),", str_lit(&c.name));
    let _ = writeln!(out, "            traits: {},", string_vec(&c.traits));
    let _ = writeln!(
        out,
        "            text: {},",
        opt_owned_str(c.text.as_deref())
    );
    let _ = writeln!(
        out,
        "            pack_code: {}.to_owned(),",
        str_lit(&c.pack_code)
    );
    let _ = writeln!(out, "            kind: {},", render_kind(c));
    let _ = writeln!(out, "        }},");
}

/// Render the `CardKind` literal for `c`, dispatched on its card type.
///
/// Enemy `hunter`/`retaliate`/`prey`/`spawn`/`health` are parsed from card
/// text (C3b); `surge`/`peril` still emit their not-yet-parsed `false`
/// default (#138).
fn render_kind(c: &NormalizedCard) -> String {
    let icons = format!(
        "SkillIcons {{ willpower: {}, intellect: {}, combat: {}, agility: {}, wild: {} }}",
        c.skill_willpower, c.skill_intellect, c.skill_combat, c.skill_agility, c.skill_wild,
    );
    match c.card_type {
        "Investigator" => format!(
            "CardKind::Investigator {{ class: Class::{}, \
             skills: Skills {{ willpower: {}, intellect: {}, combat: {}, agility: {} }}, \
             health: {}, sanity: {} }}",
            c.class,
            i8::try_from(c.skill_willpower).unwrap_or(0),
            i8::try_from(c.skill_intellect).unwrap_or(0),
            i8::try_from(c.skill_combat).unwrap_or(0),
            i8::try_from(c.skill_agility).unwrap_or(0),
            c.health.unwrap_or(0),
            c.sanity.unwrap_or(0),
        ),
        "Asset" => format!(
            "CardKind::Asset {{ class: Class::{}, cost: {}, xp: {}, slots: {}, \
             health: {}, sanity: {}, skill_icons: {}, is_fast: {}, deck_limit: {}, uses: {} }}",
            c.class,
            opt_i8(c.cost),
            opt_u8(c.xp),
            slot_vec(&c.slots),
            opt_u8(c.health),
            opt_u8(c.sanity),
            icons,
            c.is_fast,
            c.deck_limit,
            uses_lit(c.uses),
        ),
        "Event" => format!(
            "CardKind::Event {{ class: Class::{}, cost: {}, xp: {}, \
             skill_icons: {}, is_fast: {}, deck_limit: {} }}",
            c.class,
            opt_i8(c.cost),
            opt_u8(c.xp),
            icons,
            c.is_fast,
            c.deck_limit,
        ),
        "Skill" => format!(
            "CardKind::Skill {{ class: Class::{}, xp: {}, skill_icons: {}, deck_limit: {}, \
             commit_limit: {} }}",
            c.class,
            opt_u8(c.xp),
            icons,
            c.deck_limit,
            opt_u8(c.commit_limit),
        ),
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
        "Treachery" => format!(
            "CardKind::Treachery {{ surge: false, peril: false, quantity: {} }}",
            c.quantity,
        ),
        "Location" => format!(
            "CardKind::Location {{ shroud: {}, printed_clues: {}, victory: {} }}",
            c.shroud.unwrap_or(0),
            clue_value_lit(c.clues.unwrap_or(0), c.clues_fixed),
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
        other => panic!(
            "card {}: card_type {other:?} has no CardKind variant \
             (`scenario`/`story` are skipped in process_raw)",
            c.code
        ),
    }
}

fn str_lit(s: &str) -> String {
    // Use Rust's debug formatting which produces a properly escaped
    // double-quoted string literal for arbitrary unicode.
    format!("{s:?}")
}

fn opt_owned_str(s: Option<&str>) -> String {
    match s {
        Some(text) => format!("Some({}.to_owned())", str_lit(text)),
        None => "None".into(),
    }
}

fn opt_i8(v: Option<i8>) -> String {
    match v {
        Some(n) => format!("Some({n})"),
        None => "None".into(),
    }
}

fn opt_u8(v: Option<u8>) -> String {
    match v {
        Some(n) => format!("Some({n})"),
        None => "None".into(),
    }
}

/// Emit the `Option<Uses>` literal for an asset's parsed `uses`.
fn uses_lit(uses: Option<(u8, &'static str, bool)>) -> String {
    match uses {
        Some((count, variant, discard_when_empty)) => format!(
            "Some(Uses {{ kind: UseKind::{variant}, count: {count}, \
             discard_when_empty: {discard_when_empty} }})"
        ),
        None => "None".into(),
    }
}

/// Emit the `ClueValue` literal for a location's clues.
fn clue_value_lit(clues: u8, fixed: bool) -> String {
    if fixed {
        format!("ClueValue::Fixed({clues})")
    } else {
        format!("ClueValue::PerInvestigator({clues})")
    }
}

/// Strip `ArkhamDB` bold markup so keyword lines can be matched plainly.
fn strip_html_bold(s: &str) -> String {
    s.replace("<b>", "").replace("</b>", "")
}

/// True if `text` contains the standalone keyword token `"<kw>."`
/// (e.g. `"Hunter."`). Bold tags are stripped first.
fn has_keyword(text: &str, keyword: &str) -> bool {
    strip_html_bold(text).contains(&format!("{keyword}."))
}

/// Parse a printed `Uses (N <kind>)` clause into `(count, variant)`, where
/// `variant` is the `UsesKind` Rust variant name for code emission. Returns
/// `None` when absent; warns + returns `None` for an unmodeled kind rather
/// than silently approximating.
fn parse_uses(text: &str) -> Option<(u8, &'static str, bool)> {
    let plain = strip_html_bold(text);
    let start = plain.find("Uses (")? + "Uses (".len();
    let inner = &plain[start..];
    let end = inner.find(')')?;
    let body = inner[..end].trim(); // e.g. "4 ammo"
    let (num, kind) = body.split_once(' ')?;
    let count: u8 = num.trim().parse().ok()?;
    let kind_word = kind.trim().to_ascii_lowercase();
    let variant = match kind_word.as_str() {
        "ammo" => "Ammo",
        "charges" => "Charges",
        "secrets" => "Secrets",
        "supplies" => "Supplies",
        other => {
            eprintln!("warning: unmodeled Uses kind {other:?}; emitting uses: None");
            return None;
        }
    };
    // "If <name> has no <kind>, discard it." — a templated depletion clause
    // (RR p.27); tie it to this card's own uses-kind so an unrelated "discard
    // it" elsewhere doesn't trip it. TODO: the 2026 reprints use a second
    // phrasing ("If there are no <kind> on <name>, discard it.", e.g. Bandages
    // 12073); broaden to also accept "no <kind> on" when that pack is ingested.
    let lower = plain.to_ascii_lowercase();
    let discard_when_empty =
        lower.contains(&format!("has no {kind_word}")) && lower.contains("discard");
    Some((count, variant, discard_when_empty))
}

/// Parse a printed `Max N committed per skill test` clause into the cap
/// `N`. Returns `None` when absent (the card is uncapped). The phrase is
/// fixed across the cards that carry it, so a literal scan suffices.
fn parse_commit_limit(text: &str) -> Option<u8> {
    let plain = strip_html_bold(text);
    let start = plain.find("Max ")? + "Max ".len();
    let inner = &plain[start..];
    let end = inner.find(" committed per skill test")?;
    inner[..end].trim().parse().ok()
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

/// Emit the `Option<HealthValue>` literal for an enemy's health, mirroring
/// `clue_value_lit`. Polarity is the opposite of clues: per-investigator
/// only when `ArkhamDB`'s `health_per_investigator` is set.
fn health_value_opt_lit(health: Option<u8>, per_investigator: bool) -> String {
    match health {
        None => "None".to_owned(),
        Some(n) if per_investigator => format!("Some(HealthValue::PerInvestigator({n}))"),
        Some(n) => format!("Some(HealthValue::Fixed({n}))"),
    }
}

fn string_vec(xs: &[String]) -> String {
    if xs.is_empty() {
        return "Vec::new()".into();
    }
    let inner = xs
        .iter()
        .map(|s| format!("{}.to_owned()", str_lit(s)))
        .collect::<Vec<_>>()
        .join(", ");
    format!("vec![{inner}]")
}

fn slot_vec(xs: &[&str]) -> String {
    if xs.is_empty() {
        return "Vec::new()".into();
    }
    let inner = xs
        .iter()
        .map(|s| format!("Slot::{s}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("vec![{inner}]")
}

const GENERATED_HEADER: &str = "\
//! GENERATED FILE — do not edit by hand.
//!
//! Produced by `cargo run -p card-data-pipeline` from the pinned
//! snapshot at `data/arkhamdb-snapshot/`. To refresh, re-run the
//! pipeline; review the diff in your PR.

#![allow(clippy::too_many_lines, clippy::needless_raw_string_hashes)]

";

#[cfg(test)]
mod tests {
    use super::{
        clue_value_lit, emit_card, has_keyword, health_value_opt_lit, map_card_type, map_class,
        normalize, parse_commit_limit, parse_prey, parse_slots, parse_spawn_name, parse_traits,
        parse_uses, prey_lit, process_raw, strip_html_bold, NormalizedCard, PreyParse, RawCard,
    };
    use std::collections::BTreeMap;
    use std::path::Path;

    #[test]
    fn strip_html_bold_removes_bold_tags() {
        assert_eq!(
            strip_html_bold("<b>Prey</b> - Highest [combat]."),
            "Prey - Highest [combat]."
        );
    }

    #[test]
    fn has_keyword_matches_standalone_token() {
        assert!(has_keyword("Hunter. Retaliate.", "Hunter"));
        assert!(has_keyword("Hunter. Retaliate.", "Retaliate"));
        assert!(!has_keyword("<b>Spawn</b> - Attic.", "Hunter"));
    }

    #[test]
    fn parse_uses_reads_ammo_count() {
        assert_eq!(
            parse_uses("Uses (4 ammo).\n[action] Spend 1 ammo: Fight."),
            Some((4u8, "Ammo", false))
        );
        // All modeled UseKind variants map (not just ammo).
        assert_eq!(
            parse_uses("Uses (5 charges)."),
            Some((5u8, "Charges", false))
        );
        assert_eq!(
            parse_uses("Uses (3 secrets)."),
            Some((3u8, "Secrets", false))
        );
        assert_eq!(
            parse_uses("Uses (3 supplies)."),
            Some((3u8, "Supplies", false))
        );
        assert_eq!(parse_uses("Some other card text."), None);
        // Genuinely unmodeled kind → None (with a build warning).
        assert_eq!(parse_uses("Uses (4 time)."), None);
    }

    #[test]
    fn parse_uses_reads_discard_when_empty() {
        // First Aid: "Uses (3 supplies). If First Aid has no supplies, discard it."
        let first_aid = "Uses (3 supplies). If First Aid has no supplies, discard it.";
        assert_eq!(parse_uses(first_aid), Some((3, "Supplies", true)));
        // Flashlight: "Uses (3 supplies)." — no discard clause.
        assert_eq!(
            parse_uses("Uses (3 supplies)."),
            Some((3, "Supplies", false))
        );
    }

    #[test]
    fn parse_commit_limit_reads_max_committed_clause() {
        assert_eq!(
            parse_commit_limit(
                "Max 1 committed per skill test.\nIf this test is successful, draw 1 card."
            ),
            Some(1u8),
        );
        assert_eq!(
            parse_commit_limit("Max 2 committed per skill test."),
            Some(2u8)
        );
        // No clause → None (uncapped).
        assert_eq!(parse_commit_limit("Practiced. 1 [intellect] icon."), None);
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
        assert_eq!(
            prey_lit(&PreyParse::Unrecognized("Most clues".to_owned())),
            "Prey::Default"
        );
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
        assert_eq!(
            parse_spawn_name("<b>Spawn</b> - Attic."),
            Some("Attic".to_owned())
        );
        assert_eq!(
            parse_spawn_name("<b>Spawn</b> - Cellar."),
            Some("Cellar".to_owned())
        );
        assert_eq!(parse_spawn_name("Hunter."), None);
    }

    #[test]
    fn health_value_opt_lit_mirrors_clue_value() {
        assert_eq!(health_value_opt_lit(None, false), "None");
        assert_eq!(
            health_value_opt_lit(Some(4), false),
            "Some(HealthValue::Fixed(4))"
        );
        assert_eq!(
            health_value_opt_lit(Some(5), true),
            "Some(HealthValue::PerInvestigator(5))"
        );
    }

    /// Minimal `RawCard` fixture with the required fields populated and
    /// optional fields cleared. Tweak by mutating the returned value
    /// before passing to `normalize` / `process_raw`.
    fn raw_card(code: &str) -> RawCard {
        RawCard {
            code: code.to_owned(),
            name: Some(format!("Card {code}")),
            text: None,
            traits: None,
            slot: None,
            cost: None,
            xp: None,
            health: None,
            sanity: None,
            deck_limit: None,
            quantity: None,
            pack_code: "core".to_owned(),
            faction_code: Some("seeker".to_owned()),
            type_code: Some("asset".to_owned()),
            skill_willpower: None,
            skill_intellect: None,
            skill_combat: None,
            skill_agility: None,
            skill_wild: None,
            shroud: None,
            clues: None,
            clues_fixed: None,
            victory: None,
            doom: None,
            enemy_fight: None,
            enemy_evade: None,
            enemy_damage: None,
            enemy_horror: None,
            health_per_investigator: None,
        }
    }

    /// Build a `NormalizedCard` with all-default fields for the given
    /// code/name/type — so emit tests don't repeat the full literal.
    fn normalized(code: &str, name: &str, card_type: &'static str) -> NormalizedCard {
        NormalizedCard {
            code: code.into(),
            name: name.into(),
            class: "Mythos",
            card_type,
            cost: None,
            xp: None,
            text: None,
            traits: Vec::new(),
            slots: Vec::new(),
            skill_willpower: 0,
            skill_intellect: 0,
            skill_combat: 0,
            skill_agility: 0,
            skill_wild: 0,
            health: None,
            sanity: None,
            deck_limit: 0,
            quantity: 1,
            pack_code: "core".into(),
            is_fast: false,
            uses: None,
            commit_limit: None,
            shroud: None,
            clues: None,
            clues_fixed: false,
            victory: None,
            doom: None,
            enemy_fight: None,
            enemy_evade: None,
            enemy_damage: None,
            enemy_horror: None,
            health_per_investigator: false,
            hunter: false,
            retaliate: false,
            prey: PreyParse::None,
            spawn_name: None,
            spawn_code: None,
        }
    }

    // ---- parse_slots (pre-existing tests) ----------------------------

    #[test]
    fn parses_bare_slot() {
        assert_eq!(parse_slots(Some("Hand")), vec!["Hand"]);
        assert_eq!(parse_slots(Some("Accessory")), vec!["Accessory"]);
    }

    #[test]
    fn parses_multi_slot_xn_notation() {
        assert_eq!(parse_slots(Some("Hand x2")), vec!["Hand", "Hand"]);
        assert_eq!(parse_slots(Some("Arcane x2")), vec!["Arcane", "Arcane"]);
    }

    #[test]
    fn drops_unknown_slots() {
        // Head only appears on core_2026 cards we don't ingest.
        assert!(parse_slots(Some("Head")).is_empty());
        assert!(parse_slots(Some("Wibble")).is_empty());
    }

    #[test]
    fn handles_missing_or_empty() {
        assert!(parse_slots(None).is_empty());
        assert!(parse_slots(Some("")).is_empty());
        assert!(parse_slots(Some("   ")).is_empty());
    }

    #[test]
    fn dot_separated_repeats_are_dropped() {
        // The "Hand. Hand." pattern was the bug-discovery shape — the
        // original parser split on '.' and would have emitted two Hand
        // slots from this. Upstream uses "Hand x2" instead. If they
        // ever switch back, this regression test pins the breakage so
        // we notice rather than silently mis-emit.
        assert!(parse_slots(Some("Hand. Hand.")).is_empty());
    }

    #[test]
    fn zero_count_emits_no_slots() {
        // `Foo x0` is degenerate but shouldn't crash; emit nothing.
        assert!(parse_slots(Some("Hand x0")).is_empty());
    }

    #[test]
    fn high_count_does_not_crash() {
        // No real card has this; just guard against panics on weird
        // upstream data.
        assert_eq!(parse_slots(Some("Hand x10")).len(), 10);
    }

    // ---- map_class ---------------------------------------------------

    #[test]
    fn map_class_maps_all_known_factions() {
        for (faction, expected) in [
            ("guardian", "Guardian"),
            ("seeker", "Seeker"),
            ("rogue", "Rogue"),
            ("mystic", "Mystic"),
            ("survivor", "Survivor"),
            ("neutral", "Neutral"),
            ("mythos", "Mythos"),
        ] {
            assert_eq!(map_class(Some(faction), "01001").unwrap(), expected);
        }
    }

    #[test]
    fn map_class_errors_on_unknown_faction() {
        let err = map_class(Some("wibble"), "01001").unwrap_err();
        assert!(err.contains("wibble"), "{err}");
        assert!(err.contains("01001"), "{err}");
    }

    #[test]
    fn map_class_errors_on_missing_faction() {
        let err = map_class(None, "01001").unwrap_err();
        assert!(err.contains("missing"), "{err}");
        assert!(err.contains("01001"), "{err}");
    }

    // ---- map_card_type -----------------------------------------------

    #[test]
    fn map_card_type_maps_all_known_types() {
        for (type_code, expected) in [
            ("investigator", "Investigator"),
            ("asset", "Asset"),
            ("event", "Event"),
            ("skill", "Skill"),
            ("treachery", "Treachery"),
            ("enemy", "Enemy"),
            ("location", "Location"),
            ("agenda", "Agenda"),
            ("act", "Act"),
            ("scenario", "Scenario"),
            ("story", "Story"),
        ] {
            assert_eq!(map_card_type(Some(type_code), "01001").unwrap(), expected);
        }
    }

    #[test]
    fn map_card_type_errors_on_unknown_type() {
        let err = map_card_type(Some("wibble"), "01001").unwrap_err();
        assert!(err.contains("wibble"), "{err}");
        assert!(err.contains("01001"), "{err}");
    }

    #[test]
    fn map_card_type_errors_on_missing_type() {
        let err = map_card_type(None, "01001").unwrap_err();
        assert!(err.contains("missing"), "{err}");
        assert!(err.contains("01001"), "{err}");
    }

    // ---- parse_traits ------------------------------------------------

    #[test]
    fn parse_traits_handles_missing_and_empty() {
        assert!(parse_traits(None).is_empty());
        assert!(parse_traits(Some("")).is_empty());
        assert!(parse_traits(Some("   ")).is_empty());
    }

    #[test]
    fn parse_traits_single_trait() {
        assert_eq!(parse_traits(Some("Detective.")), vec!["Detective"]);
    }

    #[test]
    fn parse_traits_multi_trait_dot_separated() {
        // ArkhamDB convention: trailing dot on each trait.
        assert_eq!(
            parse_traits(Some("Item. Tool. Relic.")),
            vec!["Item", "Tool", "Relic"]
        );
    }

    #[test]
    fn parse_traits_trims_whitespace_and_skips_empties() {
        // The split on '.' produces an empty trailing element after
        // the final dot; the filter drops it. Also drop interior
        // whitespace-only fragments. Not a known real upstream shape,
        // just defensive against malformed data.
        assert_eq!(parse_traits(Some("Item.  . Tool.")), vec!["Item", "Tool"]);
    }

    // ---- normalize ---------------------------------------------------

    #[test]
    fn normalize_happy_path_populates_all_fields() {
        // Synthetic fixture; values are not meant to match any real
        // ArkhamDB card. Exercises the field-by-field normalize path
        // without coupling to snapshot data.
        let mut raw = raw_card("TEST01");
        raw.name = Some("Test Card".to_owned());
        raw.text = Some("Test ability text.".to_owned());
        raw.traits = Some("Alpha. Beta.".to_owned());
        raw.slot = None;
        raw.cost = Some(0);
        raw.xp = Some(0);
        raw.faction_code = Some("seeker".to_owned());
        raw.type_code = Some("skill".to_owned());
        raw.deck_limit = Some(2);
        raw.quantity = Some(2);
        raw.skill_intellect = Some(1);

        let n = normalize(raw).expect("happy-path RawCard normalizes");
        assert_eq!(n.code, "TEST01");
        assert_eq!(n.name, "Test Card");
        assert_eq!(n.class, "Seeker");
        assert_eq!(n.card_type, "Skill");
        assert_eq!(n.cost, Some(0));
        assert_eq!(n.xp, Some(0));
        assert_eq!(n.text.as_deref(), Some("Test ability text."));
        assert_eq!(n.traits, vec!["Alpha", "Beta"]);
        assert!(n.slots.is_empty());
        assert_eq!(n.skill_intellect, 1);
        assert_eq!(n.skill_willpower, 0);
        assert_eq!(n.deck_limit, 2);
        assert_eq!(n.quantity, 2);
    }

    #[test]
    fn normalize_defaults_optional_skills_to_zero() {
        let raw = raw_card("01001");
        // All skill_* fields are None on the fixture; normalize should
        // unwrap_or(0) without complaint. Also pins the asymmetric
        // deck_limit / quantity defaults — deck_limit defaults to 0
        // (no copies allowed), quantity defaults to 1 (one copy in the
        // physical product), and a future swap of those would be a
        // real bug.
        let n = normalize(raw).expect("fixture normalizes");
        assert_eq!(n.skill_willpower, 0);
        assert_eq!(n.skill_intellect, 0);
        assert_eq!(n.skill_combat, 0);
        assert_eq!(n.skill_agility, 0);
        assert_eq!(n.skill_wild, 0);
        assert_eq!(n.deck_limit, 0);
        assert_eq!(n.quantity, 1);
    }

    #[test]
    fn normalize_errors_on_missing_name() {
        let mut raw = raw_card("01001");
        raw.name = None;
        let err = normalize(raw).unwrap_err();
        assert!(err.contains("name"), "{err}");
    }

    #[test]
    fn normalize_propagates_unknown_faction_error() {
        let mut raw = raw_card("01001");
        raw.faction_code = Some("wibble".to_owned());
        let err = normalize(raw).unwrap_err();
        assert!(err.contains("faction_code"), "{err}");
        assert!(err.contains("wibble"), "{err}");
    }

    #[test]
    fn normalize_propagates_unknown_type_error() {
        let mut raw = raw_card("01001");
        raw.type_code = Some("wibble".to_owned());
        let err = normalize(raw).unwrap_err();
        assert!(err.contains("type_code"), "{err}");
        assert!(err.contains("wibble"), "{err}");
    }

    #[test]
    fn normalize_errors_on_cost_overflow() {
        // i8 max is 127; upstream cost is read as i32, so 200 is a
        // valid input that doesn't fit. The branch returns an
        // explanatory error; pin the message shape so the diagnostic
        // doesn't quietly become "doesn't fit" with no context.
        let mut raw = raw_card("01001");
        raw.cost = Some(200);
        let err = normalize(raw).unwrap_err();
        assert!(err.contains("cost"), "{err}");
        assert!(err.contains("200"), "{err}");
        assert!(err.contains("01001"), "{err}");
    }

    #[test]
    fn normalize_silently_drops_xp_overflow() {
        // xp is u8 in the normalized shape; values that don't fit
        // (which the snapshot never produces today) are silently
        // dropped to None via `and_then(|n| u8::try_from(n).ok())`.
        // Pin the silent-drop behavior so a future "error instead"
        // change is a conscious decision.
        let mut raw = raw_card("01001");
        raw.xp = Some(300);
        let n = normalize(raw).expect("xp overflow does not error");
        assert_eq!(n.xp, None);
    }

    // ---- process_raw -------------------------------------------------

    #[test]
    fn process_raw_skips_skeleton_entries_silently() {
        // Skeleton entries (no `name`) are reserved-code placeholders
        // upstream; the pipeline should ignore them rather than error.
        let mut raw = raw_card("01999");
        raw.name = None;
        let mut all = BTreeMap::new();
        process_raw(raw, &mut all, Path::new("fixture.json"))
            .expect("skeleton entries are not an error");
        assert!(all.is_empty(), "skipped entry should not be inserted");
    }

    #[test]
    fn process_raw_rejects_duplicate_code() {
        let mut all = BTreeMap::new();
        process_raw(raw_card("01001"), &mut all, Path::new("fixture.json"))
            .expect("first insert succeeds");
        let err = process_raw(raw_card("01001"), &mut all, Path::new("fixture.json"))
            .expect_err("second insert with same code errors");
        assert!(err.contains("duplicate"), "{err}");
        assert!(err.contains("01001"), "{err}");
    }

    #[test]
    fn process_raw_inserts_normalized_card() {
        let mut all = BTreeMap::new();
        process_raw(raw_card("01001"), &mut all, Path::new("fixture.json")).unwrap();
        assert_eq!(all.len(), 1);
        assert!(all.contains_key("01001"));
    }

    #[test]
    fn process_raw_wraps_normalize_error_with_path() {
        let mut raw = raw_card("01001");
        raw.faction_code = Some("wibble".to_owned());
        let mut all = BTreeMap::new();
        let err = process_raw(raw, &mut all, Path::new("fixture.json")).unwrap_err();
        // The wrapping format is "normalizing card in <path>: <inner>"
        assert!(err.contains("fixture.json"), "{err}");
        assert!(err.contains("wibble"), "{err}");
    }

    // ---- is_fast detection ------------------------------------------

    #[test]
    fn fast_prefix_detected_at_start_of_text() {
        let raw = RawCard {
            code: "01030".into(),
            name: Some("Magnifying Glass".into()),
            text: Some("Fast.\nYou get +1 [intellect] while investigating.".into()),
            traits: None,
            slot: Some("Hand".into()),
            cost: Some(1),
            xp: Some(0),
            health: None,
            sanity: None,
            deck_limit: Some(2),
            quantity: Some(1),
            pack_code: "core".into(),
            faction_code: Some("seeker".into()),
            type_code: Some("asset".into()),
            skill_willpower: None,
            skill_intellect: Some(1),
            skill_combat: None,
            skill_agility: None,
            skill_wild: None,
            shroud: None,
            clues: None,
            clues_fixed: None,
            victory: None,
            doom: None,
            enemy_fight: None,
            enemy_evade: None,
            enemy_damage: None,
            enemy_horror: None,
            health_per_investigator: None,
        };
        let norm = normalize(raw).expect("normalize");
        assert!(
            norm.is_fast,
            "card text begins with \"Fast.\", expected is_fast=true"
        );
    }

    #[test]
    fn emitted_treachery_renders_treachery_kind() {
        // A treachery emits a `CardKind::Treachery { … }` with its
        // surge/peril defaults and quantity — and carries no class.
        let card = normalized("01001", "Test", "Treachery");
        let mut buf = String::new();
        emit_card(&mut buf, &card);
        assert!(
            buf.contains("CardKind::Treachery {"),
            "emitted treachery should render a Treachery kind; got:\n{buf}",
        );
        assert!(
            !buf.contains("class:"),
            "treachery carries no class; got:\n{buf}",
        );
    }

    #[test]
    fn emitted_location_renders_location_kind() {
        let mut c = normalized("01111", "Study", "Location");
        c.shroud = Some(2);
        c.clues = Some(2);
        let mut buf = String::new();
        emit_card(&mut buf, &c);
        assert!(
            buf.contains(
                "CardKind::Location { shroud: 2, printed_clues: ClueValue::PerInvestigator(2)"
            ),
            "got:\n{buf}",
        );
    }

    #[test]
    fn location_clue_value_reflects_clues_fixed() {
        assert_eq!(clue_value_lit(2, false), "ClueValue::PerInvestigator(2)");
        assert_eq!(clue_value_lit(2, true), "ClueValue::Fixed(2)");
    }

    #[test]
    fn emitted_enemy_renders_combat_stats() {
        let mut c = normalized("01116", "Ghoul Priest", "Enemy");
        c.enemy_fight = Some(4);
        c.enemy_evade = Some(4);
        c.health = Some(5);
        let mut buf = String::new();
        emit_card(&mut buf, &c);
        assert!(
            buf.contains("CardKind::Enemy { fight: 4, evade: 4,"),
            "got:\n{buf}",
        );
    }

    #[test]
    fn scenario_type_card_is_skipped() {
        let mut raw = raw_card("01104");
        raw.type_code = Some("scenario".to_owned());
        let mut all = BTreeMap::new();
        process_raw(raw, &mut all, Path::new("fixture.json"))
            .expect("scenario skip is not an error");
        assert!(all.is_empty(), "scenario-type cards are skipped");
    }

    #[test]
    fn fast_marker_inside_text_is_not_a_fast_card() {
        let raw = RawCard {
            code: "01034".into(),
            name: Some("Hyperawareness".into()),
            text: Some(
                "[fast] Spend 1 resource: You get +1 [intellect] for this skill test.".into(),
            ),
            traits: None,
            slot: None,
            cost: Some(2),
            xp: Some(0),
            health: None,
            sanity: None,
            deck_limit: Some(2),
            quantity: Some(1),
            pack_code: "core".into(),
            faction_code: Some("seeker".into()),
            type_code: Some("asset".into()),
            skill_willpower: None,
            skill_intellect: Some(1),
            skill_combat: None,
            skill_agility: Some(1),
            skill_wild: None,
            shroud: None,
            clues: None,
            clues_fixed: None,
            victory: None,
            doom: None,
            enemy_fight: None,
            enemy_evade: None,
            enemy_damage: None,
            enemy_horror: None,
            health_per_investigator: None,
        };
        let norm = normalize(raw).expect("normalize");
        assert!(
            !norm.is_fast,
            "card text does NOT begin with \"Fast.\"; [fast] inside text is unrelated"
        );
    }
}
