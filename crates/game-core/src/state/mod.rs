//! Game state types.
//!
//! The engine's data model: top-level [`GameState`] plus the entities it
//! contains ([`Investigator`], [`Location`], [`ChaosBag`], [`Phase`]).
//! These are pure data with no engine logic — they describe the world,
//! they don't run the game.

pub mod builder;
pub mod card;
pub mod chaos_bag;
pub mod counter;
pub mod enemy;
pub mod game_state;
pub mod investigator;
pub mod location;
pub mod phase;

pub use builder::GameStateBuilder;
pub use card::{AbilityUsageRecord, CardCode, CardInPlay, CardInstanceId, UseKind, Zone};
pub use card_dsl::card_data::{SkillKind, Skills};
pub use chaos_bag::{resolve_token, ChaosBag, ChaosToken, TokenModifiers, TokenResolution};
pub use counter::Counter;
// `define_id!` is used by the id submodules; kept crate-internal.
pub(crate) use counter::define_id;
pub use enemy::{Enemy, EnemyId};
pub use game_state::{
    Act, ActRoundEndPending, Agenda, AttackLoopPhase, CandidateSource, ChoiceFrame, Continuation,
    EnemyAttackSource, FastActorScope, FinishContinuation, ForcedContinuation, GameState,
    HandSizeDiscard, HunterChoice, InFlightSkillTest, PendingEnemyAttack, PendingSkillModifier,
    PhaseStep, ResolutionCandidate, ResolutionFrame, ResolutionKind, RoundEndAdvance,
    SkillSubstitution, SkillTestFollowUp, SpawnEngagePending, WindowBinding, WindowKind,
};
pub use investigator::{DefeatCause, Investigator, InvestigatorId, Status};
pub use location::{Location, LocationId};
pub use phase::Phase;
