//! Combat helpers: enemy damage, investigator damage/horror, attacks.

use crate::event::Event;
use crate::state::{DefeatCause, EnemyId, InvestigatorId, Status, WindowKind};

use super::Cx;

/// Apply `amount` damage to an enemy. If the new damage reaches or
/// exceeds `max_health`, emit `EnemyDefeated` and remove the enemy
/// from `state.enemies`. `by` attributes the defeat for
/// trigger-window consumers (e.g. Roland's reaction). Used by Fight
/// today; will be reused by future damage-dealing card effects.
pub(super) fn damage_enemy(cx: &mut Cx, enemy_id: EnemyId, amount: u8, by: Option<InvestigatorId>) {
    let enemy = cx.state.enemies.get_mut(&enemy_id).unwrap_or_else(|| {
        unreachable!(
            "damage_enemy: enemy {enemy_id:?} is not in state.enemies; \
             this is a state-corruption invariant violation"
        )
    });
    let new_damage = enemy.damage.saturating_add(amount).min(enemy.max_health);
    enemy.damage = new_damage;
    cx.events.push(Event::EnemyDamaged {
        enemy: enemy_id,
        amount,
        new_damage,
    });
    if new_damage >= enemy.max_health {
        let defeated_code = enemy.code.clone(); // capture before the enemy is removed
        let defeated_victory = enemy.victory; // ditto
        cx.events.push(Event::EnemyDefeated {
            enemy: enemy_id,
            by,
        });
        cx.state.enemies.remove(&enemy_id);
        // RR p.21: a defeated enemy with a Victory value enters the victory
        // display. Captured here (not scanned at scenario resolution like
        // victory locations) because the enemy is removed above.
        if let Some(victory) = defeated_victory.filter(|v| *v > 0) {
            cx.state.victory_display.push(defeated_code.clone());
            cx.events.push(Event::EnteredVictoryDisplay {
                code: defeated_code.clone(),
                victory,
            });
        }
        // Queue the post-defeat reaction window. Emits
        // `Event::WindowOpened` immediately (inside queue_reaction_window);
        // the skill-test driver then suspends at the next step boundary
        // (between `apply_skill_test_follow_up` and
        // `fire_on_skill_test_resolution`) returning AwaitingInput so the
        // player can fire their reaction triggers; see `drive_skill_test`.
        super::reaction_windows::queue_reaction_window(
            cx,
            WindowKind::AfterEnemyDefeated {
                enemy: enemy_id,
                by,
            },
        );
        // Forced act objectives keyed to this defeat (Act 3's "If the
        // Ghoul Priest is Defeated, advance."). `()` return can't
        // propagate a 2+-trigger reject; debug_assert guards it (mirror of
        // upkeep_phase_end / advance_act). Ordering vs. the
        // AfterEnemyDefeated reaction window is fixed-deterministic for
        // now; #212/#213 revisit.
        let forced = super::forced_triggers::fire_forced_triggers(
            cx,
            &super::forced_triggers::ForcedTriggerPoint::EnemyDefeated {
                code: defeated_code,
            },
        );
        debug_assert!(
            matches!(forced, crate::engine::EngineOutcome::Done),
            "EnemyDefeated forced did not resolve to Done: {forced:?} (2+ needs #213)"
        );
    }
}

/// Add `amount` to the investigator's `damage` and emit
/// [`Event::DamageTaken`]. Returns `true` iff the new total reaches
/// `max_health` (i.e. the investigator now qualifies for defeat under
/// [`DefeatCause::Damage`]).
///
/// Does NOT flip [`Status`] or emit [`Event::InvestigatorDefeated`] —
/// the caller composes the defeat step via [`apply_investigator_defeat`]
/// when the return is `true`. This split exists so [`enemy_attack`]
/// can place damage AND horror on the investigator before either
/// triggers defeat detection, matching the Rules Reference page 7
/// "Apply Damage/Horror" clause: *"Any assigned damage/horror that
/// has not been prevented is now placed on each card to which it has
/// been assigned, simultaneously."*
///
/// No-ops when `amount == 0` or the investigator is already defeated
/// (status `!= Active`): defeated investigators are out of play and
/// don't accumulate more damage.
///
/// [`Status`]: crate::state::Status
pub(super) fn apply_damage_numeric(cx: &mut Cx, investigator: InvestigatorId, amount: u8) -> bool {
    if amount == 0 {
        return false;
    }
    let inv = cx
        .state
        .investigators
        .get_mut(&investigator)
        .unwrap_or_else(|| {
            unreachable!(
                "apply_damage_numeric: investigator {investigator:?} is not in the investigators map; \
             this is a state-corruption invariant violation"
            )
        });
    if inv.status != Status::Active {
        return false;
    }
    inv.damage = inv.damage.saturating_add(amount);
    let lethal = inv.damage >= inv.max_health;
    cx.events.push(Event::DamageTaken {
        investigator,
        amount,
    });
    lethal
}

/// Symmetric to [`apply_damage_numeric`] but against `horror` /
/// `max_sanity`. Returns `true` iff the new total reaches the
/// max-sanity threshold; defeat application is the caller's
/// responsibility (see [`super::elimination::apply_investigator_defeat`]).
pub(super) fn apply_horror_numeric(cx: &mut Cx, investigator: InvestigatorId, amount: u8) -> bool {
    if amount == 0 {
        return false;
    }
    let inv = cx
        .state
        .investigators
        .get_mut(&investigator)
        .unwrap_or_else(|| {
            unreachable!(
                "apply_horror_numeric: investigator {investigator:?} is not in the investigators map; \
             this is a state-corruption invariant violation"
            )
        });
    if inv.status != Status::Active {
        return false;
    }
    inv.horror = inv.horror.saturating_add(amount);
    let lethal = inv.horror >= inv.max_sanity;
    cx.events.push(Event::HorrorTaken {
        investigator,
        amount,
    });
    lethal
}

/// Apply an enemy's attack pattern (damage + horror) to an
/// investigator. Used by attacks of opportunity today; will be reused
/// by the enemy-phase handler (#71) when that lands.
///
/// Per the Rules Reference, an enemy making an attack of opportunity
/// does NOT exhaust. Enemy-phase attacks DO exhaust the attacker.
/// This helper therefore does NOT touch the attacker's `exhausted`
/// flag — callers that need exhaustion (i.e. the enemy phase) apply
/// it separately.
///
/// Damage and horror are placed on the investigator **simultaneously**
/// per Rules Reference page 7 ("Apply Damage/Horror"): *"Any assigned
/// damage/horror that has not been prevented is now placed on each
/// card to which it has been assigned, simultaneously. … After
/// applying damage/horror, if an investigator has damage equal to or
/// higher than his or her health or horror equal to or higher than
/// his or her sanity, he or she is defeated."* So `inv.damage` and
/// `inv.horror` BOTH update before any defeat check, even when one
/// alone would be lethal — campaign-log accounting needs both numeric
/// values to land. Only one [`Event::InvestigatorDefeated`] fires per
/// attack regardless of how many stats crossed.
///
/// Tie-break when both stats cross simultaneously: [`DefeatCause::Damage`].
/// Per Rules Reference page 6, an investigator simultaneously defeated
/// by damage and horror *"chooses which type of trauma to suffer"* —
/// physical vs. mental in the campaign log, and the corresponding
/// in-scenario status flip follows. The engine doesn't model campaign
/// trauma yet and has no [`AwaitingInput`] prompt for "pick trauma
/// type," so `DefeatCause::Damage` is a deterministic placeholder for
/// the status flip. Route the choice through `AwaitingInput` (and pick
/// the corresponding [`Status`] variant) when trauma lands; out of
/// scope for `#83`.
///
/// [`AwaitingInput`]: crate::engine::EngineOutcome::AwaitingInput
pub(super) fn enemy_attack(cx: &mut Cx, enemy_id: EnemyId, investigator: InvestigatorId) {
    let enemy = cx.state.enemies.get(&enemy_id).unwrap_or_else(|| {
        unreachable!(
            "enemy_attack: enemy {enemy_id:?} is not in state.enemies; \
             this is a state-corruption invariant violation"
        )
    });
    let damage = enemy.attack_damage;
    let horror = enemy.attack_horror;

    let damage_lethal = apply_damage_numeric(cx, investigator, damage);
    let horror_lethal = apply_horror_numeric(cx, investigator, horror);
    if damage_lethal || horror_lethal {
        let cause = if damage_lethal {
            DefeatCause::Damage
        } else {
            DefeatCause::Horror
        };
        super::elimination::apply_investigator_defeat(cx, investigator, cause);
    }
}

/// Fire attacks of opportunity from every ready enemy engaged with
/// `investigator`. Each attacker resolves via [`enemy_attack`]; order
/// is deterministic by `EnemyId` (`BTreeMap` iteration).
///
/// Per the Rules Reference, the active player chooses the order of
/// `AoOs` from multiple engaged ready enemies; v1 uses deterministic
/// `EnemyId` order. `TODO(#143)`: player-pick attack order
/// (unmilestoned) covers this site alongside
/// [`resolve_attacks_for_investigator`] — both sites share the same
/// deterministic-order TODO.
pub(super) fn fire_attacks_of_opportunity(cx: &mut Cx, investigator: InvestigatorId) {
    let attackers: Vec<EnemyId> = cx
        .state
        .enemies
        .iter()
        .filter(|(_, e)| e.engaged_with == Some(investigator) && !e.exhausted)
        .map(|(id, _)| *id)
        .collect();
    for enemy_id in attackers {
        enemy_attack(cx, enemy_id, investigator);
    }
}

/// Resolve all of one investigator's engaged ready enemies' attacks
/// (Rules Reference p.25 step 3.3 inner body). Snapshot the attacker
/// list in [`EnemyId`] order (`BTreeMap` iteration is sorted), then
/// for each attacker:
///
/// 1. Early-break if `investigator` is no longer [`Status::Active`]
///    (defeated by an earlier attack in the same loop). Remaining
///    attackers do not attack and do not exhaust, per Rules
///    Reference p.10 Elimination step 3 ("All enemies engaged with
///    that player are placed at the location ... unengaged but
///    otherwise maintaining their current game state") and p.25
///    ("Each ready, engaged enemy makes an attack" — a disengaged
///    enemy is not "engaged").
///
///    `apply_investigator_defeat` (#144) now clears `engaged_with` on
///    every enemy engaged with a defeated investigator (Rules Reference
///    p.10 Elimination step 3), so a disengaged enemy genuinely is no
///    longer "engaged" by the time the next loop iteration would run.
///    The early-break here is therefore redundant with that flow — it
///    is kept as the simpler, local form (one extra status check,
///    harmless) so the loop body stays self-evidently correct without
///    cross-referencing the elimination flow.
///
/// 2. Call [`enemy_attack`] (places damage + horror simultaneously
///    per p.7, fires [`super::elimination::apply_investigator_defeat`] if either
///    crosses).
///
/// 3. Set `enemy.exhausted = true`, emit
///    [`Event::EnemyExhausted`]. Per Rules Reference p.25,
///    exhaustion happens "Upon completion of dealing the attack (and
///    all abilities triggered by the attack)" — no carve-out for
///    "the attack defeated the target," so an attack that lands and
///    defeats its target still exhausts the attacker.
///
/// **Atomicity invariant:** the snapshot + loop run as a block
/// within `run_window_continuation`'s `BeforeInvestigatorAttacked`
/// arm — no Fast plays or reactions interpose mid-loop. The first
/// PR that adds a reaction `EventPattern` matching events emitted
/// inside this loop ([`Event::DamageTaken`] / [`Event::HorrorTaken`] /
/// [`Event::EnemyExhausted`] / [`Event::EnemyDefeated`]-from-attack)
/// must persist the remaining-attackers list on `GameState`
/// (analogous to [`GameState::enemy_attack_pending`]) so
/// resume-after-pause re-enters the right iteration point.
///
/// **Attack order:** deterministic by [`EnemyId`]. Rules Reference
/// p.25 prescribes "the order of the attacked investigator's
/// choosing" when an investigator is engaged with multiple enemies;
/// `TODO(#143)`: player-pick attack order, unmilestoned, covers both
/// this site and [`fire_attacks_of_opportunity`] (which has the same
/// TODO).
pub(super) fn resolve_attacks_for_investigator(cx: &mut Cx, investigator: InvestigatorId) {
    // Snapshot ready engaged attackers in deterministic EnemyId order.
    // BTreeMap iteration is already key-sorted.
    let attackers: Vec<EnemyId> = cx
        .state
        .enemies
        .iter()
        .filter(|(_, e)| e.engaged_with == Some(investigator) && !e.exhausted)
        .map(|(id, _)| *id)
        .collect();

    for enemy_id in attackers {
        // Early-break on defeat. See fn doc.
        let active = cx
            .state
            .investigators
            .get(&investigator)
            .is_some_and(|inv| inv.status == Status::Active);
        if !active {
            break;
        }

        // Damage + horror placement (simultaneous per p.7) + defeat.
        enemy_attack(cx, enemy_id, investigator);

        // Exhaust the attacker post-resolution.
        let enemy = cx.state.enemies.get_mut(&enemy_id).unwrap_or_else(|| {
            unreachable!(
                "resolve_attacks_for_investigator: snapshotted enemy \
                 {enemy_id:?} is gone from state.enemies; this is a \
                 state-corruption invariant violation"
            )
        });
        enemy.exhausted = true;
        cx.events.push(Event::EnemyExhausted { enemy: enemy_id });
    }
}

#[cfg(test)]
mod combat_tests {
    use super::super::Cx;
    use crate::event::Event;
    use crate::state::{EnemyId, InvestigatorId};
    use crate::test_support::{test_enemy, GameStateBuilder};
    use crate::{assert_event, assert_no_event};

    #[test]
    fn defeating_victory_enemy_places_it_in_the_victory_display() {
        let eid = EnemyId(1);
        let mut enemy = test_enemy(1, "Ghoul Priest");
        enemy.code = crate::CardCode::new("01116");
        enemy.max_health = 1;
        enemy.victory = Some(2);
        let mut state = GameStateBuilder::new().build();
        state.enemies.insert(eid, enemy);
        let mut events = Vec::new();
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };
        super::damage_enemy(&mut cx, eid, 1, Some(InvestigatorId(1)));

        assert_eq!(state.victory_display, vec![crate::CardCode::new("01116")]);
        assert_event!(
            events,
            Event::EnteredVictoryDisplay { code, victory: 2 } if code.as_str() == "01116"
        );
    }

    #[test]
    fn defeating_non_victory_enemy_places_nothing() {
        let eid = EnemyId(1);
        let mut enemy = test_enemy(1, "Ghoul");
        enemy.max_health = 1;
        enemy.victory = None;
        let mut state = GameStateBuilder::new().build();
        state.enemies.insert(eid, enemy);
        let mut events = Vec::new();
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };
        super::damage_enemy(&mut cx, eid, 1, Some(InvestigatorId(1)));

        assert!(state.victory_display.is_empty());
        assert_no_event!(events, Event::EnteredVictoryDisplay { .. });
    }

    #[test]
    fn defeating_enemy_without_registry_still_removes_it() {
        let eid = EnemyId(1);
        let mut enemy = test_enemy(1, "Ghoul");
        enemy.max_health = 1;
        let mut state = GameStateBuilder::new().build();
        state.enemies.insert(eid, enemy);
        let mut events = Vec::new();
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };
        super::damage_enemy(&mut cx, eid, 1, Some(InvestigatorId(1)));
        assert!(!state.enemies.contains_key(&eid), "defeated enemy removed");
    }
}
