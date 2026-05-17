//! Game state types.
//!
//! The engine's data model: top-level [`GameState`] plus the entities it
//! contains ([`Investigator`], [`Location`], [`ChaosBag`], [`Phase`]).
//! These are pure data with no engine logic — they describe the world,
//! they don't run the game.

pub mod card;
pub mod chaos_bag;
pub mod enemy;
pub mod game_state;
pub mod investigator;
pub mod location;
pub mod phase;

pub use card::{CardCode, CardInPlay, CardInstanceId, UseKind, Zone};
pub use chaos_bag::{resolve_token, ChaosBag, ChaosToken, TokenModifiers, TokenResolution};
pub use enemy::{Enemy, EnemyId};
pub use game_state::{GameState, InFlightSkillTest, PendingSkillModifier, SkillTestFollowUp};
pub use investigator::{DefeatCause, Investigator, InvestigatorId, SkillKind, Skills, Status};
pub use location::{Location, LocationId};
pub use phase::Phase;
