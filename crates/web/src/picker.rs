//! Pre-game investigator/scenario picker (wasm-only).
//!
//! Collects a roster and submits a `CreateGameRequest` on the `CreateTx`
//! channel; the transport creates the game.

use cards::decks::{self, DefaultDeck, WeaknessChoice};
use futures::channel::mpsc::UnboundedSender;
use leptos::prelude::*;
use protocol::CreateGameRequest;

use crate::store::{self, ConnStatus};

/// Channel the picker uses to hand a chosen `CreateGameRequest` to the
/// transport's creation loop. Provided into context by `transport::start`.
pub type CreateTx = UnboundedSender<CreateGameRequest>;

/// A radio's label: the investigator's name and code.
fn label(deck: &DefaultDeck) -> String {
    let meta =
        cards::by_code(deck.investigator).expect("a default deck's investigator is in the corpus");
    format!(" {} ({})", meta.name, deck.investigator)
}

/// Pre-game picker, rendered as a **full-viewport overlay**. Renders only while
/// `status == AwaitingRoster` — i.e. only before there is a board — and submits a
/// `CreateGameRequest` (The Gathering + the picked investigator's
/// [default deck](cards::decks::DefaultDeck)) on click. The deck brings its
/// fixed weakness unless "Random basic weakness" is checked.
#[component]
pub fn PickerView() -> impl IntoView {
    let store = store::use_store();
    let create_tx = use_context::<CreateTx>();
    // Outside the render closure, so a store update that re-renders the
    // overlay keeps the pick and the checkbox. The pick is an index into
    // `DEFAULT_DECKS`.
    let picked = RwSignal::new(0_usize);
    let weakness = RwSignal::new(WeaknessChoice::Fixed);

    view! {
        {move || {
            if store.get().status != ConnStatus::AwaitingRoster {
                return ().into_any();
            }
            let tx = create_tx.clone();
            view! {
                // A full pre-game overlay: it only renders at `AwaitingRoster`,
                // when there is no board behind it, so it shares no container
                // with in-game controls it has nothing to do with.
                <section class="picker picker-overlay">
                    <h2>"New Game"</h2>
                    <label>"Scenario: " <select><option>"The Gathering"</option></select></label>
                    <fieldset>
                        <legend>"Investigator"</legend>
                        {decks::DEFAULT_DECKS
                            .iter()
                            .enumerate()
                            .map(|(i, deck)| {
                                view! {
                                    <label>
                                        <input
                                            type="radio"
                                            name="inv"
                                            value=deck.investigator
                                            prop:checked=move || picked.get() == i
                                            on:change=move |_| picked.set(i)
                                        />
                                        {label(deck)}
                                    </label>
                                }
                            })
                            .collect_view()}
                    </fieldset>
                    <label>
                        <input
                            type="checkbox"
                            class="random-basic-weakness"
                            prop:checked=move || weakness.get() == WeaknessChoice::Random
                            on:change=move |_| {
                                weakness.update(|choice| {
                                    *choice = match choice {
                                        WeaknessChoice::Fixed => WeaknessChoice::Random,
                                        WeaknessChoice::Random => WeaknessChoice::Fixed,
                                    }
                                });
                            }
                        />
                        " Random basic weakness"
                    </label>
                    <button
                        class="create-game"
                        on:click=move |_| {
                            if let Some(tx) = tx.clone() {
                                let _ = tx.unbounded_send(CreateGameRequest {
                                    scenario_id: "the-gathering".to_string(),
                                    roster: vec![decks::DEFAULT_DECKS[picked.get_untracked()]
                                        .roster_entry(weakness.get_untracked())],
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
