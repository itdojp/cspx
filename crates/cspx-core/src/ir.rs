use std::fmt::Debug;

pub trait CoreIr: Debug + Send + Sync {}

#[derive(Debug, Clone)]
pub struct Module;

impl CoreIr for Module {}
