use super::*;

#[test]
fn active_investigator_permits_only_named() {
    let scope = FastActorScope::ActiveInvestigator(InvestigatorId(1));
    assert!(scope.permits(InvestigatorId(1)));
    assert!(!scope.permits(InvestigatorId(2)));
}

#[test]
fn any_permits_everyone() {
    let scope = FastActorScope::Any;
    assert!(scope.permits(InvestigatorId(1)));
    assert!(scope.permits(InvestigatorId(42)));
}

#[test]
fn specific_permits_only_the_named_set() {
    let mut set = BTreeSet::new();
    set.insert(InvestigatorId(1));
    set.insert(InvestigatorId(3));
    let scope = FastActorScope::Specific(set);
    assert!(scope.permits(InvestigatorId(1)));
    assert!(!scope.permits(InvestigatorId(2)));
    assert!(scope.permits(InvestigatorId(3)));
}

#[test]
fn fast_actor_scope_serde_roundtrip() {
    let mut set = BTreeSet::new();
    set.insert(InvestigatorId(7));
    for scope in [
        FastActorScope::Any,
        FastActorScope::ActiveInvestigator(InvestigatorId(1)),
        FastActorScope::Specific(set),
    ] {
        let json = serde_json::to_string(&scope).expect("serialize");
        let back: FastActorScope = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, scope);
    }
}
