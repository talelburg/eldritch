//! On the Lam ("Skids" O'Toole signature event, 01010).
//!
//! ```text
//! "Skids" O'Toole deck only.
//! Fast. Play after your turn begins.
//! Until the end of the round, non-[[Elite]] enemies cannot attack you.
//! ```
//!
//! One play-timing trigger, as Dodge 01023's is: *"Play after your turn
//! begins"* names a timing point, and `glossary/Fast.md` reads one as a
//! triggering condition — *"If the instructions specify when/after a timing
//! point, the card may be played as if the described timing point were a
//! triggering condition for playing the card."* So the card is a reaction on
//! [`EventPattern::TurnBegan`] in its `after` cell, played from hand there:
//! the first opening of the post-2.2 player window, and not the window step
//! 2.2.1 returns to after each action. Being a reaction event, it is never
//! offered at the turn menu or in another player window.
//!
//! Its effect is a native: register a round-scoped
//! [`Restriction::CannotAttack`] protecting the controller, whose exemption is
//! the printed *"non-\[\[Elite\]\]"* (Barricade 01038's pattern — the engine
//! names no trait). The engine reads it where enemies are chosen to attack: an
//! attack of opportunity, the Enemy phase's attacks and a Retaliate each leave
//! a non-Elite enemy out, so it neither attacks nor exhausts. It expires at
//! the round end, before any "when the round ends" ability.
//!
//! A card-local native rather than a DSL effect: `Effect::Restrict` is a
//! constant marker the evaluator refuses at resolution, and On the Lam is the
//! one card registering a lasting restriction (`docs/agents/standards.md`'s
//! two-consumer threshold), so it calls the engine's exit directly, as Mind
//! over Matter 01036 does for its substitution.
//!
//! "Fast" comes from the corpus metadata (`is_fast`); the pipeline leaves
//! `play_only_during_turn` clear, since the clause is a timing point rather
//! than a period.
//!
//! [`EventPattern::TurnBegan`]: card_dsl::dsl::EventPattern::TurnBegan

use card_dsl::dsl::{self, Ability, CardFilter, EventPattern, EventTiming, Restriction};
use game_core::engine::evaluator::EvalContext;
use game_core::engine::{EngineOutcome, NativeCx};

use crate::generated::traits::ELITE;
use crate::impls::CardRecord;

/// `ArkhamDB` code for the original-Core printing.
pub const CODE: &str = "01010";

/// This card's registration, listed in [`ALL`](super::ALL).
pub const CARD: CardRecord = CardRecord::new(CODE, abilities).effects(&[(PROTECT, protect)]);

const PROTECT: &str = "01010:cannot-attack";

/// *"Play after your turn begins. / Until the end of the round, non-\[\[Elite\]\]
/// enemies cannot attack you."* — played after the controller's turn begins,
/// protecting them from non-Elite attacks for the round.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![dsl::reaction_on_event(
        EventPattern::TurnBegan,
        EventTiming::After,
        dsl::native(PROTECT),
    )]
}

/// Register the round-scoped non-Elite attack restriction for the controller.
fn protect(cx: &mut NativeCx, ctx: &EvalContext) -> EngineOutcome {
    cx.restrict_until_end_of_round(
        ctx.controller,
        Restriction::CannotAttack {
            exempt: Some(CardFilter::with_trait(ELITE)),
        },
    );
    EngineOutcome::Done
}

#[cfg(test)]
mod tests {
    use card_dsl::dsl::{Effect, EventPattern, EventTiming, Trigger, TriggerKind};

    #[test]
    fn ability_is_an_after_turn_began_reaction_native() {
        let a = super::abilities();
        assert_eq!(a.len(), 1);
        assert_eq!(
            a[0].trigger,
            Trigger::OnEvent {
                pattern: EventPattern::TurnBegan,
                timing: EventTiming::After,
                kind: TriggerKind::Reaction,
            },
        );
        assert!(matches!(&a[0].effect, Effect::Native { tag } if tag == super::PROTECT));
    }
}
