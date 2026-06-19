//! Dynamite Blast (Guardian event, 01024).
//!
//! ```text
//! Choose either your location or a connecting location. Deal 3 damage to
//!   each enemy and to each investigator at the chosen location.
//! ```
//!
//! A card-local native (the #276 escape hatch), like agenda 01105 / Crypt
//! Chill: it enumerates the controller's location and its connections, applies
//! the choice convention (1 candidate → auto-target, 2+ → suspend via
//! [`suspend_for_native_choice`]), and on resume deals 3 damage to every enemy
//! ([`deal_damage_to_enemy`], which handles defeat → victory points / Roland's
//! reaction) and every investigator ([`take_damage`] — the controller included
//! if they blast their own location) at the chosen location.
//!
//! # Native, not a typed fan-out — on purpose
//!
//! A corpus audit found the only in-scope fan-out / area consumers are this card
//! and agenda 01105 — two *different* shapes, with every other consumer in
//! future Dunwich content. So a general `Effect::ForEach` / `EntityTarget`
//! would be speculative today; both stay card-local natives until Dunwich's
//! fan-out cards justify the abstraction. The deferred (and **not-yet-accepted**)
//! design is captured in #363.
//!
//! Suspending from an `OnPlay` event relies on the played event being discarded
//! on *completion* (`GameState.pending_played_event`, RR Appendix I step 4: the
//! card is placed in discard "simultaneously with the completion" of its
//! effect) — so it's discarded when the choice resolves, not stranded in hand.

use card_dsl::dsl::{native, on_play, Ability};
use game_core::card_registry::NativeEffectFn;
use game_core::state::{EnemyId, InvestigatorId, LocationId};
use game_core::{
    deal_damage_to_enemy, resolve_choice_count, suspend_for_native_choice, take_damage,
    ChoiceResolution, Cx, EngineOutcome, EvalContext,
};

/// `ArkhamDB` code for Dynamite Blast (original-Core printing).
pub const CODE: &str = "01024";

const BLAST: &str = "01024:blast";

/// Damage dealt to each enemy and investigator at the chosen location.
const DAMAGE: u8 = 3;

/// Dynamite Blast's `OnPlay` area-of-effect.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![on_play(native(BLAST))]
}

/// Resolve this event's native-effect tag. Wired into the crate registry's
/// `native_effect_for`.
pub(crate) fn native_effect_for(tag: &str) -> Option<NativeEffectFn> {
    (tag == BLAST).then_some(dynamite_blast as NativeEffectFn)
}

/// Candidate target locations: the controller's location followed by each
/// connected location, connections sorted by id so an `OptionId` indexes the
/// same location when the choice replays on resume.
fn candidate_locations(cx: &Cx, controller: InvestigatorId) -> Vec<LocationId> {
    let Some(here) = cx
        .state
        .investigators
        .get(&controller)
        .and_then(|inv| inv.current_location)
    else {
        return Vec::new();
    };
    let mut locations = vec![here];
    if let Some(loc) = cx.state.locations.get(&here) {
        let mut connections = loc.connections.clone();
        connections.sort_unstable();
        locations.extend(connections);
    }
    locations
}

fn dynamite_blast(cx: &mut Cx, ctx: &EvalContext) -> EngineOutcome {
    let controller = ctx.controller;
    let locations = candidate_locations(cx, controller);

    // Resume: a pick was threaded in — re-enumerate and index by it.
    if let Some(picked) = ctx.chosen_option() {
        let Some(&loc) = locations.get(picked.0 as usize) else {
            return EngineOutcome::Rejected {
                reason: "01024 blast: chosen_option out of range".into(),
            };
        };
        return blast_location(cx, controller, loc);
    }

    match resolve_choice_count(locations.len()) {
        // Controller is between locations — no legal target.
        ChoiceResolution::Empty => EngineOutcome::Rejected {
            reason: "01024 blast: controller has no location to target".into(),
        },
        // Exactly one (your location, no connections) → auto-target.
        ChoiceResolution::Auto(i) => blast_location(cx, controller, locations[i]),
        // 2+ → suspend for the controller's pick.
        ChoiceResolution::Suspend => {
            let labels = locations.iter().map(|id| format!("{id:?}")).collect();
            suspend_for_native_choice(cx, "Choose a location to blast", labels, BLAST, ctx)
        }
    }
}

/// Deal [`DAMAGE`] to each enemy and each investigator at `loc`. Ids are
/// snapshotted first (in `BTreeMap` order — deterministic for replay) because
/// dealing damage can defeat enemies/investigators and mutate the maps mid-loop.
/// Enemy damage is attributed to `controller` so a defeat counts as "you
/// defeat" (victory points, Roland's reaction).
fn blast_location(cx: &mut Cx, controller: InvestigatorId, loc: LocationId) -> EngineOutcome {
    let enemies: Vec<EnemyId> = cx
        .state
        .enemies
        .iter()
        .filter(|(_, e)| e.current_location == Some(loc))
        .map(|(id, _)| *id)
        .collect();
    for enemy in enemies {
        deal_damage_to_enemy(cx, enemy, DAMAGE, Some(controller));
    }

    let investigators: Vec<InvestigatorId> = cx
        .state
        .investigators
        .iter()
        .filter(|(_, i)| i.current_location == Some(loc))
        .map(|(id, _)| *id)
        .collect();
    for inv in investigators {
        take_damage(cx, inv, DAMAGE);
    }

    EngineOutcome::Done
}

#[cfg(test)]
mod tests {
    use super::*;
    use card_dsl::dsl::{Effect, Trigger};

    #[test]
    fn one_on_play_native_blast() {
        let abilities = abilities();
        assert_eq!(abilities.len(), 1);
        assert_eq!(abilities[0].trigger, Trigger::OnPlay);
        assert!(
            matches!(&abilities[0].effect, Effect::Native { tag } if tag == BLAST),
            "OnPlay is the blast native",
        );
        assert!(native_effect_for(BLAST).is_some());
        assert!(native_effect_for("nope").is_none());
    }

    /// Catches a `pub mod` rename or a fat-fingered match arm in
    /// `impls::abilities_for` — the registry must dispatch CODE here.
    #[test]
    fn registry_dispatches_to_this_modules_abilities() {
        assert_eq!(crate::abilities_for(CODE), Some(abilities()));
    }
}
