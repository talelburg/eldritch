//! Registry-backed tests for the legal-action enumerator (#393 slice 2a-ii):
//! the card actions (`PlayCard`, `ActivateAbility`, added in 2a-ii-3) plus the
//! whole-enumeration sweep covering every action category (2a-ii-4). These need
//! real card metadata/abilities, so they install `cards::REGISTRY` and live here
//! rather than in `game-core`'s registry-less unit tests.

use cards::REGISTRY;
use game_core::action::{Action, InputResponse, PlayerAction};
use game_core::engine::enumerate::{self, TurnAction};
use game_core::engine::{self, EngineOutcome, OptionId};
use game_core::state::{
    AbilityAddress, AbilitySource, Act, Agenda, CardCode, CardInPlay, CardInstanceId, ChaosBag,
    ChaosToken, EnemyId, GameState, GameStateBuilder, Investigator, InvestigatorId, LocationId,
    Owner, UseKind,
};
use game_core::test_support;

const HOLY_ROSARY: &str = "01059"; // Mystic asset, cost 2, constant +1 willpower.
const FLASHLIGHT: &str = "01087"; // Asset with an activated ability (uses: Supplies).
const INV: InvestigatorId = InvestigatorId(1);
const LOC: LocationId = LocationId(10);

#[ctor::ctor(unsafe)]
fn install_real_registry() {
    test_support::install_registry_with_test_cards(REGISTRY);
}

/// Investigator 1 at the Study (`LOC`) with `hand` in hand and `in_play` in
/// play, 3 actions, 9 resources.
fn investigator(hand: &[&str], in_play: Vec<CardInPlay>) -> Investigator {
    let mut inv = test_support::test_investigator(1);
    inv.current_location = Some(LOC);
    inv.actions_remaining = 3;
    inv.resources = 9;
    inv.hand = hand.iter().map(|c| CardCode::new(*c)).collect();
    inv.cards_in_play = in_play;
    inv
}

#[test]
fn play_card_offered_for_a_playable_hand_card() {
    let state = GameStateBuilder::new()
        .with_investigator(investigator(&[HOLY_ROSARY], Vec::new()))
        .with_location(test_support::test_location(LOC.0, "Study"))
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .open_turn(INV)
        .build();
    assert!(
        enumerate::legal_actions(&state).contains(&TurnAction::PlayCard {
            investigator: INV,
            hand_index: 0,
        })
    );
}

/// Flashlight in play with 3 Supplies uses, ready — its `ability_index: 0`
/// activated ability is usable.
fn flashlight_in_play(instance: CardInstanceId) -> CardInPlay {
    let mut torch = CardInPlay::enter_play(
        CardCode::new(FLASHLIGHT),
        instance,
        Owner::Investigator(InvestigatorId(1)),
    );
    torch.uses.insert(UseKind::Supplies, 3);
    torch
}

#[test]
fn activate_offered_for_an_in_play_activated_ability() {
    let inst = CardInstanceId(0);
    let state = GameStateBuilder::new()
        .with_investigator(investigator(&[], vec![flashlight_in_play(inst)]))
        .with_location(test_support::test_location(LOC.0, "Study"))
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .open_turn(INV)
        .build();
    assert!(
        enumerate::legal_actions(&state).contains(&TurnAction::ActivateAbility {
            investigator: INV,
            source: AbilitySource::InPlay(inst),
            address: AbilityAddress::Printed(0),
        })
    );
}

/// Investigator 1 at the Study with Holy Rosary in hand and a Flashlight in
/// play, at an open turn: the fixture of the registry-edition sweep.
fn rosary_and_flashlight_board() -> GameState {
    GameStateBuilder::new()
        .with_investigator(investigator(
            &[HOLY_ROSARY],
            vec![flashlight_in_play(CardInstanceId(0))],
        ))
        .with_location(test_support::test_location(LOC.0, "Study"))
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .open_turn(INV)
        .build()
}

#[test]
fn every_enumerated_action_applies_without_rejection_with_registry() {
    // Cross-check, registry edition: with real card data the enumeration
    // includes PlayCard (Holy Rosary) and ActivateAbility (Flashlight) alongside
    // the basic actions; each applies without Rejected (Done or AwaitingInput
    // are both acceptance).
    let state = rosary_and_flashlight_board();
    // OptionId round-trip: each enumerated action dispatches via
    // `ResolveInput(PickSingle(OptionId))` at the open turn (#447). None reject.
    let actions = enumerate::legal_actions(&state);
    for (i, action) in actions.iter().enumerate() {
        let result = engine::apply(
            state.clone(),
            Action::Player(PlayerAction::ResolveInput {
                response: InputResponse::PickSingle(OptionId(
                    u32::try_from(i).expect("action index fits u32"),
                )),
            }),
        );
        assert!(
            !matches!(result.outcome, EngineOutcome::Rejected { .. }),
            "enumerated action {action:?} (OptionId {i}) was rejected: {:?}",
            result.outcome,
        );
    }
}

/// A board offering every action category: the registry-edition fixture plus a
/// connected destination (Move), an engaged enemy (Fight/Evade), a co-located
/// unengaged enemy (Engage), and an advanceable act (`AdvanceAct`).
fn every_category_board() -> GameState {
    let inst = CardInstanceId(0);
    let mut state = GameStateBuilder::new()
        .with_investigator(investigator(&[HOLY_ROSARY], vec![flashlight_in_play(inst)]))
        .with_location(test_support::test_location(LOC.0, "Study"))
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .open_turn(INV)
        .build();
    let mut other = test_support::test_location(11, "Hall");
    other.revealed = true;
    state
        .locations
        .get_mut(&LOC)
        .unwrap()
        .connections
        .push(other.id);
    let other_id = other.id;
    state.locations.insert(other_id, other);

    let mut foe = test_support::test_enemy(7, "Ghoul");
    foe.engaged_with = Some(INV);
    foe.current_location = Some(LOC);
    state.enemies.insert(EnemyId(7), foe);
    let mut rat = test_support::test_enemy(8, "Rat");
    rat.current_location = Some(LOC);
    state.enemies.insert(EnemyId(8), rat);

    state.investigators.get_mut(&INV).unwrap().clues = 2;
    state.act_deck = vec![
        Act {
            code: CardCode("_act1".into()),
            clue_threshold: 2,
        },
        Act {
            code: CardCode("_act2".into()),
            clue_threshold: 99,
        },
    ];
    state
}

#[test]
fn full_enumeration_covers_every_action_category_and_all_apply() {
    let state = every_category_board();
    let actions = enumerate::legal_actions(&state);

    // Every category is represented.
    let has = |p: fn(&TurnAction) -> bool| actions.iter().any(p);
    assert!(actions.contains(&TurnAction::EndTurn), "EndTurn");
    assert!(has(|a| matches!(a, TurnAction::Move { .. })), "Move");
    assert!(
        has(|a| matches!(a, TurnAction::Investigate { .. })),
        "Investigate"
    );
    assert!(
        has(|a| matches!(a, TurnAction::Resource { .. })),
        "Resource"
    );
    assert!(has(|a| matches!(a, TurnAction::Draw { .. })), "Draw");
    assert!(has(|a| matches!(a, TurnAction::Fight { .. })), "Fight");
    assert!(has(|a| matches!(a, TurnAction::Evade { .. })), "Evade");
    assert!(has(|a| matches!(a, TurnAction::Engage { .. })), "Engage");
    assert!(
        has(|a| matches!(a, TurnAction::PlayCard { .. })),
        "PlayCard"
    );
    assert!(
        has(|a| matches!(a, TurnAction::ActivateAbility { .. })),
        "ActivateAbility"
    );
    assert!(
        has(|a| matches!(a, TurnAction::AdvanceAct { .. })),
        "AdvanceAct"
    );

    // And all of them apply without Rejected — via the OptionId round-trip
    // (`ResolveInput(PickSingle(OptionId))` at the open turn, #447).
    for (i, action) in actions.iter().enumerate() {
        let result = engine::apply(
            state.clone(),
            Action::Player(PlayerAction::ResolveInput {
                response: InputResponse::PickSingle(OptionId(
                    u32::try_from(i).expect("action index fits u32"),
                )),
            }),
        );
        assert!(
            !matches!(result.outcome, EngineOutcome::Rejected { .. }),
            "enumerated action {action:?} (OptionId {i}) was rejected: {:?}",
            result.outcome,
        );
    }
}

/// The Gathering's real act 1 and agenda 1, current on the board.
const TRAPPED: &str = "01108";
const WHATS_GOING_ON: &str = "01105";

/// The act/agenda bullet's real-corpus half (#709): widening which sources an
/// investigator can reach must not turn a **Forced** ability into an
/// activatable one.
///
/// Every act and agenda ability in the corpus is a `Trigger::OnEvent` reverse —
/// Trapped 01108's board build and What's Going On?! 01105's *"The lead
/// investigator must decide (choose one)…"* both fire from the advance
/// procedure, not from a player initiating them. So the current act and agenda
/// are now **reachable**, and the enumerator still offers nothing on them: the
/// filter that stops them is the trigger kind, which is what the rejection
/// reason has to say. A reason naming reachability instead would mean the
/// bullet had not landed at all, and the assertion would pass for the wrong
/// reason.
#[test]
fn the_corpus_act_and_agenda_are_reachable_but_offer_no_activation() {
    let mut state = GameStateBuilder::new()
        .with_investigator(investigator(&[], Vec::new()))
        .with_location(test_support::test_location(LOC.0, "Study"))
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .open_turn(INV)
        .build();
    state.act_deck = vec![Act {
        code: CardCode::new(TRAPPED),
        clue_threshold: 2,
    }];
    state.agenda_deck = vec![Agenda {
        code: CardCode::new(WHATS_GOING_ON),
        doom_threshold: 3,
    }];

    let menu = enumerate::legal_actions(&state);
    for source in [AbilitySource::Act, AbilitySource::Agenda] {
        assert!(
            !menu.iter().any(|a| matches!(
                a,
                TurnAction::ActivateAbility { source: s, .. } if *s == source
            )),
            "{source:?}'s only ability is Forced, so nothing on it belongs in the turn menu; \
             menu was {menu:?}",
        );

        let result = test_support::dispatch_turn_action_unchecked(
            state.clone(),
            &TurnAction::ActivateAbility {
                investigator: INV,
                source,
                address: AbilityAddress::Printed(0),
            },
        );
        let EngineOutcome::Rejected { reason } = &result.outcome else {
            panic!("a Forced ability must not be activatable; got {result:?}");
        };
        assert!(
            reason.contains("not an Activated trigger"),
            "the refusal must be about the trigger, not about reachability — the source is \
             reachable now. Got: {reason}",
        );
    }
}

// ---- Turn-menu completeness (#997) -----------------------------------

/// Every basic action the investigator could name on `state`: the three
/// targetless ones, a Move to every location in state, and a Fight, Evade and
/// Engage against every enemy in state. A superset of what is legal, so that
/// the engine, not this list, decides which are accepted.
fn every_basic_action(state: &GameState, me: InvestigatorId) -> Vec<TurnAction> {
    let mut all = vec![
        TurnAction::Resource { investigator: me },
        TurnAction::Draw { investigator: me },
        TurnAction::Investigate { investigator: me },
    ];
    all.extend(state.locations.keys().map(|&destination| TurnAction::Move {
        investigator: me,
        destination,
    }));
    for &enemy in state.enemies.keys() {
        all.extend([
            TurnAction::Fight {
                investigator: me,
                enemy,
            },
            TurnAction::Evade {
                investigator: me,
                enemy,
            },
            TurnAction::Engage {
                investigator: me,
                enemy,
            },
        ]);
    }
    all
}

/// The reverse of the "every enumerated action applies" sweep above: every
/// basic action the engine accepts when submitted straight to its handler is
/// offered by the turn menu. Returns the accepted actions, so a caller can check
/// the fixture exercised what it meant to.
fn assert_menu_offers_every_accepted_basic_action(
    state: &GameState,
    me: InvestigatorId,
) -> Vec<TurnAction> {
    let menu = enumerate::legal_actions(state);
    let mut accepted = Vec::new();
    for action in every_basic_action(state, me) {
        let result = test_support::dispatch_turn_action_unchecked(state.clone(), &action);
        if matches!(result.outcome, EngineOutcome::Rejected { .. }) {
            continue;
        }
        assert!(
            menu.contains(&action),
            "{action:?} is accepted when submitted directly but is not offered; menu was {menu:?}",
        );
        accepted.push(action);
    }
    accepted
}

/// A crowded board for the completeness sweep. The investigator stands at the
/// revealed Study (`LOC`), connected to a revealed Hall and an unrevealed Attic;
/// a Cellar is in play but not connected. A second investigator shares the
/// Study. Enemies:
/// - 7, engaged with me (Fight, Evade);
/// - 8, unengaged at the Study (Fight, Engage);
/// - 9, engaged with the other investigator at the Study (Fight, Engage);
/// - 10, at the Hall (nothing);
/// - 11, engaged with me with a malformed fight value (Evade only);
/// - 12, engaged with me with a malformed evade value (Fight only).
fn crowded_board() -> GameState {
    let other_inv = InvestigatorId(2);
    let mut second = test_support::test_investigator(2);
    second.current_location = Some(LOC);
    let mut study = test_support::test_location(LOC.0, "Study");
    study.revealed = true;
    let mut hall = test_support::test_location(11, "Hall");
    hall.revealed = true;
    let mut attic = test_support::test_location(12, "Attic");
    attic.revealed = false;
    let cellar = test_support::test_location(13, "Cellar");

    let mut rat = test_support::test_enemy(8, "Rat");
    rat.current_location = Some(LOC);
    let mut wanderer = test_support::test_enemy(10, "Wanderer");
    wanderer.current_location = Some(LocationId(11));
    let mut unhittable = test_support::test_enemy(11, "Unhittable");
    unhittable.fight = -1;
    let mut unshakeable = test_support::test_enemy(12, "Unshakeable");
    unshakeable.evade = -1;

    let mut state = GameStateBuilder::new()
        .with_investigator(investigator(&[], Vec::new()))
        .with_investigator(second)
        .with_location(study)
        .with_location(hall)
        .with_location(attic)
        .with_location(cellar)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_enemy(rat)
        .with_enemy(wanderer)
        .open_turn(INV)
        .with_enemy_engaged(test_support::test_enemy(7, "Ghoul"), INV)
        .with_enemy_engaged(test_support::test_enemy(9, "Stalker"), other_inv)
        .with_enemy_engaged(unhittable, INV)
        .with_enemy_engaged(unshakeable, INV)
        .build();
    state.connect(LOC, LocationId(11));
    state.connect(LOC, LocationId(12));
    state
}

/// Sort a list of turn actions into a stable order, for comparing sets.
fn sorted(mut actions: Vec<TurnAction>) -> Vec<TurnAction> {
    actions.sort_by_key(|a| format!("{a:?}"));
    actions
}

#[test]
fn the_menu_offers_every_basic_action_the_engine_accepts() {
    let accepted = assert_menu_offers_every_accepted_basic_action(&crowded_board(), INV);

    // The sweep is only as strong as its fixture, so pin what it accepted: the
    // targets each action's rules scope allows on this board.
    let fight = |e: u32| TurnAction::Fight {
        investigator: INV,
        enemy: EnemyId(e),
    };
    let evade = |e: u32| TurnAction::Evade {
        investigator: INV,
        enemy: EnemyId(e),
    };
    let engage = |e: u32| TurnAction::Engage {
        investigator: INV,
        enemy: EnemyId(e),
    };
    let move_to = |l: u32| TurnAction::Move {
        investigator: INV,
        destination: LocationId(l),
    };
    let expected = vec![
        TurnAction::Resource { investigator: INV },
        TurnAction::Draw { investigator: INV },
        TurnAction::Investigate { investigator: INV },
        move_to(11),
        move_to(12),
        fight(7),
        fight(8),
        fight(9),
        fight(12),
        evade(7),
        evade(11),
        engage(8),
        engage(9),
    ];
    assert_eq!(sorted(accepted), sorted(expected));
}

/// The completeness sweep under Frozen in Fear 01164, which surcharges *"one of
/// the following actions (move, fight, or evade)"*: with 1 action left the
/// surcharged kinds are neither accepted nor offered, and with 2 they are both.
/// The completeness sweep over the fixtures the "every enumerated action
/// applies" sweeps use, so the two directions are checked on the same boards.
#[test]
fn the_menu_offers_every_basic_action_the_engine_accepts_on_the_apply_sweep_boards() {
    for (name, state) in [
        ("rosary and flashlight", rosary_and_flashlight_board()),
        ("every category", every_category_board()),
    ] {
        let accepted = assert_menu_offers_every_accepted_basic_action(&state, INV);
        assert!(
            !accepted.is_empty(),
            "{name}: the engine accepted no basic action, so the sweep checked nothing",
        );
    }
}

#[test]
fn the_menu_offers_every_basic_action_the_engine_accepts_under_frozen_in_fear() {
    for (actions_remaining, surcharged_affordable) in [(1, false), (2, true)] {
        let mut state = crowded_board();
        let me = state.investigators.get_mut(&INV).expect("investigator 1");
        me.actions_remaining = actions_remaining;
        me.threat_area.push(CardInPlay::enter_play(
            CardCode::new("01164"),
            CardInstanceId(0),
            Owner::EncounterDeck,
        ));
        let accepted = assert_menu_offers_every_accepted_basic_action(&state, INV);
        let surcharged = accepted.iter().any(|a| {
            matches!(
                a,
                TurnAction::Move { .. } | TurnAction::Fight { .. } | TurnAction::Evade { .. }
            )
        });
        assert_eq!(
            surcharged, surcharged_affordable,
            "with {actions_remaining} action(s), a surcharged action was accepted: {accepted:?}",
        );
        assert!(
            accepted
                .iter()
                .any(|a| matches!(a, TurnAction::Engage { .. })),
            "Engage is never surcharged: {accepted:?}",
        );
    }
}
