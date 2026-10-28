//! A revealed chaos-symbol token's [`TokenOutcome`] end to end, with a mock
//! scenario module whose symbol table exercises each part.
//!
//! RR ST.7 (`data/rules-reference/rules/Appendix_II_Timing_and_Gameplay.md`):
//!
//! > The card ability or game rule that initiated a skill test usually
//! > indicates the consequences of success and/or failure for that test.
//! > (Additionally, some other card abilities may contribute additional
//! > consequences, or modify existing consequences, at this time.) Resolve the
//! > appropriate consequences (based on the success or failure established
//! > during step ST.6) at this time.
//!
//! A token's success or failure part is one of those consequences, resolved
//! in the same ST.7 step for a scenario symbol as for a card's own result, so
//! it reads the failure margin the same way.
//!
//! Its own integration-test binary, so its `MockRegistry` and mock scenario
//! registry get a process to themselves. Every assertion is on events crossing
//! the `apply` boundary.

use std::sync::LazyLock;

use card_dsl::dsl::{self, Effect, HarmKind, IntExpr, InvestigatorTarget, Quantity, TokenOutcome};
use game_core::engine::Registries;
use game_core::event::Event;
use game_core::scenario::{
    ScenarioEnding, ScenarioId, ScenarioModule, ScenarioRegistry, SymbolCtx,
};
use game_core::state::{
    ChaosBag, ChaosToken, GameState, GameStateBuilder, InvestigatorId, LocationId, SkillKind,
    TokenModifiers, TokenResolution,
};
use game_core::test_support::{self, seat, MockRegistry, TestSession};
use game_core::{assert_event, assert_no_event};

const SCENARIO: &str = "token-result-mock";
const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(10);

/// The mock symbol table, one part per token:
///
/// - `[skull]`: a live modifier, `+1 for each clue on your location`.
/// - `[cultist]`: `+0`, and on failure 1 horror per point the test failed by.
/// - `[tablet]`: `+0`, and on success gain 2 resources.
fn mock_resolve_symbol(token: ChaosToken, _ctx: &SymbolCtx) -> TokenOutcome {
    match token {
        ChaosToken::Skull => {
            TokenOutcome::modifier(IntExpr::Count(Quantity::CluesAtControllerLocation))
        }
        ChaosToken::Cultist => TokenOutcome {
            on_fail: Some(Effect::Deal {
                kind: HarmKind::Horror,
                target: InvestigatorTarget::You,
                amount: IntExpr::Count(Quantity::SkillTestFailedBy),
                direct: false,
            }),
            ..TokenOutcome::default()
        },
        ChaosToken::Tablet => TokenOutcome {
            on_success: Some(dsl::gain_resources(InvestigatorTarget::You, 2)),
            ..TokenOutcome::default()
        },
        _ => TokenOutcome::default(),
    }
}

fn mock_setup() -> GameState {
    GameStateBuilder::new().build()
}

fn mock_apply_resolution(_: ScenarioEnding, _: &mut GameState, _: &mut Vec<Event>) {}

static MODULE: ScenarioModule = ScenarioModule {
    resolve_symbol: Some(mock_resolve_symbol),
    setup: mock_setup,
    apply_resolution: mock_apply_resolution,
    layout: &[],
};

fn module_for(id: &ScenarioId) -> Option<&'static ScenarioModule> {
    (id.as_str() == SCENARIO).then_some(&MODULE)
}

fn registries() -> Registries {
    static REGISTRIES: LazyLock<Registries> = LazyLock::new(|| Registries {
        // `build` composes `metadata_for_test_inv`, so the horror the cultist
        // deals reads the fixture investigator's sanity through the registry.
        cards: MockRegistry::new().build(),
        scenarios: &ScenarioRegistry { module_for },
    });
    *REGISTRIES
}

/// A willpower-3 investigator at `HERE` (holding `clues` clues), drawing only
/// `token`, takes a difficulty-`difficulty` willpower test and commits nothing.
fn perform(token: ChaosToken, clues: u8, difficulty: i8) -> TestSession {
    let mut here = test_support::test_location(HERE.0, "Here");
    here.clues = clues;
    let state = GameStateBuilder::new()
        .with_registries(registries())
        .with_seat(seat(1).skill(SkillKind::Willpower, 3).at(HERE))
        .with_location(here)
        .with_chaos_bag(ChaosBag::new([token]))
        .with_token_modifiers(TokenModifiers::default())
        .with_scenario_id(ScenarioId::new(SCENARIO))
        .open_turn(INV)
        .build();
    let session = TestSession::new(state, registries())
        .test_skill(INV, SkillKind::Willpower, difficulty)
        .commit(&[]);
    session.assert_at_turn_menu(INV);
    session
}

#[test]
fn a_symbols_failure_part_reads_the_failure_margin() {
    // Willpower 3 + 0 against 5: failed by 2, so 2 horror.
    let session = perform(ChaosToken::Cultist, 0, 5);
    assert_event!(
        session.events(),
        Event::HorrorPlacedOnInvestigator {
            investigator: INV,
            amount: 2,
        }
    );
}

#[test]
fn a_symbols_success_part_resolves_only_on_success() {
    let pass = perform(ChaosToken::Tablet, 0, 3);
    assert_event!(
        pass.events(),
        Event::ResourcesGained {
            investigator: INV,
            amount: 2,
        }
    );
    let fail = perform(ChaosToken::Tablet, 0, 4);
    assert_no_event!(fail.events(), Event::ResourcesGained { .. });
}

#[test]
fn a_symbols_live_modifier_is_announced_at_its_value_on_reveal() {
    // Two clues at the location: the skull announces +2, and the test (3 + 2
    // against 5) passes on it.
    let session = perform(ChaosToken::Skull, 2, 5);
    assert_event!(
        session.events(),
        Event::ChaosTokenRevealed {
            token: ChaosToken::Skull,
            resolution: TokenResolution::Modifier(2),
        }
    );
    assert_event!(
        session.events(),
        Event::SkillTestSucceeded {
            investigator: INV,
            margin: 0,
            ..
        }
    );
}
