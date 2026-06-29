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
//! so never exercise the cede). Uses the real `cards::REGISTRY` because the
//! synthetic Cover Up fixture has no `Trigger::Revelation` (it would push
//! nothing and never trip the bug); only the real 01007 self-places into the
//! threat area with 3 clues.

use game_core::engine::EngineOutcome;
use game_core::event::Event;
use game_core::state::{CardCode, Continuation, InvestigatorId, LocationId, Phase};
use game_core::test_support::{
    take_turn_action, test_investigator, test_location, GameStateBuilder,
};
use game_core::TurnAction;

const COVER_UP: &str = "01007";
const HOLY_ROSARY: &str = "01059"; // a real non-weakness asset, deck filler

#[ctor::ctor(unsafe)]
fn install_real_registry() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
}

#[test]
fn upkeep_draw_of_cover_up_does_not_panic_and_cedes_to_the_drive_loop() {
    let id = InvestigatorId(1);
    let loc = LocationId(101);

    // Solo investigator mid-Investigation, no enemies (so the Enemy phase
    // resolves quickly), with Cover Up on top of the deck so the Upkeep 4.4
    // draw pulls it. Deck top is drawn first.
    let mut inv = test_investigator(1);
    inv.current_location = Some(loc);
    inv.actions_remaining = 3;
    inv.deck = vec![
        CardCode::new(COVER_UP),
        CardCode::new(HOLY_ROSARY),
        CardCode::new(HOLY_ROSARY),
    ];

    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator(inv)
        .with_active_investigator(id)
        .with_turn_order([id])
        .with_location(test_location(101, "Study"))
        // Mid-Investigation invariant: EndTurn cascades through the phase anchor.
        .with_phase_anchor(Continuation::InvestigationPhase {
            resume: game_core::state::InvestigationResume::TurnBegins,
        })
        // Open-turn invariant: the InvestigatorTurn frame EndTurn pops.
        .with_investigator_turn(id)
        .build();

    // The round-ending EndTurn cascades Investigation → Enemy → Upkeep. At
    // step 4.4 the draw reveals Cover Up and pushes its Revelation. Before the
    // fix this panics at `set_upkeep_resume`'s `unreachable!`; after, 4.4 cedes,
    // the drive loop resolves the Revelation, and 4.5/4.6 run on re-exposure.
    let result = take_turn_action(state, &TurnAction::EndTurn);

    assert!(
        !matches!(result.outcome, EngineOutcome::Rejected { .. }),
        "EndTurn cascade must not be rejected; got {:?}",
        result.outcome,
    );

    let inv = &result.state.investigators[&id];
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
    assert!(
        result
            .events
            .iter()
            .any(|e| matches!(e, Event::CardRevealed { code, .. } if code.as_str() == COVER_UP)),
        "a CardRevealed event must fire for the drawn weakness",
    );

    // Upkeep completed: the cascade advanced past 4.6 into the next round's
    // Mythos (the single Upkeep → Mythos exit).
    assert_eq!(
        result.state.phase,
        Phase::Mythos,
        "the upkeep cascade must complete and advance to Mythos after the cede",
    );
}
