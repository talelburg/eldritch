//! Paranoia (basic weakness treachery, 01097).
//!
//! ```text
//! Revelation - Discard all your resources.
//! ```
//!
//! A one-shot treachery: its Revelation is its only ability, so once it
//! resolves the card goes to its bearer's discard pile.
//!
//! The discard is a card-local native over
//! [`NativeCx::discard_resources`], a single consumer of "discard resources"
//! (`docs/agents/standards.md`, the DSL-primitive threshold). It announces
//! [`ResourcesDiscarded`](game_core::event::Event::ResourcesDiscarded), not a
//! payment.
//!
//! The ruling (<https://arkhamdb.com/card/01097>): *"During the Upkeep phase,
//! you draw a card first, then gain a resource. So if you draw Paranoia, you
//! immediately discard all resources, then gain 1."* That falls out of the
//! upkeep's order: every investigator's step-4.4 draw, its Revelation included,
//! resolves before the resource is gained.

use card_dsl::dsl::{self, Ability};
use game_core::engine::evaluator::EvalContext;
use game_core::engine::{EngineOutcome, NativeCx};

use crate::impls::CardRecord;

/// `ArkhamDB` code for Paranoia.
pub const CODE: &str = "01097";

/// This card's registration, listed in [`ALL`](super::ALL).
pub const CARD: CardRecord =
    CardRecord::new(CODE, abilities).effects(&[(DISCARD_RESOURCES, discard_resources)]);

/// Native tag: discard every resource in the drawer's pool.
const DISCARD_RESOURCES: &str = "01097:discard-resources";

/// "**Revelation** - Discard all your resources."
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![dsl::revelation(dsl::native(DISCARD_RESOURCES))]
}

fn discard_resources(cx: &mut NativeCx, ctx: &EvalContext) -> EngineOutcome {
    let all = cx
        .state()
        .investigators
        .get(&ctx.controller)
        .map_or(0, |inv| inv.resources);
    cx.discard_resources(ctx.controller, all);
    EngineOutcome::Done
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Catches a `CARD` record wired to the wrong code or abilities fn —
    /// the registry must dispatch CODE here.
    #[test]
    fn registry_dispatches_to_this_modules_abilities() {
        assert_eq!(crate::abilities_for(CODE), Some(abilities()));
    }
}
