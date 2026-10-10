//! Combat helpers: enemy damage, investigator damage/horror, attacks.

use card_dsl::card_data::CardKind;
use card_dsl::dsl::{EntityScope, LocationSet};

use crate::action::InputResponse;
use crate::card_registry;
use crate::engine::dispatch::emit::TimingEvent;
use crate::engine::dispatch::{choice, elimination, emit, reaction_windows};
use crate::engine::outcome::{
    ChoiceOption, EngineOutcome, InputRequest, OptionId, OptionTarget, ResumeToken,
};
use crate::engine::{board, Cx};
use crate::event::Event;
use crate::state::{
    Assignment, AttackLoopFrame, AttackLoopStage, CardInPlay, CardInstanceId, DamageSource,
    DealDamageFrame, DealDamageStep, EliminationCause, EnemyAttackSource, EnemyId, GameState,
    InvestigatorId, Status,
};

/// The scope of enemies a Fight (basic action or designated **Fight** ability)
/// may target: any enemy *at your location*. Per RR you choose an enemy at your
/// location to attack and need not already be engaged, so this is co-located
/// (`At(Here)`), not engaged-only (#451). Its only reader is the Fight action's
/// [`candidates`](crate::engine::dispatch::actions::fight::candidates), which
/// is what the basic action's target validation, the turn menu, the activation
/// pre-cost gate (`designator::can_perform`) and the evaluator's target
/// grounding (`ground_fight_target_choice`) all read in turn.
pub(crate) fn fight_target_scope() -> EntityScope {
    EntityScope::At(LocationSet::Here)
}

/// Enemies matching an [`EntityScope`](card_dsl::dsl::EntityScope), in `BTreeMap`
/// (id) order so the `OptionId` index replays deterministically. Shared by the
/// evaluator's choice-grounding and the activation pre-cost target check.
pub(crate) fn enemies_in_scope(
    state: &GameState,
    controller: InvestigatorId,
    scope: EntityScope,
) -> Vec<EnemyId> {
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
        // The defeated enemy leaves play after `EnemyDefeated`, so its disposal
        // event follows it. `glossary/Defeat.md`: *"that enemy is defeated and
        // placed on the encounter discard pile (or on its owner's discard pile
        // if it is a weakness)"* — which is the exit's owner rule: an encounter
        // enemy is the encounter deck's, and a weakness enemy such as Silver
        // Twilight Acolyte 01102 is its bearer's (`glossary/Weakness.md`). Its
        // attachments are discarded with it, each by its own owner
        // (`glossary/Leaves_Play.md`).
        //
        // A Victory enemy goes to the victory display *instead*:
        // `glossary/Victory_Display_Victory_Points.md`, *"As a victory point
        // enemy is defeated, place the card in the victory display instead of
        // in the discard pile."*
        if let Some(victory) = defeated_victory.filter(|v| *v > 0) {
            board::place_in_victory_display(cx, enemy_id, victory);
        } else {
            board::discard_from_play(cx, enemy_id);
        }
        // Enemy defeated: dispatch the timing point through the unified
        // chokepoint (Axis-B T5a). `queue_event` queues the after-defeat
        // reaction window (Roland 01001) and the forced act objectives (act 3's
        // advance-on-Ghoul-Priest-defeat) as frames for the `drive` loop.
        //
        // This is a **tail-position emit** even though it doesn't look like one
        // (ADR 0003): the caller is a skill-test follow-up step, and
        // `apply_follow_up_step` pre-advances the `SkillTest` cursor *before* the
        // follow-up pushes anything, while `advance` yields whenever the
        // `SkillTest` is no longer the top frame. So the queued frames resolve
        // first and the test resumes at the already-advanced cursor afterwards —
        // no post-emit work runs here. The outcome is deliberately discarded
        // rather than returned: this helper's callers (`Effect::Deal`,
        // `deal_damage_to_enemy`) have their own frames, and the loop drives what
        // was queued. (The former `debug_assert!(Done)` asserted the wrong
        // invariant — a legitimate 2+ ordering run returns `AwaitingInput` — and
        // would have panicked in debug on the day a second act objective keyed
        // here; #569.)
        let _ = emit::queue_event(
            cx,
            &TimingEvent::EnemyDefeated {
                enemy: enemy_id,
                by,
                code: defeated_code,
            },
        );
    }
}

// `Assignment` (the computed damage/horror distribution) lives in
// `crate::state` alongside the other `Continuation` payload types, since the
// live one is owned by the `Continuation::DealDamage` frame that walks the two
// steps of dealing it (ADR 0009). Imported above.

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
/// remaining capacity, then the **investigator card** absorbs the
/// remainder. The investigator card is the always-eligible,
/// mandatory-remainder soaker (RR: "all damage/horror that cannot be
/// assigned to an asset must be assigned to the investigator") — it takes
/// whatever the assets cannot, *uncapped* (overflowing its own capacity is
/// exactly how the investigator is defeated). Its share rides
/// `Assignment::investigator_damage/horror` (not `asset_*`) so
/// [`place_assignment`] routes it onto `investigator_card.accumulated_*`
/// via the numeric helpers, keeping the [`Event::DamageTaken`] /
/// [`Event::HorrorTaken`] emission and the elimination wiring intact.
/// Damage and horror are assigned **independently** — an asset with
/// only health soaks damage, an asset with only sanity soaks horror.
///
/// Soak-first deterministic assignment, used by [`soak_and_place`] when no
/// point is contested (no soaker with capacity) and by the remaining
/// synchronous callers of `take_damage`/`take_horror`. The interactive
/// per-point distribution (#44/K5) lives in [`deal_enemy_attack`] /
/// [`resume_damage_distribution`] and `begin_deal_damage`; the two sites
/// still routed through the synchronous auto-soak wrapper are tracked in
/// `TODO(#427)` (Dynamite Blast's native loop) and `TODO(#429)` (the
/// deck-out horror penalty).
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
    state: &mut GameState,
    investigator: InvestigatorId,
    inst: CardInstanceId,
) -> Option<&mut CardInPlay> {
    state
        .investigators
        .get_mut(&investigator)?
        .cards_in_play
        .iter_mut()
        .find(|c| c.instance_id == inst)
}

/// Discard every controlled **asset** whose accumulated damage/horror has
/// reached its printed health/sanity (C5b #237).
///
/// Reads printed health/sanity from the card registry; an asset whose
/// metadata can't be resolved (no registry installed, or a non-asset
/// kind) is never defeated here. Each defeated asset leaves play through
/// [`board::discard_from_play`], which files it by its owner
/// (`glossary/Defeat.md`: *"A defeated asset is placed on its owner's discard
/// pile."*).
///
/// The **investigator card** is the other soaker subject to the same
/// `accumulated >= printed capacity` defeat rule (#448), but it is
/// deliberately *not* swept here: it lives in `investigator_card`, not
/// `cards_in_play`, and its overflow consequence is *elimination*, not a
/// discard. That defeat is resolved one step earlier — in
/// [`place_assignment`] step 2, *before* this asset sweep — and the order
/// is load-bearing: elimination (RR p.10 step 1) removes every controlled
/// card *from the game* (`removed_from_game`, no `CardDiscarded`), so an
/// asset that co-overflows with the investigator card is already gone
/// from `cards_in_play` by the time this sweep runs and never emits an
/// asset-defeat discard. Folding the investigator card into this sweep
/// would reverse that order and emit a spurious discard — do not.
fn defeat_overflowed_assets(cx: &mut Cx, investigator: InvestigatorId) {
    let Some(reg) = card_registry::current() else {
        return;
    };
    let Some(inv) = cx.state.investigators.get(&investigator) else {
        return;
    };
    // Collect the instances to defeat first (immutable scan), then mutate —
    // avoids holding a borrow across the discard mutation.
    let defeated: Vec<CardInstanceId> = inv
        .cards_in_play
        .iter()
        .filter_map(|card| {
            let meta = (reg.metadata_for)(&card.code)?;
            let CardKind::Asset { health, sanity, .. } = meta.kind else {
                return None;
            };
            let dmg_defeated = health.is_some_and(|h| card.accumulated_damage >= h);
            let hor_defeated = sanity.is_some_and(|s| card.accumulated_horror >= s);
            (dmg_defeated || hor_defeated).then_some(card.instance_id)
        })
        .collect();

    for inst in defeated {
        // RR p.7: a defeated asset goes to its owner's discard pile.
        board::discard_from_play(cx, inst).expect("soak defeat: the defeated asset is in play");
    }
}

/// Place a computed [`Assignment`] simultaneously, then defeat overflowed
/// assets (RR p.7; C5b #237).
///
/// Steps, in order:
/// 1. Accumulate the soaked damage/horror onto each asset's
///    `accumulated_*` fields.
/// 2. Place the investigator card's share (the mandatory remainder — RR:
///    "all damage/horror that cannot be assigned to an asset must be
///    assigned to the investigator") via the numeric helpers, which write
///    `investigator_card.accumulated_*` and emit [`Event::DamageTaken`] /
///    [`Event::HorrorTaken`], then apply investigator defeat if either
///    crossed (`accumulated >= max_health()/max_sanity()` — the same
///    `accumulated >= printed capacity` rule as an asset, but the
///    consequence is *elimination*, not discard). Both stats land before
///    the defeat check, per RR p.7. This runs *before* the asset sweep so
///    elimination's "remove controlled cards from the game" step (RR p.10)
///    pre-empts any co-overflowing asset's discard (see
///    [`defeat_overflowed_assets`]).
/// 3. Defeat overflowed assets (`accumulated >= printed stat` →
///    discard) — **skipped entirely if step 2 eliminated the investigator**,
///    whose controlled cards elimination removes from the game (RR p.10 step 1).
///
/// Step 3's skip is gated on [`Status`], not on `cards_in_play` having been
/// drained: since #638 an elimination with a step-0 weakness game-end ability
/// (Cover Up 01007) finishes on a continuation frame, so the zone is still
/// populated when this returns.
///
/// Announces nothing and returns nothing: by the time this runs, both conditions
/// around it have been emitted by the
/// [`DealDamage`](crate::state::Continuation::DealDamage) frame, and this *is*
/// `DamagePlaced`'s resolve step. Which soakers survive it is therefore no
/// question of this function's — an asset assigned lethal damage had its say one
/// cursor step ago, which is what its ruling requires
/// (`data/arkhamdb-faq/core/01021.md`: *"You can use Guard Dog's ability when you
/// assign lethal damage/horror to it."*). See ADR 0009.
pub(super) fn place_assignment(cx: &mut Cx, investigator: InvestigatorId, assignment: &Assignment) {
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
            EliminationCause::Damage
        } else {
            EliminationCause::Horror
        };
        elimination::apply_investigator_elimination(cx, investigator, cause);
    }

    // An eliminated investigator's controlled assets are elimination's business,
    // not the asset sweep's: RR p.10 step 1 removes them from the game, which
    // pre-empts a co-overflowing asset's discard, and a card on its way out of
    // play gets no soak reaction window. Gated on status rather than on
    // `cards_in_play` having been drained because elimination may still be in
    // progress here — see `Continuation::Elimination` (#638).
    let eliminated = cx
        .state
        .investigators
        .get(&investigator)
        .is_some_and(|inv| inv.status != Status::Active);
    if eliminated {
        return;
    }

    // 3. Defeat overflowed assets.
    defeat_overflowed_assets(cx, investigator);
}

/// Add `amount` to the investigator's `damage` and emit
/// [`Event::DamageTaken`]. Returns `true` iff the new total reaches
/// `max_health` (i.e. the investigator now qualifies for defeat under
/// [`EliminationCause::Damage`]).
///
/// Does NOT flip [`Status`] or emit [`Event::InvestigatorEliminated`] —
/// the caller composes the defeat step via [`apply_investigator_elimination`]
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
/// responsibility (see [`super::elimination::apply_investigator_elimination`]).
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
/// assets — **synchronously, in one call** (#44/K5a).
///
/// This is the shape the whole of dealing damage had before #727, and the two
/// callers left on it are `take_damage` / `take_horror`, whose own callers do
/// synchronous work afterwards (Dynamite Blast 01024's `for inv in
/// investigators` loop most of all). So it announces neither
/// [`DamageAssigned`](super::emit::TimingEvent::DamageAssigned) nor
/// [`DamagePlaced`](super::emit::TimingEvent::DamagePlaced): there is no cursor
/// here to sequence two emits on, and emitting one synchronously is what ADR
/// 0003 forbids.
///
/// TODO(#728): migrate both entry points onto [`begin_deal_damage`]. Until then
/// an ability keyed to either condition fires for an enemy attack and for
/// `Effect::Deal` but not for these.
/// Nothing in the Core or Dunwich corpus observes the gap.
///
/// `build_soakers` returns empty when no registry is installed or the
/// investigator controls no soak-bearing asset, so the assignment then drops
/// all damage/horror on the investigator — behavior-identical to the pre-soak
/// direct-apply path.
pub(super) fn soak_and_place(cx: &mut Cx, investigator: InvestigatorId, damage: u8, horror: u8) {
    let soakers = build_soakers(cx.state, investigator);
    let assignment = assign_attack(&soakers, damage, horror);
    place_assignment(cx, investigator, &assignment);
}

/// Begin one deal of `damage` + `horror` to `investigator`: push the
/// [`Continuation::DealDamage`] frame at its first step and return `Done`, which
/// hands the `drive` loop a frame it dispatches on sight.
///
/// The whole of the Rules Reference's two-step procedure runs on that frame —
/// distribution, then the `DamageAssigned` and `DamagePlaced` conditions, then
/// the resume — so **this is a tail-position call** (ADR 0003) exactly like an
/// emit: a caller with work to do afterwards puts it on a frame beneath, and the
/// two that do (the enemy attack's coordinator, and `Effect::Deal`'s parked
/// effect walk) already have one. See
/// `docs/adr/0009-damage-is-assigned-then-placed.md`.
pub(crate) fn begin_deal_damage(
    cx: &mut Cx,
    investigator: InvestigatorId,
    damage: u8,
    horror: u8,
    source: DamageSource,
) -> EngineOutcome {
    cx.state.continuations.push(DealDamageFrame {
        investigator,
        source,
        assignment: Assignment::default(),
        step: DealDamageStep::Distribute {
            remaining_damage: damage,
            remaining_horror: horror,
        },
    });
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
fn build_soakers(state: &GameState, investigator: InvestigatorId) -> Vec<Soaker> {
    let Some(reg) = card_registry::current() else {
        return Vec::new();
    };
    let Some(inv) = state.investigators.get(&investigator) else {
        return Vec::new();
    };
    inv.cards_in_play
        .iter()
        .filter_map(|card| {
            let meta = (reg.metadata_for)(&card.code)?;
            let CardKind::Asset { health, sanity, .. } = meta.kind else {
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
/// `EnemyPhase`-gated). Caller (`fight::fire_retaliate_if_any`) has already confirmed the
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

/// Exhaust the attacker that has just completed its attack sequence — the enemy
/// phase's own step, run by the parked [`Continuation::AttackLoop`] once the
/// attack's `when → resolve → at → after` walk has popped.
///
/// Two rules put it here rather than inside the attack's resolve step.
/// `data/rules-reference/rules/Appendix_II_Timing_and_Gameplay.md` on step 3.3,
/// verbatim:
///
/// > Upon completion of dealing the attack (and all abilities triggered by the
/// > attack), exhaust the enemy.
///
/// — so it follows the `after` cell, not the damage. And `data/arkhamdb-faq/core/01023.md`
/// on Dodge, verbatim:
///
/// > If an attack was cancelled during the Enemy phase, the attacking enemy
/// > still exhausts.
///
/// — so it must survive a `when`-cell cancel, which abandons the rest of the
/// sequence (#714) and would take an exhaust inside it along.
///
/// `AoO` / `Retaliate` attackers never exhaust (RR p.7 / p.18), and an attacker
/// that is no longer on the board (defeated by Guard Dog 01021's retaliate
/// during its own attack) has nothing to exhaust.
fn exhaust_after_attack(cx: &mut Cx, enemy_id: EnemyId, source: EnemyAttackSource) {
    if source != EnemyAttackSource::EnemyPhase {
        return;
    }
    let Some(enemy) = cx.state.enemies.get_mut(&enemy_id) else {
        return;
    };
    enemy.exhausted = true;
    cx.events.push(Event::EnemyExhausted { enemy: enemy_id });
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

/// Build the per-point soak options, anchoring each to its board home so a host
/// renders it on the right card (S5, #540): a soaker asset to its card instance,
/// the defending investigator to their own investigator card (`me`, #950).
/// Each label is the anchored card's name (#989).
fn soak_options(
    state: &GameState,
    targets: &[DistributionTarget],
    me: &OptionTarget,
) -> Vec<ChoiceOption> {
    choice::candidate_options(state, targets, |t| match t {
        DistributionTarget::Asset(instance) => OptionTarget::CardInstance(*instance),
        DistributionTarget::Investigator => me.clone(),
    })
}

/// Build the `PickSingle` over the eligible targets for the next point (the top
/// `DealDamage` frame must already be at [`DealDamageStep::Distribute`]). Damage
/// points precede horror.
fn prompt_current_point(cx: &mut Cx, investigator: InvestigatorId) -> EngineOutcome {
    let DealDamageFrame {
        assignment, step, ..
    } = cx
        .state
        .continuations
        .top_expect::<DealDamageFrame>()
        .clone();
    let DealDamageStep::Distribute {
        remaining_damage: rd,
        remaining_horror: rh,
    } = step
    else {
        unreachable!("prompt_current_point: the DealDamage frame is not at Distribute");
    };
    let soakers = build_soakers(cx.state, investigator);
    let damage_point = rd > 0;
    let targets = eligible_targets(&soakers, &assignment, damage_point);
    let kind = if damage_point { "damage" } else { "horror" };
    let prompt = format!(
        "Investigator {investigator:?}: assign 1 {kind} to which target? \
         ({rd} damage / {rh} horror left)"
    );
    EngineOutcome::AwaitingInput {
        request: InputRequest::pick_single(
            prompt,
            soak_options(
                cx.state,
                &targets,
                &cx.state.investigators[&investigator].card_anchor(),
            ),
        ),
        resume_token: ResumeToken(0),
    }
}

/// Resume a soak distribution with the player's `PickSingle`: credit one point
/// to the chosen target, decrement that counter, then re-drive the frame — which
/// re-prompts if a point is still contested, or advances the cursor to
/// `Announce` once both counters drain. Invalid pick → reject, keep the frame
/// (the `HunterMove` contract).
///
/// Only Rules Reference step 1 happens here. The two conditions and the
/// placement are the frame's other steps, which the `drive` loop reaches when
/// this returns `Done` (ADR 0009).
pub(super) fn resume_damage_distribution(cx: &mut Cx, response: &InputResponse) -> EngineOutcome {
    let DealDamageFrame {
        investigator,
        mut assignment,
        step,
        ..
    } = cx
        .state
        .continuations
        .top_expect::<DealDamageFrame>()
        .clone();
    let DealDamageStep::Distribute {
        mut remaining_damage,
        mut remaining_horror,
    } = step
    else {
        unreachable!("resume_damage_distribution: the DealDamage frame is not at Distribute");
    };
    let InputResponse::PickSingle(OptionId(i)) = response else {
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
    // Valid: credit the point on the frame we validated against, then let the
    // frame's own driver decide whether to re-prompt or move on.
    credit_point(&mut assignment, target, damage_point);
    if damage_point {
        remaining_damage -= 1;
    } else {
        remaining_horror -= 1;
    }
    set_deal_damage(
        cx,
        assignment,
        DealDamageStep::Distribute {
            remaining_damage,
            remaining_horror,
        },
    );
    drive_deal_damage(cx)
}

/// Overwrite the top [`Continuation::DealDamage`] frame's live assignment and
/// cursor. The frame owns the assignment — each emit only snapshots it — so this
/// is the single writer (ADR 0009).
fn set_deal_damage(cx: &mut Cx, new_assignment: Assignment, new_step: DealDamageStep) {
    let frame = cx.state.continuations.top_mut::<DealDamageFrame>();
    frame.assignment = new_assignment;
    frame.step = new_step;
}

/// Dispatch the top [`Continuation::DealDamage`] frame one step — the `drive`
/// loop's arm for it, and the resume tail of [`resume_damage_distribution`]. All
/// four bindings come off the frame, which is the only thing that knows them.
///
/// The cursor is the Rules Reference's two steps plus the bookends that get it
/// there and hand back (`glossary/Dealing_Damage_Horror.md`; ADR 0009):
///
/// - `Distribute` — step 1's determination, interactive per point (#44/K5b). It
///   drains immediately when uncontested, so the cursor always starts at the
///   top; while a point is contested this frame *is* the prompt.
/// - `Announce` — emit `DamageAssigned`, a bare milestone whose `when` cell is
///   the window the rules put *between* the two steps.
/// - `Place` — emit `DamagePlaced`, whose resolve step places the assignment
///   simultaneously. It re-reads the frame, so what lands is whatever
///   `DamageAssigned`'s cells made of it.
/// - `Finish` — pop and resume by source.
///
/// Both emits **advance the cursor before emitting**, so each is in tail
/// position (ADR 0003): the coordinator they push lands above this frame and
/// runs its whole sequence before the loop re-exposes this one.
pub(crate) fn drive_deal_damage(cx: &mut Cx) -> EngineOutcome {
    let DealDamageFrame {
        investigator,
        source,
        assignment,
        step,
    } = cx
        .state
        .continuations
        .top_expect::<DealDamageFrame>()
        .clone();
    match step {
        DealDamageStep::Distribute {
            mut remaining_damage,
            mut remaining_horror,
        } => {
            let mut assignment = assignment;
            let soakers = build_soakers(cx.state, investigator);
            if advance_distribution(
                &soakers,
                &mut remaining_damage,
                &mut remaining_horror,
                &mut assignment,
            )
            .is_none()
            {
                // Still contested: re-park with the updated counters/assignment
                // and put the next point to the player.
                set_deal_damage(
                    cx,
                    assignment,
                    DealDamageStep::Distribute {
                        remaining_damage,
                        remaining_horror,
                    },
                );
                return prompt_current_point(cx, investigator);
            }
            set_deal_damage(cx, assignment, DealDamageStep::Announce);
            EngineOutcome::Done
        }
        DealDamageStep::Announce => {
            set_deal_damage(cx, assignment.clone(), DealDamageStep::Place);
            emit::queue_event(
                cx,
                &TimingEvent::DamageAssigned {
                    source,
                    investigator,
                    assignment,
                },
            )
        }
        DealDamageStep::Place => {
            set_deal_damage(cx, assignment.clone(), DealDamageStep::Finish);
            emit::queue_event(
                cx,
                &TimingEvent::DamagePlaced {
                    source,
                    investigator,
                    assignment,
                },
            )
        }
        DealDamageStep::Finish => {
            cx.state.continuations.pop_expect::<DealDamageFrame>();
            match source {
                // The attack's own sequence continues on the frames beneath: its
                // `at` and `after` cells on the coordinator, then the parked
                // `AttackLoop`'s exhaust and the next attacker (#704).
                DamageSource::EnemyAttack { .. } => EngineOutcome::Done,
                // K5b-2: resume the parked effect walk, so subsequent effects
                // run (and may prompt again) with no point lost (#422/#44).
                DamageSource::Effect => choice::resume_effect_walk(cx),
            }
        }
    }
}

/// Deal one enemy attack: the resolve step of the `EnemyAttacks` triggering
/// condition (#704), reached from [`super::emit`] once the `when` cell has run
/// without preventing it.
///
/// The attack's impact is the damage and horror it deals, so this is one call to
/// [`begin_deal_damage`] with the attacker's printed values — and dealing them
/// is itself the two-step procedure of ADR 0009, walked on the
/// [`Continuation::DealDamage`] frame this pushes above the coordinator. So the
/// attack's `at` and `after` cells run after *both* halves of the damage, and
/// Guard Dog 01021's retaliate resolves in between, in `DamageAssigned`'s `when`
/// cell — which is the nesting `glossary/Nested_Sequences.md` works its example
/// on.
///
/// A cancelled attack never reaches here at all: the coordinator abandons the
/// sequence at its resolve step (#714), which is also why the exhaust is not
/// here (see [`exhaust_after_attack`]).
pub(super) fn deal_enemy_attack(
    cx: &mut Cx,
    investigator: InvestigatorId,
    enemy_id: EnemyId,
) -> EngineOutcome {
    let enemy = cx.state.enemies.get(&enemy_id).unwrap_or_else(|| {
        unreachable!(
            "deal_enemy_attack: attacking enemy {enemy_id:?} is gone from \
             state.enemies; state-corruption invariant violation"
        )
    });
    let (damage, horror) = (enemy.attack_damage, enemy.attack_horror);
    begin_deal_damage(
        cx,
        investigator,
        damage,
        horror,
        DamageSource::EnemyAttack { enemy: enemy_id },
    )
}

/// Begin the head attacker's attack: park the loop on its
/// [`Continuation::AttackLoop`] frame — the head **left at the front** of
/// `attackers` — and emit the `EnemyAttacks` triggering condition in tail
/// position (ADR 0003).
///
/// This is the whole of the #704 migration's drive-shape change. The loop used
/// to emit and then read `open_windows()` synchronously to decide whether to
/// park; now it parks unconditionally and lets the coordinator above it walk the
/// attack's `when → resolve → at → after` sequence, suspending wherever that
/// sequence needs to. [`drive_parked_attack_loop`] picks the loop back up when
/// the coordinator pops.
fn begin_head_attack(
    cx: &mut Cx,
    investigator: InvestigatorId,
    attackers: Vec<EnemyId>,
    source: EnemyAttackSource,
) -> EngineOutcome {
    let enemy = *attackers
        .first()
        .expect("begin_head_attack called with an empty attacker list");
    cx.state.continuations.push(AttackLoopFrame {
        investigator,
        remaining_attackers: attackers,
        source,
        stage: AttackLoopStage::Attacking,
    });
    emit::queue_event(
        cx,
        &TimingEvent::EnemyAttacks {
            enemy,
            investigator,
        },
    )
}

/// Dispatch a [`Continuation::AttackLoop`] at [`AttackLoopStage::Attacking`] the
/// `drive` loop has re-exposed: the head attacker's sequence has fully run (or
/// was cancelled in its `when` cell), so take the head off, exhaust it, and
/// continue with the rest.
pub(super) fn drive_parked_attack_loop(cx: &mut Cx) -> EngineOutcome {
    let AttackLoopFrame {
        investigator,
        mut remaining_attackers,
        source,
        stage,
    } = cx.state.continuations.pop_expect::<AttackLoopFrame>();
    assert_eq!(
        stage,
        AttackLoopStage::Attacking,
        "drive_parked_attack_loop: the `drive` loop routes here only at Attacking"
    );
    let attacked = remaining_attackers.remove(0);
    exhaust_after_attack(cx, attacked, source);
    drive_attack_loop(cx, investigator, remaining_attackers, source)
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
    let options = choice::candidate_options(cx.state, &attackers, |e| OptionTarget::Enemy(*e));
    cx.state.continuations.push(AttackLoopFrame {
        investigator,
        remaining_attackers: attackers,
        source,
        stage: AttackLoopStage::PickOrder,
    });
    EngineOutcome::AwaitingInput {
        request: InputRequest::pick_single(prompt, options),
        resume_token: ResumeToken(0),
    }
}

/// One step of the attack loop: pick what happens to the remaining `attackers`,
/// in the order they will resolve. Entered by the three drivers
/// ([`resolve_attacks_for_investigator`], [`drive_aoo`], [`drive_retaliate`])
/// and re-entered by [`drive_parked_attack_loop`] after each attack completes.
///
/// - **The attacked investigator is no longer [`Status::Active`]** (defeated by
///   an earlier attack in the same loop) — the remaining attackers do not attack
///   and do not exhaust, per Rules Reference p.10 Elimination step 3 (*"All
///   enemies engaged with that player are placed at the location … unengaged"*)
///   and p.25 (*"Each ready, engaged enemy makes an attack"* — a disengaged enemy
///   is not "engaged"). `apply_investigator_elimination` (#144) also clears
///   `engaged_with`, so this is the simpler local form of a condition that holds
///   anyway.
/// - **None left** — run the source-keyed tail ([`finish_attack_loop`]).
/// - **One left** — begin its attack ([`begin_head_attack`]).
/// - **Two or more** — suspend on the attacked investigator's order pick (#143,
///   RR p.25 step 3.3: *"resolve their attacks in the order of the attacked
///   investigator's choosing"*), one pick at a time between attacks.
///
/// Since #704 this never resolves an attack inline: `begin_head_attack` parks
/// the loop and hands the attack to the timing coordinator, so a single call
/// makes exactly one decision and returns.
fn drive_attack_loop(
    cx: &mut Cx,
    investigator: InvestigatorId,
    attackers: Vec<EnemyId>,
    source: EnemyAttackSource,
) -> EngineOutcome {
    let active = cx
        .state
        .investigators
        .get(&investigator)
        .is_some_and(|inv| inv.status == Status::Active);
    if !active || attackers.is_empty() {
        return finish_attack_loop(cx, source, investigator);
    }
    if attackers.len() == 1 {
        return begin_head_attack(cx, investigator, attackers, source);
    }
    suspend_order_pick(cx, investigator, attackers, source)
}

/// The source-keyed step that runs once an attack loop drains to
/// [`EngineOutcome::Done`]: enemy phase advances its per-investigator cursor and
/// opens the next window; an `AoO` returns control to the parked
/// `ActionResolution` frame (`Done`, the `drive` loop resumes it); a retaliate
/// re-enters the Fight's skill-test follow-up. Run by [`drive_attack_loop`] the
/// moment the attacker list is empty (or the attacked investigator is no longer
/// active), whichever driver got it there.
fn finish_attack_loop(
    cx: &mut Cx,
    source: EnemyAttackSource,
    investigator: InvestigatorId,
) -> EngineOutcome {
    match source {
        EnemyAttackSource::EnemyPhase => {
            reaction_windows::after_enemy_phase_attacks(cx, investigator)
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

/// Resume a loop suspended on its order-pick `PickSingle` (#143). The
/// `AttackLoop{stage: PickOrder}` frame is the top frame (no window above it),
/// so [`resolve_input`](super::resolve_input) routes here directly (not via
/// window-close). Validate the `PickSingle` against the stored
/// `remaining_attackers`; on an invalid pick, reject and **leave the frame** so
/// the client can retry (mirrors `resume_hunter_choice`). On a valid pick, move
/// the chosen enemy to the head and begin its attack ([`begin_head_attack`]) —
/// the parked loop then drives the rest, re-prompting if 2+ still remain.
pub(super) fn resume_attack_order_pick(cx: &mut Cx, response: &InputResponse) -> EngineOutcome {
    let AttackLoopFrame {
        investigator,
        remaining_attackers,
        source,
        stage,
    } = cx
        .state
        .continuations
        .top_expect::<AttackLoopFrame>()
        .clone();
    assert_eq!(
        stage,
        AttackLoopStage::PickOrder,
        "resume_attack_order_pick: resolve_input routes here only at PickOrder"
    );
    let InputResponse::PickSingle(OptionId(i)) = response else {
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
    cx.state.continuations.pop_expect::<AttackLoopFrame>();
    let mut attackers = remaining_attackers;
    let chosen = attackers.remove(i);
    attackers.insert(0, chosen);

    // Begin the chosen head directly rather than re-entering `drive_attack_loop`
    // — with 2+ still in the list that would re-prompt the same pick forever.
    // The `AttackLoop` frame it parks carries the rest, so the next pick is
    // offered when this attack's sequence pops.
    begin_head_attack(cx, investigator, attackers, source)
}

#[cfg(test)]
mod tests;
