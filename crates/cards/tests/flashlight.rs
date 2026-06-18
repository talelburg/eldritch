//! PR-7 (#313) integration: Flashlight 01087's `[action] Spend 1 supply:
//! Investigate. Your location gets -2 shroud for this investigation.`
//! end-to-end against the real `cards::REGISTRY`.
//!
//! Exercises the new `Effect::Investigate` shroud modifier: the -2 lowers
//! the location difficulty (clamped at 0), the test reuses the base
//! Investigate follow-up (so a success discovers a clue), and the
//! activation rejects before spending a supply when there is no revealed
//! location to investigate.
//!
//! Own process → installs `cards::REGISTRY`.

use std::sync::Once;

use game_core::engine::EngineOutcome;
use game_core::event::Event;
use game_core::state::{
    CardCode, CardInPlay, CardInstanceId, ChaosBag, ChaosToken, InvestigatorId, LocationId, Phase,
    SkillKind, TokenModifiers, UseKind,
};
use game_core::test_support::{
    apply_no_commits, test_investigator, test_location, GameStateBuilder,
};
use game_core::{assert_event, Action, PlayerAction};

const FLASHLIGHT: &str = "01087";
const INV: InvestigatorId = InvestigatorId(1);
const LOC: LocationId = LocationId(10);
const TORCH_INST: CardInstanceId = CardInstanceId(0);

static INSTALL: Once = Once::new();
fn install() {
    INSTALL.call_once(|| {
        let _ = game_core::card_registry::install(cards::REGISTRY);
    });
}

/// Board: Flashlight in play with 3 supplies; the active investigator at a
/// revealed `shroud`-shroud location holding 1 clue, with `intellect`
/// intellect. A `Numeric(0)` chaos bag makes the Investigate deterministic.
fn board(intellect: i8, shroud: u8, revealed: bool) -> game_core::GameState {
    install();
    let mut inv = test_investigator(1);
    inv.skills.intellect = intellect;
    let mut torch = CardInPlay::enter_play(CardCode::new(FLASHLIGHT), TORCH_INST);
    torch.uses.insert(UseKind::Supplies, 3);
    inv.cards_in_play.push(torch);

    let mut location = test_location(10, "Study");
    location.shroud = shroud;
    location.clues = 1;
    location.revealed = revealed;

    GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator_at(inv, LOC)
        .with_location(location)
        .with_active_investigator(INV)
        .with_turn_order([INV])
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .build()
}

fn supplies(state: &game_core::GameState) -> Option<u8> {
    state.investigators[&INV]
        .cards_in_play
        .iter()
        .find(|c| c.instance_id == TORCH_INST)
        .map(|c| c.uses.get(&UseKind::Supplies).copied().unwrap_or(0))
}

fn activate(state: game_core::GameState) -> game_core::engine::ApplyResult {
    apply_no_commits(
        state,
        Action::Player(PlayerAction::ActivateAbility {
            investigator: INV,
            instance_id: TORCH_INST,
            ability_index: 0,
        }),
    )
}

#[test]
fn minus_two_shroud_turns_a_failing_investigation_into_a_success() {
    // Intellect 2 + 0 = 2. Base difficulty (shroud 4) would fail; the -2
    // brings it to 2 → success by 0, discovering the location's clue.
    let r = activate(board(2, 4, true));
    assert_eq!(r.outcome, EngineOutcome::Done);
    assert_event!(
        r.events,
        Event::SkillTestSucceeded { investigator, skill: SkillKind::Intellect, margin: 0 }
            if *investigator == INV
    );
    assert_event!(r.events, Event::CluePlaced { investigator, .. } if *investigator == INV);
    assert_eq!(r.state.investigators[&INV].clues, 1, "1 clue discovered");
    assert_eq!(r.state.locations[&LOC].clues, 0, "clue left the location");
    assert_eq!(supplies(&r.state), Some(2), "1 supply spent");
}

#[test]
fn reduced_shroud_clamps_at_zero() {
    // Shroud 1, -2 → difficulty (1 - 2).max(0) = 0, not -1 (which would
    // reject as a negative difficulty). Intellect 0 + 0 = 0 ≥ 0 → success.
    let r = activate(board(0, 1, true));
    assert_eq!(r.outcome, EngineOutcome::Done);
    assert_event!(
        r.events,
        Event::SkillTestSucceeded { investigator, skill: SkillKind::Intellect, margin: 0 }
            if *investigator == INV
    );
    assert_eq!(supplies(&r.state), Some(2), "1 supply spent");
}

#[test]
fn rejects_without_a_revealed_location_before_spending_a_supply() {
    // Validate-first: an unrevealed location is not investigatable, so the
    // activation rejects before the supply cost is paid.
    let r = activate(board(2, 4, false));
    assert!(matches!(r.outcome, EngineOutcome::Rejected { .. }));
    assert_eq!(
        supplies(&r.state),
        Some(3),
        "no supply spent on a rejected activation"
    );
}
