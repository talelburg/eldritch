use super::*;
use crate::action::{Action, PlayerAction};
use crate::engine::enumerate::TurnAction;
use crate::engine::outcome::OptionTarget;
use crate::state::{EnemyPhaseFrame, LocationId};
use crate::test_support::TestSession;

/// The parked loop's re-exposure (#704): the head attacker's sequence has
/// popped, so the loop takes it off, exhausts it, and — with none left —
/// runs the enemy phase's cursor advance.
///
/// One attacker, not two: with 2+ remaining the drain would re-prompt for
/// the player attack order (#143), which the order-pick tests cover. The
/// test registry (`TEST_INV`) is installed so `max_health()`/`max_sanity()`
/// resolve (#448 cp2a).
#[test]
fn drive_parked_attack_loop_exhausts_the_head_then_advances_the_cursor() {
    let inv_id = InvestigatorId(1);
    let attacker = EnemyId(2);
    let mut enemy = test_support::test_enemy(2, "Attacker");
    enemy.engaged_with = Some(inv_id);

    let mut state = GameStateBuilder::new()
        .with_seat(test_support::seat(1))
        .with_turn_order([inv_id])
        .with_enemy(enemy)
        .with_phase_anchor(EnemyPhaseFrame {
            resume: EnemyResume::BeforeInvestigatorAttacked,
            attacking: Some(inv_id),
        })
        .build();
    state.continuations.push(AttackLoopFrame {
        investigator: inv_id,
        remaining_attackers: vec![attacker], // the head, mid-sequence
        source: EnemyAttackSource::EnemyPhase,
        stage: AttackLoopStage::Attacking,
    });

    let mut events = Vec::new();
    let mut cx = Cx {
        state: &mut state,
        events: &mut events,
    };
    let outcome = drive_parked_attack_loop(&mut cx);

    assert!(
        !state
            .continuations
            .iter()
            .any(|c| matches!(c, Continuation::AttackLoop(_))),
        "the parked attack-loop frame is consumed"
    );
    assert!(
        state.enemies[&attacker].exhausted,
        "the head attacker exhausts once its sequence completes"
    );
    assert_event!(events, Event::EnemyExhausted { enemy } if *enemy == attacker);
    // Loop finished → `after_enemy_phase_attacks` advanced the cursor past
    // the only investigator and opened the all-attacked window (auto-skips
    // inline with no ability to offer), cascading the EnemyPhase anchor off
    // the stack — so no anchor is left still attacking anyone.
    assert!(
        !state.continuations.iter().any(|c| matches!(
            c,
            Continuation::EnemyPhase(EnemyPhaseFrame {
                attacking: Some(_),
                ..
            })
        )),
        "cursor advanced past the sole investigator (no anchor still attacking)"
    );
    let _ = outcome;
}

/// The exhaust is the *enemy phase's* step, not the attack's impact, so it
/// runs whether or not the attack dealt anything — `data/arkhamdb-faq/core/01023.md`:
/// *"If an attack was cancelled during the Enemy phase, the attacking enemy
/// still exhausts."* A cancelled attack reaches here having dealt no damage
/// (the coordinator abandoned its sequence before the resolve step), which
/// this fixture reproduces by parking the loop with no damage applied. The
/// end-to-end cancel is `crates/cards/tests/dodge.rs`.
#[test]
fn a_head_attacker_that_dealt_nothing_still_exhausts() {
    let inv_id = InvestigatorId(1);
    let attacker = EnemyId(2);
    let mut enemy = test_support::test_enemy(2, "Attacker");
    enemy.engaged_with = Some(inv_id);

    let mut state = GameStateBuilder::new()
        .with_seat(test_support::seat(1))
        .with_turn_order([inv_id])
        .with_enemy(enemy)
        .with_phase_anchor(EnemyPhaseFrame {
            resume: EnemyResume::BeforeInvestigatorAttacked,
            attacking: Some(inv_id),
        })
        .build();
    state.continuations.push(AttackLoopFrame {
        investigator: inv_id,
        remaining_attackers: vec![attacker],
        source: EnemyAttackSource::EnemyPhase,
        stage: AttackLoopStage::Attacking,
    });

    let mut events = Vec::new();
    let mut cx = Cx {
        state: &mut state,
        events: &mut events,
    };
    let _ = drive_parked_attack_loop(&mut cx);

    assert_eq!(
        state.investigators[&inv_id].damage(),
        0,
        "no damage: the attack's impact never ran"
    );
    assert_no_event!(events, Event::DamageTaken { .. });
    assert!(
        state.enemies[&attacker].exhausted,
        "a cancelled attack still exhausts the attacker (01023 ruling)"
    );
}

/// The mirror of the rule above on the other two sources: an `AoO` or
/// retaliate attacker never exhausts (RR p.7 / p.18), cancelled or not.
#[test]
fn an_attack_of_opportunity_attacker_never_exhausts() {
    let inv_id = InvestigatorId(1);
    let attacker = EnemyId(2);
    let mut enemy = test_support::test_enemy(2, "Attacker");
    enemy.engaged_with = Some(inv_id);

    let mut state = GameStateBuilder::new()
        .with_seat(test_support::seat(1))
        .with_turn_order([inv_id])
        .with_enemy(enemy)
        .build();
    state.continuations.push(AttackLoopFrame {
        investigator: inv_id,
        remaining_attackers: vec![attacker],
        source: EnemyAttackSource::AttackOfOpportunity,
        stage: AttackLoopStage::Attacking,
    });

    let mut events = Vec::new();
    let mut cx = Cx {
        state: &mut state,
        events: &mut events,
    };
    let _ = drive_parked_attack_loop(&mut cx);

    assert!(
        !state.enemies[&attacker].exhausted,
        "an AoO attacker never exhausts (RR p.7)"
    );
    assert_no_event!(events, Event::EnemyExhausted { .. });
}

#[test]
fn drive_retaliate_deals_damage_but_does_not_exhaust_the_attacker() {
    // RR p.18: a retaliate attack does not exhaust the attacker.
    let inv_id = InvestigatorId(1);
    let mut enemy = test_support::test_enemy(100, "Retaliator");
    enemy.retaliate = true;
    enemy.attack_damage = 1;
    enemy.attack_horror = 0;
    // Not engaged: a retaliate fires regardless of engagement, driven by enemy id.
    let mut state = GameStateBuilder::new()
        .with_seat(test_support::seat(1))
        .with_enemy(enemy)
        .build();
    let mut events = Vec::new();
    let mut cx = Cx {
        state: &mut state,
        events: &mut events,
    };

    // The attack is queued on the coordinator (#704), so drive it out: the
    // loop's `Done` means *queued*, not *dealt*.
    let outcome = drive_retaliate(&mut cx, EnemyId(100), inv_id);
    let outcome = dispatch::drive(&mut cx, outcome);

    assert!(matches!(outcome, EngineOutcome::Done));
    assert!(
        !cx.state.enemies[&EnemyId(100)].exhausted,
        "retaliate must not exhaust (RR p.18)"
    );
    assert_eq!(
        cx.state.investigators[&inv_id].damage(),
        1,
        "retaliate dealt 1 damage"
    );
    assert_event!(events, Event::DamageTaken { .. });
    assert_no_event!(events, Event::EnemyExhausted { .. });
}

#[test]
fn drive_aoo_deals_damage_but_does_not_exhaust_the_attacker() {
    // RR p.7: an enemy does not exhaust while making an attack of opportunity.
    let inv_id = InvestigatorId(1);
    let mut enemy = test_support::test_enemy(100, "Ghoul");
    enemy.engaged_with = Some(inv_id);
    enemy.attack_damage = 1;
    enemy.attack_horror = 0;
    let mut state = GameStateBuilder::new()
        .with_seat(test_support::seat(1))
        .with_enemy(enemy)
        .build();
    let mut events = Vec::new();
    let mut cx = Cx {
        state: &mut state,
        events: &mut events,
    };

    let outcome = drive_aoo(&mut cx, inv_id);
    let outcome = dispatch::drive(&mut cx, outcome);

    assert!(matches!(outcome, EngineOutcome::Done));
    assert!(
        !cx.state.enemies[&EnemyId(100)].exhausted,
        "AoO must not exhaust the attacker (RR p.7)"
    );
    assert_eq!(
        cx.state.investigators[&inv_id].damage(),
        1,
        "AoO damage landed on the investigator"
    );
    // Enemy attack fires DamageTaken (no EnemyAttacked event exists); verify
    // damage landed on the investigator and no exhaust event was emitted.
    assert_event!(events, Event::DamageTaken { .. });
    assert_no_event!(events, Event::EnemyExhausted { .. });
}

/// Investigator 1's open turn at a location, engaged by two ready enemies:
/// EnemyId(5) dealing 1 damage and EnemyId(6) dealing 2. Any non-exempt
/// action provokes both attacks of opportunity.
fn engaged_by_two() -> TestSession {
    let here = LocationId(10);
    let mut e_a = test_support::test_enemy(5, "A"); // EnemyId(5), dmg 1
    e_a.attack_damage = 1;
    let mut e_b = test_support::test_enemy(6, "B"); // EnemyId(6), dmg 2
    e_b.attack_damage = 2;
    GameStateBuilder::new()
        .with_location(test_support::test_location(here.0, "Study"))
        .with_seat(test_support::seat(1).at(here))
        .with_enemy_engaged(e_a, InvestigatorId(1))
        .with_enemy_engaged(e_b, InvestigatorId(1))
        .open_turn(InvestigatorId(1))
        .session()
}

#[test]
fn drive_aoo_offers_order_pick_for_two_engaged_enemies() {
    // 2 engaged ready enemies provoking an AoO → the loop suspends on the
    // order pick, parking the AttackLoop frame as the top frame with the AoO
    // source + PickOrder stage (so it spans the whole AoO, #143). Picking the
    // higher-id enemy first proves the pick overrides EnemyId order; neither
    // AoO attacker exhausts (RR p.7). Total AoO damage = 3 < 8 = TEST_INV's
    // max_health() (#448 cp2a).
    let inv_id = InvestigatorId(1);
    let session = engaged_by_two().take(&TurnAction::Resource {
        investigator: inv_id,
    });
    assert_eq!(
        session.offered_at(&OptionTarget::Enemy(EnemyId(6))),
        1,
        "2 engaged ready enemies → AoO order pick (#143)"
    );
    // The parked frame carries the AoO source + PickOrder stage (frame spans
    // the whole AoO, not just a window suspension) — a stack invariant.
    assert!(matches!(
        session.state().continuations.top(),
        Some(Continuation::AttackLoop(AttackLoopFrame {
            source: EnemyAttackSource::AttackOfOpportunity,
            stage: AttackLoopStage::PickOrder,
            ..
        }))
    ));

    // Pick EnemyId(6) (dmg 2) first.
    let session = session.pick(OptionTarget::Enemy(EnemyId(6)));
    session.assert_at_turn_menu(inv_id);
    let damages: Vec<u8> = session
        .events()
        .iter()
        .filter_map(|e| match e {
            Event::DamageTaken { amount, .. } => Some(*amount),
            _ => None,
        })
        .collect();
    assert_eq!(
        damages,
        vec![2, 1],
        "chosen EnemyId(6) (dmg 2) struck first"
    );
    let state = session.state();
    assert!(
        !state.enemies[&EnemyId(5)].exhausted && !state.enemies[&EnemyId(6)].exhausted,
        "AoO attackers never exhaust (RR p.7)"
    );
}

#[test]
fn resume_attack_order_pick_rejects_invalid_input_and_keeps_frame() {
    // An out-of-range option id and a wrong InputResponse variant both reject,
    // leaving the order pick up for the client to retry (mirrors
    // resume_hunter_choice).
    let inv_id = InvestigatorId(1);
    let session = engaged_by_two().take(&TurnAction::Resource {
        investigator: inv_id,
    });

    // Out-of-range option (only 0, 1 valid): a malformed wire response no
    // step can name, so the raw `ResolveInput` is sent as is.
    let session = session.apply(Action::Player(PlayerAction::ResolveInput {
        response: InputResponse::PickSingle(OptionId(9)),
    }));
    session.expect_rejected();
    // Wrong variant.
    let session = session.skip();
    session.expect_rejected();
    // The order pick survives both rejections for retry.
    assert_eq!(session.offered_at(&OptionTarget::Enemy(EnemyId(5))), 1);
    assert_eq!(session.offered_at(&OptionTarget::Enemy(EnemyId(6))), 1);
    session
        .pick(OptionTarget::Enemy(EnemyId(5)))
        .assert_at_turn_menu(inv_id);
}
