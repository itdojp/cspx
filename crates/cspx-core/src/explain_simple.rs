use crate::explain::Explainer;
use crate::types::Counterexample;
use std::collections::BTreeSet;

#[derive(Debug, Default)]
pub struct BasicExplainer;

impl Explainer for BasicExplainer {
    fn explain(&self, mut counterexample: Counterexample) -> Counterexample {
        let mut tags = counterexample.tags.clone();
        for kind in ["deadlock", "divergence", "nondeterminism", "refinement"] {
            if tags.iter().any(|tag| tag == kind) {
                tags.push(format!("kind:{kind}"));
            }
        }
        tags.push("explained".to_string());
        let mut dedup = BTreeSet::<String>::new();
        tags.retain(|tag| dedup.insert(tag.clone()));
        counterexample.tags = tags;
        counterexample
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{CounterexampleEvent, CounterexampleType, SourceSpan};

    fn sample_counterexample(tags: Vec<String>) -> Counterexample {
        Counterexample {
            kind: CounterexampleType::Trace,
            events: vec![CounterexampleEvent {
                label: "a".to_string(),
            }],
            is_minimized: false,
            tags,
            source_spans: vec![SourceSpan {
                path: "model.cspm".to_string(),
                start_line: 1,
                start_col: 1,
                end_line: 1,
                end_col: 1,
            }],
        }
    }

    #[test]
    fn basic_explainer_appends_explained_and_kind_tag() {
        let explainer = BasicExplainer;
        let counterexample = sample_counterexample(vec!["deadlock".to_string()]);
        let output = explainer.explain(counterexample);

        assert!(output.tags.iter().any(|tag| tag == "deadlock"));
        assert!(output.tags.iter().any(|tag| tag == "kind:deadlock"));
        assert!(output.tags.iter().any(|tag| tag == "explained"));
    }

    #[test]
    fn basic_explainer_deduplicates_tags() {
        let explainer = BasicExplainer;
        let counterexample = sample_counterexample(vec![
            "refinement".to_string(),
            "refinement".to_string(),
            "kind:refinement".to_string(),
        ]);
        let output = explainer.explain(counterexample);

        assert_eq!(
            output.tags.iter().filter(|tag| tag.as_str() == "refinement").count(),
            1
        );
        assert_eq!(
            output
                .tags
                .iter()
                .filter(|tag| tag.as_str() == "kind:refinement")
                .count(),
            1
        );
    }
}
