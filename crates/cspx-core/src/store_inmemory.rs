use crate::store::StateStore;
use std::collections::HashSet;
use std::hash::Hash;

#[derive(Debug, Default)]
pub struct InMemoryStateStore<S>
where
    S: Eq + Hash,
{
    states: HashSet<S>,
}

impl<S> InMemoryStateStore<S>
where
    S: Eq + Hash,
{
    pub fn new() -> Self {
        Self {
            states: HashSet::new(),
        }
    }
}

impl<S> StateStore<S> for InMemoryStateStore<S>
where
    S: Eq + Hash,
{
    fn insert(&mut self, state: S) -> bool {
        self.states.insert(state)
    }

    fn len(&self) -> usize {
        self.states.len()
    }
}
