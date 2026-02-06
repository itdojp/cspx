use crate::ir::{
    ChoiceKind, EventInput, EventSeg, EventValue, Module, ParallelKind, ProcessExpr, Spanned,
};
use crate::lts::{Transition, TransitionProvider};
use crate::state_codec::{StateCodec, StateCodecError};
use crate::types::SourceSpan;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
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
    Prefix {
        event: EventPat,
        next: ExprId,
    },
    ChoiceExternal {
        left: ExprId,
        right: ExprId,
    },
    ChoiceInternal {
        left: ExprId,
        right: ExprId,
    },
    Parallel {
        left: ExprId,
        right: ExprId,
        sync: BTreeSet<String>,
    },
    Hide {
        inner: ExprId,
        hide: BTreeSet<String>,
    },
}

#[derive(Debug)]
struct Program {
    channels: BTreeMap<String, ChannelDomain>,
    exprs: Vec<ExprNode>,
    resolved: Vec<ExprId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CspmState {
    Expr {
        expr: ExprId,
        env: BTreeMap<String, u64>,
    },
    Parallel {
        sync: BTreeSet<String>,
        left: Box<CspmState>,
        right: Box<CspmState>,
    },
    Hide {
        hide: BTreeSet<String>,
        inner: Box<CspmState>,
    },
}

#[derive(Debug, Default, Clone, Copy)]
pub struct CspmStateCodec;

impl StateCodec<CspmState> for CspmStateCodec {
    fn encode(&self, state: &CspmState) -> Vec<u8> {
        let mut out = Vec::new();
        match state {
            CspmState::Expr { expr, env } => {
                out.push(1);
                out.extend_from_slice(&expr.to_be_bytes());
                out.extend_from_slice(&(env.len() as u32).to_be_bytes());
                for (key, value) in env {
                    out.extend_from_slice(&(key.len() as u32).to_be_bytes());
                    out.extend_from_slice(key.as_bytes());
                    out.extend_from_slice(&value.to_be_bytes());
                }
            }
            CspmState::Parallel { sync, left, right } => {
                out.push(2);
                out.extend_from_slice(&(sync.len() as u32).to_be_bytes());
                for channel in sync {
                    out.extend_from_slice(&(channel.len() as u32).to_be_bytes());
                    out.extend_from_slice(channel.as_bytes());
                }
                out.extend_from_slice(&self.encode(left));
                out.extend_from_slice(&self.encode(right));
            }
            CspmState::Hide { hide, inner } => {
                out.push(3);
                out.extend_from_slice(&(hide.len() as u32).to_be_bytes());
                for channel in hide {
                    out.extend_from_slice(&(channel.len() as u32).to_be_bytes());
                    out.extend_from_slice(channel.as_bytes());
                }
                out.extend_from_slice(&self.encode(inner));
            }
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

        fn take_u32(bytes: &mut &[u8], err: &'static str) -> Result<u32, StateCodecError> {
            Ok(u32::from_be_bytes(
                take(bytes, 4)?
                    .try_into()
                    .map_err(|_| StateCodecError::new(err))?,
            ))
        }

        fn take_u64(bytes: &mut &[u8], err: &'static str) -> Result<u64, StateCodecError> {
            Ok(u64::from_be_bytes(
                take(bytes, 8)?
                    .try_into()
                    .map_err(|_| StateCodecError::new(err))?,
            ))
        }

        fn take_string(bytes: &mut &[u8]) -> Result<String, StateCodecError> {
            let len = take_u32(bytes, "invalid string length bytes")? as usize;
            let s = std::str::from_utf8(take(bytes, len)?)
                .map_err(|_| StateCodecError::new("invalid utf8 in string"))?
                .to_string();
            Ok(s)
        }

        fn decode_state(bytes: &mut &[u8]) -> Result<CspmState, StateCodecError> {
            let tag = *take(bytes, 1)?
                .first()
                .ok_or_else(|| StateCodecError::new("unexpected EOF"))?;
            match tag {
                1 => {
                    let expr = take_u32(bytes, "invalid expr bytes")?;
                    let count = take_u32(bytes, "invalid env count bytes")? as usize;
                    let mut env = BTreeMap::new();
                    for _ in 0..count {
                        let key = take_string(bytes)?;
                        let value = take_u64(bytes, "invalid value bytes")?;
                        env.insert(key, value);
                    }
                    Ok(CspmState::Expr { expr, env })
                }
                2 => {
                    let count = take_u32(bytes, "invalid sync count bytes")? as usize;
                    let mut sync = BTreeSet::new();
                    for _ in 0..count {
                        sync.insert(take_string(bytes)?);
                    }
                    let left = decode_state(bytes)?;
                    let right = decode_state(bytes)?;
                    Ok(CspmState::Parallel {
                        sync,
                        left: Box::new(left),
                        right: Box::new(right),
                    })
                }
                3 => {
                    let count = take_u32(bytes, "invalid hide count bytes")? as usize;
                    let mut hide = BTreeSet::new();
                    for _ in 0..count {
                        hide.insert(take_string(bytes)?);
                    }
                    let inner = decode_state(bytes)?;
                    Ok(CspmState::Hide {
                        hide,
                        inner: Box::new(inner),
                    })
                }
                _ => Err(StateCodecError::new("unknown CspmState tag")),
            }
        }

        let mut input = bytes;
        let state = decode_state(&mut input)?;
        if !input.is_empty() {
            return Err(StateCodecError::new("trailing bytes"));
        }
        Ok(state)
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
        let initial = state_from_expr(&program, initial_expr_id, BTreeMap::new());

        Ok(Self { program, initial })
    }

    fn transitions_for(&self, state: &CspmState) -> Vec<(Transition, CspmState)> {
        let mut out = Vec::new();
        self.transitions_for_state_unordered(state, &mut out);
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

    fn transitions_for_state_unordered(
        &self,
        state: &CspmState,
        out: &mut Vec<(Transition, CspmState)>,
    ) {
        match state {
            CspmState::Expr { expr, env } => self.transitions_for_expr_unordered(*expr, env, out),
            CspmState::Parallel { sync, left, right } => {
                self.transitions_for_parallel_unordered(sync, left, right, out)
            }
            CspmState::Hide { hide, inner } => {
                self.transitions_for_hide_unordered(hide, inner, out)
            }
        }
    }

    fn transitions_for_expr_unordered(
        &self,
        expr: ExprId,
        env: &BTreeMap<String, u64>,
        out: &mut Vec<(Transition, CspmState)>,
    ) {
        match &self.program.exprs[expr as usize] {
            ExprNode::Stop => {}
            ExprNode::Ref(_) => {
                let target = self.program.resolved[expr as usize];
                let state = state_from_expr(&self.program, target, BTreeMap::new());
                self.transitions_for_state_unordered(&state, out);
            }
            ExprNode::Prefix { event, next } => {
                for (label, next_env) in self.eval_event(event, env) {
                    out.push((
                        Transition { label },
                        state_from_expr(&self.program, *next, next_env),
                    ));
                }
            }
            ExprNode::ChoiceExternal { left, right } => {
                let left_state = state_from_expr(&self.program, *left, env.clone());
                self.transitions_for_state_unordered(&left_state, out);
                let right_state = state_from_expr(&self.program, *right, env.clone());
                self.transitions_for_state_unordered(&right_state, out);
            }
            ExprNode::ChoiceInternal { left, right } => {
                for target in [*left, *right] {
                    out.push((
                        Transition {
                            label: "tau".to_string(),
                        },
                        state_from_expr(&self.program, target, env.clone()),
                    ));
                }
            }
            ExprNode::Parallel { left, right, sync } => {
                let state = CspmState::Parallel {
                    sync: sync.clone(),
                    left: Box::new(state_from_expr(&self.program, *left, env.clone())),
                    right: Box::new(state_from_expr(&self.program, *right, env.clone())),
                };
                self.transitions_for_state_unordered(&state, out);
            }
            ExprNode::Hide { inner, hide } => {
                let state = CspmState::Hide {
                    hide: hide.clone(),
                    inner: Box::new(state_from_expr(&self.program, *inner, env.clone())),
                };
                self.transitions_for_state_unordered(&state, out);
            }
        }
    }

    fn transitions_for_parallel_unordered(
        &self,
        sync: &BTreeSet<String>,
        left: &CspmState,
        right: &CspmState,
        out: &mut Vec<(Transition, CspmState)>,
    ) {
        fn label_channel(label: &str) -> &str {
            label.split_once('.').map(|(ch, _)| ch).unwrap_or(label)
        }

        fn is_sync_event(sync: &BTreeSet<String>, label: &str) -> bool {
            if label == "tau" {
                return false;
            }
            sync.contains(label_channel(label))
        }

        let mut left_next = Vec::new();
        self.transitions_for_state_unordered(left, &mut left_next);
        let mut right_next = Vec::new();
        self.transitions_for_state_unordered(right, &mut right_next);

        let mut right_sync = HashMap::<String, Vec<CspmState>>::new();
        let mut right_nonsync = Vec::new();
        for (transition, next_state) in right_next {
            if is_sync_event(sync, &transition.label) {
                right_sync
                    .entry(transition.label)
                    .or_default()
                    .push(next_state);
            } else {
                right_nonsync.push((transition, next_state));
            }
        }

        for (transition, next_left) in left_next {
            if is_sync_event(sync, &transition.label) {
                let Some(next_right_states) = right_sync.get(&transition.label) else {
                    continue;
                };
                for next_right in next_right_states {
                    out.push((
                        Transition {
                            label: transition.label.clone(),
                        },
                        CspmState::Parallel {
                            sync: sync.clone(),
                            left: Box::new(next_left.clone()),
                            right: Box::new(next_right.clone()),
                        },
                    ));
                }
                continue;
            }

            out.push((
                transition,
                CspmState::Parallel {
                    sync: sync.clone(),
                    left: Box::new(next_left),
                    right: Box::new(right.clone()),
                },
            ));
        }

        for (transition, next_right) in right_nonsync {
            out.push((
                transition,
                CspmState::Parallel {
                    sync: sync.clone(),
                    left: Box::new(left.clone()),
                    right: Box::new(next_right),
                },
            ));
        }
    }

    fn transitions_for_hide_unordered(
        &self,
        hide: &BTreeSet<String>,
        inner: &CspmState,
        out: &mut Vec<(Transition, CspmState)>,
    ) {
        fn label_channel(label: &str) -> &str {
            label.split_once('.').map(|(ch, _)| ch).unwrap_or(label)
        }

        let mut inner_next = Vec::new();
        self.transitions_for_state_unordered(inner, &mut inner_next);
        for (transition, next_inner) in inner_next {
            let label =
                if transition.label != "tau" && hide.contains(label_channel(&transition.label)) {
                    "tau".to_string()
                } else {
                    transition.label
                };
            out.push((
                Transition { label },
                make_hide_state(hide.clone(), next_inner),
            ));
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

fn state_from_expr(program: &Program, expr: ExprId, env: BTreeMap<String, u64>) -> CspmState {
    match &program.exprs[expr as usize] {
        ExprNode::Ref(_) => {
            let target = program.resolved[expr as usize];
            state_from_expr(program, target, BTreeMap::new())
        }
        ExprNode::Parallel { left, right, sync } => CspmState::Parallel {
            sync: sync.clone(),
            left: Box::new(state_from_expr(program, *left, env.clone())),
            right: Box::new(state_from_expr(program, *right, env.clone())),
        },
        ExprNode::Hide { inner, hide } => {
            make_hide_state(hide.clone(), state_from_expr(program, *inner, env))
        }
        _ => CspmState::Expr { expr, env },
    }
}

fn make_hide_state(hide: BTreeSet<String>, inner: CspmState) -> CspmState {
    if hide.is_empty() {
        return inner;
    }
    match inner {
        CspmState::Hide {
            hide: inner_hide,
            inner,
        } => {
            let mut merged = hide;
            merged.extend(inner_hide);
            CspmState::Hide {
                hide: merged,
                inner,
            }
        }
        other => CspmState::Hide {
            hide,
            inner: Box::new(other),
        },
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
            ProcessExpr::Parallel {
                kind,
                left,
                right,
                sync,
            } => {
                let left = self.compile_expr(left)?;
                let right = self.compile_expr(right)?;

                let mut sync_channels = BTreeSet::new();
                match kind {
                    ParallelKind::Interleaving => {}
                    ParallelKind::Interface => {
                        let Some(set) = sync else {
                            return Err(CspmLtsError {
                                message: "missing sync set for interface parallel".to_string(),
                                span: Some(expr.span.clone()),
                            });
                        };
                        for channel in &set.channels {
                            sync_channels.insert(channel.value.clone());
                        }
                    }
                }

                Ok(self.intern(
                    ExprNode::Parallel {
                        left,
                        right,
                        sync: sync_channels,
                    },
                    Some(expr.span.clone()),
                ))
            }
            ProcessExpr::Hide { inner, hide } => {
                let inner = self.compile_expr(inner)?;
                let mut hide_channels = BTreeSet::new();
                for channel in &hide.channels {
                    hide_channels.insert(channel.value.clone());
                }
                Ok(self.intern(
                    ExprNode::Hide {
                        inner,
                        hide: hide_channels,
                    },
                    Some(expr.span.clone()),
                ))
            }
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
