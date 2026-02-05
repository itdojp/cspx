pub trait StateStore<S> {
    fn insert(&mut self, state: S) -> bool;
    fn len(&self) -> usize;
}
