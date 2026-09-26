use super::*;
use crate::state::{
    Act, Agenda, CardInPlay, CardInstanceId, EnemyId, GameStateBuilder, LocationId,
};
use crate::test_support;

const INV: InvestigatorId = InvestigatorId(1);
/// Deliberately resolved by no registry — these tests install none.
/// `candidate_source_present` only compares this code for *identity*
/// against what the board holds; it never looks it up. The prefix is this
/// module's, per ADR 0016.
const SOME_CODE: &str = "_rw_card";

fn candidate(source: CandidateSource) -> ResolutionCandidate {
    ResolutionCandidate::new(
        CardCode::new(SOME_CODE),
        INV,
        AbilityAddress::Printed(0),
        source,
    )
}

#[test]
fn a_hand_candidate_is_present_only_while_the_code_is_in_hand() {
    let mut inv = test_support::test_investigator(1);
    inv.hand.push(CardCode::new(SOME_CODE));
    let state = GameStateBuilder::default().with_investigator(inv).build();
    assert!(candidate_source_present(
        &state,
        &candidate(CandidateSource::Hand)
    ));

    let mut drained = state.clone();
    drained.investigators.get_mut(&INV).unwrap().hand.clear();
    assert!(!candidate_source_present(
        &drained,
        &candidate(CandidateSource::Hand)
    ));
}

#[test]
fn an_in_play_candidate_is_present_only_while_its_instance_is() {
    let instance = CardInstanceId(7);
    let mut inv = test_support::test_investigator(1);
    inv.cards_in_play
        .push(CardInPlay::enter_play(CardCode::new(SOME_CODE), instance));
    let state = GameStateBuilder::default().with_investigator(inv).build();
    assert!(candidate_source_present(
        &state,
        &candidate(CandidateSource::Ability(AbilitySource::InPlay(instance)))
    ));
    // A different instance of the same code is a different candidate.
    assert!(!candidate_source_present(
        &state,
        &candidate(CandidateSource::Ability(AbilitySource::InPlay(
            CardInstanceId(8)
        )))
    ));
}

/// A candidate minted from a **location attachment** — Barricade 01038's
/// `LeftLocation` self-discard, Obscuring Fog 01168's test forced — is
/// present while the attachment is on the board, not only while the
/// controller happens to hold it. The probe is board-wide because the
/// scans that mint these candidates are: the old controller-scoped arm
/// answered "gone" for every attachment-sourced candidate, which is a
/// mislabelled lapse reason rather than a wrong resolution (#735).
#[test]
fn an_attachment_candidate_is_present_though_no_investigator_controls_it() {
    let instance = CardInstanceId(12);
    let mut location = test_support::test_location(10, "Study");
    location
        .attachments
        .push(CardInPlay::enter_play(CardCode::new(SOME_CODE), instance));
    let state = GameStateBuilder::default()
        .with_investigator(test_support::test_investigator(1))
        .with_location(location)
        .build();
    assert!(candidate_source_present(
        &state,
        &candidate(CandidateSource::Ability(AbilitySource::InPlay(instance)))
    ));
}

#[test]
fn an_act_candidate_is_present_only_while_that_act_is_the_current_one() {
    let act = |code: &str| Act {
        code: CardCode::new(code),
        clue_threshold: 0,
    };
    let mut state = GameStateBuilder::default()
        .with_investigator(test_support::test_investigator(1))
        .build();
    state.act_deck = vec![act(SOME_CODE), act("_next_act")];
    state.act_index = 0;
    let cand = ResolutionCandidate::new(
        CardCode::new(SOME_CODE),
        INV,
        AbilityAddress::Printed(0),
        CandidateSource::Ability(AbilitySource::Act),
    );
    assert!(candidate_source_present(&state, &cand));

    // The act advanced: the source did not leave the board, it became a
    // different card — and a candidate minted from the old one is gone all
    // the same, which is the question the code comparison asks (#735).
    let mut advanced = state.clone();
    advanced.act_index = 1;
    assert!(!candidate_source_present(&advanced, &cand));

    // No act deck at all: nothing for `AbilitySource::Act` to name.
    let empty = GameStateBuilder::default()
        .with_investigator(test_support::test_investigator(1))
        .build();
    assert!(!candidate_source_present(&empty, &cand));
}

#[test]
fn an_agenda_candidate_is_present_only_while_that_agenda_is_the_current_one() {
    let mut state = GameStateBuilder::default()
        .with_investigator(test_support::test_investigator(1))
        .build();
    state.agenda_deck = vec![Agenda {
        code: CardCode::new(SOME_CODE),
        doom_threshold: 3,
    }];
    state.agenda_index = 0;
    let cand = ResolutionCandidate::new(
        CardCode::new(SOME_CODE),
        INV,
        AbilityAddress::Printed(0),
        CandidateSource::Ability(AbilitySource::Agenda),
    );
    assert!(candidate_source_present(&state, &cand));

    let mut past_the_end = state.clone();
    past_the_end.agenda_index = 1;
    assert!(!candidate_source_present(&past_the_end, &cand));
}

/// Silver Twilight Acolyte 01102's forced doom: the attacking enemy is the
/// source, so the probe asks whether that enemy is still on the board —
/// where a `Board` candidate used to compare its code against the current
/// act and agenda and always answer "gone" (#735).
#[test]
fn an_enemy_candidate_tracks_its_enemy() {
    let mut enemy = test_support::test_enemy(4, "Silver Twilight Acolyte");
    enemy.code = CardCode::new(SOME_CODE);
    let state = GameStateBuilder::default()
        .with_investigator(test_support::test_investigator(1))
        .with_enemy(enemy)
        .build();
    let cand = |id| {
        ResolutionCandidate::new(
            CardCode::new(SOME_CODE),
            INV,
            AbilityAddress::Printed(0),
            CandidateSource::Ability(AbilitySource::Enemy(id)),
        )
    };
    assert!(candidate_source_present(&state, &cand(EnemyId(4))));
    assert!(!candidate_source_present(&state, &cand(EnemyId(5))));

    let mut defeated = state.clone();
    defeated.enemies.remove(&EnemyId(4));
    assert!(!candidate_source_present(&defeated, &cand(EnemyId(4))));
}

#[test]
fn a_location_candidate_tracks_its_location() {
    let loc = LocationId(10);
    // The candidate's code is the location's own printed code — the probe
    // asks whether the source still names *the same card*, and a location's
    // `LocationId` and code move together.
    let mut location = test_support::test_location(10, "Study");
    location.code = CardCode::new(SOME_CODE);
    let state = GameStateBuilder::default()
        .with_investigator(test_support::test_investigator(1))
        .with_location(location)
        .build();
    assert!(candidate_source_present(
        &state,
        &candidate(CandidateSource::Ability(AbilitySource::Location(loc)))
    ));
    assert!(!candidate_source_present(
        &state,
        &candidate(CandidateSource::Ability(AbilitySource::Location(
            LocationId(11)
        )))
    ));
}
