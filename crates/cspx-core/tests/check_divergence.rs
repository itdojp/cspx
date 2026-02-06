use cspx_core::{CheckRequest, Checker, DivergenceChecker, Frontend, SimpleFrontend, Status};

#[test]
fn divergence_free_passes_without_tau_cycle() {
    let input = r#"channel a
P = a -> P
assert P :[divergence free [FD]]
"#;

    let frontend = SimpleFrontend;
    let module = frontend
        .parse_and_typecheck(input, "model.cspm")
        .expect("parse_and_typecheck")
        .ir;

    let checker = DivergenceChecker;
    let request = CheckRequest {
        command: cspx_core::check::CheckCommand::Check,
        model: None,
        target: Some("divergence free".to_string()),
    };
    let result = checker.check(&request, &module);
    assert_eq!(result.status, Status::Pass);
}

#[test]
fn divergence_by_hiding_fails_with_tau_trace() {
    let input = r#"channel a
Loop = a -> Loop
Div = Loop \\ {|a|}
assert Div :[divergence free [FD]]
"#;

    let frontend = SimpleFrontend;
    let module = frontend
        .parse_and_typecheck(input, "model.cspm")
        .expect("parse_and_typecheck")
        .ir;

    let checker = DivergenceChecker;
    let request = CheckRequest {
        command: cspx_core::check::CheckCommand::Check,
        model: None,
        target: Some("divergence free".to_string()),
    };
    let result = checker.check(&request, &module);
    assert_eq!(result.status, Status::Fail);

    let counterexample = result.counterexample.expect("counterexample");
    assert!(counterexample.tags.contains(&"divergence".to_string()));
    assert!(counterexample.tags.contains(&"kind:divergence".to_string()));
    assert!(counterexample.tags.contains(&"explained".to_string()));
    assert_eq!(counterexample.events.len(), 1);
    assert_eq!(counterexample.events[0].label, "tau");
}

#[test]
fn divergence_after_visible_prefix_includes_prefix_in_trace() {
    let input = r#"channel a
channel b
Loop = a -> Loop
Div = Loop \\ {|a|}
P = b -> Div
assert P :[divergence free [FD]]
"#;

    let frontend = SimpleFrontend;
    let module = frontend
        .parse_and_typecheck(input, "model.cspm")
        .expect("parse_and_typecheck")
        .ir;

    let checker = DivergenceChecker;
    let request = CheckRequest {
        command: cspx_core::check::CheckCommand::Check,
        model: None,
        target: Some("divergence free".to_string()),
    };
    let result = checker.check(&request, &module);
    assert_eq!(result.status, Status::Fail);

    let counterexample = result.counterexample.expect("counterexample");
    assert_eq!(counterexample.events.len(), 2);
    assert_eq!(counterexample.events[0].label, "b");
    assert_eq!(counterexample.events[1].label, "tau");
}
