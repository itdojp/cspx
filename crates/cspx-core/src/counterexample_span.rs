use crate::ir::Module;
use crate::types::SourceSpan;

pub(crate) fn module_counterexample_spans(module: &Module) -> Vec<SourceSpan> {
    if let Some(entry) = &module.entry {
        return vec![entry.span.clone()];
    }
    if let Some(decl) = module.declarations.first() {
        return vec![decl.expr.span.clone()];
    }
    Vec::new()
}

pub(crate) fn refinement_counterexample_spans(spec: &Module, impl_: &Module) -> Vec<SourceSpan> {
    let mut spans = module_counterexample_spans(spec);
    spans.extend(module_counterexample_spans(impl_));
    spans.sort_by(|left, right| {
        (
            left.path.as_str(),
            left.start_line,
            left.start_col,
            left.end_line,
            left.end_col,
        )
            .cmp(&(
                right.path.as_str(),
                right.start_line,
                right.start_col,
                right.end_line,
                right.end_col,
            ))
    });
    spans.dedup();
    spans
}
