use anyhow::{Context, Result};
use chrono::{SecondsFormat, Utc};
use clap::{ArgGroup, Args, Parser, Subcommand, ValueEnum};
use cspx_core::{
    CheckResult, Frontend, FrontendErrorKind, Reason, ReasonKind, SimpleFrontend, Stats, Status,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
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
            let check = build_stub_check_result(
                "check",
                None,
                target.clone(),
                io_error.as_ref(),
                "checker not implemented yet",
            );
            (
                "check".to_string(),
                vec![args.file.to_string_lossy().to_string()],
                inputs,
                check,
            )
        }
        Command::Refine(args) => {
            let (inputs, io_error) = build_inputs(&[args.spec.clone(), args.impl_.clone()]);
            let check = build_stub_check_result(
                "refine",
                Some(args.model.as_str().to_string()),
                Some(format!(
                    "{} {}",
                    args.spec.to_string_lossy(),
                    args.impl_.to_string_lossy()
                )),
                io_error.as_ref(),
                "refinement not implemented yet",
            );
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
    let mut error: Option<String> = None;

    for path in paths {
        match compute_sha256(path) {
            Ok(sha256) => inputs.push(InputInfo {
                path: path.to_string_lossy().to_string(),
                sha256,
            }),
            Err(err) => {
                if error.is_none() {
                    error = Some(err);
                }
                inputs.push(InputInfo {
                    path: path.to_string_lossy().to_string(),
                    sha256: "UNKNOWN".to_string(),
                });
            }
        }
    }

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
        Ok(_) => CheckResult {
            name: "typecheck".to_string(),
            model: None,
            target: None,
            status: Status::Pass,
            reason: None,
            counterexample: None,
            stats: Some(Stats {
                states: None,
                transitions: None,
            }),
        },
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
    let tmp_path = path.with_extension("tmp");
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
