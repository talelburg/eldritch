//! Elimination teardown (#564, #567): an eliminated investigator stops driving
//! the engine — their in-flight skill test is abandoned rather than resolved
//! against the hand elimination drained, and their threat area is emptied to the
//! right pile.
//!
//! Lives in `crates/cards/tests/` because every assertion needs real card
//! metadata (`CardMetadata::weakness`) and abilities — `game-core` can't reach
//! the corpus by crate direction, and `install_test_registry` resolves metadata
//! for `TEST_INV` only.
//!
//! Elimination is reached through the **real** path (a lethal Grasping Hands
//! revelation driven via `apply`); `apply_investigator_defeat` stays `pub(super)`.
//!
//! ## Verified card text (`data/arkhamdb-snapshot`, 2026-07-17)
//!
//! **Grasping Hands (01162):** "<b>Revelation</b> - Test [agility] (3). If you
//! fail, take 1 damage for each point you failed by."
//! **Overpower (01091):** a plain skill card — two [combat] icons, no triggered
//! ability. Contributes 0 to an agility test, so committing it leaves the fail
//! margin intact while still exercising the committed-index path.
//! **Cover Up (01007):** "<b>Revelation</b> - Put Cover Up into play in your
//! threat area, with 3 clues on it. […] <b>Forced</b> - When the game ends, if
//! there are any clues on Cover Up: You suffer 1 mental trauma."
//! **Dissonant Voices (01165):** "<b>Revelation</b> - Put Dissonant Voices into
//! play in your threat area. You cannot play assets or events. <b>Forced</b> -
//! At the end of the round: Discard Dissonant Voices."

use game_core::action::EngineRecord;
use game_core::event::Event;
use game_core::state::{
    CardCode, CardInPlay, CardInstanceId, ChaosToken, InvestigatorId, LocationId, Status,
};
use game_core::test_support::{
    drive, test_investigator, test_location, GameStateBuilder, ScriptedResolver,
};
use game_core::{assert_event_count, Action, EngineOutcome};

/// Roland Banks — health 9, sanity 5.
const ROLAND: &str = "01001";
const GRASPING_HANDS: &str = "01162";
const OVERPOWER: &str = "01091";

#[ctor::ctor(unsafe)]
fn install_registry() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
}

/// Roland at a location with `damage` already on him, `hand` in hand, and
/// `threat` (code, clues) in his threat area (instance ids 1, 2, …). Grasping
/// Hands sits on top of the encounter deck with a rigged `Numeric(-2)` token, so
/// `reveal_committing` puts him through an Agility(3) test he fails by 2.
fn board_at_lethal_range(damage: u8, hand: &[&str], threat: &[(&str, u8)]) -> game_core::GameState {
    let mut inv = test_investigator(1);
    // Real investigator code so max_health() reads from the installed cards
    // registry (#448 cp2a). Roland Banks (01001, 9/5).
    inv.investigator_card.code = CardCode::new(ROLAND);
    inv.investigator_card.accumulated_damage = damage;
    inv.hand = hand.iter().map(|c| CardCode::new(*c)).collect();
    inv.threat_area = threat
        .iter()
        .enumerate()
        .map(|(i, (code, clues))| {
            let mut card = CardInPlay::enter_play(
                CardCode::new(*code),
                CardInstanceId(u32::try_from(i).expect("fits") + 1),
            );
            card.clues = *clues;
            card
        })
        .collect();
    let mut state = GameStateBuilder::new()
        .with_investigator_at(inv, LocationId(20))
        .with_location(test_location(20, "Here"))
        .with_turn_order([InvestigatorId(1)])
        .build();
    state.chaos_bag.tokens = vec![ChaosToken::Numeric(-2)];
    state.encounter_deck.push_back(CardCode::new(GRASPING_HANDS));
    state
}

/// Reveal the top encounter card for investigator 1, committing `commit` at the
/// revelation skill-test window.
fn reveal_committing(state: game_core::GameState, commit: &[&str]) -> game_core::ApplyResult {
    let mut resolver = ScriptedResolver::new();
    resolver.commit_cards(&commit.iter().map(|c| CardCode::new(*c)).collect::<Vec<_>>());
    drive(
        state,
        Action::Engine(EngineRecord::EncounterCardRevealed {
            investigator: InvestigatorId(1),
        }),
        resolver,
    )
}

#[test]
fn tester_eliminated_mid_test_abandons_the_test_without_panicking() {
    // Agility 3 + Numeric(-2) = 1 vs difficulty 3 → fail by 2 → 2 damage.
    // Roland at 8/9 damage → lethal → elimination drains the hand while the
    // SkillTest frame is still live at FireOnResolution / the teardown discard.
    let r = reveal_committing(board_at_lethal_range(8, &[OVERPOWER], &[]), &[OVERPOWER]);

    assert_eq!(r.outcome, EngineOutcome::Done, "test abandoned cleanly");

    let inv = &r.state.investigators[&InvestigatorId(1)];
    assert_eq!(inv.status, Status::Killed, "lethal damage eliminated Roland");

    // RR p.10 step 1: the committed card was in hand, so it is removed from the
    // game — NOT discarded by the skill-test teardown.
    assert!(inv.hand.is_empty(), "hand drained by elimination");
    assert!(
        inv.discard.is_empty(),
        "committed card must not be discarded after elimination; discard = {:?}",
        inv.discard
    );
    assert!(
        inv.removed_from_game
            .iter()
            .any(|c| c.as_str() == OVERPOWER),
        "committed card removed from game; removed = {:?}",
        inv.removed_from_game
    );

    // The frame is gone and the test closed exactly once.
    assert!(
        !r.state.has_skill_test_in_flight(),
        "SkillTest frame torn down"
    );
    assert_event_count!(r.events, 1, Event::SkillTestEnded { .. });
}

#[test]
fn surviving_tester_still_discards_committed_cards() {
    // Control: same board, no preloaded damage → 2 damage is survivable → the
    // normal teardown runs and the committed card goes to the discard.
    let r = reveal_committing(board_at_lethal_range(0, &[OVERPOWER], &[]), &[OVERPOWER]);

    let inv = &r.state.investigators[&InvestigatorId(1)];
    assert_eq!(inv.status, Status::Active, "2 damage is not lethal at 0/9");
    assert!(
        inv.discard.iter().any(|c| c.as_str() == OVERPOWER),
        "surviving tester discards committed cards; discard = {:?}",
        inv.discard
    );
    assert_event_count!(r.events, 1, Event::SkillTestEnded { .. });
}
