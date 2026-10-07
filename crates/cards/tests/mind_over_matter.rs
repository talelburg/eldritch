//! #322 integration: Mind over Matter 01036 — substituting Intellect for a
//! Combat/Agility test, the intellect-icon commit rule, the play-timing gate,
//! and the weapon-bonus interaction, end-to-end against the real
//! `cards::REGISTRY`.
//!
//! Own process → installs `cards::REGISTRY`.

use card_dsl::card_data::UseKind;
use cards::REGISTRY;
use game_core::action::{Action, InputResponse, PlayerAction};
use game_core::assert_event;
use game_core::engine::enumerate::TurnAction;
use game_core::engine::{EngineOutcome, InputKind, OptionId, OptionTarget};
use game_core::event::Event;
use game_core::state::{
    AbilityAddress, AbilitySource, CardCode, CardInPlay, CardInstanceId, ChaosBag, ChaosToken,
    EnemyId, FastActorScope, FastWindowKind, GameState, GameStateBuilder, InvestigatorId,
    LocationId, Phase, PhaseStep,
};
use game_core::test_support::{self, TestSession};

const MOM: &str = "01036";
const OVERPOWER: &str = "01091"; // combat skill icons
const SPECIAL: &str = "01006"; // .38 Special — Fight weapon
const INV: InvestigatorId = InvestigatorId(1);
const LOC: LocationId = LocationId(10);
const ENEMY: EnemyId = EnemyId(100);
const WEAPON_INST: CardInstanceId = CardInstanceId(900);

#[ctor::ctor(unsafe)]
fn install() {
    test_support::install_registry_with_test_cards(REGISTRY);
}

/// Active investigator at `LOC` engaged with a fight-3 enemy. `combat` /
/// `intellect` are set so a `Numeric(0)` draw makes substitution flip the
/// outcome (combat fails the fight-3 test, intellect passes). `hand` is the
/// investigator's starting hand.
fn board(combat: i8, intellect: i8, hand: Vec<CardCode>) -> GameState {
    let mut inv = test_support::test_investigator(1);
    inv.skills.combat = combat;
    inv.skills.intellect = intellect;
    inv.current_location = Some(LOC);
    inv.hand = hand;

    let mut enemy = test_support::test_enemy(100, "Ghoul");
    enemy.fight = 3;
    enemy.max_health = 5; // survives so we can read `damage`
    enemy.engaged_with = Some(INV);
    enemy.current_location = Some(LOC); // co-located: Fight is location-gated (#401)

    GameStateBuilder::new()
        .with_investigator_at(inv, LOC)
        .with_location(test_support::test_location(10, "Study"))
        .with_enemy(enemy)
        .open_turn(INV)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .build()
}

fn play_card(session: TestSession, hand_index: u8) -> TestSession {
    session.take(&TurnAction::PlayCard {
        investigator: INV,
        hand_index,
    })
}

fn fight(session: TestSession) -> TestSession {
    session.take(&TurnAction::Fight {
        investigator: INV,
        enemy: ENEMY,
    })
}

/// Answer the substitution prompt, a Decision (ADR 0015): printed option 0
/// uses the substitute skill, 1 keeps the printed one.
fn substitute(session: TestSession, use_substitute: bool) -> TestSession {
    session.pick_nth(usize::from(!use_substitute))
}

/// Answer the commit window with the hand indices in `indices`.
fn commit(indices: Vec<u32>) -> Action {
    Action::Player(PlayerAction::ResolveInput {
        response: InputResponse::PickMultiple {
            selected: indices.into_iter().map(OptionId).collect(),
        },
    })
}

fn at_turn_menu(session: &TestSession) -> bool {
    session.prompt().target == Some(OptionTarget::TurnControl(INV))
}

#[test]
fn play_then_fight_substituting_succeeds_via_intellect() {
    // combat 1 fails fight-3; intellect 4 passes.
    let s = play_card(TestSession::new(board(1, 4, vec![CardCode::new(MOM)])), 0);
    assert!(
        at_turn_menu(&s),
        "MoM plays (Fast, your turn) and returns to the open-turn menu"
    );
    assert!(
        s.state().investigators[&INV]
            .discard
            .contains(&CardCode::new(MOM)),
        "event discarded after play",
    );

    let s = fight(s);
    assert!(!at_turn_menu(&s), "substitution prompt");
    let s = substitute(s, true); // use Intellect
    assert_eq!(s.prompt().kind, InputKind::PickMultiple, "commit window");
    let s = s.apply(commit(vec![]));
    assert!(at_turn_menu(&s));
    assert_event!(s.events(), Event::SkillTestSucceeded { .. });
    assert!(
        s.state().enemies[&ENEMY].damage >= 1,
        "Fight dealt damage on success"
    );
}

#[test]
fn fight_declining_substitution_fails_on_combat() {
    let s = play_card(TestSession::new(board(1, 4, vec![CardCode::new(MOM)])), 0);
    let s = fight(s).resolve_choices(|c| {
        c.commit_cards(&[]);
    });
    let s = substitute(s, false); // keep Combat
    assert!(at_turn_menu(&s));
    assert_event!(s.events(), Event::SkillTestFailed { .. });
    assert_eq!(
        s.state().enemies[&ENEMY].damage,
        0,
        "combat 1 < fight 3 → no damage"
    );
}

#[test]
fn substituted_intellect_test_rejects_a_committed_combat_icon() {
    // FAQ: a substituted test *is* an Intellect test, so ST.2's icon
    // eligibility is judged against Intellect — Overpower's two [combat] icons
    // are not appropriate and the commit is rejected outright (#763). This is
    // the substitution's sharpest observable consequence at the commit window:
    // the same commit is legal on the un-substituted Combat test.
    // Hand: [MoM, Overpower]. Play MoM (idx 0) → hand becomes [Overpower] (idx 0).
    let s = play_card(
        TestSession::new(board(
            4,
            2,
            vec![CardCode::new(MOM), CardCode::new(OVERPOWER)],
        )),
        0,
    );
    let s = substitute(fight(s), true); // use Intellect → it's now an Intellect test
    assert_eq!(s.prompt().kind, InputKind::PickMultiple, "commit window");
    let s = s.apply(commit(vec![0])); // commit Overpower (combat icons)
    assert!(!s.expect_rejected().is_empty());
    // Validate-first: the card is still in hand and the test still paused.
    assert_eq!(
        s.state().investigators[&INV].hand,
        vec![CardCode::new(OVERPOWER)],
    );
    assert_eq!(
        s.prompt().kind,
        InputKind::PickMultiple,
        "still at the commit window"
    );

    // Retrying with an empty commit resolves the test: intellect 2 < fight 3.
    let s = s.apply(commit(vec![]));
    assert!(at_turn_menu(&s));
    assert_event!(s.events(), Event::SkillTestFailed { .. });
    assert_eq!(
        s.state().enemies[&ENEMY].damage,
        0,
        "intellect 2 < fight 3, and no icons were admitted",
    );
}
#[test]
fn mind_over_matter_rejected_outside_your_turn() {
    let mut inv = test_support::test_investigator(1);
    inv.current_location = Some(LOC);
    inv.hand = vec![CardCode::new(MOM)];
    let state = GameStateBuilder::new()
        .with_phase(Phase::Mythos)
        .with_investigator_at(inv, LOC)
        .with_location(test_support::test_location(10, "Study"))
        .with_active_investigator(INV)
        .with_open_window(
            FastWindowKind::Phase(PhaseStep::MythosAfterDraws),
            FastActorScope::Any,
        )
        .build();
    let r = test_support::dispatch_turn_action_unchecked(
        state,
        &TurnAction::PlayCard {
            investigator: INV,
            hand_index: 0,
        },
    );
    assert!(
        matches!(r.outcome, EngineOutcome::Rejected { .. }),
        "'Play only during your turn' rejects MoM in the Mythos window: {:?}",
        r.outcome,
    );
}

#[test]
fn weapon_fight_substituting_uses_intellect_and_keeps_weapon_damage() {
    // .38 Special: "+1 [combat] (no clue here), this attack deals +1 damage."
    // combat 1 + weapon +1 = 2 < fight 3 (would fail); substituting drops the
    // weapon's combat bonus and tests intellect 4 ≥ 3 → success, and the
    // weapon's +1 damage is still dealt (base 1 + 1 = 2).
    let mut inv = test_support::test_investigator(1);
    inv.skills.combat = 1;
    inv.skills.intellect = 4;
    inv.current_location = Some(LOC);
    inv.hand = vec![CardCode::new(MOM)];
    let mut weapon = CardInPlay::enter_play(CardCode::new(SPECIAL), WEAPON_INST);
    weapon.uses.insert(UseKind::Ammo, 4);
    inv.cards_in_play.push(weapon);

    let mut enemy = test_support::test_enemy(100, "Ghoul");
    enemy.fight = 3;
    enemy.max_health = 5;
    enemy.engaged_with = Some(INV);
    enemy.current_location = Some(LOC); // co-located: Fight is location-gated (#401)

    let state = GameStateBuilder::new()
        .with_investigator_at(inv, LOC)
        .with_location(test_support::test_location(10, "Study"))
        .with_enemy(enemy)
        .open_turn(INV)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .build();

    let s = play_card(TestSession::new(state), 0) // MoM
        .take(&TurnAction::ActivateAbility {
            investigator: INV,
            source: AbilitySource::InPlay(WEAPON_INST),
            address: AbilityAddress::Printed(0),
        });
    assert!(!at_turn_menu(&s), "substitution prompt");
    let s = s.resolve_choices(|c| {
        c.commit_cards(&[]);
    });
    let s = substitute(s, true); // use Intellect (drops the +combat weapon bonus)
    assert!(at_turn_menu(&s));
    assert_event!(s.events(), Event::SkillTestSucceeded { .. });
    assert_eq!(
        s.state().enemies[&ENEMY].damage,
        2,
        ".38 Special's bonus damage is kept (1 + 1) even when substituting",
    );
}
