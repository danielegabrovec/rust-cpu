use anyhow::{Context, Result, bail};
use clap::Parser;
use rust_cpu::{
    app,
    collector::{self, Collector},
    export::{self, Recorder},
    llm::{self, Endpoints},
    model::*,
    ui,
};
use std::{
    io::IsTerminal,
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
    thread,
    time::Duration,
};

#[derive(Parser, Debug)]
#[command(
    version,
    about = "RUST-CPU | Hardware insight & Local AI Workbench",
    long_about = "TUI Rust per inventario hardware e monitoraggio LLM locali. Nessuna richiesta di inferenza senza comando esplicito.\nAvvio: rust-cpu | Guida: ? | Snapshot senza TUI: rust-cpu --snapshot report.json"
)]
struct Args {
    #[arg(
        long,
        value_name = "FILE",
        help = "Salva inventario e campione reale in JSON, senza TUI"
    )]
    snapshot: Option<PathBuf>,
    #[arg(
        long,
        value_name = "FILE",
        help = "Registra N campioni CSV senza TUI (vedi --samples)"
    )]
    record: Option<PathBuf>,
    #[arg(long,default_value_t=1,value_parser=clap::value_parser!(u32).range(1..=86400))]
    samples: u32,
    #[arg(long,default_value_t=1000,value_parser=clap::value_parser!(u64).range(500..=10000))]
    interval_ms: u64,
    #[arg(
        long,
        conflicts_with = "benchmark_runtime",
        help = "Non interroga i server LLM locali"
    )]
    no_llm: bool,
    #[arg(long, default_value = "http://127.0.0.1:11434")]
    ollama_url: String,
    #[arg(long, default_value = "http://127.0.0.1:1234")]
    lm_studio_url: String,
    #[arg(long, default_value = "http://127.0.0.1:8080")]
    llama_cpp_url: String,
    #[arg(
        long,
        value_name = "FILE",
        help = "Renderizza la TUI con dati reali in testo UTF-8 o SVG (.svg), senza terminale"
    )]
    render: Option<PathBuf>,
    #[arg(long,default_value_t=130,value_parser=clap::value_parser!(u16).range(40..=400))]
    width: u16,
    #[arg(long,default_value_t=40,value_parser=clap::value_parser!(u16).range(12..=150))]
    height: u16,
    #[arg(long,default_value_t=1,value_parser=clap::value_parser!(u8).range(1..=10),help="Scheda da renderizzare (1-10)")]
    tab: u8,
    #[arg(
        long,
        value_name = "DIR",
        help = "Cartella report interattivi (default: Documenti/RUST-CPU/exports)"
    )]
    export_dir: Option<PathBuf>,
    #[arg(long,value_parser=["ollama","lm-studio","llama.cpp"],requires="benchmark_model",help="Esegue una richiesta di benchmark esplicita senza TUI")]
    benchmark_runtime: Option<String>,
    #[arg(long, requires = "benchmark_runtime")]
    benchmark_model: Option<String>,
    #[arg(
        long,
        help = "Esegue il benchmark CPU mix64 v1 singolo/multi thread (~1,2 s di carico)"
    )]
    cpu_benchmark: bool,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("RUST-CPU: {error:#}");
        std::process::exit(1);
    }
}
fn run() -> Result<()> {
    let args = Args::parse();
    let endpoints = Endpoints {
        ollama: args.ollama_url,
        lm_studio: args.lm_studio_url,
        llama_cpp: args.llama_cpp_url,
    };
    for endpoint in [
        &endpoints.ollama,
        &endpoints.lm_studio,
        &endpoints.llama_cpp,
    ] {
        llm::validate_endpoint(endpoint)?;
    }
    let export_dir = args.export_dir.unwrap_or_else(app::default_export_dir);
    if args.snapshot.is_some()
        || args.record.is_some()
        || args.render.is_some()
        || args.benchmark_runtime.is_some()
        || args.cpu_benchmark
    {
        let inventory = collector::inventory();
        let mut collector = Collector::new();
        let _ = collector.sample();
        thread::sleep(Duration::from_millis(args.interval_ms));
        let mut snapshot = collector.sample();
        let mut llm_state = if args.no_llm {
            LlmState::default()
        } else {
            llm::probe_all(&endpoints)
        };
        let cpu_benchmark = if args.cpu_benchmark {
            let value = rust_cpu::bench::run(Arc::new(AtomicBool::new(false)))?;
            println!("{}", serde_json::to_string_pretty(&value)?);
            Some(value)
        } else {
            None
        };
        if let (Some(runtime), Some(model)) = (&args.benchmark_runtime, &args.benchmark_model) {
            let endpoint = match runtime.as_str() {
                "ollama" => &endpoints.ollama,
                "lm-studio" => &endpoints.lm_studio,
                _ => &endpoints.llama_cpp,
            };
            let result = llm::benchmark(
                runtime,
                endpoint,
                model,
                64,
                Arc::new(AtomicBool::new(false)),
            )?;
            println!("{}", serde_json::to_string_pretty(&result)?);
            llm_state.benchmark = Some(result);
        }
        let mut history = History::default();
        history.push(&snapshot);
        if let Some(path) = args.record {
            let mut recorder = Recorder::new(&path)?;
            recorder.append(&snapshot)?;
            for _ in 1..args.samples {
                thread::sleep(Duration::from_millis(args.interval_ms));
                snapshot = collector.sample();
                history.push(&snapshot);
                recorder.append(&snapshot)?;
            }
            println!("CSV: {} ({} campioni)", path.display(), recorder.rows);
        }
        if let Some(path) = args.snapshot {
            export::export_snapshot_with_bench(
                &path,
                &inventory,
                &snapshot,
                &llm_state,
                cpu_benchmark.as_ref(),
            )?;
            println!("JSON: {}", path.display());
        }
        if let Some(path) = args.render {
            let backend = ratatui::backend::TestBackend::new(args.width, args.height);
            let mut terminal = ratatui::Terminal::new(backend)?;
            let mut state = UiState {
                active_tab: usize::from(args.tab - 1),
                tick_ms: args.interval_ms,
                cpu_benchmark,
                ..Default::default()
            };
            terminal.draw(|frame| {
                ui::render(
                    frame, &inventory, &snapshot, &llm_state, &mut state, &history,
                )
            })?;
            let buffer = terminal.backend().buffer();
            let mut content = String::new();
            if path
                .extension()
                .is_some_and(|v| v.eq_ignore_ascii_case("svg"))
            {
                content = export::buffer_svg(buffer);
            } else {
                for y in 0..args.height {
                    for x in 0..args.width {
                        content.push_str(buffer[(x, y)].symbol());
                    }
                    content.push('\n');
                }
            }
            if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
                std::fs::create_dir_all(parent)?;
            }
            use std::io::Write;
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .context("Il file di rendering deve essere nuovo")?;
            f.write_all(content.as_bytes())?;
            println!("TUI: {}", path.display());
        }
        return Ok(());
    }
    if !std::io::stdout().is_terminal() || !std::io::stdin().is_terminal() {
        bail!(
            "Serve un terminale interattivo. Usa Windows Terminal oppure --snapshot report.json / --render tui.txt."
        );
    }
    app::run(endpoints, export_dir, args.interval_ms, args.no_llm)
}
