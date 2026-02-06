use cspx_core::{CheckRequest, Checker, DeadlockChecker, Frontend, SimpleFrontend, Status};

#[test]
fn deadlock_selects_last_deadlock_free_assert_target() {
    let input = r#"-- P104: components are deadlock-free, but composition deadlocks
channel a
channel b
P = a -> P
Q = b -> Q
System = P [|{|a,b|}|] Q
assert P :[deadlock free [F]]
assert Q :[deadlock free [F]]
assert System :[deadlock free [F]]
"#;

    let frontend = SimpleFrontend;
    let module = frontend
        .parse_and_typecheck(input, "model.cspm")
        .expect("parse_and_typecheck")
        .ir;

    let checker = DeadlockChecker;
    let request = CheckRequest {
        command: cspx_core::check::CheckCommand::Check,
        model: None,
        target: Some("deadlock free".to_string()),
    };
    let result = checker.check(&request, &module);
    assert_eq!(result.status, Status::Fail);

    let counterexample = result.counterexample.expect("counterexample");
    assert_eq!(counterexample.source_spans.len(), 1);
    assert_eq!(counterexample.source_spans[0].start_line, 6);
}
