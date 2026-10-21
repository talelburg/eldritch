//! The Parlor 01115's `[action]` **Resign**, end to end: the whole
//! elimination trail an investigator leaves behind when they walk out.
//!
//! ## Verified card text (`data/arkhamdb-snapshot`, 2026-08-29)
//!
//! **Parlor (01115):** "[action] <b>Resign.</b> \"This is too much for me!\"
//! You run out the front door, fleeing in panic." (Shroud 2, 0 clues. The
//! Parley the Parlor grants Lita Chantler is exercised in `lita_parley.rs`, not
//! here. The back side's barrier lives in `parlor_barrier.rs`; the Parlor is **revealed** throughout this file, which
//! is the side the Resign is printed on.)
//!
//! ## The rules being asserted
//!
//! `glossary/Resign.md`, in full:
//!
//! > Some abilities are identified with a **Resign** action designator. Such
//! > abilities are initiated using the "Activate" action (see "Activate
//! > Action" on page 4).
//! >
//! > - When an investigator resigns, the investigator is eliminated by
//! >   resignation (see "Elimination" on page 10.) An investigator who
//! >   resigns is not considered to have been defeated.
//!
//! `glossary/Elimination.md`, opening: *"A player is eliminated from a scenario
//! any time his or her investigator is defeated, or if he or she resigns"* —
//! then steps 0–6 once, for both. So the trail below is the ordinary
//! elimination trail, and the only thing marking it as a resignation is the
//! cause it carries.
//!
//! Lives in `crates/cards/tests/` because every assertion needs the real corpus:
//! the Parlor's metadata and its `abilities()`, resolved through
//! `cards::REGISTRY`. The engine-side peers are `elimination.rs`'s unit tests
//! (the steps themselves) and `game-core/tests/action_designator_aoo.rs` (the
//! designator's attack-of-opportunity exemption).

use cards::TEST_REGISTRIES;
use game_core::engine::enumerate::{self, TurnAction};
use game_core::engine::{ApplyResult, EngineOutcome};
use game_core::event::Event;
use game_core::scenario::ScenarioEnding;
use game_core::state::{
    AbilityAddress, AbilitySource, CardCode, EliminationCause, GameStateBuilder, InvestigatorId,
    LocationId, Status,
};
use game_core::test_support::seat;
use game_core::{assert_event, test_support};

/// The Parlor.
const PARLOR_CODE: &str = "01115";
/// Machete 01020 — an ordinary asset, standing in for "the cards he or she
/// controls in play".
const MACHETE: &str = "01020";
/// Holy Rosary 01059 — an ordinary card in hand.
const ROSARY: &str = "01059";

const RESIGNER: InvestigatorId = InvestigatorId(1);
const SURVIVOR: InvestigatorId = InvestigatorId(2);

const PARLOR: LocationId = LocationId(1);
const HALLWAY: LocationId = LocationId(2);

/// The Parlor prints exactly one implemented ability, so index 0 is the Resign.
const RESIGN: u8 = 0;

/// The resigner's turn: they stand at the Parlor holding 2 clues and 3
/// resources, with a Machete in play and a Rosary in hand; a survivor stands
/// in the Hallway so the scenario keeps running. `solo` drops the survivor,
/// which is what makes Elimination step 6 reachable.
fn board(solo: bool) -> GameStateBuilder {
    let mut parlor = test_support::test_location(PARLOR.0, "Parlor");
    parlor.code = CardCode::new(PARLOR_CODE);
    parlor.clues = 1;

    let builder = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(
            seat(RESIGNER.0)
                .at(PARLOR)
                .clues(2)
                .resources(3)
                .hand([ROSARY])
                .in_play([MACHETE]),
        )
        .with_location(parlor)
        .open_turn(RESIGNER);

    if solo {
        builder.with_turn_order([RESIGNER])
    } else {
        builder
            .with_seat(seat(SURVIVOR.0).at(HALLWAY))
            .with_location(test_support::test_location(HALLWAY.0, "Hallway"))
            .with_turn_order([RESIGNER, SURVIVOR])
    }
}

fn resign_action() -> TurnAction {
    TurnAction::ActivateAbility {
        investigator: RESIGNER,
        source: AbilitySource::Location(PARLOR),
        address: AbilityAddress::Printed(RESIGN),
    }
}

fn resign(board: GameStateBuilder) -> ApplyResult {
    let result = board.session().take(&resign_action()).finish();
    assert!(
        !matches!(result.outcome, EngineOutcome::Rejected { .. }),
        "the Parlor's Resign should resolve; got {:?}",
        result.outcome,
    );
    result
}

/// *"Such abilities are initiated using the 'Activate' action"* — so it is an
/// ordinary activation of the location you are standing at, and the turn menu
/// has to offer it.
#[test]
fn standing_at_the_parlor_offers_the_resign_action() {
    let state = board(false).build();
    assert!(
        enumerate::legal_actions(&state, TEST_REGISTRIES.cards).contains(&resign_action()),
        "the Parlor's Resign belongs in the turn menu; menu was {:?}",
        enumerate::legal_actions(&state, TEST_REGISTRIES.cards),
    );
}

/// *"the investigator is eliminated by resignation … not considered to have
/// been defeated"* — the status and the cause say resignation, not defeat.
#[test]
fn resigning_eliminates_by_resignation() {
    let result = resign(board(false));

    assert_event!(
        result.events,
        Event::InvestigatorEliminated { investigator, cause }
            if *investigator == RESIGNER && *cause == EliminationCause::Resigned
    );
    assert_eq!(
        result.state.investigators[&RESIGNER].status,
        Status::Resigned,
        "resigned, not defeated",
    );
}

/// Elimination step 2: *"All clue tokens that player possesses are placed at the
/// location the investigator was at when he or she was eliminated"* — the
/// Parlor. The Gathering's act 1 counts clues on locations, so a resigner's two
/// clues stay countable after they leave.
#[test]
fn the_resigners_clues_are_left_at_the_parlor() {
    let result = resign(board(false));

    assert_eq!(
        result.state.locations[&PARLOR].clues, 3,
        "the Parlor's own clue plus the two the resigner was carrying",
    );
    assert_eq!(
        result.state.investigators[&RESIGNER].clues, 0,
        "the resigner possesses no clues after step 2",
    );
    assert_event!(
        result.events,
        Event::LocationCluesChanged { location, new_count }
            if *location == PARLOR && *new_count == 3
    );
    assert_eq!(
        result.state.investigators[&RESIGNER].resources, 0,
        "step 2 also returns resources to the token pool",
    );
}

/// Elimination step 1: *"The cards he or she controls in play and all of the
/// cards in his or her out-of-play areas … are removed from the game."*
#[test]
fn the_resigners_cards_are_removed_from_the_game() {
    let result = resign(board(false));
    let resigner = &result.state.investigators[&RESIGNER];

    assert!(resigner.cards_in_play.is_empty(), "in play is drained");
    assert!(resigner.hand.is_empty(), "hand is drained");
    let removed: Vec<&str> = resigner
        .removed_from_game
        .iter()
        .map(CardCode::as_str)
        .collect();
    assert!(
        removed.contains(&MACHETE) && removed.contains(&ROSARY),
        "both cards are removed from the game; removed was {removed:?}",
    );
    assert_eq!(
        resigner.current_location, None,
        "the resigner has left play — they are at no location",
    );
}

/// Elimination step 3: an enemy engaged with the resigner is *"placed at the
/// location the investigator was at when he or she was eliminated, unengaged
/// but otherwise maintaining their current game state"* — and it takes no
/// attack of opportunity on the way out, because
/// `glossary/Attack_of_Opportunity.md` exempts *"an action other than to
/// **fight**, to **evade**, or to activate a **parley** or **resign** ability"*.
#[test]
fn an_engaged_enemy_is_left_at_the_parlor_and_takes_no_parting_shot() {
    let ghoul = test_support::test_enemy(1, "Ghoul Minion");
    let ghoul_id = ghoul.id;

    let result = resign(board(false).with_enemy_engaged(ghoul, RESIGNER));

    assert_eq!(
        result.state.enemies[&ghoul_id].engaged_with, None,
        "step 3 disengages, and there is no surviving investigator here to re-engage",
    );
    assert_eq!(
        result.state.enemies[&ghoul_id].current_location,
        Some(PARLOR),
        "the enemy stays where it was",
    );
    assert_eq!(
        result.state.investigators[&RESIGNER]
            .investigator_card
            .accumulated_damage,
        0,
        "a Resign ability provokes no attack of opportunity, so the ghoul never \
         lands a parting shot; events were {:?}",
        result.events,
    );
}

/// Elimination step 6: *"If there are no remaining players, the scenario
/// ends."* The last investigator resigning ends the scenario at **no
/// resolution** — not at a resolution point, and not a loss (`GLOSSARY.md`, **No
/// resolution reached**).
#[test]
fn the_last_investigator_resigning_ends_the_scenario_at_no_resolution() {
    let result = resign(board(true));

    assert_event!(result.events, Event::AllInvestigatorsEliminated);
    assert_eq!(
        result.state.ending,
        Some(ScenarioEnding::NoResolution),
        "the scenario ended with no resolution point reached",
    );
}

/// A survivor keeps the scenario alive: one investigator resigning is not the
/// end of it.
#[test]
fn a_survivor_keeps_the_scenario_running() {
    let result = resign(board(false));

    assert_eq!(
        result.state.investigators[&SURVIVOR].status,
        Status::Active,
        "the survivor is untouched",
    );
    assert_eq!(
        result.state.ending, None,
        "there is a remaining player, so step 6 does not fire",
    );
}
