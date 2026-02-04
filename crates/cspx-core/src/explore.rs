use crate::lts::TransitionProvider;
use crate::queue::WorkQueue;
use crate::store::StateStore;
use crate::types::Stats;
use rayon::prelude::*;
use rayon::ThreadPoolBuilder;

pub fn explore<P, SStore, Q>(provider: &P, store: &mut SStore, queue: &mut Q) -> Stats
where
    P: TransitionProvider,
    P::State: Clone,
    SStore: StateStore<P::State>,
    Q: WorkQueue<P::State>,
{
    let mut states: u64 = 0;
    let mut transitions: u64 = 0;

    let initial = provider.initial_state();
    if store.insert(initial.clone()) {
        queue.push(initial);
        states += 1;
    }

    while let Some(state) = queue.pop() {
        let next = provider.transitions(&state);
        transitions += next.len() as u64;
        for (_label, next_state) in next {
            if store.insert(next_state.clone()) {
                queue.push(next_state);
                states += 1;
            }
        }
    }

    Stats {
        states: Some(states),
        transitions: Some(transitions),
    }
}

pub fn explore_parallel<P, SStore>(provider: &P, store: &mut SStore, workers: usize) -> Stats
where
    P: TransitionProvider + Sync,
    P::State: Clone + Send + Sync,
    P::Transition: Send + Sync,
    SStore: StateStore<P::State>,
{
    let mut states: u64 = 0;
    let mut transitions: u64 = 0;

    let initial = provider.initial_state();
    if store.insert(initial.clone()) {
        states += 1;
    }

    let mut frontier = vec![initial];
    let pool = ThreadPoolBuilder::new()
        .num_threads(workers.max(1))
        .build()
        .expect("rayon pool");

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
                if store.insert(next_state.clone()) {
                    next_frontier.push(next_state);
                    states += 1;
                }
            }
        }
        frontier = next_frontier;
    }

    Stats {
        states: Some(states),
        transitions: Some(transitions),
    }
}
