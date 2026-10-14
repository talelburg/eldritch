//! Pre-game investigator/scenario picker (wasm-only). Collects a roster and
//! submits a `CreateGameRequest` on the `CreateTx` channel; the transport
//! creates the game (#459).

use futures::channel::mpsc::UnboundedSender;
use game_core::action::RosterEntry;
use game_core::state::CardCode;
use leptos::prelude::*;
use protocol::CreateGameRequest;

use crate::store::{self, ConnStatus};

/// Channel the picker uses to hand a chosen `CreateGameRequest` to the
/// transport's creation loop. Provided into context by `transport::start`.
pub type CreateTx = UnboundedSender<CreateGameRequest>;

/// Placeholder default deck for Roland (01001) until Phase 9 decklist import.
/// Implemented Guardian/Seeker/neutral cards only, so the opening hand is
/// playable. NOT a legal 30+1 deck — a scaffold for UI testing.
pub const ROLAND_DEFAULT_DECK: &[&str] = &[
    "01006", // .38 Special (signature)
    "01020", // Machete
    "01018", // Beat Cop
    "01021", // Guard Dog
    "01019", // First Aid
    "01024", // Dynamite Blast
    "01022", // Evidence!
    "01023", // Dodge
    "01025", // Vicious Blow
    "01030", // Magnifying Glass
    "01039", // Deduction
    "01037", // Working a Hunch
    "01089", // Guts
    "01090", // Perception
    "01091", // Overpower
    "01092", // Manual Dexterity
    "01093", // Unexpected Courage
    "01007", // Cover Up (signature weakness)
];

/// Placeholder default deck for Agnes Baker (01004) until decklist import
/// (#782) retires both scaffolds. Implemented cards she may take only, so every
/// draw is playable; her signatures and the rest of the Mystic pool join as
/// each lands (#777). NOT a legal deck — a scaffold for UI testing, like
/// [`ROLAND_DEFAULT_DECK`].
pub const AGNES_DEFAULT_DECK: &[&str] = &[
    "01059", "01059", // Holy Rosary
    "01061", "01061", // Scrying
    "01062", "01062", // Arcane Studies
    "01064", "01064", // Drawn to the Flame
    "01067", "01067", // Fearless
    "01086", "01086", // Knife
    "01087", "01087", // Flashlight
    "01088", "01088", // Emergency Cache
    "01089", "01089", // Guts
    "01090", "01090", // Perception
    "01091", "01091", // Overpower
    "01092", "01092", // Manual Dexterity
    "01093", "01093", // Unexpected Courage
];

/// The investigators the picker offers, each with its placeholder deck.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pick {
    Roland,
    Agnes,
}

impl Pick {
    fn roster(self) -> Vec<RosterEntry> {
        match self {
            Pick::Roland => roland_roster(),
            Pick::Agnes => agnes_roster(),
        }
    }
}

/// A one-investigator roster of `investigator` with `deck`.
fn roster(investigator: &str, deck: &[&str]) -> Vec<RosterEntry> {
    vec![RosterEntry {
        investigator: CardCode::new(investigator),
        deck: deck.iter().map(|c| CardCode::new(*c)).collect(),
    }]
}

/// Build the default Roland roster: investigator 01001 + the placeholder deck.
pub fn roland_roster() -> Vec<RosterEntry> {
    roster("01001", ROLAND_DEFAULT_DECK)
}

/// Build the default Agnes roster: investigator 01004 + the placeholder deck.
pub fn agnes_roster() -> Vec<RosterEntry> {
    roster("01004", AGNES_DEFAULT_DECK)
}

/// Pre-game picker, rendered as a **full-viewport overlay**. Renders only while
/// `status == AwaitingRoster` — i.e. only before there is a board — and submits a
/// `CreateGameRequest` (The Gathering + the picked investigator) on click.
#[component]
pub fn PickerView() -> impl IntoView {
    let store = store::use_store();
    let create_tx = use_context::<CreateTx>();
    // Outside the render closure, so a store update that re-renders the
    // overlay keeps the pick.
    let picked = RwSignal::new(Pick::Roland);

    view! {
        {move || {
            if store.get().status != ConnStatus::AwaitingRoster {
                return ().into_any();
            }
            let tx = create_tx.clone();
            view! {
                // A full pre-game overlay: it only renders at `AwaitingRoster`,
                // when there is no board behind it, so it no longer shares a
                // container with in-game controls it has nothing to do with (#541).
                <section class="picker picker-overlay">
                    <h2>"New Game"</h2>
                    <label>"Scenario: " <select><option>"The Gathering"</option></select></label>
                    <fieldset>
                        <legend>"Investigator"</legend>
                        <label>
                            <input
                                type="radio"
                                name="inv"
                                value="01001"
                                prop:checked=move || picked.get() == Pick::Roland
                                on:change=move |_| picked.set(Pick::Roland)
                            />
                            " Roland Banks (01001)"
                        </label>
                        <label>
                            <input
                                type="radio"
                                name="inv"
                                value="01004"
                                prop:checked=move || picked.get() == Pick::Agnes
                                on:change=move |_| picked.set(Pick::Agnes)
                            />
                            " Agnes Baker (01004)"
                        </label>
                    </fieldset>
                    <button
                        class="create-game"
                        on:click=move |_| {
                            if let Some(tx) = tx.clone() {
                                let _ = tx.unbounded_send(CreateGameRequest {
                                    scenario_id: "the-gathering".to_string(),
                                    roster: picked.get_untracked().roster(),
                                });
                            }
                        }
                    >
                        "Create game"
                    </button>
                </section>
            }
            .into_any()
        }}
    }
}
