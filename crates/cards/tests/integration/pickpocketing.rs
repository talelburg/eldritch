//! Pickpocketing 01046 end-to-end against the real corpus.
//!
//! Card text (`data/arkhamdb-snapshot/pack/core/core.json`, 01046):
//!
//! > [reaction] After you evade an enemy, exhaust Pickpocketing: Draw 1 card.
//!
//! Rulings (<https://arkhamdb.com/card/01046>), the second in part:
//!
//! > If you use this to draw a card with the same triggering condition (e.g.
//! > Close Call), you can play that card during the same reaction window.
//!
//! > […] the auto-evade effect from Breaking and Entering can trigger the
//! > reaction on Pickpocketing (0).
//!
//! The first ruling is not modelled. TODO(#1082): a card drawn mid-window is
//! never offered in that window. The second ruling is why the trigger is on the shared
//! evade exit rather than on the Evade action, which these tests reach through
//! two different evades: the basic action and Blinding Light 01066's designated
//! **Evade**.
//!
//! Runs against `cards::TEST_REGISTRIES`.

use cards::TEST_REGISTRIES;
use game_core::engine::enumerate::{PlaySource, TurnAction};
use game_core::engine::OptionTarget;
use game_core::event::Event;
use game_core::state::{
    CardCode, ChaosBag, ChaosToken, EnemyId, GameStateBuilder, InvestigatorId, LocationId,
    SkillKind,
};
use game_core::test_support::{enemy, location, seat, Seat, SeatLookup as _, TestSession};
use game_core::{assert_event, assert_event_sequence};

const PICKPOCKETING: &str = "01046";
/// Event. *"Evade. This evasion attempt uses \[willpower\] instead of
/// \[agility\]."*
const BLINDING_LIGHT: &str = "01066";
/// Deck filler, the card Pickpocketing draws.
const GUTS: &str = "01089";

const INV: InvestigatorId = InvestigatorId(1);
const INV2: InvestigatorId = InvestigatorId(2);
const HERE: LocationId = LocationId(10);
const GHOUL: EnemyId = EnemyId(100);
const RAT: EnemyId = EnemyId(101);

/// Investigator `id` at `HERE` with Pickpocketing in play, agility and
/// willpower 4, 3 actions, 5 resources and a two-card deck to draw from.
fn rogue(id: u32) -> Seat {
    seat(id)
        .at(HERE)
        .in_play([PICKPOCKETING])
        .skill(SkillKind::Agility, 4)
        .skill(SkillKind::Willpower, 4)
        .actions(3)
        .resources(5)
        .deck([GUTS, GUTS])
}

/// `seats`' board, on investigator 1's open turn, with each of `engaged`
/// engaged with investigator 1 and a chaos bag holding only `token`.
fn board(seats: Vec<Seat>, engaged: &[EnemyId], token: ChaosToken) -> TestSession {
    let mut builder = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(location(HERE))
        .with_chaos_bag(ChaosBag::new([token]));
    for seat in seats {
        builder = builder.with_seat(seat);
    }
    for &id in engaged {
        builder = builder.with_enemy_engaged(enemy(id).evade(2), INV);
    }
    TestSession::new(builder.open_turn(INV).build(), TEST_REGISTRIES)
}

/// Investigator 1 evading `enemy` with the basic action, committing nothing.
fn evade(session: TestSession, enemy: EnemyId) -> TestSession {
    session
        .take(&TurnAction::Evade {
            investigator: INV,
            enemy,
        })
        .commit(&[])
}

fn pickpocketing(session: &TestSession, holder: InvestigatorId) -> OptionTarget {
    session.state().card(holder, PICKPOCKETING)
}

fn exhausted(session: &TestSession, holder: InvestigatorId) -> bool {
    session.state().investigators[&holder]
        .cards_in_play
        .iter()
        .find(|c| c.code == CardCode::new(PICKPOCKETING))
        .expect("Pickpocketing is in play")
        .exhausted
}

fn hand(session: &TestSession, holder: InvestigatorId) -> &[CardCode] {
    &session.state().investigators[&holder].hand
}

#[test]
fn a_successful_evade_offers_it_and_using_it_exhausts_it_and_draws_1() {
    let s = evade(
        board(vec![rogue(1)], &[GHOUL], ChaosToken::Numeric(0)),
        GHOUL,
    );

    // The `after` cell: the enemy is already evaded when the window opens.
    assert_eq!(s.offered_at(&pickpocketing(&s, INV)), 1, "offered");
    let ghoul = &s.state().enemies[&GHOUL];
    assert_eq!(ghoul.engaged_with, None);
    assert!(ghoul.exhausted);
    assert!(hand(&s, INV).is_empty());

    let target = pickpocketing(&s, INV);
    let s = s.pick(target);
    s.assert_at_turn_menu(INV);
    assert!(exhausted(&s, INV), "the cost exhausted Pickpocketing");
    assert_eq!(hand(&s, INV), [CardCode::new(GUTS)], "drew 1 card");
    assert_event_sequence!(
        s.events(),
        Event::EnemyEvaded { enemy, investigator } if *enemy == GHOUL && *investigator == INV,
        Event::CardExhausted { .. },
        Event::CardsDrawn { .. },
    );
}

#[test]
fn declining_it_leaves_it_ready() {
    let s = evade(
        board(vec![rogue(1)], &[GHOUL], ChaosToken::Numeric(0)),
        GHOUL,
    )
    .skip();
    s.assert_at_turn_menu(INV);
    assert!(!exhausted(&s, INV));
    assert!(hand(&s, INV).is_empty());
}

#[test]
fn an_exhausted_pickpocketing_is_not_offered() {
    // Evade the ghoul and use Pickpocketing, which exhausts it; then evade the
    // rat, and nothing is offered.
    let s = evade(
        board(vec![rogue(1)], &[GHOUL, RAT], ChaosToken::Numeric(0)),
        GHOUL,
    );
    let target = pickpocketing(&s, INV);
    let s = evade(s.pick(target), RAT);

    s.assert_at_turn_menu(INV);
    assert_eq!(s.state().enemies[&RAT].engaged_with, None, "the rat evaded");
    assert_eq!(hand(&s, INV), [CardCode::new(GUTS)], "only the first draw");
}

#[test]
fn a_failed_evade_offers_nothing() {
    // Agility 4 - 3 = 1 against evade 2 fails.
    let s = evade(
        board(vec![rogue(1)], &[GHOUL], ChaosToken::Numeric(-3)),
        GHOUL,
    );
    s.assert_at_turn_menu(INV);
    assert_eq!(s.state().enemies[&GHOUL].engaged_with, Some(INV));
    assert!(hand(&s, INV).is_empty());
}

#[test]
fn a_designated_evade_triggers_it_too() {
    let s = board(
        vec![rogue(1).hand([BLINDING_LIGHT])],
        &[GHOUL],
        ChaosToken::Numeric(0),
    )
    .take(&TurnAction::PlayCard {
        investigator: INV,
        source: PlaySource::Hand(0),
    })
    .commit(&[]);

    assert_eq!(s.offered_at(&pickpocketing(&s, INV)), 1, "offered");
    let target = pickpocketing(&s, INV);
    let s = s.pick(target);
    s.assert_at_turn_menu(INV);
    assert_eq!(hand(&s, INV), [CardCode::new(GUTS)]);
    // Blinding Light's "If you succeed, deal 1 damage to the enemy just
    // evaded" still lands.
    assert_eq!(s.state().enemies[&GHOUL].damage, 1);
}

#[test]
fn another_investigators_evade_does_not_trigger_it() {
    // Both investigators hold Pickpocketing; investigator 1 evades, and only
    // their copy is offered.
    let s = evade(
        board(vec![rogue(1), rogue(2)], &[GHOUL], ChaosToken::Numeric(0)),
        GHOUL,
    );
    assert_eq!(s.offered_at(&pickpocketing(&s, INV)), 1);
    assert_eq!(s.offered_at(&pickpocketing(&s, INV2)), 0);
    assert_event!(s.events(), Event::EnemyEvaded { investigator, .. } if *investigator == INV);
}
