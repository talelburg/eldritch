//! Integration tests for the Fast play-card and activated-ability gates
//! introduced in #103.
//!
//! Per the Arkham Horror LCG Rules Reference (page 11):
//!
//! - "A fast event card may be played from a player's hand any time its
//!   play instructions specify." → permitted by any investigator a
//!   window's `fast_actors` scope allows.
//! - "A fast asset may be played by an investigator during any player
//!   window on his or her turn." → restricted to the OWNER (the active
//!   investigator); non-owner plays remain illegal even in a window.
//! - "The ⚡ icon indicates a free triggered ability that does not cost
//!   an action and may be used during any player window." → activated
//!   abilities have no owner restriction.
//!
//! These tests cover the asset gate via Magnifying Glass (01030), the
//! event gate via Working a Hunch (01037), and the activated-ability gate
//! via Beat Cop's (01018) `[fast]` *"Discard Beat Cop: Deal 1 damage to an
//! enemy at your location."*. Beat Cop rather than Hyperawareness (01034)
//! because a `for this skill test` buff is refused outside a test (#676),
//! which would mask the window gate this file is about.
//!
//! Note: we use `Phase::Mythos` (a non-Investigation phase) in the
//! "owner during permissive window" test so the open-window branch is
//! the load-bearing condition for permission — Investigation phase alone
//! is enough to play under the active-investigator branch and would mask
//! the actual rule being tested.
//!
//! The file grew a #710 section at the end: the same zero-action gate, asked of
//! an **engine-opened** player window rather than a builder-seeded one, so that
//! the corpus's existing `[fast]` abilities are checked for *being offered* and
//! not only for being accepted.
//!
//! Why this file exists at the `cards/tests/` layer: it needs real card
//! metadata + abilities from the `cards` corpus, which `game-core` itself
//! cannot reach by crate-dependency direction.

use cards::TEST_REGISTRIES;
use game_core::engine::enumerate::{PlaySource, TurnAction};
use game_core::engine::{InputKind, OptionTarget};
use game_core::state::{
    AbilityAddress, AbilitySource, CardCode, ChaosBag, ChaosToken, EnemyId, FastActorScope,
    FastWindowKind, GameState, GameStateBuilder, InvestigationPhaseFrame, InvestigationResume,
    InvestigatorId, LocationId, MythosPhaseFrame, MythosResume, Phase, PhaseStep, SkillKind,
};
use game_core::test_support::{self, seat, Seat, SeatLookup as _, TestSession};

/// Beat Cop 01018: *"You get +1 \[combat\]."* / *"\[fast\] Discard Beat Cop:
/// Deal 1 damage to an enemy at your location."*
const BEAT_COP: &str = "01018";
/// Magnifying Glass 01030: *"Fast. You get +1 \[intellect\] while
/// investigating."*
const MAGNIFYING_GLASS: &str = "01030";
/// Working a Hunch 01037: *"Fast. Play only during your turn. Discover 1 clue
/// at your location."*
const WORKING_A_HUNCH: &str = "01037";
/// Holy Rosary 01059 — a non-Fast asset, cost 2.
const HOLY_ROSARY: &str = "01059";

const A: InvestigatorId = InvestigatorId(1);
const B: InvestigatorId = InvestigatorId(2);
const HERE: LocationId = LocationId(101);
const GHOUL: EnemyId = EnemyId(100);

/// The Mythos phase after its draws, with investigator A active and the
/// step-1.4 player window open — a permissive window outside the
/// Investigation phase.
fn mythos_after_draws_window(builder: GameStateBuilder) -> GameStateBuilder {
    builder
        .with_phase(Phase::Mythos)
        .with_active_investigator(A)
        .with_phase_anchor(MythosPhaseFrame {
            resume: MythosResume::AfterDraws,
        })
        .with_open_window(
            FastWindowKind::Phase(PhaseStep::MythosAfterDraws),
            FastActorScope::Any,
        )
}

/// The Mythos phase at step 1.4, A to draw an encounter card, A active: a
/// moment outside the Investigation phase with no player window open.
fn mythos_encounter_draw(builder: GameStateBuilder) -> GameStateBuilder {
    builder
        .with_phase(Phase::Mythos)
        .with_active_investigator(A)
        .with_phase_anchor(MythosPhaseFrame {
            resume: MythosResume::AfterDraws,
        })
        .with_mythos_draw_remaining([A])
}

/// The player window that opens as A's turn begins, with B seated as `b`
/// and a Study holding a clue at `HERE`. A window that offers nothing closes
/// on its own, so A holds a Magnifying Glass the window offers them, which
/// keeps it standing open.
fn a_turn_begins_window(b: Seat) -> TestSession {
    let mut study = test_support::test_location(HERE.0, "Study");
    study.clues = 1;
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(study)
        .with_seat(seat(1).hand([MAGNIFYING_GLASS]))
        .with_seat(b)
        .with_turn_order([A, B])
        .with_phase(Phase::Investigation)
        .with_active_investigator(A)
        .with_phase_anchor(InvestigationPhaseFrame {
            resume: InvestigationResume::TurnBegins,
        })
        .with_open_window(
            FastWindowKind::Phase(PhaseStep::InvestigatorTurnBegins),
            FastActorScope::Any,
        )
        .session()
}

/// The anchor of the card at `hand_index` in `holder`'s hand.
fn hand_card(holder: InvestigatorId, hand_index: u8) -> OptionTarget {
    OptionTarget::HandCard {
        investigator: holder,
        hand_index,
    }
}

/// Assert `session` rests at an open player window still offering A's
/// Magnifying Glass — the permissive window is standing.
fn assert_window_offers_as_glass(session: &TestSession) {
    assert_eq!(
        session.offered_at(&hand_card(A, 0)),
        1,
        "the permissive window is open, offering A's Magnifying Glass: {:?}",
        session.prompt(),
    );
}

#[test]
fn fast_asset_playable_by_owner_during_permissive_window() {
    // Owner-as-active-investigator with a real permissive window (MythosAfterDraws)
    // open during a non-Investigation phase: the strict pre-#103 gate would reject
    // (phase != Investigation), but the loosened gate must accept because the
    // window permits and the owner IS the active investigator. This is the
    // rules-correct positive case for Fast assets. (#476: the window surfaces
    // the play as a choice and auto-closes once nothing remains, so the post-play
    // drive cascades through the MythosPhase anchor to the next phase — hence the
    // realistic anchor; the assertion is that the play executed, not the exact
    // post-cascade outcome.)
    let window = mythos_after_draws_window(
        GameStateBuilder::new()
            .with_registries(TEST_REGISTRIES)
            .with_seat(seat(1).resources(5).hand([MAGNIFYING_GLASS]))
            .with_turn_order([A]),
    )
    .session();
    // The window offers the play, and picking it plays the card.
    let session = window.pick(hand_card(A, 0));
    let a_after = &session.state().investigators[&A];
    assert_eq!(a_after.hand.len(), 0, "card should have left hand");
    assert!(
        a_after
            .cards_in_play
            .iter()
            .any(|c| c.code == CardCode::new(MAGNIFYING_GLASS)),
        "Magnifying Glass should be in play",
    );
}

#[test]
fn fast_asset_rejected_by_non_owner_even_with_permissive_window() {
    // Per Rules Reference p. 11: a Fast asset may only be played by its
    // owner (i.e. on the owner's turn — the active investigator). A
    // non-owner attempting the Fast play remains illegal even if an
    // open window's `fast_actors` scope permits the actor. The window does not
    // offer it, so it goes straight to the handler, as a client can send it.
    let window = a_turn_begins_window(seat(2).resources(5).hand([MAGNIFYING_GLASS]));
    assert_window_offers_as_glass(&window);
    let session = window.submit_off_menu(&TurnAction::PlayCard {
        investigator: B,
        source: PlaySource::Hand(0),
    });
    let reason = session.expect_rejected();
    assert!(
        reason.contains("owner")
            || reason.contains("asset")
            || reason.contains("active")
            || reason.contains("Fast"),
        "expected gate rejection citing Fast-asset owner restriction; got: {reason}",
    );
}

#[test]
fn non_fast_asset_still_rejected_when_not_active_investigator() {
    let window = a_turn_begins_window(seat(2).resources(5).hand([HOLY_ROSARY]));
    assert_window_offers_as_glass(&window);
    let session = window.submit_off_menu(&TurnAction::PlayCard {
        investigator: B,
        source: PlaySource::Hand(0),
    });
    let reason = session.expect_rejected();
    // Make sure the rejection cites the timing-window gate,
    // not (for instance) the missing-from-hand or resource-shortage paths.
    assert!(
        reason.contains("non-Fast")
            || reason.contains("Investigation")
            || reason.contains("active")
            || reason.contains("timing"),
        "expected non-Fast gate rejection; got: {reason}",
    );
}

#[test]
fn fast_asset_still_playable_by_active_investigator_during_investigation() {
    let session = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(1).resources(5).hand([MAGNIFYING_GLASS]))
        .open_turn(A)
        .session()
        .take(&TurnAction::PlayCard {
            investigator: A,
            source: PlaySource::Hand(0),
        });
    // Magnifying Glass plays normally for the active investigator (Phase-3
    // behavior preserved), and the turn goes on.
    session.assert_at_turn_menu(A);
    assert!(
        session.state().investigators[&A]
            .cards_in_play
            .iter()
            .any(|c| c.code == CardCode::new(MAGNIFYING_GLASS)),
        "Magnifying Glass should be in play",
    );
}

/// Board for the activated-ability gate: investigator B controls Beat Cop at
/// a location with a co-located enemy, so the ability has a live target; A is
/// the active investigator, in the Mythos phase. `moment` stages where in the
/// Mythos phase — the open permissive window, or the encounter draw with none
/// open — the condition the tests below differ on.
fn board_with_beat_cop(moment: fn(GameStateBuilder) -> GameStateBuilder) -> GameStateBuilder {
    let mut enemy = test_support::test_enemy(GHOUL.0, "Ghoul");
    enemy.max_health = 3;
    enemy.current_location = Some(HERE);
    moment(
        GameStateBuilder::new()
            .with_registries(TEST_REGISTRIES)
            .with_seat(seat(1))
            .with_seat(seat(2).at(HERE).in_play([BEAT_COP]))
            .with_turn_order([A, B])
            .with_location(test_support::test_location(HERE.0, "Study"))
            .with_enemy(enemy)
            .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)])),
    )
}

/// The anchor of B's Beat Cop.
fn beat_cop(state: &GameState) -> OptionTarget {
    state.card(B, BEAT_COP)
}

/// Beat Cop's `[fast]` ability is index 1 (index 0 is its constant
/// `+1 [combat]`).
fn beat_cop_fast(state: &GameState) -> TurnAction {
    TurnAction::ActivateAbility {
        investigator: B,
        source: AbilitySource::InPlay(state.in_play(B, BEAT_COP)),
        address: AbilityAddress::Printed(1),
    }
}

/// Assert Beat Cop's ability resolved: its damage landed and it paid its own
/// discard as the cost.
fn assert_beat_cop_fired(session: &TestSession) {
    assert_eq!(
        session.state().enemies[&GHOUL].damage,
        1,
        "the ability's damage landed",
    );
    assert!(
        session.state().investigators[&B].cards_in_play.is_empty(),
        "Beat Cop paid its own discard as the cost",
    );
}

#[test]
fn fast_activated_ability_usable_by_non_active_investigator_when_window_permits() {
    let window = board_with_beat_cop(mythos_after_draws_window).session();
    // The window offers B's ability, and it activates from the pick (its
    // DiscardSelf cost is paid and the damage lands). The #476 fast window may
    // re-prompt afterwards, so assert the activation executed rather than the
    // exact post-activation outcome.
    let beat_cop = beat_cop(window.state());
    let session = window.pick(beat_cop);
    assert_beat_cop_fired(&session);
}

#[test]
fn fast_activated_ability_rejected_when_no_permissive_window() {
    // Same board, no open window: B is not the active investigator, so
    // nothing permits the fast activation. Nothing offers it, so it goes
    // straight to the handler.
    let drawing = board_with_beat_cop(mythos_encounter_draw).session();
    let action = beat_cop_fast(drawing.state());
    let session = drawing.submit_off_menu(&action);
    let reason = session.expect_rejected();
    assert!(
        reason.contains("Fast") || reason.contains("active") || reason.contains("Investigation"),
        "expected gate-rejection wording; got: {reason}",
    );
}

#[test]
fn fast_event_play_only_during_turn_rejected_outside_investigation() {
    // Working a Hunch (01037): "Fast. Play only during your turn. Discover 1
    // clue at your location." The `play_only_during_turn` metadata flag (#322)
    // tightens the Fast gate to the active investigator's Investigation turn,
    // so even a permissive window in the Mythos phase is rejected — per the FAQ,
    // "'your turn' is within the Investigation phase." (Was previously, wrongly,
    // accepted while the clause was unenforced.) A's Magnifying Glass keeps the
    // window standing; the window does not offer the Hunch, so it goes straight
    // to the handler.
    let mut location = test_support::test_location(HERE.0, "Study");
    location.clues = 1;
    let window = mythos_after_draws_window(
        GameStateBuilder::new()
            .with_registries(TEST_REGISTRIES)
            .with_seat(
                seat(1)
                    .resources(5)
                    .at(HERE)
                    .hand([WORKING_A_HUNCH, MAGNIFYING_GLASS]),
            )
            .with_location(location)
            .with_turn_order([A]),
    )
    .session();
    assert_eq!(
        window.offered_at(&hand_card(A, 1)),
        1,
        "the permissive window is open, offering A's Magnifying Glass: {:?}",
        window.prompt(),
    );
    let session = window.submit_off_menu(&TurnAction::PlayCard {
        investigator: A,
        source: PlaySource::Hand(0),
    });
    let _ = session.expect_rejected();
    // Unchanged: still in hand, clue not taken.
    assert_eq!(
        session.state().investigators[&A].hand,
        [
            CardCode::new(WORKING_A_HUNCH),
            CardCode::new(MAGNIFYING_GLASS)
        ],
        "card stays in hand on reject",
    );
    assert_eq!(session.state().locations[&HERE].clues, 1, "no clue taken");
}

#[test]
fn fast_event_play_only_during_turn_rejected_for_non_owner() {
    // Working a Hunch (01037): "Fast. Play only during your turn." During
    // investigator 1's turn (Investigation, active = inv 1), investigator 2
    // cannot play it — it is not inv 2's turn. The `play_only_during_turn`
    // gate (#322) requires the *active* investigator, so a non-owner is
    // rejected even in a permissive window.
    let window = a_turn_begins_window(seat(2).resources(5).at(HERE).hand([WORKING_A_HUNCH]));
    assert_window_offers_as_glass(&window);
    let session = window.submit_off_menu(&TurnAction::PlayCard {
        investigator: B,
        source: PlaySource::Hand(0),
    });
    let _ = session.expect_rejected();
}

#[test]
fn fast_asset_rejected_by_owner_outside_investigation_with_no_window() {
    // Fast assets need EITHER active_during_investigation OR
    // (owner_is_active && permissive_window). Owner-during-non-
    // Investigation with no window meets neither — must reject.
    //
    // Magnifying Glass (01030) text: "Fast.\nYou get +1 [intellect]
    // while investigating."
    let drawing = mythos_encounter_draw(
        GameStateBuilder::new()
            .with_registries(TEST_REGISTRIES)
            .with_seat(seat(1).resources(5).hand([MAGNIFYING_GLASS]))
            .with_turn_order([A]),
    )
    .session();
    // No window is open: the Mythos phase rests at the encounter draw.
    drawing.assert_at_encounter_draw();
    let session = drawing.submit_off_menu(&TurnAction::PlayCard {
        investigator: A,
        source: PlaySource::Hand(0),
    });
    let reason = session.expect_rejected();
    assert!(
        reason.contains("Fast")
            || reason.contains("active")
            || reason.contains("Investigation")
            || reason.contains("window"),
        "expected gate-rejection wording; got: {reason}",
    );
}

// ---- the corpus regression for #710 --------------------------------------
//
// #710 widened *which sources* a player window reaches, by making the window's
// enumerator consult the one reachability predicate the turn menu consults
// (`engine::ability_source`). The cards that already print a zero-action
// ability all sit under the first rules bullet — *"a card in play and under his
// or her control"* — so this is the half most at risk of a silent regression,
// and the one the ticket asks be checked against the real corpus.
//
// Beat Cop 01018 (verbatim, <https://arkhamdb.com/card/01018>):
//
// > You get +1 [combat].
// > [fast] Discard Beat Cop: Deal 1 damage to an enemy at your location.
//
// with the ruling that makes the window the load-bearing part, verbatim:
//
// > You cannot use Beat Cop's ability after you assign lethal damage/horror to
// > him (there is no Player Window to use [fast]free abilities in between
// > assigning and applying damage).
//
// The two tests above activate against a builder-seeded window. This one goes
// through an **engine-opened** window — a skill test's ST.1 player window —
// and asserts the ability is *offered* there, which is the seam #710 is about:
// the Parlor's Resign missing from the menu was as much the defect as its
// being refused on submission, and the same is true of a window's option list.

/// The anchors of the options `session`'s prompt offers.
fn offered_anchors(session: &TestSession) -> Vec<OptionTarget> {
    session
        .prompt()
        .options
        .iter()
        .map(|o| o.target.clone().expect("every option here is anchored"))
        .collect()
}

/// Beat Cop's zero-action ability is offered by an engine-opened player window,
/// anchored to the card instance carrying it, and resolves from the pick — the
/// existing corpus behaviour, unchanged by the source widening.
#[test]
fn corpus_zero_action_ability_is_offered_by_an_engine_opened_player_window() {
    // No seeded window: the skill test opens its own, so the option list under
    // assertion is the one a real client would be shown. Mythos phase with
    // investigator 1 active, so nothing but the window can permit the
    // activation. The window opens before any token is revealed, but the test
    // refuses to start against an empty bag.
    let opened = board_with_beat_cop(mythos_encounter_draw)
        .session()
        .test_skill(B, SkillKind::Willpower, 4);
    // The skill test parks at its ST.1 player window.
    let request = opened.prompt();
    assert_eq!(request.kind, InputKind::PickSingle, "{request:?}");
    assert!(
        request.skippable,
        "a player window is skippable: {request:?}"
    );
    let beat_cop = beat_cop(opened.state());
    assert_eq!(
        offered_anchors(&opened),
        vec![beat_cop.clone()],
        "Beat Cop's [fast] ability is the one thing eligible on this board, anchored to \
         the instance that carries it",
    );

    // Picking the offered option resolves the ability.
    let picked = opened.pick(beat_cop);
    assert_beat_cop_fired(&picked);
}

/// Hyperawareness 01034 (verbatim, <https://arkhamdb.com/card/01034>):
///
/// > [fast] Spend 1 resource: You get +1 [intellect] for this skill test.
/// > [fast] Spend 1 resource: You get +1 [agility] for this skill test.
///
/// and Physical Training 01017 (verbatim, <https://arkhamdb.com/card/01017>):
///
/// > [fast] Spend 1 resource: You get +1 [willpower] for this skill test.
/// > [fast] Spend 1 resource: You get +1 [combat] for this skill test.
///
/// — the corpus's other zero-action abilities, and the only ones that print
/// two apiece. Both carry the same ruling, Hyperawareness's verbatim:
///
/// > You can use [fast]fast actions as many times as you want, as long as you
/// > can pay the cost; there is no limit.
///
/// The window offers all four, each anchored to the instance that prints it —
/// the control bullet, unchanged by #710's widening. Their `for this skill
/// test` scope is why they can only be checked inside a test (#676), which is
/// exactly what a ST.1 player window is.
#[test]
fn corpus_zero_action_abilities_are_offered_once_per_ability_not_once_per_card() {
    const HYPERAWARENESS: &str = "01034";
    const PHYSICAL_TRAINING: &str = "01017";

    // Mythos, so the window is the only thing that can permit the activations
    // (investigator 1 is not acting in an Investigation turn).
    let opened = mythos_encounter_draw(
        GameStateBuilder::new()
            .with_registries(TEST_REGISTRIES)
            .with_seat(
                seat(1)
                    .resources(5)
                    .in_play([HYPERAWARENESS, PHYSICAL_TRAINING]),
            )
            .with_turn_order([A])
            .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)])),
    )
    .session()
    .test_skill(A, SkillKind::Willpower, 4);

    // The skill test parks at its ST.1 player window.
    let hyperawareness = opened.state().card(A, HYPERAWARENESS);
    let physical_training = opened.state().card(A, PHYSICAL_TRAINING);
    assert_eq!(
        offered_anchors(&opened),
        vec![
            hyperawareness.clone(),
            hyperawareness,
            physical_training.clone(),
            physical_training,
        ],
        "each of the four zero-action abilities is its own option, anchored to the card \
         that prints it",
    );
}
