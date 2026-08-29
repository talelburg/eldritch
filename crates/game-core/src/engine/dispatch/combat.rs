//! Combat helpers: enemy damage, investigator damage/horror, attacks.

use crate::engine::outcome::{InputRequest, OptionId, ResumeToken};
use crate::engine::EngineOutcome;
use crate::event::Event;
use crate::state::{
    Assignment, AttackLoopStage, CardCode, CardInstanceId, Continuation, DealDamageStep,
    EliminationCause, EnemyAttackSource, EnemyId, GameState, InvestigatorId, Status,
};

use super::Cx;

/// The scope of enemies a Fight (basic action or designated **Fight** ability)
/// may target: any enemy *at your location*. Per RR you choose an enemy at your
/// location to attack and need not already be engaged, so this is co-located
/// (`At(Here)`), not engaged-only (#451). Single source of truth, read through
/// `designator::fight_candidates` by the basic action's target validation, the
/// activation pre-cost gate (`can_perform`) and the evaluator's target
/// grounding (`ground_fight_target_choice`) alike, so the three can't drift.
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
        // victory locations) because the enemy is removed above. The victory
        // display is *instead of* a discard pile, so the two arms are exclusive
        // (`glossary/Victory_Display_Victory_Points.md`, see
        // [`place_defeated_enemy_card`]).
        if let Some(victory) = defeated_victory.filter(|v| *v > 0) {
            cx.state.victory_display.push(defeated_code.clone());
            cx.events.push(Event::EnteredVictoryDisplay {
                code: defeated_code.clone(),
                victory,
            });
        } else {
            place_defeated_enemy_card(cx, defeated_code.clone());
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
        let _ = super::emit::queue_event(
            cx,
            &super::emit::TimingEvent::EnemyDefeated {
                enemy: enemy_id,
                by,
                code: defeated_code,
            },
        );
    }
}

/// Place a defeated enemy's card in the pile the Rules Reference names for it.
///
/// `data/rules-reference/rules/glossary/Defeat.md`:
///
/// > If an enemy has as much or more damage on it as it has health, that enemy
/// > is defeated and placed on the encounter discard pile (or on its owner's
/// > discard pile if it is a weakness).
///
/// The distinction is load-bearing in both directions. The encounter discard is
/// not out of the game — `glossary/Encounter_Deck.md`: "If the encounter deck is
/// empty, shuffle the encounter discard pile back into the encounter deck." — so
/// a defeated Ghoul Minion 01160 comes back around later in the scenario. And a
/// weakness rejoins its owner's deck for the campaign — `glossary/Weakness.md`:
/// "If a weakness is added to a player's deck, hand, or threat area during the
/// play of a scenario, that weakness remains a part of that investigator's deck
/// for the rest of the campaign." (#632.)
///
/// **Victory enemies never reach here**: the caller takes the victory-display
/// arm instead, per `glossary/Victory_Display_Victory_Points.md` — "As a victory
/// point enemy is defeated, place the card in the victory display instead of in
/// the discard pile."
///
/// Both placements are **eventless**, matching every other encounter-card
/// disposal path (the treachery `Discard` disposition, the unspawnable
/// `Specific` discard): the pile is observable in state, and `EnemyDefeated`
/// already marks the moment.
fn place_defeated_enemy_card(cx: &mut Cx, code: CardCode) {
    if !super::cards::is_weakness_code(&code) {
        cx.state.encounter_discard.push(code);
        return;
    }
    let Some(owner) = sole_active_investigator(cx.state) else {
        // Multiplayer: the engine cannot say whose deck this weakness came
        // from, and putting a player card in the encounter discard would feed
        // it back into the encounter deck on the next reshuffle — a worse
        // divergence than dropping it. Guessing (the engaged investigator) is
        // not available either: "Prey – Bearer only" ingests as `Prey::Default`
        // (#654), so engagement may point at the wrong seat. The card therefore
        // stays unplaced, loudly. `Rejected` is not an option here: the apply
        // boundary rolls a rejection back, which would undo the whole Fight.
        //
        // Unreachable in shipped play today — multiplayer is architecture-only
        // (`docs/phases/phase-8-multiplayer-and-auth.md`) — so the assert is a
        // tripwire for whoever builds it rather than a live panic risk.
        debug_assert!(
            false,
            "TODO(#654): defeated weakness enemy {code} has no determinable owner \
             (2+ active investigators, no bearer model); card left unplaced \
             (lands with #654)"
        );
        return;
    };
    cx.state
        .investigators
        .get_mut(&owner)
        .unwrap_or_else(|| {
            unreachable!("sole_active_investigator returned {owner:?}, which is not in the map")
        })
        .discard
        .push(code);
}

/// The investigator who owns any weakness in play, insofar as the engine can
/// determine ownership today.
///
/// `glossary/Weakness.md`: "The bearer of a weakness is the investigator who
/// started the game with the weakness in his or her deck or play area." Nothing
/// in the engine records that yet (#654), so the only answer it can give without
/// guessing is the solo one: with exactly one active investigator, every deck in
/// the game is theirs. Returns `None` for anything else — including a solo game
/// whose investigator has been eliminated, whose discard pile RR elimination
/// step 1 has already removed from the game (see
/// [`elimination`](super::elimination)).
fn sole_active_investigator(state: &GameState) -> Option<InvestigatorId> {
    let mut active = state
        .investigators
        .iter()
        .filter(|(_, inv)| inv.status == Status::Active);
    let (&id, _) = active.next()?;
    active.next().is_none().then_some(id)
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

/// Discard every controlled **asset** whose accumulated damage/horror has
/// reached its printed health/sanity (C5b #237).
///
/// Reads printed health/sanity from the card registry; an asset whose
/// metadata can't be resolved (no registry installed, or a non-asset
/// kind) is never defeated here. For each defeated asset: remove it from
/// `cards_in_play` and emit [`Event::CardDiscarded`] with
/// `from: Zone::InPlay` (matching the discard event shape used elsewhere
/// — see `dispatch/cards.rs`).
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
    let Some(reg) = crate::card_registry::current() else {
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
            let crate::card_data::CardKind::Asset { health, sanity, .. } = meta.kind else {
                return None;
            };
            let dmg_defeated = health.is_some_and(|h| card.accumulated_damage >= h);
            let hor_defeated = sanity.is_some_and(|s| card.accumulated_horror >= s);
            (dmg_defeated || hor_defeated).then_some(card.instance_id)
        })
        .collect();

    for inst in defeated {
        // RR p.7: a defeated asset goes to its owner's discard pile.
        super::cards::discard_card_from_play(cx, investigator, inst);
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
        super::elimination::apply_investigator_elimination(cx, investigator, cause);
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
    source: crate::state::DamageSource,
) -> EngineOutcome {
    cx.state.continuations.push(Continuation::DealDamage {
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
/// the investigator to `Global` (no card). Labels match the former
/// `hunters::candidate_options` debug repr, so the flat bar is byte-unchanged.
fn soak_options(targets: &[DistributionTarget]) -> Vec<crate::engine::ChoiceOption> {
    use crate::engine::{ChoiceOption, OptionId, OptionTarget};
    targets
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let id = OptionId(u32::try_from(i).expect("soak target count fits u32"));
            let opt = ChoiceOption::new(id, format!("{t:?}"));
            match t {
                DistributionTarget::Asset(instance) => {
                    opt.at(OptionTarget::CardInstance(*instance))
                }
                // The investigator themself is not yet a board anchor; the option
                // lands in the prompt banner.
                DistributionTarget::Investigator => opt,
            }
        })
        .collect()
}

/// Build the `PickSingle` over the eligible targets for the next point (the top
/// `DealDamage` frame must already be at [`DealDamageStep::Distribute`]). Damage
/// points precede horror.
fn prompt_current_point(cx: &mut Cx, investigator: InvestigatorId) -> EngineOutcome {
    let Some(Continuation::DealDamage {
        assignment,
        step:
            DealDamageStep::Distribute {
                remaining_damage,
                remaining_horror,
            },
        ..
    }) = cx.state.continuations.last()
    else {
        unreachable!("prompt_current_point: top frame is not DealDamage{{Distribute}}");
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
        request: InputRequest::pick_single(prompt, soak_options(&targets)),
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
pub(super) fn resume_damage_distribution(
    cx: &mut Cx,
    response: &crate::action::InputResponse,
) -> EngineOutcome {
    let Some(Continuation::DealDamage {
        investigator,
        mut assignment,
        step:
            DealDamageStep::Distribute {
                mut remaining_damage,
                mut remaining_horror,
            },
        ..
    }) = cx.state.continuations.last().cloned()
    else {
        unreachable!("resume_damage_distribution: top frame is not DealDamage{{Distribute}}");
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
    match cx.state.continuations.last_mut() {
        Some(Continuation::DealDamage {
            assignment, step, ..
        }) => {
            *assignment = new_assignment;
            *step = new_step;
        }
        other => unreachable!("set_deal_damage: expected a DealDamage on top, got {other:?}"),
    }
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
    let Some(Continuation::DealDamage {
        investigator,
        source,
        assignment,
        step,
    }) = cx.state.continuations.last().cloned()
    else {
        unreachable!(
            "drive_deal_damage: top frame is not DealDamage; the `drive` loop routes \
             here only when it is — state-corruption invariant violation"
        );
    };
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
            super::emit::queue_event(
                cx,
                &super::emit::TimingEvent::DamageAssigned {
                    source,
                    investigator,
                    assignment,
                },
            )
        }
        DealDamageStep::Place => {
            set_deal_damage(cx, assignment.clone(), DealDamageStep::Finish);
            super::emit::queue_event(
                cx,
                &super::emit::TimingEvent::DamagePlaced {
                    source,
                    investigator,
                    assignment,
                },
            )
        }
        DealDamageStep::Finish => {
            cx.state.continuations.pop();
            match source {
                // The attack's own sequence continues on the frames beneath: its
                // `at` and `after` cells on the coordinator, then the parked
                // `AttackLoop`'s exhaust and the next attacker (#704).
                crate::state::DamageSource::EnemyAttack { .. } => EngineOutcome::Done,
                // K5b-2: resume the parked effect walk, so subsequent effects
                // run (and may prompt again) with no point lost (#422/#44).
                crate::state::DamageSource::Effect => super::choice::resume_effect_walk(cx),
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
        crate::state::DamageSource::EnemyAttack { enemy: enemy_id },
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
    cx.state.continuations.push(Continuation::AttackLoop {
        investigator,
        remaining_attackers: attackers,
        source,
        stage: AttackLoopStage::Attacking,
    });
    super::emit::queue_event(
        cx,
        &super::emit::TimingEvent::EnemyAttacks {
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
    let Some(Continuation::AttackLoop {
        investigator,
        mut remaining_attackers,
        source,
        stage: AttackLoopStage::Attacking,
    }) = cx.state.continuations.pop()
    else {
        unreachable!(
            "drive_parked_attack_loop: top frame is not an AttackLoop{{Attacking}}; \
             the `drive` loop routes here only when it is — state-corruption \
             invariant violation"
        )
    };
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
    let options = super::hunters::candidate_options(&attackers);
    cx.state.continuations.push(Continuation::AttackLoop {
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

/// Resume a loop suspended on its order-pick `PickSingle` (#143). The
/// `AttackLoop{stage: PickOrder}` frame is the top frame (no window above it),
/// so [`resolve_input`](super::resolve_input) routes here directly (not via
/// window-close). Validate the `PickSingle` against the stored
/// `remaining_attackers`; on an invalid pick, reject and **leave the frame** so
/// the client can retry (mirrors `resume_hunter_choice`). On a valid pick, move
/// the chosen enemy to the head and begin its attack ([`begin_head_attack`]) —
/// the parked loop then drives the rest, re-prompting if 2+ still remain.
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

    // Begin the chosen head directly rather than re-entering `drive_attack_loop`
    // — with 2+ still in the list that would re-prompt the same pick forever.
    // The `AttackLoop` frame it parks carries the rest, so the next pick is
    // offered when this attack's sequence pops.
    begin_head_attack(cx, investigator, attackers, source)
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
        // "place the card in the victory display instead of in the discard
        // pile" (`glossary/Victory_Display_Victory_Points.md`) — the victory
        // display is the only pile it lands in (#632).
        assert!(
            state.encounter_discard.is_empty(),
            "a victory enemy goes to the victory display instead of the encounter discard"
        );
    }

    #[test]
    fn defeating_non_victory_enemy_places_its_card_in_the_encounter_discard() {
        // `glossary/Defeat.md`: "that enemy is defeated and placed on the
        // encounter discard pile" — not removed from the game, so the
        // `glossary/Encounter_Deck.md` reshuffle can bring it back (#632).
        let eid = EnemyId(1);
        let mut enemy = test_enemy(1, "Ghoul");
        enemy.code = crate::CardCode::new("01160");
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
        assert_eq!(
            state.encounter_discard,
            vec![crate::CardCode::new("01160")],
            "defeated non-victory enemy lands in the encounter discard"
        );
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

        super::soak_and_place(&mut cx, id, 2, 1);

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
    fn resume_damage_distribution_rejects_invalid_pick_and_keeps_frame() {
        use crate::state::{Continuation, DamageSource, DealDamageStep, EnemyId};
        let inv_id = InvestigatorId(1);
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .build();
        // Park a DealDamage frame mid-distribution (2 damage to assign).
        state.continuations.push(Continuation::DealDamage {
            investigator: inv_id,
            source: DamageSource::EnemyAttack { enemy: EnemyId(7) },
            assignment: super::Assignment::default(),
            step: DealDamageStep::Distribute {
                remaining_damage: 2,
                remaining_horror: 0,
            },
        });
        let mut events = Vec::new();
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };

        // Wrong response variant → reject, frame untouched.
        let wrong = super::resume_damage_distribution(&mut cx, &crate::action::InputResponse::Skip);
        assert!(matches!(wrong, EngineOutcome::Rejected { .. }));

        // Out-of-range option (no soakers → only the investigator is eligible,
        // so any index ≥ 1 is invalid) → reject, frame untouched.
        let oob = super::resume_damage_distribution(
            &mut cx,
            &crate::action::InputResponse::PickSingle(crate::engine::OptionId(5)),
        );
        assert!(matches!(oob, EngineOutcome::Rejected { .. }));

        // The frame survives both rejections, at the same step, for the client
        // to retry — a rejection must not advance the cursor.
        assert!(
            matches!(
                state.continuations.last(),
                Some(Continuation::DealDamage {
                    step: DealDamageStep::Distribute {
                        remaining_damage: 2,
                        remaining_horror: 0
                    },
                    ..
                })
            ),
            "DealDamage{{Distribute}} frame retained after invalid picks"
        );
    }

    /// The cursor's whole walk (#727): `Distribute → Announce → Place → Finish`,
    /// stepped one dispatch at a time with no soaker in play, so distribution
    /// drains immediately and the frame never prompts.
    ///
    /// What it pins is *where the placement is*: nothing is on the investigator
    /// until the `DamagePlaced` coordinator the `Place` step pushes has reached
    /// its own resolve step. The `when` cell between the two conditions is
    /// exactly the gap this walk opens (ADR 0009).
    #[test]
    fn deal_damage_cursor_walks_distribute_announce_place_finish() {
        use crate::state::{Continuation, DamageSource, DealDamageStep, EnemyId};
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

        // Entry parks the frame at the top of the cursor and returns `Done`:
        // dealing damage is tail position, like an emit.
        let out = super::begin_deal_damage(
            &mut cx,
            id,
            2,
            0,
            DamageSource::EnemyAttack { enemy: EnemyId(7) },
        );
        assert_eq!(out, EngineOutcome::Done);
        assert!(matches!(
            cx.state.continuations.last(),
            Some(Continuation::DealDamage {
                step: DealDamageStep::Distribute { .. },
                ..
            })
        ));

        // Distribute: no soaker can take a point, so it drains without
        // prompting and the cursor reaches `Announce` with the whole 2 on the
        // investigator's share.
        assert_eq!(super::drive_deal_damage(&mut cx), EngineOutcome::Done);
        let Some(Continuation::DealDamage {
            assignment, step, ..
        }) = cx.state.continuations.last()
        else {
            panic!("frame gone after Distribute");
        };
        assert_eq!(*step, DealDamageStep::Announce);
        assert_eq!(assignment.investigator_damage, 2);

        // Announce: the cursor advances *before* the emit (tail position), and
        // the coordinator it pushed is now on top.
        assert_eq!(super::drive_deal_damage(&mut cx), EngineOutcome::Done);
        assert!(matches!(
            cx.state.continuations.last(),
            Some(Continuation::EmitEvent {
                event: crate::engine::TimingEvent::DamageAssigned { .. },
                ..
            })
        ));
        assert_eq!(
            cx.state.investigators[&id].damage(),
            0,
            "an assignment is a proposal: nothing is placed at the announcement"
        );
        // Drain that coordinator (a bare milestone with no takers here).
        cx.state.continuations.pop();

        // Place: same shape, and again nothing has landed until the coordinator
        // reaches its resolve step.
        assert_eq!(super::drive_deal_damage(&mut cx), EngineOutcome::Done);
        let Some(Continuation::EmitEvent {
            event: placed @ crate::engine::TimingEvent::DamagePlaced { .. },
            ..
        }) = cx.state.continuations.last().cloned()
        else {
            panic!("Place must emit DamagePlaced");
        };
        assert_eq!(
            cx.state.investigators[&id].damage(),
            0,
            "still nothing placed until DamagePlaced resolves"
        );
        cx.state.continuations.pop();
        // The resolve step is where it lands.
        let resolution = placed.condition_resolution();
        let crate::engine::dispatch::emit::ConditionResolution::Coordinator(resolve) = resolution
        else {
            panic!("DamagePlaced must be coordinator-owned");
        };
        assert_eq!(resolve(&mut cx, &placed), EngineOutcome::Done);
        assert_eq!(cx.state.investigators[&id].damage(), 2, "placed at last");
        assert!(
            cx.events.iter().any(|e| matches!(
                e,
                Event::DamageTaken { investigator, amount: 2 } if *investigator == id
            )),
            "DamageTaken emitted at the placement: {:?}",
            cx.events
        );

        // Finish: pop and hand back to the caller. An enemy attack's own
        // sequence continues on the frames beneath, so this is just `Done`.
        assert_eq!(super::drive_deal_damage(&mut cx), EngineOutcome::Done);
        assert!(
            !cx.state
                .continuations
                .iter()
                .any(|c| matches!(c, Continuation::DealDamage { .. })),
            "the frame pops at Finish"
        );
    }

    /// `DamageAssigned` is the **bare milestone** of the pair and `DamagePlaced`
    /// the one with an impact (ADR 0008's classification, ADR 0009's split).
    /// Asserted directly, because the whole `when`-cell licence Guard Dog needs
    /// rests on the first of those being true.
    #[test]
    fn the_two_damage_conditions_are_classified_as_the_adr_says() {
        use crate::engine::TimingEvent;
        use crate::state::{Assignment, DamageSource, EnemyId};
        let assigned = TimingEvent::DamageAssigned {
            source: DamageSource::EnemyAttack { enemy: EnemyId(1) },
            investigator: InvestigatorId(1),
            assignment: Assignment::default(),
        };
        let placed = TimingEvent::DamagePlaced {
            source: DamageSource::EnemyAttack { enemy: EnemyId(1) },
            investigator: InvestigatorId(1),
            assignment: Assignment::default(),
        };
        // Both coordinator-owned, so both walk their `when` cell.
        for event in [&assigned, &placed] {
            assert!(
                matches!(
                    event.condition_resolution(),
                    crate::engine::dispatch::emit::ConditionResolution::Coordinator(_)
                ),
                "{event:?} must be coordinator-owned"
            );
        }
        // The milestone's resolve step changes nothing; the other's places.
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .build();
        let before = state.clone();
        let mut events = Vec::new();
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };
        let crate::engine::dispatch::emit::ConditionResolution::Coordinator(resolve) =
            assigned.condition_resolution()
        else {
            unreachable!()
        };
        assert_eq!(resolve(&mut cx, &assigned), EngineOutcome::Done);
        assert_eq!(*cx.state, before, "a bare milestone resolves to nothing");
        assert!(events.is_empty());
        // Neither fires forced abilities: every forced taker in the sweep is
        // outside the corpus (ADR 0009).
        assert!(assigned.forced_point().is_none());
        assert!(placed.forced_point().is_none());
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
    fn place_assignment_accumulates_on_asset_and_investigator() {
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

        super::place_assignment(&mut cx, id, &assignment);

        let card = &state.investigators[&id].cards_in_play[0];
        assert_eq!(card.accumulated_damage, 1, "asset soaked 1 damage");
        assert_eq!(card.accumulated_horror, 1, "asset soaked 1 horror");
        assert_eq!(
            state.investigators[&id].damage(),
            1,
            "investigator took overflow damage"
        );
        assert_event!(events, Event::DamageTaken { investigator, amount: 1 } if *investigator == id);
    }

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
        use crate::state::{AttackLoopStage, Continuation, EnemyAttackSource, InvestigatorId};
        crate::test_support::install_test_registry();

        let inv_id = InvestigatorId(1);
        let attacker = EnemyId(2);
        let mut enemy = test_enemy(2, "Attacker");
        enemy.engaged_with = Some(inv_id);

        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_turn_order([inv_id])
            .with_enemy(enemy)
            .with_phase_anchor(crate::state::Continuation::EnemyPhase {
                resume: crate::state::EnemyResume::BeforeInvestigatorAttacked,
                attacking: Some(inv_id),
            })
            .build();
        state.continuations.push(Continuation::AttackLoop {
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
        let outcome = super::drive_parked_attack_loop(&mut cx);

        assert!(
            !state
                .continuations
                .iter()
                .any(|c| matches!(c, Continuation::AttackLoop { .. })),
            "the parked attack-loop frame is consumed"
        );
        assert!(
            state.enemies[&attacker].exhausted,
            "the head attacker exhausts once its sequence completes"
        );
        assert_event!(events, Event::EnemyExhausted { enemy } if *enemy == attacker);
        // Loop finished → `after_enemy_phase_attacks` advanced the cursor past
        // the only investigator and opened the all-attacked window (auto-skips
        // inline with no registry ability), cascading the EnemyPhase anchor off
        // the stack — so no anchor is left still attacking anyone.
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
        use crate::state::{AttackLoopStage, Continuation, EnemyAttackSource, InvestigatorId};
        crate::test_support::install_test_registry();

        let inv_id = InvestigatorId(1);
        let attacker = EnemyId(2);
        let mut enemy = test_enemy(2, "Attacker");
        enemy.engaged_with = Some(inv_id);

        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_turn_order([inv_id])
            .with_enemy(enemy)
            .with_phase_anchor(crate::state::Continuation::EnemyPhase {
                resume: crate::state::EnemyResume::BeforeInvestigatorAttacked,
                attacking: Some(inv_id),
            })
            .build();
        state.continuations.push(Continuation::AttackLoop {
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
        let _ = super::drive_parked_attack_loop(&mut cx);

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
        use crate::state::{AttackLoopStage, Continuation, EnemyAttackSource, InvestigatorId};
        crate::test_support::install_test_registry();

        let inv_id = InvestigatorId(1);
        let attacker = EnemyId(2);
        let mut enemy = test_enemy(2, "Attacker");
        enemy.engaged_with = Some(inv_id);

        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_turn_order([inv_id])
            .with_enemy(enemy)
            .build();
        state.continuations.push(Continuation::AttackLoop {
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
        let _ = super::drive_parked_attack_loop(&mut cx);

        assert!(
            !state.enemies[&attacker].exhausted,
            "an AoO attacker never exhausts (RR p.7)"
        );
        assert_no_event!(events, Event::EnemyExhausted { .. });
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

        // The attack is queued on the coordinator (#704), so drive it out: the
        // loop's `Done` means *queued*, not *dealt*.
        let outcome = super::drive_retaliate(&mut cx, EnemyId(100), inv_id);
        let outcome = super::super::drive(&mut cx, outcome);

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
        let outcome = super::super::drive(&mut cx, outcome);

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

        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };
        let outcome = super::drive_aoo(&mut cx, inv_id);
        let outcome = super::super::drive(&mut cx, outcome);
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
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };
        let resumed = super::super::resolve_input(
            &mut cx,
            &crate::action::InputResponse::PickSingle(OptionId(1)),
        );
        let resumed = super::super::drive(&mut cx, resumed);
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

    #[test]
    fn soak_options_anchor_assets_to_card_instances() {
        use crate::engine::OptionTarget;
        use crate::state::CardInstanceId;
        let targets = vec![
            super::DistributionTarget::Investigator,
            super::DistributionTarget::Asset(CardInstanceId(7)),
        ];
        let opts = super::soak_options(&targets);
        // Anchors: the investigator has no card home; a soaker asset points at its card.
        assert_eq!(
            opts[0].target, None,
            "the investigator is not a board anchor"
        );
        assert_eq!(
            opts[1].target,
            Some(OptionTarget::CardInstance(CardInstanceId(7)))
        );
        // Labels unchanged from the former `hunters::candidate_options` debug repr.
        assert_eq!(opts[0].label, "Investigator");
        assert_eq!(opts[1].label, "Asset(CardInstanceId(7))");
    }
}
