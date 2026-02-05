use anyhow::{Context, Result};
use clap::{Parser, ValueEnum};
use jsonschema::JSONSchema;
use serde::Deserialize;
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;
use wait_timeout::ChildExt;

#[derive(Clone, ValueEnum)]
enum Suite {
    Fast,
    Bench,
}

#[derive(Parser)]
struct Args {
    #[arg(long, value_enum, default_value = "fast")]
    suite: Suite,
    #[arg(long)]
    list: bool,
    #[arg(long)]
    only: Vec<String>,
    #[arg(long = "only-dir")]
    only_dir: Vec<PathBuf>,
    #[arg(long)]
    cspx: Option<PathBuf>,
    #[arg(long, default_value_t = 1)]
    jobs: usize,
}

#[derive(Debug, Deserialize)]
struct ProblemSpec {
    id: String,
    title: String,
    suite: Option<String>,
    tags: Option<Vec<String>>,
    timeout_ms: Option<u64>,
    run: RunSpec,
}

#[derive(Debug, Deserialize)]
struct RunSpec {
    cmd: Vec<String>,
    cwd: Option<String>,
    env: Option<HashMap<String, String>>,
    timeout_ms: Option<u64>,
    repeat: Option<u32>,
}

struct Problem {
    dir: PathBuf,
    spec: ProblemSpec,
}

fn main() -> Result<()> {
    let args = Args::parse();
    if args.jobs > 1 {
        eprintln!("warning: --jobs > 1 is not supported yet; running sequentially");
    }

    let root = std::env::current_dir().context("current dir")?;
    let problems_dir = root.join("problems");
    let schema = load_problem_schema(&root)?;
    let mut problems = load_problems(&problems_dir, &schema)?;
    problems.sort_by(|a, b| a.spec.id.cmp(&b.spec.id));

    let filtered = filter_problems(&problems, &args)?;
    if args.list {
        for problem in &filtered {
            println!("{} {}", problem.spec.id, problem.spec.title);
        }
        return Ok(());
    }

    let out_root = problems_dir.join(".out");
    for problem in filtered {
        run_problem(&out_root, problem, &args)?;
    }

    Ok(())
}

fn load_problem_schema(root: &Path) -> Result<JSONSchema> {
    let schema_path = root.join("schemas").join("problem.schema.json");
    let schema_text = fs::read_to_string(&schema_path)
        .with_context(|| format!("read {}", schema_path.display()))?;
    let schema_json: JsonValue =
        serde_json::from_str(&schema_text).context("parse problem schema")?;
    let schema_json = Box::leak(Box::new(schema_json));
    JSONSchema::compile(schema_json).context("compile problem schema")
}

fn load_problems(problems_dir: &Path, schema: &JSONSchema) -> Result<Vec<Problem>> {
    let mut problems = Vec::new();
    if !problems_dir.exists() {
        return Ok(problems);
    }
    for entry in fs::read_dir(problems_dir).context("read problems dir")? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let dir = entry.path();
        let problem_yaml = dir.join("problem.yaml");
        if !problem_yaml.exists() {
            continue;
        }
        let text = fs::read_to_string(&problem_yaml)
            .with_context(|| format!("read {}", problem_yaml.display()))?;
        let raw_json: JsonValue =
            serde_yaml::from_str(&text).context("parse problem.yaml as json")?;
        if let Err(errors) = schema.validate(&raw_json) {
            let details: Vec<String> = errors.map(|err| err.to_string()).collect();
            anyhow::bail!(
                "problem schema validation failed: {}\n{}",
                problem_yaml.display(),
                details.join("\n")
            );
        }
        let spec: ProblemSpec = serde_yaml::from_str(&text).context("parse problem.yaml")?;
        problems.push(Problem { dir, spec });
    }
    Ok(problems)
}

fn filter_problems<'a>(problems: &'a [Problem], args: &Args) -> Result<Vec<&'a Problem>> {
    let only_ids = &args.only;
    let only_dirs = &args.only_dir;
    let use_only = !only_ids.is_empty() || !only_dirs.is_empty();
    let suite = match args.suite {
        Suite::Fast => "fast",
        Suite::Bench => "bench",
    };

    let mut filtered = Vec::new();
    for problem in problems {
        if use_only {
            let mut matched = false;
            if only_ids.iter().any(|id| id == &problem.spec.id) {
                matched = true;
            }
            if only_dirs.iter().any(|dir| dir == &problem.dir) {
                matched = true;
            }
            if matched {
                filtered.push(problem);
            }
            continue;
        }
        let problem_suite = effective_suite(&problem.spec);
        if problem_suite == suite {
            filtered.push(problem);
        }
    }
    Ok(filtered)
}

fn effective_suite(spec: &ProblemSpec) -> &str {
    if let Some(suite) = spec.suite.as_deref() {
        return suite;
    }
    if let Some(tags) = &spec.tags {
        if tags.iter().any(|tag| tag == "bench") {
            return "bench";
        }
        if tags.iter().any(|tag| tag == "fast") {
            return "fast";
        }
    }
    "fast"
}

fn run_problem(out_root: &Path, problem: &Problem, args: &Args) -> Result<()> {
    let run = &problem.spec.run;
    let repeat = run.repeat.unwrap_or(1);
    for idx in 1..=repeat {
        let out_dir = out_root.join(&problem.spec.id).join(format!("run-{}", idx));
        fs::create_dir_all(&out_dir).with_context(|| format!("create {}", out_dir.display()))?;

        let outcome = execute_run(problem, args)?;
        fs::write(out_dir.join("stdout.txt"), &outcome.stdout)?;
        fs::write(out_dir.join("stderr.txt"), &outcome.stderr)?;
        fs::write(out_dir.join("exit_code.txt"), outcome.exit_code.to_string())?;
        if let Some(result_json) = &outcome.result_json {
            fs::write(
                out_dir.join("result.json"),
                serde_json::to_vec_pretty(result_json)?,
            )?;
        }
        let status = outcome
            .result_status
            .unwrap_or_else(|| "unknown".to_string());
        println!(
            "DONE {} run={} exit={} status={}",
            problem.spec.id, idx, outcome.exit_code, status
        );
    }
    Ok(())
}

struct RunOutcome {
    exit_code: i32,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    result_json: Option<JsonValue>,
    result_status: Option<String>,
}

fn execute_run(problem: &Problem, args: &Args) -> Result<RunOutcome> {
    let run = &problem.spec.run;
    let mut cmd = run.cmd.clone();
    if let Some(cspx) = &args.cspx {
        if cmd.first().map(|s| s == "cspx").unwrap_or(false) {
            cmd[0] = cspx.to_string_lossy().to_string();
        }
    }
    let cwd = if let Some(cwd) = &run.cwd {
        let cwd_path = PathBuf::from(cwd);
        if cwd_path.is_absolute() {
            cwd_path
        } else {
            problem.dir.join(cwd_path)
        }
    } else {
        problem.dir.clone()
    };
    let timeout_ms = run.timeout_ms.or(problem.spec.timeout_ms);
    let timeout = timeout_ms.map(Duration::from_millis);

    let mut command = Command::new(&cmd[0]);
    if cmd.len() > 1 {
        command.args(&cmd[1..]);
    }
    command.current_dir(&cwd);
    if let Some(envs) = &run.env {
        for (key, value) in envs {
            command.env(key, value);
        }
    }
    command.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = command
        .spawn()
        .with_context(|| format!("spawn command: {} (cwd={})", cmd.join(" "), cwd.display()))?;
    let exit_code = if let Some(timeout) = timeout {
        match child.wait_timeout(timeout)? {
            Some(status) => status.code().unwrap_or(1),
            None => {
                child.kill().ok();
                let _ = child.wait();
                124
            }
        }
    } else {
        let status = child.wait()?;
        status.code().unwrap_or(1)
    };

    let mut stdout = Vec::new();
    if let Some(mut out) = child.stdout.take() {
        out.read_to_end(&mut stdout)?;
    }
    let mut stderr = Vec::new();
    if let Some(mut err) = child.stderr.take() {
        err.read_to_end(&mut stderr)?;
    }

    let result_json = serde_json::from_slice::<JsonValue>(&stdout).ok();
    let result_status = result_json
        .as_ref()
        .and_then(|json| json.get("status"))
        .and_then(|value| value.as_str())
        .map(|s| s.to_string());

    Ok(RunOutcome {
        exit_code,
        stdout,
        stderr,
        result_json,
        result_status,
    })
}
