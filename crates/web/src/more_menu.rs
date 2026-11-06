//! The ⋯ menu: a button that opens a list of entries, each a label and what
//! choosing it does.

use leptos::prelude::*;

/// One entry of a [`MoreMenu`].
#[derive(Clone)]
pub struct MenuEntry {
    /// What the entry says, e.g. "Leave".
    pub label: String,
    /// What choosing it does. The menu closes first.
    pub on_select: Callback<()>,
}

/// A ⋯ button opening `entries` below it, and a full-screen transparent
/// backdrop that closes them on a click outside. Choosing an entry closes the
/// menu and runs it. With no entries, the button doesn't render.
#[component]
pub fn MoreMenu(entries: Signal<Vec<MenuEntry>>) -> impl IntoView {
    let open = RwSignal::new(false);

    move || {
        let entries = entries.get();
        if entries.is_empty() {
            return ().into_any();
        }
        let items = move || {
            open.get().then(|| {
                let buttons = entries
                    .iter()
                    .cloned()
                    .map(|MenuEntry { label, on_select }| {
                        view! {
                            <button
                                class="menu-item"
                                on:click=move |ev| {
                                    ev.stop_propagation();
                                    open.set(false);
                                    on_select.run(());
                                }
                            >
                                {label}
                            </button>
                        }
                    })
                    .collect_view();
                view! {
                    <div
                        class="menu-backdrop"
                        on:click=move |ev| {
                            ev.stop_propagation();
                            open.set(false);
                        }
                    ></div>
                    <div class="more-menu-items">{buttons}</div>
                }
            })
        };
        view! {
            <div class="more-menu">
                <button
                    class="more-menu-toggle"
                    aria-label="More"
                    on:click=move |ev| {
                        ev.stop_propagation();
                        open.update(|open| *open = !*open);
                    }
                >
                    "⋯"
                </button>
                {items}
            </div>
        }
        .into_any()
    }
}
