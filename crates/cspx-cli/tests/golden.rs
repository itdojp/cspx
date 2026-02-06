use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::Value;
use std::fs;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
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

fn load_expected(name: &str) -> Value {
    let path = repo_root().join("tests").join("golden").join(name);
    let text = fs::read_to_string(path).expect("read expected");
    serde_json::from_str(&text).expect("parse expected json")
}

fn strip_volatile_fields(mut value: Value) -> Value {
    if let Value::Object(map) = &mut value {
        map.remove("started_at");
        map.remove("finished_at");
        map.remove("duration_ms");
        if let Some(Value::Object(tool)) = map.get_mut("tool") {
            tool.remove("git_sha");
        }
    }
    value
}

#[test]
fn golden_typecheck() {
    let actual = run_json(&["typecheck", "tests/cases/ok.cspm", "--format", "json"]);
    let expected = load_expected("expected_typecheck.json");
    assert_eq!(
        strip_volatile_fields(actual),
        strip_volatile_fields(expected)
    );
}

#[test]
fn golden_check_assert() {
    let actual = run_json(&[
        "check",
        "--assert",
        "deadlock free",
        "tests/cases/ok.cspm",
        "--format",
        "json",
    ]);
    let expected = load_expected("expected_check_assert.json");
    assert_eq!(
        strip_volatile_fields(actual),
        strip_volatile_fields(expected)
    );
}

#[test]
fn golden_check_all_assertions() {
    let actual = run_json(&[
        "check",
        "--all-assertions",
        "tests/cases/all_assertions.cspm",
        "--format",
        "json",
    ]);
    let expected = load_expected("expected_check_all_assertions.json");
    assert_eq!(
        strip_volatile_fields(actual),
        strip_volatile_fields(expected)
    );
}

#[test]
fn golden_refine() {
    let actual = run_json(&[
        "refine",
        "--model",
        "FD",
        "tests/cases/spec.cspm",
        "tests/cases/impl.cspm",
        "--format",
        "json",
    ]);
    let expected = load_expected("expected_refine.json");
    assert_eq!(
        strip_volatile_fields(actual),
        strip_volatile_fields(expected)
    );
}
