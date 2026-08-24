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
//! **Survival Instinct (01081):** "If this skill test is successful during an
//! evasion attempt, the evading investigator may move to a connecting
//! location." One [agility] icon, so it is legally committable to Grasping
//! Hands' agility test (RR ST.2, #763) while its ability stays inert — the
//! test fails, and this is a revelation rather than an evasion attempt.
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
const SURVIVAL_INSTINCT: &str = "01081";
const COVER_UP: &str = "01007";
const DISSONANT_VOICES: &str = "01165";

#[ctor::ctor(unsafe)]
fn install_registry() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
}

/// Roland at a location with `damage` already on him, `hand` in hand, and
/// `threat` (code, clues) in his threat area (instance ids 1, 2, …). Grasping
/// Hands sits on top of the encounter deck with a rigged `Numeric(-2)` token, so
/// `reveal_committing` puts him through an Agility(3) test he fails by 2 — or by
/// 1 when Survival Instinct's single [agility] icon is committed.
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

/// [`board_at_lethal_range`] plus a second, healthy investigator at another
/// location — so investigator 1's death eliminates *him* without ending the
/// scenario. The survivor carries a real investigator code because
/// `max_health()` reads capacity from the installed corpus registry (#448); he
/// is a stand-in whose only job is to keep the game running.
fn board_with_survivor(damage: u8, threat: &[(&str, u8)]) -> game_core::GameState {
    let mut state = board_at_lethal_range(damage, &[], threat);
    let mut survivor = test_investigator(2);
    survivor.investigator_card.code = CardCode::new(ROLAND);
    survivor.current_location = Some(LocationId(21));
    state
        .locations
        .insert(LocationId(21), test_location(21, "Elsewhere"));
    state.investigators.insert(InvestigatorId(2), survivor);
    state.turn_order = vec![InvestigatorId(1), InvestigatorId(2)];
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
    // Agility 3 + Numeric(-2) + 1 committed [agility] icon = 2 vs difficulty 3 →
    // fail by 1 → 1 damage. Roland at 8/9 damage → lethal → elimination drains the hand while the
    // SkillTest frame is still live at FireOnResolution / the teardown discard.
    let r = reveal_committing(
        board_at_lethal_range(8, &[SURVIVAL_INSTINCT], &[]),
        &[SURVIVAL_INSTINCT],
    );

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
            .any(|c| c.as_str() == SURVIVAL_INSTINCT),
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
    // Control: same board, no preloaded damage → 1 damage is survivable → the
    // normal teardown runs and the committed card goes to the discard.
    let r = reveal_committing(
        board_at_lethal_range(0, &[SURVIVAL_INSTINCT], &[]),
        &[SURVIVAL_INSTINCT],
    );

    let inv = &r.state.investigators[&InvestigatorId(1)];
    assert_eq!(inv.status, Status::Active, "1 damage is not lethal at 0/9");
    assert!(
        inv.discard.iter().any(|c| c.as_str() == SURVIVAL_INSTINCT),
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
fn eliminated_investigator_fires_cover_ups_game_end_trauma() {
    // Rules Reference p.10 Elimination **step 0** (#638):
    //
    // > For the purpose of resolving weakness cards, the game has ended for the
    // > eliminated investigator. Trigger any "when the game ends" abilities on
    // > each weakness the eliminated investigator owns that is in play. Then,
    // > remove those weaknesses from the game.
    //
    // and Cover Up's own ruling, <https://arkhamdb.com/card/01007>: "If Roland
    // is eliminated (by being defeated or taking a resign action) while Cover Up
    // is in play, Cover Up's Forced effect triggers, as per the FAQ [V1.0,
    // section 'Rulebook errata', topic "Elimination"]."
    //
    // **Supersedes #567's acceptance criterion** ("Eliminated investigator's
    // Cover Up does not fire GameEnd trauma"), which read step 1 without step 0
    // — the step that exists precisely to carve weaknesses out of it.
    //
    // Since #720 this is also the regression guard for the **fork predicate**.
    // `EliminationGameEnd` became a coordinator-owned bare milestone alongside
    // `GameEnd`, and Cover Up's declaration retagged to the `when` cell; a
    // `has_weakness_game_end_ability` that scans one hardcoded cell then finds
    // nothing, routes elimination down its inline path, and lets steps 1–6
    // remove Cover Up without ever firing it — a silent drop with no reject to
    // show for it. This test and the multiplayer one below are what caught it.
    let r = reveal_committing(board_at_lethal_range(8, &[], &[(COVER_UP, 3)]), &[]);

    assert_event!(r.events, Event::TraumaSuffered {
        investigator, kind: game_core::event::TraumaKind::Mental, amount: 1
    } if *investigator == InvestigatorId(1));
    // Exactly once. Solo, so the death also latches Resolution::Lost and the
    // ordinary scenario-end `GameEnd` scan runs — it must not fire this a second
    // time (Cover Up has left play by then, and that scan skips non-Active
    // investigators anyway, #567).
    assert_event_count!(r.events, 1, Event::TraumaSuffered { .. });
    assert!(
        r.events
            .iter()
            .any(|e| matches!(e, Event::ScenarioResolved { .. })),
        "the scenario-end GameEnd path must still have run, so the exactly-once \
         assertion above is not vacuous; events = {:?}",
        r.events
    );

    // Step 0's tail — "Then, remove those weaknesses from the game" — still runs.
    let inv = &r.state.investigators[&InvestigatorId(1)];
    assert!(inv.threat_area.is_empty(), "threat area drained");
    assert!(inv.removed_from_game.iter().any(|c| c.as_str() == COVER_UP));
}

#[test]
fn cover_ups_trauma_fires_on_elimination_while_the_scenario_continues() {
    // The multiplayer half of the hole (#638): the scenario does *not* end, so
    // the ordinary `GameEnd` path never runs at all — the trauma can only come
    // from Elimination step 0. RR p.10: "For the purpose of resolving weakness
    // cards, the game has ended for the eliminated investigator."
    let r = reveal_committing(board_with_survivor(8, &[(COVER_UP, 3)]), &[]);

    assert_eq!(
        r.state.investigators[&InvestigatorId(1)].status,
        Status::Killed
    );
    assert_eq!(
        r.state.investigators[&InvestigatorId(2)].status,
        Status::Active,
        "the survivor keeps the scenario running",
    );
    assert_no_event!(r.events, Event::AllInvestigatorsDefeated);
    assert_no_event!(r.events, Event::ScenarioResolved { .. });

    assert_event!(r.events, Event::TraumaSuffered {
        investigator, kind: game_core::event::TraumaKind::Mental, amount: 1
    } if *investigator == InvestigatorId(1));
}

#[test]
fn eliminated_investigator_with_a_clueless_cover_up_suffers_no_trauma() {
    // The Forced's own condition: "if there are any clues on Cover Up". Step 0
    // fires the ability either way; with no clues it resolves to nothing. The
    // control that keeps the test above honest about *why* the trauma landed.
    let r = reveal_committing(board_at_lethal_range(8, &[], &[(COVER_UP, 0)]), &[]);

    assert_eq!(
        r.state.investigators[&InvestigatorId(1)].status,
        Status::Killed,
        "the elimination still happened",
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
        board_at_lethal_range(8, &[SURVIVAL_INSTINCT], &[(COVER_UP, 3)]),
        &[SURVIVAL_INSTINCT],
    );
    let second = reveal_committing(
        board_at_lethal_range(8, &[SURVIVAL_INSTINCT], &[(COVER_UP, 3)]),
        &[SURVIVAL_INSTINCT],
    );

    assert_eq!(first.outcome, EngineOutcome::Done);
    assert_eq!(
        first.state, second.state,
        "the elimination interleaving must replay bit-for-bit"
    );
    assert_eq!(first.events, second.events, "events replay identically too");
}
