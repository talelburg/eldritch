//! Regression for #409: a failed attack against an enemy engaged with another
//! investigator deals its damage to that investigator.
//!
//! `data/rules-reference/rules/glossary/Fight_Action.md`:
//!
//! > If the test fails, no damage is dealt to the attacked enemy. However, if
//! > an investigator fails this test against an enemy that is engaged with
//! > another single investigator, the damage of the attack is dealt to the
//! > investigator engaged with that enemy.
//!
//! The board is two investigators sharing a location with Ghoul Priest 01116
//! (*"Hunter. Retaliate."*, fight 4, 2 damage / 2 horror). The fighter has
//! combat 1 against a `Numeric(0)` bag, so every attack here fails.
//!
//! Runs against `cards::TEST_REGISTRIES`.

use cards::TEST_REGISTRIES;
use game_core::engine::enumerate::TurnAction;
use game_core::engine::{OptionTarget, Responder};
use game_core::event::Event;
use game_core::state::{
    AbilityAddress, AbilitySource, ChaosBag, ChaosToken, EnemyId, GameStateBuilder, InvestigatorId,
    LocationId, SkillKind, UseKind,
};
use game_core::test_support::{enemy, location, seat, uses, BoardLookup as _, Seat, TestSession};
use game_core::{assert_event, assert_event_count, assert_event_sequence, assert_no_event};

/// Ghoul Priest (01116): fight 4, health 5 per investigator, attack 2 / 2,
/// *"Hunter. Retaliate."*
const GHOUL_PRIEST: &str = "01116";

/// Shrivelling (01060): *"[action] Spend 1 charge: <b>Fight.</b> This attack
/// uses [willpower] instead of [combat] and deals +1 damage. If a [skull],
/// [cultist], [tablet], [elder_thing], or [auto_fail] symbol is revealed during
/// this attack, take 1 horror."*
const SHRIVELLING: &str = "01060";

/// .45 Automatic (01016): *"[action] Spend 1 ammo: <b>Fight.</b> You get +1
/// [combat] for this attack. This attack deals +1 damage."*
const AUTOMATIC: &str = "01016";

/// Vicious Blow (01025), one `[combat]` icon: *"If this skill test is
/// successful during an attack, that attack deals +1 damage."*
const VICIOUS_BLOW: &str = "01025";

/// Guard Dog (01021), health 3: *"[reaction] When an enemy attack deals damage
/// to Guard Dog: Deal 1 damage to the attacking enemy."*
const GUARD_DOG: &str = "01021";

/// The fighter.
const AGNES: InvestigatorId = InvestigatorId(1);
/// The teammate sharing the location.
const ROLAND: InvestigatorId = InvestigatorId(2);
const HALLWAY: LocationId = LocationId(10);
const PRIEST: EnemyId = EnemyId(100);

/// A fighter whose combat 1 fails against the Priest's fight 4 on a
/// `Numeric(0)` token.
fn fighter() -> Seat {
    seat(1).at(HALLWAY).skill(SkillKind::Combat, 1)
}

/// Agnes's turn, Roland beside her, the Priest engaged with `engaged`
/// (`None`: unengaged), and a bag that holds only `Numeric(0)`.
fn hallway(agnes: Seat, roland: Seat, engaged: Option<InvestigatorId>) -> GameStateBuilder {
    hallway_on_turn(AGNES, agnes, roland, engaged)
}

/// [`hallway`] on `active`'s turn.
fn hallway_on_turn(
    active: InvestigatorId,
    agnes: Seat,
    roland: Seat,
    engaged: Option<InvestigatorId>,
) -> GameStateBuilder {
    let priest = enemy(PRIEST).as_card(GHOUL_PRIEST).at(HALLWAY);
    let priest = match engaged {
        Some(investigator) => priest.engaged_with(investigator),
        None => priest,
    };
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(location(HALLWAY))
        .with_seat(agnes)
        .with_seat(roland)
        .with_seating([AGNES, ROLAND])
        .with_enemy(priest)
        .open_turn(active)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
}

/// Agnes takes the basic Fight action against the Priest and commits nothing.
fn basic_fight(session: TestSession) -> TestSession {
    session
        .take(&TurnAction::Fight {
            investigator: AGNES,
            enemy: PRIEST,
        })
        .commit(&[])
}

/// `investigator` activates the first printed ability of their in-play
/// `code` (a **Fight**) against whatever it targets, committing `commit`.
fn attack_with(
    session: TestSession,
    investigator: InvestigatorId,
    code: &str,
    commit: &[&str],
) -> TestSession {
    let card = session.state().in_play(investigator, code);
    session
        .take(&TurnAction::ActivateAbility {
            investigator,
            source: AbilitySource::InPlay(card),
            address: AbilityAddress::Printed(0),
        })
        .commit(commit)
}

/// Case 1: a basic Fight fails against the Priest engaged with Roland. Roland
/// is dealt the attack's 1 damage, the Priest takes none, and only then does
/// the Priest's Retaliate attack Agnes (`glossary/Retaliate.md`: *"after
/// applying all results for that skill test"*).
#[test]
fn failed_fight_deals_its_damage_to_the_engaged_teammate_before_retaliate() {
    let session = basic_fight(hallway(fighter(), seat(2).at(HALLWAY), Some(ROLAND)).session());

    session.assert_at_turn_menu(AGNES);
    assert_event!(session.events(), Event::SkillTestFailed { .. });
    assert_no_event!(session.events(), Event::EnemyDamaged { .. });
    assert_eq!(session.state().enemies[&PRIEST].damage, 0);
    assert_eq!(session.state().investigators[&ROLAND].damage(), 1);
    assert_event_sequence!(
        session.events(),
        Event::DamagePlacedOnInvestigator {
            investigator: ROLAND,
            amount: 1,
        },
        Event::DamagePlacedOnInvestigator {
            investigator: AGNES,
            amount: 2,
        },
    );
}

/// Case 2: Shrivelling's *"deals +1 damage"* is part of *"the damage of the
/// attack"*, so Roland is dealt 2. Willpower 1 fails against fight 4, and the
/// `Numeric(0)` token keeps Shrivelling's horror clause out of it.
#[test]
fn a_weapons_plus_one_damage_rides_on_the_redirect() {
    let agnes = seat(1)
        .at(HALLWAY)
        .skill(SkillKind::Willpower, 1)
        .in_play([(SHRIVELLING, uses(UseKind::Charges, 4))]);
    let session = attack_with(
        hallway(agnes, seat(2).at(HALLWAY), Some(ROLAND)).session(),
        AGNES,
        SHRIVELLING,
        &[],
    );

    session.assert_at_turn_menu(AGNES);
    assert_event!(session.events(), Event::SkillTestFailed { .. });
    assert_event!(
        session.events(),
        Event::DamagePlacedOnInvestigator {
            investigator: ROLAND,
            amount: 2,
        }
    );
    assert_eq!(session.state().investigators[&ROLAND].damage(), 2);
    assert_eq!(session.state().enemies[&PRIEST].damage, 0);
}

/// Case 3, the trap: Roland fails with .45 Automatic and a committed Vicious
/// Blow against the Priest engaged with Agnes. The pistol's +1 is the
/// attack's, so it reaches Agnes; Vicious Blow's is gated on *"If this skill
/// test is successful"*, so it doesn't. Agnes is dealt 2, not 3.
///
/// Roland's combat 1, +1 for the attack and +1 for Vicious Blow's icon, is 3
/// against fight 4.
#[test]
fn a_committed_skills_success_bonus_never_reaches_the_redirect() {
    let roland = seat(2)
        .at(HALLWAY)
        .skill(SkillKind::Combat, 1)
        .hand([VICIOUS_BLOW])
        .in_play([(AUTOMATIC, uses(UseKind::Ammo, 4))]);
    let session = attack_with(
        hallway_on_turn(ROLAND, seat(1).at(HALLWAY), roland, Some(AGNES)).session(),
        ROLAND,
        AUTOMATIC,
        &[VICIOUS_BLOW],
    );

    session.assert_at_turn_menu(ROLAND);
    assert_event!(session.events(), Event::SkillTestFailed { .. });
    assert_event_count!(
        session.events(),
        1,
        Event::DamagePlacedOnInvestigator {
            investigator: AGNES,
            ..
        }
    );
    assert_event!(
        session.events(),
        Event::DamagePlacedOnInvestigator {
            investigator: AGNES,
            amount: 2,
        }
    );
    assert_eq!(session.state().investigators[&AGNES].damage(), 2);
}

/// Case 4: the redirected damage is Roland's to assign, so he may soak it onto
/// Guard Dog. Guard Dog's reaction is not offered, because no enemy attack
/// dealt the damage: the Priest takes nothing.
#[test]
fn the_teammate_soaks_the_redirect_and_guard_dog_stays_silent() {
    let roland = seat(2).at(HALLWAY).in_play([GUARD_DOG]);
    let session = hallway(fighter(), roland, Some(ROLAND)).session();
    let dog = session.state().in_play(ROLAND, GUARD_DOG);

    let session = basic_fight(session);

    assert_eq!(
        session.prompt().responder,
        Responder::Investigator(ROLAND),
        "the soak prompt is Roland's: {:?}",
        session.prompt(),
    );
    let session = session.pick(OptionTarget::CardInstance(dog));

    // No Guard Dog window: the next stop is Agnes's turn, after the Retaliate.
    session.assert_at_turn_menu(AGNES);
    assert_eq!(
        session
            .state()
            .card_in_play(ROLAND, GUARD_DOG)
            .accumulated_damage,
        1
    );
    assert_eq!(session.state().investigators[&ROLAND].damage(), 0);
    assert_eq!(session.state().enemies[&PRIEST].damage, 0);
    assert_no_event!(session.events(), Event::EnemyDamaged { .. });
}

/// Case 5: the rule's *"another single investigator"*. A failed attack on an
/// enemy engaged with the fighter, or on an unengaged enemy, deals nobody
/// anything; the only damage dealt is the Retaliate's 2 to Agnes.
#[test]
fn a_failed_fight_with_no_other_engaged_investigator_deals_nobody_damage() {
    for engaged in [Some(AGNES), None] {
        let session = basic_fight(hallway(fighter(), seat(2).at(HALLWAY), engaged).session());

        session.assert_at_turn_menu(AGNES);
        assert_event!(session.events(), Event::SkillTestFailed { .. });
        assert_no_event!(
            session.events(),
            Event::DamagePlacedOnInvestigator {
                investigator: ROLAND,
                ..
            }
        );
        assert_event_count!(
            session.events(),
            1,
            Event::DamagePlacedOnInvestigator {
                investigator: AGNES,
                amount: 2,
            }
        );
        assert_eq!(
            session.state().investigators[&ROLAND].damage(),
            0,
            "{engaged:?}"
        );
        assert_eq!(
            session.state().investigators[&AGNES].damage(),
            2,
            "{engaged:?}"
        );
        assert_eq!(session.state().enemies[&PRIEST].damage, 0, "{engaged:?}");
    }
}
