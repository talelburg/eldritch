//! An instant hover popover: the full text of something the board shows only
//! a header of.
//!
//! A native `title` won't do. It appears only after the browser's delay, and it
//! is plain text, so a card's ability text would lose its icons. The popover
//! renders whatever its host gives it, card text included (through
//! `card::render_segments`), the moment the pointer enters the host.
//!
//! The host owns the hover: it creates a [`Hover`], spreads
//! [`Hover::handlers`] onto itself, and places a [`Popover`] inside itself. The host, not the
//! popover, is what the pointer enters, because a host can be covered by a
//! layer of its own (a map node's `.menu-hit` lies over its card). The popover
//! is `position: fixed` at the pointer, so no `overflow` ancestor clips it, and
//! it takes no pointer events, so it never steals the hover from its host. It
//! opens away from the window's nearer edges, so it stays on screen wherever
//! its host sits: above the pointer in the hand along the bottom, left of it
//! in the round band down the right.
//!
//! It needs no context, so a component mounted on its own in a test can host
//! one.

use leptos::attr::Attribute;
use leptos::ev::{self, MouseEvent};
use leptos::prelude::*;

/// How far from the pointer the popover's nearest corner sits, in pixels, so
/// the pointer never covers its first line.
const OFFSET: i32 = 14;

/// Where a popover opens: the pointer, and which way from it. Anything else
/// opened beside the pointer can open the same way, from the click that
/// opened it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Spot {
    x: i32,
    y: i32,
    /// The pointer is in the window's right half, so the popover opens to its
    /// left.
    pub(crate) left: bool,
    /// The pointer is in the window's bottom half, so the popover opens above
    /// it.
    pub(crate) up: bool,
}

impl Spot {
    /// The spot `ev`'s pointer marks in the window.
    pub(crate) fn of(ev: &MouseEvent) -> Self {
        let (x, y) = (ev.client_x(), ev.client_y());
        let window = window();
        // An unreadable size opens the popover below and right.
        let half = |size: Option<f64>| size.map_or(f64::MAX, |v| v / 2.0);
        Self {
            x,
            y,
            left: f64::from(x) > half(window.inner_width().ok().and_then(|v| v.as_f64())),
            up: f64::from(y) > half(window.inner_height().ok().and_then(|v| v.as_f64())),
        }
    }

    /// The popover's inline position: its corner nearest the pointer,
    /// [`OFFSET`] away from it, with `style.css`'s `opens-left` / `opens-up`
    /// moving the box itself to that corner's far side.
    pub(crate) fn style(self) -> String {
        let away = |at: i32, back: bool| if back { at - OFFSET } else { at + OFFSET };
        format!(
            "left:{}px;top:{}px;",
            away(self.x, self.left),
            away(self.y, self.up)
        )
    }
}

/// One host's hover state: where the pointer is while it is over the host,
/// `None` while it isn't, and the host's context menu, if it opens one.
#[derive(Debug, Clone, Copy)]
pub struct Hover {
    pointer: RwSignal<Option<Spot>>,
    menu: Option<RwSignal<Option<(i32, i32)>>>,
}

impl Hover {
    /// A host the pointer isn't over.
    #[must_use]
    pub fn new() -> Self {
        Self {
            pointer: RwSignal::new(None),
            menu: None,
        }
    }

    /// A host that also opens a context menu, open while `menu` is `Some`
    /// (the signal the wasm-only `interaction::menu_layer` opens). Its
    /// popover stays closed while the menu is open: the popover stacks above
    /// every menu, so it would cover the menu it was opened beside, and the
    /// menu's backdrop lies inside the host, so the pointer never leaves it.
    #[must_use]
    pub fn beside_menu(menu: RwSignal<Option<(i32, i32)>>) -> Self {
        Self {
            menu: Some(menu),
            ..Self::new()
        }
    }

    /// Where the popover opens: the pointer, while it is over the host and
    /// no menu of the host's is open. Reactive.
    fn at(self) -> Option<Spot> {
        let menu_open = self.menu.is_some_and(|menu| menu.with(Option::is_some));
        self.pointer.get().filter(|_| !menu_open)
    }

    /// Whether the host's popover is open. Reactive.
    #[must_use]
    pub fn is_open(self) -> bool {
        self.at().is_some()
    }

    /// The pointer entered or moved over the host: open the popover beside it.
    pub fn track(self, ev: &MouseEvent) {
        self.pointer.set(Some(Spot::of(ev)));
    }

    /// The pointer left the host: close the popover.
    pub fn clear(self) {
        self.pointer.set(None);
    }

    /// The host's pointer handlers, spread onto it as `{..hover.handlers()}`:
    /// entering or moving over it [`track`](Self::track)s the pointer, and
    /// leaving it [`clear`](Self::clear)s it.
    pub fn handlers(self) -> impl Attribute {
        (
            ev::on(ev::mouseenter, move |e: MouseEvent| self.track(&e)),
            ev::on(ev::mousemove, move |e: MouseEvent| self.track(&e)),
            ev::on(ev::mouseleave, move |_| self.clear()),
        )
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
        hover.at().map(|spot| {
            view! {
                <div
                    class="popover"
                    class:opens-left=spot.left
                    class:opens-up=spot.up
                    role="tooltip"
                    style=spot.style()
                >
                    {children()}
                </div>
            }
        })
    }
}
