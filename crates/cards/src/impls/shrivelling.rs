//! Shrivelling (Mystic asset, 01060).
//!
//! ```text
//! Uses (4 charges).
//! [action] Spend 1 charge: Fight. This attack uses [willpower] instead of
//! [combat] and deals +1 damage. If a [skull], [cultist], [tablet],
//! [elder_thing], or [auto_fail] symbol is revealed during this attack, take
//! 1 horror.
//! ```
//!
//! **Designator: Fight** — the bold word **performs the attack**, modified as
//! the card prints: it tests `[willpower]` in place of `[combat]` (the
//! designator's skill override), deals `1 + 1` damage on success, and carries
//! the horror clause onto the attack's skill test as a
//! [`RevealClause`]. The ability's own effect is
//! empty.
//!
//! The horror is not a reaction to the token. Ruling
//! (<https://arkhamdb.com/card/01060>): *"The "If a symbol is revealed..."
//! effect triggers during Step 3 of the Skill Test Timing."* So the clause
//! resolves inside the test, as the token is revealed, before the result is
//! determined, whether the attack then succeeds or fails.
//!
//! Charges (4) come from the corpus (`CardKind::Asset.uses`, pipeline-parsed);
//! the ability spends 1 per use via `Cost::SpendUses`.

use card_dsl::card_data::{SkillKind, UseKind};
use card_dsl::dsl::{self, Ability, ChaosSymbol, Cost, InvestigatorTarget, RevealClause};

use crate::impls::CardRecord;

/// `ArkhamDB` code for Shrivelling.
pub const CODE: &str = "01060";

/// This card's registration, listed in [`ALL`](super::ALL).
pub const CARD: CardRecord = CardRecord::new(CODE, abilities);

#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![dsl::activated_as(
        dsl::fight(0i8, 1u8)
            .testing(SkillKind::Willpower)
            .with_reveal_clause(RevealClause {
                symbols: ChaosSymbol::ALL_BUT_ELDER_SIGN.to_vec(),
                effect: Box::new(dsl::deal_horror(InvestigatorTarget::You, 1u8)),
            }),
        1,
        vec![Cost::SpendUses {
            kind: UseKind::Charges,
            count: 1,
        }],
        dsl::seq([]),
    )]
}
