//! Crypt Chill (The Gathering treachery, 01167).
//!
//! ```text
//! Revelation - Test [willpower] (4). If you fail, choose and discard 1
//!   asset you control (if you cannot, take 2 damage instead).
//! ```
//!
//! The willpower(4) test is shared DSL; the failure branch is card-local
//! native (#276) — a single consumer of "discard an asset you control".
//!
//! TODO(#212): "choose" is an interactive decision; until a mid-revelation
//! `ChooseOne` can suspend, this discards the first asset in play order
//! (a deterministic legal outcome, mirroring the 01105 reverse). The "2
//! damage" branch is the printed fallback for controlling **no** asset,
//! not a pass/fail alternative.

use card_dsl::card_data::{CardKind, SkillKind};
use card_dsl::dsl::{native, revelation, skill_test, Ability};
use game_core::card_registry::NativeEffectFn;
use game_core::state::Zone;
use game_core::{take_damage, Cx, EngineOutcome, EvalContext, Event};

/// `ArkhamDB` code for Crypt Chill.
pub const CODE: &str = "01167";

const CRYPT_CHILL_FAIL: &str = "01167:crypt-chill-fail";

#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![revelation(skill_test(
        SkillKind::Willpower,
        4,
        None,
        Some(native(CRYPT_CHILL_FAIL)),
    ))]
}

/// Resolve this treachery's native-effect tag. Wired into the crate
/// registry's `native_effect_for`.
pub(crate) fn native_effect_for(tag: &str) -> Option<NativeEffectFn> {
    (tag == CRYPT_CHILL_FAIL).then_some(crypt_chill_fail as NativeEffectFn)
}

fn crypt_chill_fail(cx: &mut Cx, ctx: &EvalContext) -> EngineOutcome {
    let controller = ctx.controller;
    let Some(inv) = cx.state.investigators.get_mut(&controller) else {
        return EngineOutcome::Rejected {
            reason: "01167 crypt-chill-fail: controller not in state".into(),
        };
    };
    // Deterministic stand-in for "choose" (TODO #212): first asset in
    // play order. `crate::by_code` reads the corpus kind.
    let asset_pos = inv.cards_in_play.iter().position(|c| {
        matches!(
            crate::by_code(&c.code.0).map(|m| &m.kind),
            Some(CardKind::Asset { .. })
        )
    });
    if let Some(pos) = asset_pos {
        let code = inv.cards_in_play.remove(pos).code;
        inv.discard.push(code.clone());
        cx.events.push(Event::CardDiscarded {
            investigator: controller,
            code,
            from: Zone::InPlay,
        });
    } else {
        // Cannot discard an asset → take 2 damage instead (defeat
        // handled by the kernel helper).
        take_damage(cx, controller, 2);
    }
    EngineOutcome::Done
}

#[cfg(test)]
mod tests {
    use super::*;
    use card_dsl::dsl::Effect;

    #[test]
    fn revelation_tests_willpower_4_then_native_fail() {
        let abilities = abilities();
        assert_eq!(abilities.len(), 1);
        let Effect::SkillTest {
            skill,
            difficulty,
            on_success,
            on_fail,
        } = &abilities[0].effect
        else {
            panic!("expected SkillTest, got {:?}", abilities[0].effect);
        };
        assert_eq!(*skill, SkillKind::Willpower);
        assert_eq!(*difficulty, 4);
        assert!(on_success.is_none(), "no success-side effect");
        assert!(
            matches!(on_fail.as_deref(), Some(Effect::Native { tag }) if tag == CRYPT_CHILL_FAIL)
        );
        assert!(native_effect_for(CRYPT_CHILL_FAIL).is_some());
        assert!(native_effect_for("nope").is_none());
    }
}
