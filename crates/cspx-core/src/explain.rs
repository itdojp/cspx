use crate::types::Counterexample;

pub trait Explainer {
    fn explain(&self, counterexample: Counterexample) -> Counterexample;
}
