use assert_cmd::cargo::cargo_bin_cmd;
use predicates::str::contains;
use serde_json::Value;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

#[test]
fn deterministic_requires_seed() {
    let root = repo_root();
    cargo_bin_cmd!("cspx")
        .current_dir(&root)
        .args([
            "typecheck",
            "tests/cases/ok.cspm",
            "--parallel",
            "2",
            "--deterministic",
            "--format",
            "json",
        ])
        .assert()
        .code(2)
        .stderr(contains("--deterministic requires --seed <n>"));
}

#[test]
fn parallel_must_be_at_least_one() {
    let root = repo_root();
    cargo_bin_cmd!("cspx")
        .current_dir(&root)
        .args([
            "typecheck",
            "tests/cases/ok.cspm",
            "--parallel",
            "0",
            "--format",
            "json",
        ])
        .assert()
        .code(2)
        .stderr(contains("--parallel must be >= 1"));
}

#[test]
fn typecheck_parallel_options_are_recorded_in_invocation() {
    let root = repo_root();
    let output = cargo_bin_cmd!("cspx")
        .current_dir(&root)
        .args([
            "typecheck",
            "tests/cases/ok.cspm",
            "--parallel",
            "2",
            "--deterministic",
            "--seed",
            "7",
            "--format",
            "json",
        ])
        .output()
        .expect("run cspx");
    assert!(output.status.success());

    let json: Value = serde_json::from_slice(&output.stdout).expect("parse json");
    assert_eq!(json["invocation"]["parallel"], Value::from(2));
    assert_eq!(json["invocation"]["deterministic"], Value::from(true));
    assert_eq!(json["invocation"]["seed"], Value::from(7));
}
