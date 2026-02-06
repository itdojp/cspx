use crate::assertion_select::{
    list_property_assertion_candidates, module_for_property_check, property_kind_str,
};
use crate::check::{CheckRequest, CheckResult, Checker};
use crate::ir::{Module, PropertyKind};
use crate::lts::TransitionProvider;
use crate::lts_cspm::CspmTransitionProvider;
use crate::types::{
    Counterexample, CounterexampleEvent, CounterexampleType, Reason, ReasonKind, SourceSpan, Stats,
    Status,
};
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

#[derive(Debug, Default)]
pub struct DeterminismChecker;

impl Checker<Module> for DeterminismChecker {
    fn check(&self, request: &CheckRequest, input: &Module) -> CheckResult {
        let module = module_for_property_check(input, PropertyKind::Deterministic);
        match CspmTransitionProvider::from_module(&module) {
            Ok(provider) => determinism_check(&provider, request, &module),
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

    let kind = property_kind_str(PropertyKind::Deterministic);
    let candidates = list_property_assertion_candidates(module);
    if candidates.is_empty() {
        return format!(
            "{original}\nhint: add a top-level entry process expression, or add `assert <P> :[{kind} [FD]]`"
        );
    }

    let mut lines = vec![
        original.to_string(),
        "available property assertions:".to_string(),
    ];
    lines.extend(candidates.into_iter().map(|c| format!("- {c}")));
    lines.join("\n")
}

fn determinism_check(
    provider: &CspmTransitionProvider,
    request: &CheckRequest,
    module: &Module,
) -> CheckResult {
    type State = <CspmTransitionProvider as TransitionProvider>::State;

    let mut index_of: HashMap<State, usize> = HashMap::new();
    let mut states: Vec<State> = Vec::new();
    let mut prev: Vec<Option<(usize, String)>> = Vec::new();
    let mut transitions_from: Vec<Vec<(String, usize)>> = Vec::new();
    let mut tau_from: Vec<Vec<usize>> = Vec::new();
    let mut queue: VecDeque<usize> = VecDeque::new();

    let mut states_count: u64 = 0;
    let mut transitions_count: u64 = 0;

    let initial = provider.initial_state();
    index_of.insert(initial.clone(), 0);
    states.push(initial);
    prev.push(None);
    transitions_from.push(Vec::new());
    tau_from.push(Vec::new());
    queue.push_back(0);
    states_count += 1;

    while let Some(idx) = queue.pop_front() {
        let state = &states[idx];
        let next = provider.transitions(state);
        transitions_count += next.len() as u64;
        for (transition, next_state) in next {
            let next_idx = if let Some(&existing) = index_of.get(&next_state) {
                existing
            } else {
                let new_idx = states.len();
                index_of.insert(next_state.clone(), new_idx);
                states.push(next_state.clone());
                prev.push(Some((idx, transition.label.clone())));
                transitions_from.push(Vec::new());
                tau_from.push(Vec::new());
                queue.push_back(new_idx);
                states_count += 1;
                new_idx
            };

            transitions_from[idx].push((transition.label.clone(), next_idx));
            if transition.label == "tau" {
                tau_from[idx].push(next_idx);
            }
        }
    }

    let stats = Stats {
        states: Some(states_count),
        transitions: Some(transitions_count),
    };

    let tau_closures = compute_tau_closures(tau_from);

    for (state_idx, closure) in tau_closures.iter().enumerate() {
        let mut by_label: BTreeMap<String, BTreeSet<Vec<usize>>> = BTreeMap::new();
        for &u in closure {
            for (label, next) in &transitions_from[u] {
                if label == "tau" {
                    continue;
                }
                by_label
                    .entry(label.clone())
                    .or_default()
                    .insert(tau_closures[*next].clone());
            }
        }

        for (label, targets) in by_label {
            if targets.len() <= 1 {
                continue;
            }

            let mut events = trace_visible_events(&prev, state_idx);
            events.push(CounterexampleEvent {
                label: label.clone(),
            });
            let counterexample = Counterexample {
                kind: CounterexampleType::Trace,
                events,
                is_minimized: false,
                tags: vec!["nondeterminism".to_string(), format!("label:{label}")],
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

fn compute_tau_closures(tau_from: Vec<Vec<usize>>) -> Vec<Vec<usize>> {
    let n = tau_from.len();
    let mut out = Vec::new();
    for start in 0..n {
        let mut seen = vec![false; n];
        let mut queue = VecDeque::new();
        seen[start] = true;
        queue.push_back(start);
        let mut closure = Vec::new();
        while let Some(u) = queue.pop_front() {
            closure.push(u);
            for &v in &tau_from[u] {
                if seen[v] {
                    continue;
                }
                seen[v] = true;
                queue.push_back(v);
            }
        }
        closure.sort();
        out.push(closure);
    }
    out
}

fn trace_visible_events(
    prev: &[Option<(usize, String)>],
    mut current: usize,
) -> Vec<CounterexampleEvent> {
    let mut labels = Vec::new();
    while let Some((prev_idx, label)) = prev[current].as_ref() {
        labels.push(label.clone());
        current = *prev_idx;
    }
    labels.reverse();
    labels
        .into_iter()
        .filter(|label| label != "tau")
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
