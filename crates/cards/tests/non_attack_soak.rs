//! K5b-2 (#44 / #422): non-attack damage/horror from card/treachery effects is
//! distributed **interactively** across controlled soakers + self. Each point a
//! soaker can take opens a per-point `PickSingle` prompt; the multi-point case
//! (Grasping Hands' `ForEachPointFailed(deal 1)`) suspends per point and resumes
//! without losing iterations — the case the old suspend-and-replay model dropped.
//! Driven through the real `apply` revelation path against the corpus registry.

use std::sync::Once;

use game_core::action::EngineRecord;
use game_core::engine::OptionId;
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

/// Reveal the top encounter card for investigator 1, committing no cards at the
/// revelation skill-test window, then answering each per-point distribution
/// prompt with `picks` (each an `OptionId` index into `[self, soaker, …]`).
fn reveal_distributing(state: game_core::GameState, picks: &[u32]) -> game_core::ApplyResult {
    let mut resolver = ScriptedResolver::new();
    resolver.commit_cards(&[]);
    for &p in picks {
        resolver.pick_single(OptionId(p));
    }
    drive(
        state,
        Action::Engine(EngineRecord::EncounterCardRevealed {
            investigator: InvestigatorId(1),
        }),
        resolver,
    )
}

fn dog_damage(result: &game_core::ApplyResult) -> Option<u8> {
    result.state.investigators[&InvestigatorId(1)]
        .cards_in_play
        .iter()
        .find(|c| c.instance_id == CardInstanceId(1))
        .map(|c| c.accumulated_damage)
}

#[test]
fn grasping_hands_distributes_both_points_onto_guard_dog() {
    install_registry();
    // Agility 3 + Numeric(-2) = 1 vs difficulty 3 → fail by 2 → 2 damage, dealt
    // as ForEachPointFailed(deal 1): two independent per-point suspensions. The
    // player assigns both to Guard Dog (option 1 = the asset; option 0 = self).
    // The old replay model lost the second point here — this proves no loss.
    let result = reveal_distributing(
        board_with_soaker("01162", "01021", ChaosToken::Numeric(-2)),
        &[1, 1],
    );
    assert_eq!(
        result.outcome,
        EngineOutcome::Done,
        "both points distributed; no soak reaction window for treachery harm",
    );
    let inv = &result.state.investigators[&InvestigatorId(1)];
    assert_eq!(inv.damage, 0, "investigator took none (both soaked)");
    assert_eq!(
        dog_damage(&result),
        Some(2),
        "2 damage soaked onto Guard Dog"
    );
}

#[test]
fn grasping_hands_player_splits_across_soaker_and_self() {
    install_registry();
    // Same 2-damage fail, but the player soaks one point onto Guard Dog (option
    // 1) and takes the other themselves (option 0). Under the old auto-soak (K5a)
    // both would land on the dog; the split proves the distribution is the
    // player's choice (K5b-2).
    let result = reveal_distributing(
        board_with_soaker("01162", "01021", ChaosToken::Numeric(-2)),
        &[1, 0],
    );
    assert_eq!(result.outcome, EngineOutcome::Done);
    let inv = &result.state.investigators[&InvestigatorId(1)];
    assert_eq!(inv.damage, 1, "one point taken by the investigator");
    assert_eq!(
        dog_damage(&result),
        Some(1),
        "one point soaked onto Guard Dog"
    );
}

#[test]
fn rotting_remains_horror_distributes_onto_beat_cop() {
    install_registry();
    // Willpower 3 + Numeric(-1) = 2 vs difficulty 3 → fail by 1 → 1 horror. Beat
    // Cop (sanity 2) is eligible, so the point is contested → one prompt; the
    // player assigns it to Beat Cop (option 1).
    let result = reveal_distributing(
        board_with_soaker("01163", "01018", ChaosToken::Numeric(-1)),
        &[1],
    );
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
        "1 horror soaked onto Beat Cop",
    );
}
