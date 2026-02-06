use crate::minimize::Minimizer;
use crate::types::{Counterexample, CounterexampleType};

#[derive(Debug, Default)]
pub struct TraceHeuristicMinimizer;

impl Minimizer for TraceHeuristicMinimizer {
    fn minimize(&self, mut counterexample: Counterexample) -> Counterexample {
        counterexample.is_minimized = false;
        counterexample
    }

    fn minimize_with_oracle<F>(
        &self,
        mut counterexample: Counterexample,
        preserves_failure: F,
    ) -> Counterexample
    where
        F: Fn(&Counterexample) -> bool,
    {
        if counterexample.kind != CounterexampleType::Trace {
            counterexample.is_minimized = false;
            return counterexample;
        }
        if !preserves_failure(&counterexample) {
            counterexample.is_minimized = false;
            return counterexample;
        }

        let mut best = counterexample;
        loop {
            let mut reduced = false;
            for idx in 0..best.events.len() {
                let mut candidate = best.clone();
                candidate.events.remove(idx);
                if !preserves_failure(&candidate) {
                    continue;
                }
                best = candidate;
                reduced = true;
                break;
            }
            if !reduced {
                break;
            }
        }

        best.is_minimized = true;
        best
    }
}

#[deprecated(
    since = "0.1.0",
    note = "IdentityMinimizer is no longer an identity/no-op minimizer. Use TraceHeuristicMinimizer directly instead."
)]
pub type IdentityMinimizer = TraceHeuristicMinimizer;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{CounterexampleEvent, SourceSpan};

    fn trace(labels: &[&str]) -> Counterexample {
        Counterexample {
            kind: CounterexampleType::Trace,
            events: labels
                .iter()
                .map(|label| CounterexampleEvent {
                    label: (*label).to_string(),
                })
                .collect(),
            is_minimized: false,
            tags: vec!["refinement".to_string()],
            source_spans: vec![SourceSpan {
                path: "spec.cspm".to_string(),
                start_line: 1,
                start_col: 1,
                end_line: 1,
                end_col: 1,
            }],
        }
    }

    fn labels(counterexample: &Counterexample) -> Vec<String> {
        counterexample
            .events
            .iter()
            .map(|event| event.label.clone())
            .collect()
    }

    #[test]
    fn minimize_with_oracle_drops_redundant_events() {
        let minimizer = TraceHeuristicMinimizer;
        let output = minimizer.minimize_with_oracle(trace(&["x", "a", "y", "z"]), |candidate| {
            candidate.events.iter().any(|event| event.label == "a")
        });

        assert_eq!(labels(&output), vec!["a".to_string()]);
        assert!(output.is_minimized);
    }

    #[test]
    fn minimize_without_oracle_keeps_unverified_flag() {
        let minimizer = TraceHeuristicMinimizer;
        let output = minimizer.minimize(trace(&["a", "b"]));

        assert_eq!(labels(&output), vec!["a".to_string(), "b".to_string()]);
        assert!(!output.is_minimized);
    }

    #[test]
    fn minimize_with_oracle_marks_not_minimized_when_failure_not_preserved() {
        let minimizer = TraceHeuristicMinimizer;
        let output = minimizer.minimize_with_oracle(trace(&["a"]), |_candidate| false);

        assert_eq!(labels(&output), vec!["a".to_string()]);
        assert!(!output.is_minimized);
    }

    #[test]
    fn minimize_with_oracle_preserves_tau_when_required_for_failure() {
        let minimizer = TraceHeuristicMinimizer;
        let output = minimizer.minimize_with_oracle(trace(&["x", "tau", "y"]), |candidate| {
            candidate.events.iter().any(|event| event.label == "tau")
        });

        assert_eq!(labels(&output), vec!["tau".to_string()]);
        assert!(output.is_minimized);
    }
}
