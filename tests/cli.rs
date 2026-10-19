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
        .arg("--database")
        .arg(dir.path().join("archive.sqlite3"))
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
        .arg("--database")
        .arg(dir.path().join("archive.sqlite3"))
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

#[test]
fn sqlite_sessions_reload_compare_export_import_and_backup_end_to_end() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("workspace.sqlite3");
    for name in ["Baseline d'apertura", "Seconda sessione"] {
        let out = binary()
            .arg("--database")
            .arg(&db)
            .args([
                "--no-llm",
                "--save-session",
                name,
                "--samples",
                "3",
                "--interval-ms",
                "500",
            ])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let out = binary()
        .arg("--database")
        .arg(&db)
        .arg("--sessions")
        .output()
        .unwrap();
    assert!(out.status.success());
    let sessions: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(sessions.as_array().unwrap().len(), 2);
    assert_eq!(sessions[0]["samples"], 3);
    let comparison = dir.path().join("comparison.json");
    let chart = dir.path().join("comparison.svg");
    let out = binary()
        .arg("--database")
        .arg(&db)
        .args(["--compare", "1", "2", "--comparison-output"])
        .arg(&comparison)
        .arg("--render")
        .arg(&chart)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let c: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&comparison).unwrap()).unwrap();
    assert_eq!(c["left"]["id"], 1);
    assert_eq!(c["right"]["id"], 2);
    assert!(!c["metrics"].as_array().unwrap().is_empty());
    assert!(std::fs::read_to_string(&chart).unwrap().contains("<svg"));
    let export = dir.path().join("export.json");
    let replay = dir.path().join("replay.txt");
    let out = binary()
        .arg("--database")
        .arg(&db)
        .args(["--load-session", "1", "--export-session"])
        .arg(&export)
        .arg("--render")
        .arg(&replay)
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(std::fs::read_to_string(&replay).unwrap().contains("REPLAY"));
    let out = binary()
        .arg("--database")
        .arg(&db)
        .arg("--import-session")
        .arg(&export)
        .output()
        .unwrap();
    assert!(out.status.success());
    let out = binary()
        .arg("--database")
        .arg(&db)
        .args([
            "--rename-session",
            "3",
            "--session-name",
            "Import rinominato",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    let backup = dir.path().join("backup.sqlite3");
    let out = binary()
        .arg("--database")
        .arg(&db)
        .arg("--backup-db")
        .arg(&backup)
        .output()
        .unwrap();
    assert!(out.status.success());
    let out = binary()
        .arg("--database")
        .arg(&backup)
        .arg("--db-check")
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("ok"));
    let store = rust_cpu::storage::Store::open(&backup).unwrap();
    let sessions = store.list_sessions(10).unwrap();
    assert_eq!(sessions.len(), 3);
    assert_eq!(sessions[0].name, "Import rinominato");
    assert_eq!(sessions[2].samples, 3);
}

#[test]
fn imports_v010_snapshot_and_rejects_incompatible_operations() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("workspace.sqlite3");
    let old = dir.path().join("v010.json");
    let snapshot = serde_json::json!({"schema_version":1,"app_version":"0.1.0","inventory":{},"snapshot":{"timestamp_unix":1,"elapsed_secs":0.0,"sampling_ms":0,"cpu_percent":0.0,"per_core":[],"frequency_mhz":0,"memory_used":0,"memory_total":0,"swap_used":0,"swap_total":0,"processes":[],"disks":[],"networks":[],"gpus":[],"sensors":[],"warnings":[]}});
    // Inventory0.1 alwayshad fullfields; new optional GPU/source fields are deliberately absent.
    let mut snapshot = snapshot;
    snapshot["inventory"] = serde_json::to_value(rust_cpu::model::Inventory::default()).unwrap();
    std::fs::write(&old, serde_json::to_vec(&snapshot).unwrap()).unwrap();
    let out = binary()
        .arg("--database")
        .arg(&db)
        .arg("--import-session")
        .arg(&old)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = binary()
        .arg("--database")
        .arg(&db)
        .args(["--compare", "1", "1"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let out = binary()
        .arg("--database")
        .arg(&db)
        .args(["--sessions", "--cpu-benchmark"])
        .output()
        .unwrap();
    assert!(!out.status.success());
}

#[test]
fn repeated_benchmark_messages_do_not_starve_resource_sampling() {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        thread,
        time::Duration,
    };
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("bench.sqlite3");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let server = thread::spawn(move || {
        while !flag.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream
                        .set_read_timeout(Some(Duration::from_secs(2)))
                        .unwrap();
                    let mut request = Vec::new();
                    let mut buf = [0; 2048];
                    let mut end = None;
                    loop {
                        let n = stream.read(&mut buf).unwrap_or(0);
                        if n == 0 {
                            break;
                        }
                        request.extend_from_slice(&buf[..n]);
                        if let Some(index) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                            end = Some(index + 4);
                            break;
                        }
                    }
                    if let Some(end) = end {
                        let headers = String::from_utf8_lossy(&request[..end]);
                        let size = headers
                            .lines()
                            .find_map(|line| {
                                line.to_lowercase()
                                    .strip_prefix("content-length:")
                                    .and_then(|v| v.trim().parse::<usize>().ok())
                            })
                            .unwrap_or(0);
                        let is_generate = headers.starts_with("POST /api/generate");
                        let is_tags = headers.starts_with("GET /api/tags");
                        while request.len() < end + size {
                            let n = stream.read(&mut buf).unwrap_or(0);
                            if n == 0 {
                                break;
                            }
                            request.extend_from_slice(&buf[..n]);
                        }
                        let body = if is_generate {
                            thread::sleep(Duration::from_millis(350));
                            r#"{"model":"local-test","response":"ok","done":true,"eval_count":1,"eval_duration":1000000}"#
                        } else if is_tags {
                            r#"{"models":[{"name":"local-test","size":1000,"details":{"family":"llama"}}]}"#
                        } else {
                            r#"{"models":[]}"#
                        };
                        let response = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            body.len(),
                            body
                        );
                        let _ = stream.write_all(response.as_bytes());
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(5))
                }
                Err(_) => break,
            }
        }
    });
    let out = binary()
        .arg("--database")
        .arg(&db)
        .args([
            "--benchmark-runtime",
            "ollama",
            "--benchmark-model",
            "local-test",
            "--benchmark-runs",
            "3",
            "--interval-ms",
            "500",
            "--ollama-url",
        ])
        .arg(format!("http://{address}"))
        .args([
            "--lm-studio-url",
            "http://127.0.0.1:1",
            "--llama-cpp-url",
            "http://127.0.0.1:1",
        ])
        .output()
        .unwrap();
    stop.store(true, Ordering::Relaxed);
    server.join().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let store = rust_cpu::storage::Store::open(&db).unwrap();
    let session = &store.list_sessions(1).unwrap()[0];
    assert_eq!(session.benchmark_count, 3);
    assert!(
        session.samples >= 3,
        "Periodic resource samples were starved: {}",
        session.samples
    );
}
