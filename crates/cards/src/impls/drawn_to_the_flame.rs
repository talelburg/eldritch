//! Drawn to the Flame (Mystic event, 01064).
//!
//! ```text
//! Insight. Cost 0.
//! Draw the top card of the encounter deck. Then, discover 2 clues at your
//! location.
//! ```
//!
//! `OnPlay` is `Seq[Native(draw), DiscoverClue{YourLocation, 2}]`. The native
//! draws through [`NativeCx::draw_encounter_top`] (which reshuffles an empty
//! deck from the encounter discard, the encounter draw's own rule) and resolves
//! the card through [`NativeCx::resolve_encounter_card`], which pushes its
//! Revelation and disposal as frames above the `Seq`. The drive loop resolves
//! those in full before the `Seq` steps on to the discovery, which reads the
//! drawer's location when it runs. That is the order the rulings
//! (<https://arkhamdb.com/card/01064>) require:
//!
//! - *"Resolve any keywords and **Revelation** abilities on the encounter card
//!   before discovering clues."*
//! - *"… You discover clues from the location you end at, if any, rather than
//!   the original location at which you played Drawn to the Flame."*
//! - *"You can only 'discover' a clue if there is a clue on your location."* —
//!   discovering at a location with no clues discovers nothing.
//!
//! With the deck and the discard both empty, the native draws nothing and the
//! discovery still happens.
//!
//! A drawn card that eliminates the drawer leaves them at no location. The
//! discovery is then skipped (`If` over the card-local native condition
//! `01064:drawer-active`): an eliminated investigator discovers nothing, and a
//! location-less `DiscoverClue` would reject, rolling back the whole play.
//!
//! # Module gap
//!
//! TODO(#1060): resolve the drawn card's surge and peril keywords, which
//! `resolve_encounter_card` does not. The corpus has cards with each, such as
//! Core's Hunting Shadow 01135 (Peril) and The Dunwich Legacy's Something in
//! the Drinks 02081 (Surge).

use card_dsl::dsl::{self, Ability, LocationTarget};
use game_core::engine::evaluator::EvalContext;
use game_core::engine::{EngineOutcome, NativeCx};
use game_core::state::{GameState, Status};

use crate::impls::CardRecord;

/// `ArkhamDB` code for the original-Core printing.
pub const CODE: &str = "01064";

/// This card's registration, listed in [`ALL`](super::ALL).
pub const CARD: CardRecord = CardRecord::new(CODE, abilities)
    .effects(&[(DRAW, draw)])
    .conditions(&[(DRAWER_ACTIVE, drawer_active)]);

/// Native-effect tag for the encounter draw.
const DRAW: &str = "01064:draw";

/// Native-condition tag: the drawer is still in the game after the drawn card
/// resolved.
const DRAWER_ACTIVE: &str = "01064:drawer-active";

/// Draw and resolve the top encounter card, then discover 2 clues at your
/// location.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![dsl::on_play(dsl::seq([
        dsl::native(DRAW),
        dsl::if_(
            dsl::native_condition(DRAWER_ACTIVE),
            dsl::discover_clue(LocationTarget::YourLocation, 2),
        ),
    ]))]
}

/// Draw the top encounter card for the controller and push its resolution.
fn draw(cx: &mut NativeCx, ctx: &EvalContext) -> EngineOutcome {
    let Some(code) = cx.draw_encounter_top() else {
        return EngineOutcome::Done;
    };
    cx.resolve_encounter_card(ctx.controller, code)
}

/// `true` while the controller is still in the game (not eliminated).
fn drawer_active(state: &GameState, ctx: &EvalContext) -> bool {
    state
        .investigators
        .get(&ctx.controller)
        .is_some_and(|inv| inv.status == Status::Active)
}
