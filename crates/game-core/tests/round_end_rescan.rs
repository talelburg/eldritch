//! §G per-cell eligibility re-scan (EmitEvent-frame C-coordinators, #434).
//!
//! The `RoundEnded` coordinator walks `when → at → after`, re-scanning each cell
//! fresh — a `when` reaction can change whether an `at` forced even fires, so the
//! grid is **not** pre-computed. No in-scope corpus card exercises cross-bucket
//! suppression at one round-end emit, so this uses a synthetic mock registry:
//!
//! - a test **act** carries a `When`-`RoundEnded` reaction whose native removes a
//!   threat-area card from play;
//! - that threat-area card (`TESTX`) carries an `At`-`RoundEnded` *forced* ability
//!   that gives its controller a clue.
//!
//! Picking the `when` reaction removes `TESTX` before the `at` cell is scanned, so
//! the `at` forced does **not** fire (no clue gained). Skipping leaves `TESTX` in
//! play, so the `at` forced fires (one clue). The difference is the re-scan.

use std::sync::OnceLock;

use card_dsl::dsl::{
    forced_on_event, native, reaction_on_event, Ability, EventPattern, EventTiming,
};
use game_core::action::{InputResponse, PlayerAction};
use game_core::card_data::CardMetadata;
use game_core::card_registry::{self, CardRegistry, NativeEffectFn};
use game_core::engine::OptionId;
use game_core::state::{
    Act, CardCode, CardInPlay, CardInstanceId, Continuation, GameState, InvestigatorId, Phase,
    UpkeepResume,
};
use game_core::test_support::{run_upkeep_round_end, test_investigator, GameStateBuilder};
use game_core::{apply, Action, Cx, EngineOutcome, EvalContext};

const TEST_ACT: &str = "TESTACT";
const TEST_X: &str = "TESTX";

/// `when`-cell native: remove `TESTX` from every investigator's threat area —
/// flipping the `at`-cell forced's eligibility (its source leaves play).
fn when_remove_x(cx: &mut Cx, _ctx: &EvalContext) -> EngineOutcome {
    for inv in cx.state.investigators.values_mut() {
        inv.threat_area.retain(|c| c.code.as_str() != TEST_X);
    }
    EngineOutcome::Done
}

/// `at`-cell forced native: give the controller (the card's owner) one clue —
/// an observable signal Mythos doesn't touch.
fn at_gain_clue(cx: &mut Cx, ctx: &EvalContext) -> EngineOutcome {
    if let Some(inv) = cx.state.investigators.get_mut(&ctx.controller) {
        inv.clues += 1;
    }
    EngineOutcome::Done
}

fn mock_abilities_for(code: &CardCode) -> Option<Vec<Ability>> {
    match code.as_str() {
        TEST_ACT => Some(vec![reaction_on_event(
            EventPattern::RoundEnded,
            EventTiming::When,
            native("when:remove_x"),
        )]),
        TEST_X => Some(vec![forced_on_event(
            EventPattern::RoundEnded,
            EventTiming::At,
            native("at:gain_clue"),
        )]),
        _ => None,
    }
}

fn mock_native_for(tag: &str) -> Option<NativeEffectFn> {
    match tag {
        "when:remove_x" => Some(when_remove_x as NativeEffectFn),
        "at:gain_clue" => Some(at_gain_clue as NativeEffectFn),
        _ => None,
    }
}

fn mock_metadata_for(_: &CardCode) -> Option<&'static CardMetadata> {
    None
}

fn install() {
    static INSTALL: OnceLock<()> = OnceLock::new();
    INSTALL.get_or_init(|| {
        let _ = card_registry::install(CardRegistry {
            metadata_for: mock_metadata_for,
            abilities_for: mock_abilities_for,
            native_effect_for: mock_native_for,
        });
    });
}

/// Upkeep, the test act current, the lead holding `TESTX` (the `at`-forced
/// source) in their threat area with 0 clues.
fn rescan_state() -> GameState {
    install();
    let inv = InvestigatorId(1);
    let mut investigator = test_investigator(1);
    investigator.clues = 0;
    investigator.threat_area.push(CardInPlay::enter_play(
        CardCode::new(TEST_X),
        CardInstanceId(0),
    ));
    let mut state = GameStateBuilder::new()
        .with_phase(Phase::Upkeep)
        .with_phase_anchor(Continuation::UpkeepPhase {
            resume: UpkeepResume::Begins,
        })
        .with_investigator(investigator)
        .with_turn_order([inv])
        .build();
    state.act_deck = vec![Act {
        code: CardCode::new(TEST_ACT),
        clue_threshold: 0,
        resolution: None,
    }];
    state.act_index = 0;
    state
}

#[test]
fn when_cell_picked_suppresses_the_at_forced() {
    let mut state = rescan_state();
    let mut events = Vec::new();
    let out = run_upkeep_round_end(&mut state, &mut events);
    assert!(
        matches!(out, EngineOutcome::AwaitingInput { .. }),
        "the `when` reaction window opens: {out:?}"
    );
    // Pick the `when` reaction (sole candidate): it removes TESTX before the
    // `at` cell is scanned, so the `at` forced never fires.
    let r = apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(OptionId(0)),
        }),
    );
    assert_eq!(
        r.state.investigators[&InvestigatorId(1)].clues,
        0,
        "the `at` forced must NOT fire: the `when` cell removed its source (per-cell re-scan)"
    );
}

#[test]
fn when_cell_skipped_leaves_the_at_forced_eligible() {
    // Control: skip the `when` reaction → TESTX stays in play → the `at` forced
    // fires (one clue). Isolates "the re-scan suppressed it" from "it never fired".
    let mut state = rescan_state();
    let mut events = Vec::new();
    let _ = run_upkeep_round_end(&mut state, &mut events);
    let r = apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::Skip,
        }),
    );
    assert_eq!(
        r.state.investigators[&InvestigatorId(1)].clues,
        1,
        "the `at` forced fires when the `when` cell leaves its source in play"
    );
}
