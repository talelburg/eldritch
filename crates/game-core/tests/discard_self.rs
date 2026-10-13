//! `Cost::DiscardSelf` + the Beat-Cop-shaped `DiscardSelf → DealDamageToEnemy`
//! ability. Mock registry in its own integration binary (own process +
//! `OnceLock<CardRegistry>`), mirroring `weapon_fight.rs`.

use card_dsl::card_data::{CardKind, CardMetadata, Class, SkillIcons, Slot, UseKind, Uses};
use card_dsl::dsl::{self, Ability, Cost, EnemyTarget, InvestigatorTarget};
use game_core::assert_event;
use game_core::engine::enumerate::TurnAction;
use game_core::event::Event;
use game_core::state::{
    AbilityAddress, AbilitySource, CardCode, DiscardPile, EnemyId, GameState, GameStateBuilder,
    InvestigatorId, LocationId, Zone,
};
use game_core::test_support::{self, seat, uses, MockRegistry, Seat, SeatLookup as _, TestSession};

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
        back_name: None,
        back_text: None,
        pack_code: "_mock".to_owned(),
        weakness: false,
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

/// `[fast] Spend 1 supply: gain 1 resource` — both kits share the ability; they
/// differ only in the metadata `discard_when_empty` flag.
fn spend_a_supply() -> Vec<Ability> {
    vec![dsl::activated(
        0,
        vec![Cost::SpendUses {
            kind: UseKind::Supplies,
            count: 1,
        }],
        dsl::gain_resources(InvestigatorTarget::You, 1),
    )]
}

#[ctor::ctor(unsafe)]
fn install_mock_registry() {
    MockRegistry::new()
        .with_card(asset_metadata(
            TRINKET,
            "Mock Trinket",
            "[fast] Discard: gain 1 resource.",
        ))
        .with_card(asset_metadata(
            COP,
            "Mock Cop",
            "[fast] Discard: deal 1 damage to an enemy at your location.",
        ))
        .with_card(asset_metadata(
            COMBO,
            "Mock Combo",
            "[fast] Exhaust, Discard: gain 1.",
        ))
        .with_card(kit_metadata(KIT, "Mock Kit", true))
        .with_card(kit_metadata(KIT_NODISC, "Mock Kit (stays)", false))
        // [fast] Discard Mock Trinket: gain 1 resource.
        .with_abilities(TRINKET, || {
            vec![dsl::activated(
                0,
                vec![Cost::DiscardSelf],
                dsl::gain_resources(InvestigatorTarget::You, 1),
            )]
        })
        // [fast] Discard Mock Cop: deal 1 damage to an enemy at your location.
        .with_abilities(COP, || {
            vec![dsl::activated(
                0,
                vec![Cost::DiscardSelf],
                dsl::deal_damage_to_enemy(EnemyTarget::chosen_at_your_location(), 1),
            )]
        })
        // Illegal: DiscardSelf cannot combine with another source cost (Exhaust).
        .with_abilities(COMBO, || {
            vec![dsl::activated(
                0,
                vec![Cost::DiscardSelf, Cost::Exhaust],
                dsl::gain_resources(InvestigatorTarget::You, 1),
            )]
        })
        .with_abilities(KIT, spend_a_supply)
        .with_abilities(KIT_NODISC, spend_a_supply)
        .install();
}

const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(1);
const GHOUL: EnemyId = EnemyId(100);

/// `seated`'s open turn.
fn board(seated: Seat) -> GameStateBuilder {
    GameStateBuilder::new().with_seat(seated).open_turn(INV)
}

/// The activation of the one ability on the in-play card `code`.
fn activation(state: &GameState, code: &str) -> TurnAction {
    TurnAction::ActivateAbility {
        investigator: INV,
        source: AbilitySource::InPlay(state.in_play(INV, code)),
        address: AbilityAddress::Printed(0),
    }
}

/// Take the activation of `code`'s ability from the menu; it resolves back
/// to the turn menu.
fn activate(state: GameState, code: &str) -> TestSession {
    let action = activation(&state, code);
    let session = TestSession::new(state).take(&action);
    session.assert_at_turn_menu(INV);
    session
}

/// Submit the activation of `code`'s ability off the menu, which does not
/// offer it, and assert the handler itself refuses it. Returns the state after
/// the refusal.
fn refused(state: GameState, code: &str) -> GameState {
    let action = activation(&state, code);
    let session = TestSession::new(state).submit_off_menu(&action);
    session.expect_rejected();
    session.finish().state
}

#[test]
fn discard_self_removes_source_from_play_and_runs_the_effect() {
    let state = board(seat(1).in_play([TRINKET])).build();
    let before = state.investigators[&INV].resources;

    let session = activate(state, TRINKET);

    let inv_after = &session.state().investigators[&INV];
    assert!(inv_after.cards_in_play.is_empty(), "source asset left play");
    assert_eq!(inv_after.discard, vec![CardCode::new(TRINKET)]);
    assert_eq!(inv_after.resources, before + 1, "the effect still ran");
    assert_event!(
        session.events(),
        Event::CardDiscarded {
            from: Zone::InPlay,
            to: DiscardPile::Investigator(investigator),
            ..
        } if *investigator == INV
    );
}

fn board_with_cop(enemy_at_loc: bool) -> GameState {
    let mut builder = board(seat(1).at(HERE).in_play([COP]))
        .with_location(test_support::test_location(HERE.0, "A"));
    if enemy_at_loc {
        let mut e = test_support::test_enemy(GHOUL.0, "Ghoul");
        e.max_health = 3;
        e.current_location = Some(HERE);
        builder = builder.with_enemy(e);
    }
    builder.build()
}

#[test]
fn discard_self_deal_damage_rejects_with_no_enemy_and_keeps_source_in_play() {
    let after = refused(board_with_cop(false), COP);
    assert_eq!(
        after.investigators[&INV].cards_in_play.len(),
        1,
        "rejected before paying ⇒ source NOT discarded",
    );
}

#[test]
fn discard_self_combined_with_exhaust_rejects_before_paying() {
    let after = refused(board(seat(1).in_play([COMBO])).build(), COMBO);
    assert_eq!(
        after.investigators[&INV].cards_in_play.len(),
        1,
        "rejected combo ⇒ source untouched",
    );
}

#[test]
fn discard_self_deal_damage_discards_source_and_damages_the_enemy() {
    let session = activate(board_with_cop(true), COP);
    assert!(
        session.state().investigators[&INV].cards_in_play.is_empty(),
        "source discarded",
    );
    assert_eq!(session.state().enemies[&GHOUL].damage, 1);
}

/// Build a board with a 1-supply `code` asset in play (seeded pool).
fn board_with_kit(code: &str) -> GameState {
    board(seat(1).in_play([(code, uses(UseKind::Supplies, 1))])).build()
}

#[test]
fn spending_last_use_discards_a_discard_when_empty_asset() {
    let session = activate(board_with_kit(KIT), KIT);
    assert!(
        session.state().investigators[&INV].cards_in_play.is_empty(),
        "depleted discard_when_empty asset discarded",
    );
    assert_eq!(
        session.state().investigators[&INV].discard,
        vec![CardCode::new(KIT)]
    );
}

#[test]
fn spending_last_use_keeps_a_non_discarding_asset_in_play() {
    let session = activate(board_with_kit(KIT_NODISC), KIT_NODISC);
    assert_eq!(
        session.state().investigators[&INV].cards_in_play.len(),
        1,
        "non-discarding asset stays at 0 uses",
    );
}
