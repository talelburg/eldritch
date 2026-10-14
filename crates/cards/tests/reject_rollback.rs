//! Proves the engine's transactional guarantee: an action that mutates
//! state and then rejects mid-resolution leaves state AND events
//! byte-identical to the pre-action state.
//!
//! Own integration-test binary so it can build its own `MockRegistry` (a
//! probe card whose `OnPlay` effect mutates then rejects) without colliding
//! with `game-core`'s registry-free unit tests or the real-corpus
//! `play_card.rs` binary.

use card_dsl::card_data::{CardKind, CardMetadata, Class, SkillIcons};
use card_dsl::dsl::{self, InvestigatorTarget, ModifierScope, Stat};
use game_core::engine::enumerate::TurnAction;
use game_core::engine::Registries;
use game_core::scenario::ScenarioRegistry;
use game_core::state::{GameStateBuilder, InvestigatorId, LocationId};
use game_core::test_support::{self, seat, MockRegistry};
use std::sync::LazyLock;

/// Code for the synthetic probe card. Not in the real corpus; only the mock
/// registry below resolves it.
const PROBE: &str = "ROLLBACK1";

fn probe_metadata() -> CardMetadata {
    CardMetadata {
        code: PROBE.to_string(),
        name: "Rollback Probe".to_string(),
        text: None,
        traits: vec![],
        back_name: None,
        back_text: None,
        pack_code: "test".to_string(),
        weakness: false,
        kind: CardKind::Asset {
            class: Class::Neutral,
            cost: Some(0),
            xp: Some(0),
            slots: vec![],
            health: None,
            sanity: None,
            skill_icons: SkillIcons::default(),
            is_fast: false,
            deck_limit: 2,
            uses: None,
            play_only_during_turn: false,
        },
    }
}

/// The binary's one mock, built once on first use.
fn registries() -> Registries {
    static REGISTRIES: LazyLock<Registries> = LazyLock::new(|| Registries {
        cards: MockRegistry::new()
            .with_card(probe_metadata())
            // `OnPlay` that gains 2 resources (mutates) then runs a `ThisTurn`
            // Modify, which is an evaluator TODO stub that returns `Rejected` —
            // producing a mid-resolution reject after a committed mutation.
            .with_abilities(PROBE, || {
                vec![dsl::on_play(dsl::seq([
                    dsl::gain_resources(InvestigatorTarget::Active, 2),
                    dsl::modify(Stat::Willpower, 1, ModifierScope::ThisTurn),
                ]))]
            })
            .build(),
        scenarios: &ScenarioRegistry::EMPTY,
    });
    *REGISTRIES
}

#[test]
fn mid_resolution_reject_leaves_state_and_events_untouched() {
    let id = InvestigatorId(1);
    let here = LocationId(101);
    let session = GameStateBuilder::new()
        .with_registries(registries())
        .with_seat(seat(1).at(here).hand([PROBE]))
        .with_location(test_support::test_location(here.0, "Study"))
        .open_turn(id)
        .session();

    // Capture the pre-action state to compare against byte-for-byte.
    let before = session.state().clone();

    let session = session.take(&TurnAction::PlayCard {
        investigator: id,
        hand_index: 0,
    });

    let reason = session.expect_rejected();
    assert!(
        !reason.is_empty(),
        "ThisTurn Modify stub should reject: {reason}",
    );
    let result = session.finish();
    assert_eq!(
        result.state, before,
        "rejected play must leave state byte-identical \
         (pre-fix: OnPlay GainResources commits +2 before the reject)",
    );
    assert!(
        result.events.is_empty(),
        "rejected play must emit no events, got {:?}",
        result.events,
    );
}
