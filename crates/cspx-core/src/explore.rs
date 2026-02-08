use crate::lts::TransitionProvider;
use crate::queue::WorkQueue;
use crate::store::StateStore;
use crate::types::Stats;
use rayon::prelude::*;
use rayon::ThreadPoolBuilder;
use std::io;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExploreProfileMode {
    Serial,
    Parallel,
    ParallelDeterministic,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExploreHotspotProfile {
    pub mode: ExploreProfileMode,
    pub workers: usize,
    pub levels: u64,
    pub expanded_states: u64,
    pub discovered_states: u64,
    pub generated_transitions: u64,
    pub state_generation_ns: u64,
    pub state_generation_wall_ns: u64,
    pub visited_insert_ns: u64,
    pub frontier_maintenance_ns: u64,
    pub estimated_wait_ns: u64,
}

#[derive(Debug)]
struct TransitionBatch<S> {
    generated_transitions: u64,
    states: Vec<S>,
}

impl ExploreHotspotProfile {
    fn new(mode: ExploreProfileMode, workers: usize) -> Self {
        Self {
            mode,
            workers,
            levels: 0,
            expanded_states: 0,
            discovered_states: 0,
            generated_transitions: 0,
            state_generation_ns: 0,
            state_generation_wall_ns: 0,
            visited_insert_ns: 0,
            frontier_maintenance_ns: 0,
            estimated_wait_ns: 0,
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
    explore_serial_internal(provider, store, queue, None)
}

pub fn explore_profiled<P, SStore, Q>(
    provider: &P,
    store: &mut SStore,
    queue: &mut Q,
) -> std::io::Result<(Stats, ExploreHotspotProfile)>
where
    P: TransitionProvider,
    P::State: Clone,
    SStore: StateStore<P::State>,
    Q: WorkQueue<P::State>,
{
    let mut profile = ExploreHotspotProfile::new(ExploreProfileMode::Serial, 1);
    let stats = explore_serial_internal(provider, store, queue, Some(&mut profile))?;
    Ok((stats, profile))
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
    explore_parallel_nondeterministic_internal(provider, store, workers.max(1), None)
}

pub fn explore_parallel_profiled<P, SStore>(
    provider: &P,
    store: &mut SStore,
    workers: usize,
) -> std::io::Result<(Stats, ExploreHotspotProfile)>
where
    P: TransitionProvider + Sync,
    P::State: Clone + Send + Sync,
    P::Transition: Send + Sync,
    SStore: StateStore<P::State>,
{
    let worker_count = workers.max(1);
    let mut profile = ExploreHotspotProfile::new(ExploreProfileMode::Parallel, worker_count);
    let stats = explore_parallel_nondeterministic_internal(
        provider,
        store,
        worker_count,
        Some(&mut profile),
    )?;
    Ok((stats, profile))
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
        return explore_parallel_deterministic_internal(
            provider,
            store,
            options.workers.max(1),
            options.seed,
            None,
        );
    }

    explore_parallel_nondeterministic_internal(provider, store, options.workers.max(1), None)
}

pub fn explore_parallel_profiled_with_options<P, SStore>(
    provider: &P,
    store: &mut SStore,
    options: ParallelExploreOptions,
) -> std::io::Result<(Stats, ExploreHotspotProfile)>
where
    P: TransitionProvider + Sync,
    P::State: Clone + Send + Sync + Ord,
    P::Transition: Send + Sync,
    SStore: StateStore<P::State>,
{
    let worker_count = options.workers.max(1);
    let mode = if options.deterministic {
        ExploreProfileMode::ParallelDeterministic
    } else {
        ExploreProfileMode::Parallel
    };
    let mut profile = ExploreHotspotProfile::new(mode, worker_count);
    let stats = if options.deterministic {
        explore_parallel_deterministic_internal(
            provider,
            store,
            worker_count,
            options.seed,
            Some(&mut profile),
        )?
    } else {
        explore_parallel_nondeterministic_internal(
            provider,
            store,
            worker_count,
            Some(&mut profile),
        )?
    };
    Ok((stats, profile))
}

fn explore_serial_internal<P, SStore, Q>(
    provider: &P,
    store: &mut SStore,
    queue: &mut Q,
    mut profile: Option<&mut ExploreHotspotProfile>,
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
    let inserted = if let Some(p) = profile.as_deref_mut() {
        let insert_start = Instant::now();
        let inserted = store.insert(initial.clone())?;
        add_ns(
            &mut p.visited_insert_ns,
            duration_ns(insert_start.elapsed()),
        );
        inserted
    } else {
        store.insert(initial.clone())?
    };
    if inserted {
        queue.push(initial);
        states += 1;
        if let Some(p) = profile.as_deref_mut() {
            p.discovered_states = p.discovered_states.saturating_add(1);
        }
    }

    while let Some(state) = queue.pop() {
        if let Some(p) = profile.as_deref_mut() {
            p.expanded_states = p.expanded_states.saturating_add(1);
        }

        let next = if let Some(p) = profile.as_deref_mut() {
            let generation_start = Instant::now();
            let generated = provider.transitions(&state);
            let generation_ns = duration_ns(generation_start.elapsed());
            add_ns(&mut p.state_generation_ns, generation_ns);
            add_ns(&mut p.state_generation_wall_ns, generation_ns);
            generated
        } else {
            provider.transitions(&state)
        };
        let generated = next.len() as u64;
        transitions = transitions.saturating_add(generated);
        if let Some(p) = profile.as_deref_mut() {
            p.generated_transitions = p.generated_transitions.saturating_add(generated);
        }

        for (_label, next_state) in next {
            let inserted = if let Some(p) = profile.as_deref_mut() {
                let insert_start = Instant::now();
                let inserted = store.insert(next_state.clone())?;
                add_ns(
                    &mut p.visited_insert_ns,
                    duration_ns(insert_start.elapsed()),
                );
                inserted
            } else {
                store.insert(next_state.clone())?
            };
            if inserted {
                queue.push(next_state);
                states += 1;
                if let Some(p) = profile.as_deref_mut() {
                    p.discovered_states = p.discovered_states.saturating_add(1);
                }
            }
        }
    }

    Ok(Stats {
        states: Some(states),
        transitions: Some(transitions),
    })
}

fn explore_parallel_nondeterministic_internal<P, SStore>(
    provider: &P,
    store: &mut SStore,
    workers: usize,
    mut profile: Option<&mut ExploreHotspotProfile>,
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
    let inserted = if let Some(p) = profile.as_deref_mut() {
        let insert_start = Instant::now();
        let inserted = store.insert(initial.clone())?;
        add_ns(
            &mut p.visited_insert_ns,
            duration_ns(insert_start.elapsed()),
        );
        inserted
    } else {
        store.insert(initial.clone())?
    };
    if inserted {
        states += 1;
        if let Some(p) = profile.as_deref_mut() {
            p.discovered_states = p.discovered_states.saturating_add(1);
        }
    }

    let mut frontier = if inserted { vec![initial] } else { Vec::new() };
    let pool = ThreadPoolBuilder::new()
        .num_threads(workers.max(1))
        .build()
        .map_err(|err| io::Error::other(err.to_string()))?;

    while !frontier.is_empty() {
        if let Some(p) = profile.as_deref_mut() {
            p.levels = p.levels.saturating_add(1);
            p.expanded_states = p.expanded_states.saturating_add(frontier.len() as u64);
        }

        let batch = frontier;
        let batches = if profile.is_some() {
            let generation_worker_ns = AtomicU64::new(0);
            let generation_wall_start = Instant::now();
            let batches = pool.install(|| {
                batch
                    .par_iter()
                    .map(|state| {
                        let generation_start = Instant::now();
                        let generated = provider.transitions(state);
                        generation_worker_ns
                            .fetch_add(duration_ns(generation_start.elapsed()), Ordering::Relaxed);
                        let generated_transitions = generated.len() as u64;
                        let mut states = Vec::with_capacity(generated.len());
                        for (_label, next_state) in generated {
                            states.push(next_state);
                        }
                        TransitionBatch {
                            generated_transitions,
                            states,
                        }
                    })
                    .collect::<Vec<_>>()
            });
            let generation_wall_ns = duration_ns(generation_wall_start.elapsed());
            let generation_ns = generation_worker_ns.load(Ordering::Relaxed);
            if let Some(p) = profile.as_deref_mut() {
                add_ns(&mut p.state_generation_ns, generation_ns);
                add_ns(&mut p.state_generation_wall_ns, generation_wall_ns);
                add_ns(
                    &mut p.estimated_wait_ns,
                    estimate_wait_ns(generation_wall_ns, workers.max(1), generation_ns),
                );
            }
            batches
        } else {
            pool.install(|| {
                batch
                    .par_iter()
                    .map(|state| {
                        let generated = provider.transitions(state);
                        let generated_transitions = generated.len() as u64;
                        let mut states = Vec::with_capacity(generated.len());
                        for (_label, next_state) in generated {
                            states.push(next_state);
                        }
                        TransitionBatch {
                            generated_transitions,
                            states,
                        }
                    })
                    .collect::<Vec<_>>()
            })
        };

        let mut next_frontier =
            Vec::with_capacity(batches.iter().map(|batch| batch.states.len()).sum());
        for batch in batches {
            transitions = transitions.saturating_add(batch.generated_transitions);
            if let Some(p) = profile.as_deref_mut() {
                p.generated_transitions = p
                    .generated_transitions
                    .saturating_add(batch.generated_transitions);
            }
            for next_state in batch.states {
                let inserted = if let Some(p) = profile.as_deref_mut() {
                    let insert_start = Instant::now();
                    let inserted = store.insert(next_state.clone())?;
                    add_ns(
                        &mut p.visited_insert_ns,
                        duration_ns(insert_start.elapsed()),
                    );
                    inserted
                } else {
                    store.insert(next_state.clone())?
                };
                if inserted {
                    next_frontier.push(next_state);
                    states += 1;
                    if let Some(p) = profile.as_deref_mut() {
                        p.discovered_states = p.discovered_states.saturating_add(1);
                    }
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

fn explore_parallel_deterministic_internal<P, SStore>(
    provider: &P,
    store: &mut SStore,
    workers: usize,
    _seed: u64,
    mut profile: Option<&mut ExploreHotspotProfile>,
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
    let inserted = if let Some(p) = profile.as_deref_mut() {
        let insert_start = Instant::now();
        let inserted = store.insert(initial.clone())?;
        add_ns(
            &mut p.visited_insert_ns,
            duration_ns(insert_start.elapsed()),
        );
        inserted
    } else {
        store.insert(initial.clone())?
    };
    if inserted {
        states += 1;
        if let Some(p) = profile.as_deref_mut() {
            p.discovered_states = p.discovered_states.saturating_add(1);
        }
    }

    let mut frontier = if inserted { vec![initial] } else { Vec::new() };
    let pool = ThreadPoolBuilder::new()
        .num_threads(workers.max(1))
        .build()
        .map_err(|err| io::Error::other(err.to_string()))?;

    while !frontier.is_empty() {
        if let Some(p) = profile.as_deref_mut() {
            p.levels = p.levels.saturating_add(1);
            p.expanded_states = p.expanded_states.saturating_add(frontier.len() as u64);
        }

        debug_assert!(frontier.windows(2).all(|window| window[0] <= window[1]));

        let batch = frontier;
        let chunk_size = batch.len().div_ceil(workers).max(1);
        let chunks = if profile.is_some() {
            let generation_worker_ns = AtomicU64::new(0);
            let frontier_worker_ns = AtomicU64::new(0);
            let generation_wall_start = Instant::now();
            let chunks = pool.install(|| {
                batch
                    .par_chunks(chunk_size)
                    .map(|chunk| {
                        let mut states = Vec::new();
                        let mut generated_transitions = 0u64;
                        for state in chunk {
                            let generation_start = Instant::now();
                            let generated = provider.transitions(state);
                            generation_worker_ns.fetch_add(
                                duration_ns(generation_start.elapsed()),
                                Ordering::Relaxed,
                            );
                            generated_transitions =
                                generated_transitions.saturating_add(generated.len() as u64);
                            states.reserve(generated.len());
                            for (_label, next_state) in generated {
                                states.push(next_state);
                            }
                        }
                        let local_frontier_start = Instant::now();
                        states.sort();
                        states.dedup();
                        frontier_worker_ns.fetch_add(
                            duration_ns(local_frontier_start.elapsed()),
                            Ordering::Relaxed,
                        );
                        TransitionBatch {
                            generated_transitions,
                            states,
                        }
                    })
                    .collect::<Vec<_>>()
            });
            let generation_wall_ns = duration_ns(generation_wall_start.elapsed());
            let generation_ns = generation_worker_ns.load(Ordering::Relaxed);
            let frontier_ns = frontier_worker_ns.load(Ordering::Relaxed);
            let worker_busy_ns = generation_ns.saturating_add(frontier_ns);
            if let Some(p) = profile.as_deref_mut() {
                add_ns(&mut p.state_generation_ns, generation_ns);
                add_ns(&mut p.state_generation_wall_ns, generation_wall_ns);
                add_ns(&mut p.frontier_maintenance_ns, frontier_ns);
                add_ns(
                    &mut p.estimated_wait_ns,
                    estimate_wait_ns(generation_wall_ns, workers.max(1), worker_busy_ns),
                );
            }
            chunks
        } else {
            pool.install(|| {
                batch
                    .par_chunks(chunk_size)
                    .map(|chunk| {
                        let mut states = Vec::new();
                        let mut generated_transitions = 0u64;
                        for state in chunk {
                            let generated = provider.transitions(state);
                            generated_transitions =
                                generated_transitions.saturating_add(generated.len() as u64);
                            states.reserve(generated.len());
                            for (_label, next_state) in generated {
                                states.push(next_state);
                            }
                        }
                        states.sort();
                        states.dedup();
                        TransitionBatch {
                            generated_transitions,
                            states,
                        }
                    })
                    .collect::<Vec<_>>()
            })
        };

        let mut candidates =
            Vec::with_capacity(chunks.iter().map(|batch| batch.states.len()).sum());
        for mut chunk in chunks {
            transitions = transitions.saturating_add(chunk.generated_transitions);
            if let Some(p) = profile.as_deref_mut() {
                p.generated_transitions = p
                    .generated_transitions
                    .saturating_add(chunk.generated_transitions);
            }
            candidates.append(&mut chunk.states);
        }
        if let Some(p) = profile.as_deref_mut() {
            let dedup_start = Instant::now();
            candidates.sort();
            candidates.dedup();
            add_ns(
                &mut p.frontier_maintenance_ns,
                duration_ns(dedup_start.elapsed()),
            );
        } else {
            candidates.sort();
            candidates.dedup();
        }

        let mut next_frontier = Vec::new();
        for next_state in candidates {
            let inserted = if let Some(p) = profile.as_deref_mut() {
                let insert_start = Instant::now();
                let inserted = store.insert(next_state.clone())?;
                add_ns(
                    &mut p.visited_insert_ns,
                    duration_ns(insert_start.elapsed()),
                );
                inserted
            } else {
                store.insert(next_state.clone())?
            };
            if inserted {
                states += 1;
                next_frontier.push(next_state);
                if let Some(p) = profile.as_deref_mut() {
                    p.discovered_states = p.discovered_states.saturating_add(1);
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

fn duration_ns(duration: Duration) -> u64 {
    duration
        .as_nanos()
        .min(u128::from(u64::MAX))
        .try_into()
        .unwrap_or(u64::MAX)
}

fn add_ns(target: &mut u64, delta: u64) {
    *target = target.saturating_add(delta);
}

fn estimate_wait_ns(wall_ns: u64, workers: usize, busy_ns: u64) -> u64 {
    let capacity = wall_ns.saturating_mul(workers as u64);
    capacity.saturating_sub(busy_ns)
}
