//! Act-3 objective: defeating the Ghoul Priest (01116) advances Act 3 (01110),
//! whose reverse offers two printed resolution points behind *"The lead
//! investigator must decide (choose one)"*.
//!
//! Here an enemy printed as the Ghoul Priest is defeated by a real Fight
//! against the real registry. The scenario's end-to-end defeat-to-ending path
//! is `scenarios/tests/integration/the_gathering_resolutions.rs`.

use card_dsl::card_data::SkillKind;
use cards::TEST_REGISTRIES;
use game_core::assert_event;
use game_core::engine::enumerate::TurnAction;
use game_core::engine::OptionTarget;
use game_core::event::Event;
use game_core::scenario::{ResolutionId, ScenarioEnding};
use game_core::state::{
    ChaosBag, ChaosToken, EnemyId, GameState, GameStateBuilder, InvestigatorId, LocationId,
};
use game_core::test_support::{self, seat, Seat, TestSession};

const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(10);
const FOE: EnemyId = EnemyId(1);

/// `investigator`'s open turn at the Study, under Act 3 (01110), with
/// `board` adding what the test needs — or replacing the act deck.
fn act3_state(
    investigator: Seat,
    board: impl FnOnce(GameStateBuilder) -> GameStateBuilder,
) -> GameState {
    // Act 3 is current and terminal-Won (mirrors the_gathering setup()).
    board(
        GameStateBuilder::new()
            .with_registries(TEST_REGISTRIES)
            .with_location(test_support::location(HERE))
            .with_seat(investigator)
            .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
            .with_act_deck(["01110"]),
    )
    .open_turn(INV)
    .build()
}

/// Defeat the printed enemy `code`, for real: it is engaged at the Study
/// with `damage` on it, one short of its printed health, and a Fight (combat
/// 4 against a printed fight of at most 4, a `0` token, committing nothing)
/// deals the last one.
fn defeat(code: &str, damage: u8) -> TestSession {
    let enemy = test_support::enemy(FOE).as_card(code).damage(damage);
    let session = TestSession::new(
        act3_state(seat(1).at(HERE).skill(SkillKind::Combat, 4), |b| {
            b.with_enemy(enemy.engaged_with(INV))
        }),
        TEST_REGISTRIES,
    )
    .take(&TurnAction::Fight {
        investigator: INV,
        enemy: FOE,
    })
    .commit(&[]);
    assert_event!(
        session.events(),
        Event::EnemyDefeated { enemy, .. } if *enemy == FOE
    );
    session
}

/// Defeat the Ghoul Priest, 01110's objective; the session rests on the
/// prompt its reverse suspends on.
fn defeat_the_ghoul_priest() -> TestSession {
    // The Ghoul Priest: fight 4, 5 health per investigator.
    defeat("01116", 4)
}

/// Regression for #775: the printed reverse offers both resolution points,
/// verbatim, rather than latching R1 without asking. It anchors them to the
/// act — the card the choice is printed on, whose reverse face the advance is
/// already showing (ADR 0011).
#[test]
fn defeating_ghoul_priest_offers_the_lead_both_printed_resolution_points() {
    let session = defeat_the_ghoul_priest();
    assert!(
        session.state().ending.is_none(),
        "nothing latches before the pick"
    );
    let request = session.prompt();
    let labels: Vec<&str> = request.options.iter().map(|o| o.label.as_str()).collect();
    assert_eq!(
        labels,
        [
            "It was never much of a home. Burn it down! (→R1)",
            "This hell-pit is my home! No way are we burning it! (→R2)",
        ],
    );
    for option in &request.options {
        assert_eq!(
            option.target,
            Some(OptionTarget::Act),
            "the choice is printed on the act, so it renders there",
        );
    }
}

/// Picking the first bullet reaches R1; the second, R2. Both are the *same*
/// prompt — the only difference is the pick — and each lands on its own
/// `ScenarioEnding`, which is what the board's ending banner renders off
/// (ADR 0012).
#[test]
fn each_printed_bullet_reaches_its_own_resolution() {
    for (pick, expected) in [(0, 1u8), (1, 2)] {
        let result = defeat_the_ghoul_priest().pick_nth(pick);
        assert_eq!(
            result.state().ending,
            Some(ScenarioEnding::Resolution(ResolutionId::new(expected))),
            "pick {pick} should reach Resolution {expected}, got {:?}",
            result.state().ending,
        );
    }
}

#[test]
fn defeating_other_enemy_does_not_advance_act_3() {
    // Some other enemy, not the Ghoul Priest.
    // The Stubborn Detective: fight 3, health 2.
    let session = defeat("01103", 1);
    session.assert_at_turn_menu(INV);
    assert!(
        session.state().ending.is_none(),
        "only the Ghoul Priest's defeat advances Act 3"
    );
}

/// Regression for #486: Act 01110 ("What Have You Done?") advances only via its
/// Forced `EnemyDefeated` objective (the Ghoul Priest), so its corpus clue
/// threshold is `null` -> 0. The deliberate clue-spend `AdvanceAct` action is
/// rejected for it — otherwise the player could "spend 0 clues to advance" and
/// instantly latch the terminal Won resolution, bypassing the Ghoul Priest fight.
/// The legitimate defeat path stays covered by
/// `defeating_ghoul_priest_advances_act_3_to_won` above.
#[test]
fn advance_act_action_rejected_for_act_3_objective() {
    // 5 clues: plenty — reject must be the objective, not affordability.
    let state = act3_state(seat(1).at(HERE).clues(5), |b| b);

    // The menu does not offer it; submitted off the menu, the handler itself
    // must reject it.
    let session = TestSession::new(state, TEST_REGISTRIES)
        .submit_off_menu(&TurnAction::AdvanceAct { investigator: INV });
    // AdvanceAct must be rejected for Act 3's non-clue objective.
    session.expect_rejected();
    let state = session.state();
    assert!(
        state.ending.is_none(),
        "rejected AdvanceAct must not latch the Won resolution (no instant win)"
    );
    assert_eq!(state.act_index, 0, "act did not advance");
    assert_eq!(state.investigators[&INV].clues, 5, "no clues spent");
}

/// Act 01109 ("The Barrier") advances only at the end of the round (its
/// `When`-`RoundEnded` group objective), so the `AdvanceAct` *action* is rejected.
/// Registry-based detection (`act_advances_at_round_end`) — needs the real
/// registry, so it lives here rather than as a game-core lib unit test.
#[test]
fn advance_act_rejected_for_round_end_advance_act() {
    // 9 clues: plenty — reject must be the objective, not affordability.
    let state = act3_state(seat(1).at(HERE).clues(9), |b| b.with_act_deck(["01109"]));

    // The menu does not offer it; submitted off the menu, the handler itself
    // must reject it.
    let session = TestSession::new(state, TEST_REGISTRIES)
        .submit_off_menu(&TurnAction::AdvanceAct { investigator: INV });
    session.expect_rejected();
    assert_eq!(session.state().act_index, 0, "act did not advance");
    assert_eq!(
        session.state().investigators[&INV].clues,
        9,
        "no clues spent"
    );
}
