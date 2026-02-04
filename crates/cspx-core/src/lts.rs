pub type StateId = u64;

#[derive(Debug, Clone)]
pub struct Transition {
    pub label: String,
}

pub trait TransitionProvider {
    type State: Clone + Send + Sync;
    type Transition: Clone + Send + Sync;

    fn initial_state(&self) -> Self::State;
    fn transitions(&self, state: &Self::State) -> Vec<Self::Transition>;
}
