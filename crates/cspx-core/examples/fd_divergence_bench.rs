use cspx_core::check::{CheckCommand, RefinementModel};
use cspx_core::ir::{ChannelDecl, Event, EventSet, ProcessDecl, ProcessExpr, Spanned};
use cspx_core::types::{SourceSpan, Status};
use cspx_core::{CheckRequest, Checker, RefinementChecker, RefinementInput};
use std::error::Error;
use std::time::Instant;

fn spanned<T>(value: T) -> Spanned<T> {
    Spanned {
        value,
        span: SourceSpan {
            path: "fd_divergence_bench.cspm".to_string(),
            start_line: 1,
            start_col: 1,
            end_line: 1,
            end_col: 1,
        },
    }
}

fn unit_channel(name: &str) -> ChannelDecl {
    ChannelDecl {
        names: vec![spanned(name.to_string())],
        domain: None,
    }
}

fn ref_proc(name: &str) -> Spanned<ProcessExpr> {
    spanned(ProcessExpr::Ref(spanned(name.to_string())))
}

fn prefix(channel: &str, next: Spanned<ProcessExpr>) -> Spanned<ProcessExpr> {
    spanned(ProcessExpr::Prefix {
        event: spanned(Event {
            channel: spanned(channel.to_string()),
            seg: None,
        }),
        next: Box::new(next),
    })
}

fn hide(inner: Spanned<ProcessExpr>, channels: &[&str]) -> Spanned<ProcessExpr> {
    spanned(ProcessExpr::Hide {
        inner: Box::new(inner),
        hide: EventSet {
            channels: channels
                .iter()
                .map(|name| spanned((*name).to_string()))
                .collect(),
        },
    })
}

fn parse_fd_metric(tags: &[String], key: &str) -> Option<u64> {
    let prefix = format!("{key}:");
    tags.iter()
        .find_map(|tag| tag.strip_prefix(&prefix))
        .and_then(|value| value.parse().ok())
}

fn build_modules(ring_size: usize) -> (cspx_core::ir::Module, cspx_core::ir::Module) {
    let spec = cspx_core::ir::Module {
        channels: vec![unit_channel("a")],
        declarations: vec![ProcessDecl {
            name: spanned("SPEC".to_string()),
            expr: spanned(ProcessExpr::Stop),
        }],
        assertions: Vec::new(),
        entry: Some(ref_proc("SPEC")),
    };

    let mut impl_decls = Vec::new();
    for idx in 0..ring_size {
        let next = (idx + 1) % ring_size;
        impl_decls.push(ProcessDecl {
            name: spanned(format!("P{idx}")),
            expr: prefix("a", ref_proc(&format!("P{next}"))),
        });
    }
    impl_decls.push(ProcessDecl {
        name: spanned("IMPL".to_string()),
        expr: hide(ref_proc("P0"), &["a"]),
    });
    let impl_ = cspx_core::ir::Module {
        channels: vec![unit_channel("a")],
        declarations: impl_decls,
        assertions: Vec::new(),
        entry: Some(ref_proc("IMPL")),
    };

    (spec, impl_)
}

fn main() -> Result<(), Box<dyn Error>> {
    let ring_size = 512usize;
    let (spec, impl_) = build_modules(ring_size);

    let checker = RefinementChecker;
    let request = CheckRequest {
        command: CheckCommand::Refine,
        model: Some(RefinementModel::FD),
        target: Some("SPEC [FD= IMPL".to_string()),
    };
    let input = RefinementInput { spec, impl_ };

    let start = Instant::now();
    let result = checker.check(&request, &input);
    let elapsed_ns = start.elapsed().as_nanos();

    println!("ring_size={ring_size}");
    println!(
        "status={}",
        match result.status {
            Status::Pass => "pass",
            Status::Fail => "fail",
            Status::Unsupported => "unsupported",
            Status::Timeout => "timeout",
            Status::OutOfMemory => "out_of_memory",
            Status::Error => "error",
        }
    );
    println!("duration_ns={elapsed_ns}");
    println!(
        "states={}",
        result
            .stats
            .as_ref()
            .and_then(|value| value.states)
            .unwrap_or_default()
    );
    println!(
        "transitions={}",
        result
            .stats
            .as_ref()
            .and_then(|value| value.transitions)
            .unwrap_or_default()
    );
    if let Some(reason) = result.reason.as_ref() {
        println!("reason_kind={:?}", reason.kind);
        println!(
            "reason_message={}",
            reason.message.as_deref().unwrap_or_default()
        );
    }

    if let Some(counterexample) = result.counterexample {
        println!("trace_len={}", counterexample.events.len());
        println!(
            "fd_nodes={}",
            parse_fd_metric(&counterexample.tags, "fd_nodes").unwrap_or_default()
        );
        println!(
            "fd_edges={}",
            parse_fd_metric(&counterexample.tags, "fd_edges").unwrap_or_default()
        );
        println!(
            "fd_divergence_checks={}",
            parse_fd_metric(&counterexample.tags, "fd_divergence_checks").unwrap_or_default()
        );
        println!(
            "fd_pruned_nodes={}",
            parse_fd_metric(&counterexample.tags, "fd_pruned_nodes").unwrap_or_default()
        );
        println!(
            "fd_impl_closure_max={}",
            parse_fd_metric(&counterexample.tags, "fd_impl_closure_max").unwrap_or_default()
        );
        println!(
            "fd_spec_closure_max={}",
            parse_fd_metric(&counterexample.tags, "fd_spec_closure_max").unwrap_or_default()
        );
        println!(
            "fd_closure_cache_hits={}",
            parse_fd_metric(&counterexample.tags, "fd_closure_cache_hits").unwrap_or_default()
        );
        println!(
            "fd_closure_cache_misses={}",
            parse_fd_metric(&counterexample.tags, "fd_closure_cache_misses").unwrap_or_default()
        );
        println!(
            "fd_divergence_cache_hits={}",
            parse_fd_metric(&counterexample.tags, "fd_divergence_cache_hits").unwrap_or_default()
        );
        println!(
            "fd_divergence_cache_misses={}",
            parse_fd_metric(&counterexample.tags, "fd_divergence_cache_misses").unwrap_or_default()
        );
    } else {
        println!("trace_len=0");
    }

    Ok(())
}
