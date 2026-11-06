//! Deleting a game: the creator's Delete entry in each ⋯ menu, and the
//! confirmation it opens, which asks before anything is deleted.
//!
//! The entry only opens the confirmation. Confirming runs the [`DeleteGame`]
//! the page provides through context: the wasm build's sends
//! `DELETE /games/{id}`, and a test's records the game.

use leptos::prelude::*;
use protocol::GameId;

use crate::more_menu::MenuEntry;
use crate::store::{self, StoreSignal};

/// What the confirmation asks.
const QUESTION: &str = "Delete for everyone? This can't be undone.";

/// What confirming a delete does with the game, provided through context.
#[derive(Clone, Copy)]
pub struct DeleteGame(pub Callback<GameId>);

/// The ⋯ entry that asks to delete `game_id`, opening the confirmation.
pub fn delete_entry(store: StoreSignal, game_id: GameId) -> MenuEntry {
    MenuEntry {
        label: "Delete".to_string(),
        on_select: Callback::new(move |()| {
            let game_id = game_id.clone();
            store.update(|s| s.delete_asked = Some(game_id));
        }),
        children: Vec::new(),
    }
}

/// The confirmation a Delete entry opens, while the store's
/// [`delete_asked`](crate::store::ClientState::delete_asked) names a game.
/// Delete hands that game to the [`DeleteGame`] in context; Cancel deletes
/// nothing. Either closes it.
#[component]
pub fn DeleteDialog() -> impl IntoView {
    let store = store::use_store();
    let delete = use_context::<DeleteGame>();
    let asked = Memo::new(move |_| store.with(|s| s.delete_asked.clone()));
    let close = move || store.update(|s| s.delete_asked = None);

    move || {
        asked.get().map(|game_id| {
            view! {
                <div class="dialog-backdrop"></div>
                <div class="confirm-dialog" role="dialog" aria-modal="true">
                    <p class="confirm-question">{QUESTION}</p>
                    <button
                        class="confirm-delete"
                        on:click=move |_| {
                            close();
                            if let Some(DeleteGame(delete)) = delete {
                                delete.run(game_id.clone());
                            }
                        }
                    >
                        "Delete"
                    </button>
                    <button class="confirm-cancel" on:click=move |_| close()>
                        "Cancel"
                    </button>
                </div>
            }
        })
    }
}
