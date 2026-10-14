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

pub use crate::scaffold_decks::{AGNES_DEFAULT_DECK, ROLAND_DEFAULT_DECK};

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
