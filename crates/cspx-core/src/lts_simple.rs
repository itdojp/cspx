use crate::ir::{Module, ProcessExpr, Spanned};
use crate::lts::{Transition, TransitionProvider};
use crate::types::SourceSpan;
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SimpleState {
    Stop,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SimpleStateCodec;

impl crate::state_codec::StateCodec<SimpleState> for SimpleStateCodec {
    fn encode(&self, state: &SimpleState) -> Vec<u8> {
        match state {
            SimpleState::Stop => b"STOP".to_vec(),
        }
    }

    fn decode(&self, bytes: &[u8]) -> Result<SimpleState, crate::state_codec::StateCodecError> {
        match bytes {
            b"STOP" => Ok(SimpleState::Stop),
            _ => Err(crate::state_codec::StateCodecError::new(
                "unknown SimpleState encoding",
            )),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SimpleTransitionProvider {
    initial: SimpleState,
}

#[derive(Debug, Clone)]
pub struct LtsError {
    pub message: String,
    pub span: Option<SourceSpan>,
}

impl Display for LtsError {
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

impl std::error::Error for LtsError {}

impl SimpleTransitionProvider {
    pub fn from_module(module: &Module) -> Result<Self, LtsError> {
        if let Some(entry) = &module.entry {
            return Ok(Self {
                initial: state_from_expr(entry)?,
            });
        }
        if module.declarations.len() == 1 {
            return Ok(Self {
                initial: state_from_expr(&module.declarations[0].expr)?,
            });
        }
        Err(LtsError {
            message: "entry process not specified".to_string(),
            span: None,
        })
    }
}

impl TransitionProvider for SimpleTransitionProvider {
    type State = SimpleState;
    type Transition = Transition;

    fn initial_state(&self) -> Self::State {
        self.initial.clone()
    }

    fn transitions(&self, _state: &Self::State) -> Vec<(Self::Transition, Self::State)> {
        Vec::new()
    }
}

fn state_from_expr(expr: &Spanned<ProcessExpr>) -> Result<SimpleState, LtsError> {
    match &expr.value {
        ProcessExpr::Stop => Ok(SimpleState::Stop),
        _ => Err(LtsError {
            message: "process expression is not supported by SimpleTransitionProvider yet"
                .to_string(),
            span: Some(expr.span.clone()),
        }),
    }
}
