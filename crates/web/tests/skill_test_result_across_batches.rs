//! #853: the skill-test result modal survives a reaction between the result and
//! the acknowledge pause.
//!
//! **Native, not wasm** — every other file in this directory drives the DOM in a
//! headless browser; this one drives the *engine*. It mirrors the server exactly:
//! one `game_core::engine::apply` call is one `ServerMessage::Applied` frame
//! (`crates/server/src/session.rs`), so applying one at a time and folding each
//! result through the real `store::reduce` reproduces the batch boundaries the
//! client actually sees. The bug lives in those boundaries and nowhere else — a
//! unit test over hand-built batches would not have caught it, because nobody
//! would have hand-built the empty batch that carries the acknowledge pause.
//!
//! ## The card
//!
//! **Lita Chantler (01117)**, `text` verbatim
//! (`data/arkhamdb-snapshot/pack/core/core_encounter.json`):
//!
//! > While you control Lita Chantler, she gains:
//! > "Each investigator at your location gets +1 \[combat\].
//! > \[reaction\] When an investigator at your location successfully attacks a
//! > \[\[Monster\]\] enemy: That investigator deals +1 damage."
//!
//! She is the *occasion*, not the subject: her reaction sits in the `when` cell
//! of `SkillTestResolved`, which the engine walks before `AcknowledgeOutcome`.
//! When #853 was found the verdict was logged *before* that cell, so her window
//! split the result from the acknowledge. Since #1111 the condition's resolve
//! step logs it, after the `when` cell, so her pick's batch carries both and
//! she no longer splits anything; her tests stay as the control for a `when`
//! cell.
//!
//! The splitter now is an `after` reaction: **Dr. Milan Christopher (01033)**,
//! `text` verbatim (`data/arkhamdb-snapshot/pack/core/core.json`): *"You get +1
//! \[intellect\]. \[reaction\] After you successfully investigate: Gain 1
//! resource."* The result is logged, then his window suspends, and the
//! acknowledge arrives on a batch of its own. What is load-bearing is that the
//! ability **suspends for input** at all — a forced trigger that prompts for
//! nothing, like Obscuring Fog 01168's, leaves the result and the acknowledge
//! in one batch and never showed the bug.
//!
//! It runs against `cards::TEST_REGISTRIES`: the real corpus with the
//! synthetic test investigator composed in.
#![cfg(not(target_arch = "wasm32"))]

use cards::TEST_REGISTRIES;
use game_core::action::{Action, InputResponse, PlayerAction};
use game_core::engine::enumerate::{self, TurnAction};
use game_core::engine::{self, EngineOutcome, InputKind, OptionId};
use game_core::event::Event;
use game_core::state::{
    CardCode, CardInPlay, CardInstanceId, ChaosBag, ChaosToken, EnemyId, GameState,
    GameStateBuilder, InvestigatorId, LocationId, Owner as CardOwner, SkillKind, TokenModifiers,
};
use game_core::test_support;
use protocol::ServerMessage;
use web::skill_test_result;
use web::store::{self, ClientState};

/// Lita Chantler.
const LITA: &str = "01117";

/// Lita's controller.
const KEEPER: InvestigatorId = InvestigatorId(1);
/// The attacker — *"an investigator"*, not *"you"*.
const OTHER: InvestigatorId = InvestigatorId(2);
const PARLOR: LocationId = LocationId(1);
const GHOUL: EnemyId = EnemyId(100);

/// The board, differing in one bit: whether Lita is in the keeper's play area,
/// and so whether her reaction window opens between the result and the
/// acknowledge. Everything else — the `+0` bag that makes the attack succeed,
/// the `[[Monster]]` Ghoul, both investigators in the Parlor — is fixed.
fn board(lita_in_play: bool) -> GameState {
    let mut keeper = test_support::test_investigator(1);
    keeper.current_location = Some(PARLOR);
    keeper.skills.combat = 3;
    if lita_in_play {
        keeper.cards_in_play.push(CardInPlay::enter_play(
            CardCode::new(LITA),
            CardInstanceId(50),
            CardOwner::Scenario,
        ));
    }

    let mut other = test_support::test_investigator(2);
    other.current_location = Some(PARLOR);
    other.skills.combat = 3;

    // Health well clear of the attack, so the Fight resolves without a defeat
    // queueing windows of its own.
    // Ghoul Minion 01160 (Humanoid. Monster. Ghoul.).
    let mut ghoul = test_support::test_enemy(100, "Ghoul");
    ghoul.code = CardCode::new("01160");
    ghoul.fight = 3;
    ghoul.max_health = 9;
    ghoul.engaged_with = Some(OTHER);
    ghoul.current_location = Some(PARLOR);

    let mut state = GameStateBuilder::new()
        .with_investigator(keeper)
        .with_investigator(other)
        .with_location(test_support::test_location(1, "Parlor"))
        .with_enemy(ghoul)
        .with_turn_order([KEEPER, OTHER])
        .open_turn(OTHER)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .build();
    // The server's default, and the only setting under which the modal exists
    // at all: without it the acknowledge pause never surfaces (#478).
    state.interactive_acknowledge = true;
    state
}

/// What one pause looked like to the client.
struct Pause {
    prompt: InputKind,
    modal_live: bool,
}

/// Take the Fight and answer every prompt through the end of the test, folding
/// each apply's result into a `ClientState` the way the transport does. Fires
/// any offered reaction (the only `PickSingle` in flight is Lita's window) and
/// commits no cards. Returns one entry per pause the client rendered.
fn fight_ghoul(lita_in_play: bool) -> Vec<Pause> {
    drive(
        board(lita_in_play),
        &TurnAction::Fight {
            investigator: OTHER,
            enemy: GHOUL,
        },
    )
}

/// Dr. Milan Christopher.
const MILAN: &str = "01033";

/// The keeper alone in the Parlor (shroud 2, 3 clues) with Dr. Milan in play,
/// investigating against a `+0` bag: intellect 3 + 1 succeeds, and his `after`
/// reaction opens a window between the result and the acknowledge.
fn investigate_with_milan() -> Vec<Pause> {
    let mut keeper = test_support::test_investigator(1);
    keeper.current_location = Some(PARLOR);
    keeper.cards_in_play.push(CardInPlay::enter_play(
        CardCode::new(MILAN),
        CardInstanceId(50),
        CardOwner::Investigator(KEEPER),
    ));
    let mut parlor = test_support::test_location(1, "Parlor");
    parlor.shroud = 2;
    parlor.clues = 3;
    let mut state = GameStateBuilder::new()
        .with_investigator(keeper)
        .with_location(parlor)
        .with_turn_order([KEEPER])
        .open_turn(KEEPER)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .build();
    state.interactive_acknowledge = true;
    drive(
        state,
        &TurnAction::Investigate {
            investigator: KEEPER,
        },
    )
}

/// Take `action` from the turn menu and answer every prompt through the end of
/// the test, folding each apply into a `ClientState`. See [`fight_ghoul`].
fn drive(state: GameState, action: &TurnAction) -> Vec<Pause> {
    let idx = enumerate::legal_actions(&state, TEST_REGISTRIES.cards)
        .iter()
        .position(|a| a == action)
        .expect("the action is legal on this board");
    let mut result = engine::apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(OptionId(
                u32::try_from(idx).expect("action index fits u32"),
            )),
        }),
        TEST_REGISTRIES,
    );

    let mut client = ClientState::default();
    let mut pauses = Vec::new();
    // Generous bound: the longest path here is four applies. A runaway loop is a
    // failure, not a hang.
    for _ in 0..16 {
        store::reduce(
            &mut client,
            ServerMessage::Applied {
                state: Box::new(result.state.clone()),
                events: result.events.clone(),
                outcome: result.outcome.clone(),
            },
        );
        let EngineOutcome::AwaitingInput { request, .. } = result.outcome.clone() else {
            break;
        };
        pauses.push(Pause {
            prompt: request.kind,
            modal_live: skill_test_result::modal_is_live(&client),
        });
        // The test is over once ST.8 has torn it down; past that is turn
        // plumbing, which drives into the next action menu.
        if result
            .events
            .iter()
            .any(|e| matches!(e, Event::SkillTestEnded { .. }))
        {
            break;
        }
        let response = match request.kind {
            InputKind::Confirm => InputResponse::Confirm,
            // The commit window: commit nothing.
            InputKind::PickMultiple => InputResponse::PickMultiple { selected: vec![] },
            // Lita's reaction window: fire it.
            InputKind::PickSingle => InputResponse::PickSingle(OptionId(0)),
            other => panic!("unexpected prompt kind {other:?}"),
        };
        result = engine::apply(
            result.state.clone(),
            Action::Player(PlayerAction::ResolveInput { response }),
            TEST_REGISTRIES,
        );
    }
    pauses
}

/// Whether the modal was live at the acknowledge `Confirm` — the one pause it
/// renders on.
fn modal_at_acknowledge(pauses: &[Pause]) -> bool {
    pauses
        .iter()
        .find(|p| p.prompt == InputKind::Confirm)
        .map(|p| p.modal_live)
        .expect("the Fight paused to acknowledge its result")
}

/// The control: with no reaction in the way, the resolution and the acknowledge
/// arrive in one batch and the modal renders. This is what made the bug look
/// like Lita's fault rather than the store's.
#[test]
fn a_plain_attack_shows_the_result_modal() {
    assert!(modal_at_acknowledge(&fight_ghoul(false)));
}

/// A `when`-cell reaction between the verdict and the acknowledge: Lita's window
/// comes before the result is logged, so her pick's batch carries both.
#[test]
fn an_attack_through_litas_reaction_window_still_shows_the_result_modal() {
    assert!(modal_at_acknowledge(&fight_ghoul(true)));
}

/// #853: with Dr. Milan in play the result lands in the batch that opens his
/// `after` window, and the acknowledge's batch is empty — so a modal that reads
/// only the live batch never renders, and the player is asked to acknowledge a
/// result they were never shown.
#[test]
fn an_investigation_through_milans_reaction_window_still_shows_the_result_modal() {
    assert!(modal_at_acknowledge(&investigate_with_milan()));
}

/// The latch is not a leak: once the test is torn down the modal is gone, so a
/// later un-anchored `Confirm` cannot resurrect the last test's result.
#[test]
fn the_result_does_not_survive_the_end_of_its_own_test() {
    let mut client = ClientState::default();
    store::reduce(
        &mut client,
        ServerMessage::Applied {
            state: Box::new(board(false)),
            events: vec![
                Event::SkillTestSucceeded {
                    investigator: OTHER,
                    skill: SkillKind::Combat,
                    margin: 1,
                },
                Event::SkillTestEnded {
                    investigator: OTHER,
                },
            ],
            outcome: EngineOutcome::Done,
        },
    );
    assert!(
        skill_test_result::summarize(&client).is_none(),
        "SkillTestEnded clears the retained result",
    );
}
