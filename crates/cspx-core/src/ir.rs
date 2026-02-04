use crate::types::SourceSpan;
use std::fmt::Debug;

pub trait CoreIr: Debug + Send + Sync {}

#[derive(Debug, Clone)]
pub struct Spanned<T> {
    pub value: T,
    pub span: SourceSpan,
}

#[derive(Debug, Clone)]
pub enum ProcessExpr {
    Stop,
}

#[derive(Debug, Clone)]
pub struct ProcessDecl {
    pub name: String,
    pub expr: Spanned<ProcessExpr>,
}

#[derive(Debug, Clone)]
pub struct Module {
    pub declarations: Vec<ProcessDecl>,
    pub entry: Option<Spanned<ProcessExpr>>,
}

impl CoreIr for Module {}
