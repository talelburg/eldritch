//! The ⋯ menu: a button that opens a list of entries, each a label and what
//! choosing it does, or a heading over a submenu of them.

use leptos::prelude::*;

/// One entry of a [`MoreMenu`].
#[derive(Clone)]
pub enum MenuEntry {
    /// An entry chosen for what it does.
    Action {
        /// What the entry says, e.g. "Leave".
        label: String,
        /// What choosing it does. The menu closes first.
        on_select: Callback<()>,
    },
    /// A heading over a submenu, which isn't chosen itself.
    Submenu {
        /// What the heading says.
        label: String,
        /// The submenu's entries, in order.
        children: Vec<MenuEntry>,
    },
}

impl MenuEntry {
    /// An entry saying `label` that runs `on_select` when chosen.
    pub fn action(label: impl Into<String>, on_select: impl Fn() + Send + Sync + 'static) -> Self {
        Self::Action {
            label: label.into(),
            on_select: Callback::new(move |()| on_select()),
        }
    }

    /// A heading saying `label` over the submenu `children`.
    pub fn submenu(label: impl Into<String>, children: Vec<MenuEntry>) -> Self {
        Self::Submenu {
            label: label.into(),
            children,
        }
    }
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
                    .map(|entry| entry_view(entry, open))
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

/// One entry of an open menu: a button, or a heading over its submenu's.
fn entry_view(entry: MenuEntry, open: RwSignal<bool>) -> AnyView {
    match entry {
        MenuEntry::Action { label, on_select } => view! {
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
        .into_any(),
        MenuEntry::Submenu { label, children } => {
            let items = children
                .into_iter()
                .map(|child| entry_view(child, open))
                .collect_view();
            view! {
                <div class="menu-group">
                    <div class="menu-heading">{label}</div>
                    {items}
                </div>
            }
            .into_any()
        }
    }
}
