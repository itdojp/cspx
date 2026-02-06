use crate::types::Counterexample;

pub trait Minimizer {
    fn minimize(&self, counterexample: Counterexample) -> Counterexample;

    fn minimize_with_oracle<F>(
        &self,
        counterexample: Counterexample,
        preserves_failure: F,
    ) -> Counterexample
    where
        F: Fn(&Counterexample) -> bool,
    {
        let _ = preserves_failure;
        self.minimize(counterexample)
    }
}
