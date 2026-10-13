//! End-to-end reaction-window flow with a mock `CardRegistry` that
//! carries `Trigger::OnEvent` abilities.
//!
//! Lives at `crates/game-core/tests/` so it runs in its own integration-
//! test process (separate `OnceLock<CardRegistry>`), letting it install
//! a mock registry without colliding with game-core's in-crate tests or
//! with other `tests/*.rs` files. Mirrors `on_skill_test_resolution.rs`.
//!
//! Roland Banks (01001, #55) is the first real-card `Trigger::OnEvent`
//! consumer; that card's end-to-end test lives in
//! `crates/cards/tests/roland_banks.rs`. This file keeps mock cards to
//! exercise edge cases (multi-controller defeats, two abilities on one
//! card, `by_controller: false`) that no real Phase-3 card hits.

use card_dsl::dsl::{
    self, Ability, EventPattern, EventTiming, InvestigatorTarget, LocationTarget, SkillTestKind,
    TestOutcome, TestedLocationScope,
};
use game_core::action::{Action, InputResponse, PlayerAction};
use game_core::engine::enumerate::TurnAction;
use game_core::engine::{OptionId, OptionTarget};
use game_core::event::Event;
use game_core::state::{
    CardCode, ChaosBag, ChaosToken, Enemy, EnemyId, GameStateBuilder, InvestigatorId, Location,
    LocationId, TokenModifiers,
};
use game_core::test_support::{self, seat, MockRegistry, Seat, SeatLookup as _, TestSession};
use game_core::{assert_event, assert_no_event};

/// Mock: optional reaction "after you defeat an enemy, discover 1 clue
/// at your location" — the Roland-shape canonical `OnEvent` test card.
const ROLAND_REACTION: &str = "MOCK-OE-ROLAND";

/// Mock: optional reaction "after any enemy is defeated, gain 1
/// resource" — the no-`by_controller` variant. Exercises the "any
/// defeat matches" branch of `trigger_matches`.
const BYSTANDER_REACTION: &str = "MOCK-OE-BYSTANDER";

/// Mock: a card with two `OnEvent` abilities. Exercises the
/// per-ability-index loop inside the reaction scan so each
/// ability gets its own pending-trigger entry.
const TWO_REACTIONS: &str = "MOCK-OE-TWO";

/// Mock: optional reaction "after you successfully investigate, gain 1
/// resource" — the Dr. Milan Christopher 01033 shape (C6a #241).
const MILAN_REACTION: &str = "MOCK-OE-MILAN";

/// Mock: optional reaction "after **a** skill test is successfully resolved,
/// gain 1 resource" — `by_controller: false`, the unqualified form Lita
/// Chantler 01117 needs (#773). Its controller reacts to somebody else's test.
const BYSTANDER_TEST_REACTION: &str = "MOCK-OE-BYSTANDER-TEST";

/// Mock: the Roland-shape reaction whose effect is a two-branch `ChooseOne`.
/// Pins the *other* half of #555's anchor threading — a reaction fired from an
/// in-play instance, where `fire_pending_trigger` is the setter site.
const MODAL_REACTION: &str = "MOCK-OE-MODAL";

/// The `EnemyDefeated` pattern every defeat reaction here keys off.
fn enemy_defeated(by_controller: bool) -> EventPattern {
    EventPattern::EnemyDefeated {
        by_controller,
        code: None,
    }
}

/// The `SkillTestResolved` pattern the successful-investigate reactions key off.
fn investigated(by_controller: bool) -> EventPattern {
    EventPattern::SkillTestResolved {
        outcome: TestOutcome::Success,
        kind: Some(SkillTestKind::Investigate),
        by_controller,
        tested_location: TestedLocationScope::Any,
    }
}

/// A reaction on `pattern` whose whole effect is gaining 1 resource — the shape
/// both investigate reactions and the bystander defeat reaction print, differing
/// only in the condition they key off.
fn react_gaining_a_resource(pattern: EventPattern) -> Vec<Ability> {
    vec![dsl::reaction_on_event(
        pattern,
        EventTiming::After,
        dsl::gain_resources(InvestigatorTarget::You, 1),
    )]
}

#[ctor::ctor(unsafe)]
fn install_mock_registry() {
    MockRegistry::new()
        .with_abilities(MODAL_REACTION, || {
            vec![dsl::reaction_on_event(
                enemy_defeated(true),
                EventTiming::After,
                dsl::choose_one([
                    (
                        "Gain 1 resource",
                        dsl::gain_resources(InvestigatorTarget::You, 1),
                    ),
                    (
                        "Gain 3 resources",
                        dsl::gain_resources(InvestigatorTarget::You, 3),
                    ),
                ]),
            )]
        })
        .with_abilities(ROLAND_REACTION, || {
            vec![dsl::reaction_on_event(
                enemy_defeated(true),
                EventTiming::After,
                dsl::discover_clue(LocationTarget::YourLocation, 1),
            )]
        })
        .with_abilities(BYSTANDER_REACTION, || {
            react_gaining_a_resource(enemy_defeated(false))
        })
        .with_abilities(TWO_REACTIONS, || {
            vec![
                dsl::reaction_on_event(
                    enemy_defeated(true),
                    EventTiming::After,
                    dsl::discover_clue(LocationTarget::YourLocation, 1),
                ),
                dsl::reaction_on_event(
                    enemy_defeated(true),
                    EventTiming::After,
                    dsl::gain_resources(InvestigatorTarget::You, 1),
                ),
            ]
        })
        .with_abilities(MILAN_REACTION, || {
            react_gaining_a_resource(investigated(true))
        })
        .with_abilities(BYSTANDER_TEST_REACTION, || {
            react_gaining_a_resource(investigated(false))
        })
        .install();
}

const INV: InvestigatorId = InvestigatorId(1);
const OTHER: InvestigatorId = InvestigatorId(2);
const GHOUL: EnemyId = EnemyId(100);
const HERE: LocationId = LocationId(10);
const ELSEWHERE: LocationId = LocationId(11);

/// A Mock Ghoul at 1 of 2 health, so a successful Fight defeats it.
fn ghoul() -> Enemy {
    let mut enemy = test_support::test_enemy(GHOUL.0, "Mock Ghoul");
    enemy.fight = 3;
    enemy.max_health = 2;
    enemy.damage = 1;
    enemy
}

/// `INV`'s open turn at a location holding `location_clues`, engaged with the
/// Ghoul (co-located: Fight is location-gated, #401), with `seats` seated. The
/// test investigator's combat 3 against the Ghoul's fight 3 with a `+0` token
/// always succeeds, so the Fight defeats it.
fn fight_board(seats: impl IntoIterator<Item = Seat>, location_clues: u8) -> GameStateBuilder {
    let mut location = test_support::test_location(HERE.0, "Mock Location");
    location.clues = location_clues;
    seats
        .into_iter()
        .fold(GameStateBuilder::new(), GameStateBuilder::with_seat)
        .with_enemy_engaged(ghoul(), INV)
        .with_location(location)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .open_turn(INV)
}

/// The Fight-to-defeat board: `INV` alone with `in_play` in play, at a 3-clue
/// location.
fn fight_to_defeat_scenario(in_play: &[&str]) -> GameStateBuilder {
    fight_board([seat(1).at(HERE).in_play(in_play.iter().copied())], 3)
}

/// `INV` fights the Ghoul and commits `commit` to the test. The session rests
/// at the reaction window if one opened, or back at the turn menu.
fn fight_committing(board: GameStateBuilder, commit: &[&str]) -> TestSession {
    board
        .session()
        .take(&TurnAction::Fight {
            investigator: INV,
            enemy: GHOUL,
        })
        .commit(commit)
}

/// [`fight_committing`] nothing.
fn fight_through_commit_window(board: GameStateBuilder) -> TestSession {
    fight_committing(board, &[])
}

/// The anchor of the card `code` in `investigator`'s play or threat area: the
/// reactions printed on it are offered on it.
fn card(session: &TestSession, investigator: InvestigatorId, code: &str) -> OptionTarget {
    OptionTarget::CardInstance(session.state().in_play(investigator, code))
}

/// Assert the session rests at a reaction window offering `reactions` options
/// on `on` and nothing else.
#[track_caller]
fn assert_offers_only(session: &TestSession, on: &OptionTarget, reactions: usize) {
    assert_eq!(
        (session.offered_at(on), session.prompt().options.len()),
        (reactions, reactions),
        "expected {reactions} reaction(s) on {on:?} alone: {:?}",
        session.prompt(),
    );
}

#[test]
fn no_in_play_reaction_means_no_window_opens() {
    // No cards in play → no triggers → no window. The Fight resolves
    // back to the turn menu and never suspends on a reaction window. Sanity
    // check that the in-play scan is the gate.
    let session = fight_through_commit_window(fight_to_defeat_scenario(&[]));

    // No AfterEnemyDefeated reaction window (none in play): the step runs on
    // to the turn menu rather than resting at one.
    session.assert_at_turn_menu(INV);
    assert_event!(
        session.events(),
        Event::EnemyDefeated { enemy: e, by: Some(by) } if *e == GHOUL && *by == INV
    );
}

#[test]
fn matching_reaction_opens_window_and_suspends() {
    // ROLAND_REACTION in play, Fight defeats the enemy → window opens
    // with one pending trigger; the session rests at it, offering that one
    // reaction on its card.
    let session = fight_through_commit_window(fight_to_defeat_scenario(&[ROLAND_REACTION]));

    // The window is after the enemy defeat, and suspends the Fight mid-action:
    // the defeat has landed, the skill test has not ended.
    assert_event!(
        session.events(),
        Event::EnemyDefeated { enemy: e, by: Some(by) } if *e == GHOUL && *by == INV
    );
    assert_no_event!(session.events(), Event::SkillTestEnded { .. });
    // One pending trigger, the one ability printed on `INV`'s in-play card,
    // offered on that card.
    assert_offers_only(&session, &card(&session, INV, ROLAND_REACTION), 1);
    assert!(session.prompt().skippable, "an optional reaction window");
}

#[test]
fn pick_fires_pending_trigger_and_closes_window() {
    // Open the window, then pick the reaction → effect fires (clue
    // discovered), the entry drains, the window closes, and the
    // session is back at the turn menu.
    let paused = fight_through_commit_window(fight_to_defeat_scenario(&[ROLAND_REACTION]));
    let reaction = card(&paused, INV, ROLAND_REACTION);

    let resumed = paused.pick(reaction);

    resumed.assert_at_turn_menu(INV);
    assert_event!(
        resumed.events(),
        Event::CluePlaced { investigator, count: 1 } if *investigator == INV
    );
    assert_eq!(resumed.state().investigators[&INV].clues, 1);
    assert_eq!(resumed.state().locations[&HERE].clues, 2);
}

/// A `ChooseOne` inside a reaction's effect offers its branches' authored
/// labels, anchored to the in-play card the ability is printed on (#555 /
/// ADR 0011: *"the client never decides where a control belongs — it reads the
/// anchor the engine attached"*). The anchor comes from the pending trigger's
/// own `AbilitySource`, threaded through `EvalContext::ability_source` by
/// `fire_pending_trigger`; drop that and the options silently fall back to the
/// prompt banner, which is why this is pinned.
#[test]
fn a_choose_one_inside_a_reaction_anchors_to_the_firing_card() {
    let paused = fight_through_commit_window(fight_to_defeat_scenario(&[MODAL_REACTION]));
    let modal = card(&paused, INV, MODAL_REACTION);

    // Pick the reaction out of its window; its effect's ChooseOne is next.
    let resumed = paused.pick(modal.clone());
    let request = resumed.prompt();
    assert_eq!(
        request
            .options
            .iter()
            .map(|o| o.label.as_str())
            .collect::<Vec<_>>(),
        ["Gain 1 resource", "Gain 3 resources"],
        "branches are offered under their authored labels, not their Debug form",
    );
    for option in &request.options {
        assert_eq!(
            option.target,
            Some(modal.clone()),
            "anchored to the in-play card the reaction is printed on",
        );
    }
}

#[test]
fn skip_closes_an_optional_only_window_without_firing() {
    // Open the window, then Skip → no effect fires; the window
    // closes; the session is back at the turn menu.
    let resumed = fight_through_commit_window(fight_to_defeat_scenario(&[ROLAND_REACTION])).skip();

    resumed.assert_at_turn_menu(INV);
    assert_no_event!(resumed.events(), Event::CluePlaced { .. });
    assert_eq!(resumed.state().investigators[&INV].clues, 0);
    assert_eq!(resumed.state().locations[&HERE].clues, 3);
}

#[test]
fn by_controller_filter_excludes_unrelated_investigators() {
    // ROLAND_REACTION belongs to a second investigator who is NOT the
    // attacker. The trigger is `by_controller: true`, so it must NOT
    // match the defeat (the active investigator made the kill, not
    // the controller of ROLAND_REACTION). No window opens.
    let board = fight_board(
        [
            seat(1).at(HERE),
            seat(2).at(HERE).in_play([ROLAND_REACTION]),
        ],
        3,
    )
    .with_turn_order([INV, OTHER]);

    let session = fight_through_commit_window(board);

    // No AfterEnemyDefeated window for the unrelated investigator — resolution
    // ran straight back to the turn menu without resting at a reaction window.
    session.assert_at_turn_menu(INV);
}

#[test]
fn unqualified_pattern_matches_any_defeat() {
    // BYSTANDER_REACTION uses `by_controller: false`, so a defeat by
    // ANY investigator triggers it. Here the trigger's controller
    // (investigator 2) isn't the attacker, but the window still
    // opens and the effect fires on resolve.
    let board = fight_board(
        [
            seat(1).at(HERE),
            seat(2).at(HERE).in_play([BYSTANDER_REACTION]),
        ],
        0,
    )
    .with_turn_order([INV, OTHER]);

    let paused = fight_through_commit_window(board);
    let byst_resources_before = paused.state().investigators[&OTHER].resources;
    // The window opens with one reaction, on the card in `OTHER`'s play area.
    let reaction = card(&paused, OTHER, BYSTANDER_REACTION);
    assert_offers_only(&paused, &reaction, 1);

    let resumed = paused.pick(reaction);
    resumed.assert_at_turn_menu(INV);
    assert_eq!(
        resumed.state().investigators[&OTHER].resources,
        byst_resources_before + 1,
        "BYSTANDER_REACTION's gain_resources fires on the trigger's controller, \
         not on the attacker",
    );
}

#[test]
fn pick_index_out_of_bounds_rejects_window_stays_open() {
    let paused = fight_through_commit_window(fight_to_defeat_scenario(&[ROLAND_REACTION]));
    let reaction = card(&paused, INV, ROLAND_REACTION);

    // The subject is a malformed response, so it is sent raw: an option id the
    // prompt never offered.
    let bad = paused.apply(Action::Player(PlayerAction::ResolveInput {
        response: InputResponse::PickSingle(OptionId(99)),
    }));
    let reason = bad.expect_rejected();
    assert!(
        reason.contains("out of bounds"),
        "unexpected reason: {reason}"
    );
    assert_eq!(
        bad.offered_at(&reaction),
        1,
        "window must survive a rejected pick so the client can retry"
    );
}

#[test]
fn multiple_pending_triggers_resolve_one_at_a_time() {
    // TWO_REACTIONS exposes two OnEvent abilities on one card. The window
    // opens with both pending; picking the first ability fires it (clue), the
    // window re-prompts with one entry remaining; picking that fires the second
    // (resource); window closes.
    let paused = fight_through_commit_window(fight_to_defeat_scenario(&[TWO_REACTIONS]));
    let two = card(&paused, INV, TWO_REACTIONS);
    assert_offers_only(&paused, &two, 2);

    let after_first = paused.pick_nth_at(two.clone(), 0);
    assert_event!(
        after_first.events(),
        Event::CluePlaced { investigator, count: 1 } if *investigator == INV
    );
    // The window stays open while a trigger remains pending.
    assert_offers_only(&after_first, &two, 1);

    let resources_before = after_first.state().investigators[&INV].resources;
    let after_second = after_first.pick(two);
    after_second.assert_at_turn_menu(INV);
    assert_event!(
        after_second.events(),
        Event::ResourcesGained { investigator, amount: 1 } if *investigator == INV
    );
    assert_eq!(after_second.state().locations[&HERE].clues, 2);
    assert_eq!(
        after_second.state().investigators[&INV].resources,
        resources_before + 1,
    );
}

#[test]
fn fight_event_sequence_pins_window_between_enemy_defeated_and_skill_test_ended() {
    // Per the Rules Reference, "after… [reaction] abilities may be
    // used immediately after that triggering condition's impact upon
    // the game state has resolved" — i.e. mid-action. For a Fight
    // that defeats an enemy with Roland-shaped reaction in play, the
    // canonical event order is:
    //   SkillTestSucceeded → EnemyDamaged → EnemyDefeated
    //     → [AfterEnemyDefeated reaction window]
    //       → CluePlaced (Roland's clue)
    //       → LocationCluesChanged
    //     → [OnSkillTestResolution events if any committed]
    //     → CardDiscarded × committed
    //     → SkillTestEnded
    //
    // This pin protects the ordering: the reaction effect (CluePlaced) must
    // fire AFTER EnemyDefeated but BEFORE SkillTestEnded — i.e. mid-action.
    // Any future refactor that moves the reaction back to a post-action
    // position (the rules-incorrect "deferred" design that landed initially)
    // requires an intentional decision (and an update here).
    let paused = fight_through_commit_window(fight_to_defeat_scenario(&[ROLAND_REACTION]));
    let reaction = card(&paused, INV, ROLAND_REACTION);
    // The Fight suspends at the reaction window before SkillTestEnded fires.
    assert_offers_only(&paused, &reaction, 1);
    assert_no_event!(paused.events(), Event::SkillTestEnded { .. });

    // Drive past the window by firing the single pending trigger.
    let resumed = paused.pick(reaction);
    resumed.assert_at_turn_menu(INV);

    // Every event since the board was built, across the suspend point.
    let events = resumed.events();

    // Walk the event list and assert ordering of the milestones. The reaction
    // effect (CluePlaced) fires between EnemyDefeated and SkillTestEnded — the
    // window opening/closing itself is observed via the suspend (the
    // pending window above), not a discrete event.
    let mut defeated_idx: Option<usize> = None;
    let mut clue_idx: Option<usize> = None;
    let mut ended_idx: Option<usize> = None;
    for (i, ev) in events.iter().enumerate() {
        match ev {
            Event::EnemyDefeated { enemy: e, .. } if *e == GHOUL => defeated_idx = Some(i),
            Event::CluePlaced { .. } => clue_idx = Some(i),
            Event::SkillTestEnded { .. } => ended_idx = Some(i),
            _ => {}
        }
    }
    let defeated = defeated_idx.expect("EnemyDefeated must fire");
    let clue = clue_idx.expect("CluePlaced must fire (Roland's reaction)");
    let ended = ended_idx.expect("SkillTestEnded must fire");

    assert!(
        defeated < clue,
        "CluePlaced (reaction effect) fires after the EnemyDefeated it reacts to"
    );
    assert!(
        clue < ended,
        "CluePlaced precedes SkillTestEnded — the rules-correct mid-action shape \
         (the reaction resolves before the test ends)"
    );
}

#[test]
fn reaction_window_closes_before_on_skill_test_resolution_fires() {
    // A Fight that both defeats an enemy AND has a committed
    // `OnSkillTestResolution` card pins the cross-step ordering:
    //   EnemyDefeated → [reaction window opens] → [Roland fires]
    //     → [reaction window closes]
    //     → OnSkillTestResolution effect events
    //     → CardDiscarded (committed) → SkillTestEnded
    //
    // Per the Rules Reference, OnSkillTestResolution is part of the
    // skill test's resolution machinery (not a reaction window), so
    // it sits inside the test's resolution but AFTER the
    // `EnemyDefeated` reaction window — that reaction is response to
    // the defeat impact, which already resolved by the time we get
    // to OnSkillTestResolution.
    //
    // Use a registry-known card with an OnSkillTestResolution
    // ability. We don't have one in the mock registry; instead just
    // exercise the "open window resumes through the
    // OnSkillTestResolution step" boundary. The OnSkillTestResolution
    // step is a no-op without an installed registry entry for the
    // committed card, but the event-order pinning still applies: the
    // window closes BEFORE the discard-and-end sequence.
    let board = fight_board(
        [seat(1)
            .at(HERE)
            .in_play([ROLAND_REACTION])
            .hand(["COMMITTED"])],
        3,
    );

    // Commit the card → drives through follow-up → queues window → suspends.
    let paused_reaction = fight_committing(board, &["COMMITTED"]);
    let reaction = card(&paused_reaction, INV, ROLAND_REACTION);
    // The commit suspends at the reaction window.
    assert_offers_only(&paused_reaction, &reaction, 1);

    // Fire the reaction → window closes → driver resumes →
    // OnSkillTestResolution step → discard → SkillTestEnded → turn menu.
    let resumed = paused_reaction.pick(reaction).finish();

    // Verify the final event order from the resumed step: the reaction effect
    // (CluePlaced, fired inside the now-closing window) precedes the
    // OnSkillTestResolution/discard step, which precedes SkillTestEnded. The
    // window closing itself is no longer a discrete event — the reaction
    // resolving before the discard is the observable mid-action shape.
    let mut clue_idx: Option<usize> = None;
    let mut discarded_idx: Option<usize> = None;
    let mut ended_idx: Option<usize> = None;
    for (i, ev) in resumed.events.iter().enumerate() {
        match ev {
            Event::CluePlaced { .. } => clue_idx = Some(i),
            Event::CardDiscarded { .. } => discarded_idx = Some(i),
            Event::SkillTestEnded { .. } => ended_idx = Some(i),
            _ => {}
        }
    }
    let clue = clue_idx.expect("CluePlaced must fire (Roland's reaction on resume)");
    let discarded = discarded_idx.expect("CardDiscarded must fire (committed card)");
    let ended = ended_idx.expect("SkillTestEnded must fire");
    assert!(
        clue < discarded,
        "the reaction effect resolves before the discard step (window closes before discard)",
    );
    assert!(
        discarded < ended,
        "CardDiscarded precedes SkillTestEnded (pre-existing pin from #63)",
    );
    // The committed card landed in discard.
    assert_eq!(
        resumed.state.investigators[&INV].discard,
        vec![CardCode::new("COMMITTED")],
    );
    // The clue Roland's reaction discovered landed.
    assert_eq!(resumed.state.locations[&HERE].clues, 2);
    assert_eq!(resumed.state.investigators[&INV].clues, 1);
}

#[test]
fn pending_triggers_order_active_investigator_first_then_turn_order() {
    // Two investigators both carry a `by_controller: false` reaction
    // (so the defeat — credited to the active investigator — matches
    // for both). The pending list must put the active investigator's
    // trigger first, followed by the other investigator's, per
    // Arkham's active-investigator-first / turn-order priority for
    // reaction windows.
    let board = fight_board(
        [
            seat(1).at(HERE).in_play([BYSTANDER_REACTION]),
            seat(2).at(HERE).in_play([BYSTANDER_REACTION]),
        ],
        0,
    )
    .with_turn_order([INV, OTHER]);

    let paused = fight_through_commit_window(board);
    // Each reaction is offered on the card in its controller's play area, so
    // the offer order is the candidates' order.
    let offered: Vec<_> = paused
        .prompt()
        .options
        .iter()
        .map(|option| option.target.clone())
        .collect();

    assert_eq!(
        offered,
        vec![
            Some(card(&paused, INV, BYSTANDER_REACTION)),
            Some(card(&paused, OTHER, BYSTANDER_REACTION)),
        ],
        "the active investigator's trigger comes first, then the non-active \
         investigator's, in turn order",
    );
}

#[test]
fn skip_after_firing_one_drops_remaining_optionals() {
    // TWO_REACTIONS has two optional triggers on one card. Fire the first
    // (consuming it), then Skip. The second optional must NOT
    // fire — its effect (gain 1 resource) leaves resources unchanged
    // from the post-first-fire baseline. The window closes cleanly.
    let paused = fight_through_commit_window(fight_to_defeat_scenario(&[TWO_REACTIONS]));
    let two = card(&paused, INV, TWO_REACTIONS);
    assert_offers_only(&paused, &two, 2);

    let after_first = paused.pick_nth_at(two.clone(), 0);
    // One optional fired, one remains pending.
    assert_offers_only(&after_first, &two, 1);
    let resources_after_first = after_first.state().investigators[&INV].resources;

    let skipped = after_first.skip();
    skipped.assert_at_turn_menu(INV);
    // The skipped optional was the resource-gain — its effect didn't fire.
    assert_eq!(
        skipped.state().investigators[&INV].resources,
        resources_after_first,
        "Skip must not fire the remaining optional's effect",
    );
    // Sanity: the first optional did fire — the location lost a clue
    // (TWO_REACTIONS' first ability discovers 1 clue at the
    // controller's location).
    assert_eq!(skipped.state().locations[&HERE].clues, 2);
    assert_eq!(skipped.state().investigators[&INV].clues, 1);
}

/// The Fight-to-defeat board with `ROLAND_REACTION` in `INV`'s threat area
/// instead of in play.
fn threat_area_reaction_board() -> GameStateBuilder {
    fight_board([seat(1).at(HERE).threat_area([ROLAND_REACTION])], 3)
}

#[test]
fn reaction_trigger_in_threat_area_opens_window() {
    // The shared scan source spans cards_in_play + threat_area, so a
    // reaction ability on a threat-area card is offered just like one
    // in play. Build the standard fight-to-defeat scenario but seat
    // ROLAND_REACTION in the threat area.
    let session = fight_through_commit_window(threat_area_reaction_board());

    // One reaction, offered on the card in `INV`'s threat area.
    assert_offers_only(&session, &card(&session, INV, ROLAND_REACTION), 1);
}

#[test]
fn active_reaction_window_is_the_top_continuation_frame() {
    // Invariant the loop-driven dispatch relies on (Slice C-plumbing, #431): the
    // continuation stack is the resolution order, so an *active* reaction window
    // (one with pending candidates) is always `continuations.top()` — never
    // stranded beneath another frame. The engine dispatches the top frame and
    // operates on it directly (no `top_reaction_window_index` reach-down); this
    // pins the property that makes that correct. (Replaces the former
    // `close_reaction_window_at_removes_..._phase_gate_on_top` regression, which
    // hand-injected an empty gate *above* a pending reaction window — a shape the
    // framework never produces, because the `drive` loop stops at a pending
    // window's prompt, so the framework never advances past it to open one.)
    let paused = fight_through_commit_window(fight_to_defeat_scenario(&[ROLAND_REACTION]));
    let top = paused
        .state()
        .continuations
        .top()
        .expect("a reaction window is open");
    assert!(
        top.pending_candidates().is_some_and(|c| !c.is_empty()),
        "the active reaction window must be the top frame, got {top:?}",
    );
}

#[test]
fn pick_fires_threat_area_reaction_and_closes_window() {
    // ROLAND_REACTION is seated in the investigator's threat_area. The scan
    // already finds it there; now verify the fire path also resolves it:
    // picking it discovers 1 clue, the window closes, and the session is back
    // at the turn menu.
    let paused = fight_through_commit_window(threat_area_reaction_board());
    let reaction = card(&paused, INV, ROLAND_REACTION);
    // The threat-area reaction opens a window.
    assert_offers_only(&paused, &reaction, 1);

    let resumed = paused.pick(reaction);

    resumed.assert_at_turn_menu(INV);
    assert_event!(
        resumed.events(),
        Event::CluePlaced { investigator, count: 1 } if *investigator == INV
    );
    assert_eq!(resumed.state().investigators[&INV].clues, 1);
    assert_eq!(resumed.state().locations[&HERE].clues, 2);
}

// ------------------------------------------------------------------
// After-successful-investigate reaction window (Dr. Milan, C6a #241)
// ------------------------------------------------------------------

/// The 1-clue Study (shroud 2).
fn one_clue_study() -> Location {
    let mut study = test_support::test_location(HERE.0, "Study");
    study.clues = 1;
    study
}

/// `INV`'s open turn at the 1-clue Study with `in_play` in play, and a
/// `Numeric(0)` bag, so the test investigator's intellect 3 ≥ shroud 2 → the
/// Investigate succeeds and discovers the clue.
fn investigate_to_success_scenario(in_play: &[&str]) -> GameStateBuilder {
    GameStateBuilder::new()
        .with_seat(seat(1).at(HERE).in_play(in_play.iter().copied()))
        .with_location(one_clue_study())
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .open_turn(INV)
}

/// [`investigate_to_success_scenario`] with a **second** investigator, who
/// holds `bystander_card` and stands elsewhere. The first investigator takes
/// the test; the second is the one whose card may or may not react to it.
///
/// The second is seated in `turn_order`, as in a real game, so the reaction
/// scan walks their cards for the reason the tests below are about rather than
/// by the walk's fallback for an unseated fixture investigator.
fn investigate_with_a_bystander(bystander_card: &str) -> GameStateBuilder {
    GameStateBuilder::new()
        .with_seat(seat(1).at(HERE))
        .with_seat(seat(2).at(ELSEWHERE).in_play([bystander_card]))
        .with_location(one_clue_study())
        .with_location(test_support::test_location(ELSEWHERE.0, "Hallway"))
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .with_turn_order([INV, OTHER])
        .open_turn(INV)
}

/// Drive a successful Investigate by `INV`, committing nothing, stopping at
/// whatever the engine yields after the commit window closes.
fn investigate_and_commit_nothing(board: GameStateBuilder) -> TestSession {
    board
        .session()
        .take(&TurnAction::Investigate { investigator: INV })
        .commit(&[])
}

/// `by_controller: false` is the whole point of the field: the matcher runs
/// **before** a candidate is minted, so without it a reaction to another
/// investigator's test is unreachable no matter what the card says afterwards.
/// Here the bystander's card fires on a test they did not take (#773).
#[test]
fn an_unqualified_listener_reacts_to_another_investigators_test() {
    let paused =
        investigate_and_commit_nothing(investigate_with_a_bystander(BYSTANDER_TEST_REACTION));
    let before = paused.state().investigators[&OTHER].resources;
    // The unqualified listener opens a window on the other investigator's test.
    let reaction = card(&paused, OTHER, BYSTANDER_TEST_REACTION);
    assert_offers_only(&paused, &reaction, 1);

    let resumed = paused.pick(reaction);
    assert_eq!(
        resumed.state().investigators[&OTHER].resources,
        before + 1,
        "the reaction resolved for its own controller, not for the tester",
    );
}

/// The regression the field must not break: `by_controller: true` — every
/// existing consumer, Dr. Milan 01033 and Obscuring Fog 01168 — still sees only
/// its own controller's test. Same board, same event, opposite answer.
#[test]
fn a_by_controller_listener_ignores_another_investigators_test() {
    let settled = investigate_with_a_bystander(MILAN_REACTION).session();
    let before = settled.state().investigators[&OTHER].resources;

    let resumed = settled
        .take(&TurnAction::Investigate { investigator: INV })
        .commit(&[]);
    assert_eq!(
        resumed.state().investigators[&OTHER].resources,
        before,
        "a \"after **you** …\" reaction must not fire on someone else's test",
    );
    // No candidate was minted at all, so no window held the Investigate.
    resumed.assert_at_turn_menu(INV);
}

/// A successful Investigate opens the after-investigate window for the
/// in-play reaction (Dr. Milan), which the controller fires to gain a
/// resource; the skill test then resumes to completion.
#[test]
fn after_successful_investigate_fires_in_play_reaction() {
    // Commit nothing → test succeeds → clue discovered → after-investigate
    // window opens → suspends.
    let paused_reaction =
        investigate_and_commit_nothing(investigate_to_success_scenario(&[MILAN_REACTION]));
    let reaction = card(&paused_reaction, INV, MILAN_REACTION);
    // The after-investigate reaction window suspends the test.
    assert_offers_only(&paused_reaction, &reaction, 1);
    let resources_before = paused_reaction.state().investigators[&INV].resources;

    // Fire the reaction → gain 1 resource → resume the test → turn menu.
    let resumed = paused_reaction.pick(reaction);
    resumed.assert_at_turn_menu(INV);
    assert_eq!(
        resumed.state().investigators[&INV].resources,
        resources_before + 1,
        "Dr. Milan reaction gained a resource",
    );
    assert_eq!(
        resumed.state().locations[&HERE].clues,
        0,
        "clue was discovered"
    );
}

/// With no after-investigate reaction in play, a successful Investigate
/// opens no window — it completes in the single commit step (regression:
/// `queue_reaction_window` is a no-op when nothing reacts).
#[test]
fn after_successful_investigate_no_window_without_reaction() {
    let resolved = investigate_and_commit_nothing(investigate_to_success_scenario(&[]));

    // No reaction card is in play, so no AfterSuccessfulInvestigate window can
    // open: the investigate resolves straight through to the open-turn menu in
    // one step (the clue assertions below confirm it resolved).
    resolved.assert_at_turn_menu(INV);
    assert_eq!(resolved.state().investigators[&INV].clues, 1);
    assert_eq!(resolved.state().locations[&HERE].clues, 0);
}
