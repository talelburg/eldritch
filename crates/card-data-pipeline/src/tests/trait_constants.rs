use super::*;

fn card_with_traits(code: &str, traits: &str) -> (String, NormalizedCard) {
    let mut raw = raw_card(code);
    raw.traits = Some(traits.to_owned());
    let card = normalize(raw).expect("fixture normalizes");
    (code.to_owned(), card)
}

#[test]
fn single_word_trait_is_uppercased() {
    assert_eq!(trait_constant_name("Tome"), Ok("TOME".to_owned()));
}

#[test]
fn multi_word_trait_is_upper_snaked() {
    assert_eq!(
        trait_constant_name("Ancient One"),
        Ok("ANCIENT_ONE".to_owned())
    );
}

#[test]
fn trait_with_punctuation_or_accent_is_refused_by_name() {
    for bad in ["R'lyeh", "Mi-Go", "Montréal", "???"] {
        let err = trait_constant_name(bad).expect_err(bad);
        assert!(err.contains(bad), "error names the trait: {err}");
    }
}

#[test]
fn empty_or_leading_digit_trait_is_refused() {
    assert!(trait_constant_name("").is_err());
    assert!(trait_constant_name("1st Wave").is_err());
}

#[test]
fn each_constant_holds_the_exact_parsed_trait_string() {
    // Parsed through the same `normalize` the corpus goes through, so
    // the constant's value is whatever the card data carries.
    let all: BTreeMap<String, NormalizedCard> = [
        card_with_traits("90001", "Ancient One. Elite."),
        card_with_traits("90002", "Item. Tome."),
    ]
    .into_iter()
    .collect();

    let out = render_traits(&all).expect("corpus traits all map");

    for line in [
        "pub const ANCIENT_ONE: &str = \"Ancient One\";",
        "pub const ELITE: &str = \"Elite\";",
        "pub const ITEM: &str = \"Item\";",
        "pub const TOME: &str = \"Tome\";",
    ] {
        assert!(out.contains(line), "missing {line:?} in:\n{out}");
    }
}

#[test]
fn a_trait_shared_by_several_cards_is_emitted_once() {
    let all: BTreeMap<String, NormalizedCard> = [
        card_with_traits("90001", "Item. Tome."),
        card_with_traits("90002", "Item. Weapon."),
    ]
    .into_iter()
    .collect();

    let out = render_traits(&all).expect("corpus traits all map");

    assert_eq!(out.matches("pub const ITEM:").count(), 1);
}

#[test]
fn two_traits_mapping_to_one_constant_is_refused() {
    let all: BTreeMap<String, NormalizedCard> = [
        card_with_traits("90001", "Deep One."),
        card_with_traits("90002", "Deep one."),
    ]
    .into_iter()
    .collect();

    let err = render_traits(&all).expect_err("collision");
    assert!(err.contains("DEEP_ONE"), "error names the constant: {err}");
}
