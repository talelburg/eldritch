//! An enemy encounter card spawns via the disposition frame (Slice D, #423).
//!
//! `resolve_encounter_card`'s enemy arm no longer spawns inline — it pushes a
//! [`Continuation::EncounterCard`] frame carrying
//! [`EncounterDisposition::Spawn`], and the global `drive` loop spawns the enemy
//! when it disposes of that frame after the (here empty) Revelation. This test
//! draws the enemy at the Mythos step-1.4 encounter draw through the real
//! `apply` loop and proves it lands in play with no `EncounterCard` frame left
//! behind.
//!
//! Uses a process-local mock registry (the `round_end_rescan.rs` pattern): a
//! synthetic enemy `CardMetadata` with `spawn: None` (spawns at the drawer's
//! location) and no abilities (no Revelation).

use card_dsl::card_data::{CardKind, CardMetadata, HealthValue, Prey};
use game_core::state::{
    CardCode, Continuation, GameStateBuilder, InvestigatorId, LocationId, MythosPhaseFrame,
    MythosResume, Phase,
};
use game_core::test_support::{self, seat, MockRegistry, TestSession};

const ENEMY: &str = "_synth_enemy";

fn synth_enemy_metadata() -> CardMetadata {
    CardMetadata {
        code: ENEMY.into(),
        name: "Synth Enemy".into(),
        text: None,
        traits: Vec::new(),
        back_name: None,
        back_text: None,
        pack_code: "_synth".into(),
        weakness: false,
        kind: CardKind::Enemy {
            fight: 1,
            evade: 1,
            damage: 0,
            horror: 0,
            health: Some(HealthValue::Fixed(1)),
            victory: None,
            spawn: None,
            surge: false,
            peril: false,
            hunter: false,
            retaliate: false,
            prey: Prey::Default,
            quantity: 1,
        },
    }
}

#[ctor::ctor(unsafe)]
fn install() {
    MockRegistry::new()
        .with_card(synth_enemy_metadata())
        .install();
}

const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(1);

#[test]
fn enemy_encounter_card_spawns_via_the_disposition_frame() {
    // Mythos step 1.4: `INV` is to draw the top encounter card, the enemy.
    let mut state = GameStateBuilder::new()
        .with_seat(seat(1).at(HERE))
        .with_location(test_support::test_location(HERE.0, "Here"))
        .with_turn_order([INV])
        .with_phase(Phase::Mythos)
        .with_phase_anchor(MythosPhaseFrame {
            resume: MythosResume::AfterDraws,
        })
        .with_mythos_draw_remaining([INV])
        .build();
    // No builder setter stages the encounter deck.
    state.encounter_deck.push_back(CardCode::new(ENEMY));
    let drawing = TestSession::new(state);
    drawing.assert_at_encounter_draw();

    // Drawing reveals the enemy; it spawns cleanly without a prompt of its own,
    // and play runs on out of the Mythos phase to `INV`'s turn.
    let session = drawing.confirm();
    session.assert_at_turn_menu(INV);
    assert_eq!(
        session.state().enemies.len(),
        1,
        "the enemy spawned into play via the disposition frame",
    );
    let enemy = session
        .state()
        .enemies
        .values()
        .next()
        .expect("enemy present");
    assert_eq!(
        enemy.current_location,
        Some(HERE),
        "spawned at the drawer's location (spawn: None)",
    );
    // The subject is the frame's disposal, so the stack is read directly.
    assert!(
        !session
            .state()
            .continuations
            .iter()
            .any(|c| matches!(c, Continuation::EncounterCard(_))),
        "no EncounterCard frame remains after disposal",
    );
}
