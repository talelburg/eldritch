//! Deduction (Seeker skill, 01039).
//!
//! ```text
//! Practiced.
//! 1 intellect icon.
//! If this skill test is successful while investigating a location,
//! discover 1 additional clue at that location.
//! ```
//!
//! The 1 intellect icon is part of the card's printed metadata
//! (`SkillIcons { intellect: 1, .. }`), not part of `abilities()` —
//! the icon-contribution path lives in the skill-test commit window
//! (`finish_skill_test` reads icons via the registry's
//! `metadata_for`), not through the DSL.
//!
//! `abilities()` describes only the triggered effect: a
//! [`Trigger::OnCommit`] running [`Effect::DiscoverAdditionalClues`], gated by
//! an [`if_`] over [`Condition::SkillTestKind`] of
//! [`SkillTestKind::Investigate`].
//!
//! **"1 additional clue" raises one discovery's count; it does not make a
//! second discovery.** The card's FAQ is explicit: *"The word 'Additional'
//! means 'in addition to other clues you discover', i.e. it modifies the number
//! of clues that you would find, it does not add an extra effect on top of any
//! other effects."* So the bonus rides
//! [`InFlightSkillTest::bonus_clues_discovered`] — accumulated here at commit,
//! read by the Investigate follow-up, which makes a single discovery of
//! `1 + bonus`. A second `DiscoverClue` would be observably wrong, not merely
//! differently shaped: Cover Up 01007 replaces *a discovery*, so two of 1
//! discard 2 clues from Cover Up where one of 2 (capped at the location's
//! clues) discards 1 — the bug #471 fixed. See the **Discovery** entry in
//! `CONTEXT.md`.
//!
//! The **"while investigating a location"** qualifier is the kind gate — the
//! same shape as Vicious Blow 01025's "during an attack". Gating on
//! [`SkillTestKind`] rather than leaning on the Investigate follow-up being the
//! accumulator's only reader keeps the bonus from leaking if a second reader is
//! added, and it is the card-facing vocabulary: Mind over Matter 01036 makes a
//! Fight test use intellect, which Deduction's intellect icon makes committable
//! — and that test is still a Fight, so no clue bonus applies.
//!
//! The **"if this skill test is successful"** qualifier stays intrinsic to
//! [`Effect::DiscoverAdditionalClues`]. `OnCommit` effects fire ungated on
//! outcome, but the accumulator's only reader is the Investigate follow-up,
//! which runs on success only — so on a failed test the bonus is accumulated,
//! never read, and thrown away with the test's frame. (Not "because the outcome
//! isn't known yet": post-#423 the `FireOnCommit` step runs *after*
//! `DetermineOutcome`, so it is on the frame. The reason it can't be
//! card-expressed is that `Condition::SkillTest { outcome }` is unimplemented —
//! it rejects with a TODO.)
//!
//! "At that location" is likewise intrinsic: the follow-up discovers at the
//! test's [`LocationTarget::TestedLocation`] snapshot, and the accumulator lives
//! on the in-flight test — which is why the FAQ's *"If you commit Deduction to
//! another player's investigation attempt, that other player would discover 1
//! additional clue, not you"* holds structurally. The bonus is a property of the
//! test, not of the committer.
//!
//! [`Trigger::OnCommit`]: card_dsl::dsl::Trigger::OnCommit
//! [`Effect::DiscoverAdditionalClues`]: card_dsl::dsl::Effect::DiscoverAdditionalClues
//! [`if_`]: card_dsl::dsl::if_
//! [`Condition::SkillTestKind`]: card_dsl::dsl::Condition::SkillTestKind
//! [`SkillTestKind`]: card_dsl::dsl::SkillTestKind
//! [`SkillTestKind::Investigate`]: card_dsl::dsl::SkillTestKind::Investigate
//! [`LocationTarget::TestedLocation`]: card_dsl::dsl::LocationTarget::TestedLocation
//! [`InFlightSkillTest::bonus_clues_discovered`]: game_core::state::InFlightSkillTest::bonus_clues_discovered

use card_dsl::dsl::{discover_additional_clues, if_, on_commit, Ability, Condition, SkillTestKind};

/// `ArkhamDB` code for the original-Core printing.
pub const CODE: &str = "01039";

/// On commit to an Investigate test, add +1 to the investigation's discovery
/// count (consumed by the Investigate follow-up, which discovers only on
/// success).
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![on_commit(if_(
        Condition::SkillTestKind(SkillTestKind::Investigate),
        discover_additional_clues(1),
    ))]
}

#[cfg(test)]
mod tests {
    use card_dsl::dsl::{Condition, Effect, SkillTestKind, Trigger};

    #[test]
    fn abilities_are_one_on_commit_clue_buff() {
        let abilities = super::abilities();
        assert_eq!(abilities.len(), 1);
        assert_eq!(abilities[0].trigger, Trigger::OnCommit);
        // If(SkillTestKind(Investigate), DiscoverAdditionalClues(1)) —
        // "while investigating a location".
        let Effect::If {
            condition, then, ..
        } = &abilities[0].effect
        else {
            panic!("expected Effect::If, got {:?}", abilities[0].effect);
        };
        assert_eq!(
            condition,
            &Condition::SkillTestKind(SkillTestKind::Investigate),
        );
        assert_eq!(**then, Effect::DiscoverAdditionalClues(1));
    }

    /// Catches a `pub mod` rename or a fat-fingered match arm in
    /// `impls::abilities_for` — the registry must dispatch CODE to
    /// this module's `abilities()`.
    #[test]
    fn registry_dispatches_to_this_modules_abilities() {
        assert_eq!(crate::abilities_for(super::CODE), Some(super::abilities()));
    }
}
