//! End-to-end coverage of the general skill-test-outcome timing point
//! (`TimingEvent::SkillTestResolved`, Slice D #423): it fires for *every*
//! resolved test, not just a successful Investigate, and opens no window when
//! nothing listens.
//!
//! Own integration-test process (separate `OnceLock<CardRegistry>`) so it can
//! install a mock registry without colliding with other `tests/*.rs`. Mirrors
//! `on_skill_test_resolution.rs`.

use std::sync::OnceLock;

use game_core::card_data::CardMetadata;
use game_core::card_registry::CardRegistry;
use game_core::dsl::{
    deal_horror, forced_on_event, Ability, EventPattern, EventTiming, InvestigatorTarget,
    TestOutcome,
};
use game_core::engine::EngineOutcome;
use game_core::event::Event;
use game_core::state::{
    CardCode, CardInPlay, CardInstanceId, ChaosBag, ChaosToken, InvestigatorId, LocationId, Phase,
    SkillKind, TokenModifiers,
};
use game_core::test_support::{
    apply_no_commits, test_investigator, test_location, GameStateBuilder,
};
use game_core::{assert_event, Action, PlayerAction};

/// Mock threat-area card: a **forced** ability keyed to *any* successful skill
/// test (`kind: None`), dealing 1 horror to the controller. Forced (not a
/// reaction) so it fires automatically in `emit_event`'s forced phase — no
/// reaction-window resolution needed to observe the timing point.
const ANY_SUCCESS_FORCED: &str = "MOCK-STR-ANY-SUCCESS";

fn mock_metadata_for(_: &CardCode) -> Option<&'static CardMetadata> {
    None
}

fn mock_abilities_for(code: &CardCode) -> Option<Vec<Ability>> {
    (code.as_str() == ANY_SUCCESS_FORCED).then(|| {
        vec![forced_on_event(
            EventPattern::SkillTestResolved {
                outcome: TestOutcome::Success,
                kind: None,
            },
            EventTiming::After,
            deal_horror(InvestigatorTarget::You, 1u8),
        )]
    })
}

fn install_mock_registry() {
    static INSTALL: OnceLock<()> = OnceLock::new();
    INSTALL.get_or_init(|| {
        let _ = game_core::card_registry::install(CardRegistry {
            metadata_for: mock_metadata_for,
            abilities_for: mock_abilities_for,
            native_effect_for: |_| None,
        });
    });
}

/// A bare `PerformSkillTest` (kind = Plain) against intellect.
fn plain_intellect_test(id: InvestigatorId, difficulty: i8) -> Action {
    Action::Player(PlayerAction::PerformSkillTest {
        investigator: id,
        skill: SkillKind::Intellect,
        difficulty,
    })
}

/// Build a state with the investigator at `LocationId(10)`, a single-`Numeric(0)`
/// chaos bag, and the given threat-area card codes in play.
fn state_with_threat_area(threat: &[&str]) -> (game_core::GameState, InvestigatorId) {
    install_mock_registry();
    let id = InvestigatorId(1);
    let mut inv = test_investigator(1);
    inv.current_location = Some(LocationId(10));
    for (i, code) in threat.iter().enumerate() {
        inv.threat_area.push(CardInPlay::enter_play(
            CardCode::new(*code),
            CardInstanceId(u32::try_from(i).unwrap() + 1),
        ));
    }
    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_active_investigator(id)
        .with_investigator(inv)
        .with_location(test_location(10, "Study"))
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .build();
    (state, id)
}

#[test]
fn general_timing_point_fires_for_non_investigate_test() {
    // A bare PerformSkillTest is kind = Plain (not Investigate). Base intellect
    // 3 + Numeric(0) = 3 >= difficulty 2 -> success. The forced ability keyed to
    // `SkillTestResolved { Success, kind: None }` must fire on this Plain test.
    let (state, id) = state_with_threat_area(&[ANY_SUCCESS_FORCED]);
    let result = apply_no_commits(state, plain_intellect_test(id, 2));

    assert_eq!(result.outcome, EngineOutcome::Done);
    assert_event!(
        result.events,
        Event::SkillTestSucceeded { investigator, skill: SkillKind::Intellect, .. }
            if *investigator == id
    );
    assert_eq!(
        result.state.investigators[&id].horror, 1,
        "the general SkillTestResolved timing point must fire the forced ability \
         on a passing Plain (non-Investigate) test",
    );
}

#[test]
fn general_timing_point_opens_no_window_without_a_listener() {
    // A passing Plain test with no listening card resolves straight to Done and
    // takes no horror — generalizing the emit adds no spurious window/prompt.
    let (state, id) = state_with_threat_area(&[]);
    let result = apply_no_commits(state, plain_intellect_test(id, 2));

    assert_eq!(
        result.outcome,
        EngineOutcome::Done,
        "a plain test with no listener opens no window",
    );
    assert_event!(
        result.events,
        Event::SkillTestSucceeded { investigator, .. } if *investigator == id
    );
    assert_eq!(result.state.investigators[&id].horror, 0);
}
