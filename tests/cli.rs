use std::process::Command;

fn binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_rust-cpu"))
}

#[test]
fn cli_rejects_remote_endpoints_and_conflicting_inference_flags() {
    let output = binary()
        .args([
            "--snapshot",
            "unused.json",
            "--ollama-url",
            "http://example.com:11434",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("loopback"));
    let output = binary()
        .args([
            "--no-llm",
            "--benchmark-runtime",
            "ollama",
            "--benchmark-model",
            "example",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("cannot be used"));
}

#[test]
fn real_headless_snapshot_csv_and_render_work_together() {
    let dir = tempfile::tempdir().unwrap();
    let json = dir.path().join("real.json");
    let csv = dir.path().join("real.csv");
    let svg = dir.path().join("real.svg");
    let output = binary()
        .args([
            "--no-llm",
            "--interval-ms",
            "500",
            "--samples",
            "2",
            "--snapshot",
        ])
        .arg(&json)
        .arg("--record")
        .arg(&csv)
        .arg("--render")
        .arg(&svg)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&std::fs::read(&json).unwrap()).unwrap();
    assert_eq!(report["schema_version"], 1);
    assert!(report["snapshot"]["memory_total"].as_u64().unwrap() > 0);
    assert!(
        !report["snapshot"]["per_core"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        !report["snapshot"]["processes"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!((0.0..=100.0).contains(&report["snapshot"]["cpu_percent"].as_f64().unwrap()));
    assert!(report["llm"]["runtimes"].as_array().unwrap().is_empty());
    let mut rows = csv::Reader::from_path(&csv).unwrap();
    assert_eq!(rows.records().count(), 2);
    let capture = std::fs::read_to_string(&svg).unwrap();
    assert!(capture.contains("<svg"));
    assert!(capture.contains("RUST"));
    let original = std::fs::read(&json).unwrap();
    let duplicate = binary()
        .args(["--no-llm", "--interval-ms", "500", "--snapshot"])
        .arg(&json)
        .output()
        .unwrap();
    assert!(!duplicate.status.success());
    assert_eq!(std::fs::read(&json).unwrap(), original);
}

#[test]
fn help_and_version_do_not_require_a_terminal() {
    let output = binary().arg("--help").output().unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("--cpu-benchmark"));
    let output = binary().arg("--version").output().unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains(env!("CARGO_PKG_VERSION")));
    let output = binary()
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("terminale interattivo"));
}
