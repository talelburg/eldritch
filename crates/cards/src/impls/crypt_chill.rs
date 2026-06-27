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
//! controller's in-play assets and applies the resolve convention: 0 assets →
//! the printed "take 2 damage" fallback; 1 → auto-discard; 2+ → suspend for a
//! controller pick via [`game_core::suspend_for_native_choice`]. On resume the
//! native re-runs with the pick threaded through
//! [`EvalContext::chosen_option`](game_core::EvalContext::chosen_option),
//! re-enumerating in the same order and indexing by it.

use card_dsl::card_data::{CardKind, SkillKind};
use card_dsl::dsl::{native, revelation, skill_test, Ability};
use game_core::card_registry::NativeEffectFn;
use game_core::state::{CardInstanceId, InvestigatorId, Zone};
use game_core::{
    resolve_choice_count, suspend_for_native_choice, take_damage, ChoiceResolution, Cx,
    EngineOutcome, EvalContext, Event,
};

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

/// The controller's in-play asset instances, in play order. The candidate set
/// for the choice — re-enumerated identically on resume so an `OptionId`
/// indexes the same asset.
fn controlled_assets(cx: &Cx, controller: InvestigatorId) -> Vec<CardInstanceId> {
    let Some(inv) = cx.state.investigators.get(&controller) else {
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

fn crypt_chill_fail(cx: &mut Cx, ctx: &EvalContext) -> EngineOutcome {
    let controller = ctx.controller;
    let assets = controlled_assets(cx, controller);

    // Resume: a pick was threaded in — re-enumerate and index by it.
    if let Some(picked) = ctx.chosen_option() {
        let Some(&instance) = assets.get(picked.0 as usize) else {
            return EngineOutcome::Rejected {
                reason: "01167 crypt-chill-fail: chosen_option out of range".into(),
            };
        };
        return discard_asset_instance(cx, controller, instance);
    }

    match resolve_choice_count(assets.len(), cx.state.interactive_acknowledge) {
        // Cannot discard an asset → take 2 damage instead (the printed
        // fallback; defeat handled by the kernel helper).
        ChoiceResolution::Empty => {
            take_damage(cx, controller, 2);
            EngineOutcome::Done
        }
        // Exactly one → auto-discard, no input.
        ChoiceResolution::Auto(i) => discard_asset_instance(cx, controller, assets[i]),
        // 2+ → suspend for the controller's choice.
        ChoiceResolution::Suspend => {
            let labels = assets.iter().map(|id| format!("{id:?}")).collect();
            suspend_for_native_choice(
                cx,
                "Choose an asset to discard",
                labels,
                CRYPT_CHILL_FAIL,
                ctx,
            )
        }
    }
}

/// Discard the named asset instance from the controller's play area.
fn discard_asset_instance(
    cx: &mut Cx,
    controller: InvestigatorId,
    instance: CardInstanceId,
) -> EngineOutcome {
    let Some(inv) = cx.state.investigators.get_mut(&controller) else {
        return EngineOutcome::Rejected {
            reason: "01167 crypt-chill-fail: controller not in state".into(),
        };
    };
    let Some(pos) = inv
        .cards_in_play
        .iter()
        .position(|c| c.instance_id == instance)
    else {
        return EngineOutcome::Rejected {
            reason: "01167 crypt-chill-fail: chosen asset no longer in play".into(),
        };
    };
    let code = inv.cards_in_play.remove(pos).code;
    inv.discard.push(code.clone());
    cx.events.push(Event::CardDiscarded {
        investigator: controller,
        code,
        from: Zone::InPlay,
    });
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

    #[test]
    fn single_asset_discard_surfaces_under_interactive_flag() {
        use game_core::state::{CardCode, CardInPlay};
        use game_core::test_support::{test_investigator, GameStateBuilder};

        // Exactly one discardable asset (Machete 01020) in play. With
        // interactive_acknowledge on, the fail branch must surface the discard as
        // a one-option pick rather than auto-discarding silently (#466).
        let mut inv = test_investigator(1);
        inv.cards_in_play.push(CardInPlay::enter_play(
            CardCode::new("01020"),
            CardInstanceId(1),
        ));
        let mut state = GameStateBuilder::new().with_investigator(inv).build();
        state.interactive_acknowledge = true;
        let mut events: Vec<Event> = Vec::new();
        let ctx = EvalContext::for_controller(InvestigatorId(1));
        let out = {
            let mut cx = Cx {
                state: &mut state,
                events: &mut events,
            };
            super::crypt_chill_fail(&mut cx, &ctx)
        };
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
}
