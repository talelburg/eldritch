//! Elimination teardown (#564, #567): an eliminated investigator stops driving
//! the engine — their in-flight skill test is abandoned rather than resolved
//! against the hand elimination drained, and their threat area is emptied to the
//! right pile.
//!
//! Lives in `crates/cards/tests/` because every assertion needs real card
//! abilities (Grasping Hands' revelation, Cover Up's step-0 Forced) — `game-core`
//! can't reach the corpus by crate direction, and `install_test_registry`
//! resolves metadata for `TEST_INV` only. The threat area is routed by each
//! card's owner, not by its metadata (#982).
//!
//! Elimination is reached through the **real** path (a lethal Grasping Hands
//! revelation, drawn at the Mythos phase's encounter draw);
//! `apply_investigator_elimination` stays `pub(super)`.
//!
//! ## Verified card text (`data/arkhamdb-snapshot`, 2026-07-17)
//!
//! **Grasping Hands (01162):** "<b>Revelation</b> - Test [agility] (3). If you
//! fail, take 1 damage for each point you failed by."
//! **Survival Instinct (01081):** "If this skill test is successful during an
//! evasion attempt, the evading investigator may immediately disengage from
//! each other enemy engaged with him or her, and may move to a connecting
//! location." One [agility] icon, so it is legally committable to Grasping
//! Hands' agility test (RR ST.2, #763) while its ability stays inert — the
//! test fails, and this is a revelation rather than an evasion attempt.
//! **Cover Up (01007):** "<b>Revelation</b> - Put Cover Up into play in your
//! threat area, with 3 clues on it. […] <b>Forced</b> - When the game ends, if
//! there are any clues on Cover Up: You suffer 1 mental trauma."
//! **Dissonant Voices (01165):** "<b>Revelation</b> - Put Dissonant Voices into
//! play in your threat area. You cannot play assets or events. <b>Forced</b> -
//! At the end of the round: Discard Dissonant Voices."

use cards::TEST_REGISTRIES;
use game_core::engine::enumerate::TurnAction;
use game_core::engine::{ApplyResult, EngineOutcome, InputKind};
use game_core::event::{Event, TraumaKind};
use game_core::state::{
    CardCode, CardInPlay, ChaosBag, ChaosToken, DiscardPile, GameState, GameStateBuilder,
    InvestigatorId, LocationId, MythosPhaseFrame, MythosResume, Owner, Phase, Status, Zone,
};
use game_core::test_support::{self, clues, seat, Seat, TestSession};
use game_core::{assert_event, assert_event_count, assert_no_event};

const GRASPING_HANDS: &str = "01162";
const SURVIVAL_INSTINCT: &str = "01081";
const COVER_UP: &str = "01007";
const DISSONANT_VOICES: &str = "01165";
/// Threat-area cards with an owner and no weakness flag: they model the
/// primitive — a card's owner, read on its own — rather than a printed card
/// (ADR 0016), so the routing under test cannot be the weakness flag's.
const OWNED_THREAT: &str = "_owned_threat_card";
const OTHERS_THREAT: &str = "_others_threat_card";
/// A card in the eliminated investigator's play area, which they own.
const OWNED_ASSET: &str = "_owned_asset";

const INV: InvestigatorId = InvestigatorId(1);
const SURVIVOR: InvestigatorId = InvestigatorId(2);
const HERE: LocationId = LocationId(20);
const ELSEWHERE: LocationId = LocationId(21);

/// The test investigator (8 health) at the location with `damage` already on
/// it, holding `hand`.
fn hurt(damage: u8, hand: &[&str]) -> Seat {
    seat(1).at(HERE).harm(damage, 0).hand(hand.iter().copied())
}

/// [`hurt`] with `threat` (code, clues) in the threat area. Cover Up is a
/// weakness, so its bearer owns it; Dissonant Voices is an encounter card, the
/// encounter deck's.
fn hurt_with_threat(damage: u8, threat: &[(&str, u8)]) -> Seat {
    hurt(damage, &[]).threat_area(threat.iter().map(|&(code, n)| (code, clues(n))))
}

/// Mythos step 1.4 with investigator 1 to draw, and Grasping Hands on top of
/// the encounter deck with a rigged `Numeric(-2)` token, so `reveal_committing`
/// puts `investigator` through an Agility(3) test they fail by 2 — or by 1 when
/// Survival Instinct's single [agility] icon is committed. With `survivor`, a
/// second, healthy investigator stands at another location, so investigator
/// 1's death eliminates *him* without ending the scenario; the survivor (who
/// has drawn already) keeps the game running and holds a spare deck card for
/// Upkeep step 4.4's draw.
fn drawing_grasping_hands(investigator: Seat, survivor: bool) -> GameState {
    let builder = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(test_support::test_location(HERE.0, "Here"))
        .with_seat(investigator);
    let builder = if survivor {
        builder
            .with_location(test_support::test_location(ELSEWHERE.0, "Elsewhere"))
            .with_seat(seat(2).at(ELSEWHERE).deck(["01088"]))
            .with_turn_order([INV, SURVIVOR])
    } else {
        builder.with_turn_order([INV])
    };
    let mut state = builder
        .with_phase(Phase::Mythos)
        .with_phase_anchor(MythosPhaseFrame {
            resume: MythosResume::AfterDraws,
        })
        .with_mythos_draw_remaining([INV])
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(-2)]))
        .build();
    state
        .encounter_deck
        .push_back(CardCode::new(GRASPING_HANDS));
    state
}

/// Draw the top encounter card for investigator 1, committing `commit` at the
/// revelation's skill-test window, and play on to the next rest.
fn reveal_committing(state: GameState, commit: &[&str]) -> TestSession {
    TestSession::new_with_registries(state, TEST_REGISTRIES)
        .confirm()
        .commit(commit)
}

/// [`reveal_committing`], ended: the commit step's state, events and outcome.
fn revealed(state: GameState, commit: &[&str]) -> ApplyResult {
    reveal_committing(state, commit).finish()
}

#[test]
fn tester_eliminated_mid_test_abandons_the_test_without_panicking() {
    // Agility 3 + Numeric(-2) + 1 committed [agility] icon = 2 vs difficulty 3 →
    // fail by 1 → 1 damage. 7/8 damage → lethal → elimination drains the hand while the
    // SkillTest frame is still live at FireOnResolution / the teardown discard.
    let r = revealed(
        drawing_grasping_hands(hurt(7, &[SURVIVAL_INSTINCT]), false),
        &[SURVIVAL_INSTINCT],
    );

    assert_eq!(r.outcome, EngineOutcome::Done, "test abandoned cleanly");

    let inv = &r.state.investigators[&INV];
    assert_eq!(
        inv.status,
        Status::Defeated,
        "lethal damage eliminated the investigator"
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
    let r = revealed(
        drawing_grasping_hands(hurt(0, &[SURVIVAL_INSTINCT]), false),
        &[SURVIVAL_INSTINCT],
    );

    let inv = &r.state.investigators[&INV];
    assert_eq!(inv.status, Status::Active, "1 damage is not lethal at 0/8");
    assert!(
        inv.discard.iter().any(|c| c.as_str() == SURVIVAL_INSTINCT),
        "surviving tester discards committed cards; discard = {:?}",
        inv.discard
    );
    assert_event_count!(r.events, 1, Event::SkillTestEnded { .. });
}

#[test]
fn elimination_removes_a_player_owned_weakness_from_the_game() {
    // RR p.10 step 1 + the design's reading: Cover Up is owned by the eliminated investigator, whose
    // discard pile step 1 just removed from the game — so "the appropriate
    // discard pile" no longer exists and the card is removed.
    let r = revealed(
        drawing_grasping_hands(hurt_with_threat(7, &[(COVER_UP, 3)]), false),
        &[],
    );

    let inv = &r.state.investigators[&INV];
    assert_eq!(inv.status, Status::Defeated);
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
    // RR p.10 step 4: Dissonant Voices is owned by the encounter deck, so the
    // investigator's elimination must not remove it from the game.
    let r = revealed(
        drawing_grasping_hands(hurt_with_threat(7, &[(DISSONANT_VOICES, 0)]), false),
        &[],
    );

    let inv = &r.state.investigators[&INV];
    assert_eq!(inv.status, Status::Defeated);
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
        "an encounter card must NOT be removed from the game by an \
         investigator's elimination"
    );
    assert_event!(r.events, Event::CardDiscarded { code, from: Zone::ThreatArea, to: DiscardPile::Encounter }
        if code.as_str() == DISSONANT_VOICES);
}

#[test]
fn elimination_routes_a_mixed_threat_area_both_ways() {
    let r = revealed(
        drawing_grasping_hands(
            hurt_with_threat(7, &[(COVER_UP, 3), (DISSONANT_VOICES, 0)]),
            false,
        ),
        &[],
    );

    let inv = &r.state.investigators[&INV];
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
    let r = revealed(
        drawing_grasping_hands(hurt_with_threat(7, &[(COVER_UP, 3)]), false),
        &[],
    );

    assert_event!(r.events, Event::TraumaSuffered {
        investigator, kind: TraumaKind::Mental, amount: 1
    } if *investigator == INV);
    // Exactly once. Solo, so the death also latches NoResolution and the
    // ordinary scenario-end `GameEnd` scan runs — it must not fire this a second
    // time (Cover Up has left play by then, and that scan skips non-Active
    // investigators anyway, #567).
    assert_event_count!(r.events, 1, Event::TraumaSuffered { .. });
    // The scenario-end GameEnd path must still have run, so the exactly-once
    // assertion above is not vacuous.
    assert_event!(r.events, Event::ScenarioResolved { .. });

    // Step 0's tail — "Then, remove those weaknesses from the game" — still runs.
    let inv = &r.state.investigators[&INV];
    assert!(inv.threat_area.is_empty(), "threat area drained");
    assert!(inv.removed_from_game.iter().any(|c| c.as_str() == COVER_UP));
}

#[test]
fn cover_ups_trauma_fires_on_elimination_while_the_scenario_continues() {
    // The multiplayer half of the hole (#638): the scenario does *not* end, so
    // the ordinary `GameEnd` path never runs at all — the trauma can only come
    // from Elimination step 0. RR p.10: "For the purpose of resolving weakness
    // cards, the game has ended for the eliminated investigator."
    let r = revealed(
        drawing_grasping_hands(hurt_with_threat(7, &[(COVER_UP, 3)]), true),
        &[],
    );

    assert_eq!(r.state.investigators[&INV].status, Status::Defeated);
    assert_eq!(
        r.state.investigators[&SURVIVOR].status,
        Status::Active,
        "the survivor keeps the scenario running",
    );
    assert_no_event!(r.events, Event::AllInvestigatorsEliminated);
    assert_no_event!(r.events, Event::ScenarioResolved { .. });

    assert_event!(r.events, Event::TraumaSuffered {
        investigator, kind: TraumaKind::Mental, amount: 1
    } if *investigator == INV);
}

#[test]
fn eliminated_investigator_with_a_clueless_cover_up_suffers_no_trauma() {
    // The Forced's own condition: "if there are any clues on Cover Up". With no
    // clues the ability does not *initiate* at all (#786) — RR p.2, "Forced
    // Abilities": *"If a forced ability does not have the potential to change
    // the game state, the ability does not initiate."* Step 0 scans it and
    // collects nothing. The control that keeps the test above honest about
    // *why* the trauma landed.
    let r = revealed(
        drawing_grasping_hands(hurt_with_threat(7, &[(COVER_UP, 0)]), false),
        &[],
    );

    assert_eq!(
        r.state.investigators[&INV].status,
        Status::Defeated,
        "the elimination still happened",
    );
    assert_no_event!(r.events, Event::TraumaSuffered { .. });
}

#[test]
fn interactive_elimination_with_a_clueless_cover_up_raises_no_acknowledge() {
    // The elimination half of #786. RR p.10 Elimination step 0 scans the same
    // `EventPattern::GameEnd` declaration, so the initiation gate must hold
    // there too: with no clues the Forced does not initiate, and the #466
    // acknowledge `interactive_acknowledge` would otherwise raise never appears.
    let mut state = drawing_grasping_hands(hurt_with_threat(7, &[(COVER_UP, 0)]), false);
    state.interactive_acknowledge = true;

    // The one unrelated prompt on the way: `interactive_acknowledge` also
    // pauses on the skill-test result, before the damage lands.
    let acknowledging = reveal_committing(state, &[]);
    assert_eq!(
        acknowledging.prompt().kind,
        InputKind::Confirm,
        "the skill-test result acknowledge: {:?}",
        acknowledging.prompt(),
    );
    let session = acknowledging.confirm();

    assert_eq!(
        session.state().investigators[&INV].status,
        Status::Defeated,
        "the elimination still happened",
    );
    assert_no_event!(session.events(), Event::TraumaSuffered { .. });
    // The session stops at the first prompt nothing answers, so a raised
    // acknowledge would be where it rests. It rests at the game's end instead:
    // no prompt at all followed the elimination.
    let r = session.finish();
    assert_eq!(
        r.outcome,
        EngineOutcome::Done,
        "no forced acknowledge for a clueless Cover Up",
    );
}

#[test]
fn eliminated_investigator_fires_no_further_round_end_forced() {
    // #567's acceptance: Dissonant Voices' Forced ("At the end of the round:
    // Discard Dissonant Voices") must not fire again for a dead investigator —
    // step 4 already discarded it. The survivor keeps the game running, so the
    // round really ends: their turn is the round's last.
    let eliminated = reveal_committing(
        drawing_grasping_hands(hurt_with_threat(7, &[(DISSONANT_VOICES, 0)]), true),
        &[],
    );
    assert_eq!(
        eliminated.state().investigators[&INV].status,
        Status::Defeated
    );
    let before = eliminated.state().encounter_discard.clone();
    let seen = eliminated.events().len();

    let session = eliminated.take(&TurnAction::EndTurn);
    let round_end = &session.events()[seen..];

    // The round really ended, so the no-op below cannot pass on a drive that
    // never got there: play is at the next round's encounter draw.
    assert_event!(
        round_end,
        Event::PhaseEnded {
            phase: Phase::Upkeep
        }
    );
    session.assert_at_encounter_draw();
    assert_eq!(
        session.state().encounter_discard,
        before,
        "no further round-end forced for a dead investigator"
    );
    assert_no_event!(round_end, Event::CardDiscarded { .. });
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
    let r = revealed(drawing_grasping_hands(hurt(7, &[]), false), &[]);

    let inv = &r.state.investigators[&INV];
    assert_eq!(inv.status, Status::Defeated);
    assert_eq!(
        inv.investigator_card.code.as_str(),
        test_support::TEST_INV,
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
    let run = || {
        reveal_committing(
            drawing_grasping_hands(
                hurt(7, &[SURVIVAL_INSTINCT]).threat_area([(COVER_UP, clues(3))]),
                false,
            ),
            &[SURVIVAL_INSTINCT],
        )
    };
    let first = run();
    let second = run();

    assert_eq!(
        first.state(),
        second.state(),
        "the elimination interleaving must replay bit-for-bit"
    );
    assert_eq!(
        first.events(),
        second.events(),
        "events replay identically too"
    );
    assert_eq!(first.finish().outcome, EngineOutcome::Done);
}

/// Put `code`, owned by `owner`, into investigator 1's threat area. A seat
/// gives a threat-area card the owner its printed card names; these probes
/// print nothing and need a chosen owner, so they are placed on the built
/// state, with an id minted from its counter.
fn into_threat_area(state: &mut GameState, code: &str, owner: InvestigatorId) {
    let id = state.card_instance_ids.mint();
    state
        .investigators
        .get_mut(&INV)
        .expect("investigator 1")
        .threat_area
        .push(CardInPlay::enter_play(
            CardCode::new(code),
            id,
            Owner::Investigator(owner),
        ));
}

#[test]
fn elimination_partitions_the_threat_area_by_owner() {
    // RR p.10 Elimination, step 1: *"The cards he or she controls in play and
    // all of the cards in his or her out-of-play areas (such as hand, deck,
    // discard pile) are removed from the game."* and step 4: *"All other cards
    // in the eliminated investigator's threat area are placed in the
    // appropriate discard pile."* — which is the card's owner's
    // (`glossary/Discard_Piles.md`: *"Any time a card is discarded, it is placed
    // faceup on top of its owner's discard pile."*).
    //
    // So a threat-area card the eliminated investigator owns leaves at step 1,
    // weakness or not, and one another investigator owns is discarded at step 4
    // to that investigator's pile — not the encounter discard.
    let mut state = drawing_grasping_hands(hurt(7, &[]), true);
    into_threat_area(&mut state, OWNED_THREAT, INV);
    into_threat_area(&mut state, OTHERS_THREAT, SURVIVOR);
    let r = revealed(state, &[]);

    let eliminated = &r.state.investigators[&INV];
    assert_eq!(eliminated.status, Status::Defeated);
    assert!(eliminated.threat_area.is_empty(), "threat area drained");
    assert!(
        eliminated
            .removed_from_game
            .iter()
            .any(|c| c.as_str() == OWNED_THREAT),
        "their own card is removed from the game at step 1; removed = {:?}",
        eliminated.removed_from_game
    );
    assert_eq!(
        r.state.investigators[&SURVIVOR].discard,
        vec![CardCode::new(OTHERS_THREAT)],
        "another investigator's card goes to its owner's discard at step 4"
    );
    assert!(
        !r.state
            .encounter_discard
            .iter()
            .any(|c| [OWNED_THREAT, OTHERS_THREAT].contains(&c.as_str())),
        "neither is the encounter deck's; encounter discard = {:?}",
        r.state.encounter_discard
    );
    assert_event!(r.events, Event::CardRemovedFromGame { code, from: Zone::ThreatArea }
        if code.as_str() == OWNED_THREAT);
    assert_event!(r.events, Event::CardDiscarded {
        code,
        from: Zone::ThreatArea,
        to: DiscardPile::Investigator(to),
    } if code.as_str() == OTHERS_THREAT && *to == SURVIVOR);
}

#[test]
fn elimination_step_one_emits_one_removal_event_per_card_in_play() {
    // RR p.10 step 1 removes the cards in play *and* the out-of-play areas, but
    // only a card leaving play is an event: one `CardRemovedFromGame` per card
    // in play, naming the zone it left, and none for the hand, deck or discard.
    let mut state = drawing_grasping_hands(
        hurt(7, &["filler1"])
            .in_play([OWNED_ASSET])
            .deck(["filler2", "filler3"])
            .discard(["filler4"]),
        false,
    );
    into_threat_area(&mut state, OWNED_THREAT, INV);
    let r = revealed(state, &[]);

    let inv = &r.state.investigators[&INV];
    assert_eq!(inv.status, Status::Defeated);
    for code in [
        OWNED_ASSET,
        OWNED_THREAT,
        "filler1",
        "filler2",
        "filler3",
        "filler4",
    ] {
        assert!(
            inv.removed_from_game.iter().any(|c| c.as_str() == code),
            "{code} removed from the game; removed = {:?}",
            inv.removed_from_game
        );
    }
    assert_event!(r.events, Event::CardRemovedFromGame { code, from: Zone::InPlay }
        if code.as_str() == OWNED_ASSET);
    assert_event!(r.events, Event::CardRemovedFromGame { code, from: Zone::ThreatArea }
        if code.as_str() == OWNED_THREAT);
    assert_event_count!(r.events, 2, Event::CardRemovedFromGame { .. });
}
