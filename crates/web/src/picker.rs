//! Pre-game investigator/scenario picker (wasm-only). Collects a roster and
//! submits a `CreateGameRequest` on the `CreateTx` channel; the transport
//! creates the game (#459).

use futures::channel::mpsc;
use game_core::action::RosterEntry;
use game_core::state::CardCode;
use leptos::prelude::*;
use protocol::CreateGameRequest;

use crate::store::{use_store, ConnStatus};

/// Channel the picker uses to hand a chosen `CreateGameRequest` to the
/// transport's creation loop. Provided into context by `transport::start`.
pub type CreateTx = mpsc::UnboundedSender<CreateGameRequest>;

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

/// Build the default Roland roster: investigator 01001 + the placeholder deck.
pub fn roland_roster() -> Vec<RosterEntry> {
    vec![RosterEntry {
        investigator: CardCode::new("01001"),
        deck: ROLAND_DEFAULT_DECK.iter().map(|c| CardCode::new(*c)).collect(),
    }]
}

/// Pre-game picker. Renders only while `status == AwaitingRoster`. Submits a
/// `CreateGameRequest` (The Gathering + Roland) on click.
#[component]
pub fn PickerView() -> impl IntoView {
    let store = use_store();
    let create_tx = use_context::<CreateTx>();

    view! {
        {move || {
            if store.get().status != ConnStatus::AwaitingRoster {
                return ().into_any();
            }
            let tx = create_tx.clone();
            view! {
                <section class="picker">
                    <h2>"New Game"</h2>
                    <label>"Scenario: " <select><option>"The Gathering"</option></select></label>
                    <fieldset>
                        <legend>"Investigator"</legend>
                        <label><input type="radio" name="inv" checked=true/> "Roland Banks (01001)"</label>
                    </fieldset>
                    <button
                        class="create-game"
                        on:click=move |_| {
                            if let Some(tx) = tx.clone() {
                                let _ = tx.unbounded_send(CreateGameRequest {
                                    scenario_id: "the-gathering".to_string(),
                                    roster: roland_roster(),
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
