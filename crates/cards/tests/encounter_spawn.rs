//! End-to-end test of the spawn-on-reveal path (#127), against a real Core Set
//! enemy.
//!
//! Staged **at** the Mythos phase's step-1.4 encounter draw, and driven by
//! confirming that draw, so what is under test is the reveal → spawn → engage
//! path itself rather than the Mythos phase walk that leads to it
//! (`mythos_phase.rs` owns the phase walk). The test exercises:
//!
//! - Happy path: revealing the enemy from the encounter deck emits
//!   `Event::CardRevealed` (kind Enemy), then `Event::EnemySpawned` at its
//!   printed spawn location, engaged with the drawing investigator. The enemy
//!   lands in `state.enemies` and does NOT appear in `encounter_discard`.
//! - Multi-investigator suspend: two investigators at the spawn location → the
//!   spawn suspends (`AwaitingInput`) for the lead investigator's `PickSingle`
//!   (#128, option A), leaving the enemy in play but unengaged until the pick
//!   resolves.
//!
//! Default-spawn and location-not-in-play coverage lives in
//! `enemy_spawn_no_location.rs` and `enemy_spawn_unrepresented.rs`.
//!
//! Lives in `crates/cards/tests/` (ADR 0016) because it runs against
//! `cards::TEST_REGISTRIES`: `game-core` cannot reach the corpus by crate
//! direction.
//!
//! The cards, all Core Set, all verified against
//! `data/arkhamdb-snapshot/pack/core/` and their rulings files:
//!
//! - **Flesh-Eater 01118** — *"**Spawn** - Attic."* The board therefore seats
//!   its investigators at the **Attic 01113** so the spawn engages them. Its
//!   ruling — *"If an enemy should spawn at a location that is not currently in
//!   play … place that enemy card into the encounter discard pile without any
//!   further effects"* (<https://arkhamdb.com/card/01118>) — is the off-board
//!   case, pinned separately by `enemy_spawn_no_location.rs`; here the Attic is
//!   in play.
//! - **Attic 01113** — the spawn location, built through the engine from its own
//!   corpus metadata (`GameState::add_location`) rather than hand-stamped onto a
//!   `test_location`, which is the impersonation ADR 0016 forbids. Its
//!   *"**Forced** - After you enter the Attic: Take 1 horror."* is live in the
//!   registry and simply never fires — only the Move action emits
//!   `EnteredLocation`, and these tests place their investigators. Its ruling —
//!   *"The **Forced** ability triggers each time an investigator enters this
//!   location"* (<https://arkhamdb.com/card/01113>) — scopes that same entry.

use card_dsl::card_data::CardType;
use cards::TEST_REGISTRIES;
use game_core::event::Event;
use game_core::state::{
    CardCode, GameStateBuilder, InvestigatorId, LocationId, MythosPhaseFrame, MythosResume, Owner,
    Phase,
};
use game_core::test_support::{seat, TestSession};
use game_core::{assert_event_sequence, assert_no_event};

/// Flesh-Eater — *"**Spawn** - Attic."*
const FLESH_EATER: &str = "01118";
/// The Attic — Flesh-Eater's spawn location, and where the board seats everyone.
const ATTIC: &str = "01113";

const INV: InvestigatorId = InvestigatorId(1);

/// The board every test starts from: Mythos step 1.4 with investigator 1 to
/// draw, one Flesh-Eater on the encounter deck, and the Attic in play with
/// the `seated` investigators (in turn order) standing in it. Returns the
/// minted [`LocationId`] alongside, since `add_location` mints it on the built
/// state, which is also why the investigators are placed there afterwards.
fn drawing_flesh_eater(seated: &[InvestigatorId]) -> (TestSession, LocationId) {
    let mut state = seated
        .iter()
        .fold(
            GameStateBuilder::new().with_registries(TEST_REGISTRIES),
            |builder, id| builder.with_seat(seat(id.0)),
        )
        .with_turn_order(seated.iter().copied())
        .with_phase(Phase::Mythos)
        .with_phase_anchor(MythosPhaseFrame {
            resume: MythosResume::AfterDraws,
        })
        .with_mythos_draw_remaining([INV])
        .build();
    let attic = state.add_location(cards::by_code(ATTIC).expect("Attic 01113 in corpus"));
    for inv in state.investigators.values_mut() {
        inv.current_location = Some(attic);
    }
    state.encounter_deck.push_back(CardCode::new(FLESH_EATER));
    (TestSession::new(state, TEST_REGISTRIES), attic)
}

/// **An enemy drawn from the encounter deck is the encounter deck's**
/// (`glossary/Discard_Piles.md`: *"Encounter cards are owned by the encounter
/// deck."*).
#[test]
fn a_spawned_encounter_enemy_is_owned_by_the_encounter_deck() {
    let (draw, _) = drawing_flesh_eater(&[INV]);
    let session = draw.confirm();

    session.assert_at_turn_menu(INV);
    let enemy = session
        .state()
        .enemies
        .values()
        .next()
        .expect("Flesh-Eater spawned");
    assert_eq!(enemy.owner, Owner::EncounterDeck);
}

#[test]
fn revealing_flesh_eater_spawns_at_the_attic_engaged_with_drawer() {
    let (draw, attic) = drawing_flesh_eater(&[INV]);
    let session = draw.confirm();

    // The spawn needs no choice, and play runs on out of the Mythos phase.
    session.assert_at_turn_menu(INV);

    // CardRevealed (Enemy) fires first; EnemySpawned follows.
    assert_event_sequence!(
        session.events(),
        Event::CardRevealed { card_type, code, .. }
            if *card_type == CardType::Enemy
                && *code == CardCode::new(FLESH_EATER),
        Event::EnemySpawned { code, location, engaged_with, .. }
            if *code == CardCode::new(FLESH_EATER)
                && *location == attic
                && *engaged_with == Some(INV),
        Event::EnemyEngaged { investigator, .. }
            if *investigator == INV,
    );

    let state = session.state();
    // Enemy is in play.
    assert_eq!(
        state.enemies.len(),
        1,
        "exactly one enemy should be in play after spawn",
    );
    let enemy = state.enemies.values().next().unwrap();
    assert_eq!(enemy.current_location, Some(attic));
    assert_eq!(enemy.engaged_with, Some(INV));

    // Enemy is NOT in encounter_discard (enemies stay in play; only
    // treacheries discard after Revelation).
    assert!(
        !state
            .encounter_discard
            .contains(&CardCode::new(FLESH_EATER)),
        "spawned enemy must not appear in encounter_discard",
    );

    // Sanity: the Attic's code is what spawn_enemy looked up.
    let loc = state.locations.get(&attic).unwrap();
    assert_eq!(loc.code, CardCode::new(ATTIC));
}

#[test]
fn revealing_flesh_eater_with_two_investigators_at_the_attic_suspends_for_lead_pick() {
    // Second investigator co-located at the spawn location.
    let (draw, _) = drawing_flesh_eater(&[INV, InvestigatorId(2)]);
    let session = draw.confirm();

    // The spawn suspends for the lead's choice of whom it engages, offering
    // each investigator at the Attic by their investigator card.
    let state = session.state();
    let offered: Vec<_> = state
        .investigators
        .values()
        .map(|inv| session.offered_at(&inv.card_anchor()))
        .collect();
    assert_eq!(
        (offered, session.prompt().options.len()),
        (vec![1, 1], 2),
        "multi-investigator spawn suspends for the lead's pick between the two: {:?}",
        session.prompt(),
    );
    assert_no_event!(session.events(), Event::EnemyEngaged { .. });
    let enemy = state.enemies.values().next().expect("enemy placed");
    assert_eq!(
        enemy.engaged_with, None,
        "engagement deferred until the lead picks",
    );
}
