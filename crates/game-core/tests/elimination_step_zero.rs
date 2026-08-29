//! Rules Reference p.10 Elimination **step 0** (#638) — the scan's scoping,
//! against a mock `CardRegistry`.
//!
//! > 0. For the purpose of resolving weakness cards, the game has ended for the
//! >    eliminated investigator. Trigger any "when the game ends" abilities on
//! >    each weakness the eliminated investigator owns that is in play. Then,
//! >    remove those weaknesses from the game.
//!
//! The corpus consumer (Cover Up 01007) is covered end-to-end in
//! `crates/cards/tests/elimination_teardown.rs`. What *cannot* be covered there
//! is the negative half of the scoping: no in-scope non-weakness card carries a
//! `GameEnd` forced ability, so proving the weakness filter does real work needs
//! a synthetic card that is identical to the weakness in every respect except
//! `CardMetadata::weakness`. Hence a mock registry, and hence its own
//! integration-test binary (`OnceLock<CardRegistry>` is process-global).
//!
//! Lives at `crates/game-core/tests/` alongside `forced_triggers.rs` /
//! `native_effect.rs`, whose idiom it follows.

use card_dsl::dsl::{forced_on_event, native, Ability, EventPattern, EventTiming};
use game_core::card_data::{CardKind, CardMetadata};
use game_core::card_registry::{self, CardRegistry, NativeEffectFn};
use game_core::event::{Event, TraumaKind};
use game_core::state::{
    CardCode, CardInPlay, CardInstanceId, Continuation, GameState, InvestigatorId, LocationId,
    Status,
};
use game_core::test_support::{
    eliminate_by_damage, test_investigator, test_location, GameStateBuilder,
};
use game_core::{assert_event, assert_no_event, Cx, EngineOutcome, EvalContext};

/// A player-owned **weakness** in the threat area carrying a `GameEnd` forced
/// ability — the Cover Up 01007 shape, reduced to what step 0 keys off.
const WEAKNESS: &str = "test-weakness-game-end";

/// The same card in every respect **except** `weakness: false`: same zone, same
/// controller, same `GameEnd` forced ability, same native tag. The control that
/// makes the weakness filter falsifiable.
const NOT_A_WEAKNESS: &str = "test-asset-game-end";

/// The weakness shape the real corpus now has: a `GameEnd` forced declared in
/// the **`when` cell**, as Cover Up 01007 declares since #720. `EliminationGameEnd`
/// became a coordinator-owned bare milestone in the same change, so this cell is
/// walked here rather than rejected — and the fork that decides whether
/// elimination needs its frame at all has to scan every cell to find it.
const WEAKNESS_WHEN_CELL: &str = "test-weakness-game-end-when";

/// Native tag shared by all three mock cards: emit a mental-trauma event iff the
/// firing instance still holds clues. A direct port of Cover Up's `trauma`, and
/// deliberately clue-gated — reading the source instance is what proves the
/// ability fired *while the card was still in play*, i.e. before step 1 removed
/// it.
const TRAUMA_TAG: &str = "test:game-end-trauma";

fn metadata(code: &'static str, weakness: bool) -> CardMetadata {
    CardMetadata {
        code: code.to_owned(),
        name: code.to_owned(),
        traits: vec![],
        text: None,
        back_name: None,
        back_text: None,
        pack_code: "_test".to_owned(),
        weakness,
        kind: CardKind::Treachery {
            surge: false,
            peril: false,
            quantity: 1,
        },
    }
}

fn mock_metadata_for(code: &CardCode) -> Option<&'static CardMetadata> {
    static WEAK: std::sync::OnceLock<CardMetadata> = std::sync::OnceLock::new();
    static PLAIN: std::sync::OnceLock<CardMetadata> = std::sync::OnceLock::new();
    match code.as_str() {
        WEAKNESS => Some(WEAK.get_or_init(|| metadata(WEAKNESS, true))),
        WEAKNESS_WHEN_CELL => {
            static WHEN: std::sync::OnceLock<CardMetadata> = std::sync::OnceLock::new();
            Some(WHEN.get_or_init(|| metadata(WEAKNESS_WHEN_CELL, true)))
        }
        NOT_A_WEAKNESS => Some(PLAIN.get_or_init(|| metadata(NOT_A_WEAKNESS, false))),
        // `test_investigator`'s TEST_INV code, so `max_health()` resolves.
        _ => game_core::test_support::metadata_for_test_inv(code),
    }
}

fn mock_abilities_for(code: &CardCode) -> Option<Vec<Ability>> {
    let cell = match code.as_str() {
        WEAKNESS | NOT_A_WEAKNESS => EventTiming::After,
        WEAKNESS_WHEN_CELL => EventTiming::When,
        _ => return None,
    };
    Some(vec![forced_on_event(
        EventPattern::GameEnd,
        cell,
        native(TRAUMA_TAG),
    )])
}

/// "If there are any clues on it: you suffer 1 mental trauma."
fn trauma(cx: &mut Cx, ctx: &EvalContext) -> EngineOutcome {
    let Some(source) = ctx.source else {
        return EngineOutcome::Rejected {
            reason: "test trauma: no source instance".into(),
        };
    };
    let has_clues = cx
        .state
        .investigators
        .get(&ctx.controller)
        .is_some_and(|inv| {
            inv.controlled_card_instances()
                .any(|c| c.instance_id == source && c.clues > 0)
        });
    if has_clues {
        cx.events.push(Event::TraumaSuffered {
            investigator: ctx.controller,
            kind: TraumaKind::Mental,
            amount: 1,
        });
    }
    EngineOutcome::Done
}

fn mock_native_for(tag: &str) -> Option<NativeEffectFn> {
    (tag == TRAUMA_TAG).then_some(trauma as NativeEffectFn)
}

#[ctor::ctor(unsafe)]
fn install() {
    let _ = card_registry::install(CardRegistry {
        metadata_for: mock_metadata_for,
        abilities_for: mock_abilities_for,
        native_effect_for: mock_native_for,
        native_eligibility_for: |_| None,
        back_abilities_for: |_| None,
        native_condition_for: |_| None,
    });
}

/// One investigator at a location, holding `code` (with `clues` clues on it) in
/// the given zone. `TEST_INV` capacity is 8/8, so 8 damage is lethal.
fn board(code: &str, clues: u8, in_threat_area: bool) -> GameState {
    let mut inv = test_investigator(1);
    inv.current_location = Some(LocationId(10));
    let mut card = CardInPlay::enter_play(CardCode::new(code), CardInstanceId(1));
    card.clues = clues;
    if in_threat_area {
        inv.threat_area.push(card);
    } else {
        inv.cards_in_play.push(card);
    }
    GameStateBuilder::new()
        .with_investigator(inv)
        .with_location(test_location(10, "Study"))
        .with_turn_order([InvestigatorId(1)])
        .build()
}

#[test]
fn elimination_fires_an_owned_weaknesss_game_end_ability_before_removing_it() {
    // Step 0 in one assertion: the ability fires, and its clue-gated condition
    // saw the card still in play — so it ran *before* step 1's removal.
    let mut state = board(WEAKNESS, 3, true);
    let mut events = Vec::new();
    let outcome = eliminate_by_damage(&mut state, &mut events, InvestigatorId(1), 8);

    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(
        state.investigators[&InvestigatorId(1)].status,
        Status::Killed
    );
    assert_event!(events, Event::TraumaSuffered {
        investigator, kind: TraumaKind::Mental, amount: 1
    } if *investigator == InvestigatorId(1));

    // "Then, remove those weaknesses from the game."
    let inv = &state.investigators[&InvestigatorId(1)];
    assert!(
        inv.threat_area.is_empty(),
        "the weakness left the threat area"
    );
    assert!(
        inv.removed_from_game.iter().any(|c| c.as_str() == WEAKNESS),
        "removed from the game; removed = {:?}",
        inv.removed_from_game,
    );
}

#[test]
fn elimination_does_not_fire_a_non_weaknesss_game_end_ability() {
    // The acceptance criterion this file exists for. Identical card, identical
    // ability, identical zone — only `weakness: false` differs, and that alone
    // must keep it out of step 0's scan. The game has not ended for anyone else,
    // so a non-weakness card the eliminated investigator controls has no
    // game-end trigger point here.
    let mut state = board(NOT_A_WEAKNESS, 3, true);
    let mut events = Vec::new();
    let outcome = eliminate_by_damage(&mut state, &mut events, InvestigatorId(1), 8);

    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(
        state.investigators[&InvestigatorId(1)].status,
        Status::Killed
    );
    assert_no_event!(events, Event::TraumaSuffered { .. });
}

#[test]
fn elimination_fires_an_owned_weakness_held_outside_the_threat_area() {
    // Step 0 says "each weakness the eliminated investigator owns that is in
    // play", not "in the threat area" — a weakness among `cards_in_play` (a
    // weakness asset) fires too. Same scan, different zone.
    let mut state = board(WEAKNESS, 1, false);
    let mut events = Vec::new();
    let _ = eliminate_by_damage(&mut state, &mut events, InvestigatorId(1), 8);

    assert_event!(events, Event::TraumaSuffered { investigator, .. }
        if *investigator == InvestigatorId(1));
}

#[test]
fn a_weakness_whose_condition_fails_suffers_nothing() {
    // The ability fires either way; its own "if there are any clues" condition
    // is what decides. Keeps the positive tests honest about *why* they pass.
    let mut state = board(WEAKNESS, 0, true);
    let mut events = Vec::new();
    let _ = eliminate_by_damage(&mut state, &mut events, InvestigatorId(1), 8);

    assert_eq!(
        state.investigators[&InvestigatorId(1)].status,
        Status::Killed
    );
    assert_no_event!(events, Event::TraumaSuffered { .. });
}

#[test]
fn elimination_without_a_step_zero_ability_still_runs_its_steps() {
    // The other side of the fork in `apply_investigator_elimination`: with nothing
    // for step 0 to fire, elimination stays synchronous. Steps 1–6 must land
    // identically either way.
    let mut state = board("plain-card", 0, true);
    state
        .locations
        .get_mut(&LocationId(10))
        .expect("location")
        .clues = 0;
    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .expect("investigator")
        .clues = 2;

    let mut events = Vec::new();
    let outcome = eliminate_by_damage(&mut state, &mut events, InvestigatorId(1), 8);

    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(
        state.investigators[&InvestigatorId(1)].status,
        Status::Killed
    );
    assert_no_event!(events, Event::TraumaSuffered { .. });
    // Step 2 still deposited the possessed clues at the location.
    assert_eq!(state.locations[&LocationId(10)].clues, 2);
    // No `Elimination` frame is pushed at all on this path. (The stack is not
    // empty: the solo death latches `ScenarioEnding::NoResolution`, which parks the
    // `ScenarioEnd` frame at the bottom for the apply boundary to finalize.)
    assert!(
        !state
            .continuations
            .iter()
            .any(|c| matches!(c, Continuation::Elimination { .. })),
        "elimination stays synchronous with no step-0 ability; stack = {:?}",
        state.continuations,
    );
    // `plain-card` has no metadata → not a weakness → step 4's encounter discard.
    assert_eq!(state.encounter_discard.len(), 1);
}

#[test]
fn interactive_elimination_acknowledges_step_zero_before_running_the_steps() {
    // `interactive_acknowledge` is the server's default (`session.rs`), so this
    // is the shape real play takes: step 0's lone forced hit pushes an
    // `AcknowledgeForced`, elimination *suspends* mid-sequence, and steps 1–6
    // run only once the player has acknowledged. The resumability the
    // `Elimination` frame exists for (#638).
    let mut state = board(WEAKNESS, 3, true);
    state.interactive_acknowledge = true;
    let mut events = Vec::new();
    let paused = eliminate_by_damage(&mut state, &mut events, InvestigatorId(1), 8);

    assert!(
        matches!(paused, EngineOutcome::AwaitingInput { .. }),
        "step 0's forced acknowledge must surface; got {paused:?}",
    );
    assert_no_event!(events, Event::TraumaSuffered { .. });
    // Mid-sequence: already eliminated, but step 1 has not run — the weakness is
    // still in play for its own ability to read.
    let inv = &state.investigators[&InvestigatorId(1)];
    assert_eq!(inv.status, Status::Killed, "status flips before step 0");
    assert!(
        inv.threat_area.iter().any(|c| c.code.as_str() == WEAKNESS),
        "the weakness is still in play while its ability is pending",
    );

    let done = game_core::apply(
        state,
        game_core::Action::Player(game_core::PlayerAction::ResolveInput {
            response: game_core::action::InputResponse::PickSingle(game_core::engine::OptionId(0)),
        }),
    );

    assert_event!(done.events, Event::TraumaSuffered {
        investigator, kind: TraumaKind::Mental, amount: 1
    } if *investigator == InvestigatorId(1));
    let inv = &done.state.investigators[&InvestigatorId(1)];
    assert!(inv.threat_area.is_empty(), "steps 1–6 ran on resume");
    assert!(inv.removed_from_game.iter().any(|c| c.as_str() == WEAKNESS));
    assert!(
        !done
            .state
            .continuations
            .iter()
            .any(|c| matches!(c, Continuation::Elimination { .. })),
        "no stranded elimination frame; stack = {:?}",
        done.state.continuations,
    );
}

/// Step 0's condition is a **bare milestone**, so its `when` cell is walked
/// (#720). The unit-level counterpart of `GameEnd`'s cell walk in
/// `timing_resolve_step.rs`, on the point that scans the same
/// `EventPattern::GameEnd` declaration.
///
/// Two regressions fail this test, and neither announces itself the same way.
/// Put `EliminationGameEnd` back on the caller-owned arm and the elimination
/// **rejects**, because a caller-owned condition's `when` cell is not walked.
/// Leave `has_weakness_game_end_ability` scanning one hardcoded cell and the
/// ability is **silently dropped** instead: the fork takes its inline path and
/// step 1 removes the weakness unfired, with no reject to show for it. The
/// clue-gated trauma catches both, since it can only fire while the card is
/// still in play.
#[test]
fn elimination_fires_a_when_cell_weakness_ability() {
    let mut state = board(WEAKNESS_WHEN_CELL, 3, true);
    let mut events = Vec::new();
    let outcome = eliminate_by_damage(&mut state, &mut events, InvestigatorId(1), 8);

    assert_eq!(
        outcome,
        EngineOutcome::Done,
        "the `when` cell is walked rather than rejecting the declaration"
    );
    assert_event!(events, Event::TraumaSuffered {
        investigator, kind: TraumaKind::Mental, amount: 1
    } if *investigator == InvestigatorId(1));

    // Step 0's tail still runs: firing in an earlier cell does not skip it.
    let inv = &state.investigators[&InvestigatorId(1)];
    assert!(
        inv.threat_area.is_empty(),
        "the weakness left the threat area"
    );
    assert!(
        inv.removed_from_game
            .iter()
            .any(|c| c.as_str() == WEAKNESS_WHEN_CELL),
        "removed from the game; removed = {:?}",
        inv.removed_from_game,
    );
}
