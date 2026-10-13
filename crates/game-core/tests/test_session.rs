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

use std::cell::Cell;
use std::rc::Rc;

use card_dsl::dsl::{self, Ability, EnemyTarget, InvestigatorTarget};
use game_core::action::{Action, InputResponse, PlayerAction};
use game_core::engine::enumerate::TurnAction;
use game_core::engine::{
    EngineOutcome, InputKind, InputRequest, OptionId, OptionTarget, PromptNature,
};
use game_core::event::Event;
use game_core::state::{
    AbilityAddress, AbilitySource, CardCode, CardInPlay, CardInstanceId, ChaosBag, ChaosToken,
    EnemyId, FastActorScope, FastWindowKind, GameState, GameStateBuilder, InvestigationPhaseFrame,
    InvestigationResume, InvestigatorId, LocationId, Owner, Phase, PhaseStep, SkillKind,
    SubstitutionPromptFrame, UpkeepPhaseFrame, UpkeepResume,
};
use game_core::test_support::{self, ChoiceResolver, MockRegistry, TestSession};
use game_core::{assert_event, assert_event_count};

/// `[action]: Deal 1 damage to an enemy at your location.` — a choice among
/// board entities, so a Selection whose options anchor to the enemies.
const SELECT: &str = "_ts_select";
/// `[action]: Choose one — gain 1 resource, or gain 2 resources.` — two
/// alternatives printed on one card, so a Decision.
const DECIDE: &str = "_ts_decide";
/// `[free]: Gain 1 resource.` — a 0-action ability, which a Fast window offers.
const FREE: &str = "_ts_free";
/// `[action]: Heal 1 damage from an investigator at your location.` — a choice
/// among investigators, whose options anchor to each investigator's card.
const HEAL: &str = "_ts_heal";
/// `[free]: Gain 1 resource.` and `[free]: Gain 2 resources.` — two abilities
/// printed on one card, which a Fast window offers on the same anchor.
const TWO_FREE: &str = "_ts_two_free";

const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(10);
const SELECT_INST: CardInstanceId = CardInstanceId(1);
const DECIDE_INST: CardInstanceId = CardInstanceId(2);
const FREE_INST: CardInstanceId = CardInstanceId(3);
const HEAL_INST: CardInstanceId = CardInstanceId(4);
const TWO_FREE_INST: CardInstanceId = CardInstanceId(5);
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
        .with_abilities(TWO_FREE, || -> Vec<Ability> {
            vec![
                dsl::activated(0, vec![], dsl::gain_resources(InvestigatorTarget::You, 1)),
                dsl::activated(0, vec![], dsl::gain_resources(InvestigatorTarget::You, 2)),
            ]
        })
        .install();
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
    turn_begins_window_with(FREE, FREE_INST)
}

/// The turn-begins Fast window with `code` in play as `inst`.
fn turn_begins_window_with(code: &str, inst: CardInstanceId) -> GameStateBuilder {
    board_with(&[(code, inst)])
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
        inv.cards_in_play.push(CardInPlay::enter_play(
            CardCode::new(code),
            inst,
            Owner::Investigator(InvestigatorId(1)),
        ));
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
    // A substitution prompt's handler surfaces it when it pushes the frame;
    // the main loop has no way to show it again.
    let mut state = board().open_turn(INV).build();
    state.continuations = test_support::from_frames_unchecked(
        state
            .continuations
            .iter()
            .cloned()
            .chain([SubstitutionPromptFrame { investigator: INV }.into()]),
    );
    let _ = TestSession::new(state);
}

/// A staged setup mulligan settles at its prompt, and answering it runs the
/// mulligan: the redrawn card leaves the hand and setup's end opens the turn.
#[test]
fn settling_a_staged_mulligan_shows_its_prompt() {
    let mut inv = test_support::test_investigator(1);
    inv.hand = vec![CardCode::new("_ts_kept_out")];
    inv.deck = vec![CardCode::new("_ts_drawn")];
    let session = GameStateBuilder::new()
        .with_investigator_at(inv, HERE)
        .with_location(test_support::test_location(HERE.0, "Study"))
        .with_turn_order([INV])
        .with_mulligan_remaining([INV])
        .session();
    assert_eq!(session.prompt().kind, InputKind::PickMultiple);

    let session = session.pick_multiple(&[0]);

    assert_event!(session.events(), Event::MulliganPerformed { .. });
    assert_eq!(
        session.state().investigators[&INV].hand,
        vec![CardCode::new("_ts_drawn")],
    );
}

/// A staged Mythos encounter draw settles at its prompt: the `Confirm`
/// anchored to the encounter deck.
#[test]
fn settling_a_staged_encounter_draw_shows_its_prompt() {
    let session = board()
        .with_turn_order([INV])
        .with_mythos_draw_remaining([INV])
        .session();
    let prompt = session.prompt();
    assert_eq!(prompt.kind, InputKind::Confirm);
    assert_eq!(prompt.target, Some(OptionTarget::EncounterDeck));
}

/// A staged upkeep hand-size discard settles at its prompt, and answering it
/// discards down to the cap.
#[test]
fn settling_a_staged_hand_size_discard_shows_its_prompt() {
    let mut inv = test_support::test_investigator(1);
    inv.hand = (0..9).map(|_| CardCode::new("_ts_filler")).collect();
    let session = GameStateBuilder::new()
        .with_investigator_at(inv, HERE)
        .with_location(test_support::test_location(HERE.0, "Study"))
        .with_turn_order([INV])
        .with_phase(Phase::Upkeep)
        .with_phase_anchor(UpkeepPhaseFrame {
            resume: UpkeepResume::Begins,
        })
        .with_hand_size_discard_pending([INV])
        .session();
    assert_eq!(session.prompt().kind, InputKind::PickMultiple);

    let session = session.apply(Action::Player(PlayerAction::ResolveInput {
        response: InputResponse::PickMultiple {
            selected: vec![OptionId(0)],
        },
    }));

    assert_eq!(session.state().investigators[&INV].hand.len(), 8);
    assert_eq!(session.state().investigators[&INV].discard.len(), 1);
}

// ---- test_skill and commit -------------------------------------------------

/// The commit window a skill test opens: a `PickMultiple` that holds the test
/// open, so the test has not ended yet.
fn assert_at_commit_window(session: &TestSession) {
    assert_eq!(session.prompt().kind, InputKind::PickMultiple);
    assert_event_count!(session.events(), 0, Event::SkillTestEnded { .. });
}

/// `test_skill` starts a plain skill test from where the session rests and
/// stops at its commit window; `commit(&[])` commits nothing and the test
/// resolves.
#[test]
fn test_skill_stops_at_the_commit_window_and_commit_nothing_resolves_it() {
    let session = board().session().test_skill(INV, SkillKind::Willpower, 2);
    assert_at_commit_window(&session);

    let session = session.commit(&[]);

    assert_event!(session.events(), Event::SkillTestEnded { .. });
}

/// `commit` answers the commit window by card code: the named card leaves the
/// hand and, once the test resolves, lands in the discard pile; the card
/// beside it stays.
#[test]
fn commit_by_code_commits_that_card() {
    let committed = CardCode::new("_ts_committed");
    let kept = CardCode::new("_ts_kept");
    let mut inv = test_support::test_investigator(1);
    inv.hand = vec![kept.clone(), committed.clone()];
    let session = GameStateBuilder::new()
        .with_investigator_at(inv, HERE)
        .with_location(test_support::test_location(HERE.0, "Study"))
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .session()
        .test_skill(INV, SkillKind::Willpower, 2)
        .commit(std::slice::from_ref(&committed));

    assert_event!(session.events(), Event::SkillTestEnded { .. });
    let me = &session.state().investigators[&INV];
    assert_eq!(me.hand, vec![kept]);
    assert_eq!(me.discard, vec![committed]);
}

/// A skill test started from the turn menu exposes the turn again when it
/// resolves: the session comes back to the menu.
#[test]
fn test_skill_from_the_turn_menu_returns_to_the_menu() {
    let session = board()
        .open_turn(INV)
        .session()
        .test_skill(INV, SkillKind::Intellect, 2);
    assert_at_commit_window(&session);

    let session = session.commit(&[]);

    assert_event!(session.events(), Event::SkillTestEnded { .. });
    assert_eq!(
        session.prompt().target,
        Some(OptionTarget::TurnControl(INV))
    );
}

/// `commit` answers a prompt, so at the turn menu it panics rather than be
/// read as a turn action.
#[test]
#[should_panic(expected = "at the turn menu")]
fn commit_at_the_turn_menu_panics() {
    let _ = board().open_turn(INV).session().commit(&[]);
}

/// `commit` answers a skill test's commit window and nothing else: at another
/// `PickMultiple` — a mulligan — it panics rather than answer it.
#[test]
#[should_panic(expected = "not at a skill test's commit window")]
fn commit_at_another_pick_multiple_panics() {
    let _ = board()
        .with_turn_order([INV])
        .with_mulligan_remaining([INV])
        .session()
        .commit(&[]);
}

/// `pick_multiple` answers a `PickMultiple` that is not a commit window: at a
/// skill test's commit window it panics and sends the author to `commit`.
#[test]
#[should_panic(expected = "commit cards by code")]
fn pick_multiple_at_a_commit_window_panics() {
    let _ = board()
        .session()
        .test_skill(INV, SkillKind::Willpower, 2)
        .pick_multiple(&[]);
}

// ---- submit_off_menu ---------------------------------------------------------

/// `submit_off_menu` sends an action the menu does not offer straight to its
/// handler — what a client could send over the wire without consulting the
/// menu. With no actions left the menu withholds the activation, and the
/// handler itself rejects it: the reason reads back, nothing is spent, and the
/// turn menu still stands.
#[test]
fn submit_off_menu_reaches_the_handler_and_its_rejection_reads_back() {
    let mut state = board().open_turn(INV).build();
    state
        .investigators
        .get_mut(&INV)
        .expect("seated")
        .actions_remaining = 0;

    let session = TestSession::new(state).submit_off_menu(&activate(DECIDE_INST));

    assert!(
        session.expect_rejected().contains("action"),
        "the handler names the missing action: {}",
        session.expect_rejected(),
    );
    assert_eq!(resources(&session), 5);
    assert_eq!(
        session.prompt().target,
        Some(OptionTarget::TurnControl(INV))
    );
}

// ---- answer_with -------------------------------------------------------------

/// A reply policy that answers every prompt: commits nothing, confirms, and
/// declines everything else. Counts the prompts it answered.
#[derive(Default)]
struct CommitNothing {
    answered: Rc<Cell<usize>>,
}

impl ChoiceResolver for CommitNothing {
    fn next(&mut self, request: &InputRequest, _state: &GameState) -> InputResponse {
        self.answered.set(self.answered.get() + 1);
        match request.kind {
            InputKind::PickMultiple => InputResponse::PickMultiple { selected: vec![] },
            InputKind::Confirm => InputResponse::Confirm,
            _ => InputResponse::Skip,
        }
    }
}

/// A resolver installed with `answer_with` answers every prompt a step opens,
/// so the step drains to rest — and it stays installed for the steps after.
#[test]
fn answer_with_drives_each_step_to_rest_and_stays_installed() {
    let policy = CommitNothing::default();
    let answered = Rc::clone(&policy.answered);
    let session = board()
        .open_turn(INV)
        .session()
        .answer_with(policy)
        .test_skill(INV, SkillKind::Willpower, 2);
    assert_eq!(
        session.prompt().target,
        Some(OptionTarget::TurnControl(INV))
    );
    assert_eq!(answered.get(), 1, "the commit window was answered");

    let session = session.test_skill(INV, SkillKind::Agility, 2);

    assert_event_count!(session.events(), 2, Event::SkillTestEnded { .. });
    assert_eq!(answered.get(), 2);
    assert_eq!(
        session.prompt().target,
        Some(OptionTarget::TurnControl(INV))
    );
}

/// `resolve_choices` after `answer_with` puts the script back: with nothing
/// scripted, the next step stops at the first prompt again.
#[test]
fn resolve_choices_after_answer_with_restores_the_script() {
    let session = board()
        .open_turn(INV)
        .session()
        .answer_with(CommitNothing::default())
        .resolve_choices(|_| {})
        .test_skill(INV, SkillKind::Willpower, 2);

    assert_at_commit_window(&session);
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

/// An option choosing an investigator anchors to that investigator's card, so
/// `pick` names it through `Investigator::card_anchor` like any board entity
/// (#950): the heal's choice between two damaged investigators.
#[test]
fn pick_chooses_an_investigator_by_their_card() {
    let mut mine = test_support::test_investigator(1);
    mine.investigator_card.accumulated_damage = 1;
    mine.cards_in_play.push(CardInPlay::enter_play(
        CardCode::new(HEAL),
        HEAL_INST,
        Owner::Investigator(InvestigatorId(1)),
    ));
    let mut other = test_support::test_investigator(2);
    other.investigator_card.accumulated_damage = 1;
    let session = GameStateBuilder::new()
        .with_investigator_at(mine, HERE)
        .with_investigator_at(other, HERE)
        .with_location(test_support::test_location(HERE.0, "Study"))
        .open_turn(INV)
        .session()
        .take(&activate(HEAL_INST));
    let theirs = session.state().investigators[&InvestigatorId(2)].card_anchor();

    let session = session.pick(theirs);

    assert_eq!(
        session.state().investigators[&InvestigatorId(2)].damage(),
        0
    );
    assert_eq!(session.state().investigators[&INV].damage(), 1);
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

// ---- pick_nth_at -----------------------------------------------------------

/// Two abilities printed on one card share its anchor in a Fast window, so
/// `pick` cannot name either; `pick_nth_at` names the card and the ability by
/// the order the card prints it.
#[test]
fn pick_nth_at_chooses_among_abilities_printed_on_one_card() {
    let card = OptionTarget::CardInstance(TWO_FREE_INST);
    let session = turn_begins_window_with(TWO_FREE, TWO_FREE_INST).session();
    assert_eq!(
        session
            .prompt()
            .options
            .iter()
            .filter(|o| o.target.as_ref() == Some(&card))
            .count(),
        2,
        "both abilities are offered on the card: {:?}",
        session.prompt(),
    );

    let session = session.pick_nth_at(card, 1);

    assert_eq!(resources(&session), 7, "the second ability gains 2");
}

/// With a lone option at the anchor, `pick` names it, and `pick_nth_at` sends
/// the author there.
#[test]
#[should_panic(expected = "pick a lone one with `pick(OptionTarget)`")]
fn pick_nth_at_a_lone_anchored_option_panics() {
    let _ = turn_begins_window()
        .session()
        .pick_nth_at(OptionTarget::CardInstance(FREE_INST), 0);
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
    let session =
        board()
            .open_turn(INV)
            .session()
            .apply(Action::Player(PlayerAction::ResolveInput {
                response: InputResponse::Confirm,
            }));

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

/// `confirm` and `skip` answer a prompt, so at the turn menu they panic rather
/// than be read as a turn action.
#[test]
#[should_panic(expected = "at the turn menu")]
fn confirm_at_the_turn_menu_panics() {
    let _ = board().open_turn(INV).session().confirm();
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

/// A scripted `pick` answers by anchor, like the session step: the second
/// enemy is damaged whatever its position.
#[test]
fn a_scripted_pick_answers_by_anchor() {
    let session = board()
        .open_turn(INV)
        .session()
        .resolve_choices(|c| {
            c.pick(OptionTarget::Enemy(SECOND));
        })
        .take(&activate(SELECT_INST));

    assert_eq!(session.state().enemies[&SECOND].damage, 1);
    assert_eq!(session.state().enemies[&FIRST].damage, 0);
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
