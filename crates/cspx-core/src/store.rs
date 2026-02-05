pub trait StateStore<S> {
    fn insert(&mut self, state: S) -> std::io::Result<bool>;
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
