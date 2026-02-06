use cspx_core::{
    explore, CspmStateCodec, CspmTransitionProvider, Frontend, InMemoryStateStore, SimpleFrontend,
    StateCodec, TransitionProvider, VecWorkQueue,
};

fn transitions_keyed(provider: &CspmTransitionProvider) -> Vec<(String, Vec<u8>)> {
    let state = provider.initial_state();
    let mut next = provider.transitions(&state);
    next.sort_by(|(a_t, a_s), (b_t, b_s)| {
        let label_cmp = a_t.label.cmp(&b_t.label);
        if label_cmp != std::cmp::Ordering::Equal {
            return label_cmp;
        }
        let a_bytes = CspmStateCodec.encode(a_s);
        let b_bytes = CspmStateCodec.encode(b_s);
        a_bytes.cmp(&b_bytes)
    });
    next.into_iter()
        .map(|(t, s)| (t.label, CspmStateCodec.encode(&s)))
        .collect()
}

#[test]
fn explore_external_choice_is_stable() {
    let input = r#"channel a
channel b
P = a -> STOP [] b -> STOP
"#;
    let frontend = SimpleFrontend;
    let module = frontend
        .parse_and_typecheck(input, "model.cspm")
        .expect("parse_and_typecheck")
        .ir;

    let provider = CspmTransitionProvider::from_module(&module).expect("provider");
    let mut store = InMemoryStateStore::new();
    let mut queue = VecWorkQueue::new();
    let stats = explore(&provider, &mut store, &mut queue).expect("explore");

    assert_eq!(stats.states, Some(2));
    assert_eq!(stats.transitions, Some(2));

    let keyed = transitions_keyed(&provider);
    assert_eq!(keyed.len(), 2);
    assert_eq!(keyed[0].0, "a");
    assert_eq!(keyed[1].0, "b");
}

#[test]
fn explore_internal_choice_uses_tau() {
    let input = r#"channel a
channel b
P = (a -> STOP) |~| (b -> STOP)
"#;
    let frontend = SimpleFrontend;
    let module = frontend
        .parse_and_typecheck(input, "model.cspm")
        .expect("parse_and_typecheck")
        .ir;

    let provider = CspmTransitionProvider::from_module(&module).expect("provider");
    let mut store = InMemoryStateStore::new();
    let mut queue = VecWorkQueue::new();
    let stats = explore(&provider, &mut store, &mut queue).expect("explore");

    assert_eq!(stats.states, Some(4));
    assert_eq!(stats.transitions, Some(4));

    let keyed1 = transitions_keyed(&provider);
    let keyed2 = transitions_keyed(&provider);
    assert_eq!(keyed1, keyed2);
    assert_eq!(keyed1.len(), 2);
    assert_eq!(keyed1[0].0, "tau");
    assert_eq!(keyed1[1].0, "tau");
}

#[test]
fn explore_guarded_recursion_is_single_state_loop() {
    let input = r#"channel a
P = a -> P
"#;
    let frontend = SimpleFrontend;
    let module = frontend
        .parse_and_typecheck(input, "model.cspm")
        .expect("parse_and_typecheck")
        .ir;

    let provider = CspmTransitionProvider::from_module(&module).expect("provider");
    let mut store = InMemoryStateStore::new();
    let mut queue = VecWorkQueue::new();
    let stats = explore(&provider, &mut store, &mut queue).expect("explore");

    assert_eq!(stats.states, Some(1));
    assert_eq!(stats.transitions, Some(1));
}
