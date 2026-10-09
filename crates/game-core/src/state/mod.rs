//! Game state types.
//!
//! The engine's data model: top-level [`GameState`] plus the entities it
//! contains ([`Investigator`], [`Location`], [`ChaosBag`], [`Phase`]).
//! These are pure data with no engine logic — they describe the world,
//! they don't run the game.

pub mod ability_source;
pub mod builder;
pub mod card;
pub mod chaos_bag;
pub mod continuation;
pub mod counter;
pub mod enemy;
pub mod game_state;
pub mod investigator;
pub mod location;
pub mod phase;

pub use ability_source::{AbilityAddress, AbilitySource};
pub use builder::GameStateBuilder;
pub use card::{
    AbilityUsageRecord, CardCode, CardInPlay, CardInstanceId, DiscardPile, UseKind, Zone,
};
pub use card_dsl::card_data::{SkillKind, Skills};
pub use chaos_bag::{resolve_token, ChaosBag, ChaosToken, TokenModifiers, TokenResolution};
pub use continuation::{
    AcknowledgeForcedFrame, ActionResolutionFrame, ActionResume, AdvanceDeck, AdvanceReverseFrame,
    AdvanceStep, AdvanceTrigger, AssetEntry, Assignment, AttackLoopFrame, AttackLoopStage,
    CandidateSource, Continuation, ContinuationStack, DamageSource, DealDamageFrame,
    DealDamageStep, EffectFrame, EliminationFrame, EliminationStep, EmitEventFrame, EmitStep,
    EncounterCardFrame, EncounterDisposition, EncounterDrawFrame, EnemyAttackSource,
    EnemyPhaseFrame, EnemyResume, FastActorScope, FastWindowFrame, FastWindowKind, Frame,
    FrameActivity, FrameProfile, HandSizeDiscard, HunterChoice, InFlightSkillTest,
    InvestigationPhaseFrame, InvestigationResume, InvestigatorTurnFrame, MoveEnterFrame,
    MulliganFrame, MythosPhaseFrame, MythosResume, PhaseStep, PlayFromHandFrame, PlayerDrawFrame,
    ResolutionCandidate, ResolvedTest, ScenarioEndDisposition, ScenarioEndFrame, ScenarioEndStep,
    SkillTestFollowUp, SkillTestStep, SlotDiscardFrame, SpawnEngagePending,
    SubstitutionPromptFrame, TimingMode, TimingPointFrame, TimingPointWindowFrame, TimingSub,
    UpkeepPhaseFrame, UpkeepResume,
};
pub use counter::Counter;
// `define_id!` is used by the id submodules; kept crate-internal.
pub(crate) use counter::define_id;
pub use enemy::{Enemy, EnemyId};
pub use game_state::{
    Act, Agenda, DifficultyBasis, GameState, Lifetime, ModifierTarget, RecordedModifier,
    RecordedModifierKind, SkillSubstitution, SkillTestId,
};
pub use investigator::{EliminationCause, Investigator, InvestigatorId, Status};
pub use location::{Location, LocationId};
pub use phase::Phase;
