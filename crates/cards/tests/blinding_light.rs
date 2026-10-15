//! Blinding Light 01066 end-to-end against the real corpus.
//!
//! Card text (`data/arkhamdb-snapshot/pack/core/core.json`, 01066):
//!
//! > <b>Evade.</b> This evasion attempt uses [willpower] instead of [agility].
//! > If you succeed, deal 1 damage to the enemy just evaded. If a [skull],
//! > [cultist], [tablet], [elder_thing], or [auto_fail] symbol is revealed
//! > during this evasion attempt, lose 1 action this turn.
//!
//! Ruling (<https://arkhamdb.com/card/01066>):
//!
//! > The *"If a symbol is revealed..."* effect triggers during Step 3 of the
//! > Skill Test Timing.
//!
//! Runs against `cards::TEST_REGISTRIES`, whose scenario registry is empty, so
//! the four scenario symbols initiate no chaos symbol effect of their own: any
//! action lost is Blinding Light's.

use cards::TEST_REGISTRIES;
use game_core::engine::enumerate::{self, PlaySource, TurnAction};
use game_core::engine::OptionTarget;
use game_core::event::Event;
use game_core::state::{
    CardCode, ChaosBag, ChaosToken, Enemy, EnemyId, GameState, GameStateBuilder, InvestigatorId,
    LocationId, SkillKind, TokenModifiers,
};
use game_core::test_support::{self, seat, Seat, TestSession};
use game_core::{assert_event, assert_event_sequence, assert_no_event};

const BLINDING_LIGHT: &str = "01066";
/// Frozen in Fear — *"The first time you perform one of the following actions
/// (move, fight, or evade) each round, it costs 1 additional action."*
const FROZEN_IN_FEAR: &str = "01164";
const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(10);
const ENEMY: EnemyId = EnemyId(100);
const OTHER: EnemyId = EnemyId(101);

const PLAY: TurnAction = TurnAction::PlayCard {
    investigator: INV,
    source: PlaySource::Hand(0),
};

/// An evade-3, health-3 enemy coded `id`.
fn enemy(id: EnemyId) -> Enemy {
    let mut enemy = test_support::test_enemy(id.0, "Ghoul");
    enemy.evade = 3;
    enemy.max_health = 3;
    enemy
}

/// The investigator: Blinding Light in hand with the resources to play it, at
/// willpower **5** and agility **1** with 3 actions. Against evade 3 the two
/// skills land on opposite sides (5 evades, 1 does not), so the test's outcome
/// says which skill the evasion used.
fn caster() -> Seat {
    seat(1)
        .at(HERE)
        .skill(SkillKind::Willpower, 5)
        .skill(SkillKind::Agility, 1)
        .hand([BLINDING_LIGHT])
        .resources(2)
        .actions(3)
}

/// `investigator`'s turn, engaged with `engaged` evade-3 enemies, and a chaos
/// bag holding only `token`.
fn staged(investigator: Seat, token: ChaosToken, engaged: &[EnemyId]) -> GameStateBuilder {
    let mut builder = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(investigator)
        .with_location(test_support::test_location(HERE.0, "Study"))
        .open_turn(INV)
        .with_chaos_bag(ChaosBag::new([token]))
        .with_token_modifiers(TokenModifiers::default());
    for &id in engaged {
        builder = builder.with_enemy_engaged(enemy(id), INV);
    }
    builder
}

/// The [`caster`]'s turn: see [`staged`].
fn board(token: ChaosToken, engaged: &[EnemyId]) -> GameState {
    staged(caster(), token, engaged).build()
}

/// Play Blinding Light against the one engaged [`ENEMY`], committing nothing,
/// and come back to the turn menu.
fn play(token: ChaosToken) -> TestSession {
    let session = TestSession::new(board(token, &[ENEMY]), TEST_REGISTRIES)
        .take(&PLAY)
        .commit(&[]);
    session.assert_at_turn_menu(INV);
    session
}

fn actions(state: &GameState) -> u8 {
    state.investigators[&INV].actions_remaining
}

#[test]
fn playing_it_evades_testing_willpower() {
    // Willpower 5 vs evade 3 succeeds (agility 1 would not).
    let session = play(ChaosToken::Numeric(0));
    assert_event!(
        session.events(),
        Event::SkillTestStarted {
            skill: SkillKind::Willpower,
            ..
        }
    );
    assert_event!(session.events(), Event::SkillTestSucceeded { .. });
    assert_event!(session.events(), Event::EnemyDisengaged { enemy, .. } if *enemy == ENEMY);
    let evaded = &session.state().enemies[&ENEMY];
    assert_eq!(evaded.engaged_with, None);
    assert!(evaded.exhausted);
    // The play is the one action spent, and the event is discarded.
    assert_eq!(actions(session.state()), 2);
    assert_eq!(
        session.state().investigators[&INV].discard,
        vec![CardCode::new(BLINDING_LIGHT)]
    );
}

#[test]
fn success_deals_one_damage_to_the_enemy_just_evaded() {
    let session = play(ChaosToken::Numeric(0));
    assert_eq!(session.state().enemies[&ENEMY].damage, 1);
    // The damage lands once the evasion has broken the engagement.
    assert_event_sequence!(
        session.events(),
        Event::EnemyDisengaged { .. },
        Event::EnemyDamaged { enemy, amount: 1, .. } if *enemy == ENEMY,
    );
}

#[test]
fn with_two_engaged_enemies_only_the_one_evaded_is_damaged() {
    let session = TestSession::new(
        board(ChaosToken::Numeric(0), &[ENEMY, OTHER]),
        TEST_REGISTRIES,
    )
    .take(&PLAY)
    .pick(OptionTarget::Enemy(OTHER))
    .commit(&[]);
    session.assert_at_turn_menu(INV);
    let state = session.state();
    assert_eq!(state.enemies[&OTHER].damage, 1);
    assert_eq!(state.enemies[&OTHER].engaged_with, None);
    assert_eq!(state.enemies[&ENEMY].damage, 0);
    assert_eq!(state.enemies[&ENEMY].engaged_with, Some(INV));
}

#[test]
fn a_failed_evasion_deals_no_damage() {
    // Willpower 5 - 3 = 2 against evade 3 fails.
    let session = play(ChaosToken::Numeric(-3));
    assert_event!(session.events(), Event::SkillTestFailed { .. });
    assert_no_event!(session.events(), Event::EnemyDamaged { .. });
    let enemy = &session.state().enemies[&ENEMY];
    assert_eq!(enemy.damage, 0);
    assert_eq!(enemy.engaged_with, Some(INV));
}

#[test]
fn each_listed_symbol_loses_one_action_at_reveal() {
    for token in [
        ChaosToken::Skull,
        ChaosToken::Cultist,
        ChaosToken::Tablet,
        ChaosToken::ElderThing,
        ChaosToken::AutoFail,
    ] {
        let session = play(token);
        // 3 actions, 1 spent on the play, 1 lost.
        assert_eq!(actions(session.state()), 1, "{token:?} loses 1 action");
        // Step 3 of Skill Test Timing: the action is lost after the token is
        // revealed and before the test's result is determined.
        assert_event_sequence!(
            session.events(),
            Event::ChaosTokenRevealed { token: t, .. } if *t == token,
            Event::ActionsRemainingChanged { new_count: 1, .. },
            Event::SkillTestEnded { .. },
        );
    }
}

/// Step 3 of Skill Test Timing comes before the result is determined, so the
/// action is lost before the test fails — `[auto_fail]` fails the evasion.
#[test]
fn the_action_is_lost_before_the_result_is_determined() {
    let session = play(ChaosToken::AutoFail);
    assert_event_sequence!(
        session.events(),
        Event::ActionsRemainingChanged { new_count: 1, .. },
        Event::SkillTestFailed { .. },
    );
    assert_no_event!(session.events(), Event::EnemyDamaged { .. });
}

#[test]
fn any_other_token_loses_no_action() {
    for token in [ChaosToken::Numeric(0), ChaosToken::ElderSign] {
        let session = play(token);
        assert_eq!(actions(session.state()), 2, "{token:?} loses no action");
        assert_no_event!(
            session.events(),
            Event::ActionsRemainingChanged { new_count: 1, .. }
        );
    }
}

#[test]
fn the_lost_action_saturates_at_zero() {
    // With the play as the last action, there is nothing left to lose.
    let session = staged(caster().actions(1), ChaosToken::Skull, &[ENEMY])
        .session()
        .take(&PLAY)
        .commit(&[]);
    assert_eq!(actions(session.state()), 0);
}

/// Playing it performs an evade action, so Frozen in Fear's surcharge on
/// *"the first time you perform one of the following actions (move, fight, or
/// evade) each round"* reaches it. Its ruling (<https://arkhamdb.com/card/01164>)
/// names the designators: *"Also applies to \[action\] card abilities with
/// action designators (**Move**, **Fight**, **Evade**)."* The play costs its 1
/// action and 1 more.
#[test]
fn under_frozen_in_fear_the_play_is_surcharged_as_an_evade_action() {
    let session = staged(
        caster().threat_area([FROZEN_IN_FEAR]),
        ChaosToken::Numeric(0),
        &[ENEMY],
    )
    .session()
    .take(&PLAY)
    .commit(&[]);
    session.assert_at_turn_menu(INV);
    assert_event!(session.events(), Event::SkillTestSucceeded { .. });
    assert_eq!(
        actions(session.state()),
        1,
        "3 actions, 2 spent on the play"
    );
}

/// An evade action can only be performed against an enemy engaged with you
/// (`glossary/Evade_Action.md`: *"an investigator can only perform an evade
/// action against an enemy engaged with him or her"*), so with none engaged
/// Blinding Light is neither offered nor playable, and nothing is paid.
#[test]
fn it_cannot_be_played_without_an_engaged_enemy() {
    // An unengaged enemy at the location is no evade target.
    let mut bystander = enemy(ENEMY);
    bystander.current_location = Some(HERE);
    let state = staged(caster(), ChaosToken::Numeric(0), &[])
        .with_enemy(bystander)
        .build();
    assert!(
        !enumerate::legal_actions(&state, TEST_REGISTRIES.cards).contains(&PLAY),
        "Blinding Light is not offered with no engaged enemy"
    );
    let session = TestSession::new(state, TEST_REGISTRIES);
    let reason = session.assert_rejects(|s| s.submit_off_menu(&PLAY));
    assert!(reason.contains("Evade"), "{reason}");
}
