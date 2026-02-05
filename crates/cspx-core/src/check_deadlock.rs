use crate::check::{CheckRequest, CheckResult, Checker};
use crate::ir::Module;
use crate::lts::TransitionProvider;
use crate::lts_simple::SimpleTransitionProvider;
use crate::types::{
    Counterexample, CounterexampleEvent, CounterexampleType, Reason, ReasonKind, SourceSpan, Stats,
    Status,
};
use std::collections::{HashMap, VecDeque};

#[derive(Debug, Default)]
pub struct DeadlockChecker;

impl Checker<Module> for DeadlockChecker {
    fn check(&self, request: &CheckRequest, input: &Module) -> CheckResult {
        match SimpleTransitionProvider::from_module(input) {
            Ok(provider) => deadlock_free_check(&provider, request, input),
            Err(err) => CheckResult {
                name: "check".to_string(),
                model: None,
                target: request.target.clone(),
                status: Status::Error,
                reason: Some(Reason {
                    kind: ReasonKind::InvalidInput,
                    message: Some(err.to_string()),
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

fn deadlock_free_check(
    provider: &SimpleTransitionProvider,
    request: &CheckRequest,
    module: &Module,
) -> CheckResult {
    let mut visited: HashMap<
        <SimpleTransitionProvider as TransitionProvider>::State,
        Option<(
            <SimpleTransitionProvider as TransitionProvider>::State,
            String,
        )>,
    > = HashMap::new();
    let mut queue: VecDeque<<SimpleTransitionProvider as TransitionProvider>::State> =
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
            source_spans: module_spans(module),
        };
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
        .map(|label| CounterexampleEvent { label })
        .collect()
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
