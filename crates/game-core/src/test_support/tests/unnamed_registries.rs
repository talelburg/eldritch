//! A fixture that needs a registry the test never named panics and says so,
//! rather than resolving against some default the test didn't choose.

use crate::state::GameStateBuilder;
use crate::test_support::{seat, TEST_INV, TEST_TREACHERY};

#[test]
#[should_panic(expected = "name the registries first")]
fn as_card_without_named_registries_panics() {
    let _ = GameStateBuilder::new().with_seat(seat(1).as_card(TEST_INV));
}

#[test]
#[should_panic(expected = "name the registries first")]
fn a_threat_area_card_without_named_registries_panics() {
    let _ = GameStateBuilder::new().with_seat(seat(1).threat_area([TEST_TREACHERY]));
}

#[test]
#[should_panic(expected = "name the registries first")]
fn a_session_without_named_registries_panics() {
    let _ = GameStateBuilder::new().session();
}
