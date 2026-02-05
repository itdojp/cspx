use anyhow::{Context, Result};
use clap::{Parser, ValueEnum};
use jsonschema::JSONSchema;
use regex::Regex;
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

#[derive(Debug, Deserialize)]
struct ExpectSpec {
    exit_code: Option<JsonValue>,
    status: Option<JsonValue>,
    checks: Option<Vec<CheckExpect>>,
    repeat: Option<u32>,
    compare: Option<CompareExpect>,
}

#[derive(Debug, Deserialize)]
struct CheckExpect {
    name: Option<JsonValue>,
    target: Option<JsonValue>,
    model: Option<JsonValue>,
    status: Option<JsonValue>,
    reason: Option<ReasonExpect>,
    counterexample: Option<CounterexampleExpect>,
    stats: Option<StatsExpect>,
}

#[derive(Debug, Deserialize)]
struct ReasonExpect {
    kind: Option<JsonValue>,
    message: Option<JsonValue>,
}

#[derive(Debug, Deserialize)]
struct CounterexampleExpect {
    present: Option<bool>,
    trace_len: Option<JsonValue>,
    tags: Option<TagConstraint>,
    is_minimized: Option<JsonValue>,
    source_spans: Option<SpanMatch>,
}

#[derive(Debug, Deserialize)]
struct TagConstraint {
    contains: Option<Vec<String>>,
    equals: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct SpanMatch {
    any: Option<Vec<SpanConstraint>>,
}

#[derive(Debug, Deserialize)]
struct SpanConstraint {
    path: Option<JsonValue>,
    start_line: Option<JsonValue>,
    start_col: Option<JsonValue>,
    end_line: Option<JsonValue>,
    end_col: Option<JsonValue>,
}

#[derive(Debug, Deserialize)]
struct StatsExpect {
    states: Option<JsonValue>,
    transitions: Option<JsonValue>,
}

#[derive(Debug, Deserialize)]
struct CompareExpect {
    kind: String,
    ignore_fields: Option<Vec<String>>,
}

struct Problem {
    dir: PathBuf,
    spec: ProblemSpec,
}

enum ProblemResult {
    Pass,
    Fail,
}

fn main() -> Result<()> {
    let args = Args::parse();
    if args.jobs > 1 {
        eprintln!("warning: --jobs > 1 is not supported yet; running sequentially");
    }

    let root = std::env::current_dir().context("current dir")?;
    let problems_dir = root.join("problems");
    let problem_schema = load_problem_schema(&root)?;
    let expect_schema = load_expect_schema(&root)?;
    let mut problems = load_problems(&problems_dir, &problem_schema)?;
    problems.sort_by(|a, b| a.spec.id.cmp(&b.spec.id));

    let filtered = filter_problems(&problems, &args)?;
    if args.list {
        for problem in &filtered {
            println!("{} {}", problem.spec.id, problem.spec.title);
        }
        return Ok(());
    }

    let out_root = problems_dir.join(".out");
    let mut any_fail = false;
    let mut any_error = false;
    for problem in filtered {
        match run_problem(&out_root, &problem, &args, &expect_schema) {
            Ok(ProblemResult::Pass) => {}
            Ok(ProblemResult::Fail) => {
                any_fail = true;
            }
            Err(err) => {
                any_error = true;
                eprintln!("ERROR {}: {}", problem.spec.id, err);
            }
        }
    }

    if any_error {
        std::process::exit(2);
    }
    if any_fail {
        std::process::exit(1);
    }
    Ok(())
}

fn load_problem_schema(root: &Path) -> Result<JSONSchema> {
    let schema_path = root.join("schemas").join("problem.schema.json");
    let schema_text = fs::read_to_string(&schema_path)
        .with_context(|| format!("read {}", schema_path.display()))?;
    let schema_json: JsonValue =
        serde_json::from_str(&schema_text).context("parse problem schema")?;
    JSONSchema::compile(&schema_json).context("compile problem schema")
}

fn load_expect_schema(root: &Path) -> Result<JSONSchema> {
    let schema_path = root.join("schemas").join("expect.schema.json");
    let schema_text = fs::read_to_string(&schema_path)
        .with_context(|| format!("read {}", schema_path.display()))?;
    let schema_json: JsonValue =
        serde_json::from_str(&schema_text).context("parse expect schema")?;
    JSONSchema::compile(&schema_json).context("compile expect schema")
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

fn load_expect(problem: &Problem, schema: &JSONSchema) -> Result<ExpectSpec> {
    let expect_path = problem.dir.join("expect.yaml");
    if !expect_path.exists() {
        anyhow::bail!("expect.yaml is required: {}", expect_path.display());
    }
    let text = fs::read_to_string(&expect_path)
        .with_context(|| format!("read {}", expect_path.display()))?;
    let raw_json: JsonValue = serde_yaml::from_str(&text).context("parse expect.yaml as json")?;
    if let Err(errors) = schema.validate(&raw_json) {
        let details: Vec<String> = errors.map(|err| err.to_string()).collect();
        anyhow::bail!(
            "expect schema validation failed: {}\n{}",
            expect_path.display(),
            details.join("\n")
        );
    }
    let spec: ExpectSpec = serde_yaml::from_str(&text).context("parse expect.yaml")?;
    Ok(spec)
}

fn evaluate_run(expect: &ExpectSpec, outcome: &RunOutcome, run_index: usize) -> Vec<String> {
    let mut errors = Vec::new();
    if let Some(exit_expect) = &expect.exit_code {
        let actual = outcome.exit_code as i64;
        if !match_int(actual, exit_expect) {
            errors.push(format!(
                "run {}: exit_code mismatch (actual {})",
                run_index, actual
            ));
        }
    }

    let result_json = match &outcome.result_json {
        Some(json) => json,
        None => {
            if expect.status.is_some() || expect.checks.is_some() {
                errors.push(format!("run {}: result JSON missing", run_index));
            }
            return errors;
        }
    };

    if let Some(status_expect) = &expect.status {
        let actual = result_json
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if actual.is_empty() || !match_string(actual, status_expect) {
            errors.push(format!(
                "run {}: status mismatch (actual {})",
                run_index, actual
            ));
        }
    }

    if let Some(checks_expect) = &expect.checks {
        let checks_actual = result_json.get("checks").and_then(|v| v.as_array());
        if checks_actual.is_none() {
            errors.push(format!("run {}: checks missing", run_index));
        } else {
            let checks_actual = checks_actual.unwrap();
            for (idx, check_expect) in checks_expect.iter().enumerate() {
                let mut matched = false;
                for check_actual in checks_actual {
                    if check_matches(check_expect, check_actual) {
                        matched = true;
                        break;
                    }
                }
                if !matched {
                    errors.push(format!(
                        "run {}: checks[{}] not matched (expect {})",
                        run_index,
                        idx,
                        describe_check_expect(check_expect)
                    ));
                }
            }
        }
    }

    errors
}

fn evaluate_compare(expect: &ExpectSpec, outcomes: &[RunOutcome], problem_id: &str) -> Vec<String> {
    let mut errors = Vec::new();
    let Some(compare) = &expect.compare else {
        return errors;
    };
    if compare.kind != "normalized_json_equal" {
        errors.push(format!("compare: unsupported kind {}", compare.kind));
        return errors;
    }
    if outcomes.len() < 2 {
        errors.push("compare: repeat must be >= 2".to_string());
        return errors;
    }
    let ignore = compare.ignore_fields.clone().unwrap_or_default();
    let mut normalized = Vec::new();
    for (idx, outcome) in outcomes.iter().enumerate() {
        let Some(json) = &outcome.result_json else {
            errors.push(format!("compare: run {} result JSON missing", idx + 1));
            continue;
        };
        normalized.push(normalize_json(json.clone(), &ignore));
    }
    if errors.is_empty() {
        let first = &normalized[0];
        for (idx, value) in normalized.iter().enumerate().skip(1) {
            if value != first {
                errors.push(format!(
                    "compare: normalized_json_equal mismatch (run 1 vs run {}) (see problems/.out/{}/run-*/normalized.json)",
                    idx + 1,
                    problem_id
                ));
                break;
            }
        }
    }
    errors
}

fn check_matches(expect: &CheckExpect, actual: &JsonValue) -> bool {
    let Some(obj) = actual.as_object() else {
        return false;
    };
    if let Some(name) = &expect.name {
        let actual_name = obj.get("name").and_then(|v| v.as_str()).unwrap_or("");
        if actual_name.is_empty() || !match_string(actual_name, name) {
            return false;
        }
    }
    if let Some(target) = &expect.target {
        let actual_target = obj.get("target").and_then(|v| v.as_str()).unwrap_or("");
        if actual_target.is_empty() || !match_string(actual_target, target) {
            return false;
        }
    }
    if let Some(model) = &expect.model {
        let actual_model = obj.get("model").and_then(|v| v.as_str()).unwrap_or("");
        if actual_model.is_empty() || !match_string(actual_model, model) {
            return false;
        }
    }
    if let Some(status) = &expect.status {
        let actual_status = obj.get("status").and_then(|v| v.as_str()).unwrap_or("");
        if actual_status.is_empty() || !match_string(actual_status, status) {
            return false;
        }
    }
    if let Some(reason) = &expect.reason {
        let Some(reason_obj) = obj.get("reason").and_then(|v| v.as_object()) else {
            return false;
        };
        if let Some(kind) = &reason.kind {
            let actual_kind = reason_obj.get("kind").and_then(|v| v.as_str()).unwrap_or("");
            if actual_kind.is_empty() || !match_string(actual_kind, kind) {
                return false;
            }
        }
        if let Some(message) = &reason.message {
            let actual_message = reason_obj
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if actual_message.is_empty() || !match_string(actual_message, message) {
                return false;
            }
        }
    }
    if let Some(counterexample) = &expect.counterexample {
        if !counterexample_matches(counterexample, obj.get("counterexample")) {
            return false;
        }
    }
    if let Some(stats) = &expect.stats {
        if !stats_matches(stats, obj.get("stats")) {
            return false;
        }
    }
    true
}

fn describe_check_expect(expect: &CheckExpect) -> String {
    if let Some(name) = &expect.name {
        return format!("name={}", name);
    }
    if let Some(target) = &expect.target {
        return format!("target={}", target);
    }
    if let Some(model) = &expect.model {
        return format!("model={}", model);
    }
    if let Some(status) = &expect.status {
        return format!("status={}", status);
    }
    "unspecified".to_string()
}

fn counterexample_matches(expect: &CounterexampleExpect, actual: Option<&JsonValue>) -> bool {
    let is_null = actual.map(|v| v.is_null()).unwrap_or(true);
    if let Some(present) = expect.present {
        if present && is_null {
            return false;
        }
        if !present && !is_null {
            return false;
        }
    }
    if expect.trace_len.is_none()
        && expect.tags.is_none()
        && expect.is_minimized.is_none()
        && expect.source_spans.is_none()
    {
        return true;
    }
    let Some(actual) = actual else {
        return false;
    };
    let Some(obj) = actual.as_object() else {
        return false;
    };
    if let Some(trace_len) = &expect.trace_len {
        let len = obj
            .get("events")
            .and_then(|v| v.as_array())
            .map(|v| v.len() as i64)
            .unwrap_or(-1);
        if len < 0 || !match_int(len, trace_len) {
            return false;
        }
    }
    if let Some(tags) = &expect.tags {
        let actual_tags: Vec<String> = obj
            .get("tags")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        if !tags_matches(tags, &actual_tags) {
            return false;
        }
    }
    if let Some(is_minimized) = &expect.is_minimized {
        let actual_minimized = obj
            .get("is_minimized")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if !match_bool(actual_minimized, is_minimized) {
            return false;
        }
    }
    if let Some(spans) = &expect.source_spans {
        let actual_spans = obj.get("source_spans").and_then(|v| v.as_array());
        if !spans_matches(spans, actual_spans) {
            return false;
        }
    }
    true
}

fn stats_matches(expect: &StatsExpect, actual: Option<&JsonValue>) -> bool {
    let Some(actual) = actual else {
        return false;
    };
    let Some(obj) = actual.as_object() else {
        return false;
    };
    if let Some(states) = &expect.states {
        let actual_states = obj
            .get("states")
            .and_then(|v| v.as_i64())
            .unwrap_or(-1);
        if actual_states < 0 || !match_int(actual_states, states) {
            return false;
        }
    }
    if let Some(transitions) = &expect.transitions {
        let actual_transitions = obj
            .get("transitions")
            .and_then(|v| v.as_i64())
            .unwrap_or(-1);
        if actual_transitions < 0 || !match_int(actual_transitions, transitions) {
            return false;
        }
    }
    true
}

fn tags_matches(expect: &TagConstraint, actual: &[String]) -> bool {
    if let Some(contains) = &expect.contains {
        if !contains.iter().all(|tag| actual.contains(tag)) {
            return false;
        }
    }
    if let Some(equals) = &expect.equals {
        let mut actual_sorted = actual.to_vec();
        actual_sorted.sort();
        let mut expected_sorted = equals.clone();
        expected_sorted.sort();
        if actual_sorted != expected_sorted {
            return false;
        }
    }
    true
}

fn spans_matches(expect: &SpanMatch, actual: Option<&Vec<JsonValue>>) -> bool {
    let Some(list) = actual else {
        return false;
    };
    let Some(any) = &expect.any else {
        return false;
    };
    for constraint in any {
        for span in list {
            if span_matches(constraint, span) {
                return true;
            }
        }
    }
    false
}

fn span_matches(expect: &SpanConstraint, actual: &JsonValue) -> bool {
    let Some(obj) = actual.as_object() else {
        return false;
    };
    if let Some(path) = &expect.path {
        let actual_path = obj.get("path").and_then(|v| v.as_str()).unwrap_or("");
        if actual_path.is_empty() || !match_string(actual_path, path) {
            return false;
        }
    }
    if let Some(start_line) = &expect.start_line {
        let actual_start = obj
            .get("start_line")
            .and_then(|v| v.as_i64())
            .unwrap_or(-1);
        if actual_start < 0 || !match_int(actual_start, start_line) {
            return false;
        }
    }
    if let Some(start_col) = &expect.start_col {
        let actual_start = obj
            .get("start_col")
            .and_then(|v| v.as_i64())
            .unwrap_or(-1);
        if actual_start < 0 || !match_int(actual_start, start_col) {
            return false;
        }
    }
    if let Some(end_line) = &expect.end_line {
        let actual_end = obj
            .get("end_line")
            .and_then(|v| v.as_i64())
            .unwrap_or(-1);
        if actual_end < 0 || !match_int(actual_end, end_line) {
            return false;
        }
    }
    if let Some(end_col) = &expect.end_col {
        let actual_end = obj
            .get("end_col")
            .and_then(|v| v.as_i64())
            .unwrap_or(-1);
        if actual_end < 0 || !match_int(actual_end, end_col) {
            return false;
        }
    }
    true
}

fn match_int(actual: i64, expect: &JsonValue) -> bool {
    match expect {
        JsonValue::Number(num) => num.as_i64() == Some(actual),
        JsonValue::Object(map) => {
            if let Some(eq) = map.get("eq").and_then(|v| v.as_i64()) {
                if actual != eq {
                    return false;
                }
            }
            if let Some(min) = map.get("min").and_then(|v| v.as_i64()) {
                if actual < min {
                    return false;
                }
            }
            if let Some(max) = map.get("max").and_then(|v| v.as_i64()) {
                if actual > max {
                    return false;
                }
            }
            if let Some(list) = map.get("in").and_then(|v| v.as_array()) {
                let values: Vec<i64> = list.iter().filter_map(|v| v.as_i64()).collect();
                if !values.contains(&actual) {
                    return false;
                }
            }
            true
        }
        _ => false,
    }
}

fn match_bool(actual: bool, expect: &JsonValue) -> bool {
    match expect {
        JsonValue::Bool(value) => actual == *value,
        JsonValue::Object(map) => {
            if let Some(eq) = map.get("eq").and_then(|v| v.as_bool()) {
                return actual == eq;
            }
            map.is_empty()
        }
        _ => false,
    }
}

fn match_string(actual: &str, expect: &JsonValue) -> bool {
    match expect {
        JsonValue::String(value) => actual == value,
        JsonValue::Object(map) => {
            if let Some(eq) = map.get("eq").and_then(|v| v.as_str()) {
                if actual != eq {
                    return false;
                }
            }
            if let Some(contains) = map.get("contains").and_then(|v| v.as_str()) {
                if !actual.contains(contains) {
                    return false;
                }
            }
            if let Some(regex) = map.get("regex").and_then(|v| v.as_str()) {
                let Ok(pattern) = Regex::new(regex) else {
                    eprintln!("invalid regex in expect constraint: {}", regex);
                    return false;
                };
                if !pattern.is_match(actual) {
                    return false;
                }
            }
            if let Some(list) = map.get("in").and_then(|v| v.as_array()) {
                let values: Vec<&str> = list.iter().filter_map(|v| v.as_str()).collect();
                if !values.contains(&actual) {
                    return false;
                }
            }
            true
        }
        _ => false,
    }
}

fn normalize_json(mut value: JsonValue, extra_ignore: &[String]) -> JsonValue {
    let mut ignore = vec![
        "started_at".to_string(),
        "finished_at".to_string(),
        "duration_ms".to_string(),
        "tool.git_sha".to_string(),
    ];
    ignore.extend(extra_ignore.iter().cloned());
    for path in ignore {
        remove_path(&mut value, &path);
    }
    value
}

fn remove_path(value: &mut JsonValue, path: &str) {
    let parts: Vec<&str> = path.split('.').collect();
    remove_path_recursive(value, &parts);
}

fn remove_path_recursive(value: &mut JsonValue, parts: &[&str]) {
    match value {
        JsonValue::Object(map) => {
            if let Some((first, rest)) = parts.split_first() {
                if rest.is_empty() {
                    map.remove(*first);
                } else if let Some(next) = map.get_mut(*first) {
                    remove_path_recursive(next, rest);
                }
            }
            for child in map.values_mut() {
                remove_path_recursive(child, parts);
            }
        }
        JsonValue::Array(items) => {
            for child in items {
                remove_path_recursive(child, parts);
            }
        }
        _ => {}
    }
}

fn run_problem(
    out_root: &Path,
    problem: &Problem,
    args: &Args,
    expect_schema: &JSONSchema,
) -> Result<ProblemResult> {
    let expect = load_expect(problem, expect_schema)?;
    let run = &problem.spec.run;
    let repeat = expect
        .repeat
        .or(run.repeat)
        .unwrap_or(1);
    if expect.repeat.is_some() && run.repeat.is_some() && expect.repeat != run.repeat {
        eprintln!(
            "warning: repeat override (problem {}, expect={}, run={})",
            problem.spec.id,
            expect.repeat.unwrap(),
            run.repeat.unwrap()
        );
    }
    let compare_ignore = expect
        .compare
        .as_ref()
        .and_then(|compare| compare.ignore_fields.clone())
        .unwrap_or_default();

    let mut outcomes = Vec::new();
    for idx in 1..=repeat {
        let out_dir = out_root.join(&problem.spec.id).join(format!("run-{}", idx));
        fs::create_dir_all(&out_dir)
            .with_context(|| format!("create {}", out_dir.display()))?;

        let outcome = execute_run(problem, args)?;
        fs::write(out_dir.join("stdout.txt"), &outcome.stdout)?;
        fs::write(out_dir.join("stderr.txt"), &outcome.stderr)?;
        fs::write(out_dir.join("exit_code.txt"), outcome.exit_code.to_string())?;
        if let Some(result_json) = &outcome.result_json {
            fs::write(
                out_dir.join("result.json"),
                serde_json::to_vec_pretty(result_json)?,
            )?;
            let normalized = normalize_json(result_json.clone(), &compare_ignore);
            fs::write(
                out_dir.join("normalized.json"),
                serde_json::to_vec_pretty(&normalized)?,
            )?;
        }
        let status = outcome.result_status.as_deref().unwrap_or("unknown");
        println!(
            "DONE {} run={} exit={} status={}",
            problem.spec.id, idx, outcome.exit_code, status
        );
        outcomes.push(outcome);
    }

    let mut errors = Vec::new();
    for (idx, outcome) in outcomes.iter().enumerate() {
        errors.extend(evaluate_run(&expect, outcome, idx + 1));
    }
    errors.extend(evaluate_compare(&expect, &outcomes, &problem.spec.id));

    let report_path = out_root
        .join(&problem.spec.id)
        .join("report.txt");
    if errors.is_empty() {
        fs::write(&report_path, "PASS\n")?;
        Ok(ProblemResult::Pass)
    } else {
        let body = errors.join("\n");
        fs::write(&report_path, format!("FAIL\n{body}\n"))?;
        println!("FAIL {}", problem.spec.id);
        Ok(ProblemResult::Fail)
    }
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

    let mut child = command.spawn().with_context(|| {
        format!(
            "spawn command: {} (cwd={})",
            cmd.join(" "),
            cwd.display()
        )
    })?;
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
