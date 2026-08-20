//! Silver Twilight Acolyte (The Gathering enemy, 01102).
//!
//! ```text
//! Prey - Bearer only.
//! Hunter.
//! Forced - After Silver Twilight Acolyte attacks: Place 1 doom on the current
//!   agenda.
//! ```
//!
//! The first card in the corpus to declare an ability on the **enemy attack**
//! triggering condition, which became one condition with three working cells in
//! #704. `Prey` and `Hunter` are printed keywords the pipeline ingests into
//! metadata, not abilities; only the forced doom is declared here.
//!
//! Card-local native (#276), sharing Ancient Evils 01166's shape: the effect is
//! one call into the engine's `place_doom_on_current_agenda` (place + threshold
//! check), so what the two cards duplicate is the tag and its shim, not the
//! logic. This is the second consumer, which is the repo's threshold for
//! graduating a pattern to a DSL primitive — filed as #716, where the cards that
//! need it *inside* a `Seq` or `ChooseOne` make the case.
//!
//! **Cell: the `after` cell of the `EnemyAttacks` condition.** The printed word
//! is *"After"* — *"**Forced** - After Silver Twilight Acolyte attacks: Place 1
//! doom on the current agenda."* — and `glossary/After.md` puts that
//! *"immediately after the specified timing point or triggering condition has
//! fully resolved."* So the attack has landed its damage and horror before the
//! doom goes down, and Dodge 01023's `when`-cell cancel gets in ahead of it —
//! which is what the ruling below turns on.
//!
//! **Dodging the attack suppresses this.** `data/arkhamdb-faq/core/01023.md`,
//! verbatim:
//!
//! > If the attacking enemy has a **Forced** ability that says "When attacks" or
//! > "After attacks", that ability does not trigger if an attack is Dodged.
//!
//! The engine gets that for free rather than by a carve-out here: a `when`-cell
//! cancel abandons the condition's whole sequence (#714), so the `after` cell
//! this ability sits in is never walked.

use card_dsl::dsl::{forced_on_event, native, Ability, EventPattern, EventTiming};
use game_core::card_registry::NativeEffectFn;
use game_core::{place_doom_on_current_agenda, Cx, EngineOutcome, EvalContext};

/// `ArkhamDB` code for Silver Twilight Acolyte.
pub const CODE: &str = "01102";

const PLACE_DOOM: &str = "01102:place-doom";

#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![forced_on_event(
        EventPattern::EnemyAttacks,
        EventTiming::After,
        native(PLACE_DOOM),
    )]
}

/// Resolve this enemy's native-effect tag. Wired into the crate registry's
/// `native_effect_for`.
pub(crate) fn native_effect_for(tag: &str) -> Option<NativeEffectFn> {
    (tag == PLACE_DOOM).then_some(place_doom as NativeEffectFn)
}

fn place_doom(cx: &mut Cx, _ctx: &EvalContext) -> EngineOutcome {
    place_doom_on_current_agenda(cx);
    EngineOutcome::Done
}

#[cfg(test)]
mod tests {
    use super::*;
    use card_dsl::dsl::{Effect, Trigger, TriggerKind};

    #[test]
    fn forced_after_it_attacks_places_doom() {
        let abilities = abilities();
        assert_eq!(abilities.len(), 1);
        assert_eq!(
            abilities[0].trigger,
            Trigger::OnEvent {
                pattern: EventPattern::EnemyAttacks,
                timing: EventTiming::After,
                kind: TriggerKind::Forced,
            },
            "the card prints `Forced - After … attacks`, so it declares the \
             attack condition's `after` cell"
        );
        assert!(matches!(&abilities[0].effect, Effect::Native { tag } if tag == PLACE_DOOM));
        assert!(native_effect_for(PLACE_DOOM).is_some());
        assert!(native_effect_for("nope").is_none());
    }
}
