//! Documentation images use deliberately synthetic example data, never private local reports.
use rust_cpu::{export::buffer_svg, model::*, ui};
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
    ] {
        let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(132, 40))?;
        let mut state = UiState {
            active_tab: tab,
            status: "ANTEPRIMA · DATI DIMOSTRATIVI · nessun dato personale".into(),
            ..Default::default()
        };
        terminal.draw(|f| ui::render(f, &inventory, &snapshot, &llm, &mut state, &history))?;
        std::fs::write(
            format!("{directory}/{name}.svg"),
            buffer_svg(terminal.backend().buffer()),
        )?;
    }
    Ok(())
}
