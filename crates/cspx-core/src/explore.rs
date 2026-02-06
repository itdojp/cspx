use crate::lts::TransitionProvider;
use crate::queue::WorkQueue;
use crate::store::StateStore;
use crate::types::Stats;
use rayon::prelude::*;
use rayon::ThreadPoolBuilder;
use std::io;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParallelExploreOptions {
    pub workers: usize,
    pub deterministic: bool,
    pub seed: u64,
}

impl Default for ParallelExploreOptions {
    fn default() -> Self {
        Self {
            workers: 1,
            deterministic: false,
            seed: 0,
        }
    }
}

pub fn explore<P, SStore, Q>(
    provider: &P,
    store: &mut SStore,
    queue: &mut Q,
) -> std::io::Result<Stats>
where
    P: TransitionProvider,
    P::State: Clone,
    SStore: StateStore<P::State>,
    Q: WorkQueue<P::State>,
{
    let mut states: u64 = 0;
    let mut transitions: u64 = 0;

    let initial = provider.initial_state();
    if store.insert(initial.clone())? {
        queue.push(initial);
        states += 1;
    }

    while let Some(state) = queue.pop() {
        let next = provider.transitions(&state);
        transitions += next.len() as u64;
        for (_label, next_state) in next {
            if store.insert(next_state.clone())? {
                queue.push(next_state);
                states += 1;
            }
        }
    }

    Ok(Stats {
        states: Some(states),
        transitions: Some(transitions),
    })
}

pub fn explore_parallel<P, SStore>(
    provider: &P,
    store: &mut SStore,
    workers: usize,
) -> std::io::Result<Stats>
where
    P: TransitionProvider + Sync,
    P::State: Clone + Send + Sync,
    P::Transition: Send + Sync,
    SStore: StateStore<P::State>,
{
    explore_parallel_nondeterministic(provider, store, workers.max(1))
}

pub fn explore_parallel_with_options<P, SStore>(
    provider: &P,
    store: &mut SStore,
    options: ParallelExploreOptions,
) -> std::io::Result<Stats>
where
    P: TransitionProvider + Sync,
    P::State: Clone + Send + Sync + Ord,
    P::Transition: Send + Sync,
    SStore: StateStore<P::State>,
{
    if options.deterministic {
        return explore_parallel_deterministic(
            provider,
            store,
            options.workers.max(1),
            options.seed,
        );
    }

    explore_parallel_nondeterministic(provider, store, options.workers.max(1))
}

fn explore_parallel_nondeterministic<P, SStore>(
    provider: &P,
    store: &mut SStore,
    workers: usize,
) -> std::io::Result<Stats>
where
    P: TransitionProvider + Sync,
    P::State: Clone + Send + Sync,
    P::Transition: Send + Sync,
    SStore: StateStore<P::State>,
{
    let mut states: u64 = 0;
    let mut transitions: u64 = 0;

    let initial = provider.initial_state();
    if store.insert(initial.clone())? {
        states += 1;
    }

    let mut frontier = vec![initial];
    let pool = ThreadPoolBuilder::new()
        .num_threads(workers.max(1))
        .build()
        .map_err(|err| io::Error::other(err.to_string()))?;

    while !frontier.is_empty() {
        let batch = frontier;
        let batches = pool.install(|| {
            batch
                .par_iter()
                .map(|state| provider.transitions(state))
                .collect::<Vec<_>>()
        });

        let mut next_frontier = Vec::new();
        for transitions_vec in batches {
            transitions += transitions_vec.len() as u64;
            for (_label, next_state) in transitions_vec {
                if store.insert(next_state.clone())? {
                    next_frontier.push(next_state);
                    states += 1;
                }
            }
        }
        frontier = next_frontier;
    }

    Ok(Stats {
        states: Some(states),
        transitions: Some(transitions),
    })
}

fn explore_parallel_deterministic<P, SStore>(
    provider: &P,
    store: &mut SStore,
    workers: usize,
    _seed: u64,
) -> std::io::Result<Stats>
where
    P: TransitionProvider + Sync,
    P::State: Clone + Send + Sync + Ord,
    P::Transition: Send + Sync,
    SStore: StateStore<P::State>,
{
    let mut states: u64 = 0;
    let mut transitions: u64 = 0;

    let initial = provider.initial_state();
    if store.insert(initial.clone())? {
        states += 1;
    }

    let mut frontier = vec![initial];
    let pool = ThreadPoolBuilder::new()
        .num_threads(workers.max(1))
        .build()
        .map_err(|err| io::Error::other(err.to_string()))?;

    while !frontier.is_empty() {
        frontier.sort();

        let batch = frontier;
        let chunk_size = batch.len().div_ceil(workers).max(1);
        let chunks = pool.install(|| {
            batch
                .par_chunks(chunk_size)
                .map(|chunk| {
                    chunk
                        .iter()
                        .map(|state| provider.transitions(state))
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>()
        });

        let mut candidates = Vec::new();
        for chunk in chunks {
            for transitions_vec in chunk {
                transitions += transitions_vec.len() as u64;
                for (_label, next_state) in transitions_vec {
                    candidates.push(next_state);
                }
            }
        }
        candidates.sort();
        candidates.dedup();

        let mut next_frontier = Vec::new();
        for next_state in candidates {
            if store.insert(next_state.clone())? {
                states += 1;
                next_frontier.push(next_state);
            }
        }
        frontier = next_frontier;
    }

    Ok(Stats {
        states: Some(states),
        transitions: Some(transitions),
    })
}
