//! End-to-end Dodge 01023: cancel an enemy-phase attack, driven through the
//! public [`apply`] API with the real card corpus installed (#305 / Axis D
//! #336).
//!
//! `game-core`'s unit tests can't install `cards::REGISTRY` (the engine crate
//! can't depend on `cards`), so the before-attack cancel window's end-to-end
//! behaviour — Dodge offered from hand, played, the attack cancelled — is
//! covered here.
//!
//! Dodge 01023: "Fast. Play when an enemy attacks an investigator at your
//! location. Cancel that attack."

use cards::REGISTRY;
use game_core::action::{Action, InputResponse, PlayerAction};
use game_core::engine::enumerate::TurnAction;
use game_core::engine::{self, EngineOutcome, OptionId, TimingEvent};
use game_core::event::{Event, LapseReason};
use game_core::state::{
    Agenda, CardCode, CardInPlay, CardInstanceId, Enemy, EnemyId, GameState, GameStateBuilder,
    InvestigatorId, LocationId,
};
use game_core::test_support::{self, TestSession};
use game_core::{assert_event, assert_no_event};

/// Dodge (01023): Neutral Tactic, Fast, the before-attack cancel reaction.
const DODGE: &str = "01023";

#[ctor::ctor(unsafe)]
fn install_real_registry() {
    test_support::install_registry_with_test_cards(REGISTRY);
}

/// A ready enemy dealing 2 damage / 1 horror.
fn ready_attacker(id: u32) -> Enemy {
    let mut e = test_support::test_enemy(id, format!("Attacker {id}"));
    e.attack_damage = 2;
    e.attack_horror = 1;
    e
}

/// Investigation-phase state: one active investigator at a location with
/// `DODGE` in hand, engaged by one ready attacker. `EndTurn` advances into the
/// Enemy phase; the `BeforeInvestigatorAttacked` player window auto-skips
/// (Dodge is a reaction event — `check_play_card` rejects a standalone play, so
/// it is not a framework Fast play), then the attack loop opens the
/// `BeforeEnemyAttack` cancel window and offers Dodge.
fn dodge_state() -> (GameState, InvestigatorId, EnemyId) {
    let inv_id = InvestigatorId(1);
    let loc_id = LocationId(101);
    let enemy_id = EnemyId(7);

    let mut inv = test_support::test_investigator(1);
    inv.current_location = Some(loc_id);
    inv.hand = vec![CardCode::new(DODGE)];
    // A spare deck card so the round-ending cascade's upkeep step-4.4 draw has
    // something to draw. Without it, the empty deck reshuffles the discard —
    // and since a played Dodge is discarded the instant its effect completes
    // (RR Appendix I step 4, now flushed at completion rather than the apply
    // boundary — #348), the reshuffle would draw Dodge straight back into hand,
    // obscuring the "Dodge went to discard" assertion.
    inv.deck = vec![CardCode::new("01088")];

    let state = GameStateBuilder::new()
        .with_location(test_support::test_location(101, "Study"))
        .with_investigator(inv)
        .with_enemy_engaged(ready_attacker(7), inv_id)
        .open_turn(inv_id)
        .build();
    (state, inv_id, enemy_id)
}

/// Silver Twilight Acolyte (01102), verbatim from the pinned snapshot:
///
/// ```text
/// Prey - Bearer only.
/// Hunter.
/// Forced - After Silver Twilight Acolyte attacks: Place 1 doom on the current
///   agenda.
/// ```
const ACOLYTE: &str = "01102";

/// The `dodge_state` board with the attacker replaced by a real Silver Twilight
/// Acolyte and an agenda for it to place doom on. Its **printed** statistics (1
/// damage, 0 horror — `data/arkhamdb-snapshot/pack/core/core.json`) are set here
/// rather than read from metadata, matching the rest of this file's fixtures;
/// the both-tracks-cancelled claim is `ready_attacker`'s to prove, since this
/// enemy prints no horror to cancel.
fn acolyte_state() -> (GameState, InvestigatorId, EnemyId) {
    let (mut state, inv_id, enemy_id) = dodge_state();
    let enemy = state
        .enemies
        .get_mut(&enemy_id)
        .expect("dodge_state seeds one attacker");
    enemy.code = CardCode::new(ACOLYTE);
    enemy.attack_damage = 1;
    enemy.attack_horror = 0;
    state.agenda_deck = vec![Agenda {
        code: CardCode::new("01105"),
        doom_threshold: 3,
    }];
    state.agenda_index = 0;
    (state, inv_id, enemy_id)
}

/// The Dodge ruling's own example, inverted — `data/arkhamdb-faq/core/01023.md`,
/// verbatim:
///
/// > If the attacking enemy has a **Forced** ability that says "When attacks" or
/// > "After attacks", that ability does not trigger if an attack is Dodged.
///
/// The suppression is #714's, in the coordinator; what #704 adds is that the
/// attack now walks its cells at all, so the Acolyte's forced ability sits in a
/// cell the cancel can suppress. Verified against the real corpus: 01102's
/// abilities come from the installed `cards::REGISTRY`.
#[test]
fn dodging_a_silver_twilight_acolyte_stops_its_forced_doom() {
    let (state, inv_id, enemy_id) = acolyte_state();

    let result = test_support::take_turn_action(state, &TurnAction::EndTurn);
    assert!(
        matches!(result.outcome, EngineOutcome::AwaitingInput { .. }),
        "the `when` cell offers Dodge: {:?}",
        result.outcome
    );
    // Play Dodge → the attack is prevented, so its `after` cell never runs.
    let result = engine::apply(
        result.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(OptionId(0)),
        }),
    );
    let state = result.state;

    // 1, not 0: the `EndTurn` cascade runs on through Upkeep into the next
    // round's Mythos, whose step 1.2 places a doom of its own before parking on
    // the encounter-draw prompt. The Acolyte's would have made it 2 — see the
    // control test below.
    assert_eq!(
        state.agenda_doom, 1,
        "the Acolyte's `Forced - After … attacks` doom must not fire on a dodged \
         attack (only Mythos step 1.2's doom is here)"
    );
    assert_eq!(
        state.investigators[&inv_id].damage(),
        0,
        "the cancelled attack dealt no damage"
    );
    assert!(
        result.events.iter().any(|e| matches!(
            e,
            Event::EnemyExhausted { enemy } if *enemy == enemy_id
        )),
        "the attacker still exhausts (01023 ruling): {:?}",
        result.events
    );
}

/// The control: undodged, the same Acolyte's forced ability *does* fire, and it
/// fires in the `after` cell — once the damage has landed.
#[test]
fn an_undodged_silver_twilight_acolyte_places_its_doom_after_the_damage() {
    let (state, inv_id, _) = acolyte_state();

    let result = test_support::take_turn_action(state, &TurnAction::EndTurn);
    let result = engine::apply(
        result.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::Skip,
        }),
    );

    // 2 = the Acolyte's forced doom + Mythos step 1.2's, against the dodged
    // run's 1.
    assert_eq!(
        result.state.agenda_doom, 2,
        "the Acolyte's forced after-attack doom fired"
    );
    assert_eq!(
        result.state.investigators[&inv_id].damage(),
        1,
        "the attack landed its 1 damage"
    );
    assert!(
        result
            .events
            .iter()
            .any(|e| matches!(e, Event::DamageTaken { .. })),
        "damage precedes the `after` cell; events = {:?}",
        result.events
    );
}

#[test]
fn dodge_cancels_enemy_phase_attack_no_damage_attacker_exhausts() {
    let (state, inv_id, enemy_id) = dodge_state();

    // EndTurn → Enemy phase → the attack loop opens the before-attack cancel
    // window and suspends, offering Dodge from hand.
    let result = test_support::take_turn_action(state, &TurnAction::EndTurn);
    let mut state = result.state;
    assert!(
        matches!(result.outcome, EngineOutcome::AwaitingInput { .. }),
        "the before-attack cancel window suspends the loop: {:?}",
        result.outcome
    );
    // No damage dealt yet (the attack hasn't resolved).
    assert_eq!(state.investigators[&inv_id].damage(), 0);

    // Play Dodge (the single offered candidate) → cancel the attack.
    let result = engine::apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(OptionId(0)),
        }),
    );
    state = result.state;

    assert_eq!(
        state.investigators[&inv_id].damage(),
        0,
        "the cancelled attack dealt no damage"
    );
    assert_eq!(state.investigators[&inv_id].horror(), 0, "…and no horror");
    assert!(
        !result
            .events
            .iter()
            .any(|e| matches!(e, Event::DamageTaken { .. } | Event::HorrorTaken { .. })),
        "a cancelled attack deals neither damage nor horror: {:?}",
        result.events
    );
    // The attacker still exhausts (RR p.6 + p.25) — asserted via the event,
    // since the enemy-phase cascade re-readies it at upkeep step 4.3.
    assert!(
        result.events.iter().any(|e| matches!(
            e,
            Event::EnemyExhausted { enemy } if *enemy == enemy_id
        )),
        "the attacker still exhausts after a cancelled attack: {:?}",
        result.events
    );
    // Dodge left hand and went to discard (a played event).
    assert!(
        !state.investigators[&inv_id]
            .hand
            .contains(&CardCode::new(DODGE)),
        "Dodge left hand"
    );
    assert!(
        state.investigators[&inv_id]
            .discard
            .contains(&CardCode::new(DODGE)),
        "Dodge is in the discard pile after being played"
    );
}

#[test]
fn declining_the_before_attack_window_lets_the_attack_land() {
    let (state, inv_id, enemy_id) = dodge_state();

    let result = test_support::take_turn_action(state, &TurnAction::EndTurn);
    let mut state = result.state;
    assert!(matches!(
        result.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));

    // Skip the window → the attack resolves normally.
    let result = engine::apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::Skip,
        }),
    );
    state = result.state;

    assert!(
        result.events.iter().any(|e| matches!(
            e,
            Event::DamageTaken { investigator, amount: 2 } if *investigator == inv_id
        )),
        "the un-cancelled attack dealt its 2 damage: {:?}",
        result.events
    );
    assert_eq!(
        state.investigators[&inv_id].damage(),
        2,
        "investigator carries the 2 damage"
    );
    assert!(
        result.events.iter().any(|e| matches!(
            e,
            Event::EnemyExhausted { enemy } if *enemy == enemy_id
        )),
        "attacker exhausts after attacking: {:?}",
        result.events
    );
    // Dodge was never played: still in hand, nothing in discard.
    assert!(
        state.investigators[&inv_id]
            .hand
            .contains(&CardCode::new(DODGE)),
        "Dodge stays in hand when the window is declined"
    );
}

/// Dissonant Voices (01165), verbatim from the pinned snapshot
/// (`data/arkhamdb-snapshot/pack/core/core_encounter.json`; no rulings —
/// `data/arkhamdb-faq/no-rulings.txt`):
///
/// ```text
/// Revelation - Put Dissonant Voices into play in your threat area.
/// You cannot play assets or events.
/// Forced - At the end of the round: Discard Dissonant Voices.
/// ```
const DISSONANT_VOICES: &str = "01165";

/// Put Dissonant Voices into `inv_id`'s threat area.
fn ban_plays(state: &mut GameState, inv_id: InvestigatorId) {
    state
        .investigators
        .get_mut(&inv_id)
        .expect("the investigator is seated")
        .threat_area
        .push(CardInPlay::enter_play(
            CardCode::new(DISSONANT_VOICES),
            CardInstanceId(90),
        ));
}

/// #917, at the offer. A Fast event is still *played* — `glossary/Fast.md`:
/// *"A fast card does not cost an action to be played and is not played using
/// the "Play" action."* — so Dissonant Voices' ban on playing events keeps
/// Dodge out of the before-attack window, and the attack lands.
#[test]
fn dissonant_voices_keeps_dodge_out_of_the_attack_window() {
    let (mut state, inv_id, _) = dodge_state();
    ban_plays(&mut state, inv_id);

    let result = test_support::take_turn_action(state, &TurnAction::EndTurn);

    assert_event!(
        result.events,
        Event::DamageTaken { investigator, amount: 2 } if *investigator == inv_id
    );
    assert_no_event!(result.events, Event::CardPlayed { .. });
    assert!(
        result.state.investigators[&inv_id]
            .hand
            .contains(&CardCode::new(DODGE)),
        "Dodge was never offered, so it stays in hand",
    );
}

/// #917, at the pick. Dodge is offered, then the ban arrives before the player
/// picks it. The ban binds at initiation, not at the scan: the stale pick is
/// refused, and when the window next surfaces Dodge has lapsed and the attack
/// lands.
#[test]
fn dodge_lapses_when_dissonant_voices_arrives_before_the_pick() {
    let (state, inv_id, _) = dodge_state();
    let offered = test_support::take_turn_action(state, &TurnAction::EndTurn);
    assert!(
        matches!(offered.outcome, EngineOutcome::AwaitingInput { .. }),
        "without the ban the window offers Dodge: {:?}",
        offered.outcome,
    );
    let mut state = offered.state;
    ban_plays(&mut state, inv_id);

    let pick = engine::apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(OptionId(0)),
        }),
    );
    assert!(
        matches!(pick.outcome, EngineOutcome::Rejected { .. }),
        "Dodge can no longer be played, so picking it is refused: {:?}",
        pick.outcome,
    );
    assert!(
        pick.state.investigators[&inv_id]
            .hand
            .contains(&CardCode::new(DODGE)),
        "the refused pick left Dodge in hand",
    );

    let resurfaced = TestSession::new(pick.state);
    let events = resurfaced.events();
    assert_event!(
        events,
        Event::ReactionOptionLapsed { investigator, code, reason: LapseReason::PlayBanned }
            if *investigator == inv_id && code.as_str() == DODGE
    );
    assert_no_event!(events, Event::CardPlayed { .. });
    assert_event!(
        events,
        Event::DamageTaken { investigator, amount: 2 } if *investigator == inv_id
    );
}

/// Dodge's *"Play when an enemy attacks an investigator at your location."*
/// scopes the window, not the play. Seat 2 holds Dodge at the Study where seat
/// 1 is attacked, and is offered it; then seat 2 is moved to the Hallway before
/// the pick. Dodge is still playable — the gate passes it — but no longer
/// belongs to this window, so it lapses as out of scope rather than as an
/// eligibility failure.
#[test]
fn dodge_lapses_as_out_of_scope_once_its_holder_leaves_the_attacked_location() {
    let attacked = InvestigatorId(1);
    let dodger = InvestigatorId(2);
    let mut seat_1 = test_support::test_investigator(1);
    seat_1.current_location = Some(LocationId(101));
    let mut seat_2 = test_support::test_investigator(2);
    seat_2.current_location = Some(LocationId(101));
    seat_2.hand = vec![CardCode::new(DODGE)];
    let state = GameStateBuilder::new()
        .with_location(test_support::test_location(101, "Study"))
        .with_location(test_support::test_location(102, "Hallway"))
        .with_investigator(seat_1)
        .with_investigator(seat_2)
        .with_turn_order([attacked, dodger])
        .with_enemy_engaged(ready_attacker(7), attacked)
        .build();

    let opened = TestSession::new(state).fire_at(TimingEvent::EnemyAttacks {
        enemy: EnemyId(7),
        investigator: attacked,
    });
    let offered: Vec<_> = opened
        .state()
        .top_window()
        .and_then(|w| w.pending_candidates())
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|c| (c.code, c.controller))
        .collect();
    assert_eq!(offered, vec![(CardCode::new(DODGE), dodger)]);

    let mut state = opened.state().clone();
    state
        .investigators
        .get_mut(&dodger)
        .expect("seated")
        .current_location = Some(LocationId(102));
    let resurfaced = TestSession::new(state);

    assert_event!(
        resurfaced.events(),
        Event::ReactionOptionLapsed { investigator, code, reason: LapseReason::OutOfScope }
            if *investigator == dodger && code.as_str() == DODGE
    );
    assert_no_event!(resurfaced.events(), Event::CardPlayed { .. });
}
