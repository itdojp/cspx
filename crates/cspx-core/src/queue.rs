pub trait WorkQueue<S> {
    fn push(&mut self, state: S);
    fn pop(&mut self) -> Option<S>;
    fn is_empty(&self) -> bool;
}
