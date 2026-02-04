use crate::frontend::{Frontend, FrontendOutput};
use crate::ir::{Module, ProcessDecl, ProcessExpr, Spanned};
use crate::types::SourceSpan;
use std::collections::HashSet;
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrontendErrorKind {
    UnsupportedSyntax,
    InvalidInput,
}

#[derive(Debug, Clone)]
pub struct FrontendError {
    pub kind: FrontendErrorKind,
    pub message: String,
    pub span: Option<SourceSpan>,
}

impl Display for FrontendError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match &self.span {
            Some(span) => write!(
                f,
                "{}:{}:{}: {}",
                span.path, span.start_line, span.start_col, self.message
            ),
            None => write!(f, "{}", self.message),
        }
    }
}

impl std::error::Error for FrontendError {}

#[derive(Debug, Default)]
pub struct SimpleFrontend;

impl Frontend for SimpleFrontend {
    type Ir = Module;
    type Error = FrontendError;

    fn parse_and_typecheck(
        &self,
        input: &str,
        path: &str,
    ) -> Result<FrontendOutput<Self::Ir>, Self::Error> {
        let module = parse_module(input, path)?;

        if module.declarations.is_empty() && module.entry.is_none() {
            return Err(FrontendError {
                kind: FrontendErrorKind::InvalidInput,
                message: "empty input".to_string(),
                span: None,
            });
        }

        Ok(FrontendOutput {
            ir: module,
            diagnostics: Vec::new(),
        })
    }
}

fn parse_module(input: &str, path: &str) -> Result<Module, FrontendError> {
    let mut declarations = Vec::new();
    let mut entry = None;
    let mut names = HashSet::new();

    for (idx, line) in input.lines().enumerate() {
        let line_no = idx + 1;
        let no_comment = strip_comment(line);
        let trimmed = no_comment.trim();
        if trimmed.is_empty() {
            continue;
        }

        let span = span_for(path, line_no, line, trimmed);

        if let Some((left, right)) = split_assignment(trimmed) {
            let name = left.trim();
            if !is_identifier(name) {
                return Err(FrontendError {
                    kind: FrontendErrorKind::InvalidInput,
                    message: format!("invalid identifier: {name}"),
                    span,
                });
            }
            if !names.insert(name.to_string()) {
                return Err(FrontendError {
                    kind: FrontendErrorKind::InvalidInput,
                    message: format!("duplicate identifier: {name}"),
                    span,
                });
            }

            let expr = parse_expr(right.trim(), span.clone())?;
            declarations.push(ProcessDecl {
                name: name.to_string(),
                expr,
            });
            continue;
        }

        if trimmed == "STOP" {
            if entry.is_some() {
                return Err(FrontendError {
                    kind: FrontendErrorKind::InvalidInput,
                    message: "multiple top-level expressions".to_string(),
                    span,
                });
            }
            entry = Some(Spanned {
                value: ProcessExpr::Stop,
                span: span.unwrap_or_else(|| SourceSpan {
                    path: path.to_string(),
                    start_line: line_no as u32,
                    start_col: 1,
                    end_line: line_no as u32,
                    end_col: 1,
                }),
            });
            continue;
        }

        return Err(FrontendError {
            kind: FrontendErrorKind::UnsupportedSyntax,
            message: format!("unsupported syntax: {trimmed}"),
            span,
        });
    }

    Ok(Module {
        declarations,
        entry,
    })
}

fn parse_expr(expr: &str, span: Option<SourceSpan>) -> Result<Spanned<ProcessExpr>, FrontendError> {
    if expr == "STOP" {
        return Ok(Spanned {
            value: ProcessExpr::Stop,
            span: span.unwrap_or_else(|| SourceSpan {
                path: "UNKNOWN".to_string(),
                start_line: 1,
                start_col: 1,
                end_line: 1,
                end_col: 1,
            }),
        });
    }
    Err(FrontendError {
        kind: FrontendErrorKind::UnsupportedSyntax,
        message: format!("unsupported expression: {expr}"),
        span,
    })
}

fn split_assignment(line: &str) -> Option<(&str, &str)> {
    let mut parts = line.splitn(2, '=');
    let left = parts.next()?;
    let right = parts.next()?;
    Some((left, right))
}

fn strip_comment(line: &str) -> &str {
    if let Some(idx) = line.find("--") {
        &line[..idx]
    } else {
        line
    }
}

fn span_for(path: &str, line_no: usize, line: &str, trimmed: &str) -> Option<SourceSpan> {
    if trimmed.is_empty() {
        return None;
    }
    let start_idx = line.find(|c: char| !c.is_whitespace())?;
    let end_col = start_idx + trimmed.len();
    Some(SourceSpan {
        path: path.to_string(),
        start_line: line_no as u32,
        start_col: (start_idx + 1) as u32,
        end_line: line_no as u32,
        end_col: end_col as u32,
    })
}

fn is_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}
