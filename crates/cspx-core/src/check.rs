use crate::types::{Counterexample, Reason, Stats, Status};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CheckCommand {
    Typecheck,
    Check,
    Refine,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum RefinementModel {
    T,
    F,
    FD,
}

impl RefinementModel {
    pub fn as_str(&self) -> &'static str {
        match self {
            RefinementModel::T => "T",
            RefinementModel::F => "F",
            RefinementModel::FD => "FD",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CheckRequest {
    pub command: CheckCommand,
    pub model: Option<RefinementModel>,
    pub target: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CheckResult {
    pub name: String,
    pub model: Option<String>,
    pub target: Option<String>,
    pub status: Status,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<Reason>,
    pub counterexample: Option<Counterexample>,
    pub stats: Option<Stats>,
}

pub trait Checker<I> {
    fn check(&self, request: &CheckRequest, input: &I) -> CheckResult;
}
