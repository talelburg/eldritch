//! `GameId` re-export + server-side id minting.
//!
//! The `GameId` type lives in `protocol` (it is part of the client/server
//! contract). Generation uses `uuid`, a persistence concern, so it stays
//! here rather than in the wasm-safe `protocol` crate.

pub use protocol::GameId;

/// Generate a fresh random game id (UUID v4).
#[must_use]
pub fn random_game_id() -> GameId {
    GameId::new(uuid::Uuid::new_v4().to_string())
}

/// Generate a fresh random RNG seed for a new game's setup shuffle (#467).
///
/// game-core is host-agnostic (no I/O, compiles to wasm), so entropy is the
/// host's job; we reuse `uuid`'s getrandom-backed v4 generator (already a
/// dependency for [`random_game_id`]) and take the high 64 bits of its 122
/// random bits. The fixed 4-bit version nibble sits in that high word, so the
/// seed carries ~60 uniformly-random bits — ample for per-game uniqueness
/// (collision ≈ 2⁻⁶⁰). The seed need not be recorded separately: the server
/// bakes it into the game's setup state, whose post-shuffle `RngState` is
/// persisted as the frozen seed, so replay stays deterministic.
#[must_use]
pub fn random_seed() -> u64 {
    uuid::Uuid::new_v4().as_u64_pair().0
}

#[cfg(test)]
mod tests {
    use super::{random_game_id, random_seed};

    #[test]
    fn random_ids_are_distinct() {
        assert_ne!(random_game_id(), random_game_id());
    }

    #[test]
    fn random_seeds_are_distinct() {
        // Two creations must not collide on the same setup shuffle (#467).
        assert_ne!(random_seed(), random_seed());
    }
}
