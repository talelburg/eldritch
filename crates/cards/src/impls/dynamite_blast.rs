//! Dynamite Blast (Guardian event, 01024).
//!
//! ```text
//! Choose either your location or a connecting location. Deal 3 damage to
//!   each enemy and to each investigator at the chosen location.
//! ```
//!
//! A card-local native (the #276 escape hatch), like agenda 01105 / Crypt
//! Chill: it enumerates the controller's location and its connections, offers
//! them through [`resolve_grounded_choice`](engine::resolve_grounded_choice)
//! (1 candidate → auto-target, 2+ → suspend for a pick), and on the pick deals
//! 3 damage to every enemy
//! ([`deal_damage_to_enemy`](engine::deal_damage_to_enemy), which handles
//! defeat → victory points / Roland's reaction) and every investigator
//! ([`take_damage`](engine::take_damage) — the controller included if they
//! blast their own location) at the chosen location.
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
//! on *completion* (RR Appendix I step 4: the card is placed in discard
//! "simultaneously with the completion" of its effect) — so it's discarded when
//! the choice resolves, not stranded in hand. The card rides its
//! `Continuation::PlayFromHand` frame across the suspension; when the attack of
//! opportunity this play provokes lets a Fast event be played on top, the two
//! plays get their own frames rather than sharing one slot (#604).

use card_dsl::dsl::{self, Ability};
use game_core::engine::evaluator::EvalContext;
use game_core::engine::{self, Cx, EngineOutcome, Grounded, OptionTarget};
use game_core::state::{EnemyId, InvestigatorId, LocationId};

use crate::impls::CardRecord;

/// `ArkhamDB` code for Dynamite Blast (original-Core printing).
pub const CODE: &str = "01024";

/// This card's registration, listed in [`ALL`](super::ALL).
pub const CARD: CardRecord = CardRecord::new(CODE, abilities).effects(&[(BLAST, dynamite_blast)]);

const BLAST: &str = "01024:blast";

/// Damage dealt to each enemy and investigator at the chosen location.
const DAMAGE: u8 = 3;

/// Dynamite Blast's `OnPlay` area-of-effect.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![dsl::on_play(dsl::native(BLAST))]
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
    match engine::resolve_grounded_choice(
        cx.state,
        ctx,
        &locations,
        "Choose a location to blast",
        |id| OptionTarget::Location(*id),
    ) {
        Grounded::Picked(loc) => blast_location(cx, controller, loc),
        // Controller is between locations — no legal target.
        Grounded::Empty => EngineOutcome::Rejected {
            reason: "01024 blast: controller has no location to target".into(),
        },
        Grounded::Suspend(outcome) => outcome,
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
        engine::deal_damage_to_enemy(cx, enemy, DAMAGE, Some(controller));
    }

    let investigators: Vec<InvestigatorId> = cx
        .state
        .investigators
        .iter()
        .filter(|(_, i)| i.current_location == Some(loc))
        .map(|(id, _)| *id)
        .collect();
    for inv in investigators {
        engine::take_damage(cx, inv, DAMAGE);
    }

    EngineOutcome::Done
}

#[cfg(test)]
mod tests {
    use card_dsl::dsl::{Effect, Trigger};
    use game_core::event::Event;
    use game_core::state::GameStateBuilder;
    use game_core::test_support;

    use super::*;

    #[test]
    fn one_on_play_native_blast() {
        let abilities = abilities();
        assert_eq!(abilities.len(), 1);
        assert_eq!(abilities[0].trigger, Trigger::OnPlay);
        assert!(
            matches!(&abilities[0].effect, Effect::Native { tag } if tag == BLAST),
            "OnPlay is the blast native",
        );
    }

    /// Catches a `CARD` record wired to the wrong code or abilities fn —
    /// the registry must dispatch CODE here.
    #[test]
    fn registry_dispatches_to_this_modules_abilities() {
        assert_eq!(crate::abilities_for(CODE), Some(abilities()));
    }

    #[test]
    fn single_location_blast_surfaces_under_interactive_flag() {
        // Sole candidate (controller's location, no connections). With
        // interactive_acknowledge on, the blast target must surface as a
        // one-option pick rather than auto-targeting silently (#466).
        let loc = test_support::test_location(1, "Lonely Spot"); // no connections by default
        let mut inv = test_support::test_investigator(1);
        inv.current_location = Some(LocationId(1));
        let mut state = GameStateBuilder::new()
            .with_investigator(inv)
            .with_location(loc)
            .build();
        state.interactive_acknowledge = true;
        let mut events: Vec<Event> = Vec::new();
        let ctx = EvalContext::for_controller(InvestigatorId(1));
        let out = {
            let mut cx = Cx {
                state: &mut state,
                events: &mut events,
            };
            dynamite_blast(&mut cx, &ctx)
        };
        match out {
            EngineOutcome::AwaitingInput { request, .. } => {
                assert_eq!(
                    request.options.len(),
                    1,
                    "lone location surfaces as one option"
                );
            }
            other => panic!("expected a one-option blast suspend, got {other:?}"),
        }
    }
}
