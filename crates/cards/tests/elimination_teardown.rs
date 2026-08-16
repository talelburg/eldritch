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
    CardCode, CardInPlay, CardInstanceId, ChaosToken, InvestigatorId, LocationId, Status, Zone,
};
use game_core::test_support::{
    drive, test_investigator, test_location, GameStateBuilder, ScriptedResolver,
};
use game_core::{assert_event, assert_event_count, assert_no_event, Action, EngineOutcome};

/// Roland Banks — health 9, sanity 5.
const ROLAND: &str = "01001";
const GRASPING_HANDS: &str = "01162";
const OVERPOWER: &str = "01091";
const COVER_UP: &str = "01007";
const DISSONANT_VOICES: &str = "01165";

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
    state
        .encounter_deck
        .push_back(CardCode::new(GRASPING_HANDS));
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
    assert_eq!(
        inv.status,
        Status::Killed,
        "lethal damage eliminated Roland"
    );

    // RR p.10 step 1: the committed card is in limbo on the SkillTest frame
    // (#631), which `run_elimination_steps`' frame walk sweeps alongside
    // in-progress plays — so it is removed from the game, NOT discarded by the
    // skill-test teardown. Deleting that sweep arm turns this assertion red.
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

#[test]
fn elimination_removes_a_player_owned_weakness_from_the_game() {
    // RR p.10 step 1 + the design's reading: Cover Up is owned by Roland, whose
    // discard pile step 1 just removed from the game — so "the appropriate
    // discard pile" no longer exists and the card is removed.
    let r = reveal_committing(board_at_lethal_range(8, &[], &[(COVER_UP, 3)]), &[]);

    let inv = &r.state.investigators[&InvestigatorId(1)];
    assert_eq!(inv.status, Status::Killed);
    assert!(inv.threat_area.is_empty(), "threat area drained");
    assert!(
        inv.removed_from_game.iter().any(|c| c.as_str() == COVER_UP),
        "player-owned weakness removed from game; removed = {:?}",
        inv.removed_from_game
    );
    assert!(
        !r.state
            .encounter_discard
            .iter()
            .any(|c| c.as_str() == COVER_UP),
        "a player-owned weakness must NOT go to the encounter discard"
    );
}

#[test]
fn elimination_discards_an_encounter_treachery_to_the_encounter_discard() {
    // RR p.10 step 4: Dissonant Voices is owned by the scenario, so Roland's
    // elimination must not remove it from the game.
    let r = reveal_committing(board_at_lethal_range(8, &[], &[(DISSONANT_VOICES, 0)]), &[]);

    let inv = &r.state.investigators[&InvestigatorId(1)];
    assert_eq!(inv.status, Status::Killed);
    assert!(inv.threat_area.is_empty(), "threat area drained");
    assert!(
        r.state
            .encounter_discard
            .iter()
            .any(|c| c.as_str() == DISSONANT_VOICES),
        "encounter treachery goes to the encounter discard; discard = {:?}",
        r.state.encounter_discard
    );
    assert!(
        !inv.removed_from_game
            .iter()
            .any(|c| c.as_str() == DISSONANT_VOICES),
        "a scenario-owned card must NOT be removed from the game by an \
         investigator's elimination"
    );
    assert_event!(r.events, Event::CardDiscarded { code, from: Zone::ThreatArea, .. }
        if code.as_str() == DISSONANT_VOICES);
}

#[test]
fn elimination_routes_a_mixed_threat_area_both_ways() {
    let r = reveal_committing(
        board_at_lethal_range(8, &[], &[(COVER_UP, 3), (DISSONANT_VOICES, 0)]),
        &[],
    );

    let inv = &r.state.investigators[&InvestigatorId(1)];
    assert!(inv.threat_area.is_empty(), "threat area fully drained");
    assert!(inv.removed_from_game.iter().any(|c| c.as_str() == COVER_UP));
    assert!(r
        .state
        .encounter_discard
        .iter()
        .any(|c| c.as_str() == DISSONANT_VOICES));
}

#[test]
fn eliminated_investigator_fires_no_game_end_trauma() {
    // #567's acceptance: Cover Up's Forced ("When the game ends, if there are
    // any clues on Cover Up: You suffer 1 mental trauma") must not fire for a
    // dead Roland — RR p.10 step 1 took the card with him. Solo, so his death
    // latches Resolution::Lost and the game-end forced scan runs for real.
    let r = reveal_committing(board_at_lethal_range(8, &[], &[(COVER_UP, 3)]), &[]);

    // Both asserted so the no-trauma claim below can't pass vacuously: the
    // scenario must actually have ended for the GameEnd scan to have run at all.
    assert!(
        r.events
            .iter()
            .any(|e| matches!(e, Event::AllInvestigatorsDefeated)),
        "solo death latches Resolution::Lost; events = {:?}",
        r.events
    );
    assert!(
        r.events
            .iter()
            .any(|e| matches!(e, Event::ScenarioResolved { .. })),
        "the Lost latch must reach ScenarioResolved, which is what fires the \
         GameEnd forced scan (cf. crates/cards/tests/cover_up.rs, which pins the \
         positive case via AdvanceAct); events = {:?}",
        r.events
    );
    assert_no_event!(r.events, Event::TraumaSuffered { .. });
}

#[test]
fn eliminated_investigator_fires_no_further_round_end_forced() {
    // #567's acceptance: Dissonant Voices' Forced ("At the end of the round:
    // Discard Dissonant Voices") must not fire again for a dead investigator —
    // step 4 already discarded it.
    let mut r = reveal_committing(board_at_lethal_range(8, &[], &[(DISSONANT_VOICES, 0)]), &[]);
    assert_eq!(
        r.state.investigators[&InvestigatorId(1)].status,
        Status::Killed
    );
    let before = r.state.encounter_discard.len();

    let mut events = Vec::new();
    let _ = game_core::test_support::fire_forced_on_round_end(&mut r.state, &mut events);

    assert_eq!(
        r.state.encounter_discard.len(),
        before,
        "no further round-end forced for a dead investigator"
    );
    assert_no_event!(events, Event::CardDiscarded { .. });
}

#[test]
fn elimination_does_not_drain_the_investigator_card() {
    // The premise of the Status filter (#567): `investigator_card` is a
    // non-Option field carrying identity + harm (#448) and is read by
    // max_health(), so elimination cannot drain it. Without the filter it would
    // keep contributing GameEnd/RoundEnded forced candidates for a dead
    // investigator. No in-scope investigator card carries such a forced
    // (Roland's is a reaction), so the filter has no observable test today —
    // this pins the premise that makes it necessary.
    let r = reveal_committing(board_at_lethal_range(8, &[], &[]), &[]);

    let inv = &r.state.investigators[&InvestigatorId(1)];
    assert_eq!(inv.status, Status::Killed);
    assert_eq!(
        inv.investigator_card.code.as_str(),
        ROLAND,
        "the investigator card survives elimination — the premise of the \
         Status filter on the RoundEnded/GameEnd scans",
    );
}

#[test]
fn the_elimination_interleaving_replays_bit_for_bit() {
    // Deterministic re-drive: driving the same action sequence twice from an
    // identical initial state must land on an identical final state — the
    // property the persisted action log depends on. Before #564 this
    // interleaving panicked deterministically on every drive. (This mirrors the
    // repo's `deck_shuffle_is_deterministic_across_replay` idiom — two seeded
    // runs compared — rather than re-applying a recorded `Vec<Action>`; with the
    // single rigged chaos token no RNG divergence is possible either way.)
    let first = reveal_committing(
        board_at_lethal_range(8, &[OVERPOWER], &[(COVER_UP, 3)]),
        &[OVERPOWER],
    );
    let second = reveal_committing(
        board_at_lethal_range(8, &[OVERPOWER], &[(COVER_UP, 3)]),
        &[OVERPOWER],
    );

    assert_eq!(first.outcome, EngineOutcome::Done);
    assert_eq!(
        first.state, second.state,
        "the elimination interleaving must replay bit-for-bit"
    );
    assert_eq!(first.events, second.events, "events replay identically too");
}
