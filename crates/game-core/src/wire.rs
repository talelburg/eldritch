//! Serde helpers for the wire format the state and the prompts persist in.

use serde::{Deserialize, Deserializer};

/// Read an `Option` field that must be present, `null` for `None`.
///
/// serde's derive reads a missing `Option` field as `None` without being
/// asked. Naming this with `#[serde(deserialize_with = "…")]` turns that off,
/// so a payload written before the field existed is refused rather than read
/// as `None`: a persisted game from before a wire change is discarded, not
/// migrated (ADR 0010).
pub(crate) fn required_option<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::deserialize(deserializer)
}
