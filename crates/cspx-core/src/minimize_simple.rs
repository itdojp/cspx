use crate::minimize::Minimizer;
use crate::types::Counterexample;

#[derive(Debug, Default)]
pub struct IdentityMinimizer;

impl Minimizer for IdentityMinimizer {
    fn minimize(&self, mut counterexample: Counterexample) -> Counterexample {
        counterexample.is_minimized = true;
        counterexample
    }
}
