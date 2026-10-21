//! The elder sign under a text-box blank.
//!
//! Stubborn Detective 01103: *"While Stubborn Detective is at your location,
//! treat your investigator as if his or her printed text box were blank (except
//! for \[\[Traits\]\])."* [`elder_sign_outcome`] reads the investigator card
//! through the same lookup every other reader of it does, so a blank empties
//! its printed elder sign and leaves a granted one standing.

use super::*;

const HERE: LocationId = LocationId(10);
const ELSEWHERE: LocationId = LocationId(11);
const ID: InvestigatorId = InvestigatorId(1);

/// Investigator 1, printing an elder sign, at [`HERE`] with `in_play`, and the
/// blanking enemy at `blanker_at`.
fn board(blanker_at: LocationId, in_play: &[&str]) -> GameState {
    let mut blanker = test_support::test_enemy(50, "Blanker");
    blanker.code = CardCode::new("blanks-investigators-here");
    blanker.current_location = Some(blanker_at);
    let mut state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(test_support::test_location(HERE.0, "Study"))
        .with_location(test_support::test_location(ELSEWHERE.0, "Hallway"))
        .with_seat(seat(ID.0).at(HERE).in_play(in_play.iter().copied()))
        .with_enemy(blanker)
        .build();
    // Set after the build: the seat builder reads a card's metadata from the
    // registry, and the mock code prints only abilities.
    state
        .investigators
        .get_mut(&ID)
        .expect("seated")
        .investigator_card
        .code = CardCode::new("elder-sign-clues-here");
    state
}

fn elder_sign_modifier(state: &GameState) -> Option<IntExpr> {
    elder_sign_outcome(state, &mock_registry(), ID).map(|outcome| outcome.modifier)
}

#[test]
fn a_blanked_investigator_has_no_elder_sign() {
    assert_eq!(elder_sign_modifier(&board(HERE, &[])), None);
}

#[test]
fn a_blank_at_another_location_leaves_the_elder_sign() {
    assert_eq!(
        elder_sign_modifier(&board(ELSEWHERE, &[])),
        Some(IntExpr::Count(Quantity::CluesAtControllerLocation))
    );
}

/// `glossary/Blank.md`: *"Text and/or icons gained from another source are
/// not blanked."* A granted elder sign is gained text, so it survives.
#[test]
fn a_granted_elder_sign_survives_the_blank() {
    assert_eq!(
        elder_sign_modifier(&board(HERE, &["grants-elder-sign"])),
        Some(IntExpr::Lit(5))
    );
}
