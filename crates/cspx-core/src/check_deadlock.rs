use crate::assertion_select::{
    list_property_assertion_candidates, module_for_property_check, property_kind_str,
};
use crate::check::{CheckRequest, CheckResult, Checker};
use crate::counterexample_span::module_counterexample_spans;
use crate::explain::Explainer;
use crate::explain_simple::BasicExplainer;
use crate::ir::{Module, PropertyKind};
use crate::lts::TransitionProvider;
use crate::lts_cspm::CspmTransitionProvider;
use crate::types::{
    Counterexample, CounterexampleEvent, CounterexampleType, Reason, ReasonKind, Stats, Status,
};
use std::collections::{HashMap, VecDeque};

#[derive(Debug, Default)]
pub struct DeadlockChecker;

impl Checker<Module> for DeadlockChecker {
    fn check(&self, request: &CheckRequest, input: &Module) -> CheckResult {
        let module = module_for_property_check(input, PropertyKind::DeadlockFree);
        match CspmTransitionProvider::from_module(&module) {
            Ok(provider) => deadlock_free_check(&provider, request, &module),
            Err(err) => CheckResult {
                name: "check".to_string(),
                model: None,
                target: request.target.clone(),
                status: Status::Error,
                reason: Some(Reason {
                    kind: ReasonKind::InvalidInput,
                    message: Some(format_invalid_input(&err.to_string(), input)),
                }),
                counterexample: None,
                stats: Some(Stats {
                    states: None,
                    transitions: None,
                }),
            },
        }
    }
}

fn format_invalid_input(original: &str, module: &Module) -> String {
    if original != "entry process not specified" {
        return original.to_string();
    }

    let kind = property_kind_str(PropertyKind::DeadlockFree);
    let candidates = list_property_assertion_candidates(module);
    if candidates.is_empty() {
        return format!(
            "{original}\nhint: add a top-level entry process expression, or add `assert <P> :[{kind} [F]]`"
        );
    }

    let mut lines = vec![
        original.to_string(),
        "available property assertions:".to_string(),
    ];
    lines.extend(candidates.into_iter().map(|c| format!("- {c}")));
    lines.join("\n")
}

fn deadlock_free_check(
    provider: &CspmTransitionProvider,
    request: &CheckRequest,
    module: &Module,
) -> CheckResult {
    let mut visited: HashMap<
        <CspmTransitionProvider as TransitionProvider>::State,
        Option<(
            <CspmTransitionProvider as TransitionProvider>::State,
            String,
        )>,
    > = HashMap::new();
    let mut queue: VecDeque<<CspmTransitionProvider as TransitionProvider>::State> =
        VecDeque::new();
    let mut states: u64 = 0;
    let mut transitions: u64 = 0;

    let initial = provider.initial_state();
    visited.insert(initial.clone(), None);
    queue.push_back(initial.clone());
    states += 1;

    let mut deadlock_state = None;

    while let Some(state) = queue.pop_front() {
        let next = provider.transitions(&state);
        transitions += next.len() as u64;
        if next.is_empty() {
            deadlock_state = Some(state);
            break;
        }
        for (transition, next_state) in next {
            if visited.contains_key(&next_state) {
                continue;
            }
            visited.insert(
                next_state.clone(),
                Some((state.clone(), transition.label.clone())),
            );
            queue.push_back(next_state);
            states += 1;
        }
    }

    let stats = Stats {
        states: Some(states),
        transitions: Some(transitions),
    };

    if let Some(state) = deadlock_state {
        let events = trace_events(&visited, state);
        let counterexample = Counterexample {
            kind: CounterexampleType::Trace,
            events,
            is_minimized: false,
            tags: vec!["deadlock".to_string()],
            source_spans: module_counterexample_spans(module),
        };
        let explainer = BasicExplainer;
        let counterexample = explainer.explain(counterexample);
        return CheckResult {
            name: "check".to_string(),
            model: None,
            target: request.target.clone(),
            status: Status::Fail,
            reason: None,
            counterexample: Some(counterexample),
            stats: Some(stats),
        };
    }

    CheckResult {
        name: "check".to_string(),
        model: None,
        target: request.target.clone(),
        status: Status::Pass,
        reason: None,
        counterexample: None,
        stats: Some(stats),
    }
}

fn trace_events<S>(
    visited: &HashMap<S, Option<(S, String)>>,
    mut current: S,
) -> Vec<CounterexampleEvent>
where
    S: Eq + std::hash::Hash + Clone,
{
    let mut labels = Vec::new();
    while let Some(Some((prev, label))) = visited.get(&current) {
        labels.push(label.clone());
        current = prev.clone();
    }
    labels.reverse();
    labels
        .into_iter()
        .filter(|label| label != "tau")
        .map(|label| CounterexampleEvent { label })
        .collect()
}
