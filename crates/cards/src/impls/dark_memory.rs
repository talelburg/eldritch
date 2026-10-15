//! Dark Memory (Agnes Baker's signature weakness event, 01013).
//!
//! ```text
//! Place 1 doom on the current agenda. This effect can cause the current
//!   agenda to advance.
//! Forced - If Dark Memory is in your hand at the end of your turn, reveal it
//!   and take 2 horror.
//! ```
//!
//! The play effect is Ancient Evils 01166's: the second sentence makes it
//! [`place_doom_that_can_advance_the_agenda`](dsl::place_doom_that_can_advance_the_agenda).
//!
//! **Cell: the `at` cell of the `EndOfTurn` condition**, for the forced
//! ability: its *"If … at the end of your turn"* is a bare *"if"* on a settled
//! state, as Frozen in Fear 01164's *"At the end of your turn"* is.
//!
//! **The forced ability works from hand**, the one corpus ability that does.
//! `glossary/Ability.md`: *"Card abilities only interact with the game if the
//! card bearing the ability is in play, unless the ability (or rules for the
//! cardtype) specifically references its use from an out-of-play area."* This
//! one names *"in your hand"*, so it is marked live in hand
//! ([`AbilityZone::Hand`]), which the forced scan reads (ADR 0018), and the
//! `EndOfTurn` pattern's *"your turn"* binds it to the investigator holding it.
//! *"Reveal it"* shows the card to the table and changes nothing the engine
//! models; the card stays in hand.
//!
//! Its ruling (<https://arkhamdb.com/card/01013>) *"You cannot choose to discard
//! a Weakness card from your hand, neither for exceeding the max number of
//! cards in hand, …"* is the engine's, not this card's: the upkeep hand-size
//! discard refuses any weakness.

use card_dsl::dsl::{self, Ability, AbilityZone, EventPattern, EventTiming, InvestigatorTarget};

use crate::impls::CardRecord;

/// `ArkhamDB` code for Dark Memory.
pub const CODE: &str = "01013";

/// This card's registration, listed in [`ALL`](super::ALL).
pub const CARD: CardRecord = CardRecord::new(CODE, abilities);

/// On play, 1 doom that can advance the agenda; held at the end of your turn,
/// 2 horror.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![
        dsl::on_play(dsl::place_doom_that_can_advance_the_agenda(1u8)),
        dsl::forced_on_event(
            EventPattern::EndOfTurn,
            EventTiming::At,
            dsl::deal_horror(InvestigatorTarget::You, 2u8),
        )
        .with_active_in(AbilityZone::Hand),
    ]
}
