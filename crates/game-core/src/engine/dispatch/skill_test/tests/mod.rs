use card_dsl::dsl;

use super::*;
use crate::engine::{dispatch, InputKind};
use crate::event::Event;
use crate::scenario::TokenEffect;
use crate::state::{GameStateBuilder, SkillSubstitution, SkillTestId};
use crate::test_support;

mod advance;
mod other;
mod substitution;
