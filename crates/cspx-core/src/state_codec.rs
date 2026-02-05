use std::error::Error;
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone)]
pub struct StateCodecError {
    message: String,
}

impl StateCodecError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl Display for StateCodecError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl Error for StateCodecError {}

pub trait StateCodec<S> {
    fn encode(&self, state: &S) -> Vec<u8>;
    fn decode(&self, bytes: &[u8]) -> Result<S, StateCodecError>;
}
