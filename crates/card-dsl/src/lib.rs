//! Pure data types for Eldritch card declarations.
//!
//! This crate holds the two pure-data type families consumed by both
//! sides of the cards-engine boundary:
//!
//! - [`dsl`] — the card-effect DSL ([`Ability`], [`Effect`],
//!   [`Trigger`], builder functions). The alphabet card declarations
//!   speak.
//! - [`card_data`] — static card metadata ([`CardMetadata`],
//!   [`Class`], [`CardType`], [`SkillIcons`], [`Slot`]). What's
//!   printed on a card.
//!
//! These types have no I/O, no state, and no engine machinery. They
//! sit between the `cards` corpus (which constructs them) and the
//! `game-core` engine (which evaluates them). Splitting them out of
//! `game-core` avoids the asymmetric tension of housing "what cards
//! say" inside the engine crate, and the resulting layering is:
//!
//! ```text
//! card-dsl   ←  cards
//!     ↑          ↑
//!     └──  game-core
//! ```
//!
//! Both `game-core` and `cards` depend on `card-dsl`; neither depends
//! on the other directly for DSL or card-metadata types. (The
//! `cards → game-core` dependency persists for `CardCode`, the
//! card-registry binding, and `game-core::state` types that the
//! ability-registration path touches.)

pub mod card_data;
pub mod dsl;
