use crate::explain::Explainer;
use crate::types::Counterexample;

#[derive(Debug, Default)]
pub struct BasicExplainer;

impl Explainer for BasicExplainer {
    fn explain(&self, mut counterexample: Counterexample) -> Counterexample {
        if !counterexample.tags.iter().any(|t| t == "explained") {
            counterexample.tags.push("explained".to_string());
        }
        counterexample
    }
}
