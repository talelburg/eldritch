//! The step-wise [`TestSession`] (#945) over the real engine.
//!
//! Every test drives through the session's public steps and asserts only on
//! what a player or client could observe — the prompt (its options, anchors and
//! nature), the events, the board — never on the continuation stack.
//!
//! Synthetic probes only (ADR 0016). Each `_ts_*` code models one engine
//! primitive — an activated choice among board entities, a choice printed as
//! alternatives, a free ability a Fast window can offer — and none stands in
//! for a printed card. They live here, the one binary that reads them.

use card_dsl::card_data::{CardKind, CardMetadata, Class, SkillIcons};
use card_dsl::dsl::{self, Ability, EnemyTarget, InvestigatorTarget};
use game_core::action::{Action, InputResponse, PlayerAction};
use game_core::engine::enumerate::TurnAction;
use game_core::engine::{EngineOutcome, InputKind, OptionId, OptionTarget, PromptNature};
use game_core::event::Event;
use game_core::state::{
    AbilityAddress, AbilitySource, CardCode, CardInPlay, CardInstanceId, ChaosBag, ChaosToken,
    EnemyId, FastActorScope, FastWindowKind, GameStateBuilder, InvestigationPhaseFrame,
    InvestigationResume, InvestigatorId, LocationId, Phase, PhaseStep,
};
use game_core::test_support::{self, MockRegistry, TestSession};
use game_core::{assert_event, assert_event_count};

/// `[action]: Deal 1 damage to an enemy at your location.` — a choice among
/// board entities, so a Selection whose options anchor to the enemies.
const SELECT: &str = "_ts_select";
/// `[action]: Choose one — gain 1 resource, or gain 2 resources.` — two
/// alternatives printed on one card, so a Decision.
const DECIDE: &str = "_ts_decide";
/// `[free]: Gain 1 resource.` — a 0-action ability, which a Fast window offers.
const FREE: &str = "_ts_free";
/// An asset with 3 health and no abilities — a soaker for damage.
const SOAK: &str = "_ts_soak";
/// `[action]: Heal 1 damage from an investigator at your location.` — a choice
/// among investigators, whose options the engine does not anchor.
const HEAL: &str = "_ts_heal";

const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(10);
const SELECT_INST: CardInstanceId = CardInstanceId(1);
const DECIDE_INST: CardInstanceId = CardInstanceId(2);
const FREE_INST: CardInstanceId = CardInstanceId(3);
const SOAK_INST: CardInstanceId = CardInstanceId(4);
const HEAL_INST: CardInstanceId = CardInstanceId(5);
const FIRST: EnemyId = EnemyId(7);
const SECOND: EnemyId = EnemyId(8);

#[ctor::ctor(unsafe)]
fn install_mock_registry() {
    MockRegistry::new()
        .with_abilities(SELECT, || -> Vec<Ability> {
            vec![dsl::activated(
                1,
                vec![],
                dsl::deal_damage_to_enemy(EnemyTarget::chosen_at_your_location(), 1),
            )]
        })
        .with_abilities(DECIDE, || -> Vec<Ability> {
            vec![dsl::activated(
                1,
                vec![],
                dsl::choose_one([
                    (
                        "Gain 1 resource",
                        dsl::gain_resources(InvestigatorTarget::You, 1),
                    ),
                    (
                        "Gain 2 resources",
                        dsl::gain_resources(InvestigatorTarget::You, 2),
                    ),
                ]),
            )]
        })
        .with_card(soaker_metadata())
        .with_abilities(HEAL, || -> Vec<Ability> {
            vec![dsl::activated(
                1,
                vec![],
                dsl::heal_damage(InvestigatorTarget::chosen_at_your_location(), 1),
            )]
        })
        .with_abilities(FREE, || -> Vec<Ability> {
            vec![dsl::activated(
                0,
                vec![],
                dsl::gain_resources(InvestigatorTarget::You, 1),
            )]
        })
        .install();
}

fn soaker_metadata() -> CardMetadata {
    CardMetadata {
        code: SOAK.to_owned(),
        name: "Soaker".to_owned(),
        traits: vec![],
        text: None,
        back_name: None,
        back_text: None,
        pack_code: "_mock".to_owned(),
        weakness: false,
        kind: CardKind::Asset {
            class: Class::Neutral,
            cost: Some(0),
            xp: None,
            slots: vec![],
            health: Some(3),
            sanity: None,
            skill_icons: SkillIcons::default(),
            is_fast: false,
            deck_limit: 1,
            uses: None,
            play_only_during_turn: false,
        },
    }
}

/// One investigator at `HERE` with the two action probes in play, two
/// unengaged enemies at `HERE`, and a chaos bag of `Numeric(0)`.
fn board() -> GameStateBuilder {
    board_with(&[(SELECT, SELECT_INST), (DECIDE, DECIDE_INST)])
}

/// The turn-begins Fast window, open over the Investigation phase as the
/// engine leaves it just before the turn's actions, with the free-ability probe
/// in play so the window has something to offer.
fn turn_begins_window() -> GameStateBuilder {
    board_with(&[(FREE, FREE_INST)])
        .with_phase(Phase::Investigation)
        .with_active_investigator(INV)
        .with_turn_order([INV])
        .with_phase_anchor(InvestigationPhaseFrame {
            resume: InvestigationResume::TurnBegins,
        })
        .with_open_window(
            FastWindowKind::Phase(PhaseStep::InvestigatorTurnBegins),
            FastActorScope::Any,
        )
}

fn board_with(in_play: &[(&str, CardInstanceId)]) -> GameStateBuilder {
    let mut inv = test_support::test_investigator(1);
    for &(code, inst) in in_play {
        inv.cards_in_play
            .push(CardInPlay::enter_play(CardCode::new(code), inst));
    }
    let mut first = test_support::test_enemy(FIRST.0, "First");
    first.current_location = Some(HERE);
    let mut second = test_support::test_enemy(SECOND.0, "Second");
    second.current_location = Some(HERE);
    GameStateBuilder::new()
        .with_investigator_at(inv, HERE)
        .with_location(test_support::test_location(HERE.0, "Study"))
        .with_enemy(first)
        .with_enemy(second)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
}

/// An open turn with an enemy engaged, so any action provokes an attack of
/// opportunity, and the soaker probe in play to take some of it.
fn soak_board() -> GameStateBuilder {
    let mut inv = test_support::test_investigator(1);
    inv.cards_in_play
        .push(CardInPlay::enter_play(CardCode::new(SOAK), SOAK_INST));
    let mut attacker = test_support::test_enemy(FIRST.0, "Attacker");
    attacker.max_health = 9;
    GameStateBuilder::new()
        .with_investigator_at(inv, HERE)
        .with_location(test_support::test_location(HERE.0, "Study"))
        .with_enemy_engaged(attacker, INV)
        .open_turn(INV)
}

fn activate(inst: CardInstanceId) -> TurnAction {
    TurnAction::ActivateAbility {
        investigator: INV,
        source: AbilitySource::InPlay(inst),
        address: AbilityAddress::Printed(0),
    }
}

fn resources(session: &TestSession) -> u8 {
    session.state().investigators[&INV].resources
}

// ---- settling ----------------------------------------------------------

/// Construction settles an open turn to the turn menu: the prompt is anchored to
/// the acting investigator's turn control.
#[test]
fn settling_an_open_turn_yields_the_turn_menu() {
    let session = board().open_turn(INV).session();
    assert_eq!(
        session.prompt().target,
        Some(OptionTarget::TurnControl(INV))
    );
}

/// Construction settles an open Fast window by re-surfacing its prompt: the
/// skippable pick offering the free ability, anchored to its card.
#[test]
fn settling_an_open_fast_window_resurfaces_its_prompt() {
    let session = turn_begins_window().session();
    let prompt = session.prompt();
    assert!(prompt.skippable, "a Fast window is skippable: {prompt:?}");
    assert!(
        prompt
            .options
            .iter()
            .any(|o| o.target == Some(OptionTarget::CardInstance(FREE_INST))),
        "the free ability is offered on its card: {prompt:?}",
    );
}

/// A fixture whose top frame cannot re-surface its prompt is a stack the engine
/// never produces. Settling it fails loudly rather than handing back a session
/// parked nowhere.
#[test]
#[should_panic(expected = "cannot re-surface its prompt")]
fn settling_a_fixture_that_cannot_resurface_its_prompt_panics() {
    let _ = board()
        .open_turn(INV)
        .with_hand_size_discard_pending([INV])
        .session();
}

// ---- take ----------------------------------------------------------------

/// `take` applies a turn action and drains back to the turn menu; `events`
/// accumulates across steps while `finish` reports the last step alone.
#[test]
fn take_applies_a_turn_action_and_returns_to_the_turn_menu() {
    let session = board()
        .open_turn(INV)
        .session()
        .take(&TurnAction::Resource { investigator: INV })
        .take(&TurnAction::Resource { investigator: INV });

    assert_eq!(resources(&session), 7);
    assert_eq!(
        session.prompt().target,
        Some(OptionTarget::TurnControl(INV))
    );
    assert_event_count!(session.events(), 2, Event::ResourcesGained { .. });

    let last = session.finish();
    assert_event_count!(last.events, 1, Event::ResourcesGained { .. });
    assert!(matches!(last.outcome, EngineOutcome::AwaitingInput { .. }));
}

/// `take` refuses to run anywhere but the turn menu.
#[test]
#[should_panic(expected = "not at the turn menu")]
fn take_away_from_the_turn_menu_panics() {
    let _ = board()
        .open_turn(INV)
        .session()
        .take(&activate(SELECT_INST))
        .take(&TurnAction::Resource { investigator: INV });
}

// ---- pick ----------------------------------------------------------------

/// `pick` chooses the option anchored to the named entity, whatever its
/// position: the second enemy is offered second, and picking it by anchor
/// damages it and not the first.
#[test]
fn pick_chooses_the_anchored_option_whatever_its_position() {
    let session = board()
        .open_turn(INV)
        .session()
        .take(&activate(SELECT_INST));
    let prompt = session.prompt();
    assert_eq!(prompt.nature, PromptNature::Selection);
    assert_eq!(
        prompt
            .options
            .iter()
            .position(|o| o.target == Some(OptionTarget::Enemy(SECOND))),
        Some(1),
        "the second enemy is not the first option: {prompt:?}",
    );

    let session = session.pick(OptionTarget::Enemy(SECOND));

    assert_eq!(session.state().enemies[&SECOND].damage, 1);
    assert_eq!(session.state().enemies[&FIRST].damage, 0);
    assert_eq!(
        session.prompt().target,
        Some(OptionTarget::TurnControl(INV))
    );
}

/// No option anchored to the target is a test-authoring error.
#[test]
#[should_panic(expected = "no option anchored to")]
fn pick_of_an_unoffered_target_panics() {
    let _ = board()
        .open_turn(INV)
        .session()
        .take(&activate(SELECT_INST))
        .pick(OptionTarget::Enemy(EnemyId(99)));
}

/// `pick_unanchored` chooses the one option the engine left un-anchored: the
/// investigator's own entry in a soak prompt, beside the anchored soaker.
#[test]
fn pick_unanchored_chooses_the_one_option_without_an_anchor() {
    let session = soak_board()
        .session()
        .take(&TurnAction::Resource { investigator: INV });
    assert!(
        session
            .prompt()
            .options
            .iter()
            .any(|o| o.target == Some(OptionTarget::CardInstance(SOAK_INST))),
        "the attack of opportunity offers the soaker: {:?}",
        session.prompt(),
    );

    let session = session.pick_unanchored();

    assert_eq!(session.state().investigators[&INV].damage(), 1);
    assert_eq!(
        session.prompt().target,
        Some(OptionTarget::TurnControl(INV))
    );
}

/// Several un-anchored options leave nothing to tell them apart: the heal's
/// choice between two damaged investigators anchors neither.
#[test]
#[should_panic(expected = "several options are un-anchored")]
fn pick_unanchored_among_several_panics() {
    let mut mine = test_support::test_investigator(1);
    mine.investigator_card.accumulated_damage = 1;
    mine.cards_in_play
        .push(CardInPlay::enter_play(CardCode::new(HEAL), HEAL_INST));
    let mut other = test_support::test_investigator(2);
    other.investigator_card.accumulated_damage = 1;
    let _ = GameStateBuilder::new()
        .with_investigator_at(mine, HERE)
        .with_investigator_at(other, HERE)
        .with_location(test_support::test_location(HERE.0, "Study"))
        .open_turn(INV)
        .session()
        .take(&activate(HEAL_INST))
        .pick_unanchored();
}

// ---- pick_nth --------------------------------------------------------------

/// `pick_nth` answers a Decision by printed position.
#[test]
fn pick_nth_answers_a_decision_by_position() {
    let session = board()
        .open_turn(INV)
        .session()
        .take(&activate(DECIDE_INST));
    assert_eq!(session.prompt().nature, PromptNature::Decision);

    let session = session.pick_nth(1);

    assert_eq!(resources(&session), 7, "the second branch gains 2");
}

/// `pick_nth` on a Selection panics and sends the author to `pick`.
#[test]
#[should_panic(expected = "pick by target")]
fn pick_nth_on_a_selection_panics() {
    let _ = board()
        .open_turn(INV)
        .session()
        .take(&activate(SELECT_INST))
        .pick_nth(1);
}

// ---- confirm and skip ------------------------------------------------------

/// `confirm` acknowledges a pause: with the interactive acknowledge on, an
/// investigation stops at the skill-test result's `Confirm`, and confirming it
/// returns to the turn menu. The commit window before it is answered by the
/// reply policy set ahead of the step.
#[test]
fn confirm_acknowledges_a_pause() {
    let mut state = board().open_turn(INV).build();
    state.interactive_acknowledge = true;
    let session = TestSession::new(state)
        .resolve_choices(|c| {
            c.commit_cards(&[]);
        })
        .take(&TurnAction::Investigate { investigator: INV });
    assert_eq!(session.prompt().kind, InputKind::Confirm);

    let session = session.confirm();

    assert_event!(session.events(), Event::SkillTestEnded { .. });
    assert_eq!(
        session.prompt().target,
        Some(OptionTarget::TurnControl(INV))
    );
}

/// `skip` passes a skippable window; `pick` plays from it, and the window
/// re-opens after the play. Passing the turn-begins window opens the turn.
#[test]
fn skip_passes_a_fast_window_and_pick_plays_from_it() {
    let session = turn_begins_window()
        .session()
        .pick(OptionTarget::CardInstance(FREE_INST));
    assert_eq!(resources(&session), 6);
    assert!(session.prompt().skippable, "the window re-opens");

    let session = session.skip();

    assert_eq!(
        session.prompt().target,
        Some(OptionTarget::TurnControl(INV))
    );
}

// ---- apply and rejection -------------------------------------------------

/// A step the engine rejects leaves the session where it was: the reason reads
/// back through `expect_rejected`, and the prompt still stands.
#[test]
fn a_rejected_step_reads_back_and_leaves_the_prompt_standing() {
    let session = board().open_turn(INV).session().confirm();

    assert!(!session.expect_rejected().is_empty());
    assert_eq!(resources(&session), 5);
    assert_eq!(
        session.prompt().target,
        Some(OptionTarget::TurnControl(INV))
    );
}

/// A scripted reply the engine rejects partway through a step's drain leaves
/// the session at the prompt that reply answered, not where the step started:
/// the activation's action is already spent, so the prompt and the state agree
/// on standing at the choice, and the next step answers it.
#[test]
fn a_rejected_reply_in_the_drain_rests_at_the_prompt_it_answered() {
    let session = board()
        .open_turn(INV)
        .session()
        .resolve_choices(|c| {
            c.pick_single(OptionId(99));
        })
        .take(&activate(DECIDE_INST));

    assert!(!session.expect_rejected().is_empty());
    assert_eq!(session.prompt().nature, PromptNature::Decision);
    assert_eq!(session.state().investigators[&INV].actions_remaining, 2);

    let session = session.pick_nth(1);

    assert_eq!(resources(&session), 7);
    assert_eq!(
        session.prompt().target,
        Some(OptionTarget::TurnControl(INV))
    );
}

/// `expect_rejected` on a step that was accepted panics.
#[test]
#[should_panic(expected = "was not rejected")]
fn expect_rejected_on_an_accepted_step_panics() {
    let session = board()
        .open_turn(INV)
        .session()
        .take(&TurnAction::Resource { investigator: INV });
    let _ = session.expect_rejected();
}

/// `apply` takes a raw action and drains like any other step.
#[test]
fn apply_takes_a_raw_action() {
    let session = board()
        .open_turn(INV)
        .session()
        .take(&activate(DECIDE_INST))
        .apply(Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(OptionId(1)),
        }));

    assert_eq!(resources(&session), 7);
    assert_eq!(
        session.prompt().target,
        Some(OptionTarget::TurnControl(INV))
    );
}

/// The reply policy set before a step answers the prompts that step opens, so
/// the step drains past them to the next rest.
#[test]
fn the_reply_policy_set_before_a_step_answers_its_prompts() {
    let session = board()
        .open_turn(INV)
        .session()
        .resolve_choices(|c| {
            c.pick_single(OptionId(1));
        })
        .take(&activate(DECIDE_INST));

    assert_eq!(resources(&session), 7);
    assert_eq!(
        session.prompt().target,
        Some(OptionTarget::TurnControl(INV))
    );
}

/// `finish` refuses a session parked at a prompt nobody answered: the one-shot
/// shape's script ran short.
#[test]
#[should_panic(expected = "unanswered prompt")]
fn finish_at_an_unanswered_prompt_panics() {
    let _ = board()
        .open_turn(INV)
        .session()
        .take(&activate(SELECT_INST))
        .finish();
}
