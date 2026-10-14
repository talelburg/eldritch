use card_dsl::dsl::{self, InvestigatorTarget};

use super::*;
use crate::engine::{dispatch, InputKind};
use crate::event::Event;
use crate::state::{GameStateBuilder, SkillSubstitution, SkillTestId};
use crate::test_support;

mod advance;
mod other;
mod substitution;
