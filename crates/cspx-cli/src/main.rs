use anyhow::{anyhow, Context, Result};
use chrono::{SecondsFormat, Utc};
use clap::{ArgGroup, Args, Parser, Subcommand, ValueEnum};
use cspx_core::{
    explore, explore_parallel, explore_parallel_with_options, CheckRequest, CheckResult, Checker,
    DeadlockChecker, DeterminismChecker, DivergenceChecker, Frontend, FrontendErrorKind,
    InMemoryStateStore, ParallelExploreOptions, Reason, ReasonKind, RefinementChecker,
    RefinementInput, SimpleFrontend, SimpleTransitionProvider, Stats, Status, VecWorkQueue,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Parser)]
#[command(name = "cspx")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Command,

    #[arg(long, value_enum, default_value = "json", global = true)]
    format: OutputFormat,

    #[arg(long, global = true)]
    output: Option<PathBuf>,

    #[arg(long, global = true)]
    summary_json: Option<PathBuf>,

    #[arg(long, global = true)]
    timeout_ms: Option<u64>,

    #[arg(long, global = true)]
    memory_mb: Option<u64>,

    #[arg(
        long,
        default_value_t = 1,
        value_parser = clap::value_parser!(usize),
        global = true
    )]
    parallel: usize,

    #[arg(long, global = true)]
    deterministic: bool,

    #[arg(long, global = true)]
    seed: Option<u64>,
}

#[derive(Subcommand)]
enum Command {
    Typecheck { file: PathBuf },
    Check(CheckArgs),
    Refine(RefineArgs),
}

#[derive(Args)]
#[command(group(
    ArgGroup::new("mode")
        .required(true)
        .args(["assert", "all_assertions"])
))]
struct CheckArgs {
    #[arg(long)]
    assert: Option<String>,

    #[arg(long)]
    all_assertions: bool,

    file: PathBuf,
}

#[derive(Args)]
struct RefineArgs {
    #[arg(long, value_enum)]
    model: RefinementModel,

    spec: PathBuf,

    #[arg(value_name = "impl")]
    impl_: PathBuf,
}

#[derive(Clone, Copy, ValueEnum)]
enum OutputFormat {
    Json,
    Text,
}

#[derive(Clone, Copy, ValueEnum)]
enum RefinementModel {
    #[value(name = "T")]
    T,
    #[value(name = "F")]
    F,
    #[value(name = "FD")]
    FD,
}

impl RefinementModel {
    fn as_str(&self) -> &'static str {
        match self {
            RefinementModel::T => "T",
            RefinementModel::F => "F",
            RefinementModel::FD => "FD",
        }
    }
}

#[derive(Serialize)]
struct ResultJson {
    schema_version: String,
    tool: ToolInfo,
    invocation: Invocation,
    inputs: Vec<InputInfo>,
    status: Status,
    exit_code: i32,
    started_at: String,
    finished_at: String,
    duration_ms: u64,
    checks: Vec<CheckResult>,
    metrics: Option<ResultMetrics>,
}

#[derive(Serialize)]
struct ToolInfo {
    name: String,
    version: String,
    git_sha: String,
}

#[derive(Serialize)]
struct Invocation {
    command: String,
    args: Vec<String>,
    format: String,
    timeout_ms: Option<u64>,
    memory_mb: Option<u64>,
    parallel: usize,
    deterministic: bool,
    seed: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
struct ResultMetrics {
    states: Option<u64>,
    transitions: Option<u64>,
    wall_time_ms: u64,
    cpu_time_ms: Option<u64>,
    peak_rss_bytes: Option<u64>,
    disk_bytes: Option<u64>,
    states_per_sec: Option<f64>,
    transitions_per_sec: Option<f64>,
    parallelism: ParallelismMetrics,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
struct ParallelismMetrics {
    threads: usize,
    deterministic: bool,
    seed: u64,
}

#[derive(Serialize)]
struct InputInfo {
    path: String,
    sha256: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CspSummaryJson {
    tool: String,
    file: String,
    backend: String,
    details_file: Option<String>,
    result_status: Option<String>,
    ran: bool,
    status: String,
    exit_code: i32,
    timestamp: String,
    output: String,
}

type ExecuteOutput = (Status, i32, Vec<CheckResult>, Vec<InputInfo>, Invocation);

fn main() {
    let cli = Cli::parse();
    let exit_code = match run(cli) {
        Ok(code) => code,
        Err(err) => {
            eprintln!("tool error: {err}");
            2
        }
    };
    std::process::exit(exit_code);
}

fn run(cli: Cli) -> Result<i32> {
    let started_at = Utc::now();
    let timer = Instant::now();

    let (status, exit_code, checks, inputs, invocation) = execute(&cli)?;

    let finished_at = Utc::now();
    let duration_ms = timer.elapsed().as_millis() as u64;
    let metrics = build_metrics(&checks, duration_ms, &invocation);

    let result = ResultJson {
        schema_version: "0.1".to_string(),
        tool: ToolInfo {
            name: "cspx".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            git_sha: std::env::var("CSPX_GIT_SHA").unwrap_or_else(|_| "UNKNOWN".to_string()),
        },
        invocation,
        inputs,
        status,
        exit_code,
        started_at: started_at.to_rfc3339_opts(SecondsFormat::Secs, true),
        finished_at: finished_at.to_rfc3339_opts(SecondsFormat::Secs, true),
        duration_ms,
        checks,
        metrics: Some(metrics),
    };

    match cli.format {
        OutputFormat::Json => emit_json(&result, cli.output.as_deref()),
        OutputFormat::Text => emit_text(&result, cli.output.as_deref()),
    }?;

    if let Some(path) = cli.summary_json.as_deref() {
        emit_summary_json(&result, path, cli.output.as_deref(), cli.format)?;
    }

    Ok(exit_code)
}

fn execute(cli: &Cli) -> Result<ExecuteOutput> {
    if cli.parallel == 0 {
        return Err(anyhow!("--parallel must be >= 1"));
    }
    if cli.deterministic && cli.seed.is_none() {
        return Err(anyhow!("--deterministic requires --seed <n>"));
    }
    let seed = cli.seed.unwrap_or(0);

    let (command, args, inputs, checks) = match &cli.command {
        Command::Typecheck { file } => {
            let (inputs, io_error) = build_inputs(std::slice::from_ref(file));
            let checks = vec![run_typecheck(
                file,
                io_error.as_ref(),
                cli.parallel,
                cli.deterministic,
                seed,
            )];
            (
                "typecheck".to_string(),
                vec![file.to_string_lossy().to_string()],
                inputs,
                checks,
            )
        }
        Command::Check(args) => {
            let (inputs, io_error) = build_inputs(std::slice::from_ref(&args.file));
            let checks = if let Some(assertion) = &args.assert {
                vec![run_check_by_assertion(
                    &args.file,
                    io_error.as_ref(),
                    assertion,
                )]
            } else if args.all_assertions {
                run_all_assertions(&args.file, io_error.as_ref())
            } else {
                vec![build_stub_check_result(
                    "check",
                    None,
                    None,
                    io_error.as_ref(),
                    "checker not implemented yet",
                )]
            };
            (
                "check".to_string(),
                vec![args.file.to_string_lossy().to_string()],
                inputs,
                checks,
            )
        }
        Command::Refine(args) => {
            let (inputs, io_error) = build_inputs(&[args.spec.clone(), args.impl_.clone()]);
            let checks = vec![run_refine_check(args, io_error.as_ref())];
            (
                "refine".to_string(),
                vec![
                    args.spec.to_string_lossy().to_string(),
                    args.impl_.to_string_lossy().to_string(),
                ],
                inputs,
                checks,
            )
        }
    };

    let status = aggregate_status(&checks);
    let exit_code = exit_code_for_status(&status);

    let invocation = Invocation {
        command,
        args,
        format: match cli.format {
            OutputFormat::Json => "json".to_string(),
            OutputFormat::Text => "text".to_string(),
        },
        timeout_ms: cli.timeout_ms,
        memory_mb: cli.memory_mb,
        parallel: cli.parallel,
        deterministic: cli.deterministic,
        seed,
    };

    Ok((status, exit_code, checks, inputs, invocation))
}

fn build_metrics(checks: &[CheckResult], duration_ms: u64, invocation: &Invocation) -> ResultMetrics {
    let states = aggregate_stats(checks, |stats| stats.states);
    let transitions = aggregate_stats(checks, |stats| stats.transitions);

    ResultMetrics {
        states,
        transitions,
        wall_time_ms: duration_ms,
        cpu_time_ms: None,
        peak_rss_bytes: None,
        disk_bytes: None,
        states_per_sec: throughput_per_sec(states, duration_ms),
        transitions_per_sec: throughput_per_sec(transitions, duration_ms),
        parallelism: ParallelismMetrics {
            threads: invocation.parallel,
            deterministic: invocation.deterministic,
            seed: invocation.seed,
        },
    }
}

fn aggregate_stats(
    checks: &[CheckResult],
    select: fn(&Stats) -> Option<u64>,
) -> Option<u64> {
    let mut total = 0_u64;
    for check in checks {
        let stats = check.stats.as_ref()?;
        let value = select(stats)?;
        total = total.saturating_add(value);
    }
    Some(total)
}

fn throughput_per_sec(value: Option<u64>, duration_ms: u64) -> Option<f64> {
    if duration_ms == 0 {
        return None;
    }
    value.map(|count| (count as f64 * 1000.0) / duration_ms as f64)
}

fn aggregate_status(checks: &[CheckResult]) -> Status {
    let mut worst = Status::Pass;
    for check in checks {
        if status_rank(&check.status) > status_rank(&worst) {
            worst = check.status.clone();
        }
    }
    worst
}

fn status_rank(status: &Status) -> u8 {
    match status {
        Status::Pass => 0,
        Status::Unsupported => 1,
        Status::Fail => 2,
        Status::Timeout => 3,
        Status::OutOfMemory => 4,
        Status::Error => 5,
    }
}

fn exit_code_for_status(status: &Status) -> i32 {
    match status {
        Status::Pass => 0,
        Status::Fail => 1,
        Status::Error => 2,
        Status::Unsupported => 3,
        Status::Timeout => 4,
        Status::OutOfMemory => 5,
    }
}

fn build_inputs(paths: &[PathBuf]) -> (Vec<InputInfo>, Option<String>) {
    let mut inputs = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    for path in paths {
        match compute_sha256(path) {
            Ok(sha256) => inputs.push(InputInfo {
                path: path.to_string_lossy().to_string(),
                sha256,
            }),
            Err(err) => {
                errors.push(err);
                inputs.push(InputInfo {
                    path: path.to_string_lossy().to_string(),
                    sha256: "UNKNOWN".to_string(),
                });
            }
        }
    }

    let error = if errors.is_empty() {
        None
    } else {
        Some(errors.join("\n"))
    };

    (inputs, error)
}

fn compute_sha256(path: &Path) -> Result<String, String> {
    let data = fs::read(path).map_err(|err| format!("{}: {err}", path.display()))?;
    let mut hasher = Sha256::new();
    hasher.update(&data);
    Ok(hex::encode(hasher.finalize()))
}

fn run_typecheck(
    file: &Path,
    io_error: Option<&String>,
    parallel: usize,
    deterministic: bool,
    seed: u64,
) -> CheckResult {
    if let Some(message) = io_error {
        return error_check(
            "typecheck",
            None,
            None,
            ReasonKind::InvalidInput,
            message.clone(),
        );
    }

    let source = match fs::read_to_string(file) {
        Ok(source) => source,
        Err(err) => {
            return error_check(
                "typecheck",
                None,
                None,
                ReasonKind::InvalidInput,
                format!("{}: {err}", file.display()),
            )
        }
    };

    let frontend = SimpleFrontend;
    match frontend.parse_and_typecheck(&source, &file.to_string_lossy()) {
        Ok(output) => {
            let stats = build_stats(&output.ir, parallel, deterministic, seed);
            CheckResult {
                name: "typecheck".to_string(),
                model: None,
                target: None,
                status: Status::Pass,
                reason: None,
                counterexample: None,
                stats: Some(stats),
            }
        }
        Err(err) => {
            let (status, reason_kind) = match err.kind {
                FrontendErrorKind::UnsupportedSyntax => {
                    (Status::Unsupported, ReasonKind::UnsupportedSyntax)
                }
                FrontendErrorKind::InvalidInput => (Status::Error, ReasonKind::InvalidInput),
            };
            CheckResult {
                name: "typecheck".to_string(),
                model: None,
                target: None,
                status,
                reason: Some(Reason {
                    kind: reason_kind,
                    message: Some(err.to_string()),
                }),
                counterexample: None,
                stats: Some(Stats {
                    states: None,
                    transitions: None,
                }),
            }
        }
    }
}

fn build_stats(
    module: &cspx_core::ir::Module,
    parallel: usize,
    deterministic: bool,
    seed: u64,
) -> Stats {
    let provider = match SimpleTransitionProvider::from_module(module) {
        Ok(provider) => provider,
        Err(_) => {
            return Stats {
                states: None,
                transitions: None,
            }
        }
    };

    let mut store = InMemoryStateStore::new();
    let result = if deterministic {
        explore_parallel_with_options(
            &provider,
            &mut store,
            ParallelExploreOptions {
                workers: parallel,
                deterministic: true,
                seed,
            },
        )
    } else if parallel > 1 {
        explore_parallel(&provider, &mut store, parallel)
    } else {
        let mut queue = VecWorkQueue::new();
        explore(&provider, &mut store, &mut queue)
    };

    match result {
        Ok(stats) => stats,
        Err(_) => Stats {
            states: None,
            transitions: None,
        },
    }
}

fn build_stub_check_result(
    name: &str,
    model: Option<String>,
    target: Option<String>,
    io_error: Option<&String>,
    not_implemented_message: &str,
) -> CheckResult {
    if let Some(message) = io_error {
        return error_check(
            name,
            model,
            target,
            ReasonKind::InvalidInput,
            message.clone(),
        );
    }

    CheckResult {
        name: name.to_string(),
        model,
        target,
        status: Status::Unsupported,
        reason: Some(Reason {
            kind: ReasonKind::NotImplemented,
            message: Some(not_implemented_message.to_string()),
        }),
        counterexample: None,
        stats: Some(Stats {
            states: None,
            transitions: None,
        }),
    }
}

fn run_check_by_assertion(file: &Path, io_error: Option<&String>, assertion: &str) -> CheckResult {
    let supported = ["deadlock free", "divergence free", "deterministic"];
    if !supported.contains(&assertion) {
        return error_check(
            "check",
            None,
            Some(assertion.to_string()),
            ReasonKind::InvalidInput,
            format!(
                "unknown assertion: {assertion} (supported: {})",
                supported.join(", ")
            ),
        );
    }

    let module = match parse_module_for_check(file, io_error, assertion) {
        Ok(module) => module,
        Err(check) => return *check,
    };

    match assertion {
        "deadlock free" => {
            let checker = DeadlockChecker;
            let request = CheckRequest {
                command: cspx_core::check::CheckCommand::Check,
                model: None,
                target: Some(assertion.to_string()),
            };
            checker.check(&request, &module)
        }
        "divergence free" => {
            let checker = DivergenceChecker;
            let request = CheckRequest {
                command: cspx_core::check::CheckCommand::Check,
                model: None,
                target: Some(assertion.to_string()),
            };
            checker.check(&request, &module)
        }
        "deterministic" => {
            let checker = DeterminismChecker;
            let request = CheckRequest {
                command: cspx_core::check::CheckCommand::Check,
                model: None,
                target: Some(assertion.to_string()),
            };
            checker.check(&request, &module)
        }
        _ => build_stub_check_result(
            "check",
            None,
            Some(assertion.to_string()),
            None,
            "assertion not implemented yet",
        ),
    }
}

fn run_all_assertions(file: &Path, io_error: Option<&String>) -> Vec<CheckResult> {
    let module = match parse_module_for_check(file, io_error, "all-assertions") {
        Ok(module) => module,
        Err(check) => return vec![*check],
    };

    if module.assertions.is_empty() {
        return vec![error_check(
            "check",
            None,
            Some("all-assertions".to_string()),
            ReasonKind::InvalidInput,
            "no assertions found".to_string(),
        )];
    }

    let mut out = Vec::new();
    for assertion in &module.assertions {
        match assertion {
            cspx_core::ir::AssertionDecl::Property {
                target,
                kind,
                model,
            } => {
                let check_target = format!(
                    "{} :[{} [{}]]",
                    target.value,
                    property_kind_str(*kind),
                    property_model_str(*model)
                );
                match *kind {
                    cspx_core::ir::PropertyKind::DeadlockFree => out.push(
                        run_deadlock_property_assertion(&module, &target.value, check_target),
                    ),
                    cspx_core::ir::PropertyKind::DivergenceFree => out.push(
                        run_divergence_property_assertion(&module, &target.value, check_target),
                    ),
                    cspx_core::ir::PropertyKind::Deterministic => out.push(
                        run_determinism_property_assertion(&module, &target.value, check_target),
                    ),
                }
            }
            cspx_core::ir::AssertionDecl::Refinement { spec, model, impl_ } => {
                let model_str = refinement_op_str(*model).to_string();
                let check_target = format!("{} [{}= {}", spec.value, model_str, impl_.value);
                out.push(run_refinement_assertion(
                    &module,
                    &spec.value,
                    *model,
                    &impl_.value,
                    check_target,
                ));
            }
        }
    }
    out
}

fn run_deadlock_property_assertion(
    module: &cspx_core::ir::Module,
    target_proc: &str,
    target_desc: String,
) -> CheckResult {
    let Some(expr) = module
        .declarations
        .iter()
        .find(|decl| decl.name.value == target_proc)
        .map(|decl| decl.expr.clone())
    else {
        return error_check(
            "check",
            None,
            Some(target_desc),
            ReasonKind::InvalidInput,
            format!("undefined process: {target_proc}"),
        );
    };

    let mut check_module = module.clone();
    check_module.entry = Some(expr);

    let checker = DeadlockChecker;
    let request = CheckRequest {
        command: cspx_core::check::CheckCommand::Check,
        model: None,
        target: Some(target_desc),
    };
    checker.check(&request, &check_module)
}

fn run_divergence_property_assertion(
    module: &cspx_core::ir::Module,
    target_proc: &str,
    target_desc: String,
) -> CheckResult {
    let Some(expr) = module
        .declarations
        .iter()
        .find(|decl| decl.name.value == target_proc)
        .map(|decl| decl.expr.clone())
    else {
        return error_check(
            "check",
            None,
            Some(target_desc),
            ReasonKind::InvalidInput,
            format!("undefined process: {target_proc}"),
        );
    };

    let mut check_module = module.clone();
    check_module.entry = Some(expr);

    let checker = DivergenceChecker;
    let request = CheckRequest {
        command: cspx_core::check::CheckCommand::Check,
        model: None,
        target: Some(target_desc),
    };
    checker.check(&request, &check_module)
}

fn run_determinism_property_assertion(
    module: &cspx_core::ir::Module,
    target_proc: &str,
    target_desc: String,
) -> CheckResult {
    let Some(expr) = module
        .declarations
        .iter()
        .find(|decl| decl.name.value == target_proc)
        .map(|decl| decl.expr.clone())
    else {
        return error_check(
            "check",
            None,
            Some(target_desc),
            ReasonKind::InvalidInput,
            format!("undefined process: {target_proc}"),
        );
    };

    let mut check_module = module.clone();
    check_module.entry = Some(expr);

    let checker = DeterminismChecker;
    let request = CheckRequest {
        command: cspx_core::check::CheckCommand::Check,
        model: None,
        target: Some(target_desc),
    };
    checker.check(&request, &check_module)
}

fn run_refinement_assertion(
    module: &cspx_core::ir::Module,
    spec_proc: &str,
    model: cspx_core::ir::RefinementOp,
    impl_proc: &str,
    target_desc: String,
) -> CheckResult {
    let Some(spec_expr) = module
        .declarations
        .iter()
        .find(|decl| decl.name.value == spec_proc)
        .map(|decl| decl.expr.clone())
    else {
        return error_check(
            "refine",
            Some(refinement_op_str(model).to_string()),
            Some(target_desc),
            ReasonKind::InvalidInput,
            format!("undefined process: {spec_proc}"),
        );
    };
    let Some(impl_expr) = module
        .declarations
        .iter()
        .find(|decl| decl.name.value == impl_proc)
        .map(|decl| decl.expr.clone())
    else {
        return error_check(
            "refine",
            Some(refinement_op_str(model).to_string()),
            Some(target_desc),
            ReasonKind::InvalidInput,
            format!("undefined process: {impl_proc}"),
        );
    };

    let mut spec_module = module.clone();
    spec_module.entry = Some(spec_expr);
    let mut impl_module = module.clone();
    impl_module.entry = Some(impl_expr);

    let checker = RefinementChecker;
    let request = CheckRequest {
        command: cspx_core::check::CheckCommand::Refine,
        model: Some(match model {
            cspx_core::ir::RefinementOp::T => cspx_core::check::RefinementModel::T,
            cspx_core::ir::RefinementOp::F => cspx_core::check::RefinementModel::F,
            cspx_core::ir::RefinementOp::FD => cspx_core::check::RefinementModel::FD,
        }),
        target: Some(target_desc),
    };
    let input = RefinementInput {
        spec: spec_module,
        impl_: impl_module,
    };
    checker.check(&request, &input)
}

fn property_kind_str(kind: cspx_core::ir::PropertyKind) -> &'static str {
    match kind {
        cspx_core::ir::PropertyKind::DeadlockFree => "deadlock free",
        cspx_core::ir::PropertyKind::DivergenceFree => "divergence free",
        cspx_core::ir::PropertyKind::Deterministic => "deterministic",
    }
}

fn property_model_str(model: cspx_core::ir::PropertyModel) -> &'static str {
    match model {
        cspx_core::ir::PropertyModel::F => "F",
        cspx_core::ir::PropertyModel::FD => "FD",
    }
}

fn refinement_op_str(model: cspx_core::ir::RefinementOp) -> &'static str {
    match model {
        cspx_core::ir::RefinementOp::T => "T",
        cspx_core::ir::RefinementOp::F => "F",
        cspx_core::ir::RefinementOp::FD => "FD",
    }
}

fn parse_module_for_check(
    file: &Path,
    io_error: Option<&String>,
    assertion: &str,
) -> Result<cspx_core::ir::Module, Box<CheckResult>> {
    if let Some(message) = io_error {
        return Err(Box::new(error_check(
            "check",
            None,
            Some(assertion.to_string()),
            ReasonKind::InvalidInput,
            message.clone(),
        )));
    }

    let source = match fs::read_to_string(file) {
        Ok(source) => source,
        Err(err) => {
            return Err(Box::new(error_check(
                "check",
                None,
                Some(assertion.to_string()),
                ReasonKind::InvalidInput,
                format!("{}: {err}", file.display()),
            )))
        }
    };

    let frontend = SimpleFrontend;
    match frontend.parse_and_typecheck(&source, &file.to_string_lossy()) {
        Ok(output) => Ok(output.ir),
        Err(err) => {
            let (status, reason_kind) = match err.kind {
                FrontendErrorKind::UnsupportedSyntax => {
                    (Status::Unsupported, ReasonKind::UnsupportedSyntax)
                }
                FrontendErrorKind::InvalidInput => (Status::Error, ReasonKind::InvalidInput),
            };
            Err(Box::new(CheckResult {
                name: "check".to_string(),
                model: None,
                target: Some(assertion.to_string()),
                status,
                reason: Some(Reason {
                    kind: reason_kind,
                    message: Some(err.to_string()),
                }),
                counterexample: None,
                stats: Some(Stats {
                    states: None,
                    transitions: None,
                }),
            }))
        }
    }
}

fn error_check(
    name: &str,
    model: Option<String>,
    target: Option<String>,
    kind: ReasonKind,
    message: String,
) -> CheckResult {
    CheckResult {
        name: name.to_string(),
        model,
        target,
        status: Status::Error,
        reason: Some(Reason {
            kind,
            message: Some(message),
        }),
        counterexample: None,
        stats: Some(Stats {
            states: None,
            transitions: None,
        }),
    }
}

fn run_refine_check(args: &RefineArgs, io_error: Option<&String>) -> CheckResult {
    if let Some(message) = io_error {
        return error_check(
            "refine",
            Some(args.model.as_str().to_string()),
            Some(format!(
                "{} {}",
                args.spec.to_string_lossy(),
                args.impl_.to_string_lossy()
            )),
            ReasonKind::InvalidInput,
            message.clone(),
        );
    }

    let spec_source = match fs::read_to_string(&args.spec) {
        Ok(source) => source,
        Err(err) => {
            return error_check(
                "refine",
                Some(args.model.as_str().to_string()),
                Some(format!(
                    "{} {}",
                    args.spec.to_string_lossy(),
                    args.impl_.to_string_lossy()
                )),
                ReasonKind::InvalidInput,
                format!("{}: {err}", args.spec.display()),
            )
        }
    };
    let impl_source = match fs::read_to_string(&args.impl_) {
        Ok(source) => source,
        Err(err) => {
            return error_check(
                "refine",
                Some(args.model.as_str().to_string()),
                Some(format!(
                    "{} {}",
                    args.spec.to_string_lossy(),
                    args.impl_.to_string_lossy()
                )),
                ReasonKind::InvalidInput,
                format!("{}: {err}", args.impl_.display()),
            )
        }
    };

    let frontend = SimpleFrontend;
    let spec_ir = match frontend.parse_and_typecheck(&spec_source, &args.spec.to_string_lossy()) {
        Ok(output) => output.ir,
        Err(err) => {
            let (status, reason_kind) = match err.kind {
                FrontendErrorKind::UnsupportedSyntax => {
                    (Status::Unsupported, ReasonKind::UnsupportedSyntax)
                }
                FrontendErrorKind::InvalidInput => (Status::Error, ReasonKind::InvalidInput),
            };
            return CheckResult {
                name: "refine".to_string(),
                model: Some(args.model.as_str().to_string()),
                target: Some(format!(
                    "{} {}",
                    args.spec.to_string_lossy(),
                    args.impl_.to_string_lossy()
                )),
                status,
                reason: Some(Reason {
                    kind: reason_kind,
                    message: Some(err.to_string()),
                }),
                counterexample: None,
                stats: Some(Stats {
                    states: None,
                    transitions: None,
                }),
            };
        }
    };
    let impl_ir = match frontend.parse_and_typecheck(&impl_source, &args.impl_.to_string_lossy()) {
        Ok(output) => output.ir,
        Err(err) => {
            let (status, reason_kind) = match err.kind {
                FrontendErrorKind::UnsupportedSyntax => {
                    (Status::Unsupported, ReasonKind::UnsupportedSyntax)
                }
                FrontendErrorKind::InvalidInput => (Status::Error, ReasonKind::InvalidInput),
            };
            return CheckResult {
                name: "refine".to_string(),
                model: Some(args.model.as_str().to_string()),
                target: Some(format!(
                    "{} {}",
                    args.spec.to_string_lossy(),
                    args.impl_.to_string_lossy()
                )),
                status,
                reason: Some(Reason {
                    kind: reason_kind,
                    message: Some(err.to_string()),
                }),
                counterexample: None,
                stats: Some(Stats {
                    states: None,
                    transitions: None,
                }),
            };
        }
    };

    let checker = RefinementChecker;
    let request = CheckRequest {
        command: cspx_core::check::CheckCommand::Refine,
        model: Some(match args.model {
            RefinementModel::T => cspx_core::check::RefinementModel::T,
            RefinementModel::F => cspx_core::check::RefinementModel::F,
            RefinementModel::FD => cspx_core::check::RefinementModel::FD,
        }),
        target: Some(format!(
            "{} {}",
            args.spec.to_string_lossy(),
            args.impl_.to_string_lossy()
        )),
    };
    let input = RefinementInput {
        spec: spec_ir,
        impl_: impl_ir,
    };
    checker.check(&request, &input)
}

fn emit_json(result: &ResultJson, output: Option<&Path>) -> Result<()> {
    let payload = serde_json::to_string_pretty(result).context("serialize result json")?;
    if let Some(path) = output {
        write_atomic(path, payload.as_bytes())?;
        return Ok(());
    }

    println!("{payload}");
    Ok(())
}

fn emit_text(result: &ResultJson, output: Option<&Path>) -> Result<()> {
    let summary = format!(
        "status={} exit_code={}",
        status_label(&result.status),
        result.exit_code
    );
    if let Some(path) = output {
        write_atomic(path, summary.as_bytes())?;
        return Ok(());
    }
    println!("{summary}");
    Ok(())
}

fn emit_summary_json(
    result: &ResultJson,
    summary_path: &Path,
    output_path: Option<&Path>,
    output_format: OutputFormat,
) -> Result<()> {
    let mode = summary_mode(&result.invocation.command);
    let details_file = if matches!(output_format, OutputFormat::Json) {
        output_path.map(|path| path.to_string_lossy().to_string())
    } else {
        None
    };

    let summary = CspSummaryJson {
        tool: "csp".to_string(),
        file: result
            .inputs
            .first()
            .map(|input| input.path.clone())
            .unwrap_or_default(),
        backend: format!("cspx:{mode}"),
        details_file,
        result_status: Some(status_label(&result.status).to_string()),
        ran: true,
        status: summary_status_label(&result.status).to_string(),
        exit_code: result.exit_code,
        timestamp: result.finished_at.clone(),
        output: clamp_text(&summarize_result_for_summary(result), 4000),
    };

    let payload = serde_json::to_string_pretty(&summary).context("serialize summary json")?;
    write_atomic(summary_path, payload.as_bytes())
}

fn summary_mode(command: &str) -> &'static str {
    match command {
        "typecheck" => "typecheck",
        "check" => "assertions",
        "refine" => "refine",
        _ => "unknown",
    }
}

fn summary_status_label(status: &Status) -> &'static str {
    match status {
        Status::Pass => "ran",
        Status::Fail => "failed",
        Status::Unsupported => "unsupported",
        Status::Timeout => "timeout",
        Status::OutOfMemory => "out_of_memory",
        Status::Error => "error",
    }
}

fn summarize_result_for_summary(result: &ResultJson) -> String {
    let checks_line = if result.checks.is_empty() {
        "checks=n/a".to_string()
    } else {
        let checks = result
            .checks
            .iter()
            .map(|check| format!("{}:{}", check.name, status_label(&check.status)))
            .collect::<Vec<_>>()
            .join(",");
        format!("checks={checks}")
    };

    let reason_suffix = result
        .checks
        .iter()
        .find_map(|check| check.reason.as_ref())
        .map(|reason| {
            let mut reason_text = format!(" reason={}", reason_kind_label(&reason.kind));
            if let Some(message) = reason.message.as_ref() {
                reason_text.push(':');
                reason_text.push_str(&message.chars().take(120).collect::<String>());
            }
            reason_text
        })
        .unwrap_or_default();

    format!(
        "cspx schema={} status={} exit_code={} {}{}",
        result.schema_version,
        status_label(&result.status),
        result.exit_code,
        checks_line,
        reason_suffix
    )
}

fn reason_kind_label(kind: &ReasonKind) -> &'static str {
    match kind {
        ReasonKind::NotImplemented => "not_implemented",
        ReasonKind::UnsupportedSyntax => "unsupported_syntax",
        ReasonKind::InvalidInput => "invalid_input",
        ReasonKind::InternalError => "internal_error",
        ReasonKind::Timeout => "timeout",
        ReasonKind::OutOfMemory => "out_of_memory",
    }
}

fn clamp_text(text: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for (idx, ch) in text.chars().enumerate() {
        if idx >= max_chars {
            out.push('…');
            return out;
        }
        out.push(ch);
    }
    out
}

fn write_atomic(path: &Path, contents: &[u8]) -> Result<()> {
    let file_name = path
        .file_name()
        .ok_or_else(|| anyhow!("output path must include a file name"))?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let tmp_name = format!(
        "{}.tmp.{}.{}",
        file_name.to_string_lossy(),
        std::process::id(),
        nonce
    );
    let tmp_path = path.with_file_name(tmp_name);
    fs::write(&tmp_path, contents).with_context(|| format!("write {}", tmp_path.display()))?;
    fs::rename(&tmp_path, path).with_context(|| format!("rename {}", path.display()))?;
    Ok(())
}

fn status_label(status: &Status) -> &'static str {
    match status {
        Status::Pass => "pass",
        Status::Fail => "fail",
        Status::Unsupported => "unsupported",
        Status::Timeout => "timeout",
        Status::OutOfMemory => "out_of_memory",
        Status::Error => "error",
    }
}
