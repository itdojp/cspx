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
use std::collections::{HashMap, VecDeque};

#[derive(Debug, Default)]
pub struct DivergenceChecker;

impl Checker<Module> for DivergenceChecker {
    fn check(&self, request: &CheckRequest, input: &Module) -> CheckResult {
        let module = module_for_property_check(input, PropertyKind::DivergenceFree);
        match CspmTransitionProvider::from_module(&module) {
            Ok(provider) => divergence_free_check(&provider, request, &module),
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

    let kind = property_kind_str(PropertyKind::DivergenceFree);
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

fn divergence_free_check(
    provider: &CspmTransitionProvider,
    request: &CheckRequest,
    module: &Module,
) -> CheckResult {
    type State = <CspmTransitionProvider as TransitionProvider>::State;

    let mut visited: HashMap<State, Option<(State, String)>> = HashMap::new();
    let mut order: Vec<State> = Vec::new();
    let mut tau_edges: HashMap<State, Vec<State>> = HashMap::new();
    let mut queue: VecDeque<State> = VecDeque::new();
    let mut states: u64 = 0;
    let mut transitions: u64 = 0;

    let initial = provider.initial_state();
    visited.insert(initial.clone(), None);
    order.push(initial.clone());
    queue.push_back(initial);
    states += 1;

    while let Some(state) = queue.pop_front() {
        let next = provider.transitions(&state);
        transitions += next.len() as u64;
        for (transition, next_state) in next {
            if transition.label == "tau" {
                tau_edges
                    .entry(state.clone())
                    .or_default()
                    .push(next_state.clone());
            }
            if visited.contains_key(&next_state) {
                continue;
            }
            visited.insert(
                next_state.clone(),
                Some((state.clone(), transition.label.clone())),
            );
            order.push(next_state.clone());
            queue.push_back(next_state);
            states += 1;
        }
    }

    let stats = Stats {
        states: Some(states),
        transitions: Some(transitions),
    };

    let mut index_of = HashMap::<State, usize>::new();
    for (idx, state) in order.iter().cloned().enumerate() {
        index_of.insert(state, idx);
    }
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); order.len()];
    for (idx, state) in order.iter().enumerate() {
        let Some(nexts) = tau_edges.get(state) else {
            continue;
        };
        for next in nexts {
            let Some(next_idx) = index_of.get(next).copied() else {
                continue;
            };
            adj[idx].push(next_idx);
        }
    }

    let Some(cycle_state_idx) = find_first_tau_cycle_node(&adj) else {
        return CheckResult {
            name: "check".to_string(),
            model: None,
            target: request.target.clone(),
            status: Status::Pass,
            reason: None,
            counterexample: None,
            stats: Some(stats),
        };
    };

    let cycle_state = order[cycle_state_idx].clone();
    let mut events = trace_visible_events(&visited, cycle_state);
    events.push(CounterexampleEvent {
        label: "tau".to_string(),
    });
    let counterexample = Counterexample {
        kind: CounterexampleType::Trace,
        events,
        is_minimized: false,
        tags: vec!["divergence".to_string()],
        source_spans: module_spans(module),
    };

    CheckResult {
        name: "check".to_string(),
        model: None,
        target: request.target.clone(),
        status: Status::Fail,
        reason: None,
        counterexample: Some(counterexample),
        stats: Some(stats),
    }
}

fn find_first_tau_cycle_node(adj: &[Vec<usize>]) -> Option<usize> {
    let sccs = tarjan_scc(adj);
    let mut in_cycle = vec![false; adj.len()];
    for scc in sccs {
        if scc.len() > 1 {
            for v in scc {
                in_cycle[v] = true;
            }
            continue;
        }
        let v = scc[0];
        if adj[v].contains(&v) {
            in_cycle[v] = true;
        }
    }
    in_cycle.iter().position(|&v| v)
}

fn tarjan_scc(adj: &[Vec<usize>]) -> Vec<Vec<usize>> {
    #[allow(clippy::too_many_arguments)]
    fn strong_connect(
        v: usize,
        next_index: &mut usize,
        indices: &mut [Option<usize>],
        lowlink: &mut [usize],
        stack: &mut Vec<usize>,
        onstack: &mut [bool],
        adj: &[Vec<usize>],
        out: &mut Vec<Vec<usize>>,
    ) {
        indices[v] = Some(*next_index);
        lowlink[v] = *next_index;
        *next_index += 1;
        stack.push(v);
        onstack[v] = true;

        for &w in &adj[v] {
            if indices[w].is_none() {
                strong_connect(w, next_index, indices, lowlink, stack, onstack, adj, out);
                lowlink[v] = lowlink[v].min(lowlink[w]);
                continue;
            }

            if onstack[w] {
                lowlink[v] = lowlink[v].min(indices[w].unwrap_or(lowlink[w]));
            }
        }

        if lowlink[v] == indices[v].unwrap_or(lowlink[v]) {
            let mut scc = Vec::new();
            while let Some(w) = stack.pop() {
                onstack[w] = false;
                scc.push(w);
                if w == v {
                    break;
                }
            }
            out.push(scc);
        }
    }

    let n = adj.len();
    let mut next_index = 0;
    let mut indices = vec![None; n];
    let mut lowlink = vec![0; n];
    let mut stack = Vec::new();
    let mut onstack = vec![false; n];
    let mut out = Vec::new();

    for v in 0..n {
        if indices[v].is_none() {
            strong_connect(
                v,
                &mut next_index,
                &mut indices,
                &mut lowlink,
                &mut stack,
                &mut onstack,
                adj,
                &mut out,
            );
        }
    }
    out
}

fn trace_visible_events<S>(
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

fn module_spans(module: &Module) -> Vec<SourceSpan> {
    if let Some(entry) = &module.entry {
        return vec![entry.span.clone()];
    }
    if let Some(decl) = module.declarations.first() {
        return vec![decl.expr.span.clone()];
    }
    Vec::new()
}
