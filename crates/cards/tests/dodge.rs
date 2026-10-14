//! End-to-end Dodge 01023: cancel an enemy-phase attack, driven through the
//! public [`apply`] API against the real card corpus (#305 / Axis D
//! #336).
//!
//! `game-core`'s unit tests can't reach `cards::REGISTRY` (the engine crate
//! can't depend on `cards`), so the before-attack cancel window's end-to-end
//! behaviour — Dodge offered from hand, played, the attack cancelled — is
//! covered here.
//!
//! Dodge 01023: "Fast. Play when an enemy attacks an investigator at your
//! location. Cancel that attack."
//!
//! [`apply`]: game_core::engine::apply

use cards::TEST_REGISTRIES;
use game_core::engine::enumerate::TurnAction;
use game_core::engine::OptionTarget;
use game_core::event::{Event, LapseReason};
use game_core::state::{
    Agenda, CardCode, CardInPlay, Enemy, EnemyId, GameStateBuilder, InvestigatorId, LocationId,
    Owner,
};
use game_core::test_support::{self, seat, Seat, TestSession};
use game_core::{assert_event, assert_no_event};

/// Dodge (01023): Neutral Tactic, Fast, the before-attack cancel reaction.
const DODGE: &str = "01023";

const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(101);
const HALLWAY: LocationId = LocationId(102);
const ATTACKER: EnemyId = EnemyId(7);

/// A ready enemy dealing 2 damage / 1 horror.
fn ready_attacker() -> Enemy {
    let mut e = test_support::test_enemy(ATTACKER.0, "Attacker");
    e.attack_damage = 2;
    e.attack_horror = 1;
    e
}

/// Investigator 1 at the Study with `DODGE` in hand.
fn holding_dodge() -> Seat {
    // A spare deck card so the round-ending cascade's upkeep step-4.4 draw has
    // something to draw. Without it, the empty deck reshuffles the discard —
    // and since a played Dodge is discarded the instant its effect completes
    // (RR Appendix I step 4, now flushed at completion rather than the apply
    // boundary — #348), the reshuffle would draw Dodge straight back into hand,
    // obscuring the "Dodge went to discard" assertion.
    seat(1).at(HERE).hand([DODGE]).deck(["01088"])
}

/// `investigator`'s turn, engaged by `attacker` at the Study. `EndTurn`
/// advances into the Enemy phase; the `BeforeInvestigatorAttacked` player
/// window auto-skips (Dodge is a reaction event — `check_play_card` rejects a
/// standalone play, so it is not a framework Fast play), then the attack loop
/// opens the `BeforeEnemyAttack` cancel window and offers Dodge.
fn dodge_board(investigator: Seat, attacker: Enemy) -> GameStateBuilder {
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(test_support::test_location(HERE.0, "Study"))
        .with_seat(investigator)
        .with_enemy_engaged(attacker, INV)
        .open_turn(INV)
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

/// The `dodge_board` with the attacker replaced by a real Silver Twilight
/// Acolyte and an agenda for it to place doom on. Its **printed** statistics (1
/// damage, 0 horror — `data/arkhamdb-snapshot/pack/core/core.json`) are set here
/// rather than read from metadata, matching the rest of this file's fixtures;
/// the both-tracks-cancelled claim is `ready_attacker`'s to prove, since this
/// enemy prints no horror to cancel.
fn acolyte_session() -> TestSession {
    let mut acolyte = ready_attacker();
    acolyte.code = CardCode::new(ACOLYTE);
    acolyte.attack_damage = 1;
    acolyte.attack_horror = 0;
    let mut state = dodge_board(holding_dodge(), acolyte).build();
    // The builder stages no agenda.
    state.agenda_deck = vec![Agenda {
        code: CardCode::new("01105"),
        doom_threshold: 3,
    }];
    state.agenda_index = 0;
    TestSession::new_with_registries(state, TEST_REGISTRIES)
}

/// The anchor of `holder`'s Dodge in a reaction window's prompt.
fn dodge_in_hand_of(holder: InvestigatorId) -> OptionTarget {
    OptionTarget::HandCardByCode {
        investigator: holder,
        code: CardCode::new(DODGE),
    }
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
/// abilities come from the real `cards::REGISTRY`.
#[test]
fn dodging_a_silver_twilight_acolyte_stops_its_forced_doom() {
    // The `when` cell offers Dodge; play it → the attack is prevented, so its
    // `after` cell never runs.
    let session = acolyte_session()
        .take(&TurnAction::EndTurn)
        .pick(dodge_in_hand_of(INV));
    let state = session.state();

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
        state.investigators[&INV].damage(),
        0,
        "the cancelled attack dealt no damage"
    );
    assert_event!(
        session.events(),
        Event::EnemyExhausted { enemy } if *enemy == ATTACKER
    );
}

/// The control: undodged, the same Acolyte's forced ability *does* fire, and it
/// fires in the `after` cell — once the damage has landed.
#[test]
fn an_undodged_silver_twilight_acolyte_places_its_doom_after_the_damage() {
    let session = acolyte_session().take(&TurnAction::EndTurn).skip();

    // 2 = the Acolyte's forced doom + Mythos step 1.2's, against the dodged
    // run's 1.
    assert_eq!(
        session.state().agenda_doom,
        2,
        "the Acolyte's forced after-attack doom fired"
    );
    assert_eq!(
        session.state().investigators[&INV].damage(),
        1,
        "the attack landed its 1 damage"
    );
    assert_event!(session.events(), Event::DamageTaken { .. });
}

#[test]
fn dodge_cancels_enemy_phase_attack_no_damage_attacker_exhausts() {
    // EndTurn → Enemy phase → the attack loop opens the before-attack cancel
    // window and suspends, offering Dodge from hand.
    let offered = dodge_board(holding_dodge(), ready_attacker())
        .session()
        .take(&TurnAction::EndTurn);
    // No damage dealt yet (the attack hasn't resolved).
    assert_eq!(offered.state().investigators[&INV].damage(), 0);

    // Play Dodge → cancel the attack.
    let session = offered.pick(dodge_in_hand_of(INV));
    let inv = &session.state().investigators[&INV];

    assert_eq!(inv.damage(), 0, "the cancelled attack dealt no damage");
    assert_eq!(inv.horror(), 0, "…and no horror");
    assert_no_event!(
        session.events(),
        Event::DamageTaken { .. } | Event::HorrorTaken { .. }
    );
    // The attacker still exhausts (RR p.6 + p.25) — asserted via the event,
    // since the enemy-phase cascade re-readies it at upkeep step 4.3.
    assert_event!(
        session.events(),
        Event::EnemyExhausted { enemy } if *enemy == ATTACKER
    );
    // Dodge left hand and went to discard (a played event).
    assert!(!inv.hand.contains(&CardCode::new(DODGE)), "Dodge left hand");
    assert!(
        inv.discard.contains(&CardCode::new(DODGE)),
        "Dodge is in the discard pile after being played"
    );
}

#[test]
fn declining_the_before_attack_window_lets_the_attack_land() {
    // Skip the window → the attack resolves normally.
    let session = dodge_board(holding_dodge(), ready_attacker())
        .session()
        .take(&TurnAction::EndTurn)
        .skip();
    let inv = &session.state().investigators[&INV];

    assert_event!(
        session.events(),
        Event::DamageTaken { investigator, amount: 2 } if *investigator == INV
    );
    assert_eq!(inv.damage(), 2, "investigator carries the 2 damage");
    assert_event!(
        session.events(),
        Event::EnemyExhausted { enemy } if *enemy == ATTACKER
    );
    // Dodge was never played: still in hand, nothing in discard.
    assert!(
        inv.hand.contains(&CardCode::new(DODGE)),
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

/// #917, at the offer. A Fast event is still *played* — `glossary/Fast.md`:
/// *"A fast card does not cost an action to be played and is not played using
/// the "Play" action."* — so Dissonant Voices' ban on playing events keeps
/// Dodge out of the before-attack window, and the attack lands.
#[test]
fn dissonant_voices_keeps_dodge_out_of_the_attack_window() {
    let session = dodge_board(
        holding_dodge().threat_area([DISSONANT_VOICES]),
        ready_attacker(),
    )
    .session()
    .take(&TurnAction::EndTurn);

    assert_event!(
        session.events(),
        Event::DamageTaken { investigator, amount: 2 } if *investigator == INV
    );
    assert_no_event!(session.events(), Event::CardPlayed { .. });
    assert!(
        session.state().investigators[&INV]
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
    let offered_dodge = dodge_board(holding_dodge(), ready_attacker())
        .session()
        .take(&TurnAction::EndTurn);
    assert_eq!(
        (
            offered_dodge.offered_at(&dodge_in_hand_of(INV)),
            offered_dodge.prompt().options.len(),
        ),
        (1, 1),
        "without the ban the window offers Dodge alone: {:?}",
        offered_dodge.prompt(),
    );

    // No step can put a treachery into play while the window is open, so the
    // ban arrives by a state edit, with an id minted from the state's counter.
    let refused = offered_dodge
        .edit_state(|state| {
            let id = state.card_instance_ids.mint();
            let voices =
                CardInPlay::enter_play(CardCode::new(DISSONANT_VOICES), id, Owner::EncounterDeck);
            state
                .investigators
                .get_mut(&INV)
                .expect("the investigator is seated")
                .threat_area
                .push(voices);
        })
        .pick(dodge_in_hand_of(INV));
    let reason = refused.expect_rejected();
    assert!(
        reason.contains("can no longer be initiated"),
        "Dodge can no longer be played, so picking it is refused: {reason}",
    );
    assert!(
        refused.state().investigators[&INV]
            .hand
            .contains(&CardCode::new(DODGE)),
        "the refused pick left Dodge in hand",
    );

    let resurfaced = TestSession::new_with_registries(refused.state().clone(), TEST_REGISTRIES);
    let events = resurfaced.events();
    assert_event!(
        events,
        Event::ReactionOptionLapsed { investigator, code, reason: LapseReason::PlayBanned }
            if *investigator == INV && code.as_str() == DODGE
    );
    assert_no_event!(events, Event::CardPlayed { .. });
    assert_event!(
        events,
        Event::DamageTaken { investigator, amount: 2 } if *investigator == INV
    );
}

/// Dodge's *"Play when an enemy attacks an investigator at your location."*
/// scopes the window, not the play. Seat 2 holds Dodge at the Study where seat
/// 1 is attacked in the Enemy phase, and is offered it; then seat 2 is moved
/// to the Hallway before the pick. Dodge is still playable — the gate passes
/// it — but no longer belongs to this window, so it lapses as out of scope
/// rather than as an eligibility failure.
#[test]
fn dodge_lapses_as_out_of_scope_once_its_holder_leaves_the_attacked_location() {
    let attacked = INV;
    let dodger = InvestigatorId(2);
    // Both turns end, and the Enemy phase's attack on seat 1 opens the
    // before-attack window that offers seat 2 their Dodge.
    let opened = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(test_support::test_location(HERE.0, "Study"))
        .with_location(test_support::test_location(HALLWAY.0, "Hallway"))
        .with_seat(seat(1).at(HERE))
        .with_seat(seat(2).at(HERE).hand([DODGE]))
        .with_turn_order([attacked, dodger])
        .with_enemy_engaged(ready_attacker(), attacked)
        .open_turn(attacked)
        .session()
        .take(&TurnAction::EndTurn)
        .take(&TurnAction::EndTurn);
    assert_eq!(
        (
            opened.offered_at(&dodge_in_hand_of(dodger)),
            opened.prompt().options.len(),
        ),
        (1, 1),
        "the window offers seat 2's Dodge alone: {:?}",
        opened.prompt(),
    );

    // Settle the moved board afresh, so the window is re-asked against it.
    let mut state = opened.state().clone();
    state
        .investigators
        .get_mut(&dodger)
        .expect("seated")
        .current_location = Some(HALLWAY);
    let resurfaced = TestSession::new_with_registries(state, TEST_REGISTRIES);

    assert_event!(
        resurfaced.events(),
        Event::ReactionOptionLapsed { investigator, code, reason: LapseReason::OutOfScope }
            if *investigator == dodger && code.as_str() == DODGE
    );
    assert_no_event!(resurfaced.events(), Event::CardPlayed { .. });
}
