use crate::ir::{ChoiceKind, EventInput, EventSeg, EventValue, Module, ProcessExpr, Spanned};
use crate::lts::{Transition, TransitionProvider};
use crate::state_codec::{StateCodec, StateCodecError};
use crate::types::SourceSpan;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::{Display, Formatter};

type ExprId = u32;
type ProcId = u32;
type ProcIds = BTreeMap<String, ProcId>;
type ProcExprs<'a> = BTreeMap<String, &'a Spanned<ProcessExpr>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChannelDomain {
    Unit,
    IntRange { min: u64, max: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum EventPat {
    Unit { channel: String },
    Int { channel: String, value: u64 },
    OutVar { channel: String, var: String },
    InConst { channel: String, value: u64 },
    InBind { channel: String, var: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum ExprNode {
    Stop,
    Ref(ProcId),
    Prefix { event: EventPat, next: ExprId },
    ChoiceExternal { left: ExprId, right: ExprId },
    ChoiceInternal { left: ExprId, right: ExprId },
}

#[derive(Debug)]
struct Program {
    channels: BTreeMap<String, ChannelDomain>,
    proc_roots: Vec<ExprId>,
    exprs: Vec<ExprNode>,
    resolved: Vec<ExprId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CspmState {
    expr: ExprId,
    env: BTreeMap<String, u64>,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct CspmStateCodec;

impl StateCodec<CspmState> for CspmStateCodec {
    fn encode(&self, state: &CspmState) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&state.expr.to_be_bytes());
        out.extend_from_slice(&(state.env.len() as u32).to_be_bytes());
        for (key, value) in &state.env {
            out.extend_from_slice(&(key.len() as u32).to_be_bytes());
            out.extend_from_slice(key.as_bytes());
            out.extend_from_slice(&value.to_be_bytes());
        }
        out
    }

    fn decode(&self, bytes: &[u8]) -> Result<CspmState, StateCodecError> {
        fn take<'a>(bytes: &mut &'a [u8], n: usize) -> Result<&'a [u8], StateCodecError> {
            if bytes.len() < n {
                return Err(StateCodecError::new("unexpected EOF"));
            }
            let (head, tail) = bytes.split_at(n);
            *bytes = tail;
            Ok(head)
        }

        let mut input = bytes;
        let expr = u32::from_be_bytes(
            take(&mut input, 4)?
                .try_into()
                .map_err(|_| StateCodecError::new("invalid expr bytes"))?,
        );
        let count = u32::from_be_bytes(
            take(&mut input, 4)?
                .try_into()
                .map_err(|_| StateCodecError::new("invalid env count bytes"))?,
        ) as usize;

        let mut env = BTreeMap::new();
        for _ in 0..count {
            let key_len = u32::from_be_bytes(
                take(&mut input, 4)?
                    .try_into()
                    .map_err(|_| StateCodecError::new("invalid key len bytes"))?,
            ) as usize;
            let key = std::str::from_utf8(take(&mut input, key_len)?)
                .map_err(|_| StateCodecError::new("invalid utf8 in key"))?
                .to_string();
            let value = u64::from_be_bytes(
                take(&mut input, 8)?
                    .try_into()
                    .map_err(|_| StateCodecError::new("invalid value bytes"))?,
            );
            env.insert(key, value);
        }
        if !input.is_empty() {
            return Err(StateCodecError::new("trailing bytes"));
        }
        Ok(CspmState { expr, env })
    }
}

#[derive(Debug, Clone)]
pub struct CspmLtsError {
    pub message: String,
    pub span: Option<SourceSpan>,
}

impl Display for CspmLtsError {
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

impl std::error::Error for CspmLtsError {}

#[derive(Debug)]
pub struct CspmTransitionProvider {
    program: Program,
    initial: CspmState,
}

impl CspmTransitionProvider {
    pub fn from_module(module: &Module) -> Result<Self, CspmLtsError> {
        let channels = compile_channels(module)?;
        let (proc_ids, proc_exprs) = collect_processes(module)?;
        let mut builder = ProgramBuilder::new(channels, &proc_ids)?;
        let initial_expr = initial_expr(module)?;
        let initial_expr_id = builder.compile_expr(initial_expr)?;

        for (name, proc_id) in proc_ids.iter() {
            let expr = proc_exprs.get(name).ok_or_else(|| CspmLtsError {
                message: format!("undefined process: {name}"),
                span: None,
            })?;
            let root = builder.compile_expr(expr)?;
            builder.proc_roots[*proc_id as usize] = root;
        }

        let program = builder.finish()?;
        let initial = CspmState {
            expr: program.resolved[initial_expr_id as usize],
            env: BTreeMap::new(),
        };

        Ok(Self { program, initial })
    }

    fn transitions_for(&self, state: &CspmState) -> Vec<(Transition, CspmState)> {
        let mut out = Vec::new();
        self.transitions_for_expr(state.expr, &state.env, &mut out);
        out.sort_by(|(a_t, a_s), (b_t, b_s)| {
            let label_cmp = a_t.label.cmp(&b_t.label);
            if label_cmp != std::cmp::Ordering::Equal {
                return label_cmp;
            }
            let a_bytes = CspmStateCodec.encode(a_s);
            let b_bytes = CspmStateCodec.encode(b_s);
            a_bytes.cmp(&b_bytes)
        });
        out
    }

    fn transitions_for_expr(
        &self,
        expr: ExprId,
        env: &BTreeMap<String, u64>,
        out: &mut Vec<(Transition, CspmState)>,
    ) {
        let expr = self.program.resolved[expr as usize];
        match &self.program.exprs[expr as usize] {
            ExprNode::Stop => {}
            ExprNode::Ref(proc_id) => {
                let target = self.program.proc_roots[*proc_id as usize];
                let target = self.program.resolved[target as usize];
                self.transitions_for_expr(target, env, out);
            }
            ExprNode::Prefix { event, next } => {
                for (label, next_env) in self.eval_event(event, env) {
                    let next_expr = self.program.resolved[*next as usize];
                    out.push((
                        Transition { label },
                        CspmState {
                            expr: next_expr,
                            env: next_env,
                        },
                    ));
                }
            }
            ExprNode::ChoiceExternal { left, right } => {
                self.transitions_for_expr(*left, env, out);
                self.transitions_for_expr(*right, env, out);
            }
            ExprNode::ChoiceInternal { left, right } => {
                for target in [*left, *right] {
                    let target = self.program.resolved[target as usize];
                    out.push((
                        Transition {
                            label: "tau".to_string(),
                        },
                        CspmState {
                            expr: target,
                            env: env.clone(),
                        },
                    ));
                }
            }
        }
    }

    fn eval_event(
        &self,
        event: &EventPat,
        env: &BTreeMap<String, u64>,
    ) -> Vec<(String, BTreeMap<String, u64>)> {
        match event {
            EventPat::Unit { channel } => vec![(channel.clone(), env.clone())],
            EventPat::Int { channel, value } => vec![(format!("{channel}.{value}"), env.clone())],
            EventPat::OutVar { channel, var } => {
                let Some(value) = env.get(var) else {
                    return Vec::new();
                };
                vec![(format!("{channel}.{value}"), env.clone())]
            }
            EventPat::InConst { channel, value } => {
                vec![(format!("{channel}.{value}"), env.clone())]
            }
            EventPat::InBind { channel, var } => {
                let Some(domain) = self.program.channels.get(channel) else {
                    return Vec::new();
                };
                let ChannelDomain::IntRange { min, max } = domain else {
                    return Vec::new();
                };
                let mut out = Vec::new();
                for value in *min..=*max {
                    let mut next_env = env.clone();
                    next_env.insert(var.clone(), value);
                    out.push((format!("{channel}.{value}"), next_env));
                }
                out
            }
        }
    }
}

impl TransitionProvider for CspmTransitionProvider {
    type State = CspmState;
    type Transition = Transition;

    fn initial_state(&self) -> Self::State {
        self.initial.clone()
    }

    fn transitions(&self, state: &Self::State) -> Vec<(Self::Transition, Self::State)> {
        self.transitions_for(state)
    }
}

fn initial_expr(module: &Module) -> Result<&Spanned<ProcessExpr>, CspmLtsError> {
    if let Some(entry) = &module.entry {
        return Ok(entry);
    }
    if module.declarations.len() == 1 {
        return Ok(&module.declarations[0].expr);
    }
    Err(CspmLtsError {
        message: "entry process not specified".to_string(),
        span: None,
    })
}

fn compile_channels(module: &Module) -> Result<BTreeMap<String, ChannelDomain>, CspmLtsError> {
    let mut channels = BTreeMap::new();
    for decl in &module.channels {
        let domain = match &decl.domain {
            None => ChannelDomain::Unit,
            Some(domain) => match &domain.value {
                crate::ir::ChannelDomain::IntRange { min, max } => ChannelDomain::IntRange {
                    min: min.value,
                    max: max.value,
                },
                crate::ir::ChannelDomain::NamedType(_) => {
                    return Err(CspmLtsError {
                        message: "unsupported channel domain".to_string(),
                        span: Some(domain.span.clone()),
                    });
                }
            },
        };
        for name in &decl.names {
            if channels.contains_key(&name.value) {
                return Err(CspmLtsError {
                    message: format!("duplicate channel: {}", name.value),
                    span: Some(name.span.clone()),
                });
            }
            channels.insert(name.value.clone(), domain);
        }
    }
    Ok(channels)
}

fn collect_processes<'a>(module: &'a Module) -> Result<(ProcIds, ProcExprs<'a>), CspmLtsError> {
    let mut proc_ids = ProcIds::new();
    let mut proc_exprs = ProcExprs::new();

    for decl in &module.declarations {
        let name = decl.name.value.clone();
        if proc_ids.contains_key(&name) {
            return Err(CspmLtsError {
                message: format!("duplicate process: {name}"),
                span: Some(decl.name.span.clone()),
            });
        }
        proc_exprs.insert(name.clone(), &decl.expr);
        proc_ids.insert(name, 0);
    }

    for (idx, name) in proc_ids
        .keys()
        .cloned()
        .collect::<Vec<_>>()
        .iter()
        .enumerate()
    {
        proc_ids.insert(name.clone(), idx as ProcId);
    }

    Ok((proc_ids, proc_exprs))
}

struct ProgramBuilder<'a> {
    channels: BTreeMap<String, ChannelDomain>,
    proc_ids: &'a BTreeMap<String, ProcId>,
    exprs: Vec<ExprNode>,
    expr_spans: Vec<Option<SourceSpan>>,
    intern: HashMap<ExprNode, ExprId>,
    proc_roots: Vec<ExprId>,
}

impl<'a> ProgramBuilder<'a> {
    fn new(
        channels: BTreeMap<String, ChannelDomain>,
        proc_ids: &'a BTreeMap<String, ProcId>,
    ) -> Result<Self, CspmLtsError> {
        let proc_roots = vec![0; proc_ids.len()];
        Ok(Self {
            channels,
            proc_ids,
            exprs: Vec::new(),
            expr_spans: Vec::new(),
            intern: HashMap::new(),
            proc_roots,
        })
    }

    fn finish(self) -> Result<Program, CspmLtsError> {
        let resolved = compute_resolved(&self.exprs, &self.proc_roots, &self.expr_spans)?;
        Ok(Program {
            channels: self.channels,
            proc_roots: self.proc_roots,
            exprs: self.exprs,
            resolved,
        })
    }

    fn intern(&mut self, node: ExprNode, span: Option<SourceSpan>) -> ExprId {
        if let Some(id) = self.intern.get(&node) {
            return *id;
        }
        let id = self.exprs.len() as ExprId;
        self.exprs.push(node.clone());
        self.expr_spans.push(span);
        self.intern.insert(node, id);
        id
    }

    fn compile_expr(&mut self, expr: &Spanned<ProcessExpr>) -> Result<ExprId, CspmLtsError> {
        match &expr.value {
            ProcessExpr::Stop => Ok(self.intern(ExprNode::Stop, Some(expr.span.clone()))),
            ProcessExpr::Ref(name) => {
                let proc_id = self.proc_ids.get(&name.value).ok_or_else(|| CspmLtsError {
                    message: format!("undefined process: {}", name.value),
                    span: Some(name.span.clone()),
                })?;
                Ok(self.intern(ExprNode::Ref(*proc_id), Some(expr.span.clone())))
            }
            ProcessExpr::Prefix { event, next } => {
                let event_pat = compile_event_pat(event)?;
                let next = self.compile_expr(next)?;
                Ok(self.intern(
                    ExprNode::Prefix {
                        event: event_pat,
                        next,
                    },
                    Some(expr.span.clone()),
                ))
            }
            ProcessExpr::Choice { kind, left, right } => {
                let left = self.compile_expr(left)?;
                let right = self.compile_expr(right)?;
                let node = match kind {
                    ChoiceKind::External => ExprNode::ChoiceExternal { left, right },
                    ChoiceKind::Internal => ExprNode::ChoiceInternal { left, right },
                };
                Ok(self.intern(node, Some(expr.span.clone())))
            }
            ProcessExpr::Parallel { .. } => Err(CspmLtsError {
                message: "parallel composition is not supported yet".to_string(),
                span: Some(expr.span.clone()),
            }),
            ProcessExpr::Hide { .. } => Err(CspmLtsError {
                message: "hiding is not supported yet".to_string(),
                span: Some(expr.span.clone()),
            }),
        }
    }
}

fn compile_event_pat(event: &Spanned<crate::ir::Event>) -> Result<EventPat, CspmLtsError> {
    let channel = event.value.channel.value.clone();
    match &event.value.seg {
        None => Ok(EventPat::Unit { channel }),
        Some(EventSeg::Dot(value)) => match &value.value {
            EventValue::Int(n) => Ok(EventPat::Int { channel, value: *n }),
            EventValue::Ident(_) => Err(CspmLtsError {
                message: "unsupported dot payload".to_string(),
                span: Some(value.span.clone()),
            }),
        },
        Some(EventSeg::Out(value)) => match &value.value {
            EventValue::Int(n) => Ok(EventPat::Int { channel, value: *n }),
            EventValue::Ident(name) => Ok(EventPat::OutVar {
                channel,
                var: name.clone(),
            }),
        },
        Some(EventSeg::In(input)) => match &input.value {
            EventInput::Int(n) => Ok(EventPat::InConst { channel, value: *n }),
            EventInput::Bind(name) => Ok(EventPat::InBind {
                channel,
                var: name.clone(),
            }),
        },
    }
}

fn compute_resolved(
    exprs: &[ExprNode],
    proc_roots: &[ExprId],
    expr_spans: &[Option<SourceSpan>],
) -> Result<Vec<ExprId>, CspmLtsError> {
    fn resolve(
        id: ExprId,
        exprs: &[ExprNode],
        proc_roots: &[ExprId],
        spans: &[Option<SourceSpan>],
        memo: &mut Vec<Option<ExprId>>,
        visiting: &mut HashSet<ExprId>,
    ) -> Result<ExprId, CspmLtsError> {
        if let Some(resolved) = memo[id as usize] {
            return Ok(resolved);
        }
        if !visiting.insert(id) {
            return Err(CspmLtsError {
                message: "cyclic process reference".to_string(),
                span: spans.get(id as usize).and_then(|s| s.clone()),
            });
        }
        let resolved = match &exprs[id as usize] {
            ExprNode::Ref(proc_id) => {
                let target = proc_roots[*proc_id as usize];
                resolve(target, exprs, proc_roots, spans, memo, visiting)?
            }
            _ => id,
        };
        visiting.remove(&id);
        memo[id as usize] = Some(resolved);
        Ok(resolved)
    }

    let mut memo = vec![None; exprs.len()];
    let mut visiting = HashSet::new();
    let mut out = Vec::with_capacity(exprs.len());
    for id in 0..exprs.len() {
        out.push(resolve(
            id as ExprId,
            exprs,
            proc_roots,
            expr_spans,
            &mut memo,
            &mut visiting,
        )?);
    }
    Ok(out)
}
