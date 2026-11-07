//! The round band down the right edge: the round number, then the round's four
//! phases as their Rules-Reference step numbers, with ◇ for each structural
//! player window.
//!
//! The entry the round stands at is lit, and so is its phase.
//!
//! The band is a few characters wide, so it shows only the numbers. Hovering
//! one shows its step's text in the hover popover, and hovering a phase's
//! letter names the phase.
//!
//! The `ROUND` outline's step labels are transcribed from the RR Appendix II
//! "Timing and Gameplay" — the Phase Sequence timing chart and Framework Event
//! Details (`data/rules-reference/rules/Appendix_II_Timing_and_Gameplay.md`),
//! with loop/flow-control tails elided for the cheat-sheet.
//! Grey boxes are framework events; red boxes are player windows.
//! Each entry carries the engine's [`RoundPosition`] for it, and is lit when
//! the game's position equals it. During the setup mulligan there is no
//! position, so nothing is lit. Display-only.

use game_core::state::{Phase, PhaseStep, RoundPosition};
use leptos::prelude::*;
use Step::{Framework, Window};

use crate::card;
use crate::popover::{Hover, Popover};
use crate::store;

/// A player window's hover text. The second sentence is the RR Appendix II
/// timing chart's own: "The red boxes are player windows. Players may use
/// \[free\] triggered abilities in these windows."
const WINDOW_TEXT: &str =
    "Player window. Players may use [free] triggered abilities in these windows.";

/// One entry in a phase's ordered outline.
enum Step {
    /// A framework event (mandatory, grey box), with its printed text.
    Framework(RoundPosition, &'static str),
    /// A structural player window (red box).
    Window(PhaseStep),
}

impl Step {
    /// The round position this entry stands for.
    const fn position(&self) -> RoundPosition {
        match self {
            Framework(position, _) => *position,
            Window(step) => RoundPosition::Window(*step),
        }
    }
}

struct PhaseOutline {
    phase: Phase,
    label: &'static str,
    steps: &'static [Step],
}

const ROUND: &[PhaseOutline] = &[
    PhaseOutline {
        phase: Phase::Mythos,
        label: "Mythos",
        steps: &[
            Framework(
                RoundPosition::MythosBegins,
                "1.1 Round begins. Mythos phase begins.",
            ),
            Framework(
                RoundPosition::PlaceDoom,
                "1.2 Place 1 doom on the current agenda.",
            ),
            Framework(
                RoundPosition::CheckDoomThreshold,
                "1.3 Check doom threshold.",
            ),
            Framework(
                RoundPosition::EncounterDraws,
                "1.4 Each investigator draws 1 encounter card.",
            ),
            Window(PhaseStep::MythosAfterDraws),
            Framework(RoundPosition::MythosEnds, "1.5 Mythos phase ends."),
        ],
    },
    PhaseOutline {
        phase: Phase::Investigation,
        label: "Investigation",
        steps: &[
            Framework(
                RoundPosition::InvestigationBegins,
                "2.1 Investigation phase begins.",
            ),
            Window(PhaseStep::InvestigationBegins),
            Framework(
                RoundPosition::TurnBegins,
                "2.2 Next investigator's turn begins.",
            ),
            Window(PhaseStep::TurnPlayerWindow),
            Framework(
                RoundPosition::TakeAction,
                "2.2.1 Active investigator may take an action, if able.",
            ),
            Framework(RoundPosition::TurnEnds, "2.2.2 Investigator's turn ends."),
            Framework(
                RoundPosition::InvestigationEnds,
                "2.3 Investigation phase ends.",
            ),
        ],
    },
    PhaseOutline {
        phase: Phase::Enemy,
        label: "Enemy",
        steps: &[
            Framework(RoundPosition::EnemyBegins, "3.1 Enemy phase begins."),
            Framework(RoundPosition::HuntersMove, "3.2 Hunter enemies move."),
            Window(PhaseStep::BeforeInvestigatorAttacked),
            Framework(
                RoundPosition::EnemyAttacks,
                "3.3 Next investigator resolves engaged enemy attacks.",
            ),
            Window(PhaseStep::AfterAllInvestigatorsAttacked),
            Framework(RoundPosition::EnemyEnds, "3.4 Enemy phase ends."),
        ],
    },
    PhaseOutline {
        phase: Phase::Upkeep,
        label: "Upkeep",
        steps: &[
            Framework(RoundPosition::UpkeepBegins, "4.1 Upkeep phase begins."),
            Window(PhaseStep::UpkeepBegins),
            Framework(RoundPosition::ResetActions, "4.2 Reset actions."),
            Framework(
                RoundPosition::ReadyExhausted,
                "4.3 Ready each exhausted card.",
            ),
            Framework(
                RoundPosition::UpkeepDraws,
                "4.4 Each investigator draws 1 card and gains 1 resource.",
            ),
            Framework(
                RoundPosition::CheckHandSize,
                "4.5 Each investigator checks hand size.",
            ),
            Framework(
                RoundPosition::UpkeepEnds,
                "4.6 Upkeep phase ends. Round ends.",
            ),
        ],
    },
];

/// The round band: "R" and the round number at its top, then each phase as
/// its letter over its step numbers, the current phase lit.
#[component]
pub fn TurnTrackerView() -> impl IntoView {
    let store = store::use_store();
    move || {
        let game = store.get().game;
        let current = game.as_ref().and_then(|g| g.round_position);
        let round = game.as_ref().map(|g| g.round);
        let phases: Vec<_> = ROUND
            .iter()
            .map(|p| {
                let steps: Vec<_> = p
                    .steps
                    .iter()
                    .map(|s| {
                        let lit = current == Some(s.position());
                        match s {
                            Framework(_, text) => {
                                let number = text.split_once(' ').map_or(*text, |(n, _)| n);
                                hover_entry("tracker-step", lit, number, text)
                            }
                            Window(_) => hover_entry("tracker-window", lit, "◇", WINDOW_TEXT),
                        }
                    })
                    .collect();
                let letter = &p.label[..1];
                let name = format!("{} phase", p.label);
                view! {
                    <div
                        class="tracker-phase"
                        class:current=current.map(RoundPosition::phase) == Some(p.phase)
                    >
                        <ul>{hover_entry("tracker-phase-label", false, letter, &name)}{steps}</ul>
                    </div>
                }
            })
            .collect();
        view! {
            <aside class="turn-tracker">
                {round.map(|r| view! { <div class="tracker-round">{format!("R{r}")}</div> })}
                {phases}
            </aside>
        }
        .into_any()
    }
}

/// One entry in the band: `shown`, lit when the round stands at it, and `text`
/// in the hover popover while the pointer is over it.
fn hover_entry(class: &'static str, lit: bool, shown: &str, text: &str) -> AnyView {
    let hover = Hover::new();
    let text = text.to_owned();
    view! {
        <li
            class={class}
            class:current={lit}
            {..hover.handlers()}
        >
            {shown.to_owned()}
            <Popover hover=hover>
                {card::render_segments(card::parse_card_text(&text))}
            </Popover>
        </li>
    }
    .into_any()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every round position the engine can stand at has exactly one entry in
    /// the band, under the phase it falls in, so lighting by equality always
    /// lights one.
    #[test]
    fn every_round_position_has_one_entry_in_its_phase() {
        for position in RoundPosition::ALL {
            let homes: Vec<Phase> = ROUND
                .iter()
                .flat_map(|p| p.steps.iter().map(move |s| (p.phase, s.position())))
                .filter(|(_, at)| *at == position)
                .map(|(phase, _)| phase)
                .collect();
            assert_eq!(homes, [position.phase()], "{position:?}");
        }
        let entries: usize = ROUND.iter().map(|p| p.steps.len()).sum();
        assert_eq!(entries, RoundPosition::ALL.len());
    }
}
