use assert_cmd::cargo::cargo_bin_cmd;
use jsonschema::JSONSchema;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

fn load_summary_schema() -> JSONSchema {
    let schema_path = repo_root().join("schemas").join("csp-summary.schema.json");
    let schema_text = fs::read_to_string(schema_path).expect("read summary schema");
    let schema_json: Value = serde_json::from_str(&schema_text).expect("parse summary schema");
    JSONSchema::compile(&schema_json).expect("compile summary schema")
}

fn read_json(path: &Path) -> Value {
    let text = fs::read_to_string(path).expect("read json");
    serde_json::from_str(&text).expect("parse json")
}

fn run_with_summary(args: &[&str], expected_exit_code: i32) -> (Value, Value, String) {
    let root = repo_root();
    let temp = TempDir::new().expect("tmp dir");
    let result_path = temp.path().join("result.json");
    let summary_path = temp.path().join("summary.json");

    let mut all_args = args.to_vec();
    all_args.extend([
        "--format",
        "json",
        "--output",
        result_path.to_str().expect("result path utf8"),
        "--summary-json",
        summary_path.to_str().expect("summary path utf8"),
    ]);

    let output = cargo_bin_cmd!("cspx")
        .current_dir(&root)
        .args(&all_args)
        .output()
        .expect("run cspx");

    assert_eq!(output.status.code(), Some(expected_exit_code));

    let summary = read_json(&summary_path);
    let result = read_json(&result_path);
    (summary, result, result_path.to_string_lossy().to_string())
}

#[test]
fn summary_json_for_typecheck_passes_schema_and_contract() {
    let schema = load_summary_schema();
    let (summary, _result, result_path) =
        run_with_summary(&["typecheck", "tests/cases/ok.cspm"], 0);

    assert!(schema.validate(&summary).is_ok());
    assert_eq!(summary["tool"], "csp");
    assert_eq!(summary["backend"], "cspx:typecheck");
    assert_eq!(summary["file"], "tests/cases/ok.cspm");
    assert_eq!(summary["ran"], true);
    assert_eq!(summary["status"], "ran");
    assert_eq!(summary["resultStatus"], "pass");
    assert_eq!(summary["exitCode"], 0);
    assert_eq!(summary["detailsFile"], result_path);
}

#[test]
fn summary_json_maps_fail_status_for_assertions_mode() {
    let schema = load_summary_schema();
    let (summary, _result, _result_path) = run_with_summary(
        &["check", "--assert", "deadlock free", "tests/cases/ok.cspm"],
        1,
    );

    assert!(schema.validate(&summary).is_ok());
    assert_eq!(summary["backend"], "cspx:assertions");
    assert_eq!(summary["status"], "failed");
    assert_eq!(summary["resultStatus"], "fail");
    assert_eq!(summary["exitCode"], 1);
    assert!(summary["output"]
        .as_str()
        .expect("output as str")
        .contains("checks=check:fail"));
}

#[test]
fn summary_json_maps_unsupported_status() {
    let schema = load_summary_schema();
    let (summary, _result, _result_path) = run_with_summary(
        &["typecheck", "problems/P004_unsupported_feature/model.cspm"],
        3,
    );

    assert!(schema.validate(&summary).is_ok());
    assert_eq!(summary["backend"], "cspx:typecheck");
    assert_eq!(summary["status"], "unsupported");
    assert_eq!(summary["resultStatus"], "unsupported");
    assert_eq!(summary["exitCode"], 3);
}
