//! Proves the engine's transactional guarantee: an action that mutates
//! state and then rejects mid-resolution leaves state AND events
//! byte-identical to the pre-action state.
//!
//! Own integration-test binary so it can install a *hand-rolled*
//! `CardRegistry` (a probe card whose `OnPlay` effect mutates then
//! rejects) without colliding with `game-core`'s registry-free unit
//! tests or the real-corpus `play_card.rs` binary.

use std::sync::OnceLock;

use game_core::card_data::{CardKind, CardMetadata, Class, SkillIcons};
use game_core::card_registry::{self, CardRegistry};
use game_core::dsl::{gain_resources, modify, on_play, seq, Ability};
use game_core::dsl::{InvestigatorTarget, ModifierScope, Stat};
use game_core::engine::{EngineOutcome, TurnAction};
use game_core::state::{CardCode, InvestigatorId, LocationId, Phase};
use game_core::test_support::{
    dispatch_turn_action_unchecked, test_investigator, test_location, GameStateBuilder,
};

/// Code for the synthetic probe card. Not in the real corpus; only the
/// hand-rolled registry below resolves it.
const PROBE: &str = "ROLLBACK1";

/// `OnPlay` that gains 2 resources (mutates) then runs a `ThisTurn` Modify,
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

fn probe_metadata(code: &CardCode) -> Option<&'static CardMetadata> {
    static M: OnceLock<CardMetadata> = OnceLock::new();
    if code.as_str() != PROBE {
        return None;
    }
    Some(M.get_or_init(|| CardMetadata {
        code: PROBE.to_string(),
        name: "Rollback Probe".to_string(),
        text: None,
        traits: vec![],
        pack_code: "test".to_string(),
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
    }))
}

/// Install the hand-rolled probe registry for this binary. `install` is
/// idempotent at the `OnceLock` level (first call wins; later calls
/// return `Err`, discarded here), so no `Once` guard is needed for this
/// single-test binary.
fn install_probe_registry() {
    let _ = card_registry::install(CardRegistry {
        metadata_for: probe_metadata,
        abilities_for: probe_abilities,
        native_effect_for: |_| None,
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

    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator(inv)
        .with_active_investigator(id)
        .with_location(test_location(101, "Study"))
        .build();

    // Capture the pre-action state to compare against byte-for-byte.
    let before = state.clone();

    let result = dispatch_turn_action_unchecked(
        state,
        &TurnAction::PlayCard {
            investigator: id,
            hand_index: 0,
        },
    );

    assert!(
        matches!(result.outcome, EngineOutcome::Rejected { .. }),
        "ThisTurn Modify stub should reject, got {:?}",
        result.outcome,
    );
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
