//! The Necronomicon (John Dee Translation), Daisy Walker's signature asset
//! weakness, 01009.
//!
//! ```text
//! The Necronomicon. John Dee Translation.
//! Item. Tome.
//!
//! Revelation - Put The Necronomicon into play in your threat area, with 3
//!   horror on it. It cannot leave play while it has 1 or more horror on it.
//! Treat each [elder_sign] you reveal on a chaos token as a [auto_fail].
//! [action]: Move 1 horror from The Necronomicon to Daisy Walker. Then, if The
//!   Necronomicon has no horror on it, discard it.
//! ```
//!
//! An asset weakness, so drawing it resolves its Revelation
//! (`glossary/Weakness.md`: *"When an investigator draws a weakness with a
//! player cardtype … resolve any Revelation effects on the card, and add it to
//! that investigator's hand"*). The Revelation puts it into play, so it never
//! reaches the hand. Fetching it with a search resolves the Revelation too: the
//! ruling (<https://arkhamdb.com/card/01009>) is *"If you add this to your hand
//! without drawing (e.g. via Research Librarian), the **Revelation** ability
//! still triggers."* Both paths run through the engine's drawn-weakness
//! resolution, and this module only declares the Revelation they resolve. The
//! 3 horror is placed as the card enters (`Effect::PutIntoThreatArea`'s
//! `horror`), not dealt, so no harm event fires for it.
//!
//! *"It cannot leave play while it has 1 or more horror on it."* is a constant
//! `Restriction::CannotLeavePlay` whose condition reads the card's own horror
//! (`Quantity::HorrorOnSource`). The engine's leave-play router refuses every
//! exit while it holds, since `glossary/Cannot.md` reads *"The word "cannot" is
//! absolute, and cannot be countermanded by other abilities."*, and a slot
//! make-room never offers it. Crypt Chill 01167's ruling
//! (<https://arkhamdb.com/card/01167>) names this card: *"You cannot choose and
//! discard The Necronomicon or Baron Samedi because they 'cannot leave
//! play'."* Elimination removes it all the same (`glossary/Elimination.md` step
//! 0, *"remove those weaknesses from the game"*).
//!
//! *"Treat each \[elder_sign\] you reveal on a chaos token as a
//! \[auto_fail\]."* is a constant `Effect::TreatRevealedToken`, read off the
//! threat area of the investigator revealing. Everything after the
//! reveal reads the auto-fail, so Daisy's own `[elder_sign]` ability never
//! initiates while this is in play.
//!
//! *"\[action\]: Move 1 horror from The Necronomicon to Daisy Walker. Then, if
//! The Necronomicon has no horror on it, discard it."* is a native on
//! [`NativeCx::move_horror`]: a move, not a deal, so nothing keyed on harm
//! fires. The ruling (<https://arkhamdb.com/card/01009>): *"Moving damage or
//! horror is different from 'dealing' or 'taking' damage or horror, so does
//! not trigger effects or allow the player controlling Daisy Walker to
//! reassign the horror."* Daisy is the investigator whose threat area holds
//! it, the card's only bearer (it is her signature weakness). It is a Tome
//! `[action]` ability, so her Tome-only additional action can pay for it.

use card_dsl::dsl::{self, Ability, ChaosSymbol, CmpOp, Condition, Quantity, Restriction};
use game_core::engine::board::{self, NotLeft};
use game_core::engine::evaluator::EvalContext;
use game_core::engine::{EngineOutcome, NativeCx};

use crate::impls::CardRecord;

/// `ArkhamDB` code for The Necronomicon.
pub const CODE: &str = "01009";

/// This card's registration, listed in [`ALL`](super::ALL).
pub const CARD: CardRecord =
    CardRecord::new(CODE, abilities).effects(&[(MOVE_HORROR_TAG, move_horror_to_daisy)]);

/// Native tag: move 1 horror to Daisy, then discard the card if it is empty.
const MOVE_HORROR_TAG: &str = "01009:move-horror";

/// The Necronomicon's printed abilities:
///
/// - *"Revelation - Put The Necronomicon into play in your threat area, with 3
///   horror on it. It cannot leave play while it has 1 or more horror on it."*
///   The Revelation and the constant restriction, two abilities here.
/// - *"Treat each \[elder_sign\] you reveal on a chaos token as a
///   \[auto_fail\]."*
/// - *"\[action\]: Move 1 horror from The Necronomicon to Daisy Walker. Then,
///   if The Necronomicon has no horror on it, discard it."*
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![
        dsl::revelation(dsl::put_into_threat_area_with_horror(CODE, 3)),
        dsl::constant(dsl::restrict(Restriction::CannotLeavePlay {
            while_: Condition::Compare {
                quantity: Quantity::HorrorOnSource,
                op: CmpOp::Ge,
                value: 1,
            },
        })),
        dsl::constant(dsl::treat_revealed_token(
            ChaosSymbol::ElderSign,
            ChaosSymbol::AutoFail,
        )),
        dsl::activated(1, vec![], dsl::native(MOVE_HORROR_TAG)),
    ]
}

/// *"Move 1 horror from The Necronomicon to Daisy Walker. Then, if The
/// Necronomicon has no horror on it, discard it."*
///
/// A move that defeats Daisy eliminates her, which removes this card from the
/// game, so there is nothing left to discard.
fn move_horror_to_daisy(cx: &mut NativeCx, ctx: &EvalContext) -> EngineOutcome {
    let Some(source) = ctx.source_instance() else {
        return EngineOutcome::Rejected {
            reason: "the_necronomicon: no source instance".into(),
        };
    };
    // Daisy is the investigator whose threat area holds it.
    let Some(daisy) = board::find_instance(cx.state(), source)
        .and_then(|(_, placement)| placement.investigator())
        .and_then(|bearer| cx.state().investigators.get(&bearer))
        .map(|inv| inv.investigator_card.instance_id)
    else {
        return EngineOutcome::Rejected {
            reason: "the_necronomicon: the card is in no investigator's area, so there is \
                     no Daisy Walker to move horror to"
                .into(),
        };
    };
    if let Err(err) = cx.move_horror(source, daisy, 1) {
        return EngineOutcome::Rejected {
            reason: format!("the_necronomicon: moving horror failed: {err:?}").into(),
        };
    }
    let empty = board::find_instance(cx.state(), source)
        .is_some_and(|(card, _)| card.accumulated_horror == 0);
    if empty {
        match cx.discard_from_play(source) {
            Ok(_) | Err(NotLeft::NotInPlay) => {}
            Err(NotLeft::Refused) => {
                return EngineOutcome::Rejected {
                    reason: "the_necronomicon: refused to leave play with no horror".into(),
                };
            }
        }
    }
    EngineOutcome::Done
}
