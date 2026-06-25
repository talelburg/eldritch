//! Combat helpers: enemy damage, investigator damage/horror, attacks.

use crate::engine::outcome::{InputRequest, OptionId, ResumeToken};
use crate::engine::EngineOutcome;
use crate::event::Event;
use crate::state::{
    Assignment, AttackLoopStage, CardInstanceId, Continuation, DefeatCause, EnemyAttackSource,
    EnemyId, GameState, InvestigatorId, Status,
};

use super::Cx;

/// The scope of enemies a Fight (basic action or weapon `Effect::Fight`) may
/// target: any enemy *at your location*. Per RR you choose an enemy at your
/// location to attack and need not already be engaged, so this is co-located
/// (`At(Here)`), not engaged-only (#451). Single source of truth shared by the
/// activation pre-cost gate (`check_effect_target_available`) and the
/// evaluator's target grounding (`ground_fight_target_choice`), so the two
/// can't drift.
pub(crate) fn fight_target_scope() -> crate::dsl::EntityScope {
    crate::dsl::EntityScope::At(crate::dsl::LocationSet::Here)
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
        // reaction window (Roland 01001 — the window opens now;
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

// `Assignment` (the computed damage/horror distribution) lives in
// `crate::state` alongside the other `Continuation` payload types, since the
// interactive distribution (#44/K5b) parks an in-progress one on a
// `Continuation::DamageAssignment` frame. Imported above.

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
/// Soak-first deterministic assignment, used by [`soak_and_place`] when no
/// point is contested (no soaker with capacity) and by the **non-attack/effect**
/// path until K5b-2. The attack path's interactive per-point distribution
/// (#44/K5b-1) lives in [`deal_head_and_maybe_park`] /
/// [`resume_damage_assignment`]; `TODO(#44)`: route the effect path
/// (`take_damage`/`take_horror`) through it too (K5b-2).
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
/// returned for the after-enemy-attack-damaged-asset reaction window queue.
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
/// when the return is `true`. This split exists so [`place_assignment`]
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
    inv.investigator_card.accumulated_damage = inv
        .investigator_card
        .accumulated_damage
        .saturating_add(amount);
    let lethal = inv.damage() >= inv.max_health();
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
    inv.investigator_card.accumulated_horror = inv
        .investigator_card
        .accumulated_horror
        .saturating_add(amount);
    let lethal = inv.horror() >= inv.max_sanity();
    cx.events.push(Event::HorrorTaken {
        investigator,
        amount,
    });
    lethal
}

/// Distribute `damage` + `horror` to `investigator` across eligible soakers
/// then self (soak-first, RR p.7), place simultaneously, and defeat overflowed
/// assets — the shared soak entry for **both** enemy attacks and non-attack
/// card/treachery harm (#44/K5a). Returns the damaged surviving soaker assets
/// (the [`place_assignment`] survivor list) so an attack caller can queue one
/// after-enemy-attack-damaged-asset reaction window per survivor;
/// non-attack callers pass one of `damage`/`horror` as 0 and ignore the return
/// (treachery harm opens no soak reaction window — Guard Dog 01021 retaliates
/// only to enemy *attacks*).
///
/// `build_soakers` returns empty when no registry is installed or the
/// investigator controls no soak-bearing asset, so the assignment then drops
/// all damage/horror on the investigator — behavior-identical to the pre-soak
/// direct-apply path.
pub(super) fn soak_and_place(
    cx: &mut Cx,
    investigator: InvestigatorId,
    damage: u8,
    horror: u8,
) -> Vec<CardInstanceId> {
    let soakers = build_soakers(cx.state, investigator);
    let assignment = assign_attack(&soakers, damage, horror);
    place_assignment(cx, investigator, &assignment)
}

/// Interactive soak entry (#44 / K5b-2): distribute `damage`/`horror` soak-first
/// as far as deterministic; the moment a point is **contested** (a controlled
/// soaker can take it) suspend on the player's per-point distribution prompt,
/// parking the rest on a [`Continuation::DamageAssignment`] frame carrying
/// `source`. Otherwise (no soaker can take any point) place synchronously and
/// return `Done` — the uncontested path is behaviour-identical to
/// [`soak_and_place`]. The effect/treachery `Effect::Deal` path uses this with
/// `DamageSource::Effect`; the enemy-attack path has its own loop-aware entry
/// (`deal_head_and_maybe_park`). Mirrors that gating exactly.
pub(crate) fn soak_and_distribute(
    cx: &mut Cx,
    investigator: InvestigatorId,
    damage: u8,
    horror: u8,
    source: crate::state::DamageSource,
) -> EngineOutcome {
    let mut assignment = Assignment::default();
    let (mut rd, mut rh) = (damage, horror);
    let soakers = build_soakers(cx.state, investigator);
    if advance_distribution(&soakers, &mut rd, &mut rh, &mut assignment).is_none() {
        cx.state.continuations.push(Continuation::DamageAssignment {
            investigator,
            remaining_damage: rd,
            remaining_horror: rh,
            assignment,
            source,
        });
        return prompt_current_point(cx, investigator);
    }
    let _ = place_assignment(cx, investigator, &assignment);
    EngineOutcome::Done
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
/// `investigator`, driving them through the shared attack loop (#293) so each
/// `AoO` opens its before-attack cancel window (Dodge 01023) and per-soaked-asset
/// reaction window (Guard Dog 01021). Returns [`EngineOutcome::AwaitingInput`]
/// if a window suspends the loop, [`EngineOutcome::Done`] otherwise. With 2+
/// engaged ready enemies the loop suspends for the player's attack-order pick
/// (#143, RR p.25 step 3.3); a single attacker resolves inline. `AoO` attackers
/// never exhaust (RR p.7) — honored by
/// [`EnemyAttackSource::AttackOfOpportunity`].
pub(super) fn drive_aoo(cx: &mut Cx, investigator: InvestigatorId) -> EngineOutcome {
    let attackers: Vec<EnemyId> = cx
        .state
        .enemies
        .iter()
        .filter(|(_, e)| e.engaged_with == Some(investigator) && !e.exhausted)
        .map(|(id, _)| *id)
        .collect();
    drive_attack_loop(
        cx,
        investigator,
        attackers,
        EnemyAttackSource::AttackOfOpportunity,
    )
}

/// Fire a single Retaliate attack from `enemy` against `investigator`, driving it
/// through the shared attack loop (#379) so it opens the before-attack cancel
/// window (Dodge 01023) and the per-soaked-asset reaction window (Guard Dog 01021).
/// A retaliate is one enemy attacking once, so the attacker list is a singleton;
/// the two sequential suspension points are tracked by [`AttackLoopStage`]. Returns
/// [`AwaitingInput`] if a window suspends, [`Done`] otherwise. Non-exhausting
/// (RR p.18) — honored by [`EnemyAttackSource::Retaliate`] (exhaust is
/// `EnemyPhase`-gated). Caller (`fire_retaliate_if_any`) has already confirmed the
/// enemy is ready + has the retaliate keyword.
///
/// [`AwaitingInput`]: crate::engine::EngineOutcome::AwaitingInput
/// [`Done`]: crate::engine::EngineOutcome::Done
pub(super) fn drive_retaliate(
    cx: &mut Cx,
    enemy: EnemyId,
    investigator: InvestigatorId,
) -> EngineOutcome {
    drive_attack_loop(cx, investigator, vec![enemy], EnemyAttackSource::Retaliate)
}

/// Resolve all of one investigator's engaged ready enemies' attacks
/// (Rules Reference p.25 step 3.3 inner body). Snapshot the attacker
/// list in [`EnemyId`] order (`BTreeMap` iteration is sorted), then
/// delegate to [`drive_attack_loop`] — which owns the per-attacker
/// steps (early-break-on-defeat, [`place_assignment`], exhaust) and the
/// soak-window suspend/resume contract (C5b #237).
///
/// **Attack order:** player-chosen (#143). With 2+ ready engaged enemies
/// the loop suspends on a `PickSingle` ([`AttackLoopStage::PickOrder`]) so
/// the attacked investigator picks which strikes next (RR p.25 step 3.3:
/// "resolve their attacks in the order of the attacked investigator's
/// choosing"), one at a time between attacks; a single attacker resolves
/// inline. The attacker set is snapshotted here in [`EnemyId`] order (the
/// option order) and frozen for the sequence — the pick reorders the stored
/// list, never re-scanning state.
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
/// 2. Call [`place_assignment`] (places damage + horror simultaneously per
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
///    ([`place_queue_exhaust`] queued one for a soaked asset), park the
///    remaining attackers on a [`Continuation::AttackLoop`] frame
///    and return [`EngineOutcome::AwaitingInput`] for the queued window.
///    [`resume_enemy_attack`] re-enters here when the window closes.
///
/// Returns [`EngineOutcome::Done`] when the list is exhausted with no
/// suspension.
/// Place an already-computed `assignment` for one attacker, queue a soak
/// reaction window per damaged survivor, and exhaust the attacker (enemy phase
/// only). The deterministic tail shared by the no-prompt synchronous path
/// ([`deal_head_and_maybe_park`]) and the interactive
/// [`resume_damage_assignment`] (#44/K5b). `assignment` is already built
/// (soak-first or player-chosen); this never prompts. `AoO` / `Retaliate`
/// sources never exhaust (RR p.7 / p.18) — the `attack_source` guards that.
fn place_queue_exhaust(
    cx: &mut Cx,
    investigator: InvestigatorId,
    enemy_id: EnemyId,
    attack_source: EnemyAttackSource,
    assignment: &Assignment,
) {
    // Simultaneous placement + defeat (RR p.7); returns the damaged surviving
    // soaker assets (C5b #237).
    let damaged_survivors = place_assignment(cx, investigator, assignment);

    // Queue a soak reaction window per surviving damaged asset, BEFORE the
    // exhaust step — preserving the historical order in which the window
    // opens before `EnemyExhausted`. Inert unless a soaker has an
    // `EnemyAttackDamagedSelf` reaction (Guard Dog 01021).
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

    // Exhaust only on the enemy phase. AoO / Retaliate never exhaust.
    // RR p.6: a cancelled attack is "still regarded as initiated"; RR p.25:
    // the enemy exhausts "upon completion of dealing the attack". The attack
    // was made; only its effect was prevented.
    if attack_source == EnemyAttackSource::EnemyPhase {
        let enemy = cx.state.enemies.get_mut(&enemy_id).unwrap_or_else(|| {
            unreachable!(
                "place_queue_exhaust: snapshotted enemy {enemy_id:?} is gone from \
                 state.enemies; this is a state-corruption invariant violation"
            )
        });
        enemy.exhausted = true;
        cx.events.push(Event::EnemyExhausted { enemy: enemy_id });
    }
}

/// Park the loop on the soak reaction window the just-dealt attack opened and
/// surface it (C5b #237). Called only when `open_windows()` is non-empty.
fn park_on_soak_window(
    cx: &mut Cx,
    investigator: InvestigatorId,
    remaining_attackers: Vec<EnemyId>,
    source: EnemyAttackSource,
) -> EngineOutcome {
    // Single-soak-window-per-attack invariant: the parked `AttackLoop` frame
    // holds one parked loop, so resume (which pops it) drains exactly one soak window
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
    park_attack_loop_beneath_window(
        cx,
        investigator,
        remaining_attackers,
        source,
        AttackLoopStage::AfterSoak,
    );
    super::reaction_windows::open_queued_reaction_window(cx)
}

/// Park the attack loop on an [`Continuation::AttackLoop`] frame *beneath* the
/// reaction window the just-dealt (or about-to-deal) attack queued.
/// `queue_reaction_window` has already pushed that window as the top frame, so
/// the loop frame is inserted just below it — yielding the
/// `[…, AttackLoop, Resolution(window)]` shape: the window stays the
/// player-facing top prompt, and [`resume_enemy_attack`] pops the loop once the
/// window closes (#411).
fn park_attack_loop_beneath_window(
    cx: &mut Cx,
    investigator: InvestigatorId,
    remaining_attackers: Vec<EnemyId>,
    source: EnemyAttackSource,
    stage: AttackLoopStage,
) {
    let below_top_window = cx.state.continuations.len() - 1;
    cx.state.continuations.insert(
        below_top_window,
        Continuation::AttackLoop {
            investigator,
            remaining_attackers,
            source,
            stage,
        },
    );
}

// ---------------------------------------------------------------------------
// Interactive soak distribution (#44/K5b): the defending player assigns each
// point of damage/horror across themselves and eligible soakers (RR p.7), one
// point at a time. Gated to prompt only when a soaker can take the point.
// ---------------------------------------------------------------------------

/// A target for one point of soak distribution (#44/K5b): the investigator
/// itself, or a controlled soaker asset instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DistributionTarget {
    Investigator,
    Asset(CardInstanceId),
}

/// The eligible targets for one point of `damage_point` (else horror), given the
/// soakers and the assignment-so-far: always the investigator, plus each soaker
/// with remaining capacity for that harm type (printed remaining − already
/// assigned in `assignment`).
fn eligible_targets(
    soakers: &[Soaker],
    assignment: &Assignment,
    damage_point: bool,
) -> Vec<DistributionTarget> {
    let mut targets = vec![DistributionTarget::Investigator];
    for s in soakers {
        let (cap, assigned) = if damage_point {
            (
                s.remaining_health,
                assignment
                    .asset_damage
                    .get(&s.instance)
                    .copied()
                    .unwrap_or(0),
            )
        } else {
            (
                s.remaining_sanity,
                assignment
                    .asset_horror
                    .get(&s.instance)
                    .copied()
                    .unwrap_or(0),
            )
        };
        if cap.saturating_sub(assigned) > 0 {
            targets.push(DistributionTarget::Asset(s.instance));
        }
    }
    targets
}

/// Advance the distribution deterministically as far as possible, keeping the
/// `remaining_*` counters and `assignment` in lockstep (decrementing a counter
/// as it auto-assigns that point). Returns `Some(())` when both counters drain
/// with no choice left, or `None` the moment a point has a soaker option (2+
/// eligible targets) — the caller then prompts. Damage points first, then
/// horror; a point with only the investigator eligible is auto-assigned to the
/// investigator (no soaker can take it), no prompt.
fn advance_distribution(
    soakers: &[Soaker],
    remaining_damage: &mut u8,
    remaining_horror: &mut u8,
    assignment: &mut Assignment,
) -> Option<()> {
    while *remaining_damage > 0 {
        if eligible_targets(soakers, assignment, true).len() > 1 {
            return None; // a damage point has a soaker option → prompt
        }
        assignment.investigator_damage = assignment
            .investigator_damage
            .saturating_add(*remaining_damage);
        *remaining_damage = 0;
    }
    while *remaining_horror > 0 {
        if eligible_targets(soakers, assignment, false).len() > 1 {
            return None; // a horror point has a soaker option → prompt
        }
        assignment.investigator_horror = assignment
            .investigator_horror
            .saturating_add(*remaining_horror);
        *remaining_horror = 0;
    }
    Some(())
}

/// Credit one assigned point of `damage_point` (else horror) to `target`.
fn credit_point(assignment: &mut Assignment, target: DistributionTarget, damage_point: bool) {
    match (target, damage_point) {
        (DistributionTarget::Investigator, true) => assignment.investigator_damage += 1,
        (DistributionTarget::Investigator, false) => assignment.investigator_horror += 1,
        (DistributionTarget::Asset(id), true) => {
            *assignment.asset_damage.entry(id).or_insert(0) += 1;
        }
        (DistributionTarget::Asset(id), false) => {
            *assignment.asset_horror.entry(id).or_insert(0) += 1;
        }
    }
}

/// Build the `PickSingle` over the eligible targets for the next point (the top
/// `DamageAssignment` frame must already be in place). Damage points precede horror.
fn prompt_current_point(cx: &mut Cx, investigator: InvestigatorId) -> EngineOutcome {
    let Some(Continuation::DamageAssignment {
        remaining_damage,
        remaining_horror,
        assignment,
        ..
    }) = cx.state.continuations.last()
    else {
        unreachable!("prompt_current_point: top frame is not DamageAssignment");
    };
    let (rd, rh) = (*remaining_damage, *remaining_horror);
    let assignment = assignment.clone();
    let soakers = build_soakers(cx.state, investigator);
    let damage_point = rd > 0;
    let targets = eligible_targets(&soakers, &assignment, damage_point);
    let kind = if damage_point { "damage" } else { "horror" };
    let prompt = format!(
        "Investigator {investigator:?}: assign 1 {kind} to which target? \
         ({rd} damage / {rh} horror left)"
    );
    EngineOutcome::AwaitingInput {
        request: InputRequest::choice(prompt, super::hunters::candidate_options(&targets)),
        resume_token: ResumeToken(0),
    }
}

/// Resume a soak distribution with the player's `PickSingle`: credit one point
/// to the chosen target, decrement that counter, then advance — re-prompt if a
/// point is still contested, else place once (simultaneous) and resume by
/// source. Invalid pick → reject, keep the frame (the `HunterMove` contract).
/// Runs **outside** [`drive_attack_loop`], so on completion the `EnemyAttack`
/// source re-drives the remaining attackers itself.
pub(super) fn resume_damage_assignment(
    cx: &mut Cx,
    response: &crate::action::InputResponse,
) -> EngineOutcome {
    use crate::state::DamageSource;
    let Some(Continuation::DamageAssignment {
        investigator,
        mut remaining_damage,
        mut remaining_horror,
        mut assignment,
        source,
    }) = cx.state.continuations.last().cloned()
    else {
        unreachable!("resume_damage_assignment: top frame is not DamageAssignment");
    };
    let crate::action::InputResponse::PickSingle(OptionId(i)) = response else {
        return EngineOutcome::Rejected {
            reason: format!(
                "ResolveInput: damage distribution expects PickSingle, got {response:?}"
            )
            .into(),
        };
    };
    let damage_point = remaining_damage > 0;
    let soakers = build_soakers(cx.state, investigator);
    let targets = eligible_targets(&soakers, &assignment, damage_point);
    let Some(target) = targets.get(*i as usize).copied() else {
        return EngineOutcome::Rejected {
            reason: format!(
                "ResolveInput: distribution option {i} out of range (0..{})",
                targets.len()
            )
            .into(),
        };
    };
    // Valid: pop the frame we validated against, credit the point, advance.
    cx.state.continuations.pop();
    credit_point(&mut assignment, target, damage_point);
    if damage_point {
        remaining_damage -= 1;
    } else {
        remaining_horror -= 1;
    }
    if advance_distribution(
        &soakers,
        &mut remaining_damage,
        &mut remaining_horror,
        &mut assignment,
    )
    .is_none()
    {
        // Still contested: re-park with the updated counters/assignment, re-prompt.
        cx.state.continuations.push(Continuation::DamageAssignment {
            investigator,
            remaining_damage,
            remaining_horror,
            assignment,
            source,
        });
        return prompt_current_point(cx, investigator);
    }
    // Drained → place once, then resume by source.
    match source {
        DamageSource::EnemyAttack {
            enemy,
            remaining_attackers,
            attack_source,
        } => {
            place_queue_exhaust(cx, investigator, enemy, attack_source, &assignment);
            if cx.state.open_windows().is_empty() {
                let out = drive_attack_loop(cx, investigator, remaining_attackers, attack_source);
                if matches!(out, EngineOutcome::AwaitingInput { .. }) {
                    return out;
                }
                finish_attack_loop(cx, attack_source, investigator)
            } else {
                park_attack_loop_beneath_window(
                    cx,
                    investigator,
                    remaining_attackers,
                    attack_source,
                    AttackLoopStage::AfterSoak,
                );
                super::reaction_windows::open_queued_reaction_window(cx)
            }
        }
        // K5b-2 (effect/treachery path): place this point's drained assignment,
        // then resume the parked effect walk — the parent `Seq` or `Leaf` frame
        // is now on top, so subsequent effects run (and may prompt again) with
        // no point lost (#422 / #44).
        DamageSource::Effect => {
            let _ = place_assignment(cx, investigator, &assignment);
            super::choice::resume_effect_walk(cx)
        }
    }
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

    // Build the assignment. Cancelled → empty (no harm dealt, still exhausts).
    // Else distribute soak-first as far as deterministic; if a point is contested
    // (a soaker can take it), suspend on the player's distribution prompt,
    // parking the rest of the loop on the `DamageAssignment` frame (#44/K5b).
    let mut assignment = Assignment::default();
    if !cancelled {
        let enemy = cx.state.enemies.get(&enemy_id).unwrap_or_else(|| {
            unreachable!(
                "deal_head_and_maybe_park: snapshotted enemy {enemy_id:?} is gone from \
                 state.enemies; state-corruption invariant violation"
            )
        });
        let (mut rd, mut rh) = (enemy.attack_damage, enemy.attack_horror);
        let soakers = build_soakers(cx.state, investigator);
        if advance_distribution(&soakers, &mut rd, &mut rh, &mut assignment).is_none() {
            cx.state.continuations.push(Continuation::DamageAssignment {
                investigator,
                remaining_damage: rd,
                remaining_horror: rh,
                assignment,
                source: crate::state::DamageSource::EnemyAttack {
                    enemy: enemy_id,
                    remaining_attackers: std::mem::take(attackers),
                    attack_source: source,
                },
            });
            return Some(prompt_current_point(cx, investigator));
        }
        // Not contested: `assignment` is the complete soak-first assignment.
    }

    // Synchronous (no prompt): place + queue windows + exhaust, then the existing
    // window-check — `attackers` left intact for the outer `drive_attack_loop`.
    place_queue_exhaust(cx, investigator, enemy_id, source, &assignment);
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

/// Resolve the head attacker: open its `BeforeEnemyAttack` cancel window (park
/// the loop as [`AttackLoopStage::BeforeAttack`] and suspend if a cancel
/// reaction is available, Axis D #336), otherwise deal it + maybe park on its
/// `AfterEnemyAttackDamagedAsset` soak window (C5b #237). `Some(outcome)` =
/// suspended (the loop is parked beneath the queued window); `None` = continue
/// to the next attacker. Caller guarantees `attackers` is non-empty. Shared by
/// [`drive_attack_loop`] and the order-pick resume (`resume_attack_order_pick`,
/// #143).
fn process_head_attacker(
    cx: &mut Cx,
    investigator: InvestigatorId,
    attackers: &mut Vec<EnemyId>,
    source: EnemyAttackSource,
) -> Option<EngineOutcome> {
    let enemy_id = *attackers
        .first()
        .expect("process_head_attacker called with an empty attacker list");

    // Before-attack cancel window (Axis D #336): reaction-only Before timing
    // point. Opens iff a co-located cancel reaction is available (Dodge in hand,
    // or an in-play reaction); `emit_event` only queues a window when the scan
    // finds a candidate. Suspend BEFORE dealing damage, keeping the head
    // attacker at the front of `attackers` so the `BeforeAttack` resume
    // processes it (deal-or-cancel).
    let _ = super::emit::emit_event(
        cx,
        &super::emit::TimingEvent::EnemyAttacks {
            enemy: enemy_id,
            investigator,
        },
    );
    if !cx.state.open_windows().is_empty() {
        park_attack_loop_beneath_window(
            cx,
            investigator,
            std::mem::take(attackers),
            source,
            AttackLoopStage::BeforeAttack,
        );
        return Some(super::reaction_windows::open_queued_reaction_window(cx));
    }

    // No cancel reaction available: deal this (un-cancelled) attacker,
    // suspending if it opens a soak window.
    deal_head_and_maybe_park(cx, investigator, attackers, source, false)
}

/// Park the loop on its order-pick `PickSingle` (#143): push the `AttackLoop`
/// frame as the **top** frame (no window above — it *is* the prompt) at
/// [`AttackLoopStage::PickOrder`], and return `AwaitingInput` offering the
/// remaining attackers (option `i` = `remaining_attackers[i]`, `EnemyId` order).
/// `resume_attack_order_pick` resolves the `PickSingle` back. Called only with
/// `attackers.len() >= 2`.
fn suspend_order_pick(
    cx: &mut Cx,
    investigator: InvestigatorId,
    attackers: Vec<EnemyId>,
    source: EnemyAttackSource,
) -> EngineOutcome {
    let prompt = format!(
        "Investigator {investigator:?} is engaged with {} enemies: pick which attacks \
         next (RR p.25 step 3.3)",
        attackers.len()
    );
    let options = super::hunters::candidate_options(&attackers);
    cx.state.continuations.push(Continuation::AttackLoop {
        investigator,
        remaining_attackers: attackers,
        source,
        stage: AttackLoopStage::PickOrder,
    });
    EngineOutcome::AwaitingInput {
        request: InputRequest::choice(prompt, options),
        resume_token: ResumeToken(0),
    }
}

fn drive_attack_loop(
    cx: &mut Cx,
    investigator: InvestigatorId,
    mut attackers: Vec<EnemyId>,
    source: EnemyAttackSource,
) -> EngineOutcome {
    while !attackers.is_empty() {
        // Early-break on defeat. See fn doc step 1.
        let active = cx
            .state
            .investigators
            .get(&investigator)
            .is_some_and(|inv| inv.status == Status::Active);
        if !active {
            break;
        }

        // Player-chosen attack order (#143, RR p.25 step 3.3): with 2+ ready
        // attackers remaining, suspend for the order pick before resolving the
        // head. Covers the enemy phase, AoO, and (vacuously, 1-element) retaliate
        // — all three route through here. Single-attacker lists skip this and
        // resolve inline, preserving prior behaviour.
        if attackers.len() >= 2 {
            return suspend_order_pick(cx, investigator, attackers, source);
        }

        if let Some(suspended) = process_head_attacker(cx, investigator, &mut attackers, source) {
            return suspended;
        }
    }
    EngineOutcome::Done
}

/// The source-keyed step that runs once an attack loop drains to
/// [`EngineOutcome::Done`]: enemy phase advances its per-investigator cursor and
/// opens the next window; an `AoO` returns control to the parked
/// `ActionResolution` frame (`Done`, the `drive` loop resumes it); a retaliate
/// re-enters the Fight's skill-test follow-up. Shared by [`resume_enemy_attack`]
/// (window-close drain) and `resume_attack_order_pick` (order-pick drain, #143).
fn finish_attack_loop(
    cx: &mut Cx,
    source: EnemyAttackSource,
    investigator: InvestigatorId,
) -> EngineOutcome {
    match source {
        EnemyAttackSource::EnemyPhase => {
            super::reaction_windows::after_enemy_phase_attacks(cx, investigator)
        }
        // AoO: nothing follows the drain. Retaliate (#379): the Fight's `SkillTest`
        // frame is now top (cursor at `PostOnResolution`); returning `Done` lets the
        // `drive` loop dispatch it to finish teardown (Slice C-plumbing — formerly a
        // direct `skill_test::advance` reach-down here).
        EnemyAttackSource::AttackOfOpportunity | EnemyAttackSource::Retaliate => {
            EngineOutcome::Done
        }
    }
}

/// Re-enter a suspended enemy-attack loop after the reaction window it parked
/// on closed. Mirror of the other pending-resume drivers (`resume_end_turn` /
/// spawn-engage). Pops the parked [`Continuation::AttackLoop`] frame (the top
/// frame now that the window above it has closed) and resumes per its
/// [`AttackLoopStage`]:
///
/// - [`AttackLoopStage::BeforeAttack`] (Axis D #336): the before-attack cancel
///   window closed. Read-and-clear `pending_cancellation`, then deal-or-skip
///   the head attacker (still at the front of `remaining_attackers`) via
///   [`deal_head_and_maybe_park`] and exhaust it. If *that* attack opens a soak
///   window, re-park as `AfterSoak`; otherwise drain the rest.
/// - [`AttackLoopStage::AfterSoak`] (C5b #237): the soak window closed; drain
///   the remaining attackers.
///
/// If the loop suspends again, that [`EngineOutcome::AwaitingInput`] is returned
/// as-is. On completion the source-keyed post-loop step runs via
/// [`finish_attack_loop`] (shared with the order-pick resume, #143).
///
/// Called from `run_reaction_continuation`'s
/// `EnemyAttackDamagedSelf` / `EnemyAttacks` arm on window close.
pub(super) fn resume_enemy_attack(cx: &mut Cx) -> EngineOutcome {
    let Some(Continuation::AttackLoop {
        investigator,
        mut remaining_attackers,
        source,
        stage,
    }) = cx.state.continuations.pop()
    else {
        unreachable!(
            "resume_enemy_attack: top frame is not an AttackLoop; the \
             soak / before-attack continuations only fire after \
             drive_attack_loop pushed one — state-corruption invariant \
             violation"
        )
    };

    if stage == AttackLoopStage::BeforeAttack {
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
    finish_attack_loop(cx, source, investigator)
}

/// Resume a loop suspended on its order-pick `PickSingle` (#143). The
/// `AttackLoop{stage: PickOrder}` frame is the top frame (no window above it),
/// so [`resolve_input`](super::resolve_input) routes here directly (not via
/// window-close). Validate the `PickSingle` against the stored
/// `remaining_attackers`; on an invalid pick, reject and **leave the frame** so
/// the client can retry (mirrors `resume_hunter_choice`). On a valid pick, move
/// the chosen enemy to the head, resolve it via [`process_head_attacker`] (which
/// may re-suspend on its own cancel/soak window), then drive the rest —
/// re-prompting if 2+ still remain — and run the source-keyed tail
/// ([`finish_attack_loop`]) on completion.
pub(super) fn resume_attack_order_pick(
    cx: &mut Cx,
    response: &crate::action::InputResponse,
) -> EngineOutcome {
    let Some(Continuation::AttackLoop {
        investigator,
        remaining_attackers,
        source,
        stage: AttackLoopStage::PickOrder,
    }) = cx.state.continuations.last().cloned()
    else {
        unreachable!(
            "resume_attack_order_pick: top frame is not an AttackLoop{{PickOrder}}; \
             resolve_input only routes here when it is — state-corruption invariant \
             violation"
        )
    };
    let crate::action::InputResponse::PickSingle(OptionId(i)) = response else {
        return EngineOutcome::Rejected {
            reason: format!(
                "ResolveInput: attack-order pick expects InputResponse::PickSingle, got {response:?}"
            )
            .into(),
        };
    };
    let i = *i as usize;
    if i >= remaining_attackers.len() {
        return EngineOutcome::Rejected {
            reason: format!(
                "ResolveInput: attack-order option {i} out of range (0..{})",
                remaining_attackers.len()
            )
            .into(),
        };
    }

    // Valid pick: pop the frame we validated against, then move the chosen enemy
    // to the head (preserving the others' relative order for the next prompt).
    cx.state.continuations.pop();
    let mut attackers = remaining_attackers;
    let chosen = attackers.remove(i);
    attackers.insert(0, chosen);

    if let Some(suspended) = process_head_attacker(cx, investigator, &mut attackers, source) {
        return suspended; // the chosen head opened its own cancel/soak window
    }
    let outcome = drive_attack_loop(cx, investigator, attackers, source);
    if matches!(outcome, EngineOutcome::AwaitingInput { .. }) {
        return outcome; // next order pick, or a later attacker's window
    }
    debug_assert!(
        matches!(outcome, EngineOutcome::Done),
        "drive_attack_loop returned unexpected {outcome:?}"
    );
    finish_attack_loop(cx, source, investigator)
}

#[cfg(test)]
mod combat_tests {
    use super::super::Cx;
    use crate::engine::{EngineOutcome, OptionId};
    use crate::event::Event;
    use crate::state::{AttackLoopStage, Continuation, EnemyAttackSource, EnemyId, InvestigatorId};
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
    fn soak_and_place_with_no_soakers_matches_old_behavior() {
        // Regression guard for the assign/place/window rewrite: an attack
        // of 2 damage / 1 horror against an investigator controlling no
        // soak-bearing assets must land entirely on the investigator, just
        // as the pre-rewrite direct apply_damage/horror_numeric path did.
        crate::test_support::install_test_registry();
        let id = InvestigatorId(1);
        let inv = test_investigator(1);
        // max_health()/max_sanity() now read from the registry (TEST_INV = 8/8).
        // 2 damage and 1 horror both land below capacity, so no defeat fires.
        // The old explicit max_health = 10 / max_sanity = 10 are vestigial.

        let mut state = GameStateBuilder::new().with_investigator(inv).build();
        let mut events = Vec::new();
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };

        let survivors = super::soak_and_place(&mut cx, id, 2, 1);
        assert!(survivors.is_empty(), "no soakers → no survivors");

        assert_eq!(state.investigators[&id].damage(), 2, "all damage on inv");
        assert_eq!(state.investigators[&id].horror(), 1, "all horror on inv");
        assert_event!(events, Event::DamageTaken { investigator, amount: 2 } if *investigator == id);
        assert_event!(events, Event::HorrorTaken { investigator, amount: 1 } if *investigator == id);
        assert!(
            state.open_windows().is_empty(),
            "no soak window without soakers"
        );
    }

    #[test]
    fn advance_distribution_drains_without_soakers_and_prompts_with_one() {
        // No soaker → fully deterministic: all damage to the investigator, drained.
        let mut asg = super::Assignment::default();
        let (mut d, mut h) = (2u8, 0u8);
        assert!(super::advance_distribution(&[], &mut d, &mut h, &mut asg).is_some());
        assert_eq!((d, h, asg.investigator_damage), (0, 0, 2));

        // A soaker with capacity → a damage point is contested → prompt (None),
        // and the counters still show the un-assigned points.
        let soaker = super::Soaker {
            instance: crate::state::CardInstanceId(1),
            remaining_health: 3,
            remaining_sanity: 0,
        };
        let mut asg2 = super::Assignment::default();
        let (mut d2, mut h2) = (2u8, 0u8);
        assert!(super::advance_distribution(&[soaker], &mut d2, &mut h2, &mut asg2).is_none());
        assert_eq!(
            (d2, h2),
            (2, 0),
            "nothing auto-assigned while a soaker can take the point"
        );
    }

    #[test]
    fn resume_damage_assignment_rejects_invalid_pick_and_keeps_frame() {
        use crate::state::{Continuation, DamageSource, EnemyAttackSource, EnemyId};
        let inv_id = InvestigatorId(1);
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .build();
        // Park a DamageAssignment frame directly (2 damage to assign).
        state.continuations.push(Continuation::DamageAssignment {
            investigator: inv_id,
            remaining_damage: 2,
            remaining_horror: 0,
            assignment: super::Assignment::default(),
            source: DamageSource::EnemyAttack {
                enemy: EnemyId(7),
                remaining_attackers: vec![],
                attack_source: EnemyAttackSource::EnemyPhase,
            },
        });
        let mut events = Vec::new();
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };

        // Wrong response variant → reject, frame untouched.
        let wrong = super::resume_damage_assignment(&mut cx, &crate::action::InputResponse::Skip);
        assert!(matches!(wrong, EngineOutcome::Rejected { .. }));

        // Out-of-range option (no soakers → only the investigator is eligible,
        // so any index ≥ 1 is invalid) → reject, frame untouched.
        let oob = super::resume_damage_assignment(
            &mut cx,
            &crate::action::InputResponse::PickSingle(crate::engine::OptionId(5)),
        );
        assert!(matches!(oob, EngineOutcome::Rejected { .. }));

        // The frame survives both rejections for the client to retry.
        assert!(
            matches!(
                state.continuations.last(),
                Some(Continuation::DamageAssignment { .. })
            ),
            "DamageAssignment frame retained after invalid picks"
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
        use crate::state::{CardCode, CardInPlay, CardInstanceId};
        use std::collections::BTreeMap;
        // Pre-construct an Assignment placing 1 damage + 1 horror on an
        // in-play asset and 1 damage on the investigator. Registry installed
        // so max_health() / max_sanity() can resolve; TEST_INV = 8/8 and the
        // investigator damage is 1 < 8, so no defeat fires.
        // Asset defeat-on-overflow needs the real `cards` registry and is
        // covered by the EU5 integration test.
        crate::test_support::install_test_registry();

        let id = InvestigatorId(1);
        let inst = CardInstanceId(7);
        let mut inv = test_investigator(1);
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
            state.investigators[&id].damage(),
            1,
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
    fn resume_enemy_attack_drains_remaining_attacker_and_advances_cursor() {
        use crate::state::{AttackLoopStage, Continuation, EnemyAttackSource, InvestigatorId};
        // EU5 deferral: firing Guard Dog's reaction end-to-end needs the real
        // `cards` registry (so `trigger_matches` finds the ability and a soak
        // window genuinely opens mid-loop) — that is the EU5 integration test.
        // This lib-level check exercises the resume half directly: park an
        // `AttackLoop` frame with one remaining attacker (as `drive_attack_loop`
        // would on suspend), then call `resume_enemy_attack` and assert it drains
        // the attacker (exhausting it) and advances the enemy-phase cursor past
        // the (sole) investigator to `None`. One attacker, not two: with 2+
        // remaining the drain would re-prompt for the player attack order (#143),
        // which the order-pick tests cover. The test registry (TEST_INV) is
        // installed so max_health()/max_sanity() resolve (#448 cp2a); the test
        // registry has no abilities so no soak reaction window fires.
        crate::test_support::install_test_registry();

        let inv_id = InvestigatorId(1);
        let second = EnemyId(2);

        let mut e2 = test_enemy(2, "Second Attacker");
        e2.engaged_with = Some(inv_id);

        // The real resume path runs AFTER `close_reaction_window` popped
        // the soak window the loop suspended on, so `open_windows` is empty
        // when `resume_enemy_attack` re-enters `drive_attack_loop`.
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_turn_order([inv_id])
            .with_enemy(e2)
            // EnemyPhase anchor (slice 1a): the attack-loop resume opens a
            // window whose close routes to anchor_on_child_pop.
            .with_phase_anchor(crate::state::Continuation::EnemyPhase {
                resume: crate::state::EnemyResume::BeforeInvestigatorAttacked,
                attacking: Some(inv_id),
            })
            .build();
        state.continuations.push(Continuation::AttackLoop {
            investigator: inv_id,
            remaining_attackers: vec![second], // one remaining: drains without a re-prompt
            source: EnemyAttackSource::EnemyPhase,
            stage: AttackLoopStage::AfterSoak,
        });

        let mut events = Vec::new();
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };
        let outcome = super::resume_enemy_attack(&mut cx);

        // The parked attacker resolved and exhausted; the parked frame is popped.
        // Test registry has no abilities → no new soak window, no re-suspend.
        assert!(
            !state
                .continuations
                .iter()
                .any(|c| matches!(c, Continuation::AttackLoop { .. })),
            "resume consumed the parked attack loop frame"
        );
        assert!(
            state.enemies[&second].exhausted,
            "second attacker exhausted"
        );
        assert_event!(events, Event::EnemyExhausted { enemy } if *enemy == second);
        // Loop finished → `after_enemy_phase_attacks` advanced the cursor
        // past the only investigator and opened the all-attacked window
        // (auto-skips inline with no registry), cascading the EnemyPhase anchor
        // off the stack — so no anchor is left still attacking anyone.
        assert!(
            !state.continuations.iter().any(|c| matches!(
                c,
                Continuation::EnemyPhase {
                    attacking: Some(_),
                    ..
                }
            )),
            "cursor advanced past the sole investigator (no anchor still attacking)"
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
        use crate::state::{AttackLoopStage, Continuation, EnemyAttackSource, InvestigatorId};

        let inv_id = InvestigatorId(1);
        let attacker = EnemyId(2);
        let mut enemy = test_enemy(2, "Attacker"); // attack_damage: 1
        enemy.engaged_with = Some(inv_id);

        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_turn_order([inv_id])
            .with_enemy(enemy)
            // EnemyPhase anchor (slice 1a): the attack-loop resume opens a
            // window whose close routes to anchor_on_child_pop.
            .with_phase_anchor(crate::state::Continuation::EnemyPhase {
                resume: crate::state::EnemyResume::BeforeInvestigatorAttacked,
                attacking: Some(inv_id),
            })
            .build();
        state.pending_cancellation = true; // a cancel reaction fired in the window
        state.continuations.push(Continuation::AttackLoop {
            investigator: inv_id,
            remaining_attackers: vec![attacker], // head still present (BeforeAttack)
            source: EnemyAttackSource::EnemyPhase,
            stage: AttackLoopStage::BeforeAttack,
        });

        let mut events = Vec::new();
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };
        let _ = super::resume_enemy_attack(&mut cx);

        assert_eq!(
            state.investigators[&inv_id].damage(),
            0,
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
        use crate::state::{AttackLoopStage, Continuation, EnemyAttackSource, InvestigatorId};
        crate::test_support::install_test_registry();

        let inv_id = InvestigatorId(1);
        let attacker = EnemyId(2);
        let mut enemy = test_enemy(2, "Attacker"); // attack_damage: 1
        enemy.engaged_with = Some(inv_id);

        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_turn_order([inv_id])
            .with_enemy(enemy)
            // EnemyPhase anchor (slice 1a): the attack-loop resume opens a
            // window whose close routes to anchor_on_child_pop.
            .with_phase_anchor(crate::state::Continuation::EnemyPhase {
                resume: crate::state::EnemyResume::BeforeInvestigatorAttacked,
                attacking: Some(inv_id),
            })
            .build();
        // pending_cancellation defaults to false (no reaction cancelled).
        state.continuations.push(Continuation::AttackLoop {
            investigator: inv_id,
            remaining_attackers: vec![attacker],
            source: EnemyAttackSource::EnemyPhase,
            stage: AttackLoopStage::BeforeAttack,
        });

        let mut events = Vec::new();
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };
        let _ = super::resume_enemy_attack(&mut cx);

        assert_eq!(
            state.investigators[&inv_id].damage(),
            1,
            "an un-cancelled attack deals its damage"
        );
        assert!(state.enemies[&attacker].exhausted, "attacker exhausted");
    }

    #[test]
    fn drive_retaliate_deals_damage_but_does_not_exhaust_the_attacker() {
        // RR p.18: a retaliate attack does not exhaust the attacker.
        crate::test_support::install_test_registry();
        let inv_id = InvestigatorId(1);
        let mut enemy = test_enemy(100, "Retaliator");
        enemy.retaliate = true;
        enemy.attack_damage = 1;
        enemy.attack_horror = 0;
        // Not engaged: a retaliate fires regardless of engagement, driven by enemy id.
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_enemy(enemy)
            .build();
        let mut events = Vec::new();
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };

        let outcome = super::drive_retaliate(&mut cx, EnemyId(100), inv_id);

        assert!(matches!(outcome, crate::engine::EngineOutcome::Done));
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
        crate::test_support::install_test_registry();
        let inv_id = InvestigatorId(1);
        let mut enemy = test_enemy(100, "Ghoul");
        enemy.engaged_with = Some(inv_id);
        enemy.attack_damage = 1;
        enemy.attack_horror = 0;
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_enemy(enemy)
            .build();
        let mut events = Vec::new();
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };

        let outcome = super::drive_aoo(&mut cx, inv_id);

        assert!(matches!(outcome, crate::engine::EngineOutcome::Done));
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

    #[test]
    fn drive_aoo_offers_order_pick_for_two_engaged_enemies() {
        // 2 engaged ready enemies provoking an AoO → the loop suspends on the
        // order pick, parking the AttackLoop frame as the top frame with the AoO
        // source + PickOrder stage (so it spans the whole AoO, #143). Picking the
        // higher-id enemy first proves the pick overrides EnemyId order; neither
        // AoO attacker exhausts (RR p.7). Registry installed so max_health() /
        // max_sanity() resolve (#448 cp2a); total AoO damage = 3 < 8 = TEST_INV.
        crate::test_support::install_test_registry();
        let inv_id = InvestigatorId(1);
        let mut e_a = test_enemy(5, "A"); // EnemyId(5), dmg 1
        e_a.engaged_with = Some(inv_id);
        e_a.attack_damage = 1;
        let mut e_b = test_enemy(6, "B"); // EnemyId(6), dmg 2
        e_b.engaged_with = Some(inv_id);
        e_b.attack_damage = 2;

        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_enemy(e_a)
            .with_enemy(e_b)
            .build();
        let mut events = Vec::new();

        let outcome = super::drive_aoo(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            inv_id,
        );
        assert!(
            matches!(outcome, EngineOutcome::AwaitingInput { .. }),
            "2 engaged ready enemies → AoO order pick (#143)"
        );
        // The parked frame carries the AoO source + PickOrder stage (frame spans
        // the whole AoO, not just a window suspension).
        assert!(matches!(
            state.continuations.last(),
            Some(Continuation::AttackLoop {
                source: EnemyAttackSource::AttackOfOpportunity,
                stage: AttackLoopStage::PickOrder,
                ..
            })
        ));

        // Pick EnemyId(6) (dmg 2) first → option 1 in EnemyId order [5, 6].
        let resumed = super::super::resolve_input(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            &crate::action::InputResponse::PickSingle(OptionId(1)),
        );
        assert!(matches!(resumed, EngineOutcome::Done), "AoO loop drained");
        let damages: Vec<u8> = events
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
        assert!(
            !state.enemies[&EnemyId(5)].exhausted && !state.enemies[&EnemyId(6)].exhausted,
            "AoO attackers never exhaust (RR p.7)"
        );
    }

    #[test]
    fn resume_attack_order_pick_rejects_invalid_input_and_keeps_frame() {
        // An out-of-range option id and a wrong InputResponse variant both reject,
        // leaving the PickOrder frame on the stack for the client to retry
        // (mirrors resume_hunter_choice).
        let inv_id = InvestigatorId(1);
        let mut e_a = test_enemy(5, "A");
        e_a.engaged_with = Some(inv_id);
        let mut e_b = test_enemy(6, "B");
        e_b.engaged_with = Some(inv_id);

        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_enemy(e_a)
            .with_enemy(e_b)
            .build();
        let mut events = Vec::new();
        let _ = super::drive_aoo(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            inv_id,
        );

        // Out-of-range option (only 0, 1 valid).
        let rejected = super::super::resolve_input(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            &crate::action::InputResponse::PickSingle(OptionId(9)),
        );
        assert!(matches!(rejected, EngineOutcome::Rejected { .. }));
        // Wrong variant.
        let rejected2 = super::super::resolve_input(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            &crate::action::InputResponse::Skip,
        );
        assert!(matches!(rejected2, EngineOutcome::Rejected { .. }));
        // The PickOrder frame survives both rejections for retry.
        assert!(matches!(
            state.continuations.last(),
            Some(Continuation::AttackLoop {
                stage: AttackLoopStage::PickOrder,
                ..
            })
        ));
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

    #[test]
    fn damage_application_accumulates_on_the_investigator_card() {
        crate::test_support::install_test_registry();
        let id = InvestigatorId(1);
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .build();
        let mut events = Vec::new();
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };
        let defeated = super::apply_damage_numeric(&mut cx, id, 3);
        assert_eq!(
            state.investigators[&id]
                .investigator_card
                .accumulated_damage,
            3,
            "damage must accumulate on the investigator_card, not the legacy field"
        );
        assert_eq!(
            state.investigators[&id].damage(),
            3,
            "damage() accessor must read from investigator_card"
        );
        assert!(!defeated, "3 < 8 health — investigator not defeated");
    }
}
