use crate::check::{CheckRequest, CheckResult, Checker, RefinementModel};
use crate::explain::Explainer;
use crate::explain_simple::BasicExplainer;
use crate::ir::Module;
use crate::lts::TransitionProvider;
use crate::lts_cspm::{CspmStateCodec, CspmTransitionProvider};
use crate::minimize::Minimizer;
use crate::minimize_simple::IdentityMinimizer;
use crate::state_codec::StateCodec;
use crate::types::{
    Counterexample, CounterexampleEvent, CounterexampleType, Reason, ReasonKind, SourceSpan, Stats,
    Status,
};
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};

#[derive(Debug, Default)]
pub struct RefinementChecker;

#[derive(Debug, Clone)]
pub struct RefinementInput {
    pub spec: Module,
    pub impl_: Module,
}

#[derive(Clone, Debug)]
struct TraceInclusionOutcome {
    pub refines: bool,
    pub counterexample: Option<Vec<String>>,
    pub stats: Stats,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct NodeKey {
    impl_sig: Vec<Vec<u8>>,
    spec_sig: Vec<Vec<u8>>,
}

impl Checker<RefinementInput> for RefinementChecker {
    fn check(&self, request: &CheckRequest, input: &RefinementInput) -> CheckResult {
        let model = request.model.clone().unwrap_or(RefinementModel::T);
        if model != RefinementModel::T {
            return not_implemented_result(request, model);
        }

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

        let outcome = trace_includes(&spec_provider, &impl_provider);

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

        let events = outcome
            .counterexample
            .unwrap_or_default()
            .into_iter()
            .map(|label| CounterexampleEvent { label })
            .collect::<Vec<_>>();
        let counterexample = Counterexample {
            kind: CounterexampleType::Trace,
            events,
            is_minimized: false,
            tags: vec![
                "refinement".to_string(),
                format!("model:{}", model.as_str()),
            ],
            source_spans: collect_spans(&input.spec, &input.impl_),
        };
        let minimizer = IdentityMinimizer;
        let counterexample = minimizer.minimize(counterexample);
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

fn not_implemented_result(request: &CheckRequest, model: RefinementModel) -> CheckResult {
    CheckResult {
        name: "refine".to_string(),
        model: Some(model.as_str().to_string()),
        target: request.target.clone(),
        status: Status::Unsupported,
        reason: Some(Reason {
            kind: ReasonKind::NotImplemented,
            message: Some(format!(
                "refinement model {} is not implemented yet",
                model.as_str()
            )),
        }),
        counterexample: None,
        stats: Some(Stats {
            states: None,
            transitions: None,
        }),
    }
}

fn trace_includes(
    spec: &CspmTransitionProvider,
    impl_: &CspmTransitionProvider,
) -> TraceInclusionOutcome {
    type State = <CspmTransitionProvider as TransitionProvider>::State;

    const TAU: &str = "tau";
    let codec = CspmStateCodec;

    fn tau_closure(
        provider: &CspmTransitionProvider,
        codec: &CspmStateCodec,
        seeds: Vec<State>,
    ) -> (Vec<State>, Vec<Vec<u8>>) {
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
        let sig = pairs
            .iter()
            .map(|(bytes, _state)| bytes.clone())
            .collect::<Vec<_>>();
        let states = pairs.into_iter().map(|(_bytes, state)| state).collect();
        (states, sig)
    }

    fn enabled_visible_labels(
        provider: &CspmTransitionProvider,
        states: &[State],
    ) -> BTreeSet<String> {
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
        codec: &CspmStateCodec,
        from_closure: &[State],
        label: &str,
    ) -> (Vec<State>, Vec<Vec<u8>>) {
        let mut seeds = Vec::new();
        for state in from_closure {
            for (transition, next_state) in provider.transitions(state) {
                if transition.label == label {
                    seeds.push(next_state);
                }
            }
        }
        tau_closure(provider, codec, seeds)
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

    let (impl0_states, impl0_sig) = tau_closure(impl_, &codec, vec![impl_.initial_state()]);
    let (spec0_states, spec0_sig) = tau_closure(spec, &codec, vec![spec.initial_state()]);

    let initial_key = NodeKey {
        impl_sig: impl0_sig,
        spec_sig: spec0_sig,
    };
    let mut visited = HashSet::<NodeKey>::new();
    visited.insert(initial_key.clone());

    let mut predecessor = HashMap::<NodeKey, (NodeKey, String)>::new();
    let mut queue = VecDeque::<(NodeKey, Vec<State>, Vec<State>)>::new();
    queue.push_back((initial_key.clone(), impl0_states, spec0_states));

    let mut states_count: u64 = 1;
    let mut transitions_count: u64 = 0;

    while let Some((node_key, impl_states, spec_states)) = queue.pop_front() {
        let labels = enabled_visible_labels(impl_, &impl_states);
        for label in labels {
            transitions_count += 1;

            let (impl_next_states, impl_next_sig) =
                next_by_label(impl_, &codec, &impl_states, &label);
            let (spec_next_states, spec_next_sig) =
                next_by_label(spec, &codec, &spec_states, &label);
            if spec_next_states.is_empty() {
                let mut trace = reconstruct_trace(&predecessor, &node_key);
                trace.push(label);
                return TraceInclusionOutcome {
                    refines: false,
                    counterexample: Some(trace),
                    stats: Stats {
                        states: Some(states_count),
                        transitions: Some(transitions_count),
                    },
                };
            }

            let next_key = NodeKey {
                impl_sig: impl_next_sig,
                spec_sig: spec_next_sig,
            };
            if visited.insert(next_key.clone()) {
                predecessor.insert(next_key.clone(), (node_key.clone(), label));
                queue.push_back((next_key, impl_next_states, spec_next_states));
                states_count += 1;
            }
        }
    }

    TraceInclusionOutcome {
        refines: true,
        counterexample: None,
        stats: Stats {
            states: Some(states_count),
            transitions: Some(transitions_count),
        },
    }
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
