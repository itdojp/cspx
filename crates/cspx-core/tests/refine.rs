use cspx_core::{
    check::CheckCommand, check::RefinementModel, CheckRequest, Checker, RefinementChecker,
    RefinementInput,
};

#[test]
fn refinement_mismatch_fails() {
    let spec = cspx_core::ir::Module {
        declarations: vec![cspx_core::ir::ProcessDecl {
            name: "SPEC".to_string(),
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
        entry: None,
    };
    let impl_ = cspx_core::ir::Module {
        declarations: vec![
            cspx_core::ir::ProcessDecl {
                name: "IMPL".to_string(),
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
                name: "IMPL2".to_string(),
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
        entry: None,
    };

    let checker = RefinementChecker::default();
    let request = CheckRequest {
        command: CheckCommand::Refine,
        model: Some(RefinementModel::T),
        target: Some("spec impl".to_string()),
    };
    let input = RefinementInput { spec, impl_ };
    let result = checker.check(&request, &input);

    assert_eq!(result.status, cspx_core::types::Status::Fail);
    assert!(result.counterexample.is_some());
}
