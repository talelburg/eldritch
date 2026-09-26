use super::*;
use crate::state::{EnemyId, LocationId};

#[test]
fn resolution_options_anchor_by_candidate_source() {
    let cands = vec![
        ResolutionCandidate {
            code: CardCode::new("_inplay"),
            controller: InvestigatorId(1),
            address: AbilityAddress::Printed(0),
            source: CandidateSource::Ability(AbilitySource::InPlay(CardInstanceId(9))),
        },
        ResolutionCandidate {
            code: CardCode::new("01022"),
            controller: InvestigatorId(1),
            address: AbilityAddress::Printed(0),
            source: CandidateSource::Hand,
        },
        ResolutionCandidate {
            code: CardCode::new("01109"),
            controller: InvestigatorId(1),
            address: AbilityAddress::Printed(0),
            source: CandidateSource::Ability(AbilitySource::Act),
        },
    ];
    let opts = build_resolution_options(&cands);
    assert_eq!(
        opts[0].target,
        Some(OptionTarget::CardInstance(CardInstanceId(9)))
    );
    assert_eq!(
        opts[1].target,
        Some(OptionTarget::HandCardByCode {
            investigator: InvestigatorId(1),
            code: CardCode::new("01022"),
        })
    );
    assert_eq!(
        opts[2].target,
        Some(OptionTarget::Act),
        "an act-sourced candidate anchors to the act card — no act deck is seeded here, \
         because the source says which board card it is rather than the reader deriving it \
         from the code (#735)",
    );
}

/// Every source kind maps to its own anchor, and **none of them is
/// un-anchored** — a board candidate used to fall through to the then-`Global`
/// variant whenever its code matched neither the current act nor the current
/// agenda, which is exactly what an attacking enemy's own forced ability did
/// (#735).
#[test]
fn candidate_anchor_maps_each_source() {
    let anchor_of = |source| {
        candidate_anchor(&ResolutionCandidate::new(
            CardCode::new("_code"),
            InvestigatorId(2),
            AbilityAddress::Printed(0),
            source,
        ))
    };

    assert_eq!(
        anchor_of(CandidateSource::Ability(AbilitySource::InPlay(
            CardInstanceId(5)
        ))),
        OptionTarget::CardInstance(CardInstanceId(5)),
    );
    // A location's own forced ability (the Attic, 01113) anchors to its map
    // node (#553).
    assert_eq!(
        anchor_of(CandidateSource::Ability(AbilitySource::Location(
            LocationId(7)
        ))),
        OptionTarget::Location(LocationId(7)),
    );
    // Silver Twilight Acolyte 01102's forced doom anchors to the attacking
    // enemy, where it used to anchor to nothing (#735).
    assert_eq!(
        anchor_of(CandidateSource::Ability(AbilitySource::Enemy(EnemyId(3)))),
        OptionTarget::Enemy(EnemyId(3)),
    );
    assert_eq!(
        anchor_of(CandidateSource::Ability(AbilitySource::Act)),
        OptionTarget::Act,
    );
    // What's Going On?! 01105's on-advance forced (#556).
    assert_eq!(
        anchor_of(CandidateSource::Ability(AbilitySource::Agenda)),
        OptionTarget::Agenda,
    );
    assert_eq!(
        anchor_of(CandidateSource::Hand),
        OptionTarget::HandCardByCode {
            investigator: InvestigatorId(2),
            code: CardCode::new("_code"),
        },
    );
}
