//! #323 integration: Barricade 01038's attach / non-Elite movement block /
//! leave-location self-discard, end-to-end against the real `cards::REGISTRY`.
//!
//! The movement tests drive the real Enemy phase via `EndTurn` (hunter
//! movement is step 3.2) — the same entry `dodge.rs` uses.
//!
//! Own process → installs `cards::REGISTRY`.

use std::sync::Once;

use game_core::engine::EngineOutcome;
use game_core::event::Event;
use game_core::state::{
    CardCode, CardInPlay, CardInstanceId, Enemy, EnemyId, InvestigatorId, LocationId, Phase,
};
use game_core::test_support::{test_enemy, test_investigator, test_location, GameStateBuilder};
use game_core::{apply, assert_event, Action, PlayerAction};

const BARRICADE: &str = "01038";
const GHOUL_PRIEST: &str = "01116"; // Humanoid. Monster. Ghoul. Elite. + Hunter
const GHOUL_MINION: &str = "01160"; // Humanoid. Monster. Ghoul. (non-Elite)
const INV: InvestigatorId = InvestigatorId(1);
const A: LocationId = LocationId(1);
const B: LocationId = LocationId(2);
const ATT_INST: CardInstanceId = CardInstanceId(900);

static INSTALL: Once = Once::new();
fn install() {
    INSTALL.call_once(|| {
        let _ = game_core::card_registry::install(cards::REGISTRY);
    });
}

/// A ready, unengaged hunter (code `code`) at location `at`, with the printed
/// traits of that card (Elite-ness drives the movement block, read off
/// `Enemy.traits` as spawns populate it).
fn hunter(id: u32, code: &str, at: LocationId) -> Enemy {
    let mut e = test_enemy(id, "Hunter");
    e.code = CardCode::new(code);
    e.traits = if code == GHOUL_PRIEST {
        vec![
            "Humanoid".into(),
            "Monster".into(),
            "Ghoul".into(),
            "Elite".into(),
        ]
    } else {
        vec!["Humanoid".into(), "Monster".into(), "Ghoul".into()]
    };
    e.hunter = true;
    e.current_location = Some(at);
    e.engaged_with = None;
    e.exhausted = false;
    e
}

#[test]
fn playing_barricade_attaches_one_card_and_does_not_discard_the_event() {
    install();
    let mut inv = test_investigator(1);
    inv.current_location = Some(A);
    inv.hand = vec![CardCode::new(BARRICADE)];
    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator(inv)
        .with_location(test_location(1, "Study"))
        .with_active_investigator(INV)
        .with_turn_order([INV])
        .build();

    let r = apply(
        state,
        Action::Player(PlayerAction::PlayCard {
            investigator: INV,
            hand_index: 0,
        }),
    );
    assert_eq!(r.outcome, EngineOutcome::Done);
    // Exactly one Barricade: attached to the location, none in hand/discard.
    assert_eq!(
        r.state.locations[&A]
            .attachments
            .iter()
            .filter(|c| c.code == CardCode::new(BARRICADE))
            .count(),
        1,
        "attached once",
    );
    assert!(r.state.investigators[&INV].hand.is_empty(), "left hand");
    assert!(
        r.state.investigators[&INV].discard.is_empty(),
        "not discarded (re-homed, not duplicated)",
    );
    assert_event!(r.events, Event::CardAttachedToLocation { .. });
}

/// Linear map A—B with a Barricade attached at B; the investigator (prey) at B;
/// a hunter at A. Driven via `EndTurn` into the Enemy phase.
fn map_with_barricade_at_b(enemy_code: &str) -> game_core::GameState {
    install();
    let mut inv = test_investigator(1);
    inv.current_location = Some(B);
    let mut a = test_location(1, "A");
    a.connections = vec![B];
    let mut b = test_location(2, "B");
    b.connections = vec![A];
    let mut state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator(inv)
        .with_location(a)
        .with_location(b)
        .with_enemy(hunter(100, enemy_code, A))
        .with_active_investigator(INV)
        .with_turn_order([INV])
        // Mid-Investigation invariant (slice 1a): the EndTurn cascade pops the
        // InvestigationPhase anchor at investigation_phase_end.
        .with_phase_anchor(game_core::state::Continuation::InvestigationPhase {
            resume: game_core::state::InvestigationResume::TurnBegins,
        })
        // Open-turn invariant (slice 2a-i, #393): the InvestigatorTurn frame the
        // EndTurn cascade pops before rotating / cascading.
        .with_investigator_turn(INV)
        .build();
    state
        .locations
        .get_mut(&B)
        .unwrap()
        .attachments
        .push(CardInPlay::enter_play(CardCode::new(BARRICADE), ATT_INST));
    state
}

#[test]
fn non_elite_hunter_cannot_enter_the_barricaded_location() {
    let r = apply(
        map_with_barricade_at_b(GHOUL_MINION),
        Action::Player(PlayerAction::EndTurn),
    );
    assert_eq!(
        r.state.enemies[&EnemyId(100)].current_location,
        Some(A),
        "non-Elite hunter stayed (only path is into the barricaded location)",
    );
}

#[test]
fn elite_hunter_enters_the_barricaded_location() {
    let r = apply(
        map_with_barricade_at_b(GHOUL_PRIEST),
        Action::Player(PlayerAction::EndTurn),
    );
    assert_eq!(
        r.state.enemies[&EnemyId(100)].current_location,
        Some(B),
        "Elite hunter ignores the barricade",
    );
}

#[test]
fn leaving_the_barricaded_location_discards_barricade() {
    install();
    let mut inv = test_investigator(1);
    inv.current_location = Some(A);
    let mut a = test_location(1, "A");
    a.connections = vec![B];
    let mut b = test_location(2, "B");
    b.connections = vec![A];
    let mut state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator(inv)
        .with_location(a)
        .with_location(b)
        .with_active_investigator(INV)
        .with_turn_order([INV])
        .build();
    state
        .locations
        .get_mut(&A)
        .unwrap()
        .attachments
        .push(CardInPlay::enter_play(CardCode::new(BARRICADE), ATT_INST));

    let r = apply(
        state,
        Action::Player(PlayerAction::Move {
            investigator: INV,
            destination: B,
        }),
    );
    assert_eq!(r.outcome, EngineOutcome::Done);
    assert!(
        r.state.locations[&A].attachments.is_empty(),
        "Barricade discarded on leave",
    );
    assert!(
        r.state.investigators[&INV]
            .discard
            .contains(&CardCode::new(BARRICADE)),
        "to the owner's player discard",
    );
}
