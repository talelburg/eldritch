//! C6b integration: Dr. Milan Christopher 01033 end-to-end against the
//! real `cards::REGISTRY` — the +1 intellect constant ability and the
//! after-successful-investigate reaction (engine window from #241).
//!
//! Own process → installs `cards::REGISTRY`.

use cards::TEST_REGISTRIES;
use game_core::engine::enumerate::TurnAction;
use game_core::engine::OptionTarget;
use game_core::state::{
    CardCode, CardInPlay, CardInstanceId, ChaosBag, ChaosToken, GameStateBuilder, InvestigatorId,
    Location, LocationId, Owner, SkillKind, TokenModifiers,
};
use game_core::test_support::{self, seat, SeatLookup as _, TestSession};

const DR_MILAN: &str = "01033";
/// Obscuring Fog.
const OBSCURING_FOG: &str = "01168";
const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(10);
/// Obscuring Fog's instance, attached to the Study. A seat cannot attach a
/// card to a location, so its id is named here; seated cards are minted past
/// it.
const FOG_ID: CardInstanceId = CardInstanceId(2);

/// The investigator's turn at a 1-clue Study of shroud 2 (`study` adds to
/// it), with base intellect `intellect`, Dr. Milan in play, and a
/// `Numeric(0)` bag.
fn board(intellect: i8, study: impl FnOnce(&mut Location)) -> TestSession {
    let mut loc = test_support::test_location(HERE.0, "Study"); // shroud 2 by default
    loc.clues = 1;
    study(&mut loc);
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(
            seat(1)
                .at(HERE)
                .skill(SkillKind::Intellect, intellect)
                .in_play([DR_MILAN]),
        )
        .with_location(loc)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .open_turn(INV)
        .session()
}

/// Investigate and commit nothing; the session rests at the after-investigate
/// reaction window, which offers Dr. Milan.
fn investigate_to_dr_milans_window(session: TestSession) -> (TestSession, OptionTarget) {
    let dr_milan = session.state().card(INV, DR_MILAN);
    let window = session
        .take(&TurnAction::Investigate { investigator: INV })
        .commit(&[]);
    assert_eq!(
        window.offered_at(&dr_milan),
        1,
        "the investigate should succeed and open Dr. Milan's window, got {:?}",
        window.prompt(),
    );
    (window, dr_milan)
}

/// Board: **base intellect 1**. Without Dr. Milan's +1 intellect the
/// Investigate (1 vs 2) would fail; with it (2 vs 2) it succeeds — so the
/// constant ability is load-bearing here.
#[test]
fn dr_milan_plus_one_intellect_succeeds_then_reaction_gains_resource() {
    let session = board(1, |_| {});
    let resources_before = session.state().investigators[&INV].resources;

    // Investigate → commit nothing → intellect 1 + Dr. Milan's +1 = 2 ≥ shroud
    // 2 → success → clue discovered → after-investigate window suspends.
    let (window, dr_milan) = investigate_to_dr_milans_window(session);

    // Fire the reaction → gain 1 resource → back to the turn menu.
    let resumed = window.pick(dr_milan);
    resumed.assert_at_turn_menu(INV);
    assert_eq!(resumed.state().locations[&HERE].clues, 0, "clue discovered");
    assert_eq!(
        resumed.state().investigators[&INV].resources,
        resources_before + 1,
        "Dr. Milan's reaction gained a resource",
    );
}

#[test]
fn obscuring_fog_forced_discard_precedes_dr_milan_reaction_window() {
    // #213 forced-before-reaction: the "after you successfully investigate"
    // timing point fires Obscuring Fog 01168's *forced* discard before Dr.
    // Milan 01033's *reaction* window opens (RR p.2). Observable: at the
    // moment the reaction window suspends, Obscuring Fog is already gone —
    // pre-T5b it was still attached (the forced fired a later driver step).
    //
    // Effective shroud is 2 (printed) + 2 (Obscuring Fog) = 4. Intellect 4 +
    // Dr. Milan's +1 = 5 ≥ 4 → success with a Numeric(0) draw.
    let session = board(4, |loc| {
        loc.attachments.push(CardInPlay::enter_play(
            CardCode::new(OBSCURING_FOG),
            FOG_ID,
            Owner::EncounterDeck,
        ));
    });

    // Investigate → commit nothing → success; Dr. Milan's reaction window is
    // open (suspended)...
    let (window, dr_milan) = investigate_to_dr_milans_window(session);
    // ...and Obscuring Fog's forced discard has ALREADY run (forced-before-
    // reaction): the attachment is gone and the card is in the encounter
    // discard, all before the reaction is resolved.
    assert!(
        window.state().locations[&HERE].attachments.is_empty(),
        "Obscuring Fog's forced discard must precede Dr. Milan's reaction window",
    );
    assert!(
        window
            .state()
            .encounter_discard
            .contains(&CardCode::new(OBSCURING_FOG)),
        "Obscuring Fog discarded to the encounter discard before the reaction",
    );

    // Fire Dr. Milan's reaction → back to the turn menu.
    let resumed = window.pick(dr_milan);
    resumed.assert_at_turn_menu(INV);
    assert_eq!(resumed.state().locations[&HERE].clues, 0, "clue discovered");
}
