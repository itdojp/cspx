use cspx_core::{Frontend, FrontendErrorKind, SimpleFrontend};

fn err_span(err: &cspx_core::FrontendError) -> cspx_core::SourceSpan {
    err.span.clone().expect("span")
}

#[test]
fn p000_minimal_valid_passes() {
    let input = r#"-- P000: minimal valid CSPM
channel a
P = a -> STOP
"#;
    let frontend = SimpleFrontend;
    let output = frontend
        .parse_and_typecheck(input, "model.cspm")
        .expect("parse_and_typecheck");
    assert_eq!(output.ir.channels.len(), 1);
    assert_eq!(output.ir.declarations.len(), 1);
    assert!(output.ir.entry.is_none());
}

#[test]
fn p001_syntax_error_is_invalid_input_with_span() {
    let input = r#"-- P001: syntax error (missing ->)
channel a
P = a STOP
"#;
    let frontend = SimpleFrontend;
    let err = match frontend.parse_and_typecheck(input, "model.cspm") {
        Ok(_) => panic!("expected error"),
        Err(err) => err,
    };
    assert_eq!(err.kind, FrontendErrorKind::InvalidInput);
    let span = err_span(&err);
    assert_eq!(span.start_line, 3);
    assert_eq!(span.start_col, 7);
}

#[test]
fn p002_undefined_process_is_invalid_input_with_span() {
    let input = r#"-- P002: undefined process name
channel a
P = a -> STOP
System = P ||| Q
"#;
    let frontend = SimpleFrontend;
    let err = match frontend.parse_and_typecheck(input, "model.cspm") {
        Ok(_) => panic!("expected error"),
        Err(err) => err,
    };
    assert_eq!(err.kind, FrontendErrorKind::InvalidInput);
    let span = err_span(&err);
    assert_eq!(span.start_line, 4);
    assert_eq!(span.start_col, 16);
}

#[test]
fn p003_payload_out_of_range_is_invalid_input_with_span() {
    let input = r#"-- P003: payload out of declared domain
channel ch : {0..1}
P = ch!2 -> STOP
"#;
    let frontend = SimpleFrontend;
    let err = match frontend.parse_and_typecheck(input, "model.cspm") {
        Ok(_) => panic!("expected error"),
        Err(err) => err,
    };
    assert_eq!(err.kind, FrontendErrorKind::InvalidInput);
    let span = err_span(&err);
    assert_eq!(span.start_line, 3);
    assert_eq!(span.start_col, 8);
}

#[test]
fn p004_datatype_is_unsupported_syntax_with_span() {
    let input = r#"-- P004: unsupported feature (datatype)
datatype Msg = A | B
channel ch : Msg
P = ch.A -> STOP
"#;
    let frontend = SimpleFrontend;
    let err = match frontend.parse_and_typecheck(input, "model.cspm") {
        Ok(_) => panic!("expected error"),
        Err(err) => err,
    };
    assert_eq!(err.kind, FrontendErrorKind::UnsupportedSyntax);
    let span = err_span(&err);
    assert_eq!(span.start_line, 2);
    assert_eq!(span.start_col, 1);
}
