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
//! Being that second consumer is what graduated *"place N doom on the current
//! agenda"* from a card-local native tag to
//! [`Effect::PlaceDoomOnCurrentAgenda`](card_dsl::dsl::Effect::PlaceDoomOnCurrentAgenda)
//! in #716 — the repo's two-consumer bar (`CLAUDE.md`, Architecture), met
//! against Ancient Evils 01166's byte-identical tag.
//!
//! **This card does not print the advance clause, so its doom cannot advance
//! the agenda.** Ancient Evils 01166 and Dark Memory 01013 print *"This effect
//! can cause the current agenda to advance."*; this card prints the placement
//! alone, and `data/rules-reference/rules/glossary/Doom.md` makes the omission
//! load-bearing:
//!
//! > Unless a card otherwise specifies that it can advance the agenda, this is
//! > the only time at which the agenda can advance.
//!
//! *"This"* being the Mythos phase's check-doom-threshold step. So an attack
//! that tips the agenda to its threshold leaves the doom sitting there until
//! Mythos step 1.3 — which is why this card builds with the bare
//! [`place_doom_on_current_agenda`] and 01166 with
//! [`place_doom_that_can_advance_the_agenda`](card_dsl::dsl::place_doom_that_can_advance_the_agenda).
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

use card_dsl::dsl::{
    forced_on_event, place_doom_on_current_agenda, Ability, EventPattern, EventTiming,
};

/// `ArkhamDB` code for Silver Twilight Acolyte.
pub const CODE: &str = "01102";

#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![forced_on_event(
        EventPattern::EnemyAttacks,
        EventTiming::After,
        place_doom_on_current_agenda(1u8),
    )]
}

#[cfg(test)]
mod tests {
    use super::*;
    use card_dsl::dsl::{Effect, IntExpr, Trigger, TriggerKind};

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
        assert_eq!(
            abilities[0].effect,
            Effect::PlaceDoomOnCurrentAgenda {
                count: IntExpr::Lit(1),
                may_advance: false,
            },
            "the card prints `Place 1 doom on the current agenda` and stops \
             there — no advance clause, so no threshold check"
        );
    }
}
