//! Guard Dog (Guardian ally asset, 01021).
//!
//! ```text
//! [reaction] When an enemy attack deals damage to Guard Dog: Deal 1
//!   damage to the attacking enemy.
//! ```
//!
//! Health 3, sanity 1, Ally slot. As an ally with printed health/sanity,
//! Guard Dog is a soak container (`glossary/Dealing_Damage_Horror.md`: an
//! investigator *"may assign it to eligible asset cards he or she
//! controls"*): enemy-attack damage routes onto it before the controller.
//! The reaction below is
//! the *other* half — when that soak sends damage its way, it bites back.
//!
//! **Cell: the `when` cell of the `DamageDealt` condition**, which is the
//! word it prints. Dealing damage is two Rules Reference steps with a named
//! window between them (`glossary/Dealing_Damage_Horror.md`): damage is
//! **assigned** — tokens set *"next to the cards that will be taking the
//! damage/horror"* — and only then **placed**. *"When an enemy attack deals
//! damage to Guard Dog"* is the first of those, so the retaliate interrupts the
//! deal before any damage is on this card. That is what
//! `glossary/Nested_Sequences.md` works its example on, using this exact card:
//! *"**Before resolving** the damage dealt to the Guard Dog, 1 horror is dealt
//! to each investigator at the location"*. And it is what makes the card's own
//! ruling true — `data/arkhamdb-faq/core/01021.md`: *"You can use Guard Dog's
//! ability when you assign lethal damage/horror to it."* — since the Guard Dog
//! is still in play when a lethal assignment is announced. See
//! `docs/adr/0009-damage-is-assigned-then-placed.md`.
//!
//! The reaction is a card-local [`Effect::Native`](card_dsl::dsl::Effect::Native)
//! handler rather than a shared `Effect` variant: it names the attacking
//! enemy, which only exists in the firing window's context. It keys off
//! `EventPattern::EnemyAttackDamagedSelf`, which pairs with `DamageDealt`
//! narrowed to an enemy *attack* and scoped to a card the assignment gives
//! damage to, and reads the attacker from `EvalContext.attacking_enemy`, which
//! the window binds.

use card_dsl::dsl::{self, Ability, EventPattern, EventTiming};
use game_core::engine::evaluator::EvalContext;
use game_core::engine::{EngineOutcome, NativeCx};

use crate::impls::CardRecord;

/// `ArkhamDB` code for Guard Dog (original-Core printing).
pub const CODE: &str = "01021";

/// This card's registration, listed in [`ALL`](super::ALL).
pub const CARD: CardRecord = CardRecord::new(CODE, abilities).effects(&[(RETALIATE, retaliate)]);

const RETALIATE: &str = "01021:retaliate";

#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![dsl::reaction_on_event(
        EventPattern::EnemyAttackDamagedSelf,
        EventTiming::When,
        dsl::native(RETALIATE),
    )]
}

/// "Deal 1 damage to the attacking enemy." The attacker is bound on
/// `EvalContext.attacking_enemy` by the `DamageDealt` window whose source is
/// an enemy attack — the only context that fires this ability. If it is
/// somehow absent we reject loudly (matching the card-effect error
/// policy) rather than panic.
fn retaliate(cx: &mut NativeCx, ctx: &EvalContext) -> EngineOutcome {
    let Some(enemy) = ctx.attacking_enemy() else {
        return EngineOutcome::Rejected {
            reason: "01021:retaliate fired without attacking_enemy bound — \
                     only an enemy-attack DamageDealt window fires this \
                     ability"
                .into(),
        };
    };
    cx.deal_damage_to_enemy(enemy, 1, Some(ctx.controller));
    EngineOutcome::Done
}

#[cfg(test)]
mod tests {
    use card_dsl::dsl::{Effect, Trigger, TriggerKind};
    use game_core::event::Event;
    use game_core::state::{EnemyId, GameStateBuilder, InvestigatorId};
    use game_core::test_support;

    use super::*;

    /// The declared cell against the printed word, the review step
    /// `docs/agents/standards.md` names: the module's verbatim text block above
    /// reads *"\[reaction\] **When** an enemy attack deals damage to Guard Dog"*,
    /// and `GLOSSARY.md`'s **Timing cell** entry puts *"when"* in the interrupt
    /// cell — so `EventTiming::When`, on the `DamageDealt` condition.
    #[test]
    fn ability_is_one_when_reaction_native() {
        let abilities = abilities();
        assert_eq!(abilities.len(), 1);
        assert_eq!(
            abilities[0].trigger,
            Trigger::OnEvent {
                pattern: EventPattern::EnemyAttackDamagedSelf,
                timing: EventTiming::When,
                kind: TriggerKind::Reaction,
            }
        );
        assert!(matches!(&abilities[0].effect, Effect::Native { tag } if tag == RETALIATE));
    }

    #[test]
    fn retaliate_deals_1_damage_to_bound_attacker() {
        let eid = EnemyId(7);
        let mut state = GameStateBuilder::new()
            .with_enemy(test_support::enemy(eid).health(3))
            .build();

        let mut ctx = EvalContext::for_controller(InvestigatorId(1));
        ctx.set_attacking_enemy(eid);

        let (out, events) = test_support::run_native(&mut state, &crate::REGISTRY, retaliate, &ctx);
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
        let (out, events) = test_support::run_native(&mut state, &crate::REGISTRY, retaliate, &ctx);
        assert!(
            matches!(out, EngineOutcome::Rejected { .. }),
            "unbound attacker rejects: {out:?}"
        );
        assert!(events.is_empty(), "rejection emits no events");
    }
}
