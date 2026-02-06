use crate::frontend::{Frontend, FrontendOutput};
use crate::ir::{
    AssertionDecl, ChannelDecl, ChannelDomain, ChoiceKind, Event, EventInput, EventSeg, EventSet,
    EventValue, Module, ParallelKind, ProcessDecl, ProcessExpr, PropertyKind, PropertyModel,
    RefinementOp, Spanned,
};
use crate::types::SourceSpan;
use std::collections::{HashMap, HashSet};
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
        let module = parse_and_typecheck_module(input, path)?;

        Ok(FrontendOutput {
            ir: module,
            diagnostics: Vec::new(),
        })
    }
}

fn parse_and_typecheck_module(input: &str, path: &str) -> Result<Module, FrontendError> {
    let tokens = lex(input, path)?;
    let mut parser = Parser::new(tokens, path);
    let parsed = parser.parse_module()?;

    if parsed.channels.is_empty()
        && parsed.declarations.is_empty()
        && parsed.assertions.is_empty()
        && parsed.entry.is_none()
    {
        return Err(FrontendError {
            kind: FrontendErrorKind::InvalidInput,
            message: "empty input".to_string(),
            span: None,
        });
    }

    typecheck_module(parsed)
}

fn typecheck_module(parsed: ParsedModule) -> Result<Module, FrontendError> {
    if let Some(unsupported) = parsed.unsupported.first() {
        return Err(FrontendError {
            kind: FrontendErrorKind::UnsupportedSyntax,
            message: unsupported.message.clone(),
            span: Some(unsupported.span.clone()),
        });
    }

    let mut channels = HashMap::<String, ChannelType>::new();
    for decl in &parsed.channels {
        let domain_type = match &decl.domain {
            None => ChannelType::Unit,
            Some(domain) => match &domain.value {
                ChannelDomain::IntRange { min, max } => {
                    if min.value > max.value {
                        return Err(FrontendError {
                            kind: FrontendErrorKind::InvalidInput,
                            message: format!(
                                "invalid channel domain: {}..{}",
                                min.value, max.value
                            ),
                            span: Some(domain.span.clone()),
                        });
                    }
                    ChannelType::IntRange {
                        min: min.value,
                        max: max.value,
                    }
                }
                ChannelDomain::NamedType(name) => {
                    return Err(FrontendError {
                        kind: FrontendErrorKind::UnsupportedSyntax,
                        message: format!("unsupported channel domain type: {}", name.value),
                        span: Some(name.span.clone()),
                    });
                }
            },
        };

        for name in &decl.names {
            let key = &name.value;
            if channels.contains_key(key) {
                return Err(FrontendError {
                    kind: FrontendErrorKind::InvalidInput,
                    message: format!("duplicate channel: {key}"),
                    span: Some(name.span.clone()),
                });
            }
            channels.insert(key.clone(), domain_type);
        }
    }

    let mut processes = HashSet::<String>::new();
    for decl in &parsed.declarations {
        let key = &decl.name.value;
        if !processes.insert(key.clone()) {
            return Err(FrontendError {
                kind: FrontendErrorKind::InvalidInput,
                message: format!("duplicate process: {key}"),
                span: Some(decl.name.span.clone()),
            });
        }
    }

    let empty_vars = HashMap::<String, ChannelType>::new();
    for decl in &parsed.declarations {
        typecheck_process_expr(&decl.expr, &channels, &processes, &empty_vars)?;
    }
    if let Some(entry) = &parsed.entry {
        typecheck_process_expr(entry, &channels, &processes, &empty_vars)?;
    }

    for assertion in &parsed.assertions {
        match assertion {
            AssertionDecl::Property { target, .. } => {
                if !processes.contains(&target.value) {
                    return Err(FrontendError {
                        kind: FrontendErrorKind::InvalidInput,
                        message: format!("undefined process: {}", target.value),
                        span: Some(target.span.clone()),
                    });
                }
            }
            AssertionDecl::Refinement { spec, impl_, .. } => {
                for proc_ref in [spec, impl_] {
                    if !processes.contains(&proc_ref.value) {
                        return Err(FrontendError {
                            kind: FrontendErrorKind::InvalidInput,
                            message: format!("undefined process: {}", proc_ref.value),
                            span: Some(proc_ref.span.clone()),
                        });
                    }
                }
            }
        }
    }

    Ok(Module {
        channels: parsed.channels,
        declarations: parsed.declarations,
        assertions: parsed.assertions,
        entry: parsed.entry,
    })
}

fn typecheck_process_expr(
    expr: &Spanned<ProcessExpr>,
    channels: &HashMap<String, ChannelType>,
    processes: &HashSet<String>,
    vars: &HashMap<String, ChannelType>,
) -> Result<(), FrontendError> {
    match &expr.value {
        ProcessExpr::Stop => Ok(()),
        ProcessExpr::Ref(name) => {
            if !processes.contains(&name.value) {
                return Err(FrontendError {
                    kind: FrontendErrorKind::InvalidInput,
                    message: format!("undefined process: {}", name.value),
                    span: Some(name.span.clone()),
                });
            }
            Ok(())
        }
        ProcessExpr::Prefix { event, next } => {
            let mut vars = vars.clone();
            typecheck_event(event, channels, &mut vars)?;
            typecheck_process_expr(next, channels, processes, &vars)
        }
        ProcessExpr::Choice { left, right, .. } => {
            typecheck_process_expr(left, channels, processes, vars)?;
            typecheck_process_expr(right, channels, processes, vars)?;
            Ok(())
        }
        ProcessExpr::Parallel {
            left, right, sync, ..
        } => {
            typecheck_process_expr(left, channels, processes, vars)?;
            typecheck_process_expr(right, channels, processes, vars)?;
            if let Some(sync) = sync {
                typecheck_event_set(sync, channels)?;
            }
            Ok(())
        }
        ProcessExpr::Hide { inner, hide } => {
            typecheck_process_expr(inner, channels, processes, vars)?;
            typecheck_event_set(hide, channels)?;
            Ok(())
        }
    }
}

fn typecheck_event(
    event: &Spanned<Event>,
    channels: &HashMap<String, ChannelType>,
    vars: &mut HashMap<String, ChannelType>,
) -> Result<(), FrontendError> {
    let channel_name = &event.value.channel.value;
    let Some(channel_ty) = channels.get(channel_name) else {
        return Err(FrontendError {
            kind: FrontendErrorKind::InvalidInput,
            message: format!("undefined channel: {channel_name}"),
            span: Some(event.value.channel.span.clone()),
        });
    };

    match (channel_ty, &event.value.seg) {
        (ChannelType::Unit, None) => Ok(()),
        (ChannelType::Unit, Some(seg)) => {
            let span = match seg {
                EventSeg::Dot(value) => value.span.clone(),
                EventSeg::Out(value) => value.span.clone(),
                EventSeg::In(value) => value.span.clone(),
            };
            Err(FrontendError {
                kind: FrontendErrorKind::InvalidInput,
                message: format!("channel does not take payload: {channel_name}"),
                span: Some(span),
            })
        }
        (ChannelType::IntRange { .. }, None) => Err(FrontendError {
            kind: FrontendErrorKind::InvalidInput,
            message: format!("missing payload for channel: {channel_name}"),
            span: Some(event.value.channel.span.clone()),
        }),
        (ChannelType::IntRange { min, max }, Some(EventSeg::Dot(value))) => match &value.value {
            EventValue::Int(n) => check_int_domain(*n, *min, *max, &value.span),
            EventValue::Ident(_) => Err(FrontendError {
                kind: FrontendErrorKind::UnsupportedSyntax,
                message: "unsupported dot payload (identifier)".to_string(),
                span: Some(value.span.clone()),
            }),
        },
        (ChannelType::IntRange { min, max }, Some(EventSeg::Out(value))) => match &value.value {
            EventValue::Int(n) => check_int_domain(*n, *min, *max, &value.span),
            EventValue::Ident(name) => {
                let Some(var_ty) = vars.get(name) else {
                    return Err(FrontendError {
                        kind: FrontendErrorKind::InvalidInput,
                        message: format!("undefined variable: {name}"),
                        span: Some(value.span.clone()),
                    });
                };
                if var_ty != channel_ty {
                    return Err(FrontendError {
                        kind: FrontendErrorKind::InvalidInput,
                        message: format!("variable domain mismatch: {name}"),
                        span: Some(value.span.clone()),
                    });
                }
                Ok(())
            }
        },
        (ChannelType::IntRange { min, max }, Some(EventSeg::In(input))) => match &input.value {
            EventInput::Int(n) => check_int_domain(*n, *min, *max, &input.span),
            EventInput::Bind(name) => {
                if vars.contains_key(name) {
                    return Err(FrontendError {
                        kind: FrontendErrorKind::InvalidInput,
                        message: format!("duplicate variable binding: {name}"),
                        span: Some(input.span.clone()),
                    });
                }
                vars.insert(name.clone(), *channel_ty);
                Ok(())
            }
        },
    }
}

fn check_int_domain(
    value: u64,
    min: u64,
    max: u64,
    span: &SourceSpan,
) -> Result<(), FrontendError> {
    if value < min || value > max {
        return Err(FrontendError {
            kind: FrontendErrorKind::InvalidInput,
            message: format!("payload out of range: {value} (expected {min}..{max})"),
            span: Some(span.clone()),
        });
    }
    Ok(())
}

fn typecheck_event_set(
    set: &EventSet,
    channels: &HashMap<String, ChannelType>,
) -> Result<(), FrontendError> {
    for channel in &set.channels {
        if !channels.contains_key(&channel.value) {
            return Err(FrontendError {
                kind: FrontendErrorKind::InvalidInput,
                message: format!("undefined channel: {}", channel.value),
                span: Some(channel.span.clone()),
            });
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChannelType {
    Unit,
    IntRange { min: u64, max: u64 },
}

#[derive(Debug, Clone)]
struct Unsupported {
    message: String,
    span: SourceSpan,
}

#[derive(Debug, Clone)]
struct ParsedModule {
    channels: Vec<ChannelDecl>,
    declarations: Vec<ProcessDecl>,
    assertions: Vec<AssertionDecl>,
    entry: Option<Spanned<ProcessExpr>>,
    unsupported: Vec<Unsupported>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TokenKind {
    Newline,
    Eof,

    Ident(String),
    Int(u64),
    Stop,

    Channel,
    Assert,
    Datatype,

    Arrow,          // ->
    Equals,         // =
    Colon,          // :
    Comma,          // ,
    LParen,         // (
    RParen,         // )
    LBrace,         // {
    RBrace,         // }
    LBracket,       // [
    RBracket,       // ]
    Dot,            // .
    DotDot,         // ..
    Bang,           // !
    Question,       // ?
    Pipe,           // |
    HideOp,         // \\
    ExternalChoice, // []
    InternalChoice, // |~|
    Interleaving,   // |||
    IfaceOpen,      // [|{|
    IfaceClose,     // |}|]
    EventSetOpen,   // {|
    EventSetClose,  // |}
    RefineOpT,      // [T=
    RefineOpF,      // [F=
    RefineOpFD,     // [FD=
}

#[derive(Debug, Clone)]
struct Token {
    kind: TokenKind,
    span: SourceSpan,
}

fn merge_span(left: &SourceSpan, right: &SourceSpan) -> SourceSpan {
    SourceSpan {
        path: left.path.clone(),
        start_line: left.start_line,
        start_col: left.start_col,
        end_line: right.end_line,
        end_col: right.end_col,
    }
}

fn lex(input: &str, path: &str) -> Result<Vec<Token>, FrontendError> {
    let bytes = input.as_bytes();
    let mut idx = 0usize;
    let mut line = 1u32;
    let mut col = 1u32;
    let mut tokens = Vec::new();

    let make_span = |start_line: u32, start_col: u32, end_line: u32, end_col: u32| SourceSpan {
        path: path.to_string(),
        start_line,
        start_col,
        end_line,
        end_col,
    };

    let bump = |idx: &mut usize, line: &mut u32, col: &mut u32| -> Option<u8> {
        let b = *bytes.get(*idx)?;
        *idx += 1;
        match b {
            b'\n' => {
                *line += 1;
                *col = 1;
            }
            _ => {
                *col += 1;
            }
        }
        Some(b)
    };

    while idx < bytes.len() {
        let b = bytes[idx];

        if b == b' ' || b == b'\t' || b == b'\r' {
            bump(&mut idx, &mut line, &mut col);
            continue;
        }

        if b == b'\n' {
            let span = make_span(line, col, line, col);
            bump(&mut idx, &mut line, &mut col);
            tokens.push(Token {
                kind: TokenKind::Newline,
                span,
            });
            continue;
        }

        if b == b'-' && bytes.get(idx + 1) == Some(&b'-') {
            bump(&mut idx, &mut line, &mut col);
            bump(&mut idx, &mut line, &mut col);
            while idx < bytes.len() && bytes[idx] != b'\n' {
                bump(&mut idx, &mut line, &mut col);
            }
            continue;
        }

        let start_line = line;
        let start_col = col;

        let matches = |idx: usize, s: &[u8]| -> bool { bytes.get(idx..idx + s.len()) == Some(s) };

        let push_fixed = |tokens: &mut Vec<Token>,
                          kind: TokenKind,
                          len: u32,
                          idx: &mut usize,
                          line: &mut u32,
                          col: &mut u32| {
            let span = make_span(start_line, start_col, start_line, start_col + len - 1);
            for _ in 0..len {
                bump(idx, line, col);
            }
            tokens.push(Token { kind, span });
        };

        if matches(idx, b"[|{|") {
            push_fixed(
                &mut tokens,
                TokenKind::IfaceOpen,
                4,
                &mut idx,
                &mut line,
                &mut col,
            );
            continue;
        }
        if matches(idx, b"|}|]") {
            push_fixed(
                &mut tokens,
                TokenKind::IfaceClose,
                4,
                &mut idx,
                &mut line,
                &mut col,
            );
            continue;
        }
        if matches(idx, b"|~|") {
            push_fixed(
                &mut tokens,
                TokenKind::InternalChoice,
                3,
                &mut idx,
                &mut line,
                &mut col,
            );
            continue;
        }
        if matches(idx, b"|||") {
            push_fixed(
                &mut tokens,
                TokenKind::Interleaving,
                3,
                &mut idx,
                &mut line,
                &mut col,
            );
            continue;
        }
        if matches(idx, b"[]") {
            push_fixed(
                &mut tokens,
                TokenKind::ExternalChoice,
                2,
                &mut idx,
                &mut line,
                &mut col,
            );
            continue;
        }
        if matches(idx, b"->") {
            push_fixed(
                &mut tokens,
                TokenKind::Arrow,
                2,
                &mut idx,
                &mut line,
                &mut col,
            );
            continue;
        }
        if matches(idx, br"\\") {
            push_fixed(
                &mut tokens,
                TokenKind::HideOp,
                2,
                &mut idx,
                &mut line,
                &mut col,
            );
            continue;
        }
        if matches(idx, b"{|") {
            push_fixed(
                &mut tokens,
                TokenKind::EventSetOpen,
                2,
                &mut idx,
                &mut line,
                &mut col,
            );
            continue;
        }
        if matches(idx, b"|}") {
            push_fixed(
                &mut tokens,
                TokenKind::EventSetClose,
                2,
                &mut idx,
                &mut line,
                &mut col,
            );
            continue;
        }
        if matches(idx, b"..") {
            push_fixed(
                &mut tokens,
                TokenKind::DotDot,
                2,
                &mut idx,
                &mut line,
                &mut col,
            );
            continue;
        }
        if matches(idx, b"[FD=") {
            push_fixed(
                &mut tokens,
                TokenKind::RefineOpFD,
                4,
                &mut idx,
                &mut line,
                &mut col,
            );
            continue;
        }
        if matches(idx, b"[F=") {
            push_fixed(
                &mut tokens,
                TokenKind::RefineOpF,
                3,
                &mut idx,
                &mut line,
                &mut col,
            );
            continue;
        }
        if matches(idx, b"[T=") {
            push_fixed(
                &mut tokens,
                TokenKind::RefineOpT,
                3,
                &mut idx,
                &mut line,
                &mut col,
            );
            continue;
        }

        match b {
            b'=' => {
                push_fixed(
                    &mut tokens,
                    TokenKind::Equals,
                    1,
                    &mut idx,
                    &mut line,
                    &mut col,
                );
                continue;
            }
            b':' => {
                push_fixed(
                    &mut tokens,
                    TokenKind::Colon,
                    1,
                    &mut idx,
                    &mut line,
                    &mut col,
                );
                continue;
            }
            b',' => {
                push_fixed(
                    &mut tokens,
                    TokenKind::Comma,
                    1,
                    &mut idx,
                    &mut line,
                    &mut col,
                );
                continue;
            }
            b'(' => {
                push_fixed(
                    &mut tokens,
                    TokenKind::LParen,
                    1,
                    &mut idx,
                    &mut line,
                    &mut col,
                );
                continue;
            }
            b')' => {
                push_fixed(
                    &mut tokens,
                    TokenKind::RParen,
                    1,
                    &mut idx,
                    &mut line,
                    &mut col,
                );
                continue;
            }
            b'{' => {
                push_fixed(
                    &mut tokens,
                    TokenKind::LBrace,
                    1,
                    &mut idx,
                    &mut line,
                    &mut col,
                );
                continue;
            }
            b'}' => {
                push_fixed(
                    &mut tokens,
                    TokenKind::RBrace,
                    1,
                    &mut idx,
                    &mut line,
                    &mut col,
                );
                continue;
            }
            b'[' => {
                push_fixed(
                    &mut tokens,
                    TokenKind::LBracket,
                    1,
                    &mut idx,
                    &mut line,
                    &mut col,
                );
                continue;
            }
            b']' => {
                push_fixed(
                    &mut tokens,
                    TokenKind::RBracket,
                    1,
                    &mut idx,
                    &mut line,
                    &mut col,
                );
                continue;
            }
            b'.' => {
                push_fixed(
                    &mut tokens,
                    TokenKind::Dot,
                    1,
                    &mut idx,
                    &mut line,
                    &mut col,
                );
                continue;
            }
            b'!' => {
                push_fixed(
                    &mut tokens,
                    TokenKind::Bang,
                    1,
                    &mut idx,
                    &mut line,
                    &mut col,
                );
                continue;
            }
            b'?' => {
                push_fixed(
                    &mut tokens,
                    TokenKind::Question,
                    1,
                    &mut idx,
                    &mut line,
                    &mut col,
                );
                continue;
            }
            b'|' => {
                push_fixed(
                    &mut tokens,
                    TokenKind::Pipe,
                    1,
                    &mut idx,
                    &mut line,
                    &mut col,
                );
                continue;
            }
            b'0'..=b'9' => {
                let start = idx;
                while idx < bytes.len() && bytes[idx].is_ascii_digit() {
                    bump(&mut idx, &mut line, &mut col);
                }
                let text = std::str::from_utf8(&bytes[start..idx]).expect("digits are valid utf8");
                let value = text.parse::<u64>().map_err(|_| FrontendError {
                    kind: FrontendErrorKind::InvalidInput,
                    message: format!("invalid integer literal: {text}"),
                    span: Some(make_span(start_line, start_col, start_line, col - 1)),
                })?;
                let span = make_span(start_line, start_col, start_line, col - 1);
                tokens.push(Token {
                    kind: TokenKind::Int(value),
                    span,
                });
                continue;
            }
            b'A'..=b'Z' | b'a'..=b'z' | b'_' => {
                let start = idx;
                while idx < bytes.len()
                    && (bytes[idx].is_ascii_alphanumeric() || bytes[idx] == b'_')
                {
                    bump(&mut idx, &mut line, &mut col);
                }
                let text = std::str::from_utf8(&bytes[start..idx]).expect("ident is utf8");
                let kind = match text {
                    "channel" => TokenKind::Channel,
                    "assert" => TokenKind::Assert,
                    "datatype" => TokenKind::Datatype,
                    "STOP" => TokenKind::Stop,
                    _ => TokenKind::Ident(text.to_string()),
                };
                let span = make_span(start_line, start_col, start_line, col - 1);
                tokens.push(Token { kind, span });
                continue;
            }
            _ => {
                let span = make_span(start_line, start_col, start_line, start_col);
                return Err(FrontendError {
                    kind: FrontendErrorKind::InvalidInput,
                    message: format!("unexpected character: {}", b as char),
                    span: Some(span),
                });
            }
        }
    }

    tokens.push(Token {
        kind: TokenKind::Eof,
        span: SourceSpan {
            path: path.to_string(),
            start_line: line,
            start_col: col,
            end_line: line,
            end_col: col,
        },
    });

    Ok(tokens)
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    path: String,
}

impl Parser {
    fn new(tokens: Vec<Token>, path: &str) -> Self {
        Self {
            tokens,
            pos: 0,
            path: path.to_string(),
        }
    }

    fn parse_module(&mut self) -> Result<ParsedModule, FrontendError> {
        let mut module = ParsedModule {
            channels: Vec::new(),
            declarations: Vec::new(),
            assertions: Vec::new(),
            entry: None,
            unsupported: Vec::new(),
        };

        self.consume_newlines();
        while !self.peek_is(TokenKind::Eof) {
            if self.consume_is(TokenKind::Channel) {
                module.channels.push(self.parse_channel_decl()?);
            } else if self.consume_is(TokenKind::Datatype) {
                let span = self.prev_span().unwrap_or_else(|| SourceSpan {
                    path: self.path.clone(),
                    start_line: 1,
                    start_col: 1,
                    end_line: 1,
                    end_col: 1,
                });
                module.unsupported.push(Unsupported {
                    message: "unsupported syntax: datatype".to_string(),
                    span,
                });
                self.skip_until_line_end();
            } else if self.consume_is(TokenKind::Assert) {
                module.assertions.push(self.parse_assert_decl()?);
            } else if self.peek_process_decl_start() {
                module.declarations.push(self.parse_process_decl()?);
            } else {
                if module.entry.is_some() {
                    return Err(
                        self.invalid_input(self.peek_span(), "multiple top-level expressions")
                    );
                }
                let expr = self.parse_expr()?;
                module.entry = Some(expr);
                self.expect_line_end()?;
            }

            self.consume_newlines();
        }

        Ok(module)
    }

    fn parse_channel_decl(&mut self) -> Result<ChannelDecl, FrontendError> {
        let mut names = Vec::new();
        names.push(self.expect_ident_spanned("channel name")?);
        while self.consume_is(TokenKind::Comma) {
            names.push(self.expect_ident_spanned("channel name")?);
        }

        let domain = if self.consume_is(TokenKind::Colon) {
            Some(self.parse_channel_domain()?)
        } else {
            None
        };

        self.expect_line_end()?;
        Ok(ChannelDecl { names, domain })
    }

    fn parse_channel_domain(&mut self) -> Result<Spanned<ChannelDomain>, FrontendError> {
        if self.consume_is(TokenKind::LBrace) {
            let min = self.expect_int_spanned("domain min")?;
            self.expect(TokenKind::DotDot, "expected '..' in domain")?;
            let max = self.expect_int_spanned("domain max")?;
            let rbrace = self.expect(TokenKind::RBrace, "expected '}' in domain")?;
            let span = merge_span(&min.span, &rbrace.span);
            return Ok(Spanned {
                value: ChannelDomain::IntRange { min, max },
                span,
            });
        }

        let name = self.expect_ident_spanned("domain type")?;
        let span = name.span.clone();
        Ok(Spanned {
            value: ChannelDomain::NamedType(name),
            span,
        })
    }

    fn parse_process_decl(&mut self) -> Result<ProcessDecl, FrontendError> {
        let name = self.expect_ident_spanned("process name")?;
        self.expect(TokenKind::Equals, "expected '=' in process declaration")?;
        let expr = self.parse_expr()?;
        self.expect_line_end()?;
        Ok(ProcessDecl { name, expr })
    }

    fn parse_assert_decl(&mut self) -> Result<AssertionDecl, FrontendError> {
        let target = self.expect_ident_spanned("assert target")?;
        if self.consume_is(TokenKind::Colon) {
            self.expect(TokenKind::LBracket, "expected '[' after ':'")?;
            let (kind, model) = self.parse_property_assertion_spec()?;
            self.expect(TokenKind::RBracket, "expected ']' to close assert")?;
            self.expect_line_end()?;
            return Ok(AssertionDecl::Property {
                target,
                kind,
                model,
            });
        }

        let model = if self.consume_is(TokenKind::RefineOpT) {
            RefinementOp::T
        } else if self.consume_is(TokenKind::RefineOpF) {
            RefinementOp::F
        } else if self.consume_is(TokenKind::RefineOpFD) {
            RefinementOp::FD
        } else {
            return Err(self.invalid_input(
                self.peek_span(),
                "expected property assertion (:[...]) or refinement operator ([T= / [F= / [FD=)",
            ));
        };

        let impl_ = self.expect_ident_spanned("refinement impl")?;
        self.expect_line_end()?;
        Ok(AssertionDecl::Refinement {
            spec: target,
            model,
            impl_,
        })
    }

    fn parse_property_assertion_spec(
        &mut self,
    ) -> Result<(PropertyKind, PropertyModel), FrontendError> {
        let kind_ident = self.expect_ident("assertion kind")?;
        let kind = match kind_ident.as_str() {
            "deadlock" => {
                let free = self.expect_ident("expected 'free'")?;
                if free != "free" {
                    return Err(self.invalid_input(self.prev_span(), "expected 'free'"));
                }
                PropertyKind::DeadlockFree
            }
            "divergence" => {
                let free = self.expect_ident("expected 'free'")?;
                if free != "free" {
                    return Err(self.invalid_input(self.prev_span(), "expected 'free'"));
                }
                PropertyKind::DivergenceFree
            }
            "deterministic" => PropertyKind::Deterministic,
            _ => {
                return Err(self.invalid_input(
                    self.prev_span(),
                    format!("unknown assertion kind: {kind_ident}"),
                ))
            }
        };

        self.expect(TokenKind::LBracket, "expected '[' for assertion model")?;
        let model_ident = self.expect_ident("assertion model")?;
        let model = match model_ident.as_str() {
            "F" => PropertyModel::F,
            "FD" => PropertyModel::FD,
            _ => {
                return Err(self.invalid_input(
                    self.prev_span(),
                    format!("unknown assertion model: {model_ident}"),
                ))
            }
        };
        self.expect(TokenKind::RBracket, "expected ']' after assertion model")?;
        Ok((kind, model))
    }

    fn parse_expr(&mut self) -> Result<Spanned<ProcessExpr>, FrontendError> {
        self.parse_choice()
    }

    fn parse_choice(&mut self) -> Result<Spanned<ProcessExpr>, FrontendError> {
        let mut left = self.parse_parallel()?;
        loop {
            if self.consume_is(TokenKind::ExternalChoice) {
                let right = self.parse_parallel()?;
                let span = merge_span(&left.span, &right.span);
                left = Spanned {
                    value: ProcessExpr::Choice {
                        kind: ChoiceKind::External,
                        left: Box::new(left),
                        right: Box::new(right),
                    },
                    span,
                };
                continue;
            }
            if self.consume_is(TokenKind::InternalChoice) {
                let right = self.parse_parallel()?;
                let span = merge_span(&left.span, &right.span);
                left = Spanned {
                    value: ProcessExpr::Choice {
                        kind: ChoiceKind::Internal,
                        left: Box::new(left),
                        right: Box::new(right),
                    },
                    span,
                };
                continue;
            }
            break;
        }
        Ok(left)
    }

    fn parse_parallel(&mut self) -> Result<Spanned<ProcessExpr>, FrontendError> {
        let mut left = self.parse_hide()?;
        loop {
            if self.consume_is(TokenKind::Interleaving) {
                let right = self.parse_hide()?;
                let span = merge_span(&left.span, &right.span);
                left = Spanned {
                    value: ProcessExpr::Parallel {
                        kind: ParallelKind::Interleaving,
                        left: Box::new(left),
                        right: Box::new(right),
                        sync: None,
                    },
                    span,
                };
                continue;
            }
            if self.consume_is(TokenKind::IfaceOpen) {
                let sync = self.parse_iface_sync_set()?;
                self.expect(TokenKind::IfaceClose, "expected '|}|]'")?;
                let right = self.parse_hide()?;
                let span = merge_span(&left.span, &right.span);
                left = Spanned {
                    value: ProcessExpr::Parallel {
                        kind: ParallelKind::Interface,
                        left: Box::new(left),
                        right: Box::new(right),
                        sync: Some(sync),
                    },
                    span,
                };
                continue;
            }
            break;
        }
        Ok(left)
    }

    fn parse_hide(&mut self) -> Result<Spanned<ProcessExpr>, FrontendError> {
        let mut inner = self.parse_prefix()?;
        while self.consume_is(TokenKind::HideOp) {
            let (hide, hide_span) = self.parse_event_set()?;
            let span = merge_span(&inner.span, &hide_span);
            inner = Spanned {
                value: ProcessExpr::Hide {
                    inner: Box::new(inner),
                    hide,
                },
                span,
            };
        }
        Ok(inner)
    }

    fn parse_prefix(&mut self) -> Result<Spanned<ProcessExpr>, FrontendError> {
        let start_pos = self.pos;
        if let Ok(event) = self.parse_event() {
            if self.consume_is(TokenKind::Arrow) {
                let next = self.parse_prefix()?;
                let span = merge_span(&event.span, &next.span);
                return Ok(Spanned {
                    value: ProcessExpr::Prefix {
                        event,
                        next: Box::new(next),
                    },
                    span,
                });
            }
        }
        self.pos = start_pos;
        self.parse_atom()
    }

    fn parse_atom(&mut self) -> Result<Spanned<ProcessExpr>, FrontendError> {
        if self.consume_is(TokenKind::Stop) {
            let span = self.prev_span().expect("stop token has span");
            return Ok(Spanned {
                value: ProcessExpr::Stop,
                span,
            });
        }

        if self.consume_is(TokenKind::LParen) {
            let lparen_span = self.prev_span().expect("lparen span");
            let inner = self.parse_expr()?;
            let rparen = self.expect(TokenKind::RParen, "expected ')'")?;
            let span = merge_span(&lparen_span, &rparen.span);
            return Ok(Spanned {
                value: inner.value,
                span,
            });
        }

        let name = self.expect_ident_spanned("process reference")?;
        let span = name.span.clone();
        Ok(Spanned {
            value: ProcessExpr::Ref(name),
            span,
        })
    }

    fn parse_event(&mut self) -> Result<Spanned<Event>, FrontendError> {
        let channel = self.expect_ident_spanned("event channel")?;
        let mut end_span = channel.span.clone();
        let seg = if self.consume_is(TokenKind::Dot) {
            let value = self.expect_value("dot payload")?;
            end_span = value.span.clone();
            Some(EventSeg::Dot(value))
        } else if self.consume_is(TokenKind::Bang) {
            let value = self.expect_value("output payload")?;
            end_span = value.span.clone();
            Some(EventSeg::Out(value))
        } else if self.consume_is(TokenKind::Question) {
            let input = self.expect_input("input pattern")?;
            end_span = input.span.clone();
            Some(EventSeg::In(input))
        } else {
            None
        };

        let span = merge_span(&channel.span, &end_span);
        Ok(Spanned {
            value: Event { channel, seg },
            span,
        })
    }

    fn expect_value(&mut self, label: &str) -> Result<Spanned<EventValue>, FrontendError> {
        if let Some(Token {
            kind: TokenKind::Int(value),
            span,
        }) = self.peek()
        {
            let span = span.clone();
            let value = *value;
            self.pos += 1;
            return Ok(Spanned {
                value: EventValue::Int(value),
                span,
            });
        }
        let ident = self.expect_ident_spanned(label)?;
        Ok(Spanned {
            value: EventValue::Ident(ident.value),
            span: ident.span,
        })
    }

    fn expect_input(&mut self, label: &str) -> Result<Spanned<EventInput>, FrontendError> {
        if let Some(Token {
            kind: TokenKind::Int(value),
            span,
        }) = self.peek()
        {
            let span = span.clone();
            let value = *value;
            self.pos += 1;
            return Ok(Spanned {
                value: EventInput::Int(value),
                span,
            });
        }
        let ident = self.expect_ident_spanned(label)?;
        Ok(Spanned {
            value: EventInput::Bind(ident.value),
            span: ident.span,
        })
    }

    fn parse_iface_sync_set(&mut self) -> Result<EventSet, FrontendError> {
        let mut channels = Vec::new();
        channels.push(self.expect_ident_spanned("sync set element")?);
        while self.consume_is(TokenKind::Comma) {
            channels.push(self.expect_ident_spanned("sync set element")?);
        }
        Ok(EventSet { channels })
    }

    fn parse_event_set(&mut self) -> Result<(EventSet, SourceSpan), FrontendError> {
        let open = self.expect(TokenKind::EventSetOpen, "expected '{|'")?;
        let mut channels = Vec::new();
        if !self.peek_is(TokenKind::EventSetClose) {
            channels.push(self.expect_ident_spanned("set element")?);
            while self.consume_is(TokenKind::Comma) {
                channels.push(self.expect_ident_spanned("set element")?);
            }
        }
        let close = self.expect(TokenKind::EventSetClose, "expected '|}'")?;
        let span = merge_span(&open.span, &close.span);
        Ok((EventSet { channels }, span))
    }

    fn peek_process_decl_start(&self) -> bool {
        matches!(
            (self.peek_kind(), self.peek_kind_n(1)),
            (Some(TokenKind::Ident(_)), Some(TokenKind::Equals))
        )
    }

    fn consume_newlines(&mut self) {
        while self.consume_is(TokenKind::Newline) {}
    }

    fn skip_until_line_end(&mut self) {
        while !self.peek_is(TokenKind::Newline) && !self.peek_is(TokenKind::Eof) {
            self.pos += 1;
        }
    }

    fn expect_line_end(&mut self) -> Result<(), FrontendError> {
        if self.consume_is(TokenKind::Newline) || self.peek_is(TokenKind::Eof) {
            return Ok(());
        }
        Err(self.invalid_input(self.peek_span(), "expected end of line"))
    }

    fn expect(&mut self, kind: TokenKind, message: &str) -> Result<Token, FrontendError> {
        if self.peek_is(kind.clone()) {
            return Ok(self.next().expect("token exists"));
        }
        Err(self.invalid_input(self.peek_span(), message))
    }

    fn expect_ident_spanned(&mut self, message: &str) -> Result<Spanned<String>, FrontendError> {
        match self.next() {
            Some(Token {
                kind: TokenKind::Ident(value),
                span,
            }) => Ok(Spanned { value, span }),
            Some(token) => Err(self.invalid_input(Some(token.span), message)),
            None => Err(self.invalid_input(self.peek_span(), message)),
        }
    }

    fn expect_int_spanned(&mut self, message: &str) -> Result<Spanned<u64>, FrontendError> {
        match self.next() {
            Some(Token {
                kind: TokenKind::Int(value),
                span,
            }) => Ok(Spanned { value, span }),
            Some(token) => Err(self.invalid_input(Some(token.span), message)),
            None => Err(self.invalid_input(self.peek_span(), message)),
        }
    }

    fn expect_ident(&mut self, message: &str) -> Result<String, FrontendError> {
        match self.next() {
            Some(Token {
                kind: TokenKind::Ident(value),
                ..
            }) => Ok(value),
            Some(token) => Err(self.invalid_input(Some(token.span), message)),
            None => Err(self.invalid_input(self.peek_span(), message)),
        }
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn peek_kind(&self) -> Option<&TokenKind> {
        self.peek().map(|t| &t.kind)
    }

    fn peek_kind_n(&self, n: usize) -> Option<&TokenKind> {
        self.tokens.get(self.pos + n).map(|t| &t.kind)
    }

    fn peek_span(&self) -> Option<SourceSpan> {
        self.peek().map(|t| t.span.clone())
    }

    fn prev_span(&self) -> Option<SourceSpan> {
        self.pos
            .checked_sub(1)
            .and_then(|idx| self.tokens.get(idx))
            .map(|t| t.span.clone())
    }

    fn peek_is(&self, kind: TokenKind) -> bool {
        self.peek_kind() == Some(&kind)
    }

    fn consume_is(&mut self, kind: TokenKind) -> bool {
        if self.peek_is(kind) {
            self.pos += 1;
            return true;
        }
        false
    }

    fn next(&mut self) -> Option<Token> {
        let token = self.tokens.get(self.pos)?.clone();
        self.pos += 1;
        Some(token)
    }

    fn invalid_input(&self, span: Option<SourceSpan>, message: impl Into<String>) -> FrontendError {
        FrontendError {
            kind: FrontendErrorKind::InvalidInput,
            message: message.into(),
            span,
        }
    }
}
