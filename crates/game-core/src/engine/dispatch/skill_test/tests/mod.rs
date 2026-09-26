use card_dsl::dsl;

use super::*;
use crate::engine::{dispatch, InputKind};
use crate::event::Event;
use crate::scenario::TokenEffect;
use crate::state::{
    EffectFrame, EnemyId, GameStateBuilder, LocationId, SkillSubstitution, SkillTestId,
};
use crate::test_support;

mod advance;
mod other;
mod substitution;
