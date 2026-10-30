//! Agnes Baker 01004, seated from her printed card: her `[reaction]` on horror
//! **placed on** her investigator card, its once-per-phase limit, and her
//! `[elder_sign]`.
//!
//! Card text (`data/arkhamdb-snapshot/pack/core/core.json`, 01004):
//! > [reaction] After 1 or more horror is placed on Agnes Baker: Deal 1 damage
//! > to an enemy at your location. (Limit once per phase.)
//! > [elder_sign] effect: +1 for each horror on Agnes Baker.
//!
//! Ruling (<https://arkhamdb.com/card/01004>):
//! > [reaction] You cannot use Agnes' ability if you assign horror to an asset.
//!
//! Holy Rosary 01059 is the soaker (*"You get +1 \[willpower\]."*, sanity 2).
//!
//! Abandoned and Alone 01015 places horror on her as one aspect of an effect:
//! *"**Revelation** - Take 2 direct horror and remove all cards in your discard
//! pile from the game."* Psychosis 01099 reacts to the same horror:
//! *"**Forced** - After you take 1 or more horror: Take 1 direct damage."*

use cards::TEST_REGISTRIES;
use game_core::engine::enumerate::TurnAction;
use game_core::engine::OptionTarget;
use game_core::event::Event;
use game_core::state::{
    ChaosBag, ChaosToken, EnemyId, GameStateBuilder, InvestigatorId, LocationId, Phase, SkillKind,
};
use game_core::test_support::{
    self, seat, BoardLookup as _, Either, Printed, Seat, StagedEnemy, TestSession,
};
use game_core::{assert_event, assert_no_event};

const AGNES: &str = "01004";
const HOLY_ROSARY: &str = "01059";
const ABANDONED_AND_ALONE: &str = "01015";
const PSYCHOSIS: &str = "01099";
/// Deck and discard filler.
const GUTS: &str = "01089";

const INV: InvestigatorId = InvestigatorId(1);
const ENEMY: EnemyId = EnemyId(100);
const HERE: LocationId = LocationId(10);
const THERE: LocationId = LocationId(11);

/// Agnes seated from her printed card at `HERE`.
fn agnes() -> Seat<Printed> {
    seat(1).as_card(AGNES).at(HERE)
}

/// A 3-health enemy whose attack deals 1 horror and no damage, so its
/// attack is itself a horror placement and survives Agnes's 1 damage.
fn horror_enemy() -> StagedEnemy {
    test_support::enemy(ENEMY)
        .health(3)
        .attack_damage(0)
        .attack_horror(1)
}

/// `agnes`'s turn at `HERE`, with a second location, `THERE`, and no enemy.
fn staged(agnes: impl Into<Seat<Either>>) -> GameStateBuilder {
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(agnes)
        .with_location(test_support::location(HERE))
        .with_location(test_support::location(THERE))
        .open_turn(INV)
}

/// `agnes` at her turn menu with [`horror_enemy`] engaged with her (so it is
/// at her location).
fn board(agnes: impl Into<Seat<Either>>) -> TestSession {
    staged(agnes)
        .with_enemy_engaged(horror_enemy(), INV)
        .session()
}

/// Agnes's investigator card, the anchor her reaction is offered on.
fn agnes_card(session: &TestSession) -> OptionTarget {
    session.state().investigators[&INV].card_anchor()
}

fn enemy_damage(session: &TestSession) -> u8 {
    session.state().enemies[&ENEMY].damage
}

#[test]
fn horror_placed_on_agnes_offers_her_reaction_which_damages_the_enemy() {
    let session = board(agnes()).take_horror(INV, 1);
    let card = agnes_card(&session);
    assert_eq!(session.offered_at(&card), 1, "her reaction is offered");

    let session = session.pick(card);
    session.assert_at_turn_menu(INV);
    assert_eq!(session.state().investigators[&INV].horror(), 1);
    assert_eq!(enemy_damage(&session), 1, "1 damage dealt to the enemy");
    assert_event!(
        session.events(),
        Event::EnemyDamaged { enemy, amount: 1, .. } if *enemy == ENEMY
    );
}

/// Abandoned and Alone's horror is placed on her and taken by her, and both
/// "after" cells wait until the Revelation has resolved in full
/// (`glossary/Effects.md`: *"All aspects of an effect have timing priority over
/// all "after..." triggering conditions that might arise as a consequence of
/// that effect."*). They then open in the order their conditions reached them:
/// the placement, nested in the taking, first. So her reaction is offered once
/// the discard pile is gone and before Psychosis deals its damage.
#[test]
fn her_reaction_to_abandoned_and_alone_waits_for_the_revelation_and_precedes_psychosis() {
    let session = staged(
        agnes()
            .actions(3)
            .threat_area([PSYCHOSIS])
            .discard([GUTS])
            .deck([ABANDONED_AND_ALONE, GUTS]),
    )
    .with_enemy(horror_enemy().at(HERE))
    .session()
    .take(&TurnAction::Draw { investigator: INV });

    let card = agnes_card(&session);
    assert_eq!(session.offered_at(&card), 1, "her reaction is offered");
    assert_event!(
        session.events(),
        Event::DiscardPileRemovedFromGame { investigator, .. } if *investigator == INV
    );
    assert_eq!(
        session.state().investigators[&INV].damage(),
        0,
        "Psychosis waits"
    );

    let session = session.pick(card);
    session.assert_at_turn_menu(INV);
    assert_eq!(enemy_damage(&session), 1);
    assert_eq!(
        session.state().investigators[&INV].damage(),
        1,
        "then Psychosis"
    );
}

/// *"You cannot use Agnes' ability if you assign horror to an asset."*
#[test]
fn horror_fully_soaked_by_holy_rosary_does_not_offer_her_reaction() {
    let session = board(agnes().in_play([HOLY_ROSARY])).take_horror(INV, 1);
    let rosary = session.state().card(INV, HOLY_ROSARY);
    let session = session.pick(rosary);

    session.assert_at_turn_menu(INV);
    assert_eq!(session.state().investigators[&INV].horror(), 0);
    assert_eq!(enemy_damage(&session), 0, "no reaction, no damage");
}

/// Horror split between Agnes and the Rosary still places 1 on her.
#[test]
fn horror_partly_soaked_still_offers_her_reaction() {
    let session = board(agnes().in_play([HOLY_ROSARY])).take_horror(INV, 2);
    let rosary = session.state().card(INV, HOLY_ROSARY);
    let me = agnes_card(&session);
    let session = session.pick(rosary).pick(me);

    let card = agnes_card(&session);
    assert_eq!(session.offered_at(&card), 1);
    let session = session.pick(card);
    session.assert_at_turn_menu(INV);
    assert_eq!(enemy_damage(&session), 1);
}

#[test]
fn no_enemy_at_her_location_means_no_offer() {
    let session = staged(agnes())
        .with_enemy(horror_enemy().at(THERE))
        .session()
        .take_horror(INV, 1);

    session.assert_at_turn_menu(INV);
    assert_eq!(session.state().investigators[&INV].horror(), 1);
    assert_eq!(enemy_damage(&session), 0);
}

/// *"(Limit once per phase.)"* — `glossary/Limits_and_Maximums.md`: *"Each
/// instance of an ability with such a limit may be initiated X times during
/// the designated period."* A second placement in the Investigation phase is
/// not offered; the enemy's horror attack in the Enemy phase of the same round
/// is.
#[test]
fn once_per_phase_refuses_a_second_fire_but_allows_a_later_phase_of_the_round() {
    // A deck to draw from at Upkeep, so the deck-out horror does not place a
    // fourth horror there.
    let session = board(agnes().deck(["01088", "01088"])).take_horror(INV, 1);
    let card = agnes_card(&session);
    let session = session.pick(card.clone());
    assert_eq!(enemy_damage(&session), 1);
    let round = session.state().round;

    // Same phase: the placement lands, the reaction is not offered.
    let session = session.take_horror(INV, 1);
    session.assert_at_turn_menu(INV);
    assert_eq!(session.state().investigators[&INV].horror(), 2);
    assert_eq!(enemy_damage(&session), 1, "second fire refused");

    // End the turn: the Enemy phase of the same round, where the engaged
    // enemy's attack places 1 horror on her.
    let session = session.take(&TurnAction::EndTurn);
    assert_eq!(session.state().round, round, "still the same round");
    assert_eq!(session.offered_at(&card), 1, "offered again in a new phase");
    assert_eq!(session.state().phase, Phase::Enemy);
    let session = session.pick(card);
    assert_eq!(session.state().investigators[&INV].horror(), 3);
    assert_eq!(
        enemy_damage(&session),
        2,
        "fired once more in the Enemy phase"
    );
}

/// *"\[elder_sign\] effect: +1 for each horror on Agnes Baker."* Willpower 5 vs
/// difficulty 5 with N horror on her succeeds by N.
#[test]
fn elder_sign_adds_one_per_horror_on_her() {
    for horror in [0_u8, 2] {
        let session = GameStateBuilder::new()
            .with_registries(TEST_REGISTRIES)
            .with_seat(agnes().harm(0, horror))
            .with_location(test_support::location(HERE))
            .with_chaos_bag(ChaosBag::new([ChaosToken::ElderSign]))
            .open_turn(INV)
            .session()
            .test_skill(INV, SkillKind::Willpower, 5)
            .commit(&[]);
        session.assert_at_turn_menu(INV);
        assert_event!(
            session.events(),
            Event::SkillTestSucceeded { margin, .. } if i16::from(*margin) == i16::from(horror)
        );
        assert_no_event!(session.events(), Event::SkillTestFailed { .. });
    }
}
