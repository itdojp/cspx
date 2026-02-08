use cspx_core::{
    explore_parallel_profiled_with_options, InMemoryStateStore, ParallelExploreOptions, Transition,
    TransitionProvider,
};
use std::error::Error;

#[derive(Clone, Copy)]
struct DenseDuplicateProvider {
    depth: u32,
    fanout: u32,
    duplicate_factor: u32,
}

impl DenseDuplicateProvider {
    fn transition(label: String) -> Transition {
        Transition { label }
    }
}

impl TransitionProvider for DenseDuplicateProvider {
    type State = (u32, u32);
    type Transition = Transition;

    fn initial_state(&self) -> Self::State {
        (0, 0)
    }

    fn transitions(&self, state: &Self::State) -> Vec<(Self::Transition, Self::State)> {
        let (layer, _node) = *state;
        if layer >= self.depth {
            return Vec::new();
        }

        let capacity = (self.fanout as usize).saturating_mul(self.duplicate_factor as usize);
        let mut out = Vec::with_capacity(capacity);
        for branch in 0..self.fanout {
            let next_state = (layer + 1, branch);
            let transition = Self::transition("edge".to_string());
            for _dup in 0..self.duplicate_factor {
                out.push((transition.clone(), next_state));
            }
        }
        out
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let provider = DenseDuplicateProvider {
        depth: 7,
        fanout: 64,
        duplicate_factor: 8,
    };
    let mut store = InMemoryStateStore::new();
    let options = ParallelExploreOptions {
        workers: 4,
        deterministic: true,
        seed: 1,
    };

    let (stats, profile) = explore_parallel_profiled_with_options(&provider, &mut store, options)?;

    println!("states={}", stats.states.unwrap_or_default());
    println!("transitions={}", stats.transitions.unwrap_or_default());
    println!("levels={}", profile.levels);
    println!("expanded_states={}", profile.expanded_states);
    println!("generated_transitions={}", profile.generated_transitions);
    println!("state_generation_ns={}", profile.state_generation_ns);
    println!(
        "frontier_maintenance_ns={}",
        profile.frontier_maintenance_ns
    );
    println!("visited_insert_ns={}", profile.visited_insert_ns);
    println!("estimated_wait_ns={}", profile.estimated_wait_ns);

    Ok(())
}
