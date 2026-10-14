//! Forbidden Knowledge (Mystic talent asset, 01058).
//!
//! ```text
//! Uses (4 secrets). If Forbidden Knowledge has no secrets, discard it.
//! [fast] Exhaust Forbidden Knowledge and take 1 horror: Move 1 secret from
//!   Forbidden Knowledge to your resource pool, as a resource.
//! ```
//!
//! Ruling (<https://arkhamdb.com/card/01058>):
//! > Moving secrets from Forbidden Knowledge to your resource pool **does not**
//! > count as 'gaining' resources, so can be used in situations where the
//! > investigator with Forbidden Knowledge may not otherwise gain resources
//! > (such as being at Trapper's Cabin or being engaged with O'Bannion's Thug).
//!
//! `Uses (4 secrets)` and the discard at zero are corpus metadata
//! (`discard_when_empty`), so it enters play with 4 and the engine discards it
//! the moment the last one leaves. The one `[fast]` ability pays
//! `Cost::Exhaust` and `Cost::TakeHarm` (1 horror), then a native moves the
//! secret through [`NativeCx::move_uses_to_resource_pool`]: a **move**, so it
//! announces neither a gain nor a spend.

use card_dsl::card_data::UseKind;
use card_dsl::dsl::{self, Ability, Cost};
use game_core::engine::evaluator::EvalContext;
use game_core::engine::{EngineOutcome, MoveUsesError, NativeCx};

use crate::impls::CardRecord;

/// `ArkhamDB` code for Forbidden Knowledge (original-Core printing).
pub const CODE: &str = "01058";

/// This card's registration, listed in [`ALL`](super::ALL).
pub const CARD: CardRecord =
    CardRecord::new(CODE, abilities).effects(&[(MOVE_SECRET, move_secret)]);

const MOVE_SECRET: &str = "01058:move-secret";

/// `[fast]` Exhaust + 1 horror: move 1 secret to your resource pool.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![dsl::activated(
        0,
        vec![
            Cost::Exhaust,
            Cost::TakeHarm {
                damage: 0,
                horror: 1,
            },
        ],
        dsl::native(MOVE_SECRET),
    )]
}

/// *"Move 1 secret from Forbidden Knowledge to your resource pool, as a
/// resource."* A Forbidden Knowledge no longer in play has no secret to move.
fn move_secret(cx: &mut NativeCx, ctx: &EvalContext) -> EngineOutcome {
    let Some(source) = ctx.source_instance() else {
        return EngineOutcome::Rejected {
            reason: "forbidden_knowledge: no source instance".into(),
        };
    };
    match cx.move_uses_to_resource_pool(ctx.controller, source, UseKind::Secrets, 1) {
        Ok(()) | Err(MoveUsesError::NotInPlay) => EngineOutcome::Done,
        Err(err) => EngineOutcome::Rejected {
            reason: format!("forbidden_knowledge: moving a secret failed: {err:?}").into(),
        },
    }
}
