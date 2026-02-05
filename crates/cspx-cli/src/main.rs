use anyhow::{anyhow, Context, Result};
use chrono::{SecondsFormat, Utc};
use clap::{ArgGroup, Args, Parser, Subcommand, ValueEnum};
use cspx_core::{
    explore, CheckRequest, CheckResult, Checker, DeadlockChecker, Frontend, FrontendErrorKind,
    InMemoryStateStore, Reason, ReasonKind, RefinementChecker, RefinementInput, SimpleFrontend,
    SimpleTransitionProvider, Stats, Status, VecWorkQueue,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use std::time::Instant;

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
    timeout_ms: Option<u64>,

    #[arg(long, global = true)]
    memory_mb: Option<u64>,

    #[arg(long, default_value_t = 0, global = true)]
    seed: u64,
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
    seed: u64,
}

#[derive(Serialize)]
struct InputInfo {
    path: String,
    sha256: String,
}

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

    let (status, exit_code, check, inputs, invocation) = execute(&cli)?;

    let finished_at = Utc::now();
    let duration_ms = timer.elapsed().as_millis() as u64;

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
        checks: vec![check],
    };

    match cli.format {
        OutputFormat::Json => emit_json(&result, cli.output.as_deref()),
        OutputFormat::Text => emit_text(&result, cli.output.as_deref()),
    }?;

    Ok(exit_code)
}

fn execute(cli: &Cli) -> Result<(Status, i32, CheckResult, Vec<InputInfo>, Invocation)> {
    let (command, args, inputs, check) = match &cli.command {
        Command::Typecheck { file } => {
            let (inputs, io_error) = build_inputs(&[file.clone()]);
            let check = run_typecheck(file, io_error.as_ref());
            (
                "typecheck".to_string(),
                vec![file.to_string_lossy().to_string()],
                inputs,
                check,
            )
        }
        Command::Check(args) => {
            let (inputs, io_error) = build_inputs(&[args.file.clone()]);
            let target = if let Some(assertion) = &args.assert {
                Some(assertion.clone())
            } else if args.all_assertions {
                Some("all-assertions".to_string())
            } else {
                None
            };
            let check = if let Some(assertion) = &args.assert {
                if assertion == "deadlock free" {
                    run_deadlock_check(&args.file, io_error.as_ref(), assertion)
                } else {
                    build_stub_check_result(
                        "check",
                        None,
                        target.clone(),
                        io_error.as_ref(),
                        "assertion not implemented yet",
                    )
                }
            } else if args.all_assertions {
                build_stub_check_result(
                    "check",
                    None,
                    target.clone(),
                    io_error.as_ref(),
                    "all-assertions not implemented yet",
                )
            } else {
                build_stub_check_result(
                    "check",
                    None,
                    target.clone(),
                    io_error.as_ref(),
                    "checker not implemented yet",
                )
            };
            (
                "check".to_string(),
                vec![args.file.to_string_lossy().to_string()],
                inputs,
                check,
            )
        }
        Command::Refine(args) => {
            let (inputs, io_error) = build_inputs(&[args.spec.clone(), args.impl_.clone()]);
            let check = run_refine_check(args, io_error.as_ref());
            (
                "refine".to_string(),
                vec![
                    args.spec.to_string_lossy().to_string(),
                    args.impl_.to_string_lossy().to_string(),
                ],
                inputs,
                check,
            )
        }
    };

    let status = check.status.clone();
    let exit_code = match status {
        Status::Pass => 0,
        Status::Fail => 1,
        Status::Error => 2,
        Status::Unsupported => 3,
        Status::Timeout => 4,
        Status::OutOfMemory => 5,
    };

    let invocation = Invocation {
        command,
        args,
        format: match cli.format {
            OutputFormat::Json => "json".to_string(),
            OutputFormat::Text => "text".to_string(),
        },
        timeout_ms: cli.timeout_ms,
        memory_mb: cli.memory_mb,
        seed: cli.seed,
    };

    Ok((status, exit_code, check, inputs, invocation))
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

fn run_typecheck(file: &Path, io_error: Option<&String>) -> CheckResult {
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

    let frontend = SimpleFrontend::default();
    match frontend.parse_and_typecheck(&source, &file.to_string_lossy()) {
        Ok(output) => {
            let stats = build_stats(&output.ir);
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

fn build_stats(module: &cspx_core::ir::Module) -> Stats {
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
    let mut queue = VecWorkQueue::new();
    match explore(&provider, &mut store, &mut queue) {
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

fn run_deadlock_check(file: &Path, io_error: Option<&String>, assertion: &str) -> CheckResult {
    if let Some(message) = io_error {
        return error_check(
            "check",
            None,
            Some(assertion.to_string()),
            ReasonKind::InvalidInput,
            message.clone(),
        );
    }

    let source = match fs::read_to_string(file) {
        Ok(source) => source,
        Err(err) => {
            return error_check(
                "check",
                None,
                Some(assertion.to_string()),
                ReasonKind::InvalidInput,
                format!("{}: {err}", file.display()),
            )
        }
    };

    let frontend = SimpleFrontend::default();
    match frontend.parse_and_typecheck(&source, &file.to_string_lossy()) {
        Ok(output) => {
            let checker = DeadlockChecker::default();
            let request = CheckRequest {
                command: cspx_core::check::CheckCommand::Check,
                model: None,
                target: Some(assertion.to_string()),
            };
            checker.check(&request, &output.ir)
        }
        Err(err) => {
            let (status, reason_kind) = match err.kind {
                FrontendErrorKind::UnsupportedSyntax => {
                    (Status::Unsupported, ReasonKind::UnsupportedSyntax)
                }
                FrontendErrorKind::InvalidInput => (Status::Error, ReasonKind::InvalidInput),
            };
            CheckResult {
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
            }
        }
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

    let frontend = SimpleFrontend::default();
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

    let checker = RefinementChecker::default();
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

fn write_atomic(path: &Path, contents: &[u8]) -> Result<()> {
    let file_name = path
        .file_name()
        .ok_or_else(|| anyhow!("output path must include a file name"))?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let tmp_name = format!("{}.tmp.{}.{}", file_name.to_string_lossy(), std::process::id(), nonce);
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
