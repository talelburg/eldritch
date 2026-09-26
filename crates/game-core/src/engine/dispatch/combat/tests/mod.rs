use std::collections::BTreeMap;

use super::*;
use crate::action::InputResponse;
use crate::engine::dispatch::emit::{ConditionResolution, TimingEvent};
use crate::engine::outcome::{EngineOutcome, OptionId};
use crate::engine::{dispatch, Cx};
use crate::event::Event;
use crate::state::{
    Assignment, AttackLoopStage, CardCode, CardInstanceId, Continuation, EnemyAttackSource,
    EnemyId, EnemyResume, GameStateBuilder, InvestigatorId,
};
use crate::{assert_event, assert_no_event, test_support};

mod attack_loop;
mod damage_assignment;
mod deal_damage;
mod enemy_defeat;
