//! #1111: a prompt inside a skill test's verdict shows how the test stands.
//!
//! Lucky! 01080, `text` verbatim (`data/arkhamdb-snapshot/pack/core/core.json`):
//!
//! > Fast. Play when you would fail a skill test.
//! > Get +2 to your skill value for that test.
//!
//! It is offered in `SkillTestResolved`'s `when` cell, and the
//! `SkillTestFailed` line is logged only after that cell, so the result modal
//! has nothing to render yet. The banner reads the provisional verdict off the
//! live test frame instead, and the engine keeps that frame current between
//! two Lucky!s.
//!
//! **Native, not wasm**: it drives the engine and folds each step's state
//! through the real `store::reduce`, as `skill_test_result_across_batches.rs`
//! does. Runs against `cards::TEST_REGISTRIES`.
#![cfg(not(target_arch = "wasm32"))]

use cards::TEST_REGISTRIES;
use game_core::engine::{EngineOutcome, OptionTarget};
use game_core::state::{
    CardCode, ChaosBag, ChaosToken, GameStateBuilder, InvestigatorId, SkillKind, TokenModifiers,
};
use game_core::test_support::{seat, TestSession};
use protocol::ServerMessage;
use web::skill_test_result;
use web::store::{self, ClientState};

const LUCKY: &str = "01080";
const INV: InvestigatorId = InvestigatorId(1);

/// The client after `session`'s last step: its state and its events.
fn client(session: &TestSession) -> ClientState {
    let mut client = ClientState::default();
    store::reduce(
        &mut client,
        ServerMessage::Applied {
            state: Box::new(session.state().clone()),
            events: session.events().to_vec(),
            outcome: EngineOutcome::Done,
        },
    );
    client
}

fn lucky() -> OptionTarget {
    OptionTarget::HandCardByCode {
        investigator: INV,
        code: CardCode::new(LUCKY),
    }
}

#[test]
fn lucky_s_window_shows_the_provisional_verdict_and_keeps_it_current() {
    // Intellect 3 + 0 against 6: failing by 3, with two Lucky!s in hand.
    let window = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(1).resources(5).hand([LUCKY, LUCKY]))
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .open_turn(INV)
        .session()
        .test_skill(INV, SkillKind::Intellect, 6)
        .commit(&[]);
    assert_eq!(window.offered_at(&lucky()), 2, "{:?}", window.prompt());
    let shown = client(&window);
    assert!(
        skill_test_result::summarize(&shown).is_none(),
        "nothing logged yet"
    );
    assert_eq!(
        skill_test_result::provisional_verdict(&shown).as_deref(),
        Some("Failing by 3")
    );

    // One Lucky! later the test is failing by 1, and the window says so.
    let window = window.pick_nth_at(lucky(), 0);
    assert_eq!(window.offered_at(&lucky()), 1, "{:?}", window.prompt());
    assert_eq!(
        skill_test_result::provisional_verdict(&client(&window)).as_deref(),
        Some("Failing by 1")
    );
}
