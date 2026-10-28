//! Test-only support: fixtures, event-assertion macros, and the
//! [`MockRegistry`] builder a test binary builds its mock card registry
//! through.
//!
//! The submodules are private and every public item is re-exported here, so
//! `test_support::<item>` is the one spelling. The production
//! [`GameStateBuilder`](crate::state::GameStateBuilder) is reached through
//! `state`.
//!
//! The macros are exported at the crate root via `#[macro_export]`,
//! so callers see [`assert_event!`](crate::assert_event) regardless
//! of where they import the supporting types from.
//!
//! The state builder itself lives in [`crate::state`] (it constructs
//! production `GameState`s, not just test ones); it is re-exported here
//! so the existing test imports keep working.

use std::sync::OnceLock;

use card_dsl::card_data::{CardKind, CardMetadata, Class, SkillIcons, Skills};
use card_dsl::dsl::{self, Ability, EventPattern, EventTiming, InvestigatorTarget};

use crate::card_registry::CardRegistry;
use crate::engine::Registries;
use crate::scenario::ScenarioRegistry;
use crate::state::CardCode;

pub mod assertions;
mod deal;
mod fixtures;
mod mock_registry;
mod native;
mod resolver;
mod seat;
mod session;
#[cfg(test)]
mod tests;

/// Synthetic investigator-card code for unit tests. Served by
/// [`TEST_REGISTRY`] with 8 health / 8 sanity (mirroring the legacy
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
        weakness: None,
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

/// Synthetic **player-cardtype** card code for unit tests: an asset, with one
/// `[action]` ability. Served by [`TEST_REGISTRY`].
///
/// For a fixture that needs a card's cardtype to be a player one — a
/// player card in a threat area, which co-location must not reach —
/// without borrowing a real corpus code the unit registry does not know.
///
/// **A probe, shared rather than test-local.** ADR 0016 keeps probe cards
/// in the one binary that reads them; these two are shared because most of
/// game-core's unit tests name the one [`TEST_REGISTRY`] rather than a
/// registry of their own. Integration binaries define their own.
pub const TEST_ASSET: &str = "TEST_ASSET";

/// Synthetic **encounter-cardtype** card code for unit tests: a treachery, with
/// one `[action]` ability.
///
/// Served by [`TEST_REGISTRY`]. The encounter counterpart of [`TEST_ASSET`],
/// and shared for the same reason.
pub const TEST_TREACHERY: &str = "TEST_TREACHERY";

/// Metadata for the synthetic cardtype cards, [`TEST_ASSET`] and
/// [`TEST_TREACHERY`].
fn metadata_for_test_cardtypes(code: &CardCode) -> Option<&'static CardMetadata> {
    fn card(code: &str, name: &str, kind: CardKind) -> CardMetadata {
        CardMetadata {
            code: code.to_owned(),
            name: name.to_owned(),
            traits: vec![],
            text: None,
            back_name: None,
            back_text: None,
            pack_code: "_test".to_owned(),
            weakness: None,
            kind,
        }
    }
    static ASSET: OnceLock<CardMetadata> = OnceLock::new();
    static TREACHERY: OnceLock<CardMetadata> = OnceLock::new();
    match code.as_str() {
        TEST_ASSET => Some(ASSET.get_or_init(|| {
            card(
                TEST_ASSET,
                "Test Asset",
                CardKind::Asset {
                    class: Class::Neutral,
                    cost: Some(0),
                    xp: None,
                    slots: vec![],
                    health: None,
                    sanity: None,
                    skill_icons: SkillIcons::default(),
                    is_fast: false,
                    deck_limit: 2,
                    uses: None,
                    play_only_during_turn: false,
                },
            )
        })),
        TEST_TREACHERY => Some(TREACHERY.get_or_init(|| {
            card(
                TEST_TREACHERY,
                "Test Treachery",
                CardKind::Treachery {
                    surge: false,
                    peril: false,
                    quantity: 1,
                },
            )
        })),
        _ => None,
    }
}

/// Abilities for the synthetic cardtype cards: one `[action]` ability each,
/// costing nothing else, so a fixture can see which of them a turn menu
/// offers.
fn abilities_for_test_cardtypes(code: &CardCode) -> Option<Vec<Ability>> {
    matches!(code.as_str(), TEST_ASSET | TEST_TREACHERY).then(|| {
        vec![dsl::activated(
            1,
            vec![],
            dsl::gain_resources(InvestigatorTarget::You, 1),
        )]
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
/// `terminal_code(1)` → `_TEST_TERM_R1`.
///
/// Put it last in an act or agenda deck and advancing it ends the scenario at
/// `Resolution(n)`.
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
/// Composed into [`TEST_REGISTRY`], and into out-of-crate mocks the way
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

/// Metadata lookup for the synthetic `TEST_INV` investigator code.
///
/// Integration tests in `crates/game-core/tests/` build their own
/// mock `CardRegistry` rather than naming [`TEST_REGISTRY`].
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

/// game-core's test registry: it knows `TEST_INV`, the synthetic cardtype cards
/// ([`TEST_ASSET`], [`TEST_TREACHERY`]) and the synthetic terminal cards
/// ([`terminal_code`]), and nothing else.
///
/// Capacity-reading code (`max_health` / `max_sanity` / soak / defeat) needs
/// `TEST_INV`, and a fixture whose act/agenda deck ends in a terminal card
/// needs its reverse.
pub const TEST_REGISTRY: CardRegistry = CardRegistry {
    metadata_for: test_registry_metadata_for,
    abilities_for: test_registry_abilities_for,
    ..CardRegistry::EMPTY
};

/// The registries a game-core test runs against: [`TEST_REGISTRY`] and no
/// scenarios. Name this rather than hand-building the pair.
pub const TEST_REGISTRIES: Registries = Registries {
    cards: &TEST_REGISTRY,
    scenarios: &ScenarioRegistry::EMPTY,
};

fn test_registry_metadata_for(code: &CardCode) -> Option<&'static CardMetadata> {
    metadata_for_test_inv(code).or_else(|| metadata_for_test_cardtypes(code))
}

fn test_registry_abilities_for(code: &CardCode) -> Option<Vec<Ability>> {
    abilities_for_terminal(code).or_else(|| abilities_for_test_cardtypes(code))
}

pub use deal::assert_effect_deal;
pub use fixtures::{
    awaiting_commit_input, awaiting_confirm_input, awaiting_confirm_input_for,
    awaiting_pick_single_input, awaiting_pick_single_with, awaiting_request,
    awaiting_skippable_commit_input, awaiting_skippable_pick_single_input,
    awaiting_skippable_pick_single_with, from_frames_unchecked, test_enemy, test_investigator,
    test_investigator_card, test_location, test_skill_test,
};
pub use mock_registry::MockRegistry;
pub use native::run_native;
pub use resolver::{ChoiceResolver, ScriptedResolver};
pub use seat::{clues, doom, horror, seat, uses, CardSetup, Seat, SeatCard, SeatLookup};
pub use session::TestSession;
