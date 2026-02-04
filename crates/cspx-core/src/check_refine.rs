use crate::check::{CheckRequest, CheckResult, Checker, RefinementModel};
use crate::explain::Explainer;
use crate::explain_simple::BasicExplainer;
use crate::ir::Module;
use crate::minimize::Minimizer;
use crate::minimize_simple::IdentityMinimizer;
use crate::types::{
    Counterexample, CounterexampleEvent, CounterexampleType, SourceSpan, Stats, Status,
};
use crate::{explore, InMemoryStateStore, SimpleTransitionProvider, VecWorkQueue};

#[derive(Debug, Default)]
pub struct RefinementChecker;

#[derive(Debug, Clone)]
pub struct RefinementInput {
    pub spec: Module,
    pub impl_: Module,
}

impl Checker<RefinementInput> for RefinementChecker {
    fn check(&self, request: &CheckRequest, input: &RefinementInput) -> CheckResult {
        let stats = build_stats(&input.spec);
        let signature_spec = signature(&input.spec);
        let signature_impl = signature(&input.impl_);

        if signature_spec == signature_impl {
            return CheckResult {
                name: "refine".to_string(),
                model: request
                    .model
                    .as_ref()
                    .map(RefinementModel::as_str)
                    .map(|s| s.to_string()),
                target: request.target.clone(),
                status: Status::Pass,
                reason: None,
                counterexample: None,
                stats: Some(stats),
            };
        }

        let counterexample = Counterexample {
            kind: CounterexampleType::Trace,
            events: vec![CounterexampleEvent {
                label: "refinement_mismatch".to_string(),
            }],
            is_minimized: false,
            tags: vec!["refinement".to_string()],
            source_spans: collect_spans(&input.spec, &input.impl_),
        };
        let minimizer = IdentityMinimizer::default();
        let counterexample = minimizer.minimize(counterexample);
        let explainer = BasicExplainer::default();
        let counterexample = explainer.explain(counterexample);

        CheckResult {
            name: "refine".to_string(),
            model: request
                .model
                .as_ref()
                .map(RefinementModel::as_str)
                .map(|s| s.to_string()),
            target: request.target.clone(),
            status: Status::Fail,
            reason: None,
            counterexample: Some(counterexample),
            stats: Some(stats),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Signature {
    entry_present: bool,
    decls: usize,
    stop_count: usize,
}

fn signature(module: &Module) -> Signature {
    let mut stop_count = 0;
    if module.entry.is_some() {
        stop_count += 1;
    }
    stop_count += module.declarations.len();
    Signature {
        entry_present: module.entry.is_some(),
        decls: module.declarations.len(),
        stop_count,
    }
}

fn build_stats(module: &Module) -> Stats {
    let provider = match SimpleTransitionProvider::from_module(module) {
        Ok(provider) => provider,
        Err(_) => {
            return Stats {
                states: None,
                transitions: None,
            }
        }
    };

    let mut store = InMemoryStateStore::new();
    let mut queue = VecWorkQueue::new();
    explore(&provider, &mut store, &mut queue)
}

fn collect_spans(spec: &Module, impl_: &Module) -> Vec<SourceSpan> {
    let mut spans = Vec::new();
    spans.extend(module_spans(spec));
    spans.extend(module_spans(impl_));
    spans
}

fn module_spans(module: &Module) -> Vec<SourceSpan> {
    if let Some(entry) = &module.entry {
        return vec![entry.span.clone()];
    }
    if let Some(decl) = module.declarations.first() {
        return vec![decl.expr.span.clone()];
    }
    Vec::new()
}
