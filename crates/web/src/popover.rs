//! An instant hover popover: the full text of something the board shows only
//! a header of.
//!
//! A native `title` won't do. It appears only after the browser's delay, and it
//! is plain text, so a card's ability text would lose its icons. The popover
//! renders whatever its host gives it, card text included (through
//! `card::render_segments`), the moment the pointer enters the host.
//!
//! The host owns the hover: it creates a [`Hover`], wires its own
//! `mouseenter` / `mousemove` / `mouseleave` to [`Hover::track`] and
//! [`Hover::clear`], and places a [`Popover`] inside itself. The host, not the
//! popover, is what the pointer enters, because a host can be covered by a
//! layer of its own (a map node's `.menu-hit` lies over its card). The popover
//! is `position: fixed` at the pointer, so no `overflow` ancestor clips it, and
//! it takes no pointer events, so it never steals the hover from its host.
//!
//! It needs no context, so a component mounted on its own in a test can host
//! one.

use leptos::ev::MouseEvent;
use leptos::prelude::*;

/// How far below and right of the pointer the popover's corner sits, in pixels,
/// so the pointer never covers its first line.
const OFFSET: i32 = 14;

/// One host's hover state: where the pointer is while it is over the host,
/// `None` while it isn't.
#[derive(Debug, Clone, Copy)]
pub struct Hover(RwSignal<Option<(i32, i32)>>);

impl Hover {
    /// A host the pointer isn't over.
    #[must_use]
    pub fn new() -> Self {
        Self(RwSignal::new(None))
    }

    /// Whether the pointer is over the host, so its popover is open. Reactive.
    #[must_use]
    pub fn is_open(self) -> bool {
        self.0.with(Option::is_some)
    }

    /// The pointer entered or moved over the host: open the popover beside it.
    pub fn track(self, ev: &MouseEvent) {
        self.0.set(Some((ev.client_x(), ev.client_y())));
    }

    /// The pointer left the host: close the popover.
    pub fn clear(self) {
        self.0.set(None);
    }
}

impl Default for Hover {
    fn default() -> Self {
        Self::new()
    }
}

/// The popover itself: `children` in a `.popover` box beside the pointer while
/// `hover` is open, and nothing at all otherwise. Rendering nothing while closed
/// keeps the hidden text out of the host's `textContent` and out of its layout.
#[component]
pub fn Popover(hover: Hover, children: ChildrenFn) -> impl IntoView {
    move || {
        hover.0.get().map(|(x, y)| {
            let style = format!("left:{}px;top:{}px;", x + OFFSET, y + OFFSET);
            view! { <div class="popover" role="tooltip" style=style>{children()}</div> }
        })
    }
}
