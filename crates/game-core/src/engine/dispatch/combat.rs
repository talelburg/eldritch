//! Combat helpers: enemy damage, investigator damage/horror, attacks.

use std::collections::BTreeMap;

use crate::engine::EngineOutcome;
use crate::event::Event;
use crate::state::{
    AttackLoopPhase, CardInstanceId, DefeatCause, EnemyAttackSource, EnemyId, GameState,
    InvestigatorId, PendingEnemyAttack, Status,
};

use super::Cx;

/// The single enemy currently engaged with `investigator`, or `None` if
/// zero or 2+ are engaged. `Effect::Fight` auto-targets via this; the
/// 2+ case is a deferred interactive choice (lands with the #212/#213
/// interactive-choice cluster), so the activation check rejects it.
pub(crate) fn single_engaged_enemy(
    state: &GameState,
    investigator: InvestigatorId,
) -> Option<EnemyId> {
    let mut engaged = state
        .enemies
        .iter()
        .filter(|(_, e)| e.engaged_with == Some(investigator))
        .map(|(id, _)| *id);
    let first = engaged.next()?;
    if engaged.next().is_some() {
        None
    } else {
        Some(first)
    }
}

/// Enemies matching an [`EntityScope`](crate::dsl::EntityScope), in `BTreeMap`
/// (id) order so the `OptionId` index replays deterministically. Shared by the
/// evaluator's choice-grounding and the activation pre-cost target check.
pub(crate) fn enemies_in_scope(
    state: &GameState,
    controller: InvestigatorId,
    scope: crate::dsl::EntityScope,
) -> Vec<EnemyId> {
    use crate::dsl::{EntityScope, LocationSet};
    let EntityScope::At(set) = scope;
    match set {
        LocationSet::Anywhere => state.enemies.keys().copied().collect(),
        LocationSet::Here => match state
            .investigators
            .get(&controller)
            .and_then(|i| i.current_location)
        {
            Some(here) => state
                .enemies
                .iter()
                .filter(|(_, e)| e.current_location == Some(here))
                .map(|(id, _)| *id)
                .collect(),
            None => Vec::new(),
        },
    }
}

/// Public entry point for card effects to deal damage to an enemy.
///
/// A thin wrapper over `damage_enemy` (which is crate-internal) so the
/// `cards` crate can resolve `Effect::Native` retaliate effects — first
/// consumer: Guard Dog 01021's "Deal 1 damage to the attacking enemy."
/// Reusing `damage_enemy` means a card that defeats its target here runs
/// the same defeat cascade (`EnemyDefeated`, victory display) as the Fight
/// action — intended. (C5b #237.)
pub fn deal_damage_to_enemy(
    cx: &mut Cx,
    enemy_id: EnemyId,
    amount: u8,
    by: Option<InvestigatorId>,
) {
    damage_enemy(cx, enemy_id, amount, by);
}

/// Apply `amount` damage to an enemy. If the new damage reaches or
/// exceeds `max_health`, emit `EnemyDefeated` and remove the enemy
/// from `state.enemies`. `by` attributes the defeat for
/// trigger-window consumers (e.g. Roland's reaction). Used by Fight
/// today and by card effects via [`deal_damage_to_enemy`].
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
        // Enemy defeated: dispatch the timing point through the unified
        // chokepoint (Axis-B T5a). `emit_event` queues the after-defeat
        // reaction window (Roland 01001 — `Event::WindowOpened` emitted now;
        // the skill-test driver suspends at its next step boundary so the
        // player can react) and then fires the forced act objectives (Act 3's
        // advance-on-Ghoul-Priest-defeat). `()`/debug_assert guards the
        // 2+-trigger forced case (#213).
        let forced = super::emit::emit_event(
            cx,
            &super::emit::TimingEvent::EnemyDefeated {
                enemy: enemy_id,
                by,
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
/// Returns the **damaged surviving soaker assets** (the
/// [`place_assignment`] survivor list) so the caller can queue one
/// [`WindowKind::AfterEnemyAttackDamagedAsset`] reaction window per
/// survivor. This function does **not** queue the windows itself: the
/// enemy phase ([`drive_attack_loop`]) drives them (suspending the loop),
/// while attacks of opportunity ([`fire_attacks_of_opportunity`])
/// deliberately drop the list — they soak but don't yet open soak
/// windows, because that needs mid-action suspension of the triggering
/// action (deferred fast-follow, `TODO(#293)`). Keeping the queueing at the call site
/// is what lets the two callers diverge without `enemy_attack` stranding
/// an undriven window (C5b #237).
///
/// [`AwaitingInput`]: crate::engine::EngineOutcome::AwaitingInput
pub(super) fn enemy_attack(
    cx: &mut Cx,
    enemy_id: EnemyId,
    investigator: InvestigatorId,
) -> Vec<CardInstanceId> {
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
    place_assignment(cx, investigator, &assignment)
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
        // AoO soaks damage onto assets (the `enemy_attack` placement runs
        // fully) but does NOT open soak reaction windows: the returned
        // damaged-survivor list is deliberately dropped. Guard Dog 01021
        // therefore does not retaliate against attacks of opportunity yet —
        // a documented faithfulness gap. Likewise, AoO does not open the
        // before-attack cancel window (Dodge 01023, Axis D #336): it calls
        // `enemy_attack` directly, not `drive_attack_loop`. Both gaps share
        // one cause — driving a window here would require suspending the
        // *triggering* action (Move / Investigate) and resuming its primary
        // effect after the window closes, a mid-action suspension mechanism
        // deferred to `TODO(#293)` (which also keeps the AoO non-exhaust rule,
        // RR p.7, distinct from the enemy-phase loop's always-exhaust). Dropping
        // the survivors is exactly what prevents an undriven window stranded
        // on `open_windows` (the bug this seam fixes; C5b #237).
        let _ = enemy_attack(cx, enemy_id, investigator);
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
///    either crosses) and queue a soak window per damaged surviving asset
///    it returns. The queueing lives here, not in `enemy_attack`, so the
///    `AoO` caller can drop the survivors without stranding a window.
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
/// Deal one attacker's damage (unless `cancelled`), queue its soak window(s),
/// and exhaust it. The open-window suspend check is left to the caller (so the
/// before-cancel and soak resume paths can both reuse this body). Enemy-phase
/// only — attacks of opportunity neither route through here nor exhaust (RR
/// p.7); see [`fire_attacks_of_opportunity`].
fn process_attacker_dealing(
    cx: &mut Cx,
    investigator: InvestigatorId,
    enemy_id: EnemyId,
    cancelled: bool,
) {
    // Unless cancelled (Dodge 01023, RR p.6): damage + horror placement
    // (simultaneous per p.7) + defeat; returns the damaged surviving soaker
    // assets (C5b #237). On cancel, no damage/horror is dealt and no soaker
    // can survive, so no soak window opens.
    if !cancelled {
        let damaged_survivors = enemy_attack(cx, enemy_id, investigator);

        // Queue a soak reaction window per surviving damaged asset, BEFORE the
        // exhaust step below — preserving the historical order in which the
        // window's `WindowOpened` precedes `EnemyExhausted`. Inert unless a
        // soaker has an `EnemyAttackDamagedSelf` reaction (Guard Dog 01021).
        // Queued here, not in `enemy_attack`, so AoO (which shares
        // `enemy_attack`) doesn't strand an undriven window (C5b #237).
        for asset in damaged_survivors {
            let _ = super::emit::emit_event(
                cx,
                &super::emit::TimingEvent::EnemyAttackDamagedSelf {
                    asset,
                    enemy: enemy_id,
                    controller: investigator,
                },
            );
        }
    }

    // Exhaust the attacker — even on cancel. RR p.6: a cancelled attack is
    // "still regarded as initiated"; RR p.25: the enemy exhausts "upon
    // completion of dealing the attack". The attack was made; only its effect
    // was prevented. (AoO never exhausts, RR p.7 — but AoO doesn't reach here.)
    let enemy = cx.state.enemies.get_mut(&enemy_id).unwrap_or_else(|| {
        unreachable!(
            "process_attacker_dealing: snapshotted enemy {enemy_id:?} is \
             gone from state.enemies; this is a state-corruption \
             invariant violation"
        )
    });
    enemy.exhausted = true;
    cx.events.push(Event::EnemyExhausted { enemy: enemy_id });
}

/// Park the loop on the soak reaction window the just-dealt attack opened and
/// surface it (C5b #237). Called only when `open_windows()` is non-empty.
fn park_on_soak_window(
    cx: &mut Cx,
    investigator: InvestigatorId,
    remaining_attackers: Vec<EnemyId>,
    source: EnemyAttackSource,
) -> EngineOutcome {
    // Single-soak-window-per-attack invariant: `pending_enemy_attack` holds one
    // parked loop, so resume (which `take()`s it) drains exactly one soak window
    // per suspension. A single attack producing 2+ soak windows is
    // **unconstructible in the current model**, three independent ways: (a)
    // Guard Dog 01021 is the only card with the `EnemyAttackDamagedSelf`
    // retaliate reaction; (b) it is an Ally — the single Ally slot forbids two
    // copies in play; (c) `assign_attack` fills each soaker to capacity before
    // the next, so any non-final damaged soaker reaches its health and is
    // defeated (the survivor filter then gives it no window). So guard loudly
    // rather than carry an unexercised multi-window drain.
    //
    // This unlocks once *any* of those changes (#294): a second soak reactor, an
    // Ally-slot-granting permanent (Charisma) plus a second reactor, or — the one
    // that makes it reachable on its own — player-chosen damage distribution
    // (today `assign_attack` is a fill-to-capacity default standing in for that
    // choice), which lets one attack damage two soakers without defeating either.
    // Then resume must drain all same-attack windows before continuing
    // (coordinates with simultaneous-trigger ordering #213).
    debug_assert_eq!(
        cx.state.open_windows().len(),
        1,
        "drive_attack_loop suspended on {} soak windows; the multi-\
         window-per-attack drain is unconstructible in scope (one \
         soak reactor, one Ally slot, fill-to-capacity assignment) — \
         reachable only once #294's avenues land",
        cx.state.open_windows().len(),
    );
    cx.state.pending_enemy_attack = Some(PendingEnemyAttack {
        investigator,
        remaining_attackers,
        source,
        phase: AttackLoopPhase::AfterSoak,
    });
    super::reaction_windows::open_queued_reaction_window(cx)
}

/// Resolve the head attacker: remove it from `attackers`, deal-or-skip its
/// attack (per `cancelled`) and exhaust it, then — if the attack opened a soak
/// reaction window — park the loop on it (`AfterSoak`) and return the suspend.
/// `None` means continue to the next attacker. This is the single shared
/// "deal one + maybe suspend" step for both [`drive_attack_loop`] (head never
/// cancelled) and the `BeforeAttack` resume in [`resume_enemy_attack`] (head
/// cancelled iff the closed before-window cancelled it). (Axis D #336.)
fn deal_head_and_maybe_park(
    cx: &mut Cx,
    investigator: InvestigatorId,
    attackers: &mut Vec<EnemyId>,
    source: EnemyAttackSource,
    cancelled: bool,
) -> Option<EngineOutcome> {
    let enemy_id = attackers.remove(0);
    process_attacker_dealing(cx, investigator, enemy_id, cancelled);
    if cx.state.open_windows().is_empty() {
        None
    } else {
        Some(park_on_soak_window(
            cx,
            investigator,
            std::mem::take(attackers),
            source,
        ))
    }
}

fn drive_attack_loop(
    cx: &mut Cx,
    investigator: InvestigatorId,
    mut attackers: Vec<EnemyId>,
    source: EnemyAttackSource,
) -> EngineOutcome {
    while let Some(&enemy_id) = attackers.first() {
        // Early-break on defeat. See fn doc step 1.
        let active = cx
            .state
            .investigators
            .get(&investigator)
            .is_some_and(|inv| inv.status == Status::Active);
        if !active {
            break;
        }

        // Before-attack cancel window (Axis D #336): reaction-only Before
        // timing point. Opens iff a co-located cancel reaction is available
        // (Dodge in hand, or an in-play reaction); `emit_event` only queues a
        // window when the scan finds a candidate. Suspend BEFORE dealing
        // damage, keeping the head attacker at the front of `attackers` so the
        // `BeforeAttack` resume processes it (deal-or-cancel).
        let _ = super::emit::emit_event(
            cx,
            &super::emit::TimingEvent::EnemyAttacks {
                enemy: enemy_id,
                investigator,
            },
        );
        if !cx.state.open_windows().is_empty() {
            cx.state.pending_enemy_attack = Some(PendingEnemyAttack {
                investigator,
                remaining_attackers: attackers,
                source,
                phase: AttackLoopPhase::BeforeAttack,
            });
            return super::reaction_windows::open_queued_reaction_window(cx);
        }

        // No cancel reaction available: deal this (un-cancelled) attacker,
        // suspending if it opens a soak window.
        if let Some(suspended) =
            deal_head_and_maybe_park(cx, investigator, &mut attackers, source, false)
        {
            return suspended;
        }
    }
    EngineOutcome::Done
}

/// Re-enter a suspended enemy-attack loop after the reaction window it parked
/// on closed. Mirror of the other pending-resume drivers (`resume_end_turn` /
/// spawn-engage). Takes the parked [`PendingEnemyAttack`] and resumes per its
/// [`AttackLoopPhase`]:
///
/// - [`AttackLoopPhase::BeforeAttack`] (Axis D #336): the before-attack cancel
///   window closed. Read-and-clear `pending_cancellation`, then deal-or-skip
///   the head attacker (still at the front of `remaining_attackers`) via
///   [`process_attacker_dealing`] and exhaust it. If *that* attack opens a soak
///   window, re-park as `AfterSoak`; otherwise drain the rest.
/// - [`AttackLoopPhase::AfterSoak`] (C5b #237): the soak window closed; drain
///   the remaining attackers.
///
/// If the loop suspends again, that [`EngineOutcome::AwaitingInput`] is returned
/// as-is. On completion the post-loop step runs by `source`:
///
/// - [`EnemyAttackSource::EnemyPhase`]: advance the enemy-phase cursor and open
///   the next window via [`after_enemy_phase_attacks`].
/// - [`EnemyAttackSource::AttackOfOpportunity`]: return [`EngineOutcome::Done`].
///   **Currently unreachable** — attacks of opportunity
///   ([`fire_attacks_of_opportunity`]) open neither soak nor cancel windows, so
///   they never park. Reserved for the deferred fast-follow (`TODO(#293)`) that
///   suspends the triggering action; kept so the source-keyed dispatch stays
///   total (C5b #237).
///
/// Called from
/// [`run_window_continuation`](super::reaction_windows::run_window_continuation)'s
/// [`WindowKind::AfterEnemyAttackDamagedAsset`] / [`WindowKind::BeforeEnemyAttack`]
/// arm on window close.
pub(super) fn resume_enemy_attack(cx: &mut Cx) -> EngineOutcome {
    let PendingEnemyAttack {
        investigator,
        mut remaining_attackers,
        source,
        phase,
    } = cx.state.pending_enemy_attack.take().unwrap_or_else(|| {
        unreachable!(
            "resume_enemy_attack: no pending_enemy_attack parked; the \
             soak / before-attack continuations only fire after \
             drive_attack_loop parked one — state-corruption invariant \
             violation"
        )
    });

    if phase == AttackLoopPhase::BeforeAttack {
        // The before-attack cancel window for the head attacker closed. If a
        // reaction cancelled the attack (Dodge played `Effect::Cancel`), skip
        // its damage; either way the head is dealt-or-skipped + exhausted (RR
        // p.6 + p.25), and a non-cancelled attack may open its own soak window.
        let cancelled = std::mem::take(&mut cx.state.pending_cancellation);
        if let Some(suspended) = deal_head_and_maybe_park(
            cx,
            investigator,
            &mut remaining_attackers,
            source,
            cancelled,
        ) {
            return suspended;
        }
    }

    let outcome = drive_attack_loop(cx, investigator, remaining_attackers, source);
    if matches!(outcome, EngineOutcome::AwaitingInput { .. }) {
        return outcome; // suspended again on a later attacker
    }
    debug_assert!(
        matches!(outcome, EngineOutcome::Done),
        "drive_attack_loop returned unexpected {outcome:?} (only Done / \
         AwaitingInput are possible — it never rejects)"
    );
    match source {
        EnemyAttackSource::EnemyPhase => {
            super::reaction_windows::after_enemy_phase_attacks(cx, investigator)
        }
        EnemyAttackSource::AttackOfOpportunity => EngineOutcome::Done,
    }
}

#[cfg(test)]
mod combat_tests {
    use super::super::Cx;
    use super::single_engaged_enemy;
    use crate::event::Event;
    use crate::state::{EnemyId, InvestigatorId};
    use crate::test_support::{test_enemy, test_investigator, GameStateBuilder};
    use crate::{assert_event, assert_no_event};

    #[test]
    fn single_engaged_enemy_some_for_one_none_for_zero_or_two() {
        let inv_id = InvestigatorId(1);
        let mut e1 = test_enemy(100, "A");
        e1.engaged_with = Some(inv_id);

        // Exactly one engaged → Some.
        let s1 = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_enemy(e1.clone())
            .build();
        assert_eq!(single_engaged_enemy(&s1, inv_id), Some(EnemyId(100)));

        // Two engaged → None (deferred multi-target selection).
        let mut e2 = test_enemy(101, "B");
        e2.engaged_with = Some(inv_id);
        let s2 = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_enemy(e1)
            .with_enemy(e2)
            .build();
        assert_eq!(single_engaged_enemy(&s2, inv_id), None);

        // Zero engaged → None.
        let s0 = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .build();
        assert_eq!(single_engaged_enemy(&s0, inv_id), None);
    }

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
            state.open_windows().is_empty(),
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
        use crate::state::{
            AttackLoopPhase, EnemyAttackSource, InvestigatorId, PendingEnemyAttack,
        };

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
            phase: AttackLoopPhase::AfterSoak,
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

    /// Axis D #336: a `BeforeAttack`-parked resume with `pending_cancellation`
    /// set (a cancel reaction fired in the before-window) skips the head
    /// attacker's damage but still exhausts it (RR p.6 + p.25), then clears
    /// the flag. Exercises the resume half directly (no registry needed).
    #[test]
    fn resume_before_attack_cancel_skips_damage_but_exhausts() {
        use crate::state::{
            AttackLoopPhase, EnemyAttackSource, InvestigatorId, PendingEnemyAttack,
        };

        let inv_id = InvestigatorId(1);
        let attacker = EnemyId(2);
        let mut enemy = test_enemy(2, "Attacker"); // attack_damage: 1
        enemy.engaged_with = Some(inv_id);

        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_turn_order([inv_id])
            .with_enemy(enemy)
            .build();
        state.enemy_attack_pending = Some(inv_id);
        state.pending_cancellation = true; // a cancel reaction fired in the window
        state.pending_enemy_attack = Some(PendingEnemyAttack {
            investigator: inv_id,
            remaining_attackers: vec![attacker], // head still present (BeforeAttack)
            source: EnemyAttackSource::EnemyPhase,
            phase: AttackLoopPhase::BeforeAttack,
        });

        let mut events = Vec::new();
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };
        let _ = super::resume_enemy_attack(&mut cx);

        assert_eq!(
            state.investigators[&inv_id].damage, 0,
            "cancelled attack deals no damage"
        );
        assert!(
            !state.pending_cancellation,
            "the cancel flag is consumed on resume"
        );
        assert!(
            state.enemies[&attacker].exhausted,
            "a cancelled attack still exhausts the attacker (RR p.6 + p.25)"
        );
        assert_no_event!(events, Event::DamageTaken { .. });
        assert_event!(events, Event::EnemyExhausted { enemy } if *enemy == attacker);
    }

    /// Axis D #336 companion: a `BeforeAttack` resume *without* the cancel flag
    /// deals the head attacker's damage normally, then exhausts it.
    #[test]
    fn resume_before_attack_without_cancel_deals_damage() {
        use crate::state::{
            AttackLoopPhase, EnemyAttackSource, InvestigatorId, PendingEnemyAttack,
        };

        let inv_id = InvestigatorId(1);
        let attacker = EnemyId(2);
        let mut enemy = test_enemy(2, "Attacker"); // attack_damage: 1
        enemy.engaged_with = Some(inv_id);

        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_turn_order([inv_id])
            .with_enemy(enemy)
            .build();
        state.enemy_attack_pending = Some(inv_id);
        // pending_cancellation defaults to false (no reaction cancelled).
        state.pending_enemy_attack = Some(PendingEnemyAttack {
            investigator: inv_id,
            remaining_attackers: vec![attacker],
            source: EnemyAttackSource::EnemyPhase,
            phase: AttackLoopPhase::BeforeAttack,
        });

        let mut events = Vec::new();
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };
        let _ = super::resume_enemy_attack(&mut cx);

        assert_eq!(
            state.investigators[&inv_id].damage, 1,
            "an un-cancelled attack deals its damage"
        );
        assert!(state.enemies[&attacker].exhausted, "attacker exhausted");
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
