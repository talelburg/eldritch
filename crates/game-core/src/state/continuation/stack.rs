//! [`ContinuationStack`]: the one suspend/resume stack, as an owned type that
//! checks its invariants at the push that would break them.

use serde::{Deserialize, Serialize};

use crate::state::continuation::Frame;
use crate::state::{
    Continuation, FrameActivity, InFlightSkillTest, ScenarioEndFrame, ScenarioEndStep,
};

/// The continuation stack (umbrella §1 / Axis-B): every suspended resolution,
/// bottom to top, the top being the frame the engine resumes next.
///
/// Its rules, and where each is enforced:
///
/// - **No queued ability beneath a phase anchor** (ADR 0003): checked when an
///   anchor is pushed.
/// - **At most one skill test in flight**: checked when a skill test is pushed.
/// - **The ending frame sits at the bottom** (ADR 0004): it can only be created
///   by `insert_ending_at_bottom`; pushing one is refused.
/// - **A Prompt is on top whenever the engine waits for input**: not a push
///   rule — frames are legitimately pushed above a prompt while it resolves its
///   own input — but checked at the end of every `apply`
///   ([`is_at_rest`](Self::is_at_rest)).
///
/// The checks are debug assertions, ADR 0003's posture: a violation is a
/// programmer error, so release builds pay nothing for them.
///
/// The storage is private and every mutation is crate-private, so no code
/// outside the engine can bypass the checks. Other crates read the stack
/// ([`iter`](Self::iter), [`top`](Self::top), [`topmost_of`](Self::topmost_of))
/// and build one for a fixture only through
/// [`test_support::from_frames_unchecked`](crate::test_support::from_frames_unchecked):
///
/// ```compile_fail,E0624
/// use game_core::state::{Continuation, ContinuationStack, MulliganFrame};
///
/// let mut stack = ContinuationStack::new();
/// let frame: Continuation = MulliganFrame { remaining: vec![] }.into();
/// stack.push(frame); // `push` is crate-private
/// ```
///
/// Serialises transparently, as the plain array of externally tagged frames it
/// replaced, so the wire format and persisted seed states are unchanged.
/// Deserialising runs no checks: a serialised stack was produced by the
/// engine.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ContinuationStack {
    frames: Vec<Continuation>,
}

impl ContinuationStack {
    /// An empty stack.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Build a stack from raw frames, bottom first, **without** checking any
    /// invariant. For fixtures that model a state directly; reached publicly
    /// only through
    /// [`test_support::from_frames_unchecked`](crate::test_support::from_frames_unchecked).
    pub(crate) fn from_frames_unchecked(frames: Vec<Continuation>) -> Self {
        Self { frames }
    }

    /// The frames, bottom to top.
    pub fn iter(&self) -> std::slice::Iter<'_, Continuation> {
        self.frames.iter()
    }

    /// The frames, bottom to top, mutably — for edits inside frames that leave
    /// every frame's kind as it was (elimination draining a card out of each
    /// frame holding it).
    pub(crate) fn frames_mut(&mut self) -> std::slice::IterMut<'_, Continuation> {
        self.frames.iter_mut()
    }

    /// How many frames are on the stack.
    #[must_use]
    pub fn len(&self) -> usize {
        self.frames.len()
    }

    /// Whether the stack holds no frame.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// The top frame — the one the engine resumes next — or `None` when empty.
    #[must_use]
    pub fn top(&self) -> Option<&Continuation> {
        self.frames.last()
    }

    /// Mutably borrow the top frame, whatever its kind, or `None` when empty.
    /// For code that accepts more than one kind on top — either window kind,
    /// or an effect frame in any of its shapes — and so has no single [`Frame`]
    /// type to name; prefer [`top_mut`](Self::top_mut) when one kind is
    /// expected. The borrow must not change the frame's kind: that would
    /// bypass the push checks.
    pub(crate) fn top_frame_mut(&mut self) -> Option<&mut Continuation> {
        self.frames.last_mut()
    }

    /// The topmost frame of kind `F`, possibly buried beneath others — e.g.
    /// the in-flight skill test beneath a reaction window opened mid-test.
    #[must_use]
    pub fn topmost_of<F: Frame>(&self) -> Option<&F> {
        self.frames.iter().rev().find_map(F::downcast_ref)
    }

    /// Mutable counterpart to [`topmost_of`](Self::topmost_of).
    pub(crate) fn topmost_of_mut<F: Frame>(&mut self) -> Option<&mut F> {
        self.frames.iter_mut().rev().find_map(F::downcast_mut)
    }

    /// Whether the stack may rest here at an `apply` boundary: empty, or a
    /// [Prompt](FrameActivity::Prompt) on top. Any other top is a frame a
    /// resolution left stranded — nothing would ever advance it.
    #[must_use]
    pub fn is_at_rest(&self) -> bool {
        self.top()
            .is_none_or(|top| top.profile().activity == FrameActivity::Prompt)
    }

    /// Push `frame` on top.
    ///
    /// # Panics
    ///
    /// In debug builds, if the push would break a stack invariant: a phase
    /// anchor over a queued ability (ADR 0003), a second skill test, or an
    /// ending frame (which only
    /// [`insert_ending_at_bottom`](Self::insert_ending_at_bottom) may create).
    /// The panic reports the caller's location.
    #[track_caller]
    pub(crate) fn push(&mut self, frame: impl Into<Continuation>) {
        let frame = frame.into();
        if cfg!(debug_assertions) {
            self.assert_push_allowed(&frame);
        }
        self.frames.push(frame);
    }

    /// The push-time checks behind [`push`](Self::push).
    ///
    /// The anchor check is the backstop for the ADR 0003 defect class (#569).
    /// Phase anchors **pop-and-push** rather than drain — a transition pops the
    /// outgoing anchor and pushes the incoming one on whatever is left — so a
    /// queued ability that ends up below one is not merely mis-ordered, it is
    /// stranded at the bottom of the stack for the rest of the scenario. That
    /// is exactly how agenda 01107's Ghoul movement was lost: `enemy_phase_end`
    /// read the emit's `Done` as "nothing happened" and pushed the Upkeep
    /// anchor over the frame it had just queued. Checked at the push, so it
    /// names the line that made the mistake.
    #[track_caller]
    fn assert_push_allowed(&self, frame: &Continuation) {
        if frame.is_phase_anchor() {
            if let Some(buried) = self.frames.iter().find(|f| f.is_queued_ability()) {
                panic!(
                    "pushing the {frame:?} anchor would bury a queued ability frame ({buried:?}) \
                     — a phase anchor was pushed over an ability a timing-point emit had \
                     queued, which strands it (#569). Emit in tail position and resume via a \
                     frame; see docs/adr/0003-emitting-a-timing-point-queues-abilities.md."
                );
            }
        }
        assert!(
            !(InFlightSkillTest::downcast_ref(frame).is_some()
                && self.topmost_of::<InFlightSkillTest>().is_some()),
            "pushing a second skill test while one is in flight: at most one skill test \
             resolves at a time"
        );
        assert!(
            ScenarioEndFrame::downcast_ref(frame).is_none(),
            "pushing an ending frame: only `insert_ending_at_bottom` may create one, so it \
             always sits at the bottom (ADR 0004)"
        );
    }

    /// Pop the top frame, whatever its kind.
    pub(crate) fn pop(&mut self) -> Option<Continuation> {
        self.frames.pop()
    }

    /// Mutably borrow the top frame, which must be of kind `F`.
    ///
    /// # Panics
    ///
    /// If the stack is empty or its top is another kind; the panic reports the
    /// caller's location.
    #[track_caller]
    pub(crate) fn top_mut<F: Frame>(&mut self) -> &mut F {
        self.assert_top_is::<F>();
        self.frames
            .last_mut()
            .and_then(F::downcast_mut)
            .expect("assert_top_is checked the top frame's kind")
    }

    /// Pop the top frame, which must be of kind `F`, and return its payload.
    ///
    /// # Panics
    ///
    /// If the stack is empty or its top is another kind; the panic reports the
    /// caller's location.
    #[track_caller]
    pub(crate) fn pop_expect<F: Frame>(&mut self) -> F {
        self.assert_top_is::<F>();
        self.frames
            .pop()
            .and_then(F::downcast)
            .expect("assert_top_is checked the top frame's kind")
    }

    /// The accessors' shared panic: the top frame is not of kind `F`.
    #[track_caller]
    fn assert_top_is<F: Frame>(&self) {
        let Some(top) = self.top() else {
            panic!("expected a {} frame on top, found an empty stack", F::KIND);
        };
        assert!(
            F::downcast_ref(top).is_some(),
            "expected a {} frame on top, found {top:?}",
            F::KIND
        );
    }

    /// Remove the topmost frame of kind `F`, possibly buried, and return its
    /// payload; `None` if there is none. For the one teardown that ends a frame
    /// from beneath: the skill test, whose follow-up may have pushed above it.
    pub(crate) fn remove_topmost<F: Frame>(&mut self) -> Option<F> {
        let pos = self
            .frames
            .iter()
            .rposition(|f| F::downcast_ref(f).is_some())?;
        F::downcast(self.frames.remove(pos))
    }

    /// Create the scenario's ending frame at the bottom of the stack, beneath
    /// everything already under way (ADR 0004). The only route to an ending
    /// frame.
    ///
    /// # Panics
    ///
    /// In debug builds, if an ending frame already exists: the ending latches
    /// once per scenario.
    #[track_caller]
    pub(crate) fn insert_ending_at_bottom(&mut self) {
        debug_assert!(
            !self
                .frames
                .iter()
                .any(|f| ScenarioEndFrame::downcast_ref(f).is_some()),
            "a second ending frame: the ending latches once per scenario"
        );
        self.frames.insert(
            0,
            ScenarioEndFrame {
                step: ScenarioEndStep::EmitGameEnd,
            }
            .into(),
        );
    }
}

impl<'a> IntoIterator for &'a ContinuationStack {
    type Item = &'a Continuation;
    type IntoIter = std::slice::Iter<'a, Continuation>;

    fn into_iter(self) -> Self::IntoIter {
        self.frames.iter()
    }
}

impl PartialEq<Vec<Continuation>> for ContinuationStack {
    fn eq(&self, other: &Vec<Continuation>) -> bool {
        self.frames == *other
    }
}

impl PartialEq<ContinuationStack> for Vec<Continuation> {
    fn eq(&self, other: &ContinuationStack) -> bool {
        *self == other.frames
    }
}
