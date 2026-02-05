pub trait StateStore<S> {
    fn insert(&mut self, state: S) -> std::io::Result<bool>;
    fn len(&self) -> usize;
}
