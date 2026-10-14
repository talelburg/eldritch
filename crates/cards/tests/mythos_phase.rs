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
//!   damage/horror to him …"* (<https://arkhamdb.com/card/01018>) — scopes a
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
//! "The lead" below always means the first Active investigator in
//! `turn_order`, the proxy the engine
//! actually binds — not the chosen lead investigator of
//! `glossary/Lead_Investigator.md`, which we do not model (`GLOSSARY.md`, *Lead
//! investigator*). The two coincide in the solo tests and are not guaranteed to
//! in the two-investigator ones; nothing here turns on the difference.
//!
//! The two surge tests and the choice-Revelation probe that also lived in this
//! group need card shapes the corpus cannot supply; they are in
//! `mythos_phase_probes.rs` with their probes inline.

use card_dsl::card_data::{CardKind, CardType};
use cards::TEST_REGISTRIES;
use game_core::action::RosterEntry;
use game_core::engine::enumerate::TurnAction;
use game_core::engine::{self, InputKind};
use game_core::event::Event;
use game_core::state::{
    Agenda, CardCode, CardInPlay, ChaosBag, ChaosToken, GameState, GameStateBuilder,
    InvestigatorId, LocationId, Owner, Phase,
};
use game_core::test_support::{SeatLookup as _, TestSession};
use game_core::{assert_event, test_support};

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
/// Roland Banks — seated through the roster, which reads his printed metadata.
const ROLAND: &str = "01001";

const INV1: InvestigatorId = InvestigatorId(1);
const INV2: InvestigatorId = InvestigatorId(2);

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
/// callers supply the roster to [`seat_and_open`](engine::seat_and_open).
/// Returns the minted [`LocationId`] alongside, since `add_location` mints it.
///
/// The Attic is built **through the engine from its own corpus metadata**
/// (`GameState::add_location`, the same call The Gathering's `setup` makes),
/// not hand-stamped onto a `test_location`: shroud, printed clues and victory
/// come off card 01113 rather than from a fixture wearing its code, which is
/// the impersonation ADR 0016 forbids. Its *"**Forced** - After you enter the
/// Attic: Take 1 horror."* is live in the installed registry and simply never
/// fires — only the Move action emits `EnteredLocation`, and `seat_and_open`
/// *places* the roster rather than moving it.
fn board() -> (GameState, LocationId) {
    let mut state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .build();
    let attic = state.add_location(cards::by_code(ATTIC).expect("Attic 01113 in corpus"));
    state.starting_location = Some(attic);
    state.agenda_deck = vec![Agenda {
        code: CardCode::new(RISE_OF_THE_GHOULS),
        doom_threshold: agenda_doom_threshold(RISE_OF_THE_GHOULS),
    }];
    (state, attic)
}

/// Seed the encounter deck in draw order (top of deck = index 0), replacing
/// whatever is there. Where the order matters, call this *after*
/// `seat_and_open` — its setup shuffles the deck.
fn with_encounter_deck(state: &mut GameState, codes: &[&str]) {
    state.encounter_deck = codes.iter().map(|c| CardCode::new(*c)).collect();
}

/// Seat `roster` on `state` through `seat_and_open`, and keep every opening
/// hand at the mulligan, which leaves the session at the lead's turn menu.
fn seat_keeping_hands(state: GameState, investigators: &[&str]) -> TestSession {
    let roster: Vec<RosterEntry> = investigators
        .iter()
        .map(|code| RosterEntry {
            investigator: CardCode::new(*code),
            deck: vec![],
        })
        .collect();
    let session = TestSession::new_with_registries(
        engine::seat_and_open_with_registries(state, &roster, TEST_REGISTRIES).state,
        TEST_REGISTRIES,
    );
    investigators
        .iter()
        .fold(session, |session, _| session.choose_from_hand(&[]))
}

/// Build the standard single-investigator sequence up to the point
/// where the step-1.4 encounter draw is the next prompt.
///
/// The session rests after `EndTurn` has ticked through all phases
/// and landed in Mythos with the encounter-draw cursor on inv1.
fn setup_at_mythos_draw(state: GameState) -> TestSession {
    // Sole investigator ends their turn → auto-advance through
    // Investigation → Enemy → Upkeep → Mythos (round 2).
    // Pauses with the encounter-draw cursor on inv1.
    seat_keeping_hands(state, &[ROLAND]).take(&TurnAction::EndTurn)
}

/// The two-investigator equivalent: Roland and the test investigator, both seated at the
/// Attic by `seat_and_open`, both mulliganed, both turns ended — which ticks
/// through the phases into Mythos with the cursor on inv1.
fn setup_two_investigators_at_mythos_draw(state: GameState) -> TestSession {
    seat_keeping_hands(state, &[ROLAND, test_support::TEST_INV])
        // inv1 ends turn → rotates to inv2.
        .take(&TurnAction::EndTurn)
        // inv2 is the last in turn_order → ticks through phases into Mythos.
        .take(&TurnAction::EndTurn)
}

// ------------------------------------------------------------------
// Single-treachery happy path
// ------------------------------------------------------------------

#[test]
fn mythos_phase_resolves_single_treachery() {
    let (mut base, _) = board();
    with_encounter_deck(&mut base, &[ANCIENT_EVILS]);

    let drawing = setup_at_mythos_draw(base);
    drawing.assert_at_encounter_draw();
    assert_eq!(
        drawing.state().phase,
        Phase::Mythos,
        "must be in Mythos before draw"
    );
    assert_eq!(drawing.state().current_encounter_drawer(), Some(INV1));

    let session = drawing.confirm();

    // Mythos → Investigation transition completes inline (MythosAfterDraws
    // auto-closes because no fast-play-eligible cards are in any hand), and
    // investigation_phase rotates to the lead investigator's turn.
    session.assert_at_turn_menu(INV1);
    let state = session.state();
    assert_eq!(state.phase, Phase::Investigation);
    assert_eq!(state.round, 2);
    assert!(
        state.encounter_deck.is_empty(),
        "deck must be empty after draw"
    );
    assert!(
        state
            .encounter_discard
            .contains(&CardCode::new(ANCIENT_EVILS)),
        "treachery must be in discard after Revelation resolves",
    );
    assert_eq!(
        state.current_encounter_drawer(),
        None,
        "cursor must be cleared once all investigators have drawn",
    );
    assert_eq!(
        state.active_investigator,
        Some(INV1),
        "investigation_phase rotates to the lead investigator",
    );

    let result = session.finish();
    // CardRevealed fires for Ancient Evils.
    assert_event!(
        result.events,
        Event::CardRevealed { investigator, code, card_type }
            if *investigator == INV1
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
    let (mut base, attic) = board();
    with_encounter_deck(&mut base, &[FLESH_EATER]);

    let drawing = setup_at_mythos_draw(base);
    assert_eq!(drawing.state().phase, Phase::Mythos);

    let session = drawing.confirm();

    session.assert_at_turn_menu(INV1);
    let state = session.state();
    assert_eq!(
        state.enemies.len(),
        1,
        "one enemy must be in play after spawn",
    );
    let enemy = state.enemies.values().next().unwrap();
    assert_eq!(enemy.current_location, Some(attic));
    assert_eq!(enemy.engaged_with, Some(INV1));
    assert!(
        !state
            .encounter_discard
            .contains(&CardCode::new(FLESH_EATER)),
        "spawned enemy must not be in encounter_discard",
    );
    assert_eq!(state.phase, Phase::Investigation);

    // EnemySpawned event fired.
    assert_event!(
        session.finish().events,
        Event::EnemySpawned { code, location, engaged_with, .. }
            if *code == CardCode::new(FLESH_EATER)
                && *location == attic
                && *engaged_with == Some(INV1)
    );
}

#[test]
fn mythos_phase_multi_investigator_spawn_suspends_then_resumes_chain() {
    // Two investigators co-located at the Attic: the drawn Flesh-Eater ties
    // under Prey::Default, so the draw suspends for the lead's PickSingle
    // (#128, option A). Resolving the pick engages the chosen investigator and
    // resumes inv1's Mythos draw chain — which, the enemy being non-surge,
    // advances the cursor to inv2 and stays in Mythos.
    // Both investigators are seated at the Attic (starting_location) by
    // seat_and_open; no pre-seating needed.
    let drawing = setup_two_investigators_at_mythos_draw(board().0)
        // Seed the controlled draw order *after* seat_and_open's shuffle:
        // inv1 draws the enemy; inv2 draws a plain treachery afterward. The
        // draw prompt does not depend on the deck, so it stands.
        .edit_state(|state| with_encounter_deck(state, &[FLESH_EATER, ANCIENT_EVILS]));
    assert_eq!(drawing.state().phase, Phase::Mythos);
    assert_eq!(drawing.state().current_encounter_drawer(), Some(INV1));

    // Draw → spawn tie → suspend for the lead's pick, offered on each tied
    // investigator's own card.
    let suspended = drawing.confirm();
    let first = suspended.state().investigators[&INV1].card_anchor();
    let second = suspended.state().investigators[&INV2].card_anchor();
    assert_eq!(
        (suspended.offered_at(&first), suspended.offered_at(&second)),
        (1, 1),
        "the spawn tie offers both co-located investigators: {:?}",
        suspended.prompt(),
    );
    let enemy = suspended
        .state()
        .enemies
        .values()
        .next()
        .expect("enemy placed");
    assert_eq!(enemy.engaged_with, None, "engagement deferred");
    // The cursor is unchanged — still mid-chain for inv1.
    assert_eq!(suspended.state().current_encounter_drawer(), Some(INV1));

    // Lead picks inv2 → engage + resume the chain. The enemy is non-surge, so
    // no further card draws; the chain advances to inv2.
    let resumed = suspended.pick(second);
    // Picking engages inv2 and re-enters inv1's chain, which completes; the
    // loop then drains inv1 and re-prompts inv2 — the encounter draw, not the
    // spawn pick.
    resumed.assert_at_encounter_draw();
    let enemy = resumed
        .state()
        .enemies
        .values()
        .next()
        .expect("enemy still placed");
    assert_eq!(
        enemy.engaged_with,
        Some(INV2),
        "the lead's pick is now engaged",
    );
    // Chain resumed and advanced to inv2; still in Mythos.
    assert_eq!(resumed.state().phase, Phase::Mythos);
    assert_eq!(resumed.state().current_encounter_drawer(), Some(INV2));
    assert_event!(
        resumed.events(),
        Event::EnemyEngaged { investigator, .. }
            if *investigator == INV2
    );
}

// ------------------------------------------------------------------
// Multi-investigator player order
// ------------------------------------------------------------------

#[test]
fn mythos_phase_multi_investigator_player_order() {
    // Two treacheries — one per investigator.
    let (mut base, _) = board();
    with_encounter_deck(&mut base, &[ANCIENT_EVILS, ANCIENT_EVILS]);

    let drawing = setup_two_investigators_at_mythos_draw(base);

    assert_eq!(drawing.state().phase, Phase::Mythos);
    assert_eq!(
        drawing.state().current_encounter_drawer(),
        Some(INV1),
        "inv1 draws first"
    );

    // inv1 draws their card → the loop re-prompts inv2.
    let second = drawing.confirm();
    second.assert_at_encounter_draw();
    // Still in Mythos; inv2 must draw next.
    assert_eq!(second.state().phase, Phase::Mythos);
    assert_eq!(second.state().current_encounter_drawer(), Some(INV2));

    // inv2 draws their card → completes the phase.
    let done = second.confirm();
    done.assert_at_turn_menu(INV1);
    assert_eq!(done.state().current_encounter_drawer(), None);
    assert_eq!(done.state().phase, Phase::Investigation);
    assert!(done.state().encounter_deck.is_empty());
    assert_eq!(done.state().encounter_discard.len(), 2);
}

// ------------------------------------------------------------------
// Full round chain (round counter bump)
// ------------------------------------------------------------------

#[test]
fn mythos_phase_full_round_chain() {
    let (mut base, _) = board();
    with_encounter_deck(&mut base, &[ANCIENT_EVILS]);

    let drawing = setup_at_mythos_draw(base);
    // Confirm the round bumped on Mythos entry.
    assert_eq!(drawing.state().round, 2);
    assert_eq!(drawing.state().phase, Phase::Mythos);

    let session = drawing.confirm();

    session.assert_at_turn_menu(INV1);
    assert_eq!(
        session.state().round,
        2,
        "round stays 2 — it bumps on Mythos *entry*"
    );
    assert_eq!(session.state().phase, Phase::Investigation);
    assert_eq!(session.state().active_investigator, Some(INV1));
}

// ------------------------------------------------------------------
// Empty-deck rejection
// ------------------------------------------------------------------

#[test]
fn mythos_draw_rejects_when_initial_deck_and_discard_both_empty() {
    // Deck and discard both empty from `board()`; nothing seeds either.
    let drawing = setup_at_mythos_draw(board().0);
    assert_eq!(drawing.state().phase, Phase::Mythos);
    assert_eq!(drawing.state().current_encounter_drawer(), Some(INV1));

    let refused = drawing.confirm();

    let reason = refused.expect_rejected();
    assert!(
        reason.contains("encounter deck and discard both empty"),
        "unexpected reject reason: {reason:?}",
    );

    // Validate-first: state must be unchanged — still Mythos, cursor preserved.
    refused.assert_at_encounter_draw();
    assert_eq!(
        refused.state().phase,
        Phase::Mythos,
        "phase must not change on Rejected",
    );
    assert_eq!(
        refused.state().current_encounter_drawer(),
        Some(INV1),
        "cursor must be preserved on initial Rejected (validate-first)",
    );
    let result = refused.finish();
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
fn at_mythos_draw_with_beat_cop_in_play() -> TestSession {
    // No step puts a card into play at the encounter-draw prompt, so Beat Cop
    // arrives by a state edit. Minted rather than hand-picked: `seat_and_open`
    // already gave the seated investigator card instance 0, and a second card
    // sharing that id resolves to the investigator instead — the ability
    // lookup then finds no printed index 1 and the window silently auto-skips.
    setup_at_mythos_draw(board().0).edit_state(|state| {
        let instance = state.card_instance_ids.mint();
        state
            .investigators
            .get_mut(&INV1)
            .expect("inv1 must be present")
            .cards_in_play
            .push(CardInPlay::enter_play(
                CardCode::new(BEAT_COP),
                instance,
                Owner::Investigator(INV1),
            ));
        with_encounter_deck(state, &[FLESH_EATER]);
    })
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
    let drawing = at_mythos_draw_with_beat_cop_in_play();
    assert_eq!(drawing.state().phase, Phase::Mythos);
    assert_eq!(drawing.state().current_encounter_drawer(), Some(INV1));

    let session = drawing.confirm();

    // #476: the MythosAfterDraws window now surfaces the eligible Fast play as
    // a skippable choice instead of idling as Done — the player can use it or pass.
    let request = session.prompt();
    assert_eq!(request.kind, InputKind::PickSingle, "{request:?}");
    assert!(
        request.skippable,
        "the fast window is skippable (pass): {request:?}"
    );
    // The window was NOT auto-skipped: it offers Beat Cop's ability.
    let beat_cop = session.state().card(INV1, BEAT_COP);
    assert_eq!(
        session.offered_at(&beat_cop),
        1,
        "the MythosAfterDraws prompt lists the eligible Fast play; pre-fix the window \
         auto-skipped: {request:?}"
    );

    // Phase must still be Mythos — the window continuation (mythos_phase_end)
    // has NOT fired yet. (The window staying open + the phase not advancing is
    // the observable signal that it was not auto-skipped, and that the open
    // window is MythosAfterDraws.)
    assert_eq!(
        session.state().phase,
        Phase::Mythos,
        "phase must still be Mythos while MythosAfterDraws window is open"
    );
}

/// Continuation of the push-then-scan regression: after the encounter draw
/// leaves the `MythosAfterDraws` window open (because a Fast play is
/// eligible), a `Skip` must close the window, run `mythos_phase_end`, and
/// transition to Investigation.
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
    let window = at_mythos_draw_with_beat_cop_in_play().confirm();
    let beat_cop = window.state().card(INV1, BEAT_COP);
    assert!(
        window.prompt().skippable && window.offered_at(&beat_cop) == 1,
        "the draw lands on the skippable fast-window prompt, got {:?}",
        window.prompt(),
    );
    assert_eq!(
        window.state().phase,
        Phase::Mythos,
        "window must be open before Skip test"
    );

    // Now close the window with Skip (player decides not to use the Fast ability).
    let session = window.skip();

    // MythosAfterDraws is closed; investigation_phase then opens
    // InvestigationBegins. Because Beat Cop's ability is still eligible (the
    // spawned enemy is still there), that window also surfaces a skippable
    // prompt (#476) rather than idling.
    let request = session.prompt();
    assert!(
        request.skippable,
        "the InvestigationBegins fast window is skippable: {request:?}"
    );
    assert_eq!(
        session.offered_at(&beat_cop),
        1,
        "InvestigationBegins window must be open (the Fast ability is eligible): {request:?}"
    );

    // mythos_phase_end ran: phase transitioned to Investigation, so the open
    // window is InvestigationBegins and MythosAfterDraws is gone.
    // active_investigator is None until InvestigationBegins closes
    // (its continuation begin_investigator_turn rotates to the lead).
    assert_eq!(
        session.state().phase,
        Phase::Investigation,
        "phase must be Investigation after MythosAfterDraws window closes"
    );
    assert_eq!(
        session.state().active_investigator,
        None,
        "active investigator not yet set — InvestigationBegins window is still open"
    );
    assert_eq!(
        session.state().round,
        2,
        "round stays 2 — it bumped on Mythos entry"
    );
    assert_event!(
        session.events(),
        Event::PhaseEnded {
            phase: Phase::Mythos
        }
    );
}
