//! The Enemy phase's per-investigator `BeforeInvestigatorAttacked` player
//! window **pauses** when a Fast play is eligible, driven end-to-end
//! through the public [`apply`] API against the real card corpus.
//!
//! Rules Reference, Appendix II — the flow chart's Enemy phase, verbatim:
//!
//! ```text
//! 3.2 Hunter enemies move.
//! [free] PLAYER WINDOW
//! 3.3 Next investigator resolves engaged enemy attacks. If an investigator
//!     has not yet resolved enemy attacks this phase, return to previous
//!     player window. …
//! ```
//!
//! `game-core`'s own unit test for that window covers the **Skip-resume** path
//! against a synthetic (empty) window, because the engine crate cannot reach
//! `cards::REGISTRY` and so cannot make anything Fast-eligible. The other half
//! — the window pausing for input instead of auto-skipping — needs a real
//! eligible play, which is what this file supplies.
//!
//! The fixture is Beat Cop 01018's zero-action ability rather than the Fast
//! event the ticket imagined: every Fast **event** in the Core corpus is either
//! a reaction (Dodge 01023 — `check_play_card` refuses a standalone play) or
//! carries *"Play only during your turn"* (Working a Hunch 01037), and the
//! Enemy phase is nobody's turn. A `[fast]` ability has no such restriction and
//! is enumerated by the same `enumerate_fast_plays` scan that gates the window.
//!
//! Beat Cop 01018, verbatim from the pinned snapshot:
//!
//! ```text
//! You get +1 [combat].
//! [fast] Discard Beat Cop: Deal 1 damage to an enemy at your location.
//! ```
//!
//! with its ruling, verbatim (<https://arkhamdb.com/card/01018>):
//!
//! > You cannot use Beat Cop's ability after you assign lethal damage/horror to
//! > him (there is no Player Window to use [fast]free abilities in between
//! > assigning and applying damage).
//!
//! — which is the same claim from the other side: whether a `[fast]` ability
//! can be used is exactly the question of whether a player window stands open
//! there. Step 3.3's does.

use cards::TEST_REGISTRIES;
use game_core::engine::enumerate::TurnAction;
use game_core::engine::{InputKind, OptionTarget};
use game_core::event::Event;
use game_core::state::{CardCode, EnemyId, GameStateBuilder, InvestigatorId, LocationId, Phase};
use game_core::test_support::{self, seat, BoardLookup as _, StagedEnemy, TestSession};
use game_core::{assert_event, assert_no_event};

/// Beat Cop (01018): Guardian Ally, `[fast]` *"Discard Beat Cop: Deal 1 damage
/// to an enemy at your location."*
const BEAT_COP: &str = "01018";

const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(101);
const ATTACKER: EnemyId = EnemyId(7);

/// A ready enemy dealing 1 damage. `max_health` is the caller's to choose: 2
/// leaves it alive through a single point of Beat Cop damage, 1 lets the Fast
/// play defeat it before it ever attacks.
fn ready_attacker(max_health: u8) -> StagedEnemy {
    test_support::enemy(ATTACKER)
        .health(max_health)
        .attack_damage(1)
}

/// Mid-Investigation state for one investigator engaged by one ready attacker,
/// poised so that `EndTurn` cascades Investigation → Enemy and runs into the
/// step-3.3 window. `beat_cop` decides whether anything is Fast-eligible there.
fn board(beat_cop: bool, enemy_health: u8) -> GameStateBuilder {
    let me = seat(1)
        .at(HERE)
        // Something for the round-ending cascade's Upkeep step-4.4 draw to take.
        .deck(["01088"]);
    let me = if beat_cop { me.in_play([BEAT_COP]) } else { me };
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(test_support::location(HERE))
        .with_seat(me)
        .with_enemy_engaged(ready_attacker(enemy_health), INV)
        .open_turn(INV)
}

/// `EndTurn`, which cascades out of the Investigation phase and into the Enemy
/// phase's step-3.3 loop.
fn end_turn(board: GameStateBuilder) -> TestSession {
    board.session().take(&TurnAction::EndTurn)
}

/// The anchor of the seated Beat Cop: its `[fast]` ability is offered on it.
fn beat_cop(session: &TestSession) -> OptionTarget {
    session.state().card(INV, BEAT_COP)
}

#[test]
fn before_investigator_attacked_pauses_when_a_fast_play_is_eligible() {
    let session = end_turn(board(true, 2));

    assert_eq!(
        session.state().phase,
        Phase::Enemy,
        "the EndTurn cascade should stop inside the Enemy phase, not run through it; \
         events = {:?}",
        session.events()
    );

    // The window pauses for input rather than auto-skipping: the one player
    // window the Enemy phase opens before an attack, step 3.3's.
    let request = session.prompt();
    assert_eq!(request.kind, InputKind::PickSingle, "{request:?}");
    assert!(
        request.skippable,
        "a player window is passable: {request:?}"
    );
    assert_eq!(
        (
            session.offered_at(&beat_cop(&session)),
            request.options.len()
        ),
        (1, 1),
        "Beat Cop's [fast] ability is the one eligible play on this board: {request:?}",
    );

    // The pause is *before* the attack — nothing has been dealt yet.
    assert_no_event!(session.events(), Event::DamagePlacedOnInvestigator { .. });
    assert!(
        !session.state().enemies[&ATTACKER].exhausted,
        "the attacker has not attacked, so it has not exhausted",
    );
}

#[test]
fn skipping_the_pause_resumes_into_the_attack() {
    let paused = end_turn(board(true, 2));
    let me = paused.state().investigators[&INV].card_anchor();

    // Passing the window drops straight into step 3.3's attack, which stops to
    // ask where its damage goes — Beat Cop is a 2-health Ally in play and so a
    // legal soak target beside the investigator himself. Assign it to the
    // investigator, by their investigator card.
    let assigned = paused.skip().pick(me);
    let events = assigned.events();
    // Passing the window resolves step 3.3's attack…
    assert_event!(
        events,
        Event::DamagePlacedOnInvestigator { investigator, amount: 1 } if *investigator == INV
    );
    // …and the attacker exhausts on completing its attack.
    assert_event!(
        events,
        Event::EnemyExhausted { enemy } if *enemy == ATTACKER
    );
}

#[test]
fn taking_the_fast_play_resolves_before_the_attack() {
    // The attacker dies to Beat Cop's single point of damage, so "the window
    // opens before the attack" is provable by consequence and not only by
    // event order: a defeated enemy never attacks.
    let paused = end_turn(board(true, 1));
    // Beat Cop's zero-action ability is the offered play.
    let beat_cop = beat_cop(&paused);
    let played = paused.pick(beat_cop);

    assert!(
        !played.state().enemies.contains_key(&ATTACKER),
        "Beat Cop's 1 damage defeats the 1-health attacker; enemies = {:?}",
        played.state().enemies
    );
    // With its attacker gone, the investigator takes no attack damage.
    assert_no_event!(
        played.events(),
        Event::DamagePlacedOnInvestigator { investigator, .. } if *investigator == INV
    );
    let inv = &played.state().investigators[&INV];
    assert!(
        inv.cards_in_play.is_empty(),
        "Beat Cop left play as its own cost; cards_in_play = {:?}",
        inv.cards_in_play
    );
    assert!(
        inv.discard.contains(&CardCode::new(BEAT_COP)),
        "…and landed in the discard pile; discard = {:?}",
        inv.discard
    );
}

#[test]
fn the_window_auto_skips_when_nothing_is_fast_eligible() {
    // The control for the three above: the same board without Beat Cop makes
    // the same window auto-skip, so the pause is attributable to Fast
    // eligibility and not to the phase step itself.
    let session = end_turn(board(false, 2));

    // No window is left open on the auto-skip path: play runs on through the
    // round to the next Mythos phase's encounter draw.
    session.assert_at_encounter_draw();
    // The attack resolves in the same cascade…
    assert_event!(
        session.events(),
        Event::DamagePlacedOnInvestigator { investigator, amount: 1 } if *investigator == INV
    );
    // …and the attacker exhausts.
    assert_event!(
        session.events(),
        Event::EnemyExhausted { enemy } if *enemy == ATTACKER
    );
}
