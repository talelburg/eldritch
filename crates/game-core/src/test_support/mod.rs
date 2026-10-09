//! Test-only support: fixtures, event-assertion macros, and the
//! [`MockRegistry`] builder a test binary installs its mock card
//! registry through. The submodules are private and every public item is
//! re-exported here, so `test_support::<item>` is the one spelling. The
//! production [`GameStateBuilder`](crate::state::GameStateBuilder) is
//! reached through `state`.
//!
//! The macros are exported at the crate root via `#[macro_export]`,
//! so callers see [`assert_event!`](crate::assert_event) regardless
//! of where they import the supporting types from.
//!
//! The state builder itself lives in [`crate::state`] (it constructs
//! production `GameState`s, not just test ones); it is re-exported here
//! so the existing test imports keep working.

use std::sync::OnceLock;

use card_dsl::card_data::{CardKind, CardMetadata, Class, Skills};
use card_dsl::dsl::{self, Ability, EventPattern, EventTiming};

use crate::card_registry::{self, CardRegistry};
use crate::state::CardCode;

pub mod assertions;
mod fixtures;
mod mock_registry;
mod resolver;
mod session;

/// Synthetic investigator-card code for unit tests. Registered by
/// [`install_test_registry`] with 8 health / 8 sanity (mirroring the legacy
/// `test_investigator` capacity).
pub const TEST_INV: &str = "TEST_INV";

fn test_inv_metadata() -> &'static CardMetadata {
    static M: OnceLock<CardMetadata> = OnceLock::new();
    M.get_or_init(|| CardMetadata {
        code: TEST_INV.to_owned(),
        name: "Test Investigator".to_owned(),
        traits: vec![],
        text: None,
        back_name: None,
        back_text: None,
        pack_code: "_test".to_owned(),
        weakness: false,
        kind: CardKind::Investigator {
            class: Class::Neutral,
            skills: Skills {
                willpower: 3,
                intellect: 3,
                combat: 3,
                agility: 3,
            },
            health: 8,
            sanity: 8,
        },
    })
}

/// Printed-code prefix for the synthetic **terminal** act/agenda cards.
///
/// A card is terminal because it is last in its deck (ADR 0013), and a terminal
/// card ends the scenario by *running an effect on its reverse* — so a fixture
/// that wants "advancing this act/agenda ends the scenario" needs a card whose
/// abilities the registry can serve. [`terminal_code`] mints one per printed
/// resolution number and [`abilities_for_terminal`] serves its reverse.
///
/// Synthetic rather than a real corpus code (01107 / 01110): a unit test that is
/// about *terminality* should not break when a snapshot bump moves the content
/// it was never asserting on.
pub const TEST_TERMINAL_PREFIX: &str = "_TEST_TERM_R";

/// The synthetic terminal card that reaches printed resolution point `n`, e.g.
/// `terminal_code(1)` → `_TEST_TERM_R1`. Put it last in an act or agenda deck
/// and advancing it ends the scenario at `Resolution(n)`.
#[must_use]
pub fn terminal_code(n: u8) -> CardCode {
    CardCode::new(format!("{TEST_TERMINAL_PREFIX}{n}"))
}

/// Abilities lookup for the synthetic terminal cards ([`terminal_code`]).
///
/// The reverse is declared on both the act-advanced and the agenda-advanced
/// condition, in the `after` cell — the cell every real on-advance reverse uses
/// (01105, 01106, 01108, 01109), because the flip is step 2 of the Rules
/// Reference's advance procedure rather than a triggered ability. One card
/// serving both decks keeps the fixture surface to a single code family.
///
/// Composed into [`install_test_registry`], and into out-of-crate mocks the way
/// [`metadata_for_test_inv`] is:
///
/// ```ignore
/// fn mock_abilities_for(code: &CardCode) -> Option<Vec<Ability>> {
///     game_core::test_support::abilities_for_terminal(code)
///         .or_else(|| /* mock-specific lookups */)
/// }
/// ```
#[must_use]
pub fn abilities_for_terminal(code: &CardCode) -> Option<Vec<Ability>> {
    let n: u8 = code
        .as_str()
        .strip_prefix(TEST_TERMINAL_PREFIX)?
        .parse()
        .ok()?;
    Some(vec![
        dsl::forced_on_event(
            EventPattern::ActAdvanced,
            EventTiming::After,
            dsl::reach_resolution(n),
        ),
        dsl::forced_on_event(
            EventPattern::AgendaAdvanced,
            EventTiming::After,
            dsl::reach_resolution(n),
        ),
    ])
}

/// Install `base` with the synthetic test cards composed in: `TEST_INV` into its
/// `metadata_for`, and the terminal cards ([`terminal_code`]) into its
/// `abilities_for`. Whatever `base` already knows is served unchanged.
///
/// For **integration tests in other crates**, which install a real registry
/// (`cards::REGISTRY`, or one built locally in the test binary) into the
/// process-global `OnceLock` and so cannot compose at the definition site the
/// way [`install_test_registry`] does. Every real-registry binary installs
/// through here, so [`test_investigator`] resolves in all of them and no test
/// borrows a real investigator's code as a placeholder (#934):
///
/// ```ignore
/// #[ctor::ctor(unsafe)]
/// fn install() {
///     game_core::test_support::install_registry_with_test_cards(cards::REGISTRY);
/// }
/// ```
///
/// Idempotent, and — like [`card_registry::install`]
/// — first-install-wins.
pub fn install_registry_with_test_cards(base: CardRegistry) {
    static BASE: OnceLock<CardRegistry> = OnceLock::new();
    fn metadata_for(code: &CardCode) -> Option<&'static CardMetadata> {
        metadata_for_test_inv(code)
            .or_else(|| BASE.get().and_then(|base| (base.metadata_for)(code)))
    }
    fn abilities_for(code: &CardCode) -> Option<Vec<Ability>> {
        abilities_for_terminal(code)
            .or_else(|| BASE.get().and_then(|base| (base.abilities_for)(code)))
    }
    let _ = BASE.set(base);
    // `back_abilities_for` rides `..base` deliberately: the terminal cards are
    // synthetic acts/agendas with no reverse side, so there is nothing to
    // compose in, and overriding the slot would switch the *real* registry's
    // back sides off for every test that installs through here (#774).
    let _ = card_registry::install(CardRegistry {
        metadata_for,
        abilities_for,
        ..base
    });
}

/// Metadata lookup for the synthetic `TEST_INV` investigator code.
///
/// Integration tests in `crates/game-core/tests/` install their own
/// mock `CardRegistry` instead of calling [`install_test_registry`].
/// When those mocks use `test_investigator`, the investigator's
/// `investigator_card.code` is `TEST_INV`. Any code path that reads
/// `max_health()` / `max_sanity()` (damage/horror application, defeat
/// checks) calls `investigator_capacity(TEST_INV)` and needs the
/// registry to know that code. Compose this into the mock's
/// `metadata_for`:
///
/// ```ignore
/// fn mock_metadata_for(code: &CardCode) -> Option<&'static CardMetadata> {
///     game_core::test_support::metadata_for_test_inv(code)
///         .or_else(|| /* mock-specific lookups */)
/// }
/// ```
pub fn metadata_for_test_inv(code: &CardCode) -> Option<&'static CardMetadata> {
    (code.as_str() == TEST_INV).then(test_inv_metadata)
}

/// Install a minimal game-core test registry that knows `TEST_INV` and the
/// synthetic terminal cards ([`terminal_code`]), and nothing else. Idempotent;
/// safe to call from any test. Capacity-reading code (`max_health()` /
/// `max_sanity()` / soak / defeat) needs this installed, and so does any fixture
/// whose act/agenda deck ends in a terminal card — without the registry its
/// reverse never fires and the advance finds no ending latched.
///
/// game-core's own unit tests never call it: a `#[ctor]` installs it before
/// that binary's harness runs. The callers are other crates' test binaries.
///
/// One registry for the whole crate because `OnceLock<CardRegistry>` is
/// process-global: a second per-test install would collide.
pub fn install_test_registry() {
    static INSTALL: OnceLock<()> = OnceLock::new();
    INSTALL.get_or_init(|| {
        fn metadata_for(code: &CardCode) -> Option<&'static CardMetadata> {
            (code.as_str() == TEST_INV).then(test_inv_metadata)
        }
        fn abilities_for(code: &CardCode) -> Option<Vec<Ability>> {
            abilities_for_terminal(code)
        }
        let _ = card_registry::install(CardRegistry {
            metadata_for,
            abilities_for,
            ..CardRegistry::EMPTY
        });
    });
}

/// Install [`install_test_registry`] before the harness `main` of game-core's
/// own unit-test binary, the way every integration binary installs its registry
/// from a `#[ctor]` (#473). No unit test calls the install itself: a per-test
/// call is one to forget, and a test that forgot it passed in the full suite
/// only because an earlier test had installed the registry, then failed when
/// run on its own (#971).
///
/// `cfg(test)` scopes it to this crate's unit tests; integration binaries and
/// other crates link game-core without it and keep installing their own.
#[cfg(test)]
#[ctor::ctor(unsafe)]
fn install_for_unit_tests() {
    install_test_registry();
}

pub use fixtures::{
    awaiting_commit_input, awaiting_confirm_input, awaiting_pick_single_input,
    awaiting_pick_single_with, awaiting_request, awaiting_skippable_commit_input,
    awaiting_skippable_pick_single_input, awaiting_skippable_pick_single_with,
    from_frames_unchecked, test_enemy, test_investigator, test_location, test_skill_test,
};
pub use mock_registry::MockRegistry;
pub use resolver::{
    apply_no_commits, dispatch_turn_action_unchecked, drive, drive_skill_test, perform_skill_test,
    perform_skill_test_no_commits, take_turn_action, ChoiceResolver, ScriptedResolver,
    TakeOneFastPlay,
};
pub use session::TestSession;
