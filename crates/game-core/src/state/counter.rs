//! Monotonic id allocation.
//!
//! [`Counter<T>`] is a `u32` id allocator phantom-typed by the id it
//! mints, so the "which counter mints which id" invariant is structural:
//! a `Counter<EnemyId>` mints only `EnemyId`s, and minting from the wrong
//! counter won't type-check. The `define_id!` macro (crate-internal)
//! defines a `u32`-wrapping id newtype together with the `From<u32>` impl
//! `Counter` mints through, so a new id type is one macro call.

use std::marker::PhantomData;

use serde::{Deserialize, Serialize};

/// A monotonic id allocator that mints exactly one id type.
///
/// The phantom `T` ties the counter to its newtype: [`mint`](Self::mint)
/// yields a `T`, and you cannot mint an `EnemyId` from a
/// `Counter<CardInstanceId>`. Serializes transparently as its underlying
/// `u32`, so persisted state is just a number.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Counter<T> {
    next: u32,
    #[serde(skip)]
    _id: PhantomData<fn() -> T>,
}

impl<T> Counter<T> {
    /// A fresh counter starting at 0.
    #[must_use]
    pub fn new() -> Self {
        Self {
            next: 0,
            _id: PhantomData,
        }
    }

    /// A counter positioned at `next` — for test setup and replay restore.
    #[must_use]
    pub fn at(next: u32) -> Self {
        Self {
            next,
            _id: PhantomData,
        }
    }

    /// The next value that would be minted, without advancing.
    #[must_use]
    pub fn peek(&self) -> u32 {
        self.next
    }
}

impl<T: From<u32>> Counter<T> {
    /// Mint the next id and advance the counter (saturating).
    pub fn mint(&mut self) -> T {
        let id = T::from(self.next);
        self.next = self.next.saturating_add(1);
        id
    }
}

impl<T> Default for Counter<T> {
    fn default() -> Self {
        Self::new()
    }
}

// Hand-written (not derived) so equality doesn't require `T: PartialEq` —
// the phantom carries no value, so two counters are equal iff their next
// values match.
impl<T> PartialEq for Counter<T> {
    fn eq(&self, other: &Self) -> bool {
        self.next == other.next
    }
}

impl<T> Eq for Counter<T> {}

/// Define a scenario-scoped id newtype: a `u32`-wrapping tuple struct with
/// the standard id derives, plus the `From<u32>` impl a [`Counter`] mints
/// through. The single definition point for the engine's allocated ids
/// (`CardInstanceId`, `EnemyId`, `LocationId`, …).
///
/// ```ignore
/// define_id! {
///     /// Stable identifier for an enemy within a scenario.
///     pub struct EnemyId;
/// }
/// ```
macro_rules! define_id {
    ($(#[$meta:meta])* $vis:vis struct $Name:ident;) => {
        $(#[$meta])*
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord,
            ::serde::Serialize, ::serde::Deserialize,
        )]
        $vis struct $Name(pub u32);

        impl ::core::convert::From<u32> for $Name {
            fn from(raw: u32) -> Self {
                Self(raw)
            }
        }
    };
}
pub(crate) use define_id;

#[cfg(test)]
mod tests {
    use super::*;

    // A local id type to exercise the allocator without depending on the
    // engine's real id newtypes.
    define_id! {
        /// Test-only id.
        struct TestId;
    }

    #[test]
    fn mint_is_monotonic_and_peek_does_not_advance() {
        let mut c: Counter<TestId> = Counter::new();
        assert_eq!(c.peek(), 0);
        assert_eq!(c.mint(), TestId(0));
        assert_eq!(c.mint(), TestId(1));
        assert_eq!(c.peek(), 2, "peek reflects the next id without minting");
    }

    #[test]
    fn at_positions_the_counter_and_serializes_transparently() {
        let c: Counter<TestId> = Counter::at(7);
        assert_eq!(c.peek(), 7);
        // Transparent: a bare number, not an object.
        let json = serde_json::to_string(&c).unwrap();
        assert_eq!(json, "7");
        let back: Counter<TestId> = serde_json::from_str(&json).unwrap();
        assert_eq!(back.peek(), 7);
    }
}
