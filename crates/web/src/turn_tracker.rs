//! Right-hand turn tracker: the round's four phases with their Rules-Reference
//! sub-steps and structural player windows, highlighting the current phase.
//!
//! The `ROUND` outline is transcribed verbatim from the FFG Rules Reference,
//! Appendix II "Timing and Gameplay" — the Phase Sequence timing chart and
//! Framework Event Details (`data/rules-reference/ahc01_rules_reference_web.pdf`,
//! pp. 23-25). Grey boxes are framework events; red boxes are player windows.
//! The engine exposes only the coarse phase, so only the current *phase* is
//! highlighted. Display-only.

use game_core::state::Phase;
use leptos::prelude::*;

use crate::store::use_store;

/// One entry in a phase's ordered outline.
enum Step {
    /// A framework event (mandatory, grey box).
    Framework(&'static str),
    /// A structural player window (red box).
    Window,
}

struct PhaseOutline {
    phase: Phase,
    label: &'static str,
    steps: &'static [Step],
}

use Step::{Framework, Window};

const ROUND: &[PhaseOutline] = &[
    PhaseOutline {
        phase: Phase::Mythos,
        label: "Mythos",
        steps: &[
            Framework("1.1 Round begins. Mythos phase begins."),
            Framework("1.2 Place 1 doom on the current agenda."),
            Framework("1.3 Check doom threshold."),
            Framework("1.4 Each investigator draws 1 encounter card."),
            Window,
            Framework("1.5 Mythos phase ends."),
        ],
    },
    PhaseOutline {
        phase: Phase::Investigation,
        label: "Investigation",
        steps: &[
            Framework("2.1 Investigation phase begins."),
            Window,
            Framework("2.2 Next investigator's turn begins."),
            Window,
            Framework("2.2.1 Active investigator may take an action, if able."),
            Framework("2.2.2 Investigator's turn ends."),
            Framework("2.3 Investigation phase ends."),
        ],
    },
    PhaseOutline {
        phase: Phase::Enemy,
        label: "Enemy",
        steps: &[
            Framework("3.1 Enemy phase begins."),
            Framework("3.2 Hunter enemies move."),
            Window,
            Framework("3.3 Next investigator resolves engaged enemy attacks."),
            Window,
            Framework("3.4 Enemy phase ends."),
        ],
    },
    PhaseOutline {
        phase: Phase::Upkeep,
        label: "Upkeep",
        steps: &[
            Framework("4.1 Upkeep phase begins."),
            Window,
            Framework("4.2 Reset actions."),
            Framework("4.3 Ready each exhausted card."),
            Framework("4.4 Each investigator draws 1 card and gains 1 resource."),
            Framework("4.5 Each investigator checks hand size."),
            Framework("4.6 Upkeep phase ends. Round ends."),
        ],
    },
];

#[component]
pub fn TurnTrackerView() -> impl IntoView {
    let store = use_store();
    move || {
        let game = store.get().game;
        let current = game.as_ref().map(|g| g.phase);
        let round = game.as_ref().map(|g| g.round);
        let phases: Vec<_> = ROUND
            .iter()
            .map(|p| {
                let cls = if current == Some(p.phase) {
                    "tracker-phase current"
                } else {
                    "tracker-phase"
                };
                let steps: Vec<_> = p
                    .steps
                    .iter()
                    .map(|s| match s {
                        Step::Framework(t) => {
                            view! { <li class="tracker-step">{*t}</li> }.into_any()
                        }
                        Step::Window => {
                            view! { <li class="tracker-window">"player window"</li> }.into_any()
                        }
                    })
                    .collect();
                view! {
                    <div class=cls>
                        <div class="tracker-phase-label">{p.label}</div>
                        <ul>{steps}</ul>
                    </div>
                }
            })
            .collect();
        view! {
            <aside class="turn-tracker">
                <h2>"Turn"</h2>
                {round.map(|r| view! { <div class="tracker-round">{format!("Round {r}")}</div> })}
                {phases}
            </aside>
        }
        .into_any()
    }
}
