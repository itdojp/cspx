use crate::types::Counterexample;

pub trait Minimizer {
    fn minimize(&self, counterexample: Counterexample) -> Counterexample;
}
