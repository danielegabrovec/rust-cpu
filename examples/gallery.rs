//! Documentation images use deliberately synthetic example data, never private local reports.
use rust_cpu::{
    bench::CpuBenchmark,
    export::buffer_svg,
    model::*,
    settings::AlertEvent,
    storage::{
        MetricSummary, SavedBenchmark, SessionComparison, SessionData, SessionSummary,
        compare_benchmarks, comparison_metric,
    },
    ui,
};

fn metric(values: impl Iterator<Item = f64>) -> MetricSummary {
    let mut values = values.filter(|value| value.is_finite()).collect::<Vec<_>>();
    if values.is_empty() {
        return MetricSummary::default();
    }
    let average = values.iter().sum::<f64>() / values.len() as f64;
    values.sort_by(f64::total_cmp);
    let p95 = values[(values.len() * 95).div_ceil(100).saturating_sub(1)];
    MetricSummary {
        average: Some(average),
        peak: values.last().copied(),
        p95: Some(p95),
    }
}

/// Every value here is a documentation fixture. This function never opens a
/// user's database, interrogates hardware, or contacts an inference endpoint.
fn demo_session(
    id: i64,
    name: &str,
    inventory: &Inventory,
    snapshot: &Snapshot,
    candidate: bool,
) -> SessionData {
    let samples = (0..120)
        .map(|index| {
            let phase = index as f64 / 12.0;
            let mut sample = snapshot.clone();
            sample.elapsed_secs = index as f64 * 2.0;
            sample.timestamp_unix = 1792396800 + index * 2;
            sample.cpu_percent = (if candidate { 30.0 } else { 42.0 } + phase.sin() * 11.0) as f32;
            sample.memory_used = if candidate { 18 << 30 } else { 23 << 30 }
                + (phase.cos().abs() * (1u64 << 30) as f64) as u64;
            if let Some(gpu) = sample.gpus.first_mut() {
                gpu.utilization =
                    Some((if candidate { 66.0 } else { 76.0 } + phase.sin() * 10.0) as f32);
                gpu.memory_used = Some(if candidate { 8 << 30 } else { 10 << 30 });
            }
            sample
        })
        .collect::<Vec<_>>();
    let mut benchmarks = Vec::new();
    for index in 0..3 {
        let speed = if candidate {
            [52.0, 53.4, 52.8][index]
        } else {
            [48.2, 49.1, 48.8][index]
        };
        benchmarks.push(SavedBenchmark {
            id: id * 10 + index as i64,
            session_id: Some(id),
            recorded_utc: format!("2026-10-19T08:0{}:00Z", index + 1),
            kind: "llm".into(),
            llm: Some(BenchmarkResult {
                throughput_kind: "native_decode".into(),
                runtime: "Ollama (DEMO)".into(),
                model: "modello-esempio:8b-q4".into(),
                output_tokens: Some(64),
                prompt_tokens: Some(32),
                wall_seconds: 64.0 / speed + 0.4,
                generation_tokens_per_sec: Some(speed),
                prompt_tokens_per_sec: Some(180.0),
                time_to_first_token_ms: Some(if candidate { 180.0 } else { 220.0 }),
                load_ms: Some(0.0),
                note: "DEMO: valori sintetici, metodo decode nativo; nessuna inferenza reale."
                    .into(),
            }),
            ..Default::default()
        });
        benchmarks.push(SavedBenchmark {
            id: id * 10 + 3 + index as i64,
            session_id: Some(id),
            recorded_utc: format!("2026-10-19T08:0{}:10Z", index + 1),
            kind: "cpu".into(),
            cpu: Some(CpuBenchmark {
                build_profile: "optimized".into(),
                single_ops_per_sec: 100_000_000.0 + index as f64 * 1_000_000.0,
                multi_ops_per_sec: 800_000_000.0 + index as f64 * 8_000_000.0,
                threads: 16,
                duration_secs: 1.2,
                algorithm: "DEMO mix64 v1 · dati sintetici".into(),
            }),
            ..Default::default()
        });
    }
    let summary = SessionSummary {
        app_version: env!("CARGO_PKG_VERSION").into(),
        id,
        name: name.into(),
        started_utc: "2026-10-19T08:00:00Z".into(),
        ended_utc: Some("2026-10-19T08:04:00Z".into()),
        state: "complete".into(),
        samples: samples.len() as u64,
        duration_secs: 238.0,
        cpu: metric(samples.iter().map(|s| s.cpu_percent as f64)),
        ram: metric(samples.iter().map(|s| s.memory_used as f64)),
        gpu: metric(
            samples
                .iter()
                .filter_map(|s| s.gpus.first()?.utilization.map(f64::from)),
        ),
        vram: metric(
            samples
                .iter()
                .filter_map(|s| s.gpus.first()?.memory_used.map(|v| v as f64)),
        ),
        llm_ram: MetricSummary {
            average: Some((11u64 << 30) as f64),
            peak: Some((11u64 << 30) as f64),
            p95: Some((11u64 << 30) as f64),
        },
        benchmark_count: benchmarks.len() as u64,
    };
    SessionData {
        summary,
        inventory: inventory.clone(),
        total_samples: samples.len() as u64,
        samples,
        benchmarks,
        alerts: Vec::new(),
    }
}

fn demo_workspace(inventory: &Inventory, snapshot: &Snapshot) -> rust_cpu::workspace::WorkspaceUi {
    let left = demo_session(
        101,
        "DEMO · 8B Q4 · riferimento A",
        inventory,
        snapshot,
        false,
    );
    let right = demo_session(102, "DEMO · 8B Q4 · candidata B", inventory, snapshot, true);
    let metrics = [
        (
            "CPU media",
            "%",
            left.summary.cpu.average,
            right.summary.cpu.average,
        ),
        (
            "CPU picco",
            "%",
            left.summary.cpu.peak,
            right.summary.cpu.peak,
        ),
        (
            "RAM media",
            "B",
            left.summary.ram.average,
            right.summary.ram.average,
        ),
        (
            "RAM picco",
            "B",
            left.summary.ram.peak,
            right.summary.ram.peak,
        ),
        (
            "GPU media",
            "%",
            left.summary.gpu.average,
            right.summary.gpu.average,
        ),
        (
            "VRAM picco",
            "B",
            left.summary.vram.peak,
            right.summary.vram.peak,
        ),
    ]
    .into_iter()
    .map(|(label, unit, a, b)| comparison_metric(label, unit, a, b, true))
    .collect();
    let comparison=SessionComparison{left:left.summary.clone(),right:right.summary.clone(),metrics,warnings:vec!["DEMO: confronto fra serie sintetiche. Non rappresenta il PC dell'autore né un guadagno prestazionale verificato.".into(),"Minore utilizzo non dimostra lavoro più veloce: controllare stesso carico, modello, quantizzazione, contesto, cache e offload.".into()],left_series:left.samples.clone(),right_series:right.samples.clone(),left_benchmarks:left.benchmarks.clone(),right_benchmarks:right.benchmarks.clone(),benchmarks:compare_benchmarks(&left.benchmarks,&right.benchmarks)};
    let mut workspace = rust_cpu::workspace::WorkspaceUi {
        sessions: vec![
            right.summary.clone(),
            left.summary.clone(),
            SessionSummary {
                id: 100,
                name: "DEMO · controllo inventario e sensori".into(),
                started_utc: "2026-10-18T15:00:00Z".into(),
                ended_utc: Some("2026-10-18T15:03:00Z".into()),
                state: "complete".into(),
                samples: 90,
                duration_secs: 178.0,
                benchmark_count: 0,
                ..left.summary.clone()
            },
        ],
        comparison: Some(comparison),
        baseline_id: Some(101),
        candidate_id: Some(102),
        db_path: "DEMO / archivio-locale / workspace.sqlite3".into(),
        benchmark_history: right.benchmarks.clone(),
        alerts: vec![AlertEvent {
            timestamp_unix: 1792395000,
            kind: "ram".into(),
            message: "DEMO · RAM 92%: soglia90% superata in una prova sintetica precedente".into(),
            value: 92.0,
            threshold: 90.0,
        }],
        ..Default::default()
    };
    workspace.preferences.benchmark_repeats = 3;
    workspace
}
fn main() -> anyhow::Result<()> {
    let directory = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "docs/screenshots".into());
    std::fs::create_dir_all(&directory)?;
    let inventory = Inventory {
        os: "Windows x64 · dati dimostrativi".into(),
        cpu_name: "CPU esempio · 16 core / 32 thread".into(),
        physical_cores: 16,
        logical_cores: 32,
        cpu: vec![
            Detail::new("Architettura", "x86_64"),
            Detail::new("Istruzioni", "SSE4.2 · AVX2 · FMA · AES-NI"),
            Detail::new("Cache L3 (esempio)", "32 MiB / istanza"),
        ],
        ..Default::default()
    };
    let snapshot = Snapshot {
        timestamp_unix: 1792397040,
        cpu_percent: 36.8,
        per_core: (0..32).map(|i| ((i * 13 + 17) % 95) as f32).collect(),
        frequency_mhz: 4200,
        memory_used: 23 << 30,
        memory_total: 64 << 30,
        swap_used: 0,
        swap_total: 16 << 30,
        sampling_ms: 18,
        elapsed_secs: 240.0,
        gpus: vec![GpuRow {
            source: "Dati dimostrativi".into(),
            shared_memory_used: Some(0),
            name: "GPU esempio · 16 GiB".into(),
            utilization: Some(74.0),
            memory_used: Some(10 << 30),
            memory_total: Some(16 << 30),
            temperature: Some(62.0),
            power_watts: Some(145.0),
        }],
        processes: vec![
            ProcessRow {
                pid: 8400,
                name: "ollama.exe".into(),
                cpu_percent: 14.2,
                memory_bytes: 8 << 30,
                is_llm: true,
                ..Default::default()
            },
            ProcessRow {
                pid: 9120,
                name: "llama-server.exe".into(),
                cpu_percent: 7.3,
                memory_bytes: 3 << 30,
                is_llm: true,
                ..Default::default()
            },
            ProcessRow {
                pid: 1044,
                name: "rust-cpu.exe".into(),
                cpu_percent: 0.4,
                memory_bytes: 30 << 20,
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    let llm = LlmState {
        runtimes: vec![RuntimeStatus {
            name: "Ollama".into(),
            endpoint: "http://127.0.0.1:11434".into(),
            status: "online".into(),
            message: "Dati dimostrativi".into(),
            models: vec![ModelInfo {
                id: "modello-esempio:8b".into(),
                size_bytes: Some(5 << 30),
                vram_bytes: Some(6 << 30),
                context_length: Some(8192),
                quantization: Some("Q4_K_M".into()),
                loaded: Some(true),
            }],
        }],
        benchmark: Some(BenchmarkResult {
            throughput_kind: "native_decode".into(),
            runtime: "Ollama".into(),
            model: "modello-esempio:8b".into(),
            output_tokens: Some(64),
            wall_seconds: 1.9,
            generation_tokens_per_sec: Some(48.6),
            time_to_first_token_ms: Some(220.0),
            note: "Dati sintetici per documentazione; nessun benchmark reale.".into(),
            ..Default::default()
        }),
        ..Default::default()
    };
    let mut history = History::default();
    for i in 0..130 {
        history.cpu.push_back(((i * 7) % 37 + 20) as u64);
        history.memory.push_back(36 + i as u64 / 32);
        history.gpu.push_back(((i * 11) % 29 + 55) as u64);
    }
    for (tab, name) in [
        (0, "overview"),
        (6, "processi"),
        (7, "llm"),
        (8, "capacity"),
        (10, "sessioni"),
        (11, "confronto"),
        (12, "impostazioni"),
    ] {
        let height = if tab == 11 { 60 } else { 40 };
        let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(132, height))?;
        let mut state = UiState {
            active_tab: tab,
            status: "ANTEPRIMA · DATI DIMOSTRATIVI · nessun dato personale".into(),
            ..Default::default()
        };
        state.workspace = demo_workspace(&inventory, &snapshot);
        if tab == 10 {
            state.selected = 1;
        }
        if tab == 12 {
            state.workspace.settings_field = 4;
        }
        terminal.draw(|f| ui::render(f, &inventory, &snapshot, &llm, &mut state, &history))?;
        std::fs::write(
            format!("{directory}/{name}.svg"),
            buffer_svg(terminal.backend().buffer()),
        )?;
    }
    Ok(())
}
