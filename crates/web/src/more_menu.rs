//! The ⋯ menu: a button that opens a list of entries, each an action, or a
//! heading over a submenu of them.
//!
//! The board, the lobby header and a "Your games" row each build their own
//! entries; the menu only renders them and runs the one picked.

use leptos::prelude::*;

/// One entry of a [`MoreMenu`].
#[derive(Clone)]
pub struct MenuEntry {
    /// What the entry says.
    pub label: String,
    /// What picking it does. An entry with [`children`](Self::children) is a
    /// heading, and picking one of them runs theirs instead.
    pub on_select: Callback<()>,
    /// The submenu under this entry, in order: empty for an entry that is an
    /// action itself.
    pub children: Vec<MenuEntry>,
}

impl MenuEntry {
    /// An entry saying `label` that runs `on_select` when picked.
    pub fn new(label: impl Into<String>, on_select: impl Fn() + Send + Sync + 'static) -> Self {
        Self {
            label: label.into(),
            on_select: Callback::new(move |()| on_select()),
            children: Vec::new(),
        }
    }

    /// A heading saying `label` over the submenu `children`.
    pub fn submenu(label: impl Into<String>, children: Vec<MenuEntry>) -> Self {
        Self {
            label: label.into(),
            on_select: Callback::new(|()| {}),
            children,
        }
    }
}

/// The ⋯ button and, once it is clicked, its entries. With no entries there
/// is nothing to offer, so nothing renders. Picking an entry runs it and
/// closes the menu, and so does a click anywhere outside it.
#[component]
pub fn MoreMenu(entries: Signal<Vec<MenuEntry>>) -> impl IntoView {
    let open = RwSignal::new(false);
    move || {
        let entries = entries.get();
        if entries.is_empty() {
            return ().into_any();
        }
        let list = open.get().then(|| {
            let items: Vec<_> = entries
                .into_iter()
                .map(|entry| entry_view(entry, open))
                .collect();
            view! {
                <div class="menu-backdrop" on:click=move |_| open.set(false)></div>
                <div class="more-menu-entries">{items}</div>
            }
        });
        view! {
            <div class="more-menu">
                <button class="more-menu-toggle" title="More" on:click=move |_| open.update(|o| *o = !*o)>
                    "⋯"
                </button>
                {list}
            </div>
        }
        .into_any()
    }
}

/// One entry of an open menu: a button, or a heading over its submenu's.
fn entry_view(entry: MenuEntry, open: RwSignal<bool>) -> AnyView {
    let MenuEntry {
        label,
        on_select,
        children,
    } = entry;
    if children.is_empty() {
        return view! {
            <button
                class="menu-item"
                on:click=move |_| {
                    open.set(false);
                    on_select.run(());
                }
            >
                {label}
            </button>
        }
        .into_any();
    }
    let items: Vec<_> = children
        .into_iter()
        .map(|child| entry_view(child, open))
        .collect();
    view! {
        <div class="menu-group">
            <div class="menu-heading">{label}</div>
            {items}
        </div>
    }
    .into_any()
}
