//! Baseball Bat (Survivor melee weapon asset, 01074).
//!
//! ```text
//! [action]: Fight. You get +2 [combat] for this attack. This attack deals +1
//! damage. If a [skull] or [auto_fail] symbol is revealed during this attack,
//! discard Baseball Bat after the attack resolves.
//! ```
//!
//! **Designator: Fight** — the bold word **performs the attack**, modified as
//! the card prints: `+2 [combat]` and `1 + 1` damage on success. The discard
//! clause rides the attack's skill test as a [`RevealClause`] over `[skull]`
//! and `[auto_fail]`, as Shrivelling 01060's horror does. The ability's own
//! effect is empty.
//!
//! The clause is not a reaction to the token. Ruling
//! (<https://arkhamdb.com/card/01074>): *"The "If a symbol is revealed..."
//! effect triggers during Step 3 of the Skill Test Timing."* It triggers then,
//! but what it does is put off: *"after the attack resolves"*. So the clause's
//! native only marks the test with the Bat
//! ([`NativeCx::discard_after_this_test`]), and the test's ST.8 teardown
//! discards it, past the attack's ST.7 damage. A test abandoned because its
//! target left play discards it as well.
//!
//! The clause reads the token the test counts, so an `[elder_sign]` The
//! Necronomicon 01009 treats as an `[auto_fail]` discards the Bat too. A token
//! cancelled in its entirety (Wendy Adams 01005) triggers nothing, per the same
//! ruling page.
//!
//! Its two `Hand` slots come from the corpus (`slot: "Hand x2"`,
//! pipeline-parsed).

use card_dsl::dsl::{self, Ability, ChaosSymbol, RevealClause};
use game_core::engine::evaluator::EvalContext;
use game_core::engine::{EngineOutcome, NativeCx};

use crate::impls::CardRecord;

/// `ArkhamDB` code for Baseball Bat.
pub const CODE: &str = "01074";

/// This card's registration, listed in [`ALL`](super::ALL).
pub const CARD: CardRecord =
    CardRecord::new(CODE, abilities).effects(&[(DISCARD_AFTER_TAG, discard_after_attack)]);

/// Native effect tag: discard this Bat once the attack resolves.
const DISCARD_AFTER_TAG: &str = "01074:discard-after-attack";

/// The `[action]` Fight: `+2 [combat]`, `+1` damage, and the Bat discarded
/// after the attack if a `[skull]` or `[auto_fail]` is revealed.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![dsl::activated_as(
        dsl::fight(2i8, 1u8).with_reveal_clause(RevealClause {
            symbols: vec![ChaosSymbol::Skull, ChaosSymbol::AutoFail],
            effect: Box::new(dsl::native(DISCARD_AFTER_TAG)),
        }),
        1,
        vec![],
        dsl::seq([]),
    )]
}

/// *"discard Baseball Bat after the attack resolves"* — mark the attack's test
/// with the Bat that started it, for the teardown to discard.
fn discard_after_attack(cx: &mut NativeCx, ctx: &EvalContext) -> EngineOutcome {
    let Some(bat) = ctx.source_instance() else {
        return EngineOutcome::Rejected {
            reason: "baseball_bat discard after attack: no source instance".into(),
        };
    };
    cx.discard_after_this_test(bat);
    EngineOutcome::Done
}

#[cfg(test)]
mod tests {
    use card_dsl::dsl::{ActionDesignator, Effect, IntExpr, Trigger};

    use super::*;

    #[test]
    fn one_costless_activated_fight_with_a_skull_and_auto_fail_clause() {
        let abilities = abilities();
        assert_eq!(abilities.len(), 1);
        assert_eq!(abilities[0].trigger, Trigger::Activated { action_cost: 1 });
        assert!(abilities[0].costs.is_empty());
        let Some(ActionDesignator::Fight {
            combat_modifier,
            extra_damage,
            skill_override,
            reveal_clause,
        }) = &abilities[0].designator
        else {
            panic!(
                "expected a Fight designator, got {:?}",
                abilities[0].designator
            );
        };
        assert_eq!(*combat_modifier, IntExpr::Lit(2));
        assert_eq!(*extra_damage, IntExpr::Lit(1));
        assert_eq!(*skill_override, None);
        assert_eq!(
            *reveal_clause,
            Some(RevealClause {
                symbols: vec![ChaosSymbol::Skull, ChaosSymbol::AutoFail],
                effect: Box::new(dsl::native(DISCARD_AFTER_TAG)),
            })
        );
        assert_eq!(abilities[0].effect, Effect::Seq(vec![]));
    }
}
