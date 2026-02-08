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
    assert_eq!(json["metrics"]["parallelism"]["threads"], Value::from(2));
    assert_eq!(
        json["metrics"]["parallelism"]["deterministic"],
        Value::from(true)
    );
    assert_eq!(json["metrics"]["parallelism"]["seed"], Value::from(7));
}

#[test]
fn explore_profile_is_absent_by_default() {
    let root = repo_root();
    let output = cargo_bin_cmd!("cspx")
        .current_dir(&root)
        .args(["typecheck", "tests/cases/ok.cspm", "--format", "json"])
        .output()
        .expect("run cspx");
    assert!(output.status.success());

    let json: Value = serde_json::from_slice(&output.stdout).expect("parse json");
    let metrics = json["metrics"].as_object().expect("metrics");
    assert!(!metrics.contains_key("explore_hotspots"));
}

#[test]
fn explore_profile_on_off_keeps_result_and_stats_consistent() {
    let root = repo_root();
    let run = |with_profile: bool| -> Value {
        let mut args = vec![
            "typecheck",
            "tests/cases/ok.cspm",
            "--parallel",
            "2",
            "--deterministic",
            "--seed",
            "42",
            "--format",
            "json",
        ];
        if with_profile {
            args.push("--explore-profile");
        }
        let output = cargo_bin_cmd!("cspx")
            .current_dir(&root)
            .args(&args)
            .output()
            .expect("run cspx");
        assert!(output.status.success());
        serde_json::from_slice(&output.stdout).expect("parse json")
    };

    let plain = run(false);
    let profiled = run(true);

    assert_eq!(plain["status"], profiled["status"]);
    assert_eq!(plain["checks"][0]["stats"], profiled["checks"][0]["stats"]);
    assert_eq!(
        profiled["metrics"]["explore_hotspots"]["mode"],
        "parallel_deterministic"
    );
    assert_eq!(
        profiled["metrics"]["explore_hotspots"]["workers"],
        Value::from(2)
    );
}
