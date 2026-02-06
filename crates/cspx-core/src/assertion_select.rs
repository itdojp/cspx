use crate::ir::{AssertionDecl, Module, ProcessExpr, PropertyKind, PropertyModel, Spanned};
use std::collections::HashMap;

pub(crate) fn module_for_property_check(input: &Module, kind: PropertyKind) -> Module {
    if input.entry.is_some() || input.declarations.len() == 1 {
        return input.clone();
    }

    let mut module = input.clone();
    if let Some(entry) = select_last_property_assert_target_expr(input, kind) {
        module.entry = Some(entry);
    }
    module
}

fn select_last_property_assert_target_expr(
    module: &Module,
    kind: PropertyKind,
) -> Option<Spanned<ProcessExpr>> {
    let mut decls = HashMap::<&str, &Spanned<ProcessExpr>>::new();
    for decl in &module.declarations {
        decls.insert(decl.name.value.as_str(), &decl.expr);
    }

    for assertion in module.assertions.iter().rev() {
        let AssertionDecl::Property {
            target,
            kind: assert_kind,
            ..
        } = assertion
        else {
            continue;
        };
        if *assert_kind != kind {
            continue;
        }
        if let Some(expr) = decls.get(target.value.as_str()) {
            return Some((*expr).clone());
        }
    }
    None
}

pub(crate) fn list_property_assertion_candidates(module: &Module) -> Vec<String> {
    module
        .assertions
        .iter()
        .filter_map(|assertion| match assertion {
            AssertionDecl::Property {
                target,
                kind,
                model,
            } => Some(format!(
                "{} :[{} [{}]]",
                target.value,
                property_kind_str(*kind),
                property_model_str(*model)
            )),
            AssertionDecl::Refinement { .. } => None,
        })
        .collect()
}

pub(crate) fn property_kind_str(kind: PropertyKind) -> &'static str {
    match kind {
        PropertyKind::DeadlockFree => "deadlock free",
        PropertyKind::DivergenceFree => "divergence free",
        PropertyKind::Deterministic => "deterministic",
    }
}

fn property_model_str(model: PropertyModel) -> &'static str {
    match model {
        PropertyModel::F => "F",
        PropertyModel::FD => "FD",
    }
}
