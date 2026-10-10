//! Integration: The Gathering's three persistent treacheries (C4c, #235)
//! resolved through the real `cards` registry — they stay in play, enforce
//! their constant restriction, and discard at a forced timing point. Own
//! process so it can install the process-global registry against the real
//! corpus.

use cards::REGISTRY;
use game_core::action::{Action, EngineRecord};
use game_core::assert_event;
use game_core::engine::enumerate::{self, TurnAction};
use game_core::engine::modified_value::{self, ModifiedQuantity, ReadContext};
use game_core::engine::{ApplyResult, EngineOutcome, OptionId, OptionTarget, TimingEvent};
use game_core::event::Event;
use game_core::state::{
    AbilityAddress, AbilitySource, Agenda, CardCode, CardInPlay, CardInstanceId, ChaosBag,
    ChaosToken, Continuation, DiscardPile, EnemyId, GameState, GameStateBuilder, InvestigatorId,
    InvestigatorTurnFrame, Location, LocationId, ModifierTarget, Owner, Phase, SkillKind,
    TokenModifiers, UseKind, Zone,
};
use game_core::test_support::{self, ScriptedResolver, TestSession};

#[ctor::ctor(unsafe)]
fn install_registry() {
    test_support::install_registry_with_test_cards(REGISTRY);
}

/// Reveal the top encounter card for investigator 1, committing no cards
/// at any skill-test commit window that opens.
fn reveal_top(state: GameState) -> ApplyResult {
    let mut resolver = ScriptedResolver::new();
    resolver.commit_cards(&[]);
    test_support::drive(
        state,
        Action::Engine(EngineRecord::EncounterCardRevealed {
            investigator: InvestigatorId(1),
        }),
        resolver,
    )
}

/// One investigator at location 20 (printed shroud 2), with `treachery`
/// on top of the encounter deck.
fn board_with(treachery: &str) -> GameState {
    let mut state = GameStateBuilder::new()
        .with_investigator_at(test_support::test_investigator(1), LocationId(20))
        .with_location(test_support::test_location(20, "Here"))
        .with_turn_order([InvestigatorId(1)])
        .build();
    state.encounter_deck.push_back(CardCode::new(treachery));
    state
}

/// **A card revealed from the encounter deck is the encounter deck's**
/// (`glossary/Discard_Piles.md`: *"Encounter cards are owned by the encounter
/// deck."*), whichever zone its Revelation puts it in: Obscuring Fog 01168
/// attaches to a location, Frozen in Fear 01164 enters the threat area.
#[test]
fn a_revealed_persistent_treachery_is_owned_by_the_encounter_deck() {
    let fog = reveal_top(board_with("01168"));
    assert_eq!(fog.outcome, EngineOutcome::Done);
    assert_eq!(
        fog.state.locations[&LocationId(20)].attachments[0].owner,
        Owner::EncounterDeck,
    );

    let fear = reveal_top(board_with("01164"));
    assert_eq!(fear.outcome, EngineOutcome::Done);
    assert_eq!(
        fear.state.investigators[&InvestigatorId(1)].threat_area[0].owner,
        Owner::EncounterDeck,
    );
}

#[test]
fn obscuring_fog_attaches_raises_shroud_and_discards_on_investigate() {
    // Reveal: attaches to the investigator's location, not discarded.
    let result = reveal_top(board_with("01168"));
    assert_eq!(result.outcome, EngineOutcome::Done);
    let loc = &result.state.locations[&LocationId(20)];
    assert_eq!(loc.attachments.len(), 1, "Obscuring Fog attached");
    assert_eq!(loc.attachments[0].code.as_str(), "01168");
    assert!(
        !result
            .state
            .encounter_discard
            .contains(&CardCode::new("01168")),
        "a persistent treachery is not auto-discarded after its Revelation",
    );

    // +2 shroud: printed 2 → modified 4.
    assert_eq!(
        modified_value::modified_value(
            &result.state,
            Some(&REGISTRY),
            ModifierTarget::Location(loc.id),
            ModifiedQuantity::Shroud,
            ReadContext::from_state(&result.state),
        )
        .total(),
        4,
        "attached Obscuring Fog grants +2 shroud (printed 2)",
    );

    // Forced — after the attached location is successfully investigated,
    // discard Obscuring Fog. Drive a real (passing) Investigate so the
    // in-flight SkillTest frame is live when SkillTestResolved fires: the
    // trigger scan reads `tested_location` off that frame to match Obscuring
    // Fog's *"attached location"* scope (the lean, location-free timing event
    // derives the location from the stack rather than carrying it).
    let mut loc = test_support::test_location(20, "Here");
    loc.shroud = 0; // effective 0 + 2 (Obscuring Fog) = 2; intellect 3 clears it
    loc.clues = 1;
    loc.attachments.push(CardInPlay::enter_play(
        CardCode::new("01168"),
        CardInstanceId(1),
        Owner::EncounterDeck,
    ));
    let mut inv = test_support::test_investigator(1);
    inv.current_location = Some(LocationId(20));
    let state = GameStateBuilder::new()
        .open_turn(InvestigatorId(1))
        .with_investigator(inv)
        .with_location(loc)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .build();

    let result = TestSession::new(state)
        .resolve_choices(|c| {
            c.commit_cards(&[]);
        })
        .take(&TurnAction::Investigate {
            investigator: InvestigatorId(1),
        })
        .finish();
    assert!(matches!(
        result.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
    assert!(
        result.state.locations[&LocationId(20)]
            .attachments
            .is_empty(),
        "Obscuring Fog discards after its location is successfully investigated",
    );
    // Through the leave-play exit, filed by its owner: the encounter deck.
    assert_eq!(result.state.encounter_discard, vec![CardCode::new("01168")]);
    assert!(result.state.investigators[&InvestigatorId(1)]
        .discard
        .is_empty());
    assert_event!(
        result.events,
        Event::CardDiscarded {
            code,
            from: Zone::LocationAttachment,
            to: DiscardPile::Encounter,
        } if code.as_str() == "01168"
    );
}

// ---- Dissonant Voices (01165) --------------------------------------

#[test]
fn dissonant_voices_enters_threat_area_and_discards_on_round_end() {
    let result = reveal_top(board_with("01165"));
    assert_eq!(result.outcome, EngineOutcome::Done);
    let inv = &result.state.investigators[&InvestigatorId(1)];
    assert_eq!(inv.threat_area.len(), 1, "Dissonant Voices in threat area");
    assert_eq!(inv.threat_area[0].code.as_str(), "01165");
    assert!(!result
        .state
        .encounter_discard
        .contains(&CardCode::new("01165")));

    // Forced — at the end of the round, discard Dissonant Voices.
    let ApplyResult { state, outcome, .. } = TestSession::new(result.state)
        .fire_at(TimingEvent::RoundEnded)
        .finish();
    assert_eq!(outcome, EngineOutcome::Done);
    assert!(
        state.investigators[&InvestigatorId(1)]
            .threat_area
            .is_empty(),
        "Dissonant Voices discards at end of round",
    );
    assert!(state.encounter_discard.contains(&CardCode::new("01165")));
}

#[test]
fn dissonant_voices_forbids_playing_an_asset() {
    // Investigator mid-investigation with a playable asset (Holy Rosary,
    // 01059) in hand and Dissonant Voices in their threat area.
    let mut inv = test_support::test_investigator(1);
    inv.current_location = Some(LocationId(101));
    inv.hand = vec![CardCode::new("01059")];
    inv.threat_area.push(CardInPlay::enter_play(
        CardCode::new("01165"),
        CardInstanceId(0),
        Owner::EncounterDeck,
    ));
    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator(inv)
        .with_active_investigator(InvestigatorId(1))
        .with_location(test_support::test_location(101, "Study"))
        .build();

    let result = test_support::dispatch_turn_action_unchecked(
        state,
        &TurnAction::PlayCard {
            investigator: InvestigatorId(1),
            hand_index: 0,
        },
    );
    assert!(
        matches!(result.outcome, EngineOutcome::Rejected { .. }),
        "Dissonant Voices forbids playing assets; got {:?}",
        result.outcome,
    );
    // Validate-first: the asset stays in hand, nothing entered play.
    let inv = &result.state.investigators[&InvestigatorId(1)];
    assert_eq!(inv.hand, vec![CardCode::new("01059")]);
    assert!(inv.cards_in_play.is_empty());
}

#[test]
fn dissonant_voices_round_end_coexists_with_agenda_01107_doom() {
    // Both Dissonant Voices (threat area) and agenda 01107 carry an
    // `At`-`RoundEnded` forced ability. Two simultaneous forced at one timing
    // point let the lead order them (#213), so the real round-end coordinator
    // path opens the ordered forced-run: the agenda places doom per ghoul in the
    // Hallway/Parlor and Dissonant Voices discards itself, both resolving in the
    // lead's chosen order rather than rejecting. Driven through the end of the
    // Upkeep phase, the production route into the round end.
    let loc = |id, code: &str, name| Location::new(LocationId(id), CardCode::new(code), name, 1, 0);
    let mut inv = test_support::test_investigator(1);
    inv.threat_area.push(CardInPlay::enter_play(
        CardCode::new("01165"),
        CardInstanceId(0),
        Owner::EncounterDeck,
    ));
    let mut state = GameStateBuilder::new()
        .ending_upkeep_phase()
        .with_investigator(inv)
        .with_turn_order([InvestigatorId(1)])
        .with_location(loc(2, "01112", "Hallway"))
        .with_location(loc(5, "01115", "Parlor"))
        .build();
    let mut ghoul = test_support::test_enemy(1, "Ghoul");
    ghoul.traits = vec!["Monster".into(), "Ghoul".into()];
    ghoul.current_location = Some(LocationId(2)); // Hallway
    state.enemies.insert(EnemyId(1), ghoul);
    state.agenda_deck = vec![Agenda {
        code: CardCode::new("01107"),
        doom_threshold: 10,
    }];
    state.agenda_index = 0;

    // Settling ends the phase into the round end: 2+ At-forced → the lead
    // orders them.
    let opened = TestSession::new(state);
    let dissonant_voices = OptionTarget::CardInstance(CardInstanceId(0));
    let offered: Vec<_> = opened
        .prompt()
        .options
        .iter()
        .map(|o| o.target.clone())
        .collect();
    assert_eq!(
        offered,
        vec![Some(dissonant_voices.clone()), Some(OptionTarget::Agenda)],
        "two simultaneous RoundEnded forced present the lead an ordering choice",
    );
    // Resolve the forced run in the lead's chosen order: the agenda first, then
    // Dissonant Voices — both fire, neither rejects.
    let after_first = opened.pick(OptionTarget::Agenda);
    assert_eq!(
        after_first.prompt().options.len(),
        1,
        "the second forced is still pending after the first resolves",
    );
    let done = after_first.pick(dissonant_voices);
    // Both forced resolved and the round ended cleanly: play advanced into the
    // next Mythos phase, not a rejection.
    assert_eq!(done.state().phase, Phase::Mythos);
    assert!(
        done.state().agenda_doom >= 1,
        "agenda 01107 placed doom for the Ghoul in the Hallway; agenda_doom = {}",
        done.state().agenda_doom,
    );
    assert!(
        done.state().investigators[&InvestigatorId(1)]
            .threat_area
            .is_empty(),
        "Dissonant Voices also discarded in the same round-end resolution",
    );
}

// ---- Frozen in Fear (01164) ----------------------------------------

#[test]
fn frozen_in_fear_surcharges_first_move_each_round_only() {
    let mut inv = test_support::test_investigator(1);
    inv.current_location = Some(LocationId(1));
    inv.threat_area.push(CardInPlay::enter_play(
        CardCode::new("01164"),
        CardInstanceId(0),
        Owner::EncounterDeck,
    ));
    let mut state = GameStateBuilder::new()
        .with_investigator(inv)
        .with_location(test_support::test_location(1, "A"))
        .with_location(test_support::test_location(2, "B"))
        .open_turn(InvestigatorId(1))
        .build();
    state.connect(LocationId(1), LocationId(2));
    assert_eq!(state.investigators[&InvestigatorId(1)].actions_remaining, 3);

    // First move this round costs 2 (base 1 + surcharge 1): 3 → 1.
    let r = test_support::take_turn_action(
        state,
        &TurnAction::Move {
            investigator: InvestigatorId(1),
            destination: LocationId(2),
        },
    );
    assert!(matches!(r.outcome, EngineOutcome::AwaitingInput { .. }));
    assert_eq!(
        r.state.investigators[&InvestigatorId(1)].actions_remaining,
        1,
        "first move/fight/evade each round costs +1 action",
    );

    // Second move this round costs 1 (surcharge already spent): 1 → 0.
    let r = test_support::take_turn_action(
        r.state,
        &TurnAction::Move {
            investigator: InvestigatorId(1),
            destination: LocationId(1),
        },
    );
    assert!(matches!(r.outcome, EngineOutcome::AwaitingInput { .. }));
    assert_eq!(
        r.state.investigators[&InvestigatorId(1)].actions_remaining,
        0,
        "subsequent actions that round cost the normal 1",
    );
}

/// The turn menu prices each basic action the way taking it does. Frozen in
/// Fear 01164 surcharges only *"one of the following actions (move, fight, or
/// evade)"*, so with a single action left the menu drops Move, Fight and Evade
/// (each costs 2) and keeps Investigate, Resource, Draw and Engage (each costs
/// 1). Once a surcharged move has spent the surcharge for the round, a move at
/// 1 action is offered again.
#[test]
fn the_action_menu_offers_only_what_the_frozen_in_fear_surcharge_leaves_affordable() {
    let me = InvestigatorId(1);
    let mut inv = test_support::test_investigator(1);
    inv.current_location = Some(LocationId(1));
    inv.deck = vec![CardCode::new("01019")];
    inv.threat_area.push(CardInPlay::enter_play(
        CardCode::new("01164"),
        CardInstanceId(0),
        Owner::EncounterDeck,
    ));
    let mut bystander = test_support::test_enemy(8, "Bystander");
    bystander.current_location = Some(LocationId(1));
    let mut state = GameStateBuilder::new()
        .with_investigator(inv)
        .with_location(test_support::test_location(1, "A"))
        .with_location(test_support::test_location(2, "B"))
        .with_enemy(bystander)
        .open_turn(me)
        .with_enemy_engaged(test_support::test_enemy(7, "Engaged"), me)
        .build();
    state.connect(LocationId(1), LocationId(2));
    let move_to_b = TurnAction::Move {
        investigator: me,
        destination: LocationId(2),
    };
    let fight = TurnAction::Fight {
        investigator: me,
        enemy: EnemyId(7),
    };
    let evade = TurnAction::Evade {
        investigator: me,
        enemy: EnemyId(7),
    };
    let never_surcharged = [
        TurnAction::Investigate { investigator: me },
        TurnAction::Resource { investigator: me },
        TurnAction::Draw { investigator: me },
        TurnAction::Engage {
            investigator: me,
            enemy: EnemyId(8),
        },
    ];

    state
        .investigators
        .get_mut(&me)
        .expect("investigator 1")
        .actions_remaining = 1;
    let menu = enumerate::legal_actions(&state);
    for surcharged in [&move_to_b, &fight, &evade] {
        assert!(
            !menu.contains(surcharged),
            "{surcharged:?} costs 2 under Frozen in Fear, so 1 action can't pay for it",
        );
    }
    for plain in &never_surcharged {
        assert!(menu.contains(plain), "{plain:?} is never surcharged");
    }

    state
        .investigators
        .get_mut(&me)
        .expect("investigator 1")
        .actions_remaining = 2;
    assert!(
        enumerate::legal_actions(&state).contains(&move_to_b),
        "2 actions pay for the surcharged move",
    );

    // The surcharged move spends 2 of 3 actions and the surcharge for the round.
    state
        .investigators
        .get_mut(&me)
        .expect("investigator 1")
        .actions_remaining = 3;
    let r = test_support::take_turn_action(state, &move_to_b);
    assert_eq!(r.state.investigators[&me].actions_remaining, 1);
    assert!(
        enumerate::legal_actions(&r.state).contains(&TurnAction::Move {
            investigator: me,
            destination: LocationId(1),
        }),
        "with the surcharge spent this round, a move costs 1",
    );
}

/// Build a two-investigator Investigation-phase board with Frozen in Fear
/// in investigator 1's threat area and a single rigged chaos token.
fn frozen_in_fear_board(token: ChaosToken) -> GameState {
    let mut inv1 = test_support::test_investigator(1);
    inv1.threat_area.push(CardInPlay::enter_play(
        CardCode::new("01164"),
        CardInstanceId(0),
        Owner::EncounterDeck,
    ));
    let mut state = GameStateBuilder::new()
        .with_investigator(inv1)
        .with_investigator(test_support::test_investigator(2))
        .with_turn_order([InvestigatorId(1), InvestigatorId(2)])
        .open_turn(InvestigatorId(1))
        .build();
    state.chaos_bag.tokens = vec![token];
    state
}

fn end_turn_committing_nothing(state: GameState) -> ApplyResult {
    TestSession::new(state)
        .resolve_choices(|c| {
            c.commit_cards(&[]);
        })
        .take(&TurnAction::EndTurn)
        .finish()
}

#[test]
fn frozen_in_fear_end_of_turn_success_discards_and_turn_resumes() {
    // Willpower 3 + Numeric(0) = 3 vs difficulty 3 → success.
    let r = end_turn_committing_nothing(frozen_in_fear_board(ChaosToken::Numeric(0)));
    assert!(matches!(r.outcome, EngineOutcome::AwaitingInput { .. }));
    assert!(
        r.state.investigators[&InvestigatorId(1)]
            .threat_area
            .is_empty(),
        "succeeded willpower(3) test discards Frozen in Fear",
    );
    assert!(r.state.encounter_discard.contains(&CardCode::new("01164")));
    // The suspending end-of-turn test did not strand the turn: rotation ran.
    assert_eq!(
        r.state.active_investigator,
        Some(InvestigatorId(2)),
        "end_turn resumed after the test and rotated to investigator 2",
    );
    // The turn was not left stranded: no InvestigatorTurn frame is mid-end
    // (slice 2a-i absorbed the former pending_end_turn into `ending`).
    assert!(!r.state.continuations.iter().any(|c| matches!(
        c,
        Continuation::InvestigatorTurn(InvestigatorTurnFrame { ending: true, .. })
    )));
}

#[test]
fn frozen_in_fear_end_of_turn_failure_keeps_card_but_turn_still_resumes() {
    // Willpower 3 + Numeric(-1) = 2 vs difficulty 3 → fail.
    let r = end_turn_committing_nothing(frozen_in_fear_board(ChaosToken::Numeric(-1)));
    assert!(matches!(r.outcome, EngineOutcome::AwaitingInput { .. }));
    assert_eq!(
        r.state.investigators[&InvestigatorId(1)].threat_area.len(),
        1,
        "failed test leaves Frozen in Fear in the threat area",
    );
    assert!(!r.state.encounter_discard.contains(&CardCode::new("01164")));
    // Turn still progresses regardless of the test outcome.
    assert_eq!(r.state.active_investigator, Some(InvestigatorId(2)));
    // The turn was not left stranded: no InvestigatorTurn frame is mid-end
    // (slice 2a-i absorbed the former pending_end_turn into `ending`).
    assert!(!r.state.continuations.iter().any(|c| matches!(
        c,
        Continuation::InvestigatorTurn(InvestigatorTurnFrame { ending: true, .. })
    )));
}

#[test]
fn two_frozen_in_fear_end_of_turn_tests_both_resolve_then_turn_resumes() {
    // #213 reentrancy: two Frozen in Fear copies on one investigator fire two
    // simultaneous `EndOfTurn` forced abilities, each a *suspending* willpower
    // test. The lead orders them; firing the first suspends on its commit
    // window, and once it resolves the forced run resumes the second sibling —
    // rather than abandoning it. After both resolve, the end-of-turn tail runs
    // (rotation to the next investigator).

    let mut inv1 = test_support::test_investigator(1);
    inv1.threat_area.push(CardInPlay::enter_play(
        CardCode::new("01164"),
        CardInstanceId(0),
        Owner::EncounterDeck,
    ));
    inv1.threat_area.push(CardInPlay::enter_play(
        CardCode::new("01164"),
        CardInstanceId(1),
        Owner::EncounterDeck,
    ));
    let mut state = GameStateBuilder::new()
        .with_investigator(inv1)
        .with_investigator(test_support::test_investigator(2))
        .with_turn_order([InvestigatorId(1), InvestigatorId(2)])
        .open_turn(InvestigatorId(1))
        .build();
    // Two Numeric(0) draws → willpower 3 vs difficulty 3 → both succeed.
    state.chaos_bag.tokens = vec![ChaosToken::Numeric(0), ChaosToken::Numeric(0)];

    // Order the first forced, commit nothing to its test; order the second,
    // commit nothing to its test.
    let r = TestSession::new(state)
        .resolve_choices(|c| {
            c.pick_single(OptionId(0))
                .commit_cards(&[])
                .pick_single(OptionId(0))
                .commit_cards(&[]);
        })
        .take(&TurnAction::EndTurn)
        .finish();

    assert!(matches!(r.outcome, EngineOutcome::AwaitingInput { .. }));
    assert!(
        r.state.investigators[&InvestigatorId(1)]
            .threat_area
            .is_empty(),
        "both succeeded willpower tests discard both Frozen in Fear copies",
    );
    assert_eq!(
        r.state
            .encounter_discard
            .iter()
            .filter(|c| **c == CardCode::new("01164"))
            .count(),
        2,
        "both copies land in the encounter discard",
    );
    // Neither sibling was abandoned, and the end-of-turn tail still ran:
    assert_eq!(
        r.state.active_investigator,
        Some(InvestigatorId(2)),
        "end_turn resumed after both tests and rotated to investigator 2",
    );
    // The turn was not left stranded: no InvestigatorTurn frame is mid-end
    // (slice 2a-i absorbed the former pending_end_turn into `ending`).
    assert!(!r.state.continuations.iter().any(|c| matches!(
        c,
        Continuation::InvestigatorTurn(InvestigatorTurnFrame { ending: true, .. })
    )));
}

#[test]
fn obscuring_fog_limit_one_per_location_discards_the_second_copy() {
    // First copy attaches.
    let result = reveal_top(board_with("01168"));
    let mut state = result.state;
    assert_eq!(state.locations[&LocationId(20)].attachments.len(), 1);

    // Second copy revealed at the same location: limit 1 → discarded, not
    // attached.
    state.encounter_deck.push_back(CardCode::new("01168"));
    let result = reveal_top(state);
    assert_eq!(result.outcome, EngineOutcome::Done);
    assert_eq!(
        result.state.locations[&LocationId(20)].attachments.len(),
        1,
        "limit 1 per location: the second copy does not attach",
    );
    assert!(
        result
            .state
            .encounter_discard
            .contains(&CardCode::new("01168")),
        "the over-limit copy is discarded",
    );
    // An encounter card's discard names the encounter pile, not a stand-in
    // investigator.
    assert_event!(
        result.events,
        Event::CardDiscarded {
            code,
            from: Zone::LocationAttachment,
            to: DiscardPile::Encounter,
        } if code.as_str() == "01168"
    );
}

// ---- Frozen in Fear (01164) × a bold action designator (#754) -------

/// Board: investigator 1 at location 20, Frozen in Fear 01164 in the threat
/// area, a .45 Automatic 01016 (4 ammo) in play, and a co-located enemy to
/// shoot. Rigged `Numeric(0)` bag so the attack resolves deterministically.
///
/// The weapon's `[action] Spend 1 ammo: **Fight**.` prints a bold **Fight**
/// designator, so per the Official FAQ it *counts as a Fight action* — and
/// Frozen in Fear surcharges *"the first time you perform one of the following
/// actions (move, fight, or evade) each round"*, one budget across the three.
fn frozen_in_fear_with_weapon_board() -> GameState {
    let mut inv = test_support::test_investigator(1);
    inv.threat_area.push(CardInPlay::enter_play(
        CardCode::new("01164"),
        CardInstanceId(0),
        Owner::EncounterDeck,
    ));
    let mut weapon = CardInPlay::enter_play(
        CardCode::new("01016"),
        CardInstanceId(1),
        Owner::Investigator(InvestigatorId(1)),
    );
    weapon.uses.insert(UseKind::Ammo, 4);
    inv.cards_in_play.push(weapon);

    let mut enemy = test_support::test_enemy(100, "Ghoul");
    enemy.fight = 3;
    enemy.max_health = 9;
    enemy.engaged_with = Some(InvestigatorId(1));
    enemy.current_location = Some(LocationId(20));

    GameStateBuilder::new()
        .with_investigator_at(inv, LocationId(20))
        .with_location(test_support::test_location(20, "Here"))
        .with_enemy(enemy)
        .open_turn(InvestigatorId(1))
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .build()
}

/// Fire the .45 Automatic, committing nothing to the attack's skill test.
fn fire_weapon(state: GameState) -> ApplyResult {
    TestSession::new(state)
        .resolve_choices(|c| {
            c.commit_cards(&[]);
        })
        .take(&TurnAction::ActivateAbility {
            investigator: InvestigatorId(1),
            source: AbilitySource::InPlay(CardInstanceId(1)),
            address: AbilityAddress::Printed(0),
        })
        .finish()
}

/// Official FAQ, `Frequently_Asked_Questions.md`:
///
/// > Abilities with a bold action designator (like Fight, Evade or
/// > Investigate) **count as an action of that type**.
///
/// and Frozen in Fear's own ruling (<https://arkhamdb.com/card/01164>):
///
/// > Also applies to \[action\] card abilities with action designators
/// > (**Move**, **Fight**, **Evade**).
///
/// So shooting the .45 Automatic *is* a Fight action, and the first one each
/// round costs 2 — the same as punching. Before #754 it cost 1, making the
/// weapon a way to duck the treachery entirely.
#[test]
fn frozen_in_fear_surcharges_a_fight_designated_ability() {
    let r = fire_weapon(frozen_in_fear_with_weapon_board());
    assert!(matches!(r.outcome, EngineOutcome::AwaitingInput { .. }));
    assert_eq!(
        r.state.investigators[&InvestigatorId(1)].actions_remaining,
        1,
        "first Fight each round costs base 1 + surcharge 1, whether basic or designated",
    );
}

/// The surcharge's `first_each_round` budget is one *shared* budget: a
/// designated Fight consumes it, so the basic Fight that follows pays the
/// plain 1. Before #754 the activation path never marked the source spent,
/// so the treachery could surcharge twice in a round.
#[test]
fn a_designated_fight_consumes_the_once_each_round_surcharge() {
    let r = fire_weapon(frozen_in_fear_with_weapon_board());
    assert_eq!(
        r.state.investigators[&InvestigatorId(1)].actions_remaining,
        1,
    );
    assert!(
        r.state.investigators[&InvestigatorId(1)]
            .action_surcharge_spent_this_round
            .contains(&CardInstanceId(0)),
        "the designated Fight marks Frozen in Fear's once-each-round surcharge spent",
    );

    // The follow-up basic Fight pays the plain 1: 1 → 0.
    let r = TestSession::new(r.state)
        .resolve_choices(|c| {
            c.commit_cards(&[]);
        })
        .take(&TurnAction::Fight {
            investigator: InvestigatorId(1),
            enemy: EnemyId(100),
        })
        .finish();
    assert_eq!(
        r.state.investigators[&InvestigatorId(1)].actions_remaining,
        0,
        "surcharge already spent this round, so the basic Fight costs the normal 1",
    );
}

/// The action menu and the validator agree about the surcharged cost. With a
/// single action left the .45 Automatic's Fight costs 2, so it must not be
/// offered — an enumerator reading the printed cost would list an activation
/// the handler then rejects.
///
/// `legal_actions` gets this for free by delegating to
/// `check_activate_ability`; the test pins that delegation, since the
/// alternative (a parallel affordability check in the enumerator) is exactly
/// how the basic-action and ability paths drifted apart in the first place.
#[test]
fn the_action_menu_hides_a_designated_fight_the_surcharge_makes_unaffordable() {
    let mut state = frozen_in_fear_with_weapon_board();
    let activation = TurnAction::ActivateAbility {
        investigator: InvestigatorId(1),
        source: AbilitySource::InPlay(CardInstanceId(1)),
        address: AbilityAddress::Printed(0),
    };
    assert!(
        enumerate::legal_actions(&state).contains(&activation),
        "with 3 actions the surcharged Fight (cost 2) is affordable",
    );

    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .expect("investigator 1")
        .actions_remaining = 1;
    assert!(
        !enumerate::legal_actions(&state).contains(&activation),
        "1 action left cannot pay the surcharged cost of 2, so the menu omits it",
    );
}

#[test]
fn dissonant_voices_keeps_a_fast_event_out_of_a_player_window() {
    // Same restriction, asked of a *fast window's option list* rather than of
    // the `PlayCard` handler: Working a Hunch 01037 ("Fast. Play only during
    // your turn. / Discover 1 clue at your location.") is fast-eligible on this
    // board, so an engine-opened ST.1 player window enumerates it — but
    // Dissonant Voices forbids playing events, so it must not be offered.
    let mut inv = test_support::test_investigator(1);
    inv.current_location = Some(LocationId(101));
    inv.resources = 5;
    inv.hand = vec![CardCode::new("01037")];
    inv.threat_area.push(CardInPlay::enter_play(
        CardCode::new("01165"),
        CardInstanceId(0),
        Owner::EncounterDeck,
    ));
    let mut loc = test_support::test_location(101, "Study");
    loc.clues = 2;
    let mut state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator(inv)
        .with_active_investigator(InvestigatorId(1))
        .with_location(loc)
        .with_turn_order([InvestigatorId(1)])
        .build();
    state.chaos_bag = ChaosBag::new([ChaosToken::Numeric(0)]);

    let result =
        test_support::perform_skill_test(state, InvestigatorId(1), SkillKind::Willpower, 4);
    if let EngineOutcome::AwaitingInput { ref request, .. } = result.outcome {
        assert!(
            request.options.is_empty(),
            "Dissonant Voices forbids events, so the window must not offer \
             Working a Hunch; got {:?}",
            request.options,
        );
    }
    assert!(
        result.state.open_windows().is_empty(),
        "with nothing eligible the window auto-skips; it stayed open: {:?}",
        result.state.open_windows(),
    );
}
