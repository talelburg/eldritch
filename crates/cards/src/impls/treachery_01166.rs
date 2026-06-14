//! Ancient Evils (The Gathering treachery, 01166).
//!
//! ```text
//! Revelation - Place 1 doom on the current agenda. This effect can cause
//!   the current agenda to advance.
//! ```
//!
//! Card-local native (#276): a single consumer of "place doom on the
//! current agenda", so it doesn't earn a shared `Effect` variant. Calls
//! the engine's `place_doom_on_current_agenda` (place + threshold check).

use card_dsl::dsl::{native, revelation, Ability};
use game_core::card_registry::NativeEffectFn;
use game_core::{place_doom_on_current_agenda, Cx, EngineOutcome, EvalContext};

/// `ArkhamDB` code for Ancient Evils.
pub const CODE: &str = "01166";

const PLACE_DOOM: &str = "01166:place-doom";

#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![revelation(native(PLACE_DOOM))]
}

/// Resolve this treachery's native-effect tag. Wired into the crate
/// registry's `native_effect_for`.
pub(crate) fn native_effect_for(tag: &str) -> Option<NativeEffectFn> {
    match tag {
        PLACE_DOOM => Some(place_doom as NativeEffectFn),
        _ => None,
    }
}

fn place_doom(cx: &mut Cx, _ctx: &EvalContext) -> EngineOutcome {
    place_doom_on_current_agenda(cx);
    EngineOutcome::Done
}

#[cfg(test)]
mod tests {
    use super::*;
    use card_dsl::dsl::{Effect, Trigger};

    #[test]
    fn revelation_is_native_place_doom() {
        let abilities = abilities();
        assert_eq!(abilities.len(), 1);
        assert_eq!(abilities[0].trigger, Trigger::Revelation);
        assert!(matches!(&abilities[0].effect, Effect::Native { tag } if tag == PLACE_DOOM));
        assert!(native_effect_for(PLACE_DOOM).is_some());
        assert!(native_effect_for("nope").is_none());
    }
}
