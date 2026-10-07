//! The [`Frame`] downcast trait and the `impl_frame!` macro that implements
//! it, one invocation per newtype [`Continuation`] variant. The invocation
//! list sits beside the enum in `continuation.rs`, where every payload type is
//! already in scope.

use crate::state::Continuation;

/// A frame payload: the type wrapped by exactly one newtype [`Continuation`]
/// variant.
///
/// It is what lets the stack's accessors —
/// [`topmost_of`](crate::state::ContinuationStack::topmost_of), `top_mut`,
/// `pop_expect` — name a frame kind as a type parameter instead of each caller
/// hand-writing a `match` with an "expected X on top" arm. Implemented only by
/// the `impl_frame!` macro, never by hand, so every impl is the same downcast.
pub trait Frame: Sized + Into<Continuation> {
    /// The variant's name, for accessor panic messages.
    const KIND: &'static str;

    /// Borrow this payload out of `frame`, if `frame` is this kind.
    fn downcast_ref(frame: &Continuation) -> Option<&Self>;

    /// Mutably borrow this payload out of `frame`, if `frame` is this kind.
    fn downcast_mut(frame: &mut Continuation) -> Option<&mut Self>;

    /// Take this payload out of `frame`, if `frame` is this kind.
    fn downcast(frame: Continuation) -> Option<Self>;
}

/// Implement [`Frame`] for a newtype variant's payload, plus the conversion
/// from payload into frame that the stack's checked push accepts.
///
/// `impl_frame!(Variant, Payload);` — `Variant` is the `Continuation` variant
/// wrapping `Payload`.
macro_rules! impl_frame {
    ($variant:ident, $payload:ty) => {
        impl $crate::state::continuation::Frame for $payload {
            const KIND: &'static str = stringify!($variant);

            fn downcast_ref(frame: &$crate::state::Continuation) -> Option<&Self> {
                match frame {
                    $crate::state::Continuation::$variant(payload) => Some(payload),
                    _ => None,
                }
            }

            fn downcast_mut(frame: &mut $crate::state::Continuation) -> Option<&mut Self> {
                match frame {
                    $crate::state::Continuation::$variant(payload) => Some(payload),
                    _ => None,
                }
            }

            fn downcast(frame: $crate::state::Continuation) -> Option<Self> {
                match frame {
                    $crate::state::Continuation::$variant(payload) => Some(payload),
                    _ => None,
                }
            }
        }

        impl From<$payload> for $crate::state::Continuation {
            fn from(payload: $payload) -> Self {
                $crate::state::Continuation::$variant(payload)
            }
        }
    };
}

pub(crate) use impl_frame;
