use super::*;
use crate::state::{Assignment, CardInstanceId, EnemyId, LocationId};

fn enemy_attacks(inv: InvestigatorId) -> TimingEvent {
    TimingEvent::EnemyAttacks {
        enemy: EnemyId(1),
        investigator: inv,
    }
}

/// The pattern↔condition pairings, which since #704 are **timing-free**:
/// `trigger_matches` no longer takes a timing, so a card pairs with its
/// condition identically in all three cells and the cell filtering happens
/// in the scans. Deleting the `When` whitelist is what makes an
/// `after`-an-enemy-attacks ability declarable at all.
#[test]
fn a_pattern_pairs_with_its_condition_in_every_cell() {
    let inv = InvestigatorId(1);
    let discover = TimingEvent::DiscoverClues {
        investigator: inv,
        location: LocationId(2),
        count: 1,
    };
    // EnemyAttacks ↔ EnemyAttacks — Dodge 01023 (`when`) and any
    // `at`/`after` ability on the same condition.
    assert!(trigger_matches(
        &enemy_attacks(inv),
        &EventPattern::EnemyAttacks {
            attacker: AttackerScope::Any,
            target: TargetScope::Any,
        },
        inv,
    ));
    // DiscoverClues ↔ DiscoverClues — Cover Up 01007.
    assert!(trigger_matches(
        &discover,
        &EventPattern::DiscoverClues,
        inv
    ));
    // A condition still only matches its own pattern.
    assert!(!trigger_matches(
        &enemy_attacks(inv),
        &EventPattern::DiscoverClues,
        inv,
    ));
}

#[test]
fn round_ended_matches_its_own_pattern_board_scoped() {
    let lead = InvestigatorId(1);
    // RoundEnded ↔ RoundEnded — act 01109's group advance (#434).
    // Board-scoped: matches regardless of the candidate's controller.
    assert!(trigger_matches(
        &TimingEvent::RoundEnded,
        &EventPattern::RoundEnded,
        lead,
    ));
    assert!(trigger_matches(
        &TimingEvent::RoundEnded,
        &EventPattern::RoundEnded,
        InvestigatorId(2),
    ));
    // Another condition's pattern still does not match it.
    assert!(!trigger_matches(
        &TimingEvent::RoundEnded,
        &EventPattern::EnemyAttacks {
            attacker: AttackerScope::Any,
            target: TargetScope::Any,
        },
        lead,
    ));
}

/// An [`Assignment`](crate::state::Assignment) giving 1 damage to `inst` —
/// the shape a `DamageAssigned` event carries when that card is a soaker.
fn assignment_damaging(inst: CardInstanceId) -> Assignment {
    let mut assignment = Assignment::default();
    assignment.asset_damage.insert(inst, 1);
    assignment
}

/// Direct `trigger_matches` coverage for the `EnemyAttackDamagedSelf` soak
/// pairing (Guard Dog 01021, C5b #237). The instance-level scoping (only an
/// asset the assignment gives damage to fires) is the source's half of the
/// matcher, `scope_matches`, and is exercised end-to-end in
/// `crates/cards/tests/guard_dog_soak.rs` (which installs the real registry);
/// what *this* layer owns since #727 is the card's narrowing to an enemy
/// **attack**.
#[test]
fn soak_event_matches_only_the_self_soak_pattern() {
    let controller = InvestigatorId(1);
    let soak = TimingEvent::DamageAssigned {
        source: DamageSource::EnemyAttack { enemy: EnemyId(1) },
        investigator: controller,
        assignment: assignment_damaging(CardInstanceId(7)),
    };
    // The soak-self pattern matches the soak event. (C5b #237.)
    assert!(trigger_matches(
        &soak,
        &EventPattern::EnemyAttackDamagedSelf,
        controller,
    ));
    // …but not the same condition from a non-attack source: Guard Dog
    // retaliates to an enemy *attack*, not to treachery harm.
    let effect_harm = TimingEvent::DamageAssigned {
        source: DamageSource::Effect,
        investigator: controller,
        assignment: assignment_damaging(CardInstanceId(7)),
    };
    assert!(!trigger_matches(
        &effect_harm,
        &EventPattern::EnemyAttackDamagedSelf,
        controller,
    ));
    // No other pattern matches the soak event.
    assert!(!trigger_matches(
        &soak,
        &EventPattern::EnemyDefeated {
            by_controller: false,
            code: None,
        },
        controller,
    ));
    // The soak pattern must NOT match a different event (guards the
    // arm ordering — the `=> true` arm is scoped to the soak event).
    let defeat = TimingEvent::EnemyDefeated {
        enemy: EnemyId(1),
        by: Some(controller),
        code: CardCode("01000".into()),
    };
    assert!(!trigger_matches(
        &defeat,
        &EventPattern::EnemyAttackDamagedSelf,
        controller,
    ));
}
