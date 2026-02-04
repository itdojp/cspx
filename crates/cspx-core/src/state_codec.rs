pub trait StateCodec<S> {
    fn encode(&self, state: &S) -> Vec<u8>;
}
