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
//! Inputs (relative to repo root):
//! - `data/arkhamdb-snapshot/pack/core/core.json`
//! - `data/arkhamdb-snapshot/pack/dwl/*.json` (excluding the
//!   `*_encounter.json` companion files; encounter sets are handled
//!   in a later phase)
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

/// Pack files we read for Phase 2. Encounter-companion files
/// (`*_encounter.json`) are skipped — encounter-set support lands
/// when scenario plumbing does.
const PACK_FILES: &[&str] = &[
    "pack/core/core.json",
    "pack/dwl/dwl.json",
    "pack/dwl/tmm.json",
    "pack/dwl/tece.json",
    "pack/dwl/bota.json",
    "pack/dwl/uau.json",
    "pack/dwl/wda.json",
    "pack/dwl/litas.json",
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
            // Skip skeleton entries (codes reserved upstream but no
            // populated card data — name is the cheap presence check).
            if raw.name.is_none() {
                continue;
            }
            let normalized = normalize(raw)
                .map_err(|e| format!("normalizing card in {}: {e}", path.display()))?;
            if let Some(prev) = all.insert(normalized.code.clone(), normalized) {
                return Err(format!("duplicate card code: {}", prev.code));
            }
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
    flavor: Option<String>,
    illustrator: Option<String>,
    traits: Option<String>,
    slot: Option<String>,
    cost: Option<i32>,
    xp: Option<i32>,
    health: Option<u8>,
    sanity: Option<u8>,
    deck_limit: Option<u8>,
    quantity: Option<u8>,
    pack_code: String,
    position: u32,
    faction_code: Option<String>,
    type_code: Option<String>,
    skill_willpower: Option<u8>,
    skill_intellect: Option<u8>,
    skill_combat: Option<u8>,
    skill_agility: Option<u8>,
    skill_wild: Option<u8>,
}

// ---- normalized shape we emit -----------------------------------

#[derive(Debug)]
struct NormalizedCard {
    code: String,
    name: String,
    class: &'static str,
    card_type: &'static str,
    cost: Option<i8>,
    xp: Option<u8>,
    text: Option<String>,
    flavor: Option<String>,
    illustrator: Option<String>,
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
    position: u32,
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

    Ok(NormalizedCard {
        code: raw.code,
        name,
        class,
        card_type,
        cost,
        xp: raw.xp.and_then(|n| u8::try_from(n).ok()),
        text: raw.text,
        flavor: raw.flavor,
        illustrator: raw.illustrator,
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
        position: raw.position,
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
/// `Head` variant to `game_core::card_data::Slot` when widening coverage.
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
        "use game_core::card_data::{CardMetadata, CardType, Class, SkillIcons, Slot};\n\n\
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

fn render_card(out: &mut String, c: &NormalizedCard) {
    let _ = writeln!(out, "        CardMetadata {{");
    let _ = writeln!(out, "            code: {}.to_owned(),", str_lit(&c.code));
    let _ = writeln!(out, "            name: {}.to_owned(),", str_lit(&c.name));
    let _ = writeln!(out, "            class: Class::{},", c.class);
    let _ = writeln!(out, "            card_type: CardType::{},", c.card_type);
    let _ = writeln!(out, "            cost: {},", opt_i8(c.cost));
    let _ = writeln!(out, "            xp: {},", opt_u8(c.xp));
    let _ = writeln!(
        out,
        "            text: {},",
        opt_owned_str(c.text.as_deref())
    );
    let _ = writeln!(
        out,
        "            flavor: {},",
        opt_owned_str(c.flavor.as_deref())
    );
    let _ = writeln!(
        out,
        "            illustrator: {},",
        opt_owned_str(c.illustrator.as_deref())
    );
    let _ = writeln!(out, "            traits: {},", string_vec(&c.traits));
    let _ = writeln!(out, "            slots: {},", slot_vec(&c.slots));
    let _ = writeln!(out, "            skill_icons: SkillIcons {{");
    let _ = writeln!(out, "                willpower: {},", c.skill_willpower);
    let _ = writeln!(out, "                intellect: {},", c.skill_intellect);
    let _ = writeln!(out, "                combat: {},", c.skill_combat);
    let _ = writeln!(out, "                agility: {},", c.skill_agility);
    let _ = writeln!(out, "                wild: {},", c.skill_wild);
    let _ = writeln!(out, "            }},");
    let _ = writeln!(out, "            health: {},", opt_u8(c.health));
    let _ = writeln!(out, "            sanity: {},", opt_u8(c.sanity));
    let _ = writeln!(out, "            deck_limit: {},", c.deck_limit);
    let _ = writeln!(out, "            quantity: {},", c.quantity);
    let _ = writeln!(
        out,
        "            pack_code: {}.to_owned(),",
        str_lit(&c.pack_code)
    );
    let _ = writeln!(out, "            position: {},", c.position);
    let _ = writeln!(out, "        }},");
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
    use super::parse_slots;

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
}
