use cspx_core::{
    check::CheckCommand, check::RefinementModel, CheckRequest, Checker, RefinementChecker,
    RefinementInput,
};

fn spanned<T>(value: T, path: &str) -> cspx_core::ir::Spanned<T> {
    cspx_core::ir::Spanned {
        value,
        span: cspx_core::types::SourceSpan {
            path: path.to_string(),
            start_line: 1,
            start_col: 1,
            end_line: 1,
            end_col: 1,
        },
    }
}

fn unit_channel(name: &str, path: &str) -> cspx_core::ir::ChannelDecl {
    cspx_core::ir::ChannelDecl {
        names: vec![spanned(name.to_string(), path)],
        domain: None,
    }
}

fn stop(path: &str) -> cspx_core::ir::Spanned<cspx_core::ir::ProcessExpr> {
    spanned(cspx_core::ir::ProcessExpr::Stop, path)
}

fn prefix(
    channel: &str,
    next: cspx_core::ir::Spanned<cspx_core::ir::ProcessExpr>,
    path: &str,
) -> cspx_core::ir::Spanned<cspx_core::ir::ProcessExpr> {
    spanned(
        cspx_core::ir::ProcessExpr::Prefix {
            event: spanned(
                cspx_core::ir::Event {
                    channel: spanned(channel.to_string(), path),
                    seg: None,
                },
                path,
            ),
            next: Box::new(next),
        },
        path,
    )
}

fn ref_proc(name: &str, path: &str) -> cspx_core::ir::Spanned<cspx_core::ir::ProcessExpr> {
    spanned(
        cspx_core::ir::ProcessExpr::Ref(spanned(name.to_string(), path)),
        path,
    )
}

fn hide(
    inner: cspx_core::ir::Spanned<cspx_core::ir::ProcessExpr>,
    channels: &[&str],
    path: &str,
) -> cspx_core::ir::Spanned<cspx_core::ir::ProcessExpr> {
    spanned(
        cspx_core::ir::ProcessExpr::Hide {
            inner: Box::new(inner),
            hide: cspx_core::ir::EventSet {
                channels: channels
                    .iter()
                    .map(|&ch| spanned(ch.to_string(), path))
                    .collect(),
            },
        },
        path,
    )
}

fn choice(
    kind: cspx_core::ir::ChoiceKind,
    left: cspx_core::ir::Spanned<cspx_core::ir::ProcessExpr>,
    right: cspx_core::ir::Spanned<cspx_core::ir::ProcessExpr>,
    path: &str,
) -> cspx_core::ir::Spanned<cspx_core::ir::ProcessExpr> {
    spanned(
        cspx_core::ir::ProcessExpr::Choice {
            kind,
            left: Box::new(left),
            right: Box::new(right),
        },
        path,
    )
}

fn single_process_module(
    process_name: &str,
    expr: cspx_core::ir::Spanned<cspx_core::ir::ProcessExpr>,
    channels: Vec<cspx_core::ir::ChannelDecl>,
    path: &str,
) -> cspx_core::ir::Module {
    cspx_core::ir::Module {
        channels,
        declarations: vec![cspx_core::ir::ProcessDecl {
            name: spanned(process_name.to_string(), path),
            expr,
        }],
        assertions: Vec::new(),
        entry: None,
    }
}

#[test]
fn refinement_mismatch_fails() {
    let spec = cspx_core::ir::Module {
        channels: Vec::new(),
        declarations: vec![cspx_core::ir::ProcessDecl {
            name: cspx_core::ir::Spanned {
                value: "SPEC".to_string(),
                span: cspx_core::types::SourceSpan {
                    path: "spec.cspm".to_string(),
                    start_line: 1,
                    start_col: 1,
                    end_line: 1,
                    end_col: 4,
                },
            },
            expr: cspx_core::ir::Spanned {
                value: cspx_core::ir::ProcessExpr::Stop,
                span: cspx_core::types::SourceSpan {
                    path: "spec.cspm".to_string(),
                    start_line: 1,
                    start_col: 1,
                    end_line: 1,
                    end_col: 4,
                },
            },
        }],
        assertions: Vec::new(),
        entry: None,
    };
    let impl_ = cspx_core::ir::Module {
        channels: Vec::new(),
        declarations: vec![
            cspx_core::ir::ProcessDecl {
                name: cspx_core::ir::Spanned {
                    value: "IMPL".to_string(),
                    span: cspx_core::types::SourceSpan {
                        path: "impl.cspm".to_string(),
                        start_line: 1,
                        start_col: 1,
                        end_line: 1,
                        end_col: 4,
                    },
                },
                expr: cspx_core::ir::Spanned {
                    value: cspx_core::ir::ProcessExpr::Stop,
                    span: cspx_core::types::SourceSpan {
                        path: "impl.cspm".to_string(),
                        start_line: 1,
                        start_col: 1,
                        end_line: 1,
                        end_col: 4,
                    },
                },
            },
            cspx_core::ir::ProcessDecl {
                name: cspx_core::ir::Spanned {
                    value: "IMPL2".to_string(),
                    span: cspx_core::types::SourceSpan {
                        path: "impl.cspm".to_string(),
                        start_line: 2,
                        start_col: 1,
                        end_line: 2,
                        end_col: 5,
                    },
                },
                expr: cspx_core::ir::Spanned {
                    value: cspx_core::ir::ProcessExpr::Stop,
                    span: cspx_core::types::SourceSpan {
                        path: "impl.cspm".to_string(),
                        start_line: 2,
                        start_col: 1,
                        end_line: 2,
                        end_col: 4,
                    },
                },
            },
        ],
        assertions: Vec::new(),
        entry: None,
    };

    let checker = RefinementChecker;
    let request = CheckRequest {
        command: CheckCommand::Refine,
        model: Some(RefinementModel::T),
        target: Some("spec impl".to_string()),
    };
    let input = RefinementInput { spec, impl_ };
    let result = checker.check(&request, &input);

    assert_eq!(result.status, cspx_core::types::Status::Error);
    assert!(result.counterexample.is_none());
    let reason = result.reason.expect("reason");
    assert_eq!(reason.kind, cspx_core::types::ReasonKind::InvalidInput);
}

#[test]
fn traces_refinement_passes_on_subset() {
    let spec_path = "spec.cspm";
    let impl_path = "impl.cspm";

    let spec_expr = choice(
        cspx_core::ir::ChoiceKind::External,
        prefix("a", stop(spec_path), spec_path),
        prefix("b", stop(spec_path), spec_path),
        spec_path,
    );
    let impl_expr = prefix("a", stop(impl_path), impl_path);

    let spec = single_process_module(
        "SPEC",
        spec_expr,
        vec![unit_channel("a", spec_path), unit_channel("b", spec_path)],
        spec_path,
    );
    let impl_ = single_process_module(
        "IMPL",
        impl_expr,
        vec![unit_channel("a", impl_path), unit_channel("b", impl_path)],
        impl_path,
    );

    let checker = RefinementChecker;
    let request = CheckRequest {
        command: CheckCommand::Refine,
        model: Some(RefinementModel::T),
        target: Some("spec impl".to_string()),
    };
    let input = RefinementInput { spec, impl_ };
    let result = checker.check(&request, &input);

    assert_eq!(result.status, cspx_core::types::Status::Pass);
    assert!(result.counterexample.is_none());
}

#[test]
fn traces_refinement_fails_on_extra_visible_event() {
    let spec_path = "spec.cspm";
    let impl_path = "impl.cspm";

    let spec_expr = prefix("a", stop(spec_path), spec_path);
    let impl_expr = choice(
        cspx_core::ir::ChoiceKind::External,
        prefix("a", stop(impl_path), impl_path),
        prefix("b", stop(impl_path), impl_path),
        impl_path,
    );

    let spec = single_process_module(
        "SPEC",
        spec_expr,
        vec![unit_channel("a", spec_path), unit_channel("b", spec_path)],
        spec_path,
    );
    let impl_ = single_process_module(
        "IMPL",
        impl_expr,
        vec![unit_channel("a", impl_path), unit_channel("b", impl_path)],
        impl_path,
    );

    let checker = RefinementChecker;
    let request = CheckRequest {
        command: CheckCommand::Refine,
        model: Some(RefinementModel::T),
        target: Some("spec impl".to_string()),
    };
    let input = RefinementInput { spec, impl_ };
    let result = checker.check(&request, &input);

    assert_eq!(result.status, cspx_core::types::Status::Fail);
    let counterexample = result.counterexample.expect("counterexample");
    assert_eq!(counterexample.events.len(), 1);
    assert_eq!(counterexample.events[0].label, "b");
    assert!(counterexample.is_minimized);
}

#[test]
fn traces_refinement_handles_tau_closure() {
    let spec_path = "spec.cspm";
    let impl_path = "impl.cspm";

    // spec: a -> STOP
    let spec_expr = prefix("a", stop(spec_path), spec_path);
    // impl: (a -> STOP) |~| (b -> STOP)  (internal choice -> tau -> ...)
    let impl_expr = choice(
        cspx_core::ir::ChoiceKind::Internal,
        prefix("a", stop(impl_path), impl_path),
        prefix("b", stop(impl_path), impl_path),
        impl_path,
    );

    let spec = single_process_module(
        "SPEC",
        spec_expr,
        vec![unit_channel("a", spec_path), unit_channel("b", spec_path)],
        spec_path,
    );
    let impl_ = single_process_module(
        "IMPL",
        impl_expr,
        vec![unit_channel("a", impl_path), unit_channel("b", impl_path)],
        impl_path,
    );

    let checker = RefinementChecker;
    let request = CheckRequest {
        command: CheckCommand::Refine,
        model: Some(RefinementModel::T),
        target: Some("spec impl".to_string()),
    };
    let input = RefinementInput { spec, impl_ };
    let result = checker.check(&request, &input);

    assert_eq!(result.status, cspx_core::types::Status::Fail);
    let counterexample = result.counterexample.expect("counterexample");
    assert_eq!(counterexample.events.len(), 1);
    assert_eq!(counterexample.events[0].label, "b");
    assert!(counterexample.is_minimized);
}

#[test]
fn failures_refinement_passes_on_identical_offers() {
    let spec_path = "spec.cspm";
    let impl_path = "impl.cspm";

    let spec_expr = choice(
        cspx_core::ir::ChoiceKind::External,
        prefix("a", stop(spec_path), spec_path),
        prefix("b", stop(spec_path), spec_path),
        spec_path,
    );
    let impl_expr = choice(
        cspx_core::ir::ChoiceKind::External,
        prefix("a", stop(impl_path), impl_path),
        prefix("b", stop(impl_path), impl_path),
        impl_path,
    );

    let spec = single_process_module(
        "SPEC",
        spec_expr,
        vec![unit_channel("a", spec_path), unit_channel("b", spec_path)],
        spec_path,
    );
    let impl_ = single_process_module(
        "IMPL",
        impl_expr,
        vec![unit_channel("a", impl_path), unit_channel("b", impl_path)],
        impl_path,
    );

    let checker = RefinementChecker;
    let request = CheckRequest {
        command: CheckCommand::Refine,
        model: Some(RefinementModel::F),
        target: Some("spec impl".to_string()),
    };
    let input = RefinementInput { spec, impl_ };
    let result = checker.check(&request, &input);

    assert_eq!(result.status, cspx_core::types::Status::Pass);
    assert!(result.counterexample.is_none());
}

#[test]
fn failures_refinement_fails_on_refusal_mismatch() {
    let spec_path = "spec.cspm";
    let impl_path = "impl.cspm";

    let spec_expr = choice(
        cspx_core::ir::ChoiceKind::External,
        prefix("a", stop(spec_path), spec_path),
        prefix("b", stop(spec_path), spec_path),
        spec_path,
    );
    let impl_expr = prefix("a", stop(impl_path), impl_path);

    let spec = single_process_module(
        "SPEC",
        spec_expr,
        vec![unit_channel("a", spec_path), unit_channel("b", spec_path)],
        spec_path,
    );
    let impl_ = single_process_module(
        "IMPL",
        impl_expr,
        vec![unit_channel("a", impl_path), unit_channel("b", impl_path)],
        impl_path,
    );

    let checker = RefinementChecker;
    let request = CheckRequest {
        command: CheckCommand::Refine,
        model: Some(RefinementModel::F),
        target: Some("spec impl".to_string()),
    };
    let input = RefinementInput { spec, impl_ };
    let result = checker.check(&request, &input);

    assert_eq!(result.status, cspx_core::types::Status::Fail);
    let counterexample = result.counterexample.expect("counterexample");
    assert_eq!(counterexample.events.len(), 0);
    assert!(counterexample.is_minimized);
    assert!(counterexample.tags.iter().any(|t| t == "refusal_mismatch"));
    assert!(counterexample.tags.iter().any(|t| t == "refuse:b"));
}

#[test]
fn failures_divergences_refinement_passes_on_stop() {
    let spec_path = "spec.cspm";
    let impl_path = "impl.cspm";

    let spec = single_process_module(
        "SPEC",
        stop(spec_path),
        vec![unit_channel("a", spec_path)],
        spec_path,
    );
    let impl_ = single_process_module(
        "IMPL",
        stop(impl_path),
        vec![unit_channel("a", impl_path)],
        impl_path,
    );

    let checker = RefinementChecker;
    let request = CheckRequest {
        command: CheckCommand::Refine,
        model: Some(RefinementModel::FD),
        target: Some("spec impl".to_string()),
    };
    let input = RefinementInput { spec, impl_ };
    let result = checker.check(&request, &input);

    assert_eq!(result.status, cspx_core::types::Status::Pass);
    assert!(result.counterexample.is_none());
}

#[test]
fn failures_divergences_refinement_fails_on_impl_divergence() {
    let spec_path = "spec.cspm";
    let impl_path = "impl.cspm";

    let spec = single_process_module(
        "SPEC",
        stop(spec_path),
        vec![unit_channel("a", spec_path)],
        spec_path,
    );
    let impl_expr = hide(
        prefix("a", ref_proc("IMPL", impl_path), impl_path),
        &["a"],
        impl_path,
    );
    let impl_ = single_process_module(
        "IMPL",
        impl_expr,
        vec![unit_channel("a", impl_path)],
        impl_path,
    );

    let checker = RefinementChecker;
    let request = CheckRequest {
        command: CheckCommand::Refine,
        model: Some(RefinementModel::FD),
        target: Some("spec impl".to_string()),
    };
    let input = RefinementInput { spec, impl_ };
    let result = checker.check(&request, &input);

    assert_eq!(result.status, cspx_core::types::Status::Fail);
    let counterexample = result.counterexample.expect("counterexample");
    assert!(counterexample.events.iter().any(|e| e.label == "tau"));
    assert!(counterexample.is_minimized);
    assert!(counterexample
        .tags
        .iter()
        .any(|t| t == "divergence_mismatch"));
    assert!(counterexample.tags.iter().any(|t| t == "divergence"));
}
