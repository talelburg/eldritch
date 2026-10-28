//! Amnesia (basic weakness treachery, 01096).
//!
//! ```text
//! Revelation - Choose and discard all but 1 card from your hand.
//! ```
//!
//! A one-shot treachery: its Revelation is its only ability, so once it
//! resolves the card goes to its bearer's discard pile.
//!
//! **The choice is of the card to keep.** Choosing the 1 card that stays is the
//! same as choosing every card that goes, and it is one pick rather than many.
//! The native offers the hand through
//! [`resolve_grounded_choice`](engine::resolve_grounded_choice), each option on
//! its hand card, asks before it writes, then discards the rest through
//! [`NativeCx::discard_from_hand`], from the highest hand index down so each
//! index still names its card. A lone card is kept without a prompt, and an
//! empty hand has nothing to keep.
//!
//! **A weakness in hand must be kept.** The ruling
//! (<https://arkhamdb.com/card/01096>): *"RRG entry for Weakness says: "A player
//! may not optionally choose to discard a weakness card from hand". So if you
//! have Dark Memory in hand, you must keep it and discard all other cards."* So
//! with a weakness in hand, only the weaknesses are candidates to keep, and a
//! lone one is kept with no choice offered.
//!
//! # Module gap
//!
//! Two weaknesses in hand at once is not settled by the ruling, which keeps
//! only one card, and is unreachable with the core set: Dark Memory 01013 is
//! its only weakness that stays in hand. The native offers either to keep and
//! discards the other. TODO(#1133): settle which of the two is kept.

use card_dsl::dsl::{self, Ability};
use game_core::engine::evaluator::EvalContext;
use game_core::engine::{self, EngineOutcome, Grounded, NativeCx, OptionTarget};
use game_core::state::{GameState, InvestigatorId};

use crate::impls::CardRecord;

/// `ArkhamDB` code for Amnesia.
pub const CODE: &str = "01096";

/// This card's registration, listed in [`ALL`](super::ALL).
pub const CARD: CardRecord = CardRecord::new(CODE, abilities).effects(&[(KEEP_ONE, keep_one)]);

/// Native tag: keep the chosen card in hand and discard the rest.
const KEEP_ONE: &str = "01096:keep-one";

/// "**Revelation** - Choose and discard all but 1 card from your hand."
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![dsl::revelation(dsl::native(KEEP_ONE))]
}

/// The hand indices of the cards `investigator` may keep, in hand order: the
/// weaknesses if they hold any, otherwise the whole hand.
fn keepable(state: &GameState, investigator: InvestigatorId) -> Vec<u8> {
    let Some(inv) = state.investigators.get(&investigator) else {
        return Vec::new();
    };
    let index = |i: usize| u8::try_from(i).expect("a hand fits in u8");
    let weaknesses: Vec<u8> = inv
        .hand
        .iter()
        .enumerate()
        .filter(|(_, code)| crate::by_code(code.as_str()).is_some_and(|m| m.weakness.is_some()))
        .map(|(i, _)| index(i))
        .collect();
    if weaknesses.is_empty() {
        (0..inv.hand.len()).map(index).collect()
    } else {
        weaknesses
    }
}

fn keep_one(cx: &mut NativeCx, ctx: &EvalContext) -> EngineOutcome {
    let investigator = ctx.controller;
    let candidates = keepable(cx.state(), investigator);
    let kept = match engine::resolve_grounded_choice(
        cx.state(),
        cx.cards(),
        ctx,
        &candidates,
        "Amnesia: choose the 1 card to keep in your hand",
        |&hand_index| OptionTarget::HandCard {
            investigator,
            hand_index,
        },
    ) {
        Grounded::Picked(kept) => usize::from(kept),
        Grounded::Empty => return EngineOutcome::Done,
        Grounded::Suspend(outcome) => return outcome,
    };
    let hand_len = cx
        .state()
        .investigators
        .get(&investigator)
        .map_or(0, |inv| inv.hand.len());
    for hand_index in (0..hand_len).rev().filter(|&i| i != kept) {
        cx.discard_from_hand(investigator, hand_index);
    }
    EngineOutcome::Done
}
