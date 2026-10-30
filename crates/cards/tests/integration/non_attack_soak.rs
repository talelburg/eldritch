//! Non-attack damage/horror from card and treachery effects is distributed
//! **interactively** across controlled soakers and self.
//!
//! Each point a soaker can take opens a per-point `PickSingle` prompt; the
//! multi-point case (Grasping Hands deals `Count(SkillTestFailedBy)` damage)
//! suspends per point and resumes without losing a point. The damage is one
//! `Deal { amount: N }`, and the per-point prompt loop fires via its
//! `DealDamage` frame. Driven through the real `apply` revelation path.

use cards::TEST_REGISTRIES;
use game_core::state::{
    CardCode, ChaosToken, GameStateBuilder, InvestigatorId, LocationId, MythosPhaseFrame,
    MythosResume, Phase,
};
use game_core::test_support::{self, seat, SeatLookup as _, TestSession};

const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(20);

/// Where a distributed point goes.
#[derive(Clone, Copy)]
enum Onto {
    /// The investigator themself (their investigator card).
    Investigator,
    /// The seated soaker.
    Soaker,
}

/// Mythos step 1.4 with investigator 1 at a location to draw, controlling
/// `soaker`, with `treachery` on top of the encounter deck and one rigged
/// chaos token.
fn board_with_soaker(treachery: &str, soaker: &str, token: ChaosToken) -> TestSession {
    let mut state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(test_support::test_location(HERE.0, "Here"))
        .with_seat(seat(1).at(HERE).in_play([soaker]))
        .with_seating([INV])
        .with_phase(Phase::Mythos)
        .with_phase_anchor(MythosPhaseFrame {
            resume: MythosResume::AfterDraws,
        })
        .with_mythos_draw_remaining([INV])
        .build();
    state.chaos_bag.tokens = vec![token];
    state.encounter_deck.push_back(CardCode::new(treachery));
    TestSession::new(state, TEST_REGISTRIES)
}

/// Draw the top encounter card for investigator 1, committing no cards at the
/// revelation skill-test window, then answering each per-point distribution
/// prompt with `picks`.
fn reveal_distributing(session: TestSession, soaker: &str, picks: &[Onto]) -> TestSession {
    let soaker = session.state().card(INV, soaker);
    let me = session.state().investigators[&INV].card_anchor();
    let session = session.confirm().commit(&[]);
    picks.iter().fold(session, |session, onto| {
        session.pick(match onto {
            Onto::Investigator => me.clone(),
            Onto::Soaker => soaker.clone(),
        })
    })
}

fn soaker_damage(session: &TestSession, soaker: &str) -> u8 {
    let id = session.state().in_play(INV, soaker);
    session.state().investigators[&INV]
        .cards_in_play
        .iter()
        .find(|c| c.instance_id == id)
        .expect("soaker in play")
        .accumulated_damage
}

#[test]
fn grasping_hands_distributes_both_points_onto_guard_dog() {
    // Agility 3 + Numeric(-2) = 1 vs difficulty 3 → fail by 2 → 2 damage, dealt
    // as a single Deal { amount: 2 }. Two per-point prompts still fire via the
    // DealDamage frame at its Distribute step. The player assigns both to Guard
    // Dog, and neither point is lost across the suspension.
    let session = reveal_distributing(
        board_with_soaker("01162", "01021", ChaosToken::Numeric(-2)),
        "01021",
        &[Onto::Soaker, Onto::Soaker],
    );
    // Both points distributed; no soak reaction window for treachery harm, so
    // the Mythos phase ends and the next turn opens.
    session.assert_at_turn_menu(INV);
    let inv = &session.state().investigators[&INV];
    assert_eq!(inv.damage(), 0, "investigator took none (both soaked)");
    assert_eq!(
        soaker_damage(&session, "01021"),
        2,
        "2 damage soaked onto Guard Dog"
    );
}

#[test]
fn grasping_hands_player_splits_across_soaker_and_self() {
    // Same 2-damage fail, but the player soaks one point onto Guard Dog and
    // takes the other themselves; the split proves the distribution is the
    // player's choice, not an automatic soak.
    let session = reveal_distributing(
        board_with_soaker("01162", "01021", ChaosToken::Numeric(-2)),
        "01021",
        &[Onto::Soaker, Onto::Investigator],
    );
    session.assert_at_turn_menu(INV);
    let inv = &session.state().investigators[&INV];
    assert_eq!(inv.damage(), 1, "one point taken by the investigator");
    assert_eq!(
        soaker_damage(&session, "01021"),
        1,
        "one point soaked onto Guard Dog"
    );
}

#[test]
fn rotting_remains_horror_distributes_onto_beat_cop() {
    // Willpower 3 + Numeric(-1) = 2 vs difficulty 3 → fail by 1 → 1 horror. Beat
    // Cop (sanity 2) is eligible, so the point is contested → one prompt; the
    // player assigns it to Beat Cop.
    let session = reveal_distributing(
        board_with_soaker("01163", "01018", ChaosToken::Numeric(-1)),
        "01018",
        &[Onto::Soaker],
    );
    session.assert_at_turn_menu(INV);
    let inv = &session.state().investigators[&INV];
    assert_eq!(inv.horror(), 0, "horror soaked, investigator took none");
    let cop = session.state().in_play(INV, "01018");
    assert_eq!(
        inv.cards_in_play
            .iter()
            .find(|c| c.instance_id == cop)
            .map(|c| c.accumulated_horror),
        Some(1),
        "1 horror soaked onto Beat Cop",
    );
}
