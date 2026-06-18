//! #322 integration: Mind over Matter 01036 — substituting Intellect for a
//! Combat/Agility test, the intellect-icon commit rule, the play-timing gate,
//! and the weapon-bonus interaction, end-to-end against the real
//! `cards::REGISTRY`.
//!
//! Own process → installs `cards::REGISTRY`.

use std::sync::Once;

use game_core::engine::EngineOutcome;
use game_core::event::Event;
use game_core::state::{
    CardCode, CardInPlay, CardInstanceId, ChaosBag, ChaosToken, EnemyId, FastActorScope,
    InvestigatorId, LocationId, Phase, PhaseStep, WindowKind,
};
use game_core::test_support::{test_enemy, test_investigator, test_location, GameStateBuilder};
use game_core::{apply, assert_event, Action, InputResponse, OptionId, PlayerAction};

const MOM: &str = "01036";
const OVERPOWER: &str = "01091"; // combat skill icons
const SPECIAL: &str = "01006"; // .38 Special — Fight weapon
const INV: InvestigatorId = InvestigatorId(1);
const LOC: LocationId = LocationId(10);
const ENEMY: EnemyId = EnemyId(100);
const WEAPON_INST: CardInstanceId = CardInstanceId(900);

static INSTALL: Once = Once::new();
fn install() {
    INSTALL.call_once(|| {
        let _ = game_core::card_registry::install(cards::REGISTRY);
    });
}

/// Active investigator at `LOC` engaged with a fight-3 enemy. `combat` /
/// `intellect` are set so a `Numeric(0)` draw makes substitution flip the
/// outcome (combat fails the fight-3 test, intellect passes). `hand` is the
/// investigator's starting hand.
fn board(combat: i8, intellect: i8, hand: Vec<CardCode>) -> game_core::GameState {
    install();
    let mut inv = test_investigator(1);
    inv.skills.combat = combat;
    inv.skills.intellect = intellect;
    inv.current_location = Some(LOC);
    inv.hand = hand;

    let mut enemy = test_enemy(100, "Ghoul");
    enemy.fight = 3;
    enemy.max_health = 5; // survives so we can read `damage`
    enemy.engaged_with = Some(INV);

    GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator_at(inv, LOC)
        .with_location(test_location(10, "Study"))
        .with_enemy(enemy)
        .with_active_investigator(INV)
        .with_turn_order([INV])
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .build()
}

fn play_card(state: game_core::GameState, hand_index: u8) -> game_core::engine::ApplyResult {
    apply(
        state,
        Action::Player(PlayerAction::PlayCard {
            investigator: INV,
            hand_index,
        }),
    )
}

fn fight(state: game_core::GameState) -> game_core::engine::ApplyResult {
    apply(
        state,
        Action::Player(PlayerAction::Fight {
            investigator: INV,
            enemy: ENEMY,
        }),
    )
}

fn pick(state: game_core::GameState, opt: u32) -> game_core::engine::ApplyResult {
    apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(OptionId(opt)),
        }),
    )
}

fn commit(state: game_core::GameState, indices: Vec<u32>) -> game_core::engine::ApplyResult {
    apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::CommitCards { indices },
        }),
    )
}

#[test]
fn play_then_fight_substituting_succeeds_via_intellect() {
    // combat 1 fails fight-3; intellect 4 passes.
    let r = play_card(board(1, 4, vec![CardCode::new(MOM)]), 0);
    assert_eq!(r.outcome, EngineOutcome::Done, "MoM plays (Fast, your turn)");
    assert!(
        r.state.investigators[&INV]
            .discard
            .contains(&CardCode::new(MOM)),
        "event discarded after play",
    );

    let r = fight(r.state);
    assert!(matches!(r.outcome, EngineOutcome::AwaitingInput { .. }), "substitution prompt");
    let r = pick(r.state, 0); // use Intellect
    assert!(matches!(r.outcome, EngineOutcome::AwaitingInput { .. }), "commit window");
    let r = commit(r.state, vec![]);
    assert_eq!(r.outcome, EngineOutcome::Done);
    assert_event!(r.events, Event::SkillTestSucceeded { .. });
    assert!(r.state.enemies[&ENEMY].damage >= 1, "Fight dealt damage on success");
}

#[test]
fn fight_declining_substitution_fails_on_combat() {
    let r = play_card(board(1, 4, vec![CardCode::new(MOM)]), 0);
    let r = fight(r.state);
    let r = pick(r.state, 1); // keep Combat
    let r = commit(r.state, vec![]);
    assert_eq!(r.outcome, EngineOutcome::Done);
    assert_event!(r.events, Event::SkillTestFailed { .. });
    assert_eq!(r.state.enemies[&ENEMY].damage, 0, "combat 1 < fight 3 → no damage");
}

#[test]
fn substituted_intellect_test_ignores_committed_combat_icons() {
    // FAQ: a substituted test is an Intellect test, so only Intellect/Wild
    // icons count. intellect 2 < fight 3 fails; committing Overpower (two
    // [combat] icons) does NOT help — combat icons don't count for an Intellect
    // test, so the total stays 2 and the test fails. (The engine sums only
    // matching icons; it does not *reject* an off-icon commit — RR ST.2
    // eligibility enforcement is a separate, pre-existing gap.)
    // Hand: [MoM, Overpower]. Play MoM (idx 0) → hand becomes [Overpower] (idx 0).
    let r = play_card(
        board(4, 2, vec![CardCode::new(MOM), CardCode::new(OVERPOWER)]),
        0,
    );
    let r = fight(r.state);
    let r = pick(r.state, 0); // use Intellect → it's now an Intellect test
    assert!(matches!(r.outcome, EngineOutcome::AwaitingInput { .. }), "commit window");
    let r = commit(r.state, vec![0]); // commit Overpower (combat icons)
    assert_eq!(r.outcome, EngineOutcome::Done);
    assert_event!(r.events, Event::SkillTestFailed { .. });
    assert_eq!(
        r.state.enemies[&ENEMY].damage, 0,
        "combat icons don't count toward the substituted Intellect test",
    );
}

#[test]
fn mind_over_matter_rejected_outside_your_turn() {
    install();
    let mut inv = test_investigator(1);
    inv.current_location = Some(LOC);
    inv.hand = vec![CardCode::new(MOM)];
    let state = GameStateBuilder::new()
        .with_phase(Phase::Mythos)
        .with_investigator_at(inv, LOC)
        .with_location(test_location(10, "Study"))
        .with_active_investigator(INV)
        .with_open_window(
            WindowKind::PlayerWindow(PhaseStep::MythosAfterDraws),
            FastActorScope::Any,
        )
        .build();
    let r = play_card(state, 0);
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
    install();
    let mut inv = test_investigator(1);
    inv.skills.combat = 1;
    inv.skills.intellect = 4;
    inv.current_location = Some(LOC);
    inv.hand = vec![CardCode::new(MOM)];
    let mut weapon = CardInPlay::enter_play(CardCode::new(SPECIAL), WEAPON_INST);
    weapon.uses.insert(game_core::card_data::UseKind::Ammo, 4);
    inv.cards_in_play.push(weapon);

    let mut enemy = test_enemy(100, "Ghoul");
    enemy.fight = 3;
    enemy.max_health = 5;
    enemy.engaged_with = Some(INV);

    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator_at(inv, LOC)
        .with_location(test_location(10, "Study"))
        .with_enemy(enemy)
        .with_active_investigator(INV)
        .with_turn_order([INV])
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .build();

    let r = play_card(state, 0); // MoM
    let r = apply(
        r.state,
        Action::Player(PlayerAction::ActivateAbility {
            investigator: INV,
            instance_id: WEAPON_INST,
            ability_index: 0,
        }),
    );
    assert!(matches!(r.outcome, EngineOutcome::AwaitingInput { .. }), "substitution prompt");
    let r = pick(r.state, 0); // use Intellect (drops the +combat weapon bonus)
    let r = commit(r.state, vec![]);
    assert_eq!(r.outcome, EngineOutcome::Done);
    assert_event!(r.events, Event::SkillTestSucceeded { .. });
    assert_eq!(
        r.state.enemies[&ENEMY].damage, 2,
        ".38 Special's bonus damage is kept (1 + 1) even when substituting",
    );
}
