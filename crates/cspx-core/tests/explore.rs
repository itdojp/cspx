use cspx_core::{
    explore, explore_parallel, explore_parallel_profiled_with_options,
    explore_parallel_with_options, explore_profiled, ExploreProfileMode, InMemoryStateStore,
    ParallelExploreOptions, SimpleTransitionProvider, StateStore, Transition, TransitionProvider,
    VecWorkQueue,
};
use std::collections::HashSet;

#[test]
fn explore_stop_yields_single_state() {
    let module = cspx_core::ir::Module {
        channels: Vec::new(),
        declarations: Vec::new(),
        assertions: Vec::new(),
        entry: Some(cspx_core::ir::Spanned {
            value: cspx_core::ir::ProcessExpr::Stop,
            span: cspx_core::types::SourceSpan {
                path: "test.cspm".to_string(),
                start_line: 1,
                start_col: 1,
                end_line: 1,
                end_col: 4,
            },
        }),
    };

    let provider = SimpleTransitionProvider::from_module(&module).expect("provider");
    let mut store = InMemoryStateStore::new();
    let mut queue = VecWorkQueue::new();
    let stats = explore(&provider, &mut store, &mut queue).expect("explore");

    assert_eq!(stats.states, Some(1));
    assert_eq!(stats.transitions, Some(0));
}

#[test]
fn explore_parallel_stop_yields_single_state() {
    let module = cspx_core::ir::Module {
        channels: Vec::new(),
        declarations: Vec::new(),
        assertions: Vec::new(),
        entry: Some(cspx_core::ir::Spanned {
            value: cspx_core::ir::ProcessExpr::Stop,
            span: cspx_core::types::SourceSpan {
                path: "test.cspm".to_string(),
                start_line: 1,
                start_col: 1,
                end_line: 1,
                end_col: 4,
            },
        }),
    };

    let provider = SimpleTransitionProvider::from_module(&module).expect("provider");
    let mut store = InMemoryStateStore::new();
    let stats = explore_parallel(&provider, &mut store, 2).expect("explore_parallel");

    assert_eq!(stats.states, Some(1));
    assert_eq!(stats.transitions, Some(0));
}

#[derive(Default)]
struct OrderedStore {
    seen: HashSet<u8>,
    insertion_order: Vec<u8>,
}

impl OrderedStore {
    fn new() -> Self {
        Self::default()
    }
}

impl StateStore<u8> for OrderedStore {
    fn insert(&mut self, state: u8) -> std::io::Result<bool> {
        if self.seen.insert(state) {
            self.insertion_order.push(state);
            return Ok(true);
        }
        Ok(false)
    }

    fn len(&self) -> usize {
        self.seen.len()
    }
}

#[derive(Clone, Copy)]
struct BranchyProvider;

impl BranchyProvider {
    fn tr(label: &str) -> Transition {
        Transition {
            label: label.to_string(),
        }
    }
}

impl TransitionProvider for BranchyProvider {
    type State = u8;
    type Transition = Transition;

    fn initial_state(&self) -> Self::State {
        0
    }

    fn transitions(&self, state: &Self::State) -> Vec<(Self::Transition, Self::State)> {
        match *state {
            0 => vec![(Self::tr("b"), 2), (Self::tr("a"), 1)],
            1 => vec![(Self::tr("d"), 4), (Self::tr("c"), 3)],
            2 => vec![(Self::tr("f"), 6), (Self::tr("e"), 5)],
            _ => Vec::new(),
        }
    }
}

#[derive(Clone, Copy)]
struct DenseDuplicateProvider {
    depth: u8,
    fanout: u8,
    duplicate_factor: u8,
}

impl DenseDuplicateProvider {
    fn tr(label: &str) -> Transition {
        Transition {
            label: label.to_string(),
        }
    }
}

impl TransitionProvider for DenseDuplicateProvider {
    type State = (u8, u8);
    type Transition = Transition;

    fn initial_state(&self) -> Self::State {
        (0, 0)
    }

    fn transitions(&self, state: &Self::State) -> Vec<(Self::Transition, Self::State)> {
        let (layer, _node) = *state;
        if layer >= self.depth {
            return Vec::new();
        }

        let mut out = Vec::with_capacity(self.fanout as usize * self.duplicate_factor as usize);
        for branch in 0..self.fanout {
            let next_state = (layer + 1, branch);
            for _ in 0..self.duplicate_factor {
                out.push((Self::tr("step"), next_state));
            }
        }
        out
    }
}

#[test]
fn explore_parallel_deterministic_mode_is_reproducible() {
    let provider = BranchyProvider;
    let options = ParallelExploreOptions {
        workers: 2,
        deterministic: true,
        seed: 3,
    };

    let mut store1 = OrderedStore::new();
    let stats1 =
        explore_parallel_with_options(&provider, &mut store1, options).expect("deterministic #1");

    let mut store2 = OrderedStore::new();
    let stats2 =
        explore_parallel_with_options(&provider, &mut store2, options).expect("deterministic #2");

    assert_eq!(stats1, stats2);
    assert_eq!(store1.insertion_order, store2.insertion_order);
}

#[test]
fn explore_parallel_deterministic_mode_normalizes_frontier_order() {
    let provider = BranchyProvider;
    let mut store = OrderedStore::new();
    let stats = explore_parallel_with_options(
        &provider,
        &mut store,
        ParallelExploreOptions {
            workers: 2,
            deterministic: true,
            seed: 42,
        },
    )
    .expect("deterministic");

    assert_eq!(stats.states, Some(7));
    assert_eq!(stats.transitions, Some(6));
    assert_eq!(store.insertion_order, vec![0, 1, 2, 3, 4, 5, 6]);
}

#[test]
fn explore_profiled_matches_explore_stats() {
    let module = cspx_core::ir::Module {
        channels: Vec::new(),
        declarations: Vec::new(),
        assertions: Vec::new(),
        entry: Some(cspx_core::ir::Spanned {
            value: cspx_core::ir::ProcessExpr::Stop,
            span: cspx_core::types::SourceSpan {
                path: "test.cspm".to_string(),
                start_line: 1,
                start_col: 1,
                end_line: 1,
                end_col: 4,
            },
        }),
    };
    let provider = SimpleTransitionProvider::from_module(&module).expect("provider");

    let mut store_plain = InMemoryStateStore::new();
    let mut queue_plain = VecWorkQueue::new();
    let plain = explore(&provider, &mut store_plain, &mut queue_plain).expect("plain");

    let mut store_profiled = InMemoryStateStore::new();
    let mut queue_profiled = VecWorkQueue::new();
    let (profiled, profile) =
        explore_profiled(&provider, &mut store_profiled, &mut queue_profiled).expect("profiled");

    assert_eq!(plain, profiled);
    assert_eq!(profile.mode, ExploreProfileMode::Serial);
    assert_eq!(profile.workers, 1);
    assert_eq!(profile.discovered_states, 1);
}

#[test]
fn explore_parallel_profiled_deterministic_matches_regular_stats() {
    let provider = BranchyProvider;
    let options = ParallelExploreOptions {
        workers: 2,
        deterministic: true,
        seed: 123,
    };
    let mut plain_store = OrderedStore::new();
    let plain =
        explore_parallel_with_options(&provider, &mut plain_store, options).expect("plain stats");

    let mut profiled_store = OrderedStore::new();
    let (profiled, profile) =
        explore_parallel_profiled_with_options(&provider, &mut profiled_store, options)
            .expect("profiled stats");

    assert_eq!(plain, profiled);
    assert_eq!(profile.mode, ExploreProfileMode::ParallelDeterministic);
    assert_eq!(profile.workers, 2);
    assert_eq!(
        profile.discovered_states,
        profiled.states.unwrap_or_default()
    );
    assert!(profile.generated_transitions > 0);
}

#[test]
fn explore_parallel_deterministic_dense_duplicates_preserve_stats() {
    let provider = DenseDuplicateProvider {
        depth: 5,
        fanout: 24,
        duplicate_factor: 6,
    };
    let options = ParallelExploreOptions {
        workers: 4,
        deterministic: true,
        seed: 99,
    };

    let mut plain_store = InMemoryStateStore::new();
    let plain =
        explore_parallel_with_options(&provider, &mut plain_store, options).expect("plain stats");

    let mut profiled_store = InMemoryStateStore::new();
    let (profiled, profile) =
        explore_parallel_profiled_with_options(&provider, &mut profiled_store, options)
            .expect("profiled stats");

    assert_eq!(plain, profiled);
    assert_eq!(profiled.states, Some(121));
    assert_eq!(profiled.transitions, Some(13_968));
    assert_eq!(profile.levels, 6);
    assert_eq!(profile.discovered_states, 121);
    assert_eq!(profile.generated_transitions, 13_968);
    assert!(profile.frontier_maintenance_ns > 0);
}
