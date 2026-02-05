use crate::check::{CheckRequest, CheckResult, Checker, RefinementModel};
use crate::explain::Explainer;
use crate::explain_simple::BasicExplainer;
use crate::ir::Module;
use crate::minimize::Minimizer;
use crate::minimize_simple::IdentityMinimizer;
use crate::types::{
    Counterexample, CounterexampleEvent, CounterexampleType, Reason, ReasonKind, SourceSpan, Stats,
    Status,
};
use crate::{explore, InMemoryStateStore, SimpleTransitionProvider, VecWorkQueue};
use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Debug, Default)]
pub struct RefinementChecker;

#[derive(Debug, Clone)]
pub struct RefinementInput {
    pub spec: Module,
    pub impl_: Module,
}

impl Checker<RefinementInput> for RefinementChecker {
    fn check(&self, request: &CheckRequest, input: &RefinementInput) -> CheckResult {
        let model = request.model.clone().unwrap_or(RefinementModel::T);
        let spec_provider = match SimpleTransitionProvider::from_module(&input.spec) {
            Ok(provider) => provider,
            Err(err) => {
                return invalid_input_result(request, err.to_string());
            }
        };
        let impl_provider = match SimpleTransitionProvider::from_module(&input.impl_) {
            Ok(provider) => provider,
            Err(err) => {
                return invalid_input_result(request, err.to_string());
            }
        };

        let stats = build_stats(&input.spec);
        let refines = match model {
            RefinementModel::T => trace_refines(&spec_provider, &impl_provider),
            RefinementModel::F => failures_refines(&spec_provider, &impl_provider),
            RefinementModel::FD => failures_divergences_refines(&spec_provider, &impl_provider),
        };

        if refines {
            return CheckResult {
                name: "refine".to_string(),
                model: Some(model.as_str().to_string()),
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
                label: format!("refinement_violation_{}", model.as_str()),
            }],
            is_minimized: false,
            tags: vec![
                "refinement".to_string(),
                format!("model:{}", model.as_str()),
            ],
            source_spans: collect_spans(&input.spec, &input.impl_),
        };
        let minimizer = IdentityMinimizer::default();
        let counterexample = minimizer.minimize(counterexample);
        let explainer = BasicExplainer::default();
        let counterexample = explainer.explain(counterexample);

        CheckResult {
            name: "refine".to_string(),
            model: Some(model.as_str().to_string()),
            target: request.target.clone(),
            status: Status::Fail,
            reason: None,
            counterexample: Some(counterexample),
            stats: Some(stats),
        }
    }
}

fn invalid_input_result(request: &CheckRequest, message: String) -> CheckResult {
    CheckResult {
        name: "refine".to_string(),
        model: request
            .model
            .as_ref()
            .map(RefinementModel::as_str)
            .map(|s| s.to_string()),
        target: request.target.clone(),
        status: Status::Error,
        reason: Some(Reason {
            kind: ReasonKind::InvalidInput,
            message: Some(message),
        }),
        counterexample: None,
        stats: Some(Stats {
            states: None,
            transitions: None,
        }),
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

fn trace_refines(
    spec: &SimpleTransitionProvider,
    impl_: &SimpleTransitionProvider,
) -> bool {
    let mut queue = VecDeque::new();
    let mut visited = HashSet::new();
    let initial = (impl_.initial_state(), spec.initial_state());
    visited.insert(initial.clone());
    queue.push_back(initial);

    while let Some((impl_state, spec_state)) = queue.pop_front() {
        let impl_transitions = impl_.transitions(&impl_state);
        let mut spec_by_label: HashMap<String, Vec<_>> = HashMap::new();
        for (transition, next_state) in spec.transitions(&spec_state) {
            spec_by_label
                .entry(transition.label)
                .or_default()
                .push(next_state);
        }

        for (transition, impl_next) in impl_transitions {
            let Some(spec_nexts) = spec_by_label.get(&transition.label) else {
                return false;
            };
            for spec_next in spec_nexts {
                let pair = (impl_next.clone(), spec_next.clone());
                if visited.insert(pair.clone()) {
                    queue.push_back(pair);
                }
            }
        }
    }

    true
}

fn failures_refines(
    spec: &SimpleTransitionProvider,
    impl_: &SimpleTransitionProvider,
) -> bool {
    trace_refines(spec, impl_)
}

fn failures_divergences_refines(
    spec: &SimpleTransitionProvider,
    impl_: &SimpleTransitionProvider,
) -> bool {
    trace_refines(spec, impl_)
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
