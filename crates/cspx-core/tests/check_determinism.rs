use cspx_core::{CheckRequest, Checker, DeterminismChecker, Frontend, SimpleFrontend, Status};

#[test]
fn deterministic_passes_for_single_visible_choice() {
    let input = r#"channel a
P = a -> P
assert P :[deterministic [FD]]
"#;

    let frontend = SimpleFrontend;
    let module = frontend
        .parse_and_typecheck(input, "model.cspm")
        .expect("parse_and_typecheck")
        .ir;

    let checker = DeterminismChecker;
    let request = CheckRequest {
        command: cspx_core::check::CheckCommand::Check,
        model: None,
        target: Some("deterministic".to_string()),
    };
    let result = checker.check(&request, &module);
    assert_eq!(result.status, Status::Pass);
}

#[test]
fn nondeterminism_by_internal_choice_fails_with_branch_label() {
    let input = r#"channel a
channel b
P = (a -> STOP) |~| (a -> b -> STOP)
assert P :[deterministic [FD]]
"#;

    let frontend = SimpleFrontend;
    let module = frontend
        .parse_and_typecheck(input, "model.cspm")
        .expect("parse_and_typecheck")
        .ir;

    let checker = DeterminismChecker;
    let request = CheckRequest {
        command: cspx_core::check::CheckCommand::Check,
        model: None,
        target: Some("deterministic".to_string()),
    };
    let result = checker.check(&request, &module);
    assert_eq!(result.status, Status::Fail);

    let counterexample = result.counterexample.expect("counterexample");
    assert!(counterexample.tags.contains(&"nondeterminism".to_string()));
    assert!(counterexample
        .tags
        .contains(&"kind:nondeterminism".to_string()));
    assert!(counterexample.tags.contains(&"explained".to_string()));
    assert_eq!(counterexample.events.len(), 1);
    assert_eq!(counterexample.events[0].label, "a");
}
