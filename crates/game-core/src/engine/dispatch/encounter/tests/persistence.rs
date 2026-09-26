use card_dsl::dsl::{self, Ability, ModifierScope, Stat};

use super::*;

#[test]
fn persistence_is_derived_from_non_revelation_abilities() {
    let one_shot: Vec<Ability> = vec![dsl::revelation(dsl::native("x:rev"))];
    assert!(!treachery_is_persistent(&one_shot));

    let persistent: Vec<Ability> = vec![
        dsl::revelation(dsl::native("y:rev")),
        dsl::constant(dsl::modify(Stat::Willpower, 1, ModifierScope::WhileInPlay)),
    ];
    assert!(treachery_is_persistent(&persistent));
}
