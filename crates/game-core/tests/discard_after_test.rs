//! [`NativeCx::discard_after_this_test`]: a native marks the in-flight test
//! with a card in play, and the test's teardown discards it.
//!
//! Baseball Bat 01074 is the corpus consumer (*"If a \[skull\] or \[auto_fail\]
//! symbol is revealed during this attack, discard Baseball Bat after the
//! attack resolves."*, `data/arkhamdb-snapshot/pack/core/core.json`), tested
//! against the real registry in `crates/cards/tests/integration/baseball_bat.rs`. This
//! binary pins the two teardowns that honour the mark: the ST.8 one, and the
//! abandonment of a test whose target left play, which no Core card
//! reaches between the ST.3 mark and ST.6 — so the abandoning native here
//! takes the target out of play itself.
//!
//! Synthetic cards only (ADR 0016). Its own integration-test binary, so its
//! `MockRegistry` gets a process to itself.
//!
//! [`NativeCx::discard_after_this_test`]: game_core::engine::NativeCx::discard_after_this_test

use std::sync::LazyLock;

use card_dsl::card_data::{CardKind, CardMetadata, Class, SkillIcons, Slot};
use card_dsl::dsl::{self, Ability, ChaosSymbol, RevealClause};
use game_core::engine::enumerate::TurnAction;
use game_core::engine::evaluator::EvalContext;
use game_core::engine::{EngineOutcome, NativeCx, Registries};
use game_core::event::Event;
use game_core::scenario::ScenarioRegistry;
use game_core::state::{
    AbilityAddress, AbilitySource, CardCode, ChaosBag, ChaosToken, DifficultyBasis, EnemyId,
    GameState, GameStateBuilder, InvestigatorId, LocationId, SkillKind, Zone,
};
use game_core::test_support::{self, seat, MockRegistry, SeatLookup as _, TestSession};
use game_core::{assert_event, assert_event_sequence, assert_no_event};

/// Mock weapon: *"\[action\]: Fight. If a \[skull\] is revealed during this
/// attack, discard this card after the attack resolves."*
const MARKING: &str = "MOCK-DA-MARK";
/// Mock weapon: as [`MARKING`], and the same clause also discards the attacked
/// enemy, so the test is abandoned before ST.6.
const ABANDONING: &str = "MOCK-DA-ABANDON";

const MARK_TAG: &str = "MOCK-DA-MARK:mark";
const ABANDON_TAG: &str = "MOCK-DA-ABANDON:mark-and-discard-target";

const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(10);
const ENEMY: EnemyId = EnemyId(100);

fn weapon_metadata(code: &str) -> CardMetadata {
    CardMetadata {
        code: code.to_owned(),
        name: "Mock Weapon".to_owned(),
        traits: vec!["Item".to_owned(), "Weapon".to_owned()],
        text: None,
        back_name: None,
        back_text: None,
        pack_code: "_mock".to_owned(),
        weakness: None,
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

/// A costless `[action]` Fight whose `[skull]` clause runs `tag`.
fn fight_with_skull_clause(tag: &str) -> Vec<Ability> {
    vec![dsl::activated_as(
        dsl::fight(0i8, 0u8).with_reveal_clause(RevealClause {
            symbols: vec![ChaosSymbol::Skull],
            effect: Box::new(dsl::native(tag)),
        }),
        1,
        vec![],
        dsl::seq([]),
    )]
}

fn mark(cx: &mut NativeCx, ctx: &EvalContext) -> EngineOutcome {
    let source = ctx
        .source_instance()
        .expect("the weapon is the clause's source");
    cx.discard_after_this_test(source);
    EngineOutcome::Done
}

fn mark_and_discard_target(cx: &mut NativeCx, ctx: &EvalContext) -> EngineOutcome {
    let source = ctx
        .source_instance()
        .expect("the weapon is the clause's source");
    let Some(DifficultyBasis::Fight(target)) =
        cx.state().current_skill_test().map(|t| t.difficulty_basis)
    else {
        panic!("the clause fires on a Fight's test");
    };
    cx.discard_after_this_test(source);
    cx.discard_from_play(target)
        .expect("the attacked enemy is in play");
    EngineOutcome::Done
}

fn registries() -> Registries {
    static REGISTRIES: LazyLock<Registries> = LazyLock::new(|| Registries {
        cards: MockRegistry::new()
            .with_card(weapon_metadata(MARKING))
            .with_card(weapon_metadata(ABANDONING))
            .with_abilities(MARKING, || fight_with_skull_clause(MARK_TAG))
            .with_abilities(ABANDONING, || fight_with_skull_clause(ABANDON_TAG))
            .with_native_effect(MARK_TAG, mark)
            .with_native_effect(ABANDON_TAG, mark_and_discard_target)
            .build(),
        scenarios: &ScenarioRegistry::EMPTY,
    });
    *REGISTRIES
}

/// Board: `weapon` in play under a combat-5 investigator engaged with a
/// fight-3, health-3 enemy, drawing only `token`.
fn board(weapon: &str, token: ChaosToken) -> GameState {
    let mut enemy = test_support::test_enemy(ENEMY.0, "Ghoul");
    enemy.fight = 3;
    enemy.max_health = 3;
    GameStateBuilder::new()
        .with_registries(registries())
        .with_seat(
            seat(1)
                .at(HERE)
                .skill(SkillKind::Combat, 5)
                .in_play([weapon]),
        )
        .with_location(test_support::test_location(HERE.0, "Here"))
        .with_enemy_engaged(enemy, INV)
        .open_turn(INV)
        .with_chaos_bag(ChaosBag::new([token]))
        .build()
}

/// Fight with `weapon`, committing nothing, back to the turn menu.
fn attack(weapon: &str, token: ChaosToken) -> TestSession {
    let state = board(weapon, token);
    let card = state.in_play(INV, weapon);
    let session = TestSession::new(state, registries())
        .take(&TurnAction::ActivateAbility {
            investigator: INV,
            source: AbilitySource::InPlay(card),
            address: AbilityAddress::Printed(0),
        })
        .commit(&[]);
    session.assert_at_turn_menu(INV);
    session
}

#[test]
fn a_marked_card_is_discarded_as_the_test_ends_after_its_results() {
    let session = attack(MARKING, ChaosToken::Skull);
    assert_event_sequence!(
        session.events(),
        Event::SkillTestSucceeded { .. },
        Event::EnemyDamaged { .. },
        Event::CardDiscarded { code, from: Zone::InPlay, .. } if *code == CardCode::new(MARKING),
        Event::SkillTestEnded { .. },
    );
    assert_eq!(
        session.state().investigators[&INV].discard,
        vec![CardCode::new(MARKING)]
    );
}

#[test]
fn an_unmarked_test_discards_nothing() {
    let session = attack(MARKING, ChaosToken::Numeric(0));
    assert_no_event!(session.events(), Event::CardDiscarded { .. });
    assert!(session.state().investigators[&INV].discard.is_empty());
}

#[test]
fn a_test_abandoned_because_its_target_left_play_still_discards_the_marked_card() {
    let session = attack(ABANDONING, ChaosToken::Skull);
    // Abandoned: no verdict, no damage — and the marked card goes all the same.
    assert_no_event!(session.events(), Event::SkillTestSucceeded { .. });
    assert_no_event!(session.events(), Event::SkillTestFailed { .. });
    assert_event!(session.events(), Event::SkillTestEnded { .. });
    assert_event_sequence!(
        session.events(),
        Event::CardDiscarded { code, from: Zone::InPlay, .. } if *code == CardCode::new(ABANDONING),
        Event::SkillTestEnded { .. },
    );
    assert_eq!(
        session.state().investigators[&INV].discard,
        vec![CardCode::new(ABANDONING)]
    );
}
