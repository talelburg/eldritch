//! K5a (#44): non-attack damage/horror from card/treachery effects soaks onto
//! controlled assets via the shared soak entry, like enemy attacks already did.
//! Driven through the real `apply` revelation path against the corpus registry.

use std::sync::Once;

use game_core::action::EngineRecord;
use game_core::state::{
    CardCode, CardInPlay, CardInstanceId, ChaosToken, InvestigatorId, LocationId,
};
use game_core::test_support::{
    drive, test_investigator, test_location, GameStateBuilder, ScriptedResolver,
};
use game_core::{Action, EngineOutcome};

static INSTALL: Once = Once::new();
fn install_registry() {
    INSTALL.call_once(|| {
        let _ = game_core::card_registry::install(cards::REGISTRY);
    });
}

/// Reveal the top encounter card for investigator 1, committing no cards at the
/// revelation skill-test window.
fn reveal_top(state: game_core::GameState) -> game_core::ApplyResult {
    let mut resolver = ScriptedResolver::new();
    resolver.commit_cards(&[]);
    drive(
        state,
        Action::Engine(EngineRecord::EncounterCardRevealed {
            investigator: InvestigatorId(1),
        }),
        resolver,
    )
}

/// Investigator 1 at a location, controlling `soaker` (instance 1), with
/// `treachery` on top of the encounter deck and one rigged chaos token.
fn board_with_soaker(treachery: &str, soaker: &str, token: ChaosToken) -> game_core::GameState {
    let mut inv = test_investigator(1);
    inv.cards_in_play = vec![CardInPlay::enter_play(
        CardCode::new(soaker),
        CardInstanceId(1),
    )];
    let mut state = GameStateBuilder::new()
        .with_investigator_at(inv, LocationId(20))
        .with_location(test_location(20, "Here"))
        .with_turn_order([InvestigatorId(1)])
        .build();
    state.chaos_bag.tokens = vec![token];
    state.encounter_deck.push_back(CardCode::new(treachery));
    state
}

#[test]
fn grasping_hands_damage_soaks_onto_guard_dog() {
    install_registry();
    // Agility 3 + Numeric(-2) = 1 vs difficulty 3 → fail by 2 → 2 damage.
    // Guard Dog (health 3) soaks both and survives; no soak reaction window
    // (Effect source — Guard Dog retaliates only to enemy *attacks*).
    let result = reveal_top(board_with_soaker("01162", "01021", ChaosToken::Numeric(-2)));
    assert_eq!(
        result.outcome,
        EngineOutcome::Done,
        "no soak reaction window for treachery harm"
    );
    let inv = &result.state.investigators[&InvestigatorId(1)];
    assert_eq!(inv.damage, 0, "damage soaked, investigator took none");
    let dog = inv
        .cards_in_play
        .iter()
        .find(|c| c.instance_id == CardInstanceId(1));
    assert_eq!(
        dog.map(|c| c.accumulated_damage),
        Some(2),
        "2 damage soaked onto Guard Dog"
    );
}

#[test]
fn rotting_remains_horror_soaks_onto_beat_cop() {
    install_registry();
    // Willpower 3 + Numeric(-1) = 2 vs difficulty 3 → fail by 1 → 1 horror.
    // Beat Cop (sanity 2) soaks it and survives (accumulated 1 < 2).
    let result = reveal_top(board_with_soaker("01163", "01018", ChaosToken::Numeric(-1)));
    assert_eq!(result.outcome, EngineOutcome::Done);
    let inv = &result.state.investigators[&InvestigatorId(1)];
    assert_eq!(inv.horror, 0, "horror soaked, investigator took none");
    let cop = inv
        .cards_in_play
        .iter()
        .find(|c| c.instance_id == CardInstanceId(1));
    assert_eq!(
        cop.map(|c| c.accumulated_horror),
        Some(1),
        "1 horror soaked onto Beat Cop"
    );
}
