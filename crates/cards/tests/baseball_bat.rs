//! Baseball Bat 01074 end-to-end against the real corpus.
//!
//! Card text (`data/arkhamdb-snapshot/pack/core/core.json`, 01074):
//!
//! > [action]: <b>Fight.</b> You get +2 [combat] for this attack. This attack
//! > deals +1 damage. If a [skull] or [auto_fail] symbol is revealed during
//! > this attack, discard Baseball Bat after the attack resolves.
//!
//! Slot `Hand x2`. Ruling (<https://arkhamdb.com/card/01074>):
//!
//! > The *"If a symbol is revealed..."* effect triggers during Step 3 of the
//! > Skill Test Timing.
//!
//! The ruling's second paragraph — a token cancelled in its entirety never
//! triggers the clause — is Wendy Adams 01005's to test.
//!
//! Runs against `cards::TEST_REGISTRIES`, whose scenario registry is empty, so
//! the scenario symbols initiate no chaos symbol effect of their own and every
//! symbol modifies the test by `+0`.

use cards::TEST_REGISTRIES;
use game_core::engine::enumerate::{PlaySource, TurnAction};
use game_core::event::Event;
use game_core::state::{
    AbilityAddress, AbilitySource, CardCode, ChaosBag, ChaosToken, DiscardPile, EnemyId, GameState,
    GameStateBuilder, InvestigatorId, LocationId, SkillKind, TokenModifiers, Zone,
};
use game_core::test_support::{self, seat, SeatLookup as _, TestSession};
use game_core::{assert_event, assert_event_sequence, assert_no_event};

const BASEBALL_BAT: &str = "01074";
/// Machete 01020, a single-`Hand` asset: *"[action]: <b>Fight.</b> You get +1
/// [combat] for this attack. …"*
const MACHETE: &str = "01020";
const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(10);
const ENEMY: EnemyId = EnemyId(100);

/// Board: the Bat in play under the active investigator at combat `combat`,
/// engaged with a fight-3, health-3 enemy, and a chaos bag holding only
/// `token`.
fn board(combat: i8, token: ChaosToken) -> GameState {
    let mut enemy = test_support::test_enemy(ENEMY.0, "Ghoul");
    enemy.fight = 3;
    enemy.max_health = 3;

    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(
            seat(1)
                .at(HERE)
                .skill(SkillKind::Combat, combat)
                .in_play([BASEBALL_BAT]),
        )
        .with_location(test_support::test_location(HERE.0, "Study"))
        .with_enemy_engaged(enemy, INV)
        .open_turn(INV)
        .with_chaos_bag(ChaosBag::new([token]))
        .with_token_modifiers(TokenModifiers::default())
        .build()
}

/// Swing the Bat, committing nothing, and come back to the turn menu.
fn swing(combat: i8, token: ChaosToken) -> TestSession {
    let state = board(combat, token);
    let bat = state.in_play(INV, BASEBALL_BAT);
    let session = TestSession::new(state, TEST_REGISTRIES)
        .take(&TurnAction::ActivateAbility {
            investigator: INV,
            source: AbilitySource::InPlay(bat),
            address: AbilityAddress::Printed(0),
        })
        .commit(&[]);
    session.assert_at_turn_menu(INV);
    session
}

fn bat_in_play(state: &GameState) -> bool {
    state.investigators[&INV]
        .cards_in_play
        .iter()
        .any(|c| c.code == CardCode::new(BASEBALL_BAT))
}

#[test]
fn the_attack_gets_two_combat_and_deals_one_extra_damage() {
    // Combat 1 + 2 = 3 against fight 3 succeeds (combat 1 alone would not),
    // and the hit deals 1 + 1 = 2.
    let session = swing(1, ChaosToken::Numeric(0));
    assert_event!(session.events(), Event::SkillTestSucceeded { .. });
    assert_event!(session.events(), Event::EnemyDamaged { amount: 2, .. });
    assert_eq!(session.state().enemies[&ENEMY].damage, 2);
}

#[test]
fn a_skull_discards_the_bat_after_the_attack_resolves() {
    // Combat 3 + 2 = 5 succeeds through a `+0` skull, so the attack's damage
    // lands — and the Bat is discarded after it, as the test ends.
    let session = swing(3, ChaosToken::Skull);
    assert_event_sequence!(
        session.events(),
        Event::ChaosTokenRevealed {
            token: ChaosToken::Skull,
            ..
        },
        Event::EnemyDamaged { amount: 2, .. },
        Event::CardDiscarded { code, from: Zone::InPlay, to: DiscardPile::Investigator(INV) }
            if *code == CardCode::new(BASEBALL_BAT),
        Event::SkillTestEnded { .. },
    );
    assert!(!bat_in_play(session.state()));
    assert_eq!(
        session.state().investigators[&INV].discard,
        vec![CardCode::new(BASEBALL_BAT)]
    );
}

#[test]
fn an_auto_fail_discards_the_bat_after_the_failed_attack() {
    let session = swing(3, ChaosToken::AutoFail);
    assert_event_sequence!(
        session.events(),
        Event::SkillTestFailed { .. },
        Event::CardDiscarded { code, .. } if *code == CardCode::new(BASEBALL_BAT),
        Event::SkillTestEnded { .. },
    );
    assert_no_event!(session.events(), Event::EnemyDamaged { .. });
    assert!(!bat_in_play(session.state()));
}

#[test]
fn any_other_token_leaves_the_bat_in_play() {
    for token in [
        ChaosToken::Numeric(0),
        ChaosToken::Numeric(-2),
        ChaosToken::Cultist,
        ChaosToken::Tablet,
        ChaosToken::ElderThing,
        ChaosToken::ElderSign,
    ] {
        let session = swing(3, token);
        assert_no_event!(session.events(), Event::CardDiscarded { .. });
        assert!(
            bat_in_play(session.state()),
            "{token:?} leaves the Bat in play"
        );
    }
}

#[test]
fn the_bat_takes_both_hand_slots() {
    // Machete fills one `Hand` slot; playing the Bat needs both, so Machete
    // is discarded to make room without a choice (it is the only candidate).
    let session = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(
            seat(1)
                .at(HERE)
                .resources(20)
                .in_play([MACHETE])
                .hand([BASEBALL_BAT]),
        )
        .with_location(test_support::test_location(HERE.0, "Study"))
        .open_turn(INV)
        .session()
        .take(&TurnAction::PlayCard {
            investigator: INV,
            source: PlaySource::Hand(0),
        });
    session.assert_at_turn_menu(INV);
    let inv = &session.state().investigators[&INV];
    assert!(bat_in_play(session.state()));
    assert_eq!(inv.discard, vec![CardCode::new(MACHETE)]);
}
