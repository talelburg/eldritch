use super::*;

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
fn spawn_lit_separates_no_instruction_from_unrepresented_one() {
    // #635: the two must not collapse. No Spawn line at all → `None`,
    // which the engine reads as "spawns engaged with the drawer".
    assert_eq!(spawn_lit(None, None), "None");
    // A resolved clause → the location code.
    assert_eq!(
        spawn_lit(Some("Attic"), Some("01113")),
        "Some(Spawn { location: SpawnLocation::Specific(\"01113\".to_owned()) })",
    );
    // A printed clause we could not resolve → an explicit marker the
    // engine refuses on, carrying the clause for the rejection message.
    assert_eq!(
        spawn_lit(Some("Any empty location"), None),
        "Some(Spawn { location: SpawnLocation::Unrepresented(\"Any empty location\"\
         .to_owned()) })",
    );
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
