//! The round band down the right edge: the round number, then the round's four
//! phases as their Rules-Reference step numbers, with ◇ for each structural
//! player window, and the current phase lit.
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
//! The engine exposes only the coarse phase, so only the current *phase* is
//! lit. Display-only.

use game_core::state::Phase;
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

/// The round band: "R" and the round number at its top, then each phase as
/// its letter over its step numbers, the current phase lit.
#[component]
pub fn TurnTrackerView() -> impl IntoView {
    let store = store::use_store();
    move || {
        let game = store.get().game;
        let current = game.as_ref().map(|g| g.phase);
        let round = game.as_ref().map(|g| g.round);
        let phases: Vec<_> = ROUND
            .iter()
            .map(|p| {
                let steps: Vec<_> = p
                    .steps
                    .iter()
                    .map(|s| match s {
                        Framework(text) => {
                            let number = text.split_once(' ').map_or(*text, |(n, _)| n);
                            hover_entry("tracker-step", number, text)
                        }
                        Window => hover_entry("tracker-window", "◇", WINDOW_TEXT),
                    })
                    .collect();
                let letter = &p.label[..1];
                let name = format!("{} phase", p.label);
                view! {
                    <div class="tracker-phase" class:current=current == Some(p.phase)>
                        <ul>{hover_entry("tracker-phase-label", letter, &name)}{steps}</ul>
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

/// One entry in the band: `shown`, and `text` in the hover popover while the
/// pointer is over it.
fn hover_entry(class: &'static str, shown: &str, text: &str) -> AnyView {
    let hover = Hover::new();
    let text = text.to_owned();
    view! {
        <li
            class=class
            on:mouseenter=move |ev| hover.track(&ev)
            on:mousemove=move |ev| hover.track(&ev)
            on:mouseleave=move |_| hover.clear()
        >
            {shown.to_owned()}
            <Popover hover=hover>
                {card::render_segments(card::parse_card_text(&text))}
            </Popover>
        </li>
    }
    .into_any()
}
