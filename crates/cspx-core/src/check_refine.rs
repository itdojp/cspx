use crate::check::{CheckRequest, CheckResult, Checker, RefinementModel};
use crate::counterexample_span::refinement_counterexample_spans;
use crate::explain::Explainer;
use crate::explain_simple::BasicExplainer;
use crate::ir::Module;
use crate::lts::TransitionProvider;
use crate::lts_cspm::{CspmStateCodec, CspmTransitionProvider};
use crate::minimize::Minimizer;
use crate::minimize_simple::TraceHeuristicMinimizer;
use crate::state_codec::StateCodec;
use crate::types::{
    Counterexample, CounterexampleEvent, CounterexampleType, Reason, ReasonKind, Stats, Status,
};
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};

const TAU: &str = "tau";

type State = <CspmTransitionProvider as TransitionProvider>::State;

#[derive(Debug, Default)]
pub struct RefinementChecker;

#[derive(Debug, Clone)]
pub struct RefinementInput {
    pub spec: Module,
    pub impl_: Module,
}

#[derive(Clone, Debug)]
struct RefinementOutcome {
    pub refines: bool,
    pub failure: Option<RefinementFailure>,
    pub stats: Stats,
    pub diagnostic_tags: Vec<String>,
}

#[derive(Clone, Debug)]
struct RefinementFailure {
    pub trace: Vec<String>,
    pub tags: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct NodeKey {
    impl_sig: Vec<Vec<u8>>,
    spec_sig: Vec<Vec<u8>>,
}

#[derive(Clone, Debug)]
struct Closure {
    states: Vec<State>,
    sig: Vec<Vec<u8>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum ProviderSide {
    Spec,
    Impl,
}

enum NodeAction {
    Continue,
    Prune,
    Fail(RefinementFailure),
}

type NextClosureByLabel = HashMap<String, Closure>;
type NextClosureBySig = HashMap<Vec<Vec<u8>>, NextClosureByLabel>;

#[derive(Debug, Default)]
struct NextClosureCache {
    entries: HashMap<ProviderSide, NextClosureBySig>,
    hits: u64,
    misses: u64,
}

impl NextClosureCache {
    fn next_by_label(
        &mut self,
        side: ProviderSide,
        provider: &CspmTransitionProvider,
        from_closure: &Closure,
        label: &str,
    ) -> Closure {
        if let Some(by_sig) = self.entries.get(&side) {
            if let Some(by_label) = by_sig.get(from_closure.sig.as_slice()) {
                if let Some(cached) = by_label.get(label) {
                    self.hits = self.hits.saturating_add(1);
                    return cached.clone();
                }
            }
        }
        self.misses = self.misses.saturating_add(1);
        let computed = next_by_label(provider, &from_closure.states, label);
        self.entries
            .entry(side)
            .or_default()
            .entry(from_closure.sig.clone())
            .or_default()
            .insert(label.to_string(), computed.clone());
        computed
    }
}

type DivergenceBySig = HashMap<Vec<Vec<u8>>, bool>;

#[derive(Debug, Default)]
struct DivergenceCache {
    entries: HashMap<ProviderSide, DivergenceBySig>,
    hits: u64,
    misses: u64,
}

impl DivergenceCache {
    fn has_tau_cycle(
        &mut self,
        side: ProviderSide,
        closure_sig: &[Vec<u8>],
        provider: &CspmTransitionProvider,
        closure_states: &[State],
    ) -> bool {
        if let Some(by_sig) = self.entries.get(&side) {
            if let Some(cached) = by_sig.get(closure_sig).copied() {
                self.hits = self.hits.saturating_add(1);
                return cached;
            }
        }
        self.misses = self.misses.saturating_add(1);
        let computed = closure_has_tau_cycle(provider, closure_states);
        self.entries
            .entry(side)
            .or_default()
            .insert(closure_sig.to_vec(), computed);
        computed
    }
}

impl Checker<RefinementInput> for RefinementChecker {
    fn check(&self, request: &CheckRequest, input: &RefinementInput) -> CheckResult {
        let model = request.model.clone().unwrap_or(RefinementModel::T);

        let spec_provider = match CspmTransitionProvider::from_module(&input.spec) {
            Ok(provider) => provider,
            Err(err) => {
                return invalid_input_result(request, err.to_string());
            }
        };
        let impl_provider = match CspmTransitionProvider::from_module(&input.impl_) {
            Ok(provider) => provider,
            Err(err) => {
                return invalid_input_result(request, err.to_string());
            }
        };

        let outcome = match model {
            RefinementModel::T => trace_includes(&spec_provider, &impl_provider),
            RefinementModel::F => failures_includes(&spec_provider, &impl_provider),
            RefinementModel::FD => failures_divergences_includes(&spec_provider, &impl_provider),
        };

        if outcome.refines {
            return CheckResult {
                name: "refine".to_string(),
                model: Some(model.as_str().to_string()),
                target: request.target.clone(),
                status: Status::Pass,
                reason: None,
                counterexample: None,
                stats: Some(outcome.stats),
            };
        }

        let failure = outcome.failure.unwrap_or(RefinementFailure {
            trace: Vec::new(),
            tags: Vec::new(),
        });
        let events = failure
            .trace
            .into_iter()
            .map(|label| CounterexampleEvent { label })
            .collect::<Vec<_>>();

        let mut tags = vec![
            "refinement".to_string(),
            format!("model:{}", model.as_str()),
        ];
        tags.extend(outcome.diagnostic_tags);
        tags.extend(failure.tags);
        let required_tags = tags.clone();

        let counterexample = Counterexample {
            kind: CounterexampleType::Trace,
            events,
            is_minimized: false,
            tags,
            source_spans: refinement_counterexample_spans(&input.spec, &input.impl_),
        };
        let minimizer = TraceHeuristicMinimizer;
        let counterexample = minimizer.minimize_with_oracle(counterexample, |candidate| {
            counterexample_still_fails(
                &model,
                &spec_provider,
                &impl_provider,
                candidate,
                &required_tags,
            )
        });
        let explainer = BasicExplainer;
        let counterexample = explainer.explain(counterexample);

        CheckResult {
            name: "refine".to_string(),
            model: Some(model.as_str().to_string()),
            target: request.target.clone(),
            status: Status::Fail,
            reason: None,
            counterexample: Some(counterexample),
            stats: Some(outcome.stats),
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

fn tau_closure(provider: &CspmTransitionProvider, seeds: Vec<State>) -> Closure {
    let codec = CspmStateCodec;

    let mut visited = HashSet::<State>::new();
    let mut queue = VecDeque::<State>::new();
    for seed in seeds {
        if visited.insert(seed.clone()) {
            queue.push_back(seed);
        }
    }

    while let Some(state) = queue.pop_front() {
        for (transition, next_state) in provider.transitions(&state) {
            if transition.label != TAU {
                continue;
            }
            if visited.insert(next_state.clone()) {
                queue.push_back(next_state);
            }
        }
    }

    let mut pairs = visited
        .into_iter()
        .map(|state| (codec.encode(&state), state))
        .collect::<Vec<_>>();
    pairs.sort_by(|(a_bytes, _), (b_bytes, _)| a_bytes.cmp(b_bytes));

    Closure {
        sig: pairs.iter().map(|(bytes, _)| bytes.clone()).collect(),
        states: pairs.into_iter().map(|(_bytes, state)| state).collect(),
    }
}

fn enabled_visible_labels(provider: &CspmTransitionProvider, states: &[State]) -> BTreeSet<String> {
    let mut labels = BTreeSet::new();
    for state in states {
        for (transition, _next) in provider.transitions(state) {
            if transition.label == TAU {
                continue;
            }
            labels.insert(transition.label);
        }
    }
    labels
}

fn next_by_label(
    provider: &CspmTransitionProvider,
    from_closure: &[State],
    label: &str,
) -> Closure {
    let mut seeds = Vec::new();
    for state in from_closure {
        for (transition, next_state) in provider.transitions(state) {
            if transition.label == label {
                seeds.push(next_state);
            }
        }
    }
    tau_closure(provider, seeds)
}

fn reconstruct_trace(
    predecessor: &HashMap<NodeKey, (NodeKey, String)>,
    to: &NodeKey,
) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = to.clone();
    while let Some((prev, label)) = predecessor.get(&cur) {
        out.push(label.clone());
        cur = prev.clone();
    }
    out.reverse();
    out
}

fn is_stable(provider: &CspmTransitionProvider, state: &State) -> bool {
    provider
        .transitions(state)
        .into_iter()
        .all(|(transition, _)| transition.label != TAU)
}

fn offered_visible_labels(provider: &CspmTransitionProvider, state: &State) -> BTreeSet<String> {
    provider
        .transitions(state)
        .into_iter()
        .filter_map(|(transition, _next)| {
            if transition.label == TAU {
                return None;
            }
            Some(transition.label)
        })
        .collect()
}

fn stable_offer_sets(
    provider: &CspmTransitionProvider,
    closure_states: &[State],
) -> Vec<BTreeSet<String>> {
    let mut out = Vec::new();
    for state in closure_states {
        if !is_stable(provider, state) {
            continue;
        }
        out.push(offered_visible_labels(provider, state));
    }
    out
}

fn closure_has_tau_cycle(provider: &CspmTransitionProvider, closure_states: &[State]) -> bool {
    if closure_states.is_empty() {
        return false;
    }
    const WHITE: u8 = 0;
    const GRAY: u8 = 1;
    const BLACK: u8 = 2;

    let mut index_of = HashMap::<State, usize>::new();
    for (idx, state) in closure_states.iter().cloned().enumerate() {
        index_of.insert(state, idx);
    }

    #[derive(Debug)]
    struct DfsFrame {
        node: usize,
        neighbors: Vec<usize>,
        cursor: usize,
    }

    fn tau_successor_indices(
        provider: &CspmTransitionProvider,
        state: &State,
        index_of: &HashMap<State, usize>,
    ) -> Vec<usize> {
        let mut out = Vec::new();
        for (transition, next_state) in provider.transitions(state) {
            if transition.label != TAU {
                continue;
            }
            let Some(next_idx) = index_of.get(&next_state).copied() else {
                continue;
            };
            out.push(next_idx);
        }
        out
    }

    let mut color = vec![WHITE; closure_states.len()];
    for start in 0..closure_states.len() {
        if color[start] != WHITE {
            continue;
        }

        color[start] = GRAY;
        let mut stack = vec![DfsFrame {
            node: start,
            neighbors: tau_successor_indices(provider, &closure_states[start], &index_of),
            cursor: 0,
        }];

        while let Some(frame) = stack.last_mut() {
            if frame.cursor >= frame.neighbors.len() {
                color[frame.node] = BLACK;
                stack.pop();
                continue;
            }
            let next = frame.neighbors[frame.cursor];
            frame.cursor += 1;
            match color[next] {
                WHITE => {
                    color[next] = GRAY;
                    stack.push(DfsFrame {
                        node: next,
                        neighbors: tau_successor_indices(
                            provider,
                            &closure_states[next],
                            &index_of,
                        ),
                        cursor: 0,
                    });
                }
                GRAY => return true,
                _ => {}
            }
        }
    }

    false
}

fn bfs_refinement<F>(
    spec: &CspmTransitionProvider,
    impl_: &CspmTransitionProvider,
    mut node_check: F,
    mut next_closure_cache: Option<&mut NextClosureCache>,
) -> RefinementOutcome
where
    F: FnMut(&NodeKey, &[State], &[State], &HashMap<NodeKey, (NodeKey, String)>) -> NodeAction,
{
    let impl0 = tau_closure(impl_, vec![impl_.initial_state()]);
    let spec0 = tau_closure(spec, vec![spec.initial_state()]);

    let initial_key = NodeKey {
        impl_sig: impl0.sig.clone(),
        spec_sig: spec0.sig.clone(),
    };
    let mut visited = HashSet::<NodeKey>::new();
    visited.insert(initial_key.clone());

    let mut predecessor = HashMap::<NodeKey, (NodeKey, String)>::new();
    let mut queue = VecDeque::<(NodeKey, Closure, Closure)>::new();
    queue.push_back((initial_key.clone(), impl0, spec0));

    let mut states_count: u64 = 1;
    let mut transitions_count: u64 = 0;

    while let Some((node_key, impl_closure, spec_closure)) = queue.pop_front() {
        match node_check(
            &node_key,
            &impl_closure.states,
            &spec_closure.states,
            &predecessor,
        ) {
            NodeAction::Continue => {}
            NodeAction::Prune => continue,
            NodeAction::Fail(failure) => {
                return RefinementOutcome {
                    refines: false,
                    failure: Some(failure),
                    stats: Stats {
                        states: Some(states_count),
                        transitions: Some(transitions_count),
                    },
                    diagnostic_tags: Vec::new(),
                }
            }
        }

        let labels = enabled_visible_labels(impl_, &impl_closure.states);
        for label in labels {
            transitions_count += 1;

            let impl_next = if let Some(cache) = next_closure_cache.as_deref_mut() {
                cache.next_by_label(ProviderSide::Impl, impl_, &impl_closure, &label)
            } else {
                next_by_label(impl_, &impl_closure.states, &label)
            };
            let spec_next = if let Some(cache) = next_closure_cache.as_deref_mut() {
                cache.next_by_label(ProviderSide::Spec, spec, &spec_closure, &label)
            } else {
                next_by_label(spec, &spec_closure.states, &label)
            };
            if spec_next.states.is_empty() {
                let mut trace = reconstruct_trace(&predecessor, &node_key);
                trace.push(label);
                return RefinementOutcome {
                    refines: false,
                    failure: Some(RefinementFailure {
                        trace,
                        tags: vec!["trace_mismatch".to_string()],
                    }),
                    stats: Stats {
                        states: Some(states_count),
                        transitions: Some(transitions_count),
                    },
                    diagnostic_tags: Vec::new(),
                };
            }

            let next_key = NodeKey {
                impl_sig: impl_next.sig.clone(),
                spec_sig: spec_next.sig.clone(),
            };
            if visited.insert(next_key.clone()) {
                predecessor.insert(next_key.clone(), (node_key.clone(), label));
                queue.push_back((next_key, impl_next, spec_next));
                states_count += 1;
            }
        }
    }

    RefinementOutcome {
        refines: true,
        failure: None,
        stats: Stats {
            states: Some(states_count),
            transitions: Some(transitions_count),
        },
        diagnostic_tags: Vec::new(),
    }
}

fn trace_includes(
    spec: &CspmTransitionProvider,
    impl_: &CspmTransitionProvider,
) -> RefinementOutcome {
    bfs_refinement(
        spec,
        impl_,
        |_node_key, _impl_states, _spec_states, _pred| NodeAction::Continue,
        None,
    )
}

fn failures_includes(
    spec: &CspmTransitionProvider,
    impl_: &CspmTransitionProvider,
) -> RefinementOutcome {
    bfs_refinement(
        spec,
        impl_,
        |node_key, impl_states, spec_states, pred| {
            let spec_stable_offers = stable_offer_sets(spec, spec_states);

            for impl_state in impl_states {
                if !is_stable(impl_, impl_state) {
                    continue;
                }
                let impl_offer = offered_visible_labels(impl_, impl_state);
                let ok = spec_stable_offers
                    .iter()
                    .any(|spec_offer| spec_offer.is_subset(&impl_offer));
                if ok {
                    continue;
                }

                return NodeAction::Fail(RefinementFailure {
                    trace: reconstruct_trace(pred, node_key),
                    tags: refusal_mismatch_tags(&spec_stable_offers, &impl_offer),
                });
            }
            NodeAction::Continue
        },
        None,
    )
}

fn failures_divergences_includes(
    spec: &CspmTransitionProvider,
    impl_: &CspmTransitionProvider,
) -> RefinementOutcome {
    let mut next_closure_cache = NextClosureCache::default();
    let mut divergence_cache = DivergenceCache::default();
    let mut divergence_checks = 0u64;
    let mut divergence_prunes = 0u64;
    let mut impl_closure_max = 0u64;
    let mut spec_closure_max = 0u64;

    let mut outcome = bfs_refinement(
        spec,
        impl_,
        |node_key, impl_states, spec_states, pred| {
            impl_closure_max = impl_closure_max.max(impl_states.len() as u64);
            spec_closure_max = spec_closure_max.max(spec_states.len() as u64);
            divergence_checks = divergence_checks.saturating_add(2);

            let spec_diverges = divergence_cache.has_tau_cycle(
                ProviderSide::Spec,
                &node_key.spec_sig,
                spec,
                spec_states,
            );
            let impl_diverges = divergence_cache.has_tau_cycle(
                ProviderSide::Impl,
                &node_key.impl_sig,
                impl_,
                impl_states,
            );
            if impl_diverges && !spec_diverges {
                let mut trace = reconstruct_trace(pred, node_key);
                trace.push(TAU.to_string());
                return NodeAction::Fail(RefinementFailure {
                    trace,
                    tags: vec!["divergence_mismatch".to_string(), "divergence".to_string()],
                });
            }
            if spec_diverges {
                divergence_prunes = divergence_prunes.saturating_add(1);
                return NodeAction::Prune;
            }

            let spec_stable_offers = stable_offer_sets(spec, spec_states);
            for impl_state in impl_states {
                if !is_stable(impl_, impl_state) {
                    continue;
                }
                let impl_offer = offered_visible_labels(impl_, impl_state);
                let ok = spec_stable_offers
                    .iter()
                    .any(|spec_offer| spec_offer.is_subset(&impl_offer));
                if ok {
                    continue;
                }
                return NodeAction::Fail(RefinementFailure {
                    trace: reconstruct_trace(pred, node_key),
                    tags: refusal_mismatch_tags(&spec_stable_offers, &impl_offer),
                });
            }

            NodeAction::Continue
        },
        Some(&mut next_closure_cache),
    );

    outcome.diagnostic_tags = vec![
        format!("fd_nodes:{}", outcome.stats.states.unwrap_or_default()),
        format!("fd_edges:{}", outcome.stats.transitions.unwrap_or_default()),
        format!("fd_divergence_checks:{divergence_checks}"),
        format!("fd_pruned_nodes:{divergence_prunes}"),
        format!("fd_impl_closure_max:{impl_closure_max}"),
        format!("fd_spec_closure_max:{spec_closure_max}"),
        format!("fd_closure_cache_hits:{}", next_closure_cache.hits),
        format!("fd_closure_cache_misses:{}", next_closure_cache.misses),
        format!("fd_divergence_cache_hits:{}", divergence_cache.hits),
        format!("fd_divergence_cache_misses:{}", divergence_cache.misses),
    ];
    outcome
}

#[derive(Clone, Debug)]
struct ReplayOutcome {
    impl_closure: Closure,
    spec_closure: Closure,
    trace_mismatch: bool,
}

fn replay_trace(
    spec: &CspmTransitionProvider,
    impl_: &CspmTransitionProvider,
    trace: &[String],
) -> Option<ReplayOutcome> {
    let mut impl_closure = tau_closure(impl_, vec![impl_.initial_state()]);
    let mut spec_closure = tau_closure(spec, vec![spec.initial_state()]);
    let mut trace_mismatch = false;

    for label in trace {
        let impl_next = next_by_label(impl_, &impl_closure.states, label);
        if impl_next.states.is_empty() {
            return None;
        }
        if !trace_mismatch {
            let spec_next = next_by_label(spec, &spec_closure.states, label);
            if spec_next.states.is_empty() {
                trace_mismatch = true;
            }
            spec_closure = spec_next;
        }
        impl_closure = impl_next;
    }

    Some(ReplayOutcome {
        impl_closure,
        spec_closure,
        trace_mismatch,
    })
}

fn has_refusal_mismatch(
    spec: &CspmTransitionProvider,
    impl_: &CspmTransitionProvider,
    replay: &ReplayOutcome,
) -> bool {
    let spec_stable_offers = stable_offer_sets(spec, &replay.spec_closure.states);
    for impl_state in &replay.impl_closure.states {
        if !is_stable(impl_, impl_state) {
            continue;
        }
        let impl_offer = offered_visible_labels(impl_, impl_state);
        let ok = spec_stable_offers
            .iter()
            .any(|spec_offer| spec_offer.is_subset(&impl_offer));
        if !ok {
            return true;
        }
    }
    false
}

fn counterexample_still_fails(
    model: &RefinementModel,
    spec: &CspmTransitionProvider,
    impl_: &CspmTransitionProvider,
    counterexample: &Counterexample,
    required_tags: &[String],
) -> bool {
    if counterexample.kind != CounterexampleType::Trace {
        return false;
    }
    let trace = counterexample
        .events
        .iter()
        .map(|event| event.label.clone())
        .collect::<Vec<_>>();
    if required_tags.iter().any(|tag| tag == "divergence_mismatch")
        && !trace.iter().any(|label| label == TAU)
    {
        return false;
    }
    let Some(replay) = replay_trace(spec, impl_, &trace) else {
        return false;
    };
    if replay.trace_mismatch {
        return true;
    }

    match model {
        RefinementModel::T => false,
        RefinementModel::F => has_refusal_mismatch(spec, impl_, &replay),
        RefinementModel::FD => {
            let spec_diverges = closure_has_tau_cycle(spec, &replay.spec_closure.states);
            let impl_diverges = closure_has_tau_cycle(impl_, &replay.impl_closure.states);
            if impl_diverges && !spec_diverges {
                return true;
            }
            if spec_diverges {
                return false;
            }
            has_refusal_mismatch(spec, impl_, &replay)
        }
    }
}

fn refusal_mismatch_tags(
    spec_stable_offers: &[BTreeSet<String>],
    impl_offer: &BTreeSet<String>,
) -> Vec<String> {
    let mut tags = vec!["refusal_mismatch".to_string()];
    for spec_offer in spec_stable_offers {
        if let Some(label) = spec_offer.difference(impl_offer).next() {
            tags.push(format!("refuse:{label}"));
            break;
        }
    }
    tags
}
