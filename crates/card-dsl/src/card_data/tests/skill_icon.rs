use super::*;

const ALL: [SkillKind; 4] = [
    SkillKind::Willpower,
    SkillKind::Intellect,
    SkillKind::Combat,
    SkillKind::Agility,
];

/// Only the tested skill's own icons are counted; wild is deliberately
/// left out, because ST.5's sum and ST.2's eligibility add it back
/// themselves.
#[test]
fn matching_counts_only_the_tested_skill_and_never_wild() {
    let icons = SkillIcons {
        willpower: 1,
        intellect: 2,
        combat: 3,
        agility: 4,
        wild: 5,
    };
    assert_eq!(icons.matching(SkillKind::Willpower), 1);
    assert_eq!(icons.matching(SkillKind::Intellect), 2);
    assert_eq!(icons.matching(SkillKind::Combat), 3);
    assert_eq!(icons.matching(SkillKind::Agility), 4);
}

/// The "non-matching icons contribute 0" arithmetic: Overpower's shape
/// (two [combat], no wild) counts zero against any other skill.
#[test]
fn matching_is_zero_for_every_skill_the_card_does_not_print() {
    let combat_only = SkillIcons {
        combat: 2,
        ..SkillIcons::default()
    };
    for skill in ALL {
        let expected = u8::from(skill == SkillKind::Combat) * 2;
        assert_eq!(combat_only.matching(skill), expected, "{skill:?}");
    }
}

/// RR ST.2: a matching icon is appropriate, and only for its own skill.
#[test]
fn a_matching_icon_is_appropriate_only_for_its_own_skill() {
    let combat_only = SkillIcons {
        combat: 2,
        ..SkillIcons::default()
    };
    for skill in ALL {
        assert_eq!(
            combat_only.appropriate_for(skill),
            skill == SkillKind::Combat,
            "{skill:?}",
        );
    }
}

/// RR ST.2 / glossary *Wild Skill Icons*: a wild icon is appropriate for
/// every skill, even with no printed icon of that skill.
#[test]
fn a_wild_icon_is_appropriate_for_every_skill() {
    let wild_only = SkillIcons {
        wild: 1,
        ..SkillIcons::default()
    };
    for skill in ALL {
        assert!(wild_only.appropriate_for(skill), "{skill:?}");
    }
}

/// A card with no icons at all — Emergency Cache's shape — is appropriate
/// for nothing, which is the free-discard hole ST.2 closes.
#[test]
fn no_icons_is_appropriate_for_nothing() {
    for skill in ALL {
        assert!(!SkillIcons::default().appropriate_for(skill), "{skill:?}");
    }
}
