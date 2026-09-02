//! Drag-to-move for a centred modal (#857).
//!
//! A modal that covers the board answers one question and hides everything the
//! player might want to check while answering it. This lets them shove it aside
//! rather than dismiss it — which for the skill-test result modal is the only
//! move they have, since dismissal submits engine input (`skill_test_result`'s
//! *"dismissible only by that button"*).
//!
//! # Shape
//!
//! One [`Drag`] per modal, `Copy`, holding two signals. The view spends it in
//! three places — the scrim's style, the modal's transform, and the pointer
//! handlers:
//!
//! ```text
//! let drag = Drag::new();
//! view! {
//!     <div class="scrim" style=move || drag.scrim_style()></div>
//!     <section
//!         style=move || drag.transform_style()
//!         on:pointerdown=move |ev| drag.down(&ev)
//!         on:pointermove=move |ev| drag.movement(&ev)
//!         on:pointerup=move |ev| drag.up(&ev)
//!     >…</section>
//! }
//! ```
//!
//! **The scrim fades once the modal has been moved.** Dragging a modal off the
//! centre is a request to see what is underneath, and a scrim at full strength
//! refuses it — so the two are one gesture, not two. The scrim keeps *some*
//! tint so the board still reads as inert, and it keeps having no click
//! handler: moving the modal is not dismissing it.
//!
//! **Drag state is per prompt**, so each newly-opened modal appears centred: a
//! position persisted across prompts can strand a modal half off-screen after a
//! window resize, which then needs its own affordance to dig out of. Both views
//! mount once for the app's lifetime, so [`Drag::per_prompt`] is what ties the
//! offset to the prompt rather than to the mount.
//!
//! # Two targets, one piece of markup
//!
//! The pointer handlers are no-ops on the host build — generic over the event,
//! so nothing in a host build names `web_sys`, a wasm-only dependency — and the
//! same `view!` therefore serves both targets rather than being cfg-split in
//! two.
//!
//! Pointer capture keeps the move and up handlers on the modal itself rather
//! than on `window`: a fast drag that outruns the cursor does not drop the
//! modal mid-gesture, and there is no listener left to leak on unmount.
//!
//! The geometry below is pure and native-tested; the handlers' DOM reads are
//! wasm-only.

use leptos::prelude::*;

/// A drag in flight: where the pointer went down, and the offset it started
/// from. Both are needed because a second drag resumes from where the first
/// left the modal rather than from the centre.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gesture {
    /// Pointer position at pointerdown, in client coordinates.
    origin: (f64, f64),
    /// The modal's offset when the press landed.
    from: (f64, f64),
}

/// The gesture a pointerdown starts, or `None` if it starts none.
///
/// `on_control` is the load-bearing argument: **a press that landed on a button
/// starts no gesture**, because a button inside a modal is the way out of it,
/// and the skill-test modal's Confirm is its *only* way out. Pure.
#[must_use]
pub fn gesture_from(on_control: bool, pointer: (f64, f64), offset: (f64, f64)) -> Option<Gesture> {
    if on_control {
        return None;
    }
    Some(Gesture {
        origin: pointer,
        from: offset,
    })
}

/// The offset a pointer at `pointer` puts the modal at, mid-`gesture`. Pure.
#[must_use]
pub fn offset_during(gesture: Gesture, pointer: (f64, f64)) -> (f64, f64) {
    (
        gesture.from.0 + pointer.0 - gesture.origin.0,
        gesture.from.1 + pointer.1 - gesture.origin.1,
    )
}

/// True once the modal sits off the centred position. Pure.
#[must_use]
pub fn moved_off_centre(offset: (f64, f64)) -> bool {
    offset.0 != 0.0 || offset.1 != 0.0
}

/// `transform` that layers `offset` over the CSS centring. Both modals centre
/// with `translate(-50%, -50%)`, so the offset rides inside that same transform
/// via `calc` rather than fighting it from a second property. Pure.
#[must_use]
pub fn transform_style(offset: (f64, f64)) -> String {
    let (x, y) = offset;
    format!("transform: translate(calc(-50% + {x}px), calc(-50% + {y}px));")
}

/// The alpha a modal's scrim shows at `offset`: [`FULL`] while the modal is
/// centred, [`FADED`] once it has been moved. Pure.
#[must_use]
pub fn scrim_alpha(offset: (f64, f64)) -> f64 {
    if moved_off_centre(offset) {
        FADED
    } else {
        FULL
    }
}

/// The scrim's alpha under a centred modal. Both scrims are styled inline from
/// here rather than from `style.css`, so the alpha has one home: it is a
/// function of how far the modal has been dragged, not a constant.
pub const FULL: f64 = 0.45;

/// The scrim's alpha once its modal has been moved: enough tint that the board
/// still reads as inert, little enough that it reads at all.
pub const FADED: f64 = 0.12;

/// Drag state for one modal: its offset from the centred position, and the
/// gesture in flight, if any.
#[derive(Clone, Copy)]
pub struct Drag {
    /// Offset in px from the CSS-centred position. `(0, 0)` until dragged.
    offset: RwSignal<(f64, f64)>,
    /// The in-flight gesture, `None` between drags. Read by the pointer
    /// handlers, which are inert on the host build.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    gesture: RwSignal<Option<Gesture>>,
}

impl Default for Drag {
    fn default() -> Self {
        Self::new()
    }
}

impl Drag {
    /// A modal that starts centred and un-dragged.
    #[must_use]
    pub fn new() -> Self {
        Self {
            offset: RwSignal::new((0.0, 0.0)),
            gesture: RwSignal::new(None),
        }
    }

    /// The modal's `transform`, offset included. Reactive.
    #[must_use]
    pub fn transform_style(self) -> String {
        transform_style(self.offset.get())
    }

    /// The scrim's `background`, faded once the modal has been moved. Reactive.
    #[must_use]
    pub fn scrim_style(self) -> String {
        let alpha = scrim_alpha(self.offset.get());
        format!("background: rgba(0, 0, 0, {alpha});")
    }

    /// A handle that re-centres its modal whenever `prompt` reports a different
    /// prompt — a fingerprint that changes as one prompt gives way to the next,
    /// and that both views build from the store.
    ///
    /// **Liveness alone is not that fingerprint.** A view mounts once for the
    /// app's lifetime, and answering one prompt into another of the same kind
    /// never takes liveness false in between — so a modal keyed on liveness
    /// would open the second prompt wherever the player left the first, exactly
    /// the stranding this is here to prevent.
    #[must_use]
    pub fn per_prompt<T>(prompt: impl Fn() -> T + Send + Sync + 'static) -> Self
    where
        T: Clone + PartialEq + Send + Sync + 'static,
    {
        let drag = Self::new();
        // The effect is handed what it returned last time, which is the whole
        // comparison: no previous fingerprint means the first run, and a
        // different one means a different prompt is up.
        Effect::new(move |previous: Option<T>| {
            let current = prompt();
            if previous.as_ref() != Some(&current) {
                drag.reset();
            }
            current
        });
        drag
    }

    /// Put the modal back at the centre, cancelling any gesture.
    fn reset(self) {
        self.offset.set((0.0, 0.0));
        self.gesture.set(None);
    }

    /// Begin a drag, unless the press landed on a button.
    #[cfg(target_arch = "wasm32")]
    pub fn down(self, ev: &web_sys::PointerEvent) {
        use wasm_bindgen::JsCast as _;

        let on_control = ev
            .target()
            .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
            .is_some_and(|el| el.closest("button").ok().flatten().is_some());
        let pointer = (f64::from(ev.client_x()), f64::from(ev.client_y()));
        let Some(gesture) = gesture_from(on_control, pointer, self.offset.get_untracked()) else {
            return;
        };
        // Capture routes every later pointer event to this element, so a fast
        // drag that outruns the cursor doesn't drop the modal mid-gesture.
        if let Some(el) = ev
            .current_target()
            .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
        {
            let _ = el.set_pointer_capture(ev.pointer_id());
        }
        self.gesture.set(Some(gesture));
    }

    /// Track the pointer while a gesture is in flight.
    #[cfg(target_arch = "wasm32")]
    pub fn movement(self, ev: &web_sys::PointerEvent) {
        let Some(gesture) = self.gesture.get_untracked() else {
            return;
        };
        let pointer = (f64::from(ev.client_x()), f64::from(ev.client_y()));
        self.offset.set(offset_during(gesture, pointer));
    }

    /// End the gesture. The offset stays: the modal keeps where it was put.
    #[cfg(target_arch = "wasm32")]
    pub fn up(self, _ev: &web_sys::PointerEvent) {
        self.gesture.set(None);
    }

    /// Host build: inert, so the same `view!` compiles on both targets.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn down<E>(self, _ev: &E) {}

    /// Host-build counterpart of [`Self::movement`].
    #[cfg(not(target_arch = "wasm32"))]
    pub fn movement<E>(self, _ev: &E) {}

    /// Host-build counterpart of [`Self::up`].
    #[cfg(not(target_arch = "wasm32"))]
    pub fn up<E>(self, _ev: &E) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_press_on_a_button_starts_no_gesture() {
        assert_eq!(gesture_from(true, (10.0, 10.0), (0.0, 0.0)), None);
    }

    #[test]
    fn a_press_off_a_button_starts_a_gesture_from_the_current_offset() {
        let g = gesture_from(false, (10.0, 20.0), (3.0, 4.0)).expect("a gesture starts");
        assert_eq!(offset_during(g, (10.0, 20.0)), (3.0, 4.0));
    }

    #[test]
    fn the_modal_follows_the_pointer_by_the_distance_it_travelled() {
        let g = gesture_from(false, (100.0, 100.0), (0.0, 0.0)).expect("a gesture starts");
        assert_eq!(offset_during(g, (140.0, 70.0)), (40.0, -30.0));
    }

    /// A second drag resumes from where the first left the modal, rather than
    /// snapping it back to the centre first.
    #[test]
    fn a_later_drag_resumes_from_the_offset_it_was_left_at() {
        let g = gesture_from(false, (100.0, 100.0), (40.0, -30.0)).expect("a gesture starts");
        assert_eq!(offset_during(g, (110.0, 110.0)), (50.0, -20.0));
    }

    #[test]
    fn a_centred_modal_has_not_been_moved() {
        assert!(!moved_off_centre((0.0, 0.0)));
        assert!(moved_off_centre((0.0, 1.0)));
        assert!(moved_off_centre((-1.0, 0.0)));
    }

    /// The offset rides inside the centring transform rather than beside it.
    #[test]
    fn the_offset_layers_inside_the_centring_transform() {
        assert_eq!(
            transform_style((40.0, -30.0)),
            "transform: translate(calc(-50% + 40px), calc(-50% + -30px));"
        );
    }

    #[test]
    fn the_scrim_is_at_full_strength_until_the_modal_is_moved() {
        assert!((scrim_alpha((0.0, 0.0)) - FULL).abs() < f64::EPSILON);
    }

    /// Faded, not cleared: the board must still read as inert.
    #[test]
    fn the_scrim_fades_but_keeps_its_tint_once_moved() {
        let faded = scrim_alpha((40.0, 0.0));
        assert!(faded < FULL, "fades: {faded}");
        assert!(faded > 0.0, "keeps a tint: {faded}");
    }
}
