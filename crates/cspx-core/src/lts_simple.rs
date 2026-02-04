use crate::ir::{Module, ProcessExpr, Spanned};
use crate::lts::{Transition, TransitionProvider};
use crate::types::SourceSpan;
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SimpleState {
    Stop,
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
                initial: state_from_expr(entry),
            });
        }
        if module.declarations.len() == 1 {
            return Ok(Self {
                initial: state_from_expr(&module.declarations[0].expr),
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

fn state_from_expr(expr: &Spanned<ProcessExpr>) -> SimpleState {
    match expr.value {
        ProcessExpr::Stop => SimpleState::Stop,
    }
}
