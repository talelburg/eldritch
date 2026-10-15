//! On the Lam ("Skids" O'Toole signature event, 01010).
//!
//! ```text
//! "Skids" O'Toole deck only.
//! Fast. Play after your turn begins.
//! Until the end of the round, non-[[Elite]] enemies cannot attack you.
//! ```
//!
//! One `OnPlay` native: register a round-scoped
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
//! "Fast" and "Play after your turn begins" come from the corpus metadata
//! (`is_fast`, and the clause ⇒ `CardMetadata::play_only_during_turn()`),
//! enforced by the play-card gate: the investigator's turn has begun once its
//! `InvestigatorTurnBegins` window opens, so the card is offered there and at
//! the turn menu, and nowhere outside that investigator's turn.

use card_dsl::dsl::{self, Ability, CardFilter, Restriction};
use game_core::engine::evaluator::EvalContext;
use game_core::engine::{EngineOutcome, NativeCx};

use crate::generated::traits::ELITE;
use crate::impls::CardRecord;

/// `ArkhamDB` code for the original-Core printing.
pub const CODE: &str = "01010";

/// This card's registration, listed in [`ALL`](super::ALL).
pub const CARD: CardRecord = CardRecord::new(CODE, abilities).effects(&[(PROTECT, protect)]);

const PROTECT: &str = "01010:cannot-attack";

/// `OnPlay`: protect the controller from non-Elite attacks for the round.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![dsl::on_play(dsl::native(PROTECT))]
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
    use card_dsl::dsl::{Effect, Trigger};

    #[test]
    fn ability_is_on_play_native() {
        let a = super::abilities();
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].trigger, Trigger::OnPlay);
        assert!(matches!(&a[0].effect, Effect::Native { tag } if tag == super::PROTECT));
    }
}
