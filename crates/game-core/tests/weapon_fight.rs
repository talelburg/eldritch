//! End-to-end weapon flow with a mock `CardRegistry`: a firearm-shaped
//! asset that carries `Uses (4 ammo)` and an `[action] Spend 1 ammo:
//! Fight` activated ability — an `ActionDesignator::Fight` on the trigger,
//! carrying the `+1 [combat]` / `+1` damage the card prints. The designator is
//! what makes it a fight action and what **performs** the attack (#696, #805):
//! it is what the attack-of-opportunity exemption, the pre-cost "needs a
//! co-located enemy" check, and the attack itself all read.
//!
//! Lives at `crates/game-core/tests/` (its own integration-test binary,
//! hence its own process and its own `MockRegistry`) so the mock
//! registry doesn't collide with the in-crate tests. No real card has a
//! weapon ability yet — Roland's .38 Special (C5c) is the first; until
//! then a mock card exercises the full path.

use card_dsl::card_data::{CardKind, CardMetadata, Class, SkillIcons, Slot, UseKind, Uses};
use card_dsl::dsl::{self, Cost, IntExpr};
use game_core::assert_event;
use game_core::engine::enumerate::TurnAction;
use game_core::engine::{InputKind, OptionTarget, Registries};
use game_core::event::Event;
use game_core::scenario::ScenarioRegistry;
use game_core::state::{
    AbilityAddress, AbilitySource, CardInstanceId, ChaosBag, ChaosToken, Enemy, EnemyId, GameState,
    GameStateBuilder, InvestigatorId, LocationId, TokenModifiers,
};
use game_core::test_support::{
    self, seat, uses, MockRegistry, SeatCard, SeatLookup as _, TestSession,
};
use std::sync::LazyLock;

/// Mock firearm: `Uses (4 ammo)`, `[action] Spend 1 ammo: Fight. +1
/// [combat], +1 damage.`
const WEAPON: &str = "WEAP1";

/// Mock asset printing a bare `[action]: Fight.` with **no** modification —
/// the designated action stripped to exactly what the basic Fight action is.
/// Only [`a_designated_fight_is_a_fight_action`] uses it.
const BARE: &str = "WEAP0";

fn weapon_metadata() -> CardMetadata {
    CardMetadata {
        code: WEAPON.to_owned(),
        name: "Mock Firearm".to_owned(),
        traits: vec!["Item".to_owned(), "Weapon".to_owned(), "Firearm".to_owned()],
        text: Some("Uses (4 ammo).\n[action] Spend 1 ammo: Fight.".to_owned()),
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
            uses: Some(Uses {
                kind: UseKind::Ammo,
                count: 4,
                discard_when_empty: false,
            }),
            play_only_during_turn: false,
        },
    }
}

fn bare_metadata() -> CardMetadata {
    CardMetadata {
        code: BARE.to_owned(),
        name: "Mock Cudgel".to_owned(),
        text: Some("[action]: Fight.".to_owned()),
        kind: match weapon_metadata().kind {
            CardKind::Asset { class, cost, .. } => CardKind::Asset {
                class,
                cost,
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
            other => other,
        },
        ..weapon_metadata()
    }
}

fn registries() -> Registries {
    static REGISTRIES: LazyLock<Registries> = LazyLock::new(|| Registries {
        cards: MockRegistry::new()
            .with_card(weapon_metadata())
            .with_card(bare_metadata())
            // [action] Spend 1 ammo: Fight. +1 [combat], +1 damage.
            .with_abilities(WEAPON, || {
                vec![dsl::activated_as(
                    dsl::fight(IntExpr::Lit(1), 1u8),
                    1,
                    vec![Cost::SpendUses {
                        kind: UseKind::Ammo,
                        count: 1,
                    }],
                    dsl::seq([]),
                )]
            })
            // [action]: Fight. No modification at all.
            .with_abilities(BARE, || {
                vec![dsl::activated_as(
                    dsl::fight(0u8, 0u8),
                    1,
                    vec![],
                    dsl::seq([]),
                )]
            })
            .build(),
        scenarios: &ScenarioRegistry::EMPTY,
    });
    *REGISTRIES
}

/// The single location the controller and its enemies share. Co-location is
/// load-bearing: a weapon Fight targets any enemy *at your location*
/// (`EntityScope::At(Here)`), so the board must place everyone there.
const HERE: LocationId = LocationId(1);

/// A location the controller is not at.
const ELSEWHERE: LocationId = LocationId(2);

const INV: InvestigatorId = InvestigatorId(1);

/// The first enemy a board seats; the rest follow it.
const GHOUL: EnemyId = EnemyId(100);

/// A Ghoul (fight 3, health 3) at `location`, engaged with the controller when
/// `engaged`.
fn ghoul(id: EnemyId, location: LocationId, engaged: bool) -> Enemy {
    let mut enemy = test_support::test_enemy(id.0, "Ghoul");
    enemy.fight = 3;
    enemy.max_health = 3;
    enemy.current_location = Some(location);
    enemy.engaged_with = engaged.then_some(INV);
    enemy
}

/// Board: `asset` in play for the controller, whose turn is open at [`HERE`],
/// with `enemy_count` Ghouls co-located there — engaged with the controller
/// when `engaged`. A `Numeric(0)` chaos bag makes the combat total
/// deterministic.
fn board(asset: SeatCard, enemy_count: u32, engaged: bool) -> GameStateBuilder {
    (0..enemy_count)
        .fold(
            GameStateBuilder::new().with_registries(registries()),
            |builder, n| builder.with_enemy(ghoul(EnemyId(GHOUL.0 + n), HERE, engaged)),
        )
        .with_seat(seat(INV.0).at(HERE).in_play([asset]))
        .with_location(test_support::test_location(HERE.0, "Study"))
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .open_turn(INV)
}

/// The weapon with a freshly-seeded 4-ammo pool, seeded as `play_card` would.
fn loaded_weapon() -> SeatCard {
    (WEAPON, uses(UseKind::Ammo, 4)).into()
}

/// [`board`] with the loaded weapon in play, engaged with `enemy_count` Ghouls.
fn board_with_weapon(enemy_count: u32) -> GameStateBuilder {
    board(loaded_weapon(), enemy_count, true)
}

/// The instance of the asset `code` the controller holds.
fn held(state: &GameState, code: &str) -> CardInstanceId {
    state.in_play(INV, code)
}

/// Activate the printed ability of the asset `code` the controller holds.
fn activate(state: &GameState, code: &str) -> TurnAction {
    TurnAction::ActivateAbility {
        investigator: INV,
        source: AbilitySource::InPlay(held(state, code)),
        address: AbilityAddress::Printed(0),
    }
}

fn ammo_remaining(state: &GameState) -> u8 {
    let weapon = held(state, WEAPON);
    state.investigators[&INV]
        .cards_in_play
        .iter()
        .find(|c| c.instance_id == weapon)
        .and_then(|c| c.uses.get(&UseKind::Ammo).copied())
        .expect("weapon must carry an ammo pool")
}

#[test]
fn play_card_seeds_the_ammo_pool_from_metadata() {
    let session = GameStateBuilder::new()
        .with_registries(registries())
        .with_seat(seat(INV.0).hand([WEAPON]))
        .open_turn(INV)
        .session()
        .take(&TurnAction::PlayCard {
            investigator: INV,
            hand_index: 0,
        });
    session.assert_at_turn_menu(INV);
    let weapon = session.state().investigators[&INV]
        .cards_in_play
        .first()
        .expect("weapon entered play");
    assert_eq!(weapon.uses.get(&UseKind::Ammo).copied(), Some(4));
}

#[test]
fn weapon_fight_spends_ammo_and_deals_bonus_damage() {
    let session = board_with_weapon(1).session();
    assert_eq!(ammo_remaining(session.state()), 4);

    // Activate → pays 1 ammo up front, then the Combat test resolves
    // (combat 3 + modifier 1 vs fight 3 → success) dealing 1 + 1 = 2.
    let action = activate(session.state(), WEAPON);
    let session = session.take(&action).commit(&[]);

    session.assert_at_turn_menu(INV);
    assert_event!(
        session.events(),
        Event::UsesSpent {
            kind: UseKind::Ammo,
            amount: 1,
            ..
        }
    );
    assert_event!(
        session.events(),
        Event::SkillTestStarted { difficulty: 3, .. }
    );
    assert_event!(session.events(), Event::EnemyDamaged { amount: 2, .. });
    assert_eq!(ammo_remaining(session.state()), 3);
    assert_eq!(session.state().enemies[&GHOUL].damage, 2);
}

#[test]
fn weapon_fight_targets_a_co_located_unengaged_enemy() {
    // A co-located but unengaged enemy (e.g. Aloof, or engaged with another
    // investigator in MP) is a legal weapon Fight target — RR: you choose an
    // enemy *at your location* to attack; you need not already be engaged
    // (#451). With exactly one such enemy the target auto-binds (no suspend),
    // mirroring the single-engaged case.
    let session = board(loaded_weapon(), 1, false).session();
    assert_eq!(
        session.state().enemies[&GHOUL].engaged_with,
        None,
        "precondition: the enemy is co-located but NOT engaged"
    );

    let action = activate(session.state(), WEAPON);
    let session = session.take(&action).commit(&[]);

    session.assert_at_turn_menu(INV);
    assert_event!(
        session.events(),
        Event::SkillTestStarted { difficulty: 3, .. }
    );
    assert_event!(
        session.events(),
        Event::EnemyDamaged {
            enemy: GHOUL,
            amount: 2,
            ..
        }
    );
    assert_eq!(ammo_remaining(session.state()), 3);
    assert_eq!(session.state().enemies[&GHOUL].damage, 2);
}

#[test]
fn weapon_fight_rejects_an_enemy_at_a_different_location() {
    // The scope is co-located (`At(Here)`), NOT global: an enemy elsewhere is
    // no target, even though it exists. Guards against widening past the basic
    // Fight action (#451). The menu already leaves the activation out, so it
    // is submitted off the menu, as a client could send it, to prove the
    // handler itself rejects it.
    let session = board(loaded_weapon(), 0, false)
        .with_location(test_support::test_location(ELSEWHERE.0, "Hallway"))
        .with_enemy(ghoul(GHOUL, ELSEWHERE, false))
        .session();
    let action = activate(session.state(), WEAPON);
    let session = session.submit_off_menu(&action);

    let reason = session.expect_rejected();
    assert!(
        reason.contains("at your location"),
        "should reject for the missing co-located target, not an unrelated \
         precondition; got: {reason}"
    );
    assert_eq!(ammo_remaining(session.state()), 4);
    assert_eq!(session.state().enemies[&GHOUL].damage, 0);
}

#[test]
fn weapon_fight_rejects_when_no_co_located_enemy() {
    // No enemy at your location → illegal (no target); reject before charging
    // anything. Submitted off the menu, which already leaves it out, to prove
    // the handler itself rejects it.
    let session = board_with_weapon(0).session();
    let actions_before = session.state().investigators[&INV].actions_remaining;
    let action = activate(session.state(), WEAPON);
    let session = session.submit_off_menu(&action);

    session.expect_rejected();
    // Nothing charged: ammo still 4, actions unchanged.
    assert_eq!(ammo_remaining(session.state()), 4);
    assert_eq!(
        session.state().investigators[&INV].actions_remaining,
        actions_before
    );
}

#[test]
fn weapon_fight_with_two_enemies_suspends_for_pick_then_attacks_chosen() {
    // 2+ engaged → suspends for target pick; after picking enemy 100, the
    // Fight resolves for 1 + 1 = 2 damage (combat 3 + mod 1 vs fight 3 →
    // success; extra_damage == 1 because the weapon's `fight` hardcodes `1u8`
    // as `extra_damage`, not "sole-engaged" conditional).
    let session = board_with_weapon(2).session();
    let other = EnemyId(GHOUL.0 + 1);

    // Step 1: activation suspends for the target pick, offering both enemies.
    let action = activate(session.state(), WEAPON);
    let paused = session.take(&action);
    assert_eq!(
        (
            paused.offered_at(&OptionTarget::Enemy(GHOUL)),
            paused.offered_at(&OptionTarget::Enemy(other)),
        ),
        (1, 1),
        "expected the target pick; got {:?}",
        paused.prompt(),
    );

    // Step 2: pick enemy 100, then commit nothing to resolve the test.
    let session = paused.pick(OptionTarget::Enemy(GHOUL)).commit(&[]);
    session.assert_at_turn_menu(INV);
    // Enemy 100 was attacked (chosen); enemy 101 was not.
    assert_event!(
        session.events(),
        Event::EnemyDamaged {
            enemy: GHOUL,
            amount: 2,
            ..
        }
    );
    assert_eq!(session.state().enemies[&GHOUL].damage, 2);
    assert_eq!(session.state().enemies[&other].damage, 0);
    // Ammo was spent on activation.
    assert_eq!(ammo_remaining(session.state()), 3);
}

/// **A designated Fight *is* a Fight action** (#805). `glossary/Ability.md`:
/// *"Activating such an ability **performs the designated action** as described
/// in the rules, but modified in the manner described by the ability."*
///
/// So a designated **Fight** carrying *no* modification and the basic Fight
/// action, taken against the same board, must resolve identically. Both run
/// through `actions::fight::perform`; before this issue they built their own
/// near-identical skill tests side by side, and the assertion below is what
/// keeps them from drifting apart again.
///
/// Compared from `SkillTestStarted` onward: the prefixes differ legitimately —
/// an activation announces `AbilityActivated` and charges the ability's printed
/// cost, a basic action charges 1 — and everything after is the action itself.
///
/// *"Apart from the modifier row"* is the whole of the licensed difference, so
/// the row count is asserted too rather than left implicit: the designated path
/// records one (the modification, `+0` here) and the basic path records none.
/// Without that assertion an unmodified designated Fight and a basic one could
/// differ in a way the event stream never shows.
#[test]
fn a_designated_fight_is_a_fight_action() {
    /// The board of [`board_with_weapon`], with the bare-Fight asset in play
    /// instead of the firearm.
    fn board_with_bare_asset() -> TestSession {
        board(BARE.into(), 1, true).session()
    }

    /// The bare asset's designated Fight, activated.
    fn designated(session: &TestSession) -> TurnAction {
        activate(session.state(), BARE)
    }

    const BASIC: TurnAction = TurnAction::Fight {
        investigator: INV,
        enemy: GHOUL,
    };

    /// Everything from the skill test's start onward, as `Debug` strings.
    fn from_the_test_onward(events: &[Event]) -> Vec<String> {
        let start = events
            .iter()
            .position(|e| matches!(e, Event::SkillTestStarted { .. }))
            .expect("the fight must start a skill test");
        events[start..].iter().map(|e| format!("{e:?}")).collect()
    }

    /// The rows recorded while the fight's test is still in flight. The step
    /// stops at the commit window, because a `Lifetime::SkillTest` row is
    /// swept when the test resolves, so after resolution both paths read empty
    /// and the comparison would be vacuous.
    fn rows_mid_test(session: TestSession, action: &TurnAction) -> usize {
        let session = session.take(action);
        assert_eq!(
            session.prompt().kind,
            InputKind::PickMultiple,
            "expected the commit window with the test in flight; got {:?}",
            session.prompt(),
        );
        session.state().recorded_modifiers.len()
    }

    /// Take `action` and commit nothing, back to the turn menu.
    fn resolve(session: TestSession, action: &TurnAction) -> TestSession {
        let session = session.take(action).commit(&[]);
        session.assert_at_turn_menu(INV);
        session
    }

    let board = board_with_bare_asset();
    let action = designated(&board);
    let designated_fight = resolve(board, &action);
    let basic = resolve(board_with_bare_asset(), &BASIC);

    assert_eq!(
        from_the_test_onward(designated_fight.events()),
        from_the_test_onward(basic.events()),
        "an unmodified designated Fight must resolve exactly as the basic \
         Fight action does",
    );
    assert_eq!(
        designated_fight.state().enemies[&GHOUL].damage,
        basic.state().enemies[&GHOUL].damage,
    );
    // The one licensed difference, pinned in both directions while the test is
    // still in flight: the designated Fight carries the ability's modification
    // as a row over the controller's combat (a `+0` one here, since this asset
    // prints no bonus), and the basic action carries none at all.
    let board = board_with_bare_asset();
    let action = designated(&board);
    assert_eq!(
        rows_mid_test(board, &action),
        1,
        "the designated Fight records its modification as one row",
    );
    assert_eq!(
        rows_mid_test(board_with_bare_asset(), &BASIC),
        0,
        "the basic Fight action carries no modification",
    );
}
