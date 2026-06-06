//! Proves the engine's transactional guarantee: an action that mutates
//! state and then rejects mid-resolution leaves state AND events
//! byte-identical to the pre-action state.
//!
//! Own integration-test binary so it can install a *hand-rolled*
//! `CardRegistry` (a probe card whose OnPlay effect mutates then
//! rejects) without colliding with `game-core`'s registry-free unit
//! tests or the real-corpus `play_card.rs` binary.

use std::sync::OnceLock;

use game_core::card_data::{CardMetadata, CardType, Class, SkillIcons};
use game_core::card_registry::{self, CardRegistry};
use game_core::dsl::{gain_resources, modify, on_play, seq, Ability};
use game_core::dsl::{InvestigatorTarget, ModifierScope, Stat};
use game_core::engine::{apply, EngineOutcome};
use game_core::state::{CardCode, InvestigatorId, LocationId, Phase};
use game_core::test_support::{test_investigator, test_location, TestGame};
use game_core::{Action, PlayerAction};

/// Code for the synthetic probe card. Not in the real corpus; only the
/// hand-rolled registry below resolves it.
const PROBE: &str = "ROLLBACK1";

/// OnPlay that gains 2 resources (mutates) then runs a ThisTurn Modify,
/// which is an evaluator TODO stub that returns `Rejected` — producing a
/// mid-resolution reject after a committed mutation.
fn probe_abilities(code: &CardCode) -> Option<Vec<Ability>> {
    if code.as_str() != PROBE {
        return None;
    }
    Some(vec![on_play(seq([
        gain_resources(InvestigatorTarget::Active, 2),
        modify(Stat::Willpower, 1, ModifierScope::ThisTurn),
    ]))])
}

fn probe_metadata_static() -> &'static CardMetadata {
    static M: OnceLock<CardMetadata> = OnceLock::new();
    M.get_or_init(|| CardMetadata {
        code: PROBE.to_string(),
        name: "Rollback Probe".to_string(),
        class: Class::Neutral,
        card_type: CardType::Asset,
        cost: Some(0),
        xp: Some(0),
        text: None,
        flavor: None,
        illustrator: None,
        traits: vec![],
        slots: vec![],
        skill_icons: SkillIcons::default(),
        health: None,
        sanity: None,
        deck_limit: 2,
        quantity: 1,
        pack_code: "test".to_string(),
        position: 1,
        is_fast: false,
        spawn: None,
        surge: false,
        peril: false,
    })
}

fn probe_metadata(code: &CardCode) -> Option<&'static CardMetadata> {
    if code.as_str() == PROBE {
        Some(probe_metadata_static())
    } else {
        None
    }
}

/// Install the hand-rolled probe registry once for this binary.
fn install_probe_registry() {
    let _ = card_registry::install(CardRegistry {
        metadata_for: probe_metadata,
        abilities_for: probe_abilities,
    });
}

#[test]
fn mid_resolution_reject_leaves_state_and_events_untouched() {
    install_probe_registry();

    let id = InvestigatorId(1);
    let loc_id = LocationId(101);
    let mut inv = test_investigator(1);
    inv.current_location = Some(loc_id);
    inv.hand = vec![CardCode::new(PROBE)];

    let state = TestGame::new()
        .with_phase(Phase::Investigation)
        .with_investigator(inv)
        .with_active_investigator(id)
        .with_location(test_location(101, "Study"))
        .build();

    // Capture the pre-action state to compare against byte-for-byte.
    let before = state.clone();

    let result = apply(
        state,
        Action::Player(PlayerAction::PlayCard {
            investigator: id,
            hand_index: 0,
        }),
    );

    assert!(
        matches!(result.outcome, EngineOutcome::Rejected { .. }),
        "ThisTurn Modify stub should reject, got {:?}",
        result.outcome,
    );
    assert_eq!(
        result.state, before,
        "rejected play must leave state byte-identical (resources, hand, cards_in_play)",
    );
    assert!(
        result.events.is_empty(),
        "rejected play must emit no events, got {:?}",
        result.events,
    );
}
