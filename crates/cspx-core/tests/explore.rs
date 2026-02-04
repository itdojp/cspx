use cspx_core::{
    explore, explore_parallel, InMemoryStateStore, SimpleTransitionProvider, VecWorkQueue,
};

#[test]
fn explore_stop_yields_single_state() {
    let module = cspx_core::ir::Module {
        declarations: Vec::new(),
        entry: Some(cspx_core::ir::Spanned {
            value: cspx_core::ir::ProcessExpr::Stop,
            span: cspx_core::types::SourceSpan {
                path: "test.cspm".to_string(),
                start_line: 1,
                start_col: 1,
                end_line: 1,
                end_col: 4,
            },
        }),
    };

    let provider = SimpleTransitionProvider::from_module(&module).expect("provider");
    let mut store = InMemoryStateStore::new();
    let mut queue = VecWorkQueue::new();
    let stats = explore(&provider, &mut store, &mut queue);

    assert_eq!(stats.states, Some(1));
    assert_eq!(stats.transitions, Some(0));
}

#[test]
fn explore_parallel_stop_yields_single_state() {
    let module = cspx_core::ir::Module {
        declarations: Vec::new(),
        entry: Some(cspx_core::ir::Spanned {
            value: cspx_core::ir::ProcessExpr::Stop,
            span: cspx_core::types::SourceSpan {
                path: "test.cspm".to_string(),
                start_line: 1,
                start_col: 1,
                end_line: 1,
                end_col: 4,
            },
        }),
    };

    let provider = SimpleTransitionProvider::from_module(&module).expect("provider");
    let mut store = InMemoryStateStore::new();
    let stats = explore_parallel(&provider, &mut store, 2);

    assert_eq!(stats.states, Some(1));
    assert_eq!(stats.transitions, Some(0));
}
