//! `GameId`: the server-side identifier for a persisted game session.

use serde::{Deserialize, Serialize};

/// Stable identifier for a persisted game — it names a row in the
/// `games` table. A server/persistence concept, distinct from
/// game-core's domain ids (`ScenarioId`, `InvestigatorId`, …), so it
/// lives in the host crate rather than the kernel.
///
/// Transparent over [`String`]: it serializes as a bare JSON string,
/// binds directly to a `SQLite` TEXT column, and extracts straight from
/// a URL path segment.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct GameId(String);

impl GameId {
    /// Wrap an existing id string.
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    /// Generate a fresh random id (UUID v4).
    #[must_use]
    pub fn random() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    /// Borrow the underlying string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for GameId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for GameId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl From<String> for GameId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

#[cfg(test)]
mod tests {
    use super::GameId;

    #[test]
    fn random_ids_are_distinct() {
        assert_ne!(GameId::random(), GameId::random());
    }

    #[test]
    fn serializes_as_a_bare_string() {
        let id = GameId::new("abc");
        assert_eq!(serde_json::to_string(&id).unwrap(), "\"abc\"");
        let back: GameId = serde_json::from_str("\"abc\"").unwrap();
        assert_eq!(back, id);
    }
}
