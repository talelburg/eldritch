//! #509 regression: drawing a persistent treachery weakness (Cover Up 01007)
//! during **Upkeep step 4.4** must not panic the engine.
//!
//! `upkeep_resume` runs steps 4.2→4.6 synchronously, relying on the
//! `UpkeepPhase` anchor staying on top of the continuation stack. The 4.4 draw
//! resolving a drawn-weakness Revelation `push_effect`s onto the live stack,
//! burying the anchor — so step 4.6's `set_upkeep_resume` used to find the
//! pushed effect instead of the anchor and hit `unreachable!()`. The fix makes
//! 4.4 cede to the drive loop (resume at `AfterDraw`) when it pushed.
//!
//! Drives a solo game via the public `EndTurn` apply path so the Investigation
//! → Enemy → Upkeep cascade actually engages the drive loop (unlike the
//! registry-free `upkeep_resume` unit tests, which call the helper directly and
//! so never exercise the cede). Uses the real `cards::REGISTRY` because only
//! the real Cover Up 01007 self-places into the threat area with 3 clues. The
//! synthetic fixture this file once contrasted against had no
//! `Trigger::Revelation` at all, so it would have pushed nothing and never
//! tripped the bug — which is the gap ADR 0016 was written from, and #878
//! deleted the fixture.
use cards::REGISTRY;
use game_core::assert_event;
use game_core::engine::enumerate::TurnAction;
use game_core::event::Event;
use game_core::state::{GameStateBuilder, InvestigatorId, LocationId, Phase};
use game_core::test_support::{self, seat};

const COVER_UP: &str = "01007";
const HOLY_ROSARY: &str = "01059"; // a real non-weakness asset, deck filler

const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(101);

#[ctor::ctor(unsafe)]
fn install_real_registry() {
    test_support::install_registry_with_test_cards(REGISTRY);
}

#[test]
fn upkeep_draw_of_cover_up_does_not_panic_and_cedes_to_the_drive_loop() {
    // Solo investigator mid-Investigation, no enemies (so the Enemy phase
    // resolves quickly), with Cover Up on top of the deck so the Upkeep 4.4
    // draw pulls it. Deck top is drawn first.
    //
    // The round-ending EndTurn cascades Investigation → Enemy → Upkeep. At
    // step 4.4 the draw reveals Cover Up and pushes its Revelation. Before the
    // fix this panics at `set_upkeep_resume`'s `unreachable!`; after, 4.4 cedes,
    // the drive loop resolves the Revelation, and 4.5/4.6 run on re-exposure.
    let session = GameStateBuilder::new()
        .with_seat(seat(1).at(HERE).deck([COVER_UP, HOLY_ROSARY, HOLY_ROSARY]))
        .with_location(test_support::test_location(HERE.0, "Study"))
        .open_turn(INV)
        .session()
        .take(&TurnAction::EndTurn);

    let inv = &session.state().investigators[&INV];
    assert!(
        !inv.hand.iter().any(|c| c.as_str() == COVER_UP),
        "Cover Up must not stay in hand — it is revealed on the Upkeep 4.4 draw",
    );
    let placed = inv
        .threat_area
        .iter()
        .find(|c| c.code.as_str() == COVER_UP)
        .expect("Cover Up should be in the threat area after the Upkeep draw resolves");
    assert_eq!(
        placed.clues, 3,
        "Cover Up enters the threat area with 3 clues"
    );
    assert_event!(
        session.events(),
        Event::CardRevealed { code, .. } if code.as_str() == COVER_UP
    );

    // Upkeep completed, so the EndTurn cascade was not rejected: it advanced
    // past 4.6 into the next round's Mythos (the single Upkeep → Mythos exit).
    assert_eq!(
        session.state().phase,
        Phase::Mythos,
        "the upkeep cascade must complete and advance to Mythos after the cede",
    );
}
