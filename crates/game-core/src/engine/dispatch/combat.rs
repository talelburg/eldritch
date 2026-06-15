//! Combat helpers: enemy damage, investigator damage/horror, attacks.

use std::collections::BTreeMap;

use crate::engine::EngineOutcome;
use crate::event::Event;
use crate::state::{
    CardInstanceId, DefeatCause, EnemyAttackSource, EnemyId, InvestigatorId, PendingEnemyAttack,
    Status, WindowKind,
};

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

/// A computed damage/horror distribution for one enemy attack (C5b #237).
///
/// The product of [`assign_attack`]: how much of the attack's damage and
/// horror lands on the defending investigator versus each soak-bearing
/// asset. Placed simultaneously by [`place_assignment`], per Rules
/// Reference page 7's "Apply Damage/Horror" clause.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Assignment {
    /// Damage absorbed by the investigator.
    pub investigator_damage: u8,
    /// Horror absorbed by the investigator.
    pub investigator_horror: u8,
    /// instance → damage soaked onto that asset.
    pub asset_damage: BTreeMap<CardInstanceId, u8>,
    /// instance → horror soaked onto that asset.
    pub asset_horror: BTreeMap<CardInstanceId, u8>,
}

/// One eligible soaker for [`assign_attack`] (C5b #237).
///
/// `remaining_health` / `remaining_sanity` are the asset's *remaining*
/// damage / horror capacity — printed stat (registry metadata) minus
/// already-`accumulated_*`. The caller ([`build_soakers`]) derives these
/// so [`assign_attack`] stays a pure function with no registry coupling.
#[derive(Debug)]
pub(super) struct Soaker {
    /// The asset instance that may soak.
    pub instance: CardInstanceId,
    /// Remaining damage capacity (printed health − accumulated damage).
    pub remaining_health: u8,
    /// Remaining horror capacity (printed sanity − accumulated horror).
    pub remaining_sanity: u8,
}

/// Deterministic soak-first assignment of an enemy attack's damage and
/// horror (C5b #237).
///
/// Fills `soakers` (already ordered by the caller, by `CardInstanceId`
/// to match the codebase's other simultaneous loops) up to each one's
/// remaining capacity, then the investigator absorbs the remainder.
/// Damage and horror are assigned **independently** — an asset with
/// only health soaks damage, an asset with only sanity soaks horror.
///
/// Soak-first is the deterministic stand-in for the interactive
/// distribution choice the rules grant the defending investigator: it
/// is the only default that makes a soak reaction (Guard Dog 01021)
/// observable — investigator-first would render the reaction dead code.
///
/// TODO(#44): interactive distribution — replace this body with a parked
/// window surfacing eligible soakers and accepting a player-chosen
/// `{target → points}` distribution, feeding the identical placement
/// path.
pub(super) fn assign_attack(soakers: &[Soaker], mut damage: u8, mut horror: u8) -> Assignment {
    let mut assignment = Assignment::default();
    for soaker in soakers {
        let soaked_damage = damage.min(soaker.remaining_health);
        if soaked_damage > 0 {
            assignment
                .asset_damage
                .insert(soaker.instance, soaked_damage);
            damage -= soaked_damage;
        }
        let soaked_horror = horror.min(soaker.remaining_sanity);
        if soaked_horror > 0 {
            assignment
                .asset_horror
                .insert(soaker.instance, soaked_horror);
            horror -= soaked_horror;
        }
    }
    assignment.investigator_damage = damage;
    assignment.investigator_horror = horror;
    assignment
}

/// Mutable handle to the controlled in-play instance `inst`, or `None`
/// if the investigator doesn't control it (C5b #237).
fn find_controlled_mut(
    state: &mut crate::state::GameState,
    investigator: InvestigatorId,
    inst: CardInstanceId,
) -> Option<&mut crate::state::CardInPlay> {
    state
        .investigators
        .get_mut(&investigator)?
        .cards_in_play
        .iter_mut()
        .find(|c| c.instance_id == inst)
}

/// Discard every controlled asset whose accumulated damage/horror has
/// reached its printed health/sanity (C5b #237).
///
/// Reads printed health/sanity from the card registry; an asset whose
/// metadata can't be resolved (no registry installed, or a non-asset
/// kind) is never defeated here. For each defeated asset: remove it from
/// `cards_in_play` and emit [`Event::CardDiscarded`] with
/// `from: Zone::InPlay` (matching the discard event shape used elsewhere
/// — see `dispatch/cards.rs`).
fn defeat_overflowed_assets(cx: &mut Cx, investigator: InvestigatorId) {
    let Some(reg) = crate::card_registry::current() else {
        return;
    };
    let Some(inv) = cx.state.investigators.get(&investigator) else {
        return;
    };
    // Collect the instances to defeat first (immutable scan), then mutate —
    // avoids holding a borrow across the discard mutation.
    let defeated: Vec<(CardInstanceId, crate::state::CardCode)> = inv
        .cards_in_play
        .iter()
        .filter_map(|card| {
            let meta = (reg.metadata_for)(&card.code)?;
            let crate::card_data::CardKind::Asset { health, sanity, .. } = meta.kind else {
                return None;
            };
            let dmg_defeated = health.is_some_and(|h| card.accumulated_damage >= h);
            let hor_defeated = sanity.is_some_and(|s| card.accumulated_horror >= s);
            (dmg_defeated || hor_defeated).then(|| (card.instance_id, card.code.clone()))
        })
        .collect();

    for (inst, code) in defeated {
        let inv = cx
            .state
            .investigators
            .get_mut(&investigator)
            .unwrap_or_else(|| {
                unreachable!(
                    "defeat_overflowed_assets: investigator {investigator:?} vanished; \
                     state-corruption invariant violation"
                )
            });
        // RR: a defeated asset goes to its owner's discard pile.
        if let Some(pos) = inv.cards_in_play.iter().position(|c| c.instance_id == inst) {
            inv.cards_in_play.remove(pos);
            inv.discard.push(code.clone());
            cx.events.push(Event::CardDiscarded {
                investigator,
                code,
                from: crate::state::Zone::InPlay,
            });
        }
    }
}

/// Place a computed [`Assignment`] simultaneously, then defeat overflowed
/// assets (RR p.7; C5b #237).
///
/// Steps, in order:
/// 1. Accumulate the soaked damage/horror onto each asset's
///    `accumulated_*` fields.
/// 2. Place the investigator's share via the numeric helpers (which emit
///    [`Event::DamageTaken`] / [`Event::HorrorTaken`] and report
///    lethality), then apply investigator defeat if either crossed — so
///    both stats land before any defeat check, per RR p.7.
/// 3. Defeat overflowed assets (`accumulated >= printed stat` →
///    discard).
///
/// Returns the damaged assets that **survive** step 3 — i.e. instances
/// still in `cards_in_play` after defeat resolution. This is the chosen
/// reading of the soak reaction's timing: an asset defeated by the same
/// attack that damaged it has left play before the reaction would
/// resolve, so it does **not** get a reaction window (Guard Dog 01021
/// does not retaliate on the attack that kills it). Only survivors are
/// returned for the [`WindowKind::AfterEnemyAttackDamagedAsset`] queue.
pub(super) fn place_assignment(
    cx: &mut Cx,
    investigator: InvestigatorId,
    assignment: &Assignment,
) -> Vec<CardInstanceId> {
    // 1. Accumulate on assets (simultaneous placement).
    for (inst, dmg) in &assignment.asset_damage {
        if let Some(card) = find_controlled_mut(cx.state, investigator, *inst) {
            card.accumulated_damage = card.accumulated_damage.saturating_add(*dmg);
        }
    }
    for (inst, hor) in &assignment.asset_horror {
        if let Some(card) = find_controlled_mut(cx.state, investigator, *inst) {
            card.accumulated_horror = card.accumulated_horror.saturating_add(*hor);
        }
    }

    // 2. Place the investigator's share (both before any defeat check).
    let dmg_lethal = apply_damage_numeric(cx, investigator, assignment.investigator_damage);
    let hor_lethal = apply_horror_numeric(cx, investigator, assignment.investigator_horror);
    if dmg_lethal || hor_lethal {
        let cause = if dmg_lethal {
            DefeatCause::Damage
        } else {
            DefeatCause::Horror
        };
        super::elimination::apply_investigator_defeat(cx, investigator, cause);
    }

    // 3. Defeat overflowed assets, then return only the surviving damaged
    //    assets (see fn doc for the defeated-soaker timing reading).
    defeat_overflowed_assets(cx, investigator);
    let still_in_play = |inst: &CardInstanceId| {
        cx.state
            .investigators
            .get(&investigator)
            .is_some_and(|inv| inv.cards_in_play.iter().any(|c| c.instance_id == *inst))
    };
    assignment
        .asset_damage
        .keys()
        .copied()
        .filter(still_in_play)
        .collect()
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

    // Soak-first assignment → simultaneous placement → defeat check
    // (RR p.7; C5b #237). `build_soakers` returns empty when no registry
    // is installed or the investigator controls no soak-bearing assets,
    // so the assignment drops all damage/horror on the investigator —
    // behavior-identical to the pre-soak direct-apply path.
    let soakers = build_soakers(cx.state, investigator);
    let assignment = assign_attack(&soakers, damage, horror);
    let damaged_survivors = place_assignment(cx, investigator, &assignment);

    // Queue a soak reaction window per surviving damaged asset. The
    // window only opens if a matching reaction is pending
    // (`queue_reaction_window` no-ops on an empty trigger scan), so this
    // is inert until Guard Dog's `EnemyAttackDamagedSelf` ability + the
    // `trigger_matches` arm land (Tasks 8–11).
    for asset in damaged_survivors {
        super::reaction_windows::queue_reaction_window(
            cx,
            WindowKind::AfterEnemyAttackDamagedAsset {
                asset,
                enemy: enemy_id,
                controller: investigator,
            },
        );
    }
}

/// Build the eligible soakers for an enemy attack against `investigator`
/// (C5b #237).
///
/// Iterates the investigator's `cards_in_play` in order (already
/// `CardInstanceId`-ordered, since instances are pushed in mint order),
/// reads printed health/sanity from the card registry, and emits one
/// [`Soaker`] per controlled asset with any remaining soak capacity
/// (printed stat − accumulated). An asset with `health: None` can't soak
/// damage; `sanity: None` can't soak horror. Assets with both capacities
/// exhausted (or non-asset cards) are skipped. Returns empty when no
/// registry is installed, so attacks resolve as before in registry-free
/// tests.
fn build_soakers(state: &crate::state::GameState, investigator: InvestigatorId) -> Vec<Soaker> {
    let Some(reg) = crate::card_registry::current() else {
        return Vec::new();
    };
    let Some(inv) = state.investigators.get(&investigator) else {
        return Vec::new();
    };
    inv.cards_in_play
        .iter()
        .filter_map(|card| {
            let meta = (reg.metadata_for)(&card.code)?;
            let crate::card_data::CardKind::Asset { health, sanity, .. } = meta.kind else {
                return None;
            };
            let remaining_health = health.unwrap_or(0).saturating_sub(card.accumulated_damage);
            let remaining_sanity = sanity.unwrap_or(0).saturating_sub(card.accumulated_horror);
            if remaining_health == 0 && remaining_sanity == 0 {
                return None;
            }
            Some(Soaker {
                instance: card.instance_id,
                remaining_health,
                remaining_sanity,
            })
        })
        .collect()
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
/// delegate to [`drive_attack_loop`] — which owns the per-attacker
/// steps (early-break-on-defeat, [`enemy_attack`], exhaust) and the
/// soak-window suspend/resume contract (C5b #237).
///
/// **Attack order:** deterministic by [`EnemyId`]. Rules Reference
/// p.25 prescribes "the order of the attacked investigator's
/// choosing" when an investigator is engaged with multiple enemies;
/// `TODO(#143)`: player-pick attack order, unmilestoned, covers both
/// this site and [`fire_attacks_of_opportunity`] (which has the same
/// TODO).
pub(super) fn resolve_attacks_for_investigator(
    cx: &mut Cx,
    investigator: InvestigatorId,
) -> EngineOutcome {
    // Snapshot ready engaged attackers in deterministic EnemyId order.
    // BTreeMap iteration is already key-sorted.
    let attackers: Vec<EnemyId> = cx
        .state
        .enemies
        .iter()
        .filter(|(_, e)| e.engaged_with == Some(investigator) && !e.exhausted)
        .map(|(id, _)| *id)
        .collect();
    drive_attack_loop(cx, investigator, attackers, EnemyAttackSource::EnemyPhase)
}

/// Resolve a list of attackers one at a time, suspending if an attack
/// opens a reaction window (C5b #237). Shared by the enemy phase
/// ([`resolve_attacks_for_investigator`]) and — once Task 13 wires it —
/// attacks of opportunity, distinguished by `source` so
/// [`resume_enemy_attack`] returns to the right driver.
///
/// For each attacker, in order:
///
/// 1. Early-break if `investigator` is no longer [`Status::Active`]
///    (defeated by an earlier attack in the same loop). Remaining
///    attackers do not attack and do not exhaust, per Rules
///    Reference p.10 Elimination step 3 ("All enemies engaged with
///    that player are placed at the location ... unengaged") and p.25
///    ("Each ready, engaged enemy makes an attack" — a disengaged
///    enemy is not "engaged").
///
///    `apply_investigator_defeat` (#144) clears `engaged_with` on every
///    enemy engaged with a defeated investigator, so a disengaged enemy
///    genuinely is no longer "engaged" by the next iteration. The
///    early-break here is therefore redundant with that flow — kept as
///    the simpler, local form so the loop body stays self-evidently
///    correct without cross-referencing the elimination flow.
///
/// 2. Call [`enemy_attack`] (places damage + horror simultaneously per
///    p.7, fires [`super::elimination::apply_investigator_defeat`] if
///    either crosses, and queues a soak window per damaged asset).
///
/// 3. Exhaust the attacker: set `enemy.exhausted = true`, emit
///    [`Event::EnemyExhausted`]. Per Rules Reference p.25, exhaustion
///    happens "Upon completion of dealing the attack (and all abilities
///    triggered by the attack)" — no carve-out for "the attack defeated
///    the target," so an attack that lands and defeats its target still
///    exhausts the attacker. Done here *before* the suspend check
///    (preserving the pre-C5b ordering); the deferred-exhaust-until-
///    after-reactions RR nuance is out of scope.
///
/// 4. If the attack left an open reaction window
///    ([`enemy_attack`] queued one for a soaked asset), park the
///    remaining attackers on
///    [`GameState::pending_enemy_attack`](crate::state::GameState::pending_enemy_attack)
///    and return [`EngineOutcome::AwaitingInput`] for the queued window.
///    [`resume_enemy_attack`] re-enters here when the window closes.
///
/// Returns [`EngineOutcome::Done`] when the list is exhausted with no
/// suspension.
fn drive_attack_loop(
    cx: &mut Cx,
    investigator: InvestigatorId,
    mut attackers: Vec<EnemyId>,
    source: EnemyAttackSource,
) -> EngineOutcome {
    while !attackers.is_empty() {
        let enemy_id = attackers.remove(0);

        // Early-break on defeat. See fn doc step 1.
        let active = cx
            .state
            .investigators
            .get(&investigator)
            .is_some_and(|inv| inv.status == Status::Active);
        if !active {
            break;
        }

        // Damage + horror placement (simultaneous per p.7) + defeat;
        // queues a soak window per damaged asset (C5b #237).
        enemy_attack(cx, enemy_id, investigator);

        // Exhaust the attacker post-resolution (pre-suspend, see step 3).
        let enemy = cx.state.enemies.get_mut(&enemy_id).unwrap_or_else(|| {
            unreachable!(
                "drive_attack_loop: snapshotted enemy {enemy_id:?} is \
                 gone from state.enemies; this is a state-corruption \
                 invariant violation"
            )
        });
        enemy.exhausted = true;
        cx.events.push(Event::EnemyExhausted { enemy: enemy_id });

        // If the attack opened a soak reaction window, suspend: park the
        // rest and surface the queued window (see fn doc step 4).
        if !cx.state.open_windows.is_empty() {
            // Single-soak-window-per-attack invariant: `pending_enemy_attack`
            // holds one parked loop, so resume (which `take()`s it) assumes
            // exactly one soak window per suspension. One attack queues one
            // window per reacting soaker; two would strand the second's
            // resume on `pending_enemy_attack == None`. Unreachable in scope
            // (only Guard Dog 01021 reacts, and two copies need two illegal
            // Ally slots), so guard loudly rather than handle the multi-window
            // drain — that belongs with simultaneous-trigger ordering (#213).
            debug_assert_eq!(
                cx.state.open_windows.len(),
                1,
                "drive_attack_loop suspended on {} open windows; multi-soak-\
                 window-per-attack resume is unhandled (see #213)",
                cx.state.open_windows.len(),
            );
            cx.state.pending_enemy_attack = Some(PendingEnemyAttack {
                investigator,
                remaining_attackers: attackers,
                source,
            });
            return super::reaction_windows::open_queued_reaction_window(cx);
        }
    }
    EngineOutcome::Done
}

/// Re-enter a suspended enemy-attack loop after its soak reaction window
/// closed (C5b #237). Mirror of the other pending-resume drivers
/// (`resume_end_turn` / spawn-engage). Takes the parked
/// [`PendingEnemyAttack`] and re-enters [`drive_attack_loop`] at the
/// next attacker.
///
/// If the loop suspends again (a later attacker also soaks), that
/// [`EngineOutcome::AwaitingInput`] is returned as-is. If it completes,
/// the post-loop step runs by `source`:
///
/// - For [`EnemyAttackSource::EnemyPhase`], advance the enemy-phase
///   cursor and open the next window via [`after_enemy_phase_attacks`].
/// - For [`EnemyAttackSource::AttackOfOpportunity`], return
///   [`EngineOutcome::Done`] (the `AoO` call-site wiring is Task 13;
///   this arm exists so resume from an `AoO` soak is well-defined).
///
/// Called from
/// [`run_window_continuation`](super::reaction_windows::run_window_continuation)'s
/// [`WindowKind::AfterEnemyAttackDamagedAsset`] arm on window close.
pub(super) fn resume_enemy_attack(cx: &mut Cx) -> EngineOutcome {
    let pending = cx.state.pending_enemy_attack.take().unwrap_or_else(|| {
        unreachable!(
            "resume_enemy_attack: no pending_enemy_attack parked; the \
             AfterEnemyAttackDamagedAsset continuation only fires after \
             drive_attack_loop parked one — state-corruption invariant \
             violation"
        )
    });
    let outcome = drive_attack_loop(
        cx,
        pending.investigator,
        pending.remaining_attackers,
        pending.source,
    );
    if matches!(outcome, EngineOutcome::AwaitingInput { .. }) {
        return outcome; // suspended again on a later attacker
    }
    debug_assert!(
        matches!(outcome, EngineOutcome::Done),
        "drive_attack_loop returned unexpected {outcome:?} (only Done / \
         AwaitingInput are possible — it never rejects)"
    );
    match pending.source {
        EnemyAttackSource::EnemyPhase => {
            super::reaction_windows::after_enemy_phase_attacks(cx, pending.investigator)
        }
        EnemyAttackSource::AttackOfOpportunity => EngineOutcome::Done,
    }
}

#[cfg(test)]
mod combat_tests {
    use super::super::Cx;
    use crate::event::Event;
    use crate::state::{EnemyId, InvestigatorId};
    use crate::test_support::{test_enemy, test_investigator, GameStateBuilder};
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

    #[test]
    fn enemy_attack_with_no_soakers_matches_old_behavior() {
        // Regression guard for the assign/place/window rewrite: an attack
        // of 2 damage / 1 horror against an investigator controlling no
        // soak-bearing assets must land entirely on the investigator, just
        // as the pre-rewrite direct apply_damage/horror_numeric path did.
        let id = InvestigatorId(1);
        let eid = EnemyId(1);
        let mut inv = test_investigator(1);
        inv.max_health = 10;
        inv.max_sanity = 10;

        let mut enemy = test_enemy(1, "Ghoul");
        enemy.attack_damage = 2;
        enemy.attack_horror = 1;

        let mut state = GameStateBuilder::new().with_investigator(inv).build();
        state.enemies.insert(eid, enemy);
        let mut events = Vec::new();
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };

        super::enemy_attack(&mut cx, eid, id);

        assert_eq!(state.investigators[&id].damage, 2, "all damage on inv");
        assert_eq!(state.investigators[&id].horror, 1, "all horror on inv");
        assert_event!(events, Event::DamageTaken { investigator, amount: 2 } if *investigator == id);
        assert_event!(events, Event::HorrorTaken { investigator, amount: 1 } if *investigator == id);
        assert!(
            state.open_windows.is_empty(),
            "no soak window without soakers"
        );
    }

    #[test]
    fn assign_attack_fills_soaker_before_investigator() {
        // 1 ally with remaining health 3, attack deals 2 damage / 0 horror →
        // all 2 damage soaks onto the ally, none on the investigator.
        let inst = crate::state::CardInstanceId(7);
        let soakers = [super::Soaker {
            instance: inst,
            remaining_health: 3,
            remaining_sanity: 1,
        }];
        let assignment = super::assign_attack(&soakers, 2, 0);
        assert_eq!(assignment.investigator_damage, 0);
        assert_eq!(assignment.investigator_horror, 0);
        assert_eq!(assignment.asset_damage.get(&inst), Some(&2));
        assert!(assignment.asset_horror.is_empty());
    }

    #[test]
    fn assign_attack_overflows_to_investigator_past_capacity() {
        // Ally with remaining health 1, attack deals 2 damage → 1 soaks onto
        // the ally, 1 overflows onto the investigator.
        let inst = crate::state::CardInstanceId(7);
        let soakers = [super::Soaker {
            instance: inst,
            remaining_health: 1,
            remaining_sanity: 0,
        }];
        let assignment = super::assign_attack(&soakers, 2, 0);
        assert_eq!(assignment.asset_damage.get(&inst), Some(&1));
        assert_eq!(assignment.investigator_damage, 1);
        // Horror side trivially zero (attack deals no horror) — asserted so
        // the test is a complete contract, not a damage-only partial.
        assert_eq!(assignment.investigator_horror, 0);
        assert!(assignment.asset_horror.is_empty());
    }

    #[test]
    fn place_assignment_accumulates_on_asset_and_returns_damaged_list() {
        // Pre-construct an Assignment placing 1 damage + 1 horror on an
        // in-play asset and 1 damage on the investigator. No registry is
        // installed, so the asset is NOT defeated (printed health/sanity
        // unreadable) — accumulation + the returned damaged-survivors list
        // are what's verified here. Defeat-on-overflow needs registry
        // metadata and is covered by the EU5 integration test.
        use crate::state::{CardCode, CardInPlay, CardInstanceId};
        use std::collections::BTreeMap;

        let id = InvestigatorId(1);
        let inst = CardInstanceId(7);
        let mut inv = test_investigator(1);
        inv.max_health = 10;
        inv.max_sanity = 10;
        inv.cards_in_play = vec![CardInPlay::enter_play(CardCode::new("01021"), inst)];

        let mut state = GameStateBuilder::new().with_investigator(inv).build();
        let mut events = Vec::new();
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };

        let mut asset_damage = BTreeMap::new();
        asset_damage.insert(inst, 1u8);
        let mut asset_horror = BTreeMap::new();
        asset_horror.insert(inst, 1u8);
        let assignment = super::Assignment {
            investigator_damage: 1,
            investigator_horror: 0,
            asset_damage,
            asset_horror,
        };

        let survivors = super::place_assignment(&mut cx, id, &assignment);

        let card = &state.investigators[&id].cards_in_play[0];
        assert_eq!(card.accumulated_damage, 1, "asset soaked 1 damage");
        assert_eq!(card.accumulated_horror, 1, "asset soaked 1 horror");
        assert_eq!(
            state.investigators[&id].damage, 1,
            "investigator took overflow damage"
        );
        assert_eq!(
            survivors,
            vec![inst],
            "the surviving damaged asset is returned for the soak window"
        );
        assert_event!(events, Event::DamageTaken { investigator, amount: 1 } if *investigator == id);
    }

    #[test]
    fn resume_enemy_attack_drains_remaining_attackers_and_advances_cursor() {
        // EU5 deferral: firing Guard Dog's reaction end-to-end across two
        // attackers needs the real `cards` registry (so `trigger_matches`
        // finds the ability and a soak window genuinely opens mid-loop) —
        // that is the EU5 integration test. This lib-level check exercises
        // the resume half directly: park a `pending_enemy_attack` with two
        // remaining attackers (as `drive_attack_loop` would on suspend) plus
        // an already-open soak window, then call `resume_enemy_attack` and
        // assert it drains both attackers (exhausting each) and advances the
        // enemy-phase cursor past the (sole) investigator to `None`.
        use crate::state::{EnemyAttackSource, InvestigatorId, PendingEnemyAttack};

        let inv_id = InvestigatorId(1);
        let second = EnemyId(2);
        let third = EnemyId(3);

        let mut e2 = test_enemy(2, "Second Attacker");
        e2.engaged_with = Some(inv_id);
        let mut e3 = test_enemy(3, "Third Attacker");
        e3.engaged_with = Some(inv_id);

        // The real resume path runs AFTER `close_reaction_window_at` popped
        // the soak window the loop suspended on, so `open_windows` is empty
        // when `resume_enemy_attack` re-enters `drive_attack_loop`.
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_turn_order([inv_id])
            .with_enemy(e2)
            .with_enemy(e3)
            .build();
        // The enemy phase set the cursor to this investigator before opening
        // the BeforeInvestigatorAttacked window; resume must advance it.
        state.enemy_attack_pending = Some(inv_id);
        state.pending_enemy_attack = Some(PendingEnemyAttack {
            investigator: inv_id,
            remaining_attackers: vec![second, third],
            source: EnemyAttackSource::EnemyPhase,
        });

        let mut events = Vec::new();
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };
        let outcome = super::resume_enemy_attack(&mut cx);

        // Both parked attackers resolved and exhausted; the parked slot is
        // cleared (taken). No registry → no new soak window, no re-suspend.
        assert!(
            state.pending_enemy_attack.is_none(),
            "resume consumed the parked attack"
        );
        assert!(
            state.enemies[&second].exhausted,
            "second attacker exhausted"
        );
        assert!(state.enemies[&third].exhausted, "third attacker exhausted");
        assert_event!(events, Event::EnemyExhausted { enemy } if *enemy == second);
        assert_event!(events, Event::EnemyExhausted { enemy } if *enemy == third);
        // Loop finished → `after_enemy_phase_attacks` advanced the cursor
        // past the only investigator to None and opened the all-attacked
        // window (auto-skips inline with no registry).
        assert!(
            state.enemy_attack_pending.is_none(),
            "cursor advanced past the sole investigator to None"
        );
        // Outcome is whatever the all-investigators-attacked continuation
        // returns (Done when it auto-skips and cascades). The contract this
        // test pins is the drain + cursor advance, not the cascade tail.
        let _ = outcome;
    }

    #[test]
    fn assign_attack_soaks_damage_and_horror_independently() {
        // Two soakers: A has only health, B has only sanity. Attack 1/1 →
        // damage to A, horror to B, nothing to the investigator.
        let a = crate::state::CardInstanceId(1);
        let b = crate::state::CardInstanceId(2);
        let soakers = [
            super::Soaker {
                instance: a,
                remaining_health: 2,
                remaining_sanity: 0,
            },
            super::Soaker {
                instance: b,
                remaining_health: 0,
                remaining_sanity: 2,
            },
        ];
        let assignment = super::assign_attack(&soakers, 1, 1);
        assert_eq!(assignment.asset_damage.get(&a), Some(&1));
        assert!(!assignment.asset_damage.contains_key(&b));
        assert_eq!(assignment.asset_horror.get(&b), Some(&1));
        assert!(!assignment.asset_horror.contains_key(&a));
        assert_eq!(assignment.investigator_damage, 0);
        assert_eq!(assignment.investigator_horror, 0);
    }
}
