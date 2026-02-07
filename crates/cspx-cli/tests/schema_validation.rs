use assert_cmd::cargo::cargo_bin_cmd;
use jsonschema::JSONSchema;
use serde_json::Value;
use std::fs;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

fn load_schema() -> JSONSchema {
    let schema_path = repo_root().join("schemas").join("cspx-result.schema.json");
    let schema_text = fs::read_to_string(schema_path).expect("read schema");
    let schema_json: Value = serde_json::from_str(&schema_text).expect("parse schema");
    JSONSchema::compile(&schema_json).expect("compile schema")
}

fn run_json(args: &[&str]) -> Value {
    let root = repo_root();
    let output = cargo_bin_cmd!("cspx")
        .current_dir(&root)
        .args(args)
        .output()
        .expect("run cspx");
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    serde_json::from_str(&stdout).expect("parse json")
}

#[test]
fn schema_typecheck() {
    let schema = load_schema();
    let actual = run_json(&["typecheck", "tests/cases/ok.cspm", "--format", "json"]);
    let result = schema.validate(&actual);
    assert!(result.is_ok());
}

#[test]
fn schema_check_assert() {
    let schema = load_schema();
    let actual = run_json(&[
        "check",
        "--assert",
        "deadlock free",
        "tests/cases/ok.cspm",
        "--format",
        "json",
    ]);
    let result = schema.validate(&actual);
    assert!(result.is_ok());
}

#[test]
fn schema_check_all_assertions() {
    let schema = load_schema();
    let actual = run_json(&[
        "check",
        "--all-assertions",
        "tests/cases/all_assertions.cspm",
        "--format",
        "json",
    ]);
    let result = schema.validate(&actual);
    assert!(result.is_ok());
}

#[test]
fn schema_refine() {
    let schema = load_schema();
    let actual = run_json(&[
        "refine",
        "--model",
        "FD",
        "tests/cases/spec.cspm",
        "tests/cases/impl.cspm",
        "--format",
        "json",
    ]);
    let result = schema.validate(&actual);
    assert!(result.is_ok());
}

#[test]
fn schema_accepts_legacy_payload_without_metrics() {
    let schema = load_schema();
    let mut actual = run_json(&["typecheck", "tests/cases/ok.cspm", "--format", "json"]);
    actual
        .as_object_mut()
        .expect("result should be object")
        .remove("metrics");
    let result = schema.validate(&actual);
    assert!(result.is_ok());
}
