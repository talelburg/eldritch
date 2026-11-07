//! Game phases.

use serde::{Deserialize, Serialize};

use crate::state::continuation::PhaseStep;

/// One of the four phases in an Arkham Horror round.
///
/// Phases cycle in order: [`Mythos`] → [`Investigation`] → [`Enemy`] → [`Upkeep`] → [`Mythos`]…
///
/// Deliberately NOT `#[non_exhaustive]`: the four phases are fixed by FFG's
/// rules, not an open extension point. Scenario-specific timing
/// (interludes, special encounter windows) is modeled with special rules,
/// not by adding new phase variants.
///
/// [`Mythos`]: Phase::Mythos
/// [`Investigation`]: Phase::Investigation
/// [`Enemy`]: Phase::Enemy
/// [`Upkeep`]: Phase::Upkeep
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Phase {
    /// Encounter cards drawn, doom advances.
    Mythos,
    /// Investigators take turns, spending action points.
    Investigation,
    /// Engaged enemies attack; investigators may resolve hunter movement.
    Enemy,
    /// Cards ready, hand size adjusted, agenda/act effects.
    Upkeep,
}

impl Phase {
    /// The phase that follows this one in normal round order.
    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Self::Mythos => Self::Investigation,
            Self::Investigation => Self::Enemy,
            Self::Enemy => Self::Upkeep,
            Self::Upkeep => Self::Mythos,
        }
    }
}

/// Where the round stands: one of Appendix II's numbered steps, or one of its
/// structural player windows. See **Round position** in `GLOSSARY.md`.
///
/// `Appendix_II_Timing_and_Gameplay.md`: *"The "Phase Sequence timing chart"
/// depicts the phases and steps of a game round."* and *"Numbered items
/// presented in the grey boxes are known as framework events."* One variant per
/// box: the twenty grey ones by number, and the six red ones (*"The red boxes
/// are player windows"*) as [`Window`](Self::Window).
///
/// The phase drivers set it as they enter each step or window, and it holds
/// until the next one is entered, so whatever pauses inside a step (a reaction
/// window, a skill test, an agenda's reverse, a tie) reads as that step. Stored
/// rather than derived from the continuation stack: a phase anchor's resume
/// names what runs *next*, not where play is. Display-only; no rule reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum RoundPosition {
    /// 1.1 Round begins. Mythos phase begins.
    MythosBegins,
    /// 1.2 Place 1 doom on the current agenda.
    PlaceDoom,
    /// 1.3 Check doom threshold.
    CheckDoomThreshold,
    /// 1.4 Each investigator draws 1 encounter card.
    EncounterDraws,
    /// 1.5 Mythos phase ends.
    MythosEnds,
    /// 2.1 Investigation phase begins.
    InvestigationBegins,
    /// 2.2 Next investigator's turn begins.
    TurnBegins,
    /// 2.2.1 Active investigator may take an action, if able.
    TakeAction,
    /// 2.2.2 Investigator's turn ends.
    TurnEnds,
    /// 2.3 Investigation phase ends.
    InvestigationEnds,
    /// 3.1 Enemy phase begins.
    EnemyBegins,
    /// 3.2 Hunter enemies move.
    HuntersMove,
    /// 3.3 Next investigator resolves engaged enemy attacks.
    EnemyAttacks,
    /// 3.4 Enemy phase ends.
    EnemyEnds,
    /// 4.1 Upkeep phase begins.
    UpkeepBegins,
    /// 4.2 Reset actions.
    ResetActions,
    /// 4.3 Ready each exhausted card.
    ReadyExhausted,
    /// 4.4 Each investigator draws 1 card and gains 1 resource.
    UpkeepDraws,
    /// 4.5 Each investigator checks hand size.
    CheckHandSize,
    /// 4.6 Upkeep phase ends. Round ends.
    UpkeepEnds,
    /// One of the chart's player windows, named by the framework window the
    /// engine opens there.
    Window(PhaseStep),
}

impl RoundPosition {
    /// Every position, in the chart's order.
    pub const ALL: [Self; 26] = [
        Self::MythosBegins,
        Self::PlaceDoom,
        Self::CheckDoomThreshold,
        Self::EncounterDraws,
        Self::Window(PhaseStep::MythosAfterDraws),
        Self::MythosEnds,
        Self::InvestigationBegins,
        Self::Window(PhaseStep::InvestigationBegins),
        Self::TurnBegins,
        Self::Window(PhaseStep::TurnPlayerWindow),
        Self::TakeAction,
        Self::TurnEnds,
        Self::InvestigationEnds,
        Self::EnemyBegins,
        Self::HuntersMove,
        Self::Window(PhaseStep::BeforeInvestigatorAttacked),
        Self::EnemyAttacks,
        Self::Window(PhaseStep::AfterAllInvestigatorsAttacked),
        Self::EnemyEnds,
        Self::UpkeepBegins,
        Self::Window(PhaseStep::UpkeepBegins),
        Self::ResetActions,
        Self::ReadyExhausted,
        Self::UpkeepDraws,
        Self::CheckHandSize,
        Self::UpkeepEnds,
    ];

    /// The phase this position falls in.
    #[must_use]
    pub const fn phase(self) -> Phase {
        match self {
            Self::MythosBegins
            | Self::PlaceDoom
            | Self::CheckDoomThreshold
            | Self::EncounterDraws
            | Self::MythosEnds
            | Self::Window(PhaseStep::MythosAfterDraws) => Phase::Mythos,
            Self::InvestigationBegins
            | Self::TurnBegins
            | Self::TakeAction
            | Self::TurnEnds
            | Self::InvestigationEnds
            | Self::Window(PhaseStep::InvestigationBegins | PhaseStep::TurnPlayerWindow) => {
                Phase::Investigation
            }
            Self::EnemyBegins
            | Self::HuntersMove
            | Self::EnemyAttacks
            | Self::EnemyEnds
            | Self::Window(
                PhaseStep::BeforeInvestigatorAttacked | PhaseStep::AfterAllInvestigatorsAttacked,
            ) => Phase::Enemy,
            Self::UpkeepBegins
            | Self::ResetActions
            | Self::ReadyExhausted
            | Self::UpkeepDraws
            | Self::CheckHandSize
            | Self::UpkeepEnds
            | Self::Window(PhaseStep::UpkeepBegins) => Phase::Upkeep,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_position_is_listed_once_in_its_phases_run() {
        // `phase` is an exhaustive match, so a new variant must be placed in a
        // phase; this pins that `ALL` lists each once, grouped by phase in round
        // order.
        let mut seen = std::collections::HashSet::new();
        assert!(RoundPosition::ALL.iter().all(|p| seen.insert(*p)));
        let phases: Vec<Phase> = RoundPosition::ALL.iter().map(|p| p.phase()).collect();
        let mut runs = phases.clone();
        runs.dedup();
        assert_eq!(
            runs,
            [
                Phase::Mythos,
                Phase::Investigation,
                Phase::Enemy,
                Phase::Upkeep
            ]
        );
    }

    #[test]
    fn phases_cycle_in_round_order() {
        let mut p = Phase::Mythos;
        for expected in [
            Phase::Investigation,
            Phase::Enemy,
            Phase::Upkeep,
            Phase::Mythos,
        ] {
            p = p.next();
            assert_eq!(p, expected);
        }
    }
}
