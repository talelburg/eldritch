//! Asserting the deal frame an effect's harm pushes.

use crate::state::{Continuation, DamageSource, DealDamageFrame, DealDamageStep, InvestigatorId};

/// Assert that `frame` is an effect's deal of `damage` + `horror` to
/// `investigator`, none of it assigned yet: a
/// [`DealDamage`](Continuation::DealDamage) frame with source
/// [`Effect`](DamageSource::Effect) at its
/// [`Distribute`](DealDamageStep::Distribute) step.
///
/// The one check each caller of the harm adapter makes that it routes its harm
/// through the deal; the walk from there is pinned once, on the kernel
/// adapter.
///
/// # Panics
///
/// Panics, printing `frame`, if it is anything else.
#[track_caller]
pub fn assert_effect_deal(
    frame: Option<&Continuation>,
    investigator: InvestigatorId,
    damage: u8,
    horror: u8,
) {
    assert!(
        matches!(
            frame,
            Some(Continuation::DealDamage(DealDamageFrame {
                investigator: dealt_to,
                source: DamageSource::Effect,
                step: DealDamageStep::Distribute {
                    remaining_damage,
                    remaining_horror,
                },
                ..
            })) if *dealt_to == investigator
                && *remaining_damage == damage
                && *remaining_horror == horror
        ),
        "expected an effect's DealDamage frame dealing {damage} damage + {horror} horror \
         to {investigator:?}, got {frame:?}",
    );
}
