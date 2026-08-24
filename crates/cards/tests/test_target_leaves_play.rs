//! #682: an attack whose target leaves play mid-test is **abandoned**, not
//! resolved.
//!
//! A Fight action's difficulty *is* the attacked enemy's modified fight value,
//! read at ST.6 (#677, ADR 0005). If the enemy leaves `state.enemies` between
//! ST.1 and ST.6 the query finds nothing, answers 0, and the test
//! automatically succeeds — the defect this file pins shut.
//!
//! **The vendored sources do not settle what becomes of such a test**, so the
//! behaviour asserted here is an engine decision, recorded on
//! `DifficultyBasis`. The nearest passage is
//! `data/official-faq/Frequently_Asked_Questions.md`, and it is about the
//! *investigator* moving rather than the target leaving play:
//!
//! > Q: If I initiate a skill test at a given location, then trigger an effect
//! > that causes me to move before that test finishes resolving, what happens
//! > to that skill test?
//! >
//! > A: Once you initiate a skill test or ability, you'll resolve that test or
//! > ability as completely as possible, regardless of your location (unless
//! > another effect cancels or interrupts it).
//!
//! The word the decision leans on is *possible*: resolving an attack against
//! an enemy that is not in play is not.
//!
//! The instrument is Beat Cop 01018 (`data/arkhamdb-snapshot/pack/core/core.json`):
//!
//! > You get +1 \[combat\].
//! > \[fast\] Discard Beat Cop: Deal 1 damage to an enemy at your location.
//!
//! bought at the test's **ST.2 player window** — inside the test, before the
//! chaos token is revealed — against a 1-health enemy. Its ruling
//! (<https://arkhamdb.com/card/01018>) bounds where the ability may be used:
//!
//! > You cannot use Beat Cop's ability after you assign lethal damage/horror to
//! > him (there is no Player Window to use \[fast\]free abilities in between
//! > assigning and applying damage).
//!
//! Own process → installs `cards::REGISTRY`.

use game_core::action::{Action, InputResponse, PlayerAction};
use game_core::engine::enumerate::{legal_actions, TurnAction};
use game_core::engine::{EngineOutcome, InputKind, InputRequest, OptionId};
use game_core::event::Event;
use game_core::state::{
    CardCode, CardInPlay, CardInstanceId, ChaosBag, ChaosToken, EnemyId, GameState, InvestigatorId,
    LocationId, Phase, TokenModifiers, Zone,
};
use game_core::test_support::{
    drive, test_enemy, test_investigator, test_location, ChoiceResolver, GameStateBuilder,
};
use game_core::{assert_event, assert_no_event};

const BEAT_COP: &str = "01018";
/// Unexpected Courage 01093: *"Max 1 committed per skill test."*, two wild
/// icons and no triggers — a card that is only ever *committed*, so what
/// becomes of it at an abandoned teardown is unambiguous.
const UNEXPECTED_COURAGE: &str = "01093";
const INV: InvestigatorId = InvestigatorId(1);
const LOC: LocationId = LocationId(10);
const ENEMY: EnemyId = EnemyId(100);
const COP_INST: CardInstanceId = CardInstanceId(0);

#[ctor::ctor(unsafe)]
fn install_cards_registry() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
}

/// Skip the ST.1 window, commit `commit`, then — when `kill_at_st2` — take the
/// one offered fast ability at the **ST.2** window: Beat Cop's discard, which
/// defeats the 1-health enemy the in-flight attack is against.
///
/// Fast player windows arrive as skippable `PickSingle` prompts and the commit
/// window as a non-skippable `PickMultiple`, so the resolver can tell them
/// apart without scripting option ids that depend on enumeration order. The
/// two windows a skill test opens are ST.1 (before the commit window) and ST.2
/// (after it, before the token), in that order — hence the count.
#[derive(Default)]
struct KillTargetAtStTwo {
    /// Whether to buy Beat Cop's ability at the second window.
    kill_at_st2: bool,
    /// Hand indices to submit at the commit window (empty for no commits).
    commit: Vec<OptionId>,
    windows_seen: u32,
}

impl ChoiceResolver for KillTargetAtStTwo {
    fn next(&mut self, request: &InputRequest, _state: &GameState) -> InputResponse {
        if request.skippable && request.kind == InputKind::PickSingle {
            self.windows_seen += 1;
            if self.kill_at_st2 && self.windows_seen == 2 {
                assert_eq!(
                    request.options.len(),
                    1,
                    "Beat Cop's [fast] ability is the one thing eligible at ST.2: {request:?}",
                );
                return InputResponse::PickSingle(request.options[0].id);
            }
            return InputResponse::Skip;
        }
        match request.kind {
            InputKind::Confirm => InputResponse::Confirm,
            _ => InputResponse::PickMultiple {
                selected: self.commit.clone(),
            },
        }
    }
}

/// Board: Beat Cop in play under the active investigator at `LOC`, `hand` in
/// hand, and a 1-health enemy there, engaged — so both Fight and Evade are
/// legal against it, and one point of damage defeats it.
fn board_with_hand(hand: &[&str]) -> GameState {
    let mut inv = test_investigator(1);
    inv.cards_in_play
        .push(CardInPlay::enter_play(CardCode::new(BEAT_COP), COP_INST));
    inv.hand = hand.iter().map(|c| CardCode::new(*c)).collect();

    let mut enemy = test_enemy(100, "Ghoul");
    enemy.max_health = 1;
    enemy.current_location = Some(LOC);
    enemy.engaged_with = Some(INV);

    GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator_at(inv, LOC)
        .with_location(test_location(10, "Study"))
        .with_enemy(enemy)
        .with_active_investigator(INV)
        .with_turn_order([INV])
        .with_investigator_turn(INV)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .build()
}

fn board() -> GameState {
    board_with_hand(&[])
}

/// Take `action` on `state`, committing `commit` (hand indices) and then
/// killing the attacked enemy at the test's ST.2 window.
fn attack_then_kill_the_target_on(
    state: GameState,
    action: &TurnAction,
    commit: Vec<OptionId>,
) -> game_core::engine::ApplyResult {
    let idx = legal_actions(&state)
        .iter()
        .position(|a| a == action)
        .unwrap_or_else(|| panic!("{action:?} is not legal on this board"));
    drive(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(OptionId(
                u32::try_from(idx).expect("action index fits u32"),
            )),
        }),
        KillTargetAtStTwo {
            kill_at_st2: true,
            commit,
            windows_seen: 0,
        },
    )
}

/// [`attack_then_kill_the_target_on`] against the bare board, committing
/// nothing.
fn attack_then_kill_the_target(action: &TurnAction) -> game_core::engine::ApplyResult {
    attack_then_kill_the_target_on(board(), action, Vec::new())
}

/// The core assertion, shared by Fight and Evade: the enemy is gone, the test
/// ended without a verdict, and nothing of it is left on the board.
fn assert_abandoned(result: &game_core::engine::ApplyResult) {
    assert_event!(result.events, Event::EnemyDefeated { enemy, .. } if *enemy == ENEMY);
    assert!(
        !result.state.enemies.contains_key(&ENEMY),
        "the attacked enemy left play at the ST.2 window",
    );

    // No verdict: the difficulty-0 auto-success is exactly what this pins shut,
    // and a failure would be just as invented.
    assert_no_event!(result.events, Event::SkillTestSucceeded { .. });
    assert_no_event!(result.events, Event::SkillTestFailed { .. });
    // The test *is* over, and `SkillTestEnded` is the documented signal for it.
    assert_event!(result.events, Event::SkillTestEnded { investigator } if *investigator == INV);

    assert!(
        result.state.current_skill_test().is_none(),
        "the abandoned test's frame is torn down",
    );
    assert!(
        result.state.recorded_modifiers.is_empty(),
        "test-scoped rows expire with the test, however it ended",
    );
    assert!(
        !matches!(result.outcome, EngineOutcome::Rejected { .. }),
        "abandonment is not a rejection: {:?}",
        result.outcome,
    );
}

#[test]
fn a_fight_whose_target_leaves_play_at_st2_is_abandoned() {
    let result = attack_then_kill_the_target(&TurnAction::Fight {
        investigator: INV,
        enemy: ENEMY,
    });
    assert_abandoned(&result);
    // The only damage dealt is Beat Cop's own. The Fight follow-up is
    // success-only, there is no success to trigger it, and no enemy left to
    // damage — before #682 it reached `damage_enemy` against an absent enemy
    // and tripped that handler's state-corruption `unreachable!`.
    assert_eq!(
        result
            .events
            .iter()
            .filter(|e| matches!(e, Event::EnemyDamaged { .. }))
            .count(),
        1,
        "Beat Cop's 1 damage and nothing else: {:?}",
        result.events,
    );
}

#[test]
fn an_evade_whose_target_leaves_play_at_st2_is_abandoned() {
    let result = attack_then_kill_the_target(&TurnAction::Evade {
        investigator: INV,
        enemy: ENEMY,
    });
    assert_abandoned(&result);
    // The Evade follow-up would exhaust and disengage the enemy; it never runs.
    // (Defeat emits no paired `EnemyDisengaged` of its own — see `Event::EnemyDefeated`.)
    assert_no_event!(result.events, Event::EnemyDisengaged { .. });
    assert_no_event!(result.events, Event::EnemyExhausted { .. });
}

/// The action is still spent. Abandoning the test rewinds nothing that
/// happened before it — the Fight action was taken, and RR has no refund for a
/// test that cannot finish.
#[test]
fn abandoning_the_test_does_not_refund_the_action() {
    let before = board().investigators[&INV].actions_remaining;
    let result = attack_then_kill_the_target(&TurnAction::Fight {
        investigator: INV,
        enemy: ENEMY,
    });
    assert_eq!(
        result.state.investigators[&INV].actions_remaining,
        before - 1,
        "the Fight action was charged and stays charged",
    );
}

/// A card committed to the abandoned test is **discarded**, not deleted. The
/// eliminated-tester abandonment (#564) drops limboed cards with the frame
/// because RR p.10 Elimination step 1 has already removed them from the game;
/// here the investigator is still in the scenario and the cards are still
/// theirs, so the teardown must send them to the pile exactly as ST.8 would.
///
/// `Limbo.md`, on where a committed card sits until then:
///
/// > While the effects of an event or treachery card are being resolved, or
/// > while a skill is committed to a skill test, it is neither in play, in the
/// > discard pile, nor is it in an investigator's hand.
#[test]
fn a_card_committed_to_the_abandoned_test_is_discarded_not_deleted() {
    let result = attack_then_kill_the_target_on(
        board_with_hand(&[UNEXPECTED_COURAGE]),
        &TurnAction::Fight {
            investigator: INV,
            enemy: ENEMY,
        },
        vec![OptionId(0)],
    );
    assert_abandoned(&result);

    let inv = &result.state.investigators[&INV];
    assert!(inv.hand.is_empty(), "the commit took it out of hand");
    assert_eq!(
        inv.discard,
        // Beat Cop paid its own discard as the ability's cost, and lands first.
        vec![CardCode::new(BEAT_COP), CardCode::new(UNEXPECTED_COURAGE)],
        "the abandoned teardown discards what the test held in limbo",
    );
    // `Zone::Hand` is the `from` the ST.8 teardown reports for a committed
    // card too — the engine has no `Limbo` zone of its own, and this teardown
    // shares that path rather than inventing a second one.
    assert_event!(
        result.events,
        Event::CardDiscarded {
            from: Zone::Hand,
            ..
        }
    );
}

/// The bound on the gate: a test **past ST.6** is not abandoned when its target
/// leaves play, because that is the ordinary shape of a successful attack — the
/// Fight follow-up's own damage is what defeats the enemy. Same 1-health Ghoul,
/// same board, nothing bought at ST.2.
#[test]
fn a_fight_that_defeats_its_own_target_at_st7_still_resolves() {
    let state = board();
    let action = TurnAction::Fight {
        investigator: INV,
        enemy: ENEMY,
    };
    let idx = legal_actions(&state)
        .iter()
        .position(|a| a == &action)
        .expect("Fight is legal on this board");
    let result = drive(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(OptionId(
                u32::try_from(idx).expect("action index fits u32"),
            )),
        }),
        KillTargetAtStTwo::default(),
    );

    // Combat 3 (Beat Cop's constant +1 on a base 2) + Numeric(0) vs the Ghoul's
    // fight 2 → succeeds by 1, and the follow-up's 1 damage defeats it.
    assert_event!(
        result.events,
        Event::SkillTestSucceeded { investigator, .. } if *investigator == INV
    );
    assert_event!(result.events, Event::EnemyDefeated { enemy, .. } if *enemy == ENEMY);
    assert!(
        !result.state.enemies.contains_key(&ENEMY),
        "the enemy left play — at ST.7, which is not a reason to abandon anything",
    );
}
