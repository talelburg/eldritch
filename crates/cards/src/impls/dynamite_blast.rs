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
//! ([`deal_damage_to_enemy`](NativeCx::deal_damage_to_enemy), which handles
//! defeat → victory points / Roland's reaction) and every investigator
//! ([`deal_harm`](NativeCx::deal_harm) — the controller included if they
//! blast their own location) at the chosen location. Each investigator gets
//! their own deal, with its own soak prompt, resolving in ascending id order.
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
use game_core::engine::{self, EngineOutcome, Grounded, NativeCx, OptionTarget};
use game_core::state::{EnemyId, GameState, InvestigatorId, LocationId};

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
fn candidate_locations(state: &GameState, controller: InvestigatorId) -> Vec<LocationId> {
    let Some(here) = state
        .investigators
        .get(&controller)
        .and_then(|inv| inv.current_location)
    else {
        return Vec::new();
    };
    let mut locations = vec![here];
    if let Some(loc) = state.locations.get(&here) {
        let mut connections = loc.connections.clone();
        connections.sort_unstable();
        locations.extend(connections);
    }
    locations
}

fn dynamite_blast(cx: &mut NativeCx, ctx: &EvalContext) -> EngineOutcome {
    let controller = ctx.controller;
    let locations = candidate_locations(cx.state(), controller);
    match engine::resolve_grounded_choice(
        cx.state(),
        cx.cards(),
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
fn blast_location(cx: &mut NativeCx, controller: InvestigatorId, loc: LocationId) -> EngineOutcome {
    let enemies: Vec<EnemyId> = cx
        .state()
        .enemies
        .iter()
        .filter(|(_, e)| e.current_location == Some(loc))
        .map(|(id, _)| *id)
        .collect();
    for enemy in enemies {
        cx.deal_damage_to_enemy(enemy, DAMAGE, Some(controller));
    }

    let investigators: Vec<InvestigatorId> = cx
        .state()
        .investigators
        .iter()
        .filter(|(_, i)| i.current_location == Some(loc))
        .map(|(id, _)| *id)
        .collect();
    // One deal per investigator, pushed in reverse id order so they resolve
    // ascending: each deal (its soak prompt, both conditions, any reaction)
    // completes before the next frame is exposed. The deals are the native's
    // tail (ADR 0003): every push returns `Done`, and the fold returns the
    // last one's outcome, so nothing runs after them.
    investigators
        .into_iter()
        .rev()
        .fold(EngineOutcome::Done, |_, inv| cx.deal_harm(inv, DAMAGE, 0))
}

#[cfg(test)]
mod tests {
    use card_dsl::dsl::{Effect, Trigger};
    use game_core::state::{Continuation, GameStateBuilder};
    use game_core::test_support;

    use super::*;

    /// The lone location, with no connections, so the blast auto-targets it.
    const HERE: LocationId = LocationId(1);

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
        let loc = test_support::test_location(HERE.0, "Lonely Spot"); // no connections by default
        let mut state = GameStateBuilder::new()
            .with_registries(crate::TEST_REGISTRIES)
            .with_seat(test_support::seat(1).at(HERE))
            .with_location(loc)
            .build();
        state.interactive_acknowledge = true;
        let ctx = EvalContext::for_controller(InvestigatorId(1));
        let (out, _) = test_support::run_native(&mut state, &crate::REGISTRY, dynamite_blast, &ctx);
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

    #[test]
    fn blast_pushes_one_effect_deal_per_investigator_ascending_from_the_top() {
        // Two investigators at the lone location → auto-target; each is dealt
        // 3 damage through the one harm adapter (#1028), investigator 1's
        // frame on top so the deals resolve in ascending id order. Nothing is
        // placed until the frames run.
        let mut state = GameStateBuilder::new()
            .with_registries(crate::TEST_REGISTRIES)
            .with_seat(test_support::seat(1).at(HERE))
            .with_seat(test_support::seat(2).at(HERE))
            .with_location(test_support::test_location(HERE.0, "Lonely Spot"))
            .build();
        let ctx = EvalContext::for_controller(InvestigatorId(1));
        let (out, _) = test_support::run_native(&mut state, &crate::REGISTRY, dynamite_blast, &ctx);
        assert_eq!(out, EngineOutcome::Done);

        // Top first.
        let deals: Vec<&Continuation> = state
            .continuations
            .iter()
            .rev()
            .filter(|frame| matches!(frame, Continuation::DealDamage(_)))
            .collect();
        assert_eq!(deals.len(), 2, "one deal per investigator: {deals:?}");
        test_support::assert_effect_deal(Some(deals[0]), InvestigatorId(1), 3, 0);
        test_support::assert_effect_deal(Some(deals[1]), InvestigatorId(2), 3, 0);
        assert_eq!(state.investigators[&InvestigatorId(1)].damage(), 0);
        assert_eq!(state.investigators[&InvestigatorId(2)].damage(), 0);
    }
}
