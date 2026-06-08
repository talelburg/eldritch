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

#[cfg(test)]
mod tests {
    use super::random_game_id;

    #[test]
    fn random_ids_are_distinct() {
        assert_ne!(random_game_id(), random_game_id());
    }
}
