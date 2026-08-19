//! Guard Dog (Guardian ally asset, 01021).
//!
//! ```text
//! [reaction] When an enemy attack deals damage to Guard Dog: Deal 1
//!   damage to the attacking enemy.
//! ```
//!
//! Health 3, sanity 1, Ally slot. As an ally with printed health/sanity,
//! Guard Dog is a soak container (RR p.7): enemy-attack damage routes onto
//! it before the controller (C5b's soak pipeline). The reaction below is
//! the *other* half — when that soak sends damage its way, it bites back.
//!
//! **The ability resolves in the `when` cell of the `DamageAssigned`
//! condition**, which is the word it prints. Dealing damage is two Rules
//! Reference steps with a named window between them
//! (`glossary/Dealing_Damage_Horror.md`): damage is **assigned** — tokens set
//! *"next to the cards that will be taking the damage/horror"* — and only then
//! **placed**. *"When an enemy attack deals damage to Guard Dog"* is the first of
//! those, so the retaliate interrupts the deal before any damage is on this
//! card. That is what `glossary/Nested_Sequences.md` works its example on, using
//! this exact card: *"**Before resolving** the damage dealt to the Guard Dog, 1
//! horror is dealt to each investigator at the location"*. And it is what makes
//! the card's own ruling true — `data/arkhamdb-faq/core/01021.md`: *"You can use
//! Guard Dog's ability when you assign lethal damage/horror to it."* — since the
//! Guard Dog is still in play when a lethal assignment is announced. See
//! `docs/adr/0009-damage-is-assigned-then-placed.md`.
//!
//! The reaction is a card-local [`Effect::Native`](card_dsl::dsl::Effect::Native)
//! handler rather than a shared `Effect` variant: it names the attacking
//! enemy, which only exists in the firing window's context. It keys off
//! `EventPattern::EnemyAttackDamagedSelf`, which pairs with `DamageAssigned`
//! narrowed to an enemy *attack* and scoped to a card the assignment gives
//! damage to, and reads the attacker from `EvalContext.attacking_enemy`, which
//! the window binds. (C5b #237.)

use card_dsl::dsl::{native, reaction_on_event, Ability, EventPattern, EventTiming};
use game_core::card_registry::NativeEffectFn;
use game_core::{deal_damage_to_enemy, Cx, EngineOutcome, EvalContext};

/// `ArkhamDB` code for Guard Dog (original-Core printing).
pub const CODE: &str = "01021";

const RETALIATE: &str = "01021:retaliate";

#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![reaction_on_event(
        EventPattern::EnemyAttackDamagedSelf,
        EventTiming::When,
        native(RETALIATE),
    )]
}

/// Resolve this card's native-effect tags. Wired into the crate registry's
/// `native_effect_for`.
pub(crate) fn native_effect_for(tag: &str) -> Option<NativeEffectFn> {
    match tag {
        RETALIATE => Some(retaliate as NativeEffectFn),
        _ => None,
    }
}

/// "Deal 1 damage to the attacking enemy." The attacker is bound on
/// `EvalContext.attacking_enemy` by the `DamageAssigned` window whose source is
/// an enemy attack — the only context that fires this ability. If it is
/// somehow absent we reject loudly (matching the card-effect error
/// policy) rather than panic.
fn retaliate(cx: &mut Cx, ctx: &EvalContext) -> EngineOutcome {
    let Some(enemy) = ctx.attacking_enemy() else {
        return EngineOutcome::Rejected {
            reason: "01021:retaliate fired without attacking_enemy bound — \
                     only an enemy-attack DamageAssigned window fires this \
                     ability"
                .into(),
        };
    };
    deal_damage_to_enemy(cx, enemy, 1, Some(ctx.controller));
    EngineOutcome::Done
}

#[cfg(test)]
mod tests {
    use super::*;
    use card_dsl::dsl::{Effect, Trigger};
    use game_core::event::Event;
    use game_core::state::{EnemyId, InvestigatorId};
    use game_core::test_support::{test_enemy, GameStateBuilder};

    fn cx_apply(
        state: &mut game_core::state::GameState,
        ctx: &EvalContext,
        f: NativeEffectFn,
    ) -> (EngineOutcome, Vec<Event>) {
        let mut events = Vec::new();
        let mut cx = Cx {
            state,
            events: &mut events,
        };
        let out = f(&mut cx, ctx);
        (out, events)
    }

    /// The declared cell against the printed word, the review step
    /// `docs/agents/standards.md` names: the module's verbatim text block above
    /// reads *"[reaction] **When** an enemy attack deals damage to Guard Dog"*,
    /// and `CONTEXT.md`'s **Timing cell** entry puts *"when"* in the interrupt
    /// cell — so `EventTiming::When`, on the `DamageAssigned` condition (#722).
    #[test]
    fn ability_is_one_when_reaction_native() {
        let abilities = abilities();
        assert_eq!(abilities.len(), 1);
        assert_eq!(
            abilities[0].trigger,
            Trigger::OnEvent {
                pattern: EventPattern::EnemyAttackDamagedSelf,
                timing: EventTiming::When,
                kind: card_dsl::dsl::TriggerKind::Reaction,
            }
        );
        assert!(matches!(&abilities[0].effect, Effect::Native { tag } if tag == RETALIATE));
    }

    #[test]
    fn native_effect_for_resolves_retaliate() {
        assert!(native_effect_for(RETALIATE).is_some());
        assert!(native_effect_for("01021:other").is_none());
    }

    /// Catches a `pub mod` rename or a fat-fingered match arm in
    /// `impls::abilities_for` — the registry must dispatch CODE here.
    #[test]
    fn registry_dispatches_to_this_modules_abilities() {
        assert_eq!(crate::abilities_for(CODE), Some(abilities()));
    }

    #[test]
    fn retaliate_deals_1_damage_to_bound_attacker() {
        let eid = EnemyId(7);
        let mut enemy = test_enemy(7, "Brute");
        enemy.max_health = 3;
        let mut state = GameStateBuilder::new().build();
        state.enemies.insert(eid, enemy);

        let mut ctx = EvalContext::for_controller(InvestigatorId(1));
        ctx.set_attacking_enemy(eid);

        let (out, events) = cx_apply(&mut state, &ctx, retaliate);
        assert_eq!(out, EngineOutcome::Done);
        assert_eq!(state.enemies[&eid].damage, 1, "attacker took 1 damage");
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::EnemyDamaged { enemy, amount: 1, .. } if *enemy == eid
            )),
            "EnemyDamaged emitted: {events:?}"
        );
    }

    #[test]
    fn retaliate_rejects_when_attacking_enemy_unbound() {
        let mut state = GameStateBuilder::new().build();
        let ctx = EvalContext::for_controller(InvestigatorId(1));
        let (out, events) = cx_apply(&mut state, &ctx, retaliate);
        assert!(
            matches!(out, EngineOutcome::Rejected { .. }),
            "unbound attacker rejects: {out:?}"
        );
        assert!(events.is_empty(), "rejection emits no events");
    }
}
