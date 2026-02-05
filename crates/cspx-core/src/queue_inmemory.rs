use crate::queue::WorkQueue;
use std::collections::VecDeque;

#[derive(Debug, Default)]
pub struct VecWorkQueue<S> {
    queue: VecDeque<S>,
}

impl<S> VecWorkQueue<S> {
    pub fn new() -> Self {
        Self {
            queue: VecDeque::new(),
        }
    }
}

impl<S> WorkQueue<S> for VecWorkQueue<S> {
    fn push(&mut self, state: S) {
        self.queue.push_back(state);
    }

    fn pop(&mut self) -> Option<S> {
        self.queue.pop_front()
    }

    fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
}
