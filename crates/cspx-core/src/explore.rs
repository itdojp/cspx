use crate::lts::TransitionProvider;
use crate::queue::WorkQueue;
use crate::store::StateStore;
use crate::types::Stats;

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
