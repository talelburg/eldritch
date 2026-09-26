use super::*;

fn pack(code: &str, cycle: &str) -> RawPack {
    RawPack {
        code: code.into(),
        cycle_code: cycle.into(),
    }
}

fn files(paths: &[&str]) -> Vec<PathBuf> {
    paths.iter().map(PathBuf::from).collect()
}

/// A manifest matching the fixtures below: one ingested pack, one
/// reference pack, one out-of-scope pack in an out-of-scope cycle.
fn fixture_manifest() -> Manifest<'static> {
    Manifest {
        corpus: &["pack/core/core.json"],
        reference: &["pack/ptc/ptc.json", "pack/ptc/ptc_encounter.json"],
        out_of_scope: &["pack/core/core_2026.json"],
        out_of_scope_cycles: &["core_ch2"],
        packs_without_files: &["ptcp"],
    }
}

fn fixture_packs() -> Vec<RawPack> {
    vec![
        pack("core", "core"),
        pack("ptc", "ptc"),
        pack("ptcp", "ptc"),
        pack("core_2026", "core_ch2"),
    ]
}

fn fixture_files() -> Vec<PathBuf> {
    files(&[
        "pack/core/core.json",
        "pack/core/core_2026.json",
        "pack/ptc/ptc.json",
        "pack/ptc/ptc_encounter.json",
    ])
}

#[test]
fn classify_accepts_a_tree_the_manifest_describes() {
    assert_eq!(
        classify_against(&fixture_files(), &fixture_packs(), &fixture_manifest()),
        Ok(())
    );
}

#[test]
fn classify_flags_a_vendored_file_in_no_list() {
    let mut vendored = fixture_files();
    vendored.push(PathBuf::from("pack/tfa/tfa.json"));
    assert_eq!(
        classify_against(&vendored, &fixture_packs(), &fixture_manifest()),
        Err(vec![Discrepancy::Unclassified("pack/tfa/tfa.json".into())])
    );
}

#[test]
fn classify_flags_a_manifest_entry_with_no_vendored_file() {
    let vendored: Vec<PathBuf> = fixture_files()
        .into_iter()
        .filter(|p| !p.ends_with("ptc_encounter.json"))
        .collect();
    assert_eq!(
        classify_against(&vendored, &fixture_packs(), &fixture_manifest()),
        Err(vec![Discrepancy::Missing(
            "pack/ptc/ptc_encounter.json".into()
        )])
    );
}

#[test]
fn classify_flags_an_in_scope_pack_with_no_vendored_file() {
    let mut packs = fixture_packs();
    packs.push(pack("tfa", "tfa"));
    assert_eq!(
        classify_against(&fixture_files(), &packs, &fixture_manifest()),
        Err(vec![Discrepancy::MissingPack {
            code: "tfa".into(),
            cycle: "tfa".into(),
        }])
    );
}

/// `ptcp` is a new-format reprint pack: `packs.json` lists it, but
/// upstream ships no `ptcp.json`. The exception list absorbs it.
#[test]
fn classify_accepts_a_pack_on_the_no_file_exception_list() {
    // The clean case is `classify_accepts_a_tree_the_manifest_describes`
    // above; what this asserts is that the exception is load-bearing —
    // drop `ptcp` from the list and the same fixtures report it missing.
    let mut manifest = fixture_manifest();
    manifest.packs_without_files = &[];
    assert_eq!(
        classify_against(&fixture_files(), &fixture_packs(), &manifest),
        Err(vec![Discrepancy::MissingPack {
            code: "ptcp".into(),
            cycle: "ptc".into(),
        }])
    );
}

/// Chapter 2 packs are out of scope, so their absence is not a
/// discrepancy — only their *presence* unclassified would be.
#[test]
fn classify_ignores_packs_from_out_of_scope_cycles() {
    let mut packs = fixture_packs();
    packs.push(pack("tom", "investigator_decks_ch2"));
    let mut manifest = fixture_manifest();
    manifest.out_of_scope_cycles = &["core_ch2", "investigator_decks_ch2"];
    assert_eq!(
        classify_against(&fixture_files(), &packs, &manifest),
        Ok(())
    );
}

/// The guard that actually protects the tree: the real snapshot,
/// checked on every CI run rather than only when someone happens to
/// regenerate the corpus.
#[test]
fn the_real_snapshot_matches_the_manifest() {
    let snapshot = repo_root().expect("repo root").join(SNAPSHOT_DIR);
    let vendored = vendored_pack_files(&snapshot).expect("listing vendored pack files");
    let packs = read_packs(&snapshot).expect("reading packs.json");
    assert_eq!(classify(&vendored, &packs), Ok(()));
}
