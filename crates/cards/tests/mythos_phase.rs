//! Integration tests for #69 Mythos phase content, against real Core Set cards.
//!
//! Drives full apply cycles through `seat_and_open` → `Mulligan` →
//! `EndTurn` → `DrawEncounterCard`, verifying the per-card 5-step
//! sub-sequence and the post-1.4 Fast window end-to-end.
//!
//! Lives in `crates/cards/tests/` (ADR 0016) so it can install the real
//! `cards::REGISTRY`: `game-core` cannot reach the corpus by crate direction,
//! and each `tests/*.rs` is its own process, so this install does not collide
//! with the registries other integration binaries claim.
//!
//! The cards, all Core Set, all verified against
//! `data/arkhamdb-snapshot/pack/core/` and their rulings files:
//!
//! - **Ancient Evils 01166** — *"**Revelation** - Place 1 doom on the current
//!   agenda. This effect can cause the current agenda to advance."* The plain
//!   treachery of every draw-ordering test: its Revelation resolves inline (no
//!   skill test, no choice) and observably moves `agenda_doom`, so a draw that
//!   silently skipped the Revelation would not pass. No rulings
//!   (`data/arkhamdb-faq/no-rulings.txt`).
//! - **Flesh-Eater 01118** — *"**Spawn** - Attic."* The spawn tests' enemy;
//!   the board therefore seats investigators at the **Attic 01113** so the
//!   spawn engages them. Its ruling — *"If an enemy should spawn at a location
//!   that is not currently in play … place that enemy card into the encounter
//!   discard pile without any further effects"*
//!   (<https://arkhamdb.com/card/01118>) — is the off-board case, pinned
//!   separately by `enemy_spawn_no_location.rs`; here the Attic is in play.
//! - **Beat Cop 01018** — *"You get +1 \[combat\]."* / *"\[fast\] Discard Beat
//!   Cop: Deal 1 damage to an enemy at your location."* The `MythosAfterDraws`
//!   Fast-window pair runs on its 0-action activated ability, which
//!   `glossary/Ability.md` puts in *any* player window: *"A \[free\] triggered
//!   ability may be triggered as a player ability during any player window."*
//!   Its ruling — *"You cannot use Beat Cop's ability after you assign lethal
//!   damage/horror to him"* (<https://arkhamdb.com/card/01018>) — scopes a
//!   damage-assignment window these tests never enter.
//!
//! **Why not a Fast card in hand for the window pair.** No printed event
//! reaches an out-of-turn Fast window: a *"Play when/after …"* clause makes it
//! a reaction (RR p.11), and *"Play only during your turn"* is excluded by
//! `check_play_card`'s turn gate. A Fast **asset** is scoped to
//! `owner_is_active && permissive_window`, and Mythos has no active
//! investigator — which is why `issue_476_fast_window.rs` (Magnifying Glass
//! 01030) proves the property at `InvestigatorTurnBegins` and cannot prove it
//! here. The 0-action activated ability is the other half of
//! `enumerate_fast_plays`, and it is not turn-gated.
//!
//! **Agenda deck, no act deck.** One agenda (Rise of the Ghouls 01106, doom 7)
//! so Mythos step 1.2's doom placement and Ancient Evils' Revelation both land
//! somewhere observable; the threshold is read from the corpus so it cannot
//! drift. Seven is far above the 3 doom the longest test here accumulates, so
//! no agenda ever advances — the advance path is `issue_482_advance.rs`'s. No
//! act deck: nothing here discovers a clue.
//!
//! The two surge tests and the choice-Revelation probe that also lived in this
//! group need card shapes the corpus cannot supply; they are in
//! `mythos_phase_probes.rs` with their probes inline.

use game_core::action::RosterEntry;
use game_core::card_data::{CardKind, CardType};
use game_core::engine::{apply, EngineOutcome};
use game_core::event::Event;
use game_core::seat_and_open;
use game_core::state::{
    Agenda, CardCode, CardInPlay, ChaosBag, ChaosToken, Continuation, FastWindowKind, GameState,
    InvestigatorId, LocationId, Phase, PhaseStep,
};
use game_core::test_support::{take_turn_action, test_location, GameStateBuilder};
use game_core::{assert_event, Action, InputKind, InputResponse, PlayerAction, TurnAction};

/// Ancient Evils — *"**Revelation** - Place 1 doom on the current agenda. This
/// effect can cause the current agenda to advance."*
const ANCIENT_EVILS: &str = "01166";
/// Flesh-Eater — *"**Spawn** - Attic."*
const FLESH_EATER: &str = "01118";
/// The Attic — Flesh-Eater's spawn location, and where the roster is seated.
const ATTIC: &str = "01113";
/// Beat Cop — *"\[fast\] Discard Beat Cop: Deal 1 damage to an enemy at your
/// location."*
const BEAT_COP: &str = "01018";
/// Rise of the Ghouls — the board's sole agenda.
const RISE_OF_THE_GHOULS: &str = "01106";
/// Roland Banks / Daisy Walker — two distinct seated investigators.
const ROLAND: &str = "01001";
const DAISY: &str = "01002";

/// The Attic's `LocationId` on this board.
const ATTIC_ID: LocationId = LocationId(10);

#[ctor::ctor(unsafe)]
fn install_real_registry() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
}

/// Rise of the Ghouls' printed doom threshold, read from the corpus rather
/// than transcribed, so a snapshot refresh cannot silently invalidate the
/// "no agenda ever advances" premise in the module docs.
fn agenda_doom_threshold(code: &str) -> u8 {
    match cards::by_code(code).expect("agenda in corpus").kind {
        CardKind::Agenda { doom_threshold } => doom_threshold,
        ref kind => panic!("{code} is not an Agenda ({kind:?})"),
    }
}

/// The board every test starts from: the Attic in play as the starting
/// location, a +0 chaos bag, and a one-card agenda deck. No investigators —
/// callers supply the roster to [`seat_and_open`].
fn board() -> GameState {
    let mut attic = test_location(ATTIC_ID.0, "Attic");
    attic.code = CardCode::new(ATTIC);

    let mut state = GameStateBuilder::new()
        .with_location(attic)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .build();
    state.starting_location = Some(ATTIC_ID);
    state.agenda_deck = vec![Agenda {
        code: CardCode::new(RISE_OF_THE_GHOULS),
        doom_threshold: agenda_doom_threshold(RISE_OF_THE_GHOULS),
    }];
    state
}

/// Seed the encounter deck in draw order (top of deck = index 0), replacing
/// whatever is there. Where the order matters, call this *after*
/// `seat_and_open` — its setup shuffles the deck.
fn with_encounter_deck(state: &mut GameState, codes: &[&str]) {
    state.encounter_deck = codes.iter().map(|c| CardCode::new(*c)).collect();
}

/// Close one investigator's mulligan prompt, keeping the whole opening hand.
fn keep_hand(state: GameState) -> GameState {
    apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickMultiple { selected: vec![] },
        }),
    )
    .state
}

/// Build the standard single-investigator sequence up to the point
/// where `DrawEncounterCard` is the next expected action.
///
/// Returns the state after `EndTurn` has ticked through all phases
/// and landed in Mythos with the encounter-draw cursor on inv1.
fn setup_at_mythos_draw(state: GameState) -> GameState {
    let roster = vec![RosterEntry {
        investigator: CardCode::new(ROLAND),
        deck: vec![],
    }];
    // seat_and_open opens the mulligan prompt; close it (keep hand).
    let state = keep_hand(seat_and_open(state, &roster).state);
    // Sole investigator ends their turn → auto-advance through
    // Investigation → Enemy → Upkeep → Mythos (round 2).
    // Pauses with the encounter-draw cursor on inv1.
    take_turn_action(state, &TurnAction::EndTurn).state
}

/// The two-investigator equivalent: Roland and Daisy, both seated at the
/// Attic by `seat_and_open`, both mulliganed, both turns ended — which ticks
/// through the phases into Mythos with the cursor on inv1.
fn setup_two_investigators_at_mythos_draw(state: GameState) -> GameState {
    let roster = vec![
        RosterEntry {
            investigator: CardCode::new(ROLAND),
            deck: vec![],
        },
        RosterEntry {
            investigator: CardCode::new(DAISY),
            deck: vec![],
        },
    ];
    let mut state = keep_hand(keep_hand(seat_and_open(state, &roster).state));
    // inv1 ends turn → rotates to inv2.
    state = take_turn_action(state, &TurnAction::EndTurn).state;
    // inv2 is the last in turn_order → ticks through phases into Mythos.
    take_turn_action(state, &TurnAction::EndTurn).state
}

/// The `Confirm` that answers the step-1.4 encounter-draw prompt.
fn draw_encounter_card(state: GameState) -> game_core::ApplyResult {
    apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::Confirm,
        }),
    )
}

// ------------------------------------------------------------------
// Single-treachery happy path
// ------------------------------------------------------------------

#[test]
fn mythos_phase_resolves_single_treachery() {
    let mut base = board();
    with_encounter_deck(&mut base, &[ANCIENT_EVILS]);

    let state = setup_at_mythos_draw(base);
    assert_eq!(state.phase, Phase::Mythos, "must be in Mythos before draw");
    assert_eq!(state.current_encounter_drawer(), Some(InvestigatorId(1)));

    let result = draw_encounter_card(state);

    assert!(matches!(
        result.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
    // Mythos → Investigation transition completes inline (MythosAfterDraws
    // auto-closes because no fast-play-eligible cards are in any hand).
    assert_eq!(result.state.phase, Phase::Investigation);
    assert_eq!(result.state.round, 2);
    assert!(
        result.state.encounter_deck.is_empty(),
        "deck must be empty after draw"
    );
    assert!(
        result
            .state
            .encounter_discard
            .contains(&CardCode::new(ANCIENT_EVILS)),
        "treachery must be in discard after Revelation resolves",
    );
    assert_eq!(
        result.state.current_encounter_drawer(),
        None,
        "cursor must be cleared once all investigators have drawn",
    );
    assert_eq!(
        result.state.active_investigator,
        Some(InvestigatorId(1)),
        "investigation_phase rotates to the lead investigator",
    );

    // CardRevealed fires for Ancient Evils.
    assert_event!(
        result.events,
        Event::CardRevealed { investigator, code, card_type }
            if *investigator == InvestigatorId(1)
                && *code == CardCode::new(ANCIENT_EVILS)
                && *card_type == CardType::Treachery
    );
    // …and its Revelation actually ran: 1 doom from Mythos step 1.2, 1 more
    // from the card. (The synthetic predecessor gained a resource here; the
    // printed clause is doom.)
    assert_eq!(
        result.state.agenda_doom, 2,
        "step 1.2's doom plus Ancient Evils' Revelation",
    );
}

// ------------------------------------------------------------------
// Spawn enemy via Mythos
// ------------------------------------------------------------------

#[test]
fn mythos_phase_resolves_single_spawn_enemy() {
    // seat_and_open (called inside setup_at_mythos_draw) places the investigator
    // at starting_location = the Attic, which is where Flesh-Eater spawns, so
    // the spawned enemy engages them.
    let mut base = board();
    with_encounter_deck(&mut base, &[FLESH_EATER]);

    let state = setup_at_mythos_draw(base);
    assert_eq!(state.phase, Phase::Mythos);

    let result = draw_encounter_card(state);

    assert!(matches!(
        result.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
    assert_eq!(
        result.state.enemies.len(),
        1,
        "one enemy must be in play after spawn",
    );
    let enemy = result.state.enemies.values().next().unwrap();
    assert_eq!(enemy.current_location, Some(ATTIC_ID));
    assert_eq!(enemy.engaged_with, Some(InvestigatorId(1)));
    assert!(
        !result
            .state
            .encounter_discard
            .contains(&CardCode::new(FLESH_EATER)),
        "spawned enemy must not be in encounter_discard",
    );
    assert_eq!(result.state.phase, Phase::Investigation);

    // EnemySpawned event fired.
    assert_event!(
        result.events,
        Event::EnemySpawned { code, location, engaged_with, .. }
            if *code == CardCode::new(FLESH_EATER)
                && *location == ATTIC_ID
                && *engaged_with == Some(InvestigatorId(1))
    );
}

#[test]
#[allow(clippy::too_many_lines)] // end-to-end multi-investigator spawn-suspend walkthrough
fn mythos_phase_multi_investigator_spawn_suspends_then_resumes_chain() {
    // Two investigators co-located at the Attic: the drawn Flesh-Eater ties
    // under Prey::Default, so the draw suspends for the lead's PickSingle
    // (#128, option A). Resolving the pick engages the chosen investigator and
    // resumes inv1's Mythos draw chain — which, the enemy being non-surge,
    // advances the cursor to inv2 and stays in Mythos.
    // Both investigators are seated at the Attic (starting_location) by
    // seat_and_open; no pre-seating needed.
    let mut state = setup_two_investigators_at_mythos_draw(board());
    assert_eq!(state.phase, Phase::Mythos);
    assert_eq!(state.current_encounter_drawer(), Some(InvestigatorId(1)));

    // Seed the controlled draw order *after* seat_and_open's shuffle:
    // inv1 draws the enemy; inv2 draws a plain treachery afterward.
    with_encounter_deck(&mut state, &[FLESH_EATER, ANCIENT_EVILS]);

    // Draw → spawn tie → suspend.
    let suspended = draw_encounter_card(state);
    assert!(
        matches!(suspended.outcome, EngineOutcome::AwaitingInput { .. }),
        "spawn tie must suspend, got {:?}",
        suspended.outcome,
    );
    assert!(matches!(
        suspended.state.continuations.last(),
        Some(Continuation::SpawnEngage(_))
    ));
    let enemy = suspended
        .state
        .enemies
        .values()
        .next()
        .expect("enemy placed");
    assert_eq!(enemy.engaged_with, None, "engagement deferred");
    // The cursor is unchanged — still mid-chain for inv1.
    assert_eq!(
        suspended.state.current_encounter_drawer(),
        Some(InvestigatorId(1))
    );

    // Lead picks inv2 (by its offered option id) → engage + resume the chain.
    // The enemy is non-surge, so no further card draws; the chain advances to inv2.
    let pick = {
        let EngineOutcome::AwaitingInput { request, .. } = &suspended.outcome else {
            unreachable!("asserted AwaitingInput above");
        };
        request
            .options
            .iter()
            .find(|o| o.label == format!("{:?}", InvestigatorId(2)))
            .expect("InvestigatorId(2) among offered options")
            .id
    };
    let resumed = apply(
        suspended.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(pick),
        }),
    );
    // Picking engages inv2 and re-enters inv1's chain, which completes; the
    // loop then drains inv1 and re-prompts inv2 (AwaitingInput).
    assert!(matches!(
        resumed.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
    assert!(!matches!(
        resumed.state.continuations.last(),
        Some(Continuation::SpawnEngage(_))
    ));
    let enemy = resumed
        .state
        .enemies
        .values()
        .next()
        .expect("enemy still placed");
    assert_eq!(
        enemy.engaged_with,
        Some(InvestigatorId(2)),
        "the lead's pick is now engaged",
    );
    // Chain resumed and advanced to inv2; still in Mythos.
    assert_eq!(resumed.state.phase, Phase::Mythos);
    assert_eq!(
        resumed.state.current_encounter_drawer(),
        Some(InvestigatorId(2))
    );
    assert_event!(
        resumed.events,
        Event::EnemyEngaged { investigator, .. }
            if *investigator == InvestigatorId(2)
    );
}

// ------------------------------------------------------------------
// Multi-investigator player order
// ------------------------------------------------------------------

#[test]
fn mythos_phase_multi_investigator_player_order() {
    let inv1 = InvestigatorId(1);
    let inv2 = InvestigatorId(2);

    // Two treacheries — one per investigator.
    let mut base = board();
    with_encounter_deck(&mut base, &[ANCIENT_EVILS, ANCIENT_EVILS]);

    let state = setup_two_investigators_at_mythos_draw(base);

    assert_eq!(state.phase, Phase::Mythos);
    assert_eq!(
        state.current_encounter_drawer(),
        Some(inv1),
        "inv1 draws first"
    );

    // inv1 draws their card → the loop re-prompts inv2 (AwaitingInput).
    let result1 = draw_encounter_card(state);
    assert!(matches!(
        result1.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
    // Still in Mythos; inv2 must draw next.
    assert_eq!(result1.state.phase, Phase::Mythos);
    assert_eq!(result1.state.current_encounter_drawer(), Some(inv2));

    // inv2 draws their card → completes the phase.
    let result2 = draw_encounter_card(result1.state);
    assert!(matches!(
        result2.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
    assert_eq!(result2.state.current_encounter_drawer(), None);
    assert_eq!(result2.state.phase, Phase::Investigation);
    assert!(result2.state.encounter_deck.is_empty());
    assert_eq!(result2.state.encounter_discard.len(), 2);
}

// ------------------------------------------------------------------
// Full round chain (round counter bump)
// ------------------------------------------------------------------

#[test]
fn mythos_phase_full_round_chain() {
    let mut base = board();
    with_encounter_deck(&mut base, &[ANCIENT_EVILS]);

    let state = setup_at_mythos_draw(base);
    // Confirm the round bumped on Mythos entry.
    assert_eq!(state.round, 2);
    assert_eq!(state.phase, Phase::Mythos);

    let result = draw_encounter_card(state);

    assert!(matches!(
        result.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
    assert_eq!(
        result.state.round, 2,
        "round stays 2 — it bumps on Mythos *entry*"
    );
    assert_eq!(result.state.phase, Phase::Investigation);
    assert_eq!(result.state.active_investigator, Some(InvestigatorId(1)));
}

// ------------------------------------------------------------------
// Empty-deck rejection
// ------------------------------------------------------------------

#[test]
fn mythos_draw_rejects_when_initial_deck_and_discard_both_empty() {
    // Deck and discard both empty from `board()`; nothing seeds either.
    let state = setup_at_mythos_draw(board());
    assert_eq!(state.phase, Phase::Mythos);
    assert_eq!(state.current_encounter_drawer(), Some(InvestigatorId(1)));

    let result = draw_encounter_card(state);

    match result.outcome {
        EngineOutcome::Rejected { reason } => {
            assert!(
                reason.contains("encounter deck and discard both empty"),
                "unexpected reject reason: {reason:?}",
            );
        }
        other => panic!("expected Rejected, got {other:?}"),
    }

    // Validate-first: state must be unchanged — still Mythos, cursor preserved.
    assert_eq!(
        result.state.phase,
        Phase::Mythos,
        "phase must not change on Rejected",
    );
    assert_eq!(
        result.state.current_encounter_drawer(),
        Some(InvestigatorId(1)),
        "cursor must be preserved on initial Rejected (validate-first)",
    );
    assert!(
        result.events.is_empty(),
        "no events should fire on empty-deck reject; got {:?}",
        result.events,
    );
}

// ------------------------------------------------------------------
// Fast-window push-then-scan fix (defect A + B from #476's pre-PR review)
// ------------------------------------------------------------------

/// The board for the `MythosAfterDraws` pair: the standard solo setup, with
/// Beat Cop put into play afterwards (so it does not interact with
/// `seat_and_open`'s opening-hand draw) and Flesh-Eater on top of the
/// encounter deck. The draw spawns the enemy at the investigator's own
/// location, which is what makes Beat Cop's `[fast]` ability pass its pre-cost
/// target check and hold the window open.
fn at_mythos_draw_with_beat_cop_in_play() -> GameState {
    let mut state = setup_at_mythos_draw(board());
    // Minted rather than hand-picked: `seat_and_open` already gave the seated
    // investigator card instance 0, and a second card sharing that id resolves
    // to the investigator instead — the ability lookup then finds no printed
    // index 1 and the window silently auto-skips.
    let instance = state.card_instance_ids.mint();
    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .expect("inv1 must be present")
        .cards_in_play
        .push(CardInPlay::enter_play(CardCode::new(BEAT_COP), instance));
    with_encounter_deck(&mut state, &[FLESH_EATER]);
    state
}

/// Regression test for the push-then-scan ordering fix in
/// `open_fast_window`. Before the fix, `any_fast_play_eligible` was
/// called BEFORE the `MythosAfterDraws` window was pushed onto
/// `state.open_windows`, so `check_play_card`'s `permissive_window`
/// check saw an empty stack and evaluated every Fast card as
/// ineligible. The window would auto-skip even when the player had an
/// eligible Fast play — silently denying plays in the post-1.4 window.
///
/// The eligible play here arrives from the Mythos draw itself: Flesh-Eater
/// spawns at the investigator's location, which gives Beat Cop's `[fast]`
/// *"Deal 1 damage to an enemy at your location"* a legal target. The test
/// asserts that the window STAYS OPEN (not auto-skipped) and — post-#476 —
/// surfaces the eligible play as a **skippable** `PickSingle` prompt rather
/// than idling silently as `Done`.
#[test]
fn mythos_after_draws_window_stays_open_when_a_fast_play_is_eligible() {
    let state = at_mythos_draw_with_beat_cop_in_play();
    assert_eq!(state.phase, Phase::Mythos);
    assert_eq!(state.current_encounter_drawer(), Some(InvestigatorId(1)));

    let result = draw_encounter_card(state);

    // #476: the MythosAfterDraws window now surfaces the eligible Fast play as
    // a skippable choice instead of idling as Done — the player can use it or pass.
    let EngineOutcome::AwaitingInput { request, .. } = &result.outcome else {
        panic!(
            "MythosAfterDraws must prompt the eligible Fast play, got {:?}",
            result.outcome
        );
    };
    assert_eq!(request.kind, InputKind::PickSingle, "{request:?}");
    assert!(
        request.skippable,
        "the fast window is skippable (pass): {request:?}"
    );
    assert!(
        !request.options.is_empty(),
        "the prompt lists the eligible Fast play: {request:?}"
    );

    // The window must remain on the stack — it was NOT auto-skipped.
    assert!(
        !result.state.open_windows().is_empty(),
        "MythosAfterDraws window must stay on stack when a Fast play is eligible; \
         pre-fix this would have been empty (window auto-skipped)"
    );
    assert!(
        matches!(
            result.state.open_windows().last(),
            Some(Continuation::FastWindow {
                kind: FastWindowKind::Phase(PhaseStep::MythosAfterDraws),
                ..
            })
        ),
        "top open window must be MythosAfterDraws; got {:?}",
        result.state.open_windows().last()
    );

    // Phase must still be Mythos — the window continuation (mythos_phase_end)
    // has NOT fired yet. (The window staying open + the phase not advancing is
    // the observable signal that it was not auto-skipped.)
    assert_eq!(
        result.state.phase,
        Phase::Mythos,
        "phase must still be Mythos while MythosAfterDraws window is open"
    );
}

/// Continuation of the push-then-scan regression: after `DrawEncounterCard`
/// leaves the `MythosAfterDraws` window open (because a Fast play is
/// eligible), `ResolveInput::Skip` must close the window, run
/// `mythos_phase_end`, and transition to Investigation.
///
/// `resolve_input`'s Skip arm closes the top frame: a pure-Fast
/// `MythosAfterDraws` gate (empty `pending_triggers`) on top is closed via
/// `close_reaction_window`. (Historically this routed through an empty-skipping
/// `top_reaction_window_index`, which failed to find the pure-Fast window and
/// left it stuck — Slice C-plumbing replaced that with top-frame dispatch.)
#[test]
fn mythos_after_draws_window_closed_by_skip_and_transitions_to_investigation() {
    // Advance through the draw to land in the open-window state (post-#476 a
    // skippable fast-window prompt, not Done-idle).
    let draw_result = draw_encounter_card(at_mythos_draw_with_beat_cop_in_play());
    assert!(
        matches!(draw_result.outcome, EngineOutcome::AwaitingInput { .. }),
        "the draw lands on the skippable fast-window prompt, got {:?}",
        draw_result.outcome
    );
    assert!(
        !draw_result.state.open_windows().is_empty(),
        "window must be open before Skip test"
    );

    // Now close the window with Skip (player decides not to use the Fast ability).
    let skip_result = apply(
        draw_result.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::Skip,
        }),
    );

    // MythosAfterDraws is closed; investigation_phase then opens
    // InvestigationBegins. Because Beat Cop's ability is still eligible (the
    // spawned enemy is still there), that window also surfaces a skippable
    // prompt (#476) rather than idling.
    let EngineOutcome::AwaitingInput { request, .. } = &skip_result.outcome else {
        panic!(
            "Skip closes MythosAfterDraws and InvestigationBegins prompts the Fast play, got {:?}",
            skip_result.outcome
        );
    };
    assert!(
        request.skippable,
        "the InvestigationBegins fast window is skippable: {request:?}"
    );

    // MythosAfterDraws is gone; InvestigationBegins is the one open window.
    assert_eq!(
        skip_result.state.open_windows().len(),
        1,
        "InvestigationBegins window must be open (the Fast ability is eligible); \
         MythosAfterDraws must be gone"
    );
    assert!(
        matches!(
            skip_result.state.open_windows().last(),
            Some(Continuation::FastWindow {
                kind: FastWindowKind::Phase(PhaseStep::InvestigationBegins),
                ..
            })
        ),
        "top window must be InvestigationBegins; got {:?}",
        skip_result.state.open_windows().last()
    );

    // mythos_phase_end ran: phase transitioned to Investigation.
    // active_investigator is None until InvestigationBegins closes
    // (its continuation begin_investigator_turn rotates to the lead).
    assert_eq!(
        skip_result.state.phase,
        Phase::Investigation,
        "phase must be Investigation after MythosAfterDraws window closes"
    );
    assert_eq!(
        skip_result.state.active_investigator, None,
        "active investigator not yet set — InvestigationBegins window is still open"
    );
    assert_eq!(
        skip_result.state.round, 2,
        "round stays 2 — it bumped on Mythos entry"
    );
    // MythosAfterDraws closed and InvestigationBegins opened — both observable
    // via the open-window stack + phase transition asserted above.
}
