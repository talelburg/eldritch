//! Regression for #378: **playing an action-cost card** provokes attacks of
//! opportunity, driven through the public [`apply`] API against the real card
//! corpus.
//!
//! Per the Rules Reference (p.5, "Attack of Opportunity"), playing a card is an
//! action, so playing a **non-fast** card (asset or event) while engaged with a
//! ready enemy provokes one `AoO` from each — after the action cost is paid and
//! before the card's effect resolves. The Dynamite Blast 01024 FAQ pins the
//! order: "first you spend an action and pay the cost, then each engaged enemy
//! makes an attack of opportunity against you, and then the effects of the card
//! resolve — but only if you're still alive." Fast events/assets are not
//! actions and never provoke.
//!
//! It also covers the one action a non-fast play spends.
//!
//! `game-core`'s unit tests can't reach `cards::REGISTRY`, so this lives in
//! `crates/cards/tests/`. The `AoO` window/suspend mechanism is covered by
//! `dodge_aoo.rs` / `guard_dog_soak.rs`; these tests prove the **play-card
//! firing site** routes through it.
//!
//! ## Verified card text (`ArkhamDB`, 2026-06-21)
//!
//! **Emergency Cache (01088):** "Gain 3 resources." A non-fast event → provokes.
//! **Working a Hunch (01037):** "Fast. Play only during your turn. … discover 1
//! clue …" A fast event → `AoO`-exempt.
#![allow(clippy::too_many_lines)]

use cards::TEST_REGISTRIES;
use game_core::assert_no_event;
use game_core::engine::enumerate::{self, PlaySource, TurnAction};
use game_core::engine::OptionTarget;
use game_core::event::Event;
use game_core::state::{CardCode, Enemy, GameStateBuilder, InvestigatorId, LocationId, Status};
use game_core::test_support::{self, seat, Seat, SeatLookup as _, TestSession};

/// Emergency Cache (01088): non-fast event, `OnPlay` gain 3 resources → provokes.
const EMERGENCY_CACHE: &str = "01088";
/// Guard Dog (01021): Guardian Ally, health 3 / sanity 1, damage-retaliate soak.
const GUARD_DOG: &str = "01021";
/// Working a Hunch (01037): Fast event, `OnPlay` discover 1 clue → `AoO`-exempt.
const WORKING_A_HUNCH: &str = "01037";
/// Machete (01020): non-fast Guardian weapon asset → playing it provokes.
const MACHETE: &str = "01020";

const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(101);

/// Play the first card in hand.
const PLAY_FIRST: TurnAction = TurnAction::PlayCard {
    investigator: INV,
    source: PlaySource::Hand(0),
};

/// A ready enemy dealing `damage` / 0 horror, with `max_health`. Engage it
/// with `with_enemy_engaged`, which places it at the investigator's location.
fn ready_attacker(id: u32, damage: u8, max_health: u8) -> Enemy {
    let mut e = test_support::test_enemy(id, format!("Attacker {id}"));
    e.attack_damage = damage;
    e.attack_horror = 0;
    e.max_health = max_health;
    e
}

/// Investigator 1 at the Study holding `card`, with 3 actions.
fn holding(card: &str) -> Seat {
    seat(1).at(HERE).hand([card]).actions(3)
}

/// `investigator`'s turn at the Study (with `clues` clues), engaged by
/// `attacker` if one is given.
fn board(investigator: Seat, clues: u8, attacker: Option<Enemy>) -> TestSession {
    let mut location = test_support::test_location(HERE.0, "Study");
    location.clues = clues;
    let builder = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(location)
        .with_seat(investigator)
        .open_turn(INV);
    match attacker {
        Some(enemy) => builder.with_enemy_engaged(enemy, INV),
        None => builder,
    }
    .session()
}

/// The accumulated damage on the seated Guard Dog.
fn dog_damage(session: &TestSession) -> u8 {
    let dog = session.state().in_play(INV, GUARD_DOG);
    session.state().investigators[&INV]
        .cards_in_play
        .iter()
        .find(|c| c.instance_id == dog)
        .expect("Guard Dog still in play")
        .accumulated_damage
}

// -----------------------------------------------------------------------
// Playing a non-fast event provokes an AoO.
// -----------------------------------------------------------------------

/// Playing Emergency Cache (a non-fast event) while engaged with a ready enemy
/// provokes an `AoO`. With Guard Dog in play and no Dodge, the `AoO` damage soaks
/// onto Guard Dog and its reaction window opens — before the card's "gain 3
/// resources" effect resolves.
#[test]
fn playing_a_non_fast_event_while_engaged_provokes_an_aoo() {
    let session = board(
        seat(1)
            .at(HERE)
            .hand([EMERGENCY_CACHE])
            .in_play([GUARD_DOG]),
        0,
        Some(ready_attacker(7, 2, 5)),
    );
    let dog = session.state().card(INV, GUARD_DOG);
    let resources_before = session.state().investigators[&INV].resources;

    // The AoO prompts for the soak distribution, one prompt per point
    // of damage: assign both onto Guard Dog.
    let session = session.take(&PLAY_FIRST).pick(dog.clone()).pick(dog);
    let state = session.state();

    assert_ne!(
        session.prompt().target,
        Some(OptionTarget::TurnControl(INV)),
        "the AoO's damage window must suspend the play"
    );
    // Assigned to Guard Dog, not yet placed: the open window is its own `when`
    // cell, between the rules' two steps. This test is about the AoO
    // suspending the play.
    assert_eq!(
        dog_damage(&session),
        0,
        "AoO damage from the play is assigned to Guard Dog, not yet placed"
    );
    assert_eq!(
        state.investigators[&INV].damage(),
        0,
        "investigator took no AoO damage"
    );
    // The "gain 3 resources" effect has NOT run yet — it resolves only after the
    // AoO window closes.
    assert_eq!(
        state.investigators[&INV].resources, resources_before,
        "the card's effect resolves after the AoO, not before"
    );
}

// -----------------------------------------------------------------------
// Playing a non-fast card costs one action.
// -----------------------------------------------------------------------

/// Playing a non-fast event spends one action. With no engaged enemy the play
/// completes immediately (no `AoO`), so we observe the spent action and the
/// resolved effect together.
#[test]
fn playing_a_non_fast_event_spends_one_action() {
    let session = board(holding(EMERGENCY_CACHE), 0, None);
    let resources_before = session.state().investigators[&INV].resources;

    let session = session.take(&PLAY_FIRST);
    session.assert_at_turn_menu(INV);
    let state = session.state();

    assert_eq!(
        state.investigators[&INV].actions_remaining, 2,
        "playing a non-fast card spent one action"
    );
    assert_eq!(
        state.investigators[&INV].resources,
        resources_before + 3,
        "the effect still resolved (gain 3 resources)"
    );
}

/// Playing a non-fast card with no actions left is rejected (validate-first).
#[test]
fn playing_a_non_fast_card_with_no_actions_is_rejected() {
    let session = board(holding(EMERGENCY_CACHE).actions(0), 0, None);

    // 0 actions remaining → PlayCard not offered; send it off the menu to prove
    // the handler itself refuses it.
    assert!(
        !enumerate::legal_actions(session.state(), TEST_REGISTRIES.cards).contains(&PLAY_FIRST),
        "the menu does not offer a non-fast play with 0 actions",
    );
    let result = session.submit_off_menu(&PLAY_FIRST);

    let reason = result.expect_rejected();
    assert!(
        !reason.is_empty(),
        "a non-fast play with 0 actions must be rejected: {reason}",
    );
    // Card stays in hand, untouched.
    assert!(
        result.state().investigators[&INV]
            .hand
            .contains(&CardCode::new(EMERGENCY_CACHE)),
        "the rejected play left the card in hand"
    );
}

// -----------------------------------------------------------------------
// A fast event is not an action — no AoO, no action spent (RR p.11).
// -----------------------------------------------------------------------

/// Playing Working a Hunch (a Fast event) while engaged with a ready enemy
/// provokes no `AoO` and spends no action; its effect (discover 1 clue) resolves.
#[test]
fn playing_a_fast_event_while_engaged_provokes_no_aoo_and_spends_no_action() {
    let session = board(
        holding(WORKING_A_HUNCH).in_play([GUARD_DOG]),
        1,
        Some(ready_attacker(7, 2, 5)),
    )
    .take(&PLAY_FIRST);

    // A fast event provokes no AoO: the play resolves and returns to the
    // open-turn menu. The *no-AoO* proof is the Guard Dog damage assertion
    // below — an AoO would have soaked onto Guard Dog — not where the session
    // rests.
    session.assert_at_turn_menu(INV);
    let state = session.state();
    assert_eq!(dog_damage(&session), 0, "no AoO soaked onto Guard Dog");
    assert_eq!(
        state.investigators[&INV].actions_remaining, 3,
        "a fast play spends no action"
    );
    assert_eq!(
        state.locations[&HERE].clues, 0,
        "the fast event's effect resolved (discovered the clue)"
    );
}

// -----------------------------------------------------------------------
// An AoO that defeats the player mid-play suppresses the card's effect.
// -----------------------------------------------------------------------

/// Per the Dynamite Blast FAQ ("…the effects of the card resolve — but only if
/// you're still alive"): if the play's `AoO` defeats the investigator, the event's
/// effect does not resolve. The card was still played, but it never reaches a
/// discard pile: elimination removes an eliminated investigator's owned cards
/// from the game (RR p.10, step 1), and the mid-play event is one of them.
#[test]
fn aoo_that_defeats_the_player_suppresses_the_event_effect() {
    // Lethal AoO, no soaker / no Dodge → the actor is defeated before the
    // "gain 3 resources" effect can resolve.
    let result = board(holding(EMERGENCY_CACHE), 0, Some(ready_attacker(7, 50, 5)))
        .take(&PLAY_FIRST)
        .finish();
    let state = result.state;

    assert_ne!(
        state.investigators[&INV].status,
        Status::Active,
        "the lethal AoO defeated the actor"
    );
    // Elimination zeroes the wallet, so assert on the absent event, not the value.
    // The event's effect did not resolve (no ResourcesGained) — defeated
    // mid-play.
    assert_no_event!(result.events, Event::ResourcesGained { .. });
    // The card was still played — it left hand — but a dead investigator has no
    // discard pile to receive it: elimination removed it from the game.
    assert!(
        !state.investigators[&INV]
            .discard
            .contains(&CardCode::new(EMERGENCY_CACHE)),
        "the mid-play event must not land in the drained discard pile of an \
         eliminated investigator"
    );
    assert!(
        state.investigators[&INV]
            .removed_from_game
            .contains(&CardCode::new(EMERGENCY_CACHE)),
        "the mid-play event was removed from the game with its owner (RR p.10 step 1)"
    );
    assert!(
        state.play_in_progress().is_none(),
        "elimination swept the in-progress play off its frame"
    );
}

// -----------------------------------------------------------------------
// Playing a non-fast ASSET also provokes, then the asset enters play on resume.
// -----------------------------------------------------------------------

/// Playing Machete (a non-fast asset) while engaged provokes an `AoO` (soaking
/// onto Guard Dog); after the soak window closes, the asset enters play.
#[test]
fn playing_a_non_fast_asset_provokes_an_aoo_then_enters_play() {
    let session = board(
        holding(MACHETE).in_play([GUARD_DOG]),
        0,
        Some(ready_attacker(7, 2, 5)),
    );
    let dog = session.state().card(INV, GUARD_DOG);

    // Step 1: play Machete → AoO soaks onto Guard Dog → soak window; Machete is
    // mid-play — it left hand when the play commenced and enters play only once
    // the play completes. The AoO prompts for the soak distribution,
    // one prompt per point of damage: assign both onto Guard Dog.
    let session = session
        .take(&PLAY_FIRST)
        .pick(dog.clone())
        .pick(dog.clone());
    let state = session.state();
    assert_eq!(
        state.investigators[&INV].actions_remaining, 2,
        "playing the asset spent one action before the AoO"
    );
    assert!(
        !state.investigators[&INV]
            .hand
            .contains(&CardCode::new(MACHETE)),
        "the asset left hand when the play commenced (RR Appendix I step 3)"
    );
    assert_eq!(
        state.play_in_progress().map(|(_, c)| c.clone()),
        Some(CardCode::new(MACHETE)),
        "the asset rides its frame while the AoO window is open — no hand index \
         to go stale (#565)",
    );

    // Step 2: fire Guard Dog's reaction (closes the soak window) → the play
    // resumes → Machete enters play.
    let session = session.pick(dog);
    let state = session.state();
    assert!(
        state.investigators[&INV]
            .cards_in_play
            .iter()
            .any(|c| c.code == CardCode::new(MACHETE)),
        "Machete entered play after the AoO soak window closed"
    );
    assert!(
        !state.investigators[&INV]
            .hand
            .contains(&CardCode::new(MACHETE)),
        "Machete left hand once it entered play"
    );
}

/// If the asset play's `AoO` defeats the player, the asset never enters play (the
/// §D gate suppresses the resume before the enter-play step). Elimination sweeps
/// the un-entered card off the frame carrying it and out of the game (RR p.10
/// step 1) — it is neither in play nor stranded mid-play.
#[test]
fn aoo_that_defeats_the_player_mid_asset_play_leaves_no_asset_in_play() {
    // Lethal AoO, no soaker / no Dodge → defeated before Machete enters play.
    let state = board(holding(MACHETE), 0, Some(ready_attacker(7, 50, 5)))
        .take(&PLAY_FIRST)
        .finish()
        .state;

    assert_ne!(
        state.investigators[&INV].status,
        Status::Active,
        "the lethal AoO defeated the actor"
    );
    assert!(
        !state.investigators[&INV]
            .cards_in_play
            .iter()
            .any(|c| c.code == CardCode::new(MACHETE)),
        "Machete never entered play — the defeat suppressed the enter-play step"
    );
    assert!(
        !state.investigators[&INV]
            .hand
            .contains(&CardCode::new(MACHETE)),
        "the asset had already left hand when the play commenced"
    );
    assert!(
        state.investigators[&INV]
            .removed_from_game
            .contains(&CardCode::new(MACHETE)),
        "elimination swept the un-entered asset off its frame and out of the game"
    );
    assert!(
        state.play_in_progress().is_none(),
        "no play is left in progress after the sweep"
    );
}
