//! Unexpected Courage (Neutral skill, 01093).
//!
//! ```text
//! Max 1 committed per skill test.
//! 2 wild icons.
//! ```
//!
//! A pure-icon skill: its whole effect is the 2 wild icons it contributes
//! when committed, which are printed metadata (`SkillIcons`), and the "Max
//! 1 committed per skill test" cap (`CardKind::Skill.commit_limit`,
//! enforced at the commit window, #311) — neither lives in `abilities()`.
//! It has **no triggered ability**, so `abilities()` is empty. (An empty
//! `abilities()` is still "implemented" — `is_playable` keys off the
//! presence of the impl, so the card passes the deck-import gate.)

use card_dsl::dsl::Ability;

/// `ArkhamDB` code for the original-Core printing.
pub const CODE: &str = "01093";

/// No triggered ability — the card is entirely its icons + commit cap.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    #[test]
    fn has_no_triggered_abilities() {
        assert!(super::abilities().is_empty());
    }

    #[test]
    fn registry_dispatches_to_this_modules_abilities() {
        assert_eq!(crate::abilities_for(super::CODE), Some(super::abilities()));
        assert!(crate::is_playable(super::CODE));
    }
}
