//! The Necronomicon (John Dee Translation), Daisy Walker's signature asset
//! weakness, 01009.
//!
//! ```text
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
//! *"Treat each `[elder_sign]` you reveal on a chaos token as a
//! `[auto_fail]`."* is a constant `Effect::TreatRevealedToken`, read off the
//! threat area of the investigator revealing (#1094). Everything after the
//! reveal reads the auto-fail, so Daisy's own `[elder_sign]` ability never
//! initiates while this is in play.
//!
//! # Module gap
//!
//! Still missing, with its own ticket under #779: the `[action]` that moves 1
//! horror to Daisy Walker and discards the card once it is empty (#1095).
//! Until it lands, the card's 3 horror never moves, so it stays in play for the
//! rest of the game.

use card_dsl::dsl::{self, Ability, ChaosSymbol, CmpOp, Condition, Quantity, Restriction};

use crate::impls::CardRecord;

/// `ArkhamDB` code for The Necronomicon.
pub const CODE: &str = "01009";

/// This card's registration, listed in [`ALL`](super::ALL).
pub const CARD: CardRecord = CardRecord::new(CODE, abilities);

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
    ]
}
