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
//! **Interactive choice (Axis A, #334).** The fail branch enumerates the
//! controller's in-play assets and offers them through
//! [`game_core::engine::resolve_grounded_choice`]: 0 assets → the printed "take
//! 2 damage" fallback; 1 → auto-discard; 2+ → suspend for a controller pick. On
//! resume the native re-runs, re-enumerates in the same order, and the engine
//! function indexes the list by the threaded pick.

use card_dsl::card_data::{CardKind, SkillKind};
use card_dsl::dsl::{self, Ability};
use game_core::engine::evaluator::EvalContext;
use game_core::engine::{self, EngineOutcome, Grounded, NativeCx, OptionTarget};
use game_core::state::{CardInstanceId, GameState, InvestigatorId};

use crate::impls::CardRecord;

/// `ArkhamDB` code for Crypt Chill.
pub const CODE: &str = "01167";

/// This card's registration, listed in [`ALL`](super::ALL).
pub const CARD: CardRecord =
    CardRecord::new(CODE, abilities).effects(&[(CRYPT_CHILL_FAIL, crypt_chill_fail)]);

const CRYPT_CHILL_FAIL: &str = "01167:crypt-chill-fail";

#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![dsl::revelation(dsl::skill_test(
        SkillKind::Willpower,
        4,
        None,
        Some(dsl::native(CRYPT_CHILL_FAIL)),
    ))]
}

/// The controller's in-play asset instances, in play order. The candidate set
/// for the choice — re-enumerated identically on resume so an `OptionId`
/// indexes the same asset.
fn controlled_assets(state: &GameState, controller: InvestigatorId) -> Vec<CardInstanceId> {
    let Some(inv) = state.investigators.get(&controller) else {
        return Vec::new();
    };
    inv.cards_in_play
        .iter()
        .filter(|c| {
            matches!(
                crate::by_code(&c.code.0).map(|m| &m.kind),
                Some(CardKind::Asset { .. })
            )
        })
        .map(|c| c.instance_id)
        .collect()
}

fn crypt_chill_fail(cx: &mut NativeCx, ctx: &EvalContext) -> EngineOutcome {
    let controller = ctx.controller;
    let assets = controlled_assets(cx.state(), controller);
    match engine::resolve_grounded_choice(
        cx.state(),
        cx.cards(),
        ctx,
        &assets,
        "Choose an asset to discard",
        |id| OptionTarget::CardInstance(*id),
    ) {
        Grounded::Picked(instance) => discard_asset_instance(cx, instance),
        // Cannot discard an asset → take 2 damage instead (the printed
        // fallback), dealt as the native's tail.
        Grounded::Empty => cx.deal_harm(controller, 2, 0),
        Grounded::Suspend(outcome) => outcome,
    }
}

/// Discard the chosen asset through the engine's discard-from-play exit, which
/// files it by its **owner** — so a Parleyed Lita Chantler 01117 is removed
/// from the game rather than discarded (#918, <https://arkhamdb.com/card/01117>).
fn discard_asset_instance(cx: &mut NativeCx, instance: CardInstanceId) -> EngineOutcome {
    match cx.discard_from_play(instance) {
        Some(_) => EngineOutcome::Done,
        None => EngineOutcome::Rejected {
            reason: "01167 crypt-chill-fail: chosen asset no longer in play".into(),
        },
    }
}

#[cfg(test)]
mod tests {
    use card_dsl::dsl::Effect;
    use game_core::state::{CardCode, GameStateBuilder};
    use game_core::test_support::{self, seat};

    use super::*;

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
    }

    #[test]
    fn single_asset_discard_surfaces_under_interactive_flag() {
        // Exactly one discardable asset (Machete 01020) in play. With
        // interactive_acknowledge on, the fail branch must surface the discard as
        // a one-option pick rather than auto-discarding silently (#466).
        let mut state = GameStateBuilder::new()
            .with_seat(seat(1).in_play(["01020"]))
            .build();
        state.interactive_acknowledge = true;
        let ctx = EvalContext::for_controller(InvestigatorId(1));
        let (out, _) =
            test_support::run_native(&mut state, &crate::REGISTRY, crypt_chill_fail, &ctx);
        match out {
            EngineOutcome::AwaitingInput { request, .. } => {
                assert_eq!(
                    request.options.len(),
                    1,
                    "lone asset surfaces as one option"
                );
            }
            other => panic!("expected a one-option discard suspend, got {other:?}"),
        }
    }

    #[test]
    fn single_asset_auto_discards_when_flag_off() {
        // Flag off (default): the lone asset auto-discards silently, as today —
        // the n=1 regression guard local to this card (#466).
        let mut state = GameStateBuilder::new()
            .with_seat(seat(1).in_play(["01020"]))
            .build();
        let ctx = EvalContext::for_controller(InvestigatorId(1));
        let (out, _) =
            test_support::run_native(&mut state, &crate::REGISTRY, crypt_chill_fail, &ctx);
        assert!(
            matches!(out, EngineOutcome::Done),
            "flag off: auto-discard, no suspend"
        );
        let inv = &state.investigators[&InvestigatorId(1)];
        assert!(inv.cards_in_play.is_empty(), "the asset was discarded");
        assert_eq!(
            inv.discard,
            vec![CardCode::new("01020")],
            "asset moved to discard"
        );
    }

    #[test]
    fn no_asset_fallback_deals_two_damage_through_the_deal_frame() {
        // No asset to discard → "take 2 damage instead", dealt through the one
        // harm adapter: a `DealDamage { Effect }` frame for 2 damage is the
        // native's tail, and nothing is placed until that frame runs (#1027).
        let mut state = GameStateBuilder::new().with_seat(seat(1)).build();
        let ctx = EvalContext::for_controller(InvestigatorId(1));
        let (out, _) =
            test_support::run_native(&mut state, &crate::REGISTRY, crypt_chill_fail, &ctx);
        assert_eq!(out, EngineOutcome::Done);
        test_support::assert_effect_deal(state.continuations.top(), InvestigatorId(1), 2, 0);
        assert_eq!(state.investigators[&InvestigatorId(1)].damage(), 0);
    }
}
