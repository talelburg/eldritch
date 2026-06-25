//! `Cost::DiscardSelf` + the Beat-Cop-shaped `DiscardSelf → DealDamageToEnemy`
//! ability. Mock registry in its own integration binary (own process +
//! `OnceLock<CardRegistry>`), mirroring `weapon_fight.rs`.

use std::sync::OnceLock;

use game_core::card_data::{CardKind, CardMetadata, Class, SkillIcons, Slot, UseKind, Uses};
use game_core::dsl::{
    activated, deal_damage_to_enemy, gain_resources, Ability, Cost, EnemyTarget, InvestigatorTarget,
};
use game_core::engine::EngineOutcome;
use game_core::event::Event;
use game_core::state::{
    CardCode, CardInPlay, CardInstanceId, EnemyId, InvestigatorId, LocationId, Phase,
};
use game_core::test_support::{
    apply_no_commits, dispatch_turn_action_unchecked, test_enemy, test_investigator, test_location,
    GameStateBuilder,
};
use game_core::{assert_event, Action, InputResponse, OptionId, PlayerAction, TurnAction};

const TRINKET: &str = "TRNK1";
const COP: &str = "MCOP1";
const COMBO: &str = "CMBO1";
const KIT: &str = "KIT01"; // Uses (1 supply), discard_when_empty: true
const KIT_NODISC: &str = "KIT02"; // Uses (1 supply), discard_when_empty: false

/// A 1-supply asset with the given depletion-discard flag, and a
/// `[fast] Spend 1 supply: gain 1 resource` ability.
fn kit_metadata(code: &str, name: &str, discard_when_empty: bool) -> CardMetadata {
    let mut m = asset_metadata(code, name, "[fast] Spend 1 supply: gain 1 resource.");
    if let CardKind::Asset { uses, .. } = &mut m.kind {
        *uses = Some(Uses {
            kind: UseKind::Supplies,
            count: 1,
            discard_when_empty,
        });
    }
    m
}

fn asset_metadata(code: &str, name: &str, text: &str) -> CardMetadata {
    CardMetadata {
        code: code.to_owned(),
        name: name.to_owned(),
        traits: vec!["Item".to_owned()],
        text: Some(text.to_owned()),
        pack_code: "_mock".to_owned(),
        kind: CardKind::Asset {
            class: Class::Neutral,
            cost: Some(0),
            xp: None,
            slots: vec![Slot::Hand],
            health: None,
            sanity: None,
            skill_icons: SkillIcons::default(),
            is_fast: false,
            deck_limit: 1,
            uses: None,
            play_only_during_turn: false,
        },
    }
}

fn trinket_static() -> &'static CardMetadata {
    static M: OnceLock<CardMetadata> = OnceLock::new();
    M.get_or_init(|| asset_metadata(TRINKET, "Mock Trinket", "[fast] Discard: gain 1 resource."))
}

fn cop_static() -> &'static CardMetadata {
    static M: OnceLock<CardMetadata> = OnceLock::new();
    M.get_or_init(|| {
        asset_metadata(
            COP,
            "Mock Cop",
            "[fast] Discard: deal 1 damage to an enemy at your location.",
        )
    })
}

fn combo_static() -> &'static CardMetadata {
    static M: OnceLock<CardMetadata> = OnceLock::new();
    M.get_or_init(|| asset_metadata(COMBO, "Mock Combo", "[fast] Exhaust, Discard: gain 1."))
}

fn kit_static() -> &'static CardMetadata {
    static M: OnceLock<CardMetadata> = OnceLock::new();
    M.get_or_init(|| kit_metadata(KIT, "Mock Kit", true))
}

fn kit_nodisc_static() -> &'static CardMetadata {
    static M: OnceLock<CardMetadata> = OnceLock::new();
    M.get_or_init(|| kit_metadata(KIT_NODISC, "Mock Kit (stays)", false))
}

fn mock_metadata_for(code: &CardCode) -> Option<&'static CardMetadata> {
    match code.as_str() {
        TRINKET => Some(trinket_static()),
        COP => Some(cop_static()),
        COMBO => Some(combo_static()),
        KIT => Some(kit_static()),
        KIT_NODISC => Some(kit_nodisc_static()),
        _ => None,
    }
}

fn mock_abilities_for(code: &CardCode) -> Option<Vec<Ability>> {
    match code.as_str() {
        // [fast] Discard Mock Trinket: gain 1 resource.
        TRINKET => Some(vec![activated(
            0,
            vec![Cost::DiscardSelf],
            gain_resources(InvestigatorTarget::You, 1),
        )]),
        // [fast] Discard Mock Cop: deal 1 damage to an enemy at your location.
        COP => Some(vec![activated(
            0,
            vec![Cost::DiscardSelf],
            deal_damage_to_enemy(EnemyTarget::chosen_at_your_location(), 1),
        )]),
        // Illegal: DiscardSelf cannot combine with another source cost (Exhaust).
        COMBO => Some(vec![activated(
            0,
            vec![Cost::DiscardSelf, Cost::Exhaust],
            gain_resources(InvestigatorTarget::You, 1),
        )]),
        // [fast] Spend 1 supply: gain 1 resource (both kits share the ability;
        // they differ only in the metadata discard_when_empty flag).
        KIT | KIT_NODISC => Some(vec![activated(
            0,
            vec![Cost::SpendUses {
                kind: UseKind::Supplies,
                count: 1,
            }],
            gain_resources(InvestigatorTarget::You, 1),
        )]),
        _ => None,
    }
}

fn install_mock_registry() {
    static INSTALL: OnceLock<()> = OnceLock::new();
    INSTALL.get_or_init(|| {
        let _ = game_core::card_registry::install(game_core::card_registry::CardRegistry {
            metadata_for: mock_metadata_for,
            abilities_for: mock_abilities_for,
            native_effect_for: |_| None,
        });
    });
}

#[test]
fn discard_self_removes_source_from_play_and_runs_the_effect() {
    install_mock_registry();
    let id = InvestigatorId(1);
    let inst = CardInstanceId(0);
    let mut inv = test_investigator(1);
    let before = inv.resources;
    inv.cards_in_play
        .push(CardInPlay::enter_play(CardCode::new(TRINKET), inst));
    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_active_investigator(id)
        .with_investigator_turn(id)
        .with_investigator(inv)
        .build();

    let idx = game_core::engine::enumerate::legal_actions(&state)
        .iter()
        .position(|a| {
            a == &TurnAction::ActivateAbility {
                investigator: id,
                instance_id: inst,
                ability_index: 0,
            }
        })
        .expect("ability must be legal");
    let result = apply_no_commits(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(OptionId(u32::try_from(idx).unwrap())),
        }),
    );

    assert_eq!(result.outcome, EngineOutcome::Done);
    let inv_after = &result.state.investigators[&id];
    assert!(inv_after.cards_in_play.is_empty(), "source asset left play");
    assert_eq!(inv_after.discard, vec![CardCode::new(TRINKET)]);
    assert_eq!(inv_after.resources, before + 1, "the effect still ran");
    assert_event!(
        result.events,
        Event::CardDiscarded {
            from: game_core::state::Zone::InPlay,
            ..
        }
    );
}

fn board_with_cop(enemy_at_loc: bool) -> (game_core::GameState, InvestigatorId, CardInstanceId) {
    install_mock_registry();
    let id = InvestigatorId(1);
    let inst = CardInstanceId(0);
    let mut inv = test_investigator(1);
    inv.current_location = Some(LocationId(1));
    inv.cards_in_play
        .push(CardInPlay::enter_play(CardCode::new(COP), inst));
    let mut builder = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_active_investigator(id)
        .with_investigator_turn(id)
        .with_location(test_location(1, "A"));
    if enemy_at_loc {
        let mut e = test_enemy(100, "Ghoul");
        e.max_health = 3;
        e.current_location = Some(LocationId(1));
        builder = builder.with_enemy(e);
    }
    let state = builder.with_investigator(inv).build();
    (state, id, inst)
}

#[test]
fn discard_self_deal_damage_rejects_with_no_enemy_and_keeps_source_in_play() {
    let (state, id, inst) = board_with_cop(false);
    let result = dispatch_turn_action_unchecked(
        state,
        &TurnAction::ActivateAbility {
            investigator: id,
            instance_id: inst,
            ability_index: 0,
        },
    );
    assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert_eq!(
        result.state.investigators[&id].cards_in_play.len(),
        1,
        "rejected before paying ⇒ source NOT discarded",
    );
}

#[test]
fn discard_self_combined_with_exhaust_rejects_before_paying() {
    install_mock_registry();
    let id = InvestigatorId(1);
    let inst = CardInstanceId(0);
    let mut inv = test_investigator(1);
    inv.cards_in_play
        .push(CardInPlay::enter_play(CardCode::new(COMBO), inst));
    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_active_investigator(id)
        .with_investigator(inv)
        .build();

    let result = dispatch_turn_action_unchecked(
        state,
        &TurnAction::ActivateAbility {
            investigator: id,
            instance_id: inst,
            ability_index: 0,
        },
    );
    assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert_eq!(
        result.state.investigators[&id].cards_in_play.len(),
        1,
        "rejected combo ⇒ source untouched",
    );
}

#[test]
fn discard_self_deal_damage_discards_source_and_damages_the_enemy() {
    let (state, id, inst) = board_with_cop(true);
    let idx = game_core::engine::enumerate::legal_actions(&state)
        .iter()
        .position(|a| {
            a == &TurnAction::ActivateAbility {
                investigator: id,
                instance_id: inst,
                ability_index: 0,
            }
        })
        .expect("ability must be legal");
    let result = apply_no_commits(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(OptionId(u32::try_from(idx).unwrap())),
        }),
    );
    assert_eq!(result.outcome, EngineOutcome::Done);
    assert!(
        result.state.investigators[&id].cards_in_play.is_empty(),
        "source discarded",
    );
    assert_eq!(result.state.enemies[&EnemyId(100)].damage, 1);
}

/// Build a board with a 1-supply `code` asset in play (instance 0, seeded pool).
fn board_with_kit(code: &str) -> (game_core::GameState, InvestigatorId, CardInstanceId) {
    install_mock_registry();
    let id = InvestigatorId(1);
    let inst = CardInstanceId(0);
    let mut inv = test_investigator(1);
    let mut kit = CardInPlay::enter_play(CardCode::new(code), inst);
    kit.uses.insert(UseKind::Supplies, 1);
    inv.cards_in_play.push(kit);
    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_active_investigator(id)
        .with_investigator_turn(id)
        .with_investigator(inv)
        .build();
    (state, id, inst)
}

#[test]
fn spending_last_use_discards_a_discard_when_empty_asset() {
    let (state, id, inst) = board_with_kit(KIT);
    let idx = game_core::engine::enumerate::legal_actions(&state)
        .iter()
        .position(|a| {
            a == &TurnAction::ActivateAbility {
                investigator: id,
                instance_id: inst,
                ability_index: 0,
            }
        })
        .expect("ability must be legal");
    let result = apply_no_commits(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(OptionId(u32::try_from(idx).unwrap())),
        }),
    );
    assert_eq!(result.outcome, EngineOutcome::Done);
    assert!(
        result.state.investigators[&id].cards_in_play.is_empty(),
        "depleted discard_when_empty asset discarded",
    );
    assert_eq!(
        result.state.investigators[&id].discard,
        vec![CardCode::new(KIT)]
    );
}

#[test]
fn spending_last_use_keeps_a_non_discarding_asset_in_play() {
    let (state, id, inst) = board_with_kit(KIT_NODISC);
    let idx = game_core::engine::enumerate::legal_actions(&state)
        .iter()
        .position(|a| {
            a == &TurnAction::ActivateAbility {
                investigator: id,
                instance_id: inst,
                ability_index: 0,
            }
        })
        .expect("ability must be legal");
    let result = apply_no_commits(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(OptionId(u32::try_from(idx).unwrap())),
        }),
    );
    assert_eq!(result.outcome, EngineOutcome::Done);
    assert_eq!(
        result.state.investigators[&id].cards_in_play.len(),
        1,
        "non-discarding asset stays at 0 uses",
    );
}
