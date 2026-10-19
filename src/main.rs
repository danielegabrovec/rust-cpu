use anyhow::{Context, Result, bail};
use clap::Parser;
use rust_cpu::{
    app,
    collector::{self, Collector},
    export::{self, Recorder},
    llm::{self, Endpoints},
    model::*,
    settings::Preferences,
    storage::{SessionComparison, SessionData, Store},
    ui,
    workspace_runtime::{ArchiveWorker, Command, Reply},
};
use std::{
    io::{IsTerminal, Write},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

#[derive(Parser, Debug)]
#[command(
    version,
    about = "RUST-CPU | Hardware, Local AI & SQLite Session Lab",
    long_about = "TUI Rust con archivio SQLite, sessioni e benchmark confrontabili.\nF2 sessioni | F3 confronto | F4 impostazioni | ? guida.\nEsempio: rust-cpu --save-session baseline --samples 30 --database archivio.sqlite3"
)]
struct Args {
    #[arg(long)]
    snapshot: Option<PathBuf>,
    #[arg(long, help = "CSV e sessione SQLite di N campioni")]
    record: Option<PathBuf>,
    #[arg(long,default_value_t=1,value_parser=clap::value_parser!(u32).range(1..=86400))]
    samples: u32,
    #[arg(long,value_parser=clap::value_parser!(u64).range(500..=10000),help="Intervallo ms; default dalle preferenze salvate")]
    interval_ms: Option<u64>,
    #[arg(long,conflicts_with_all=["benchmark_runtime","observe_runtime"])]
    no_llm: bool,
    #[arg(long)]
    ollama_url: Option<String>,
    #[arg(long)]
    lm_studio_url: Option<String>,
    #[arg(long)]
    llama_cpp_url: Option<String>,
    #[arg(
        long,
        value_name = "FILE",
        help = "Rendering testuale o SVG (.svg), anche di sessioni/confronti salvati"
    )]
    render: Option<PathBuf>,
    #[arg(long,default_value_t=130,value_parser=clap::value_parser!(u16).range(40..=400))]
    width: u16,
    #[arg(long,default_value_t=40,value_parser=clap::value_parser!(u16).range(12..=150))]
    height: u16,
    #[arg(long,default_value_t=1,value_parser=clap::value_parser!(u8).range(1..=13))]
    tab: u8,
    #[arg(long)]
    export_dir: Option<PathBuf>,
    #[arg(long,value_parser=["ollama","lm-studio","llama.cpp"],requires="benchmark_model")]
    benchmark_runtime: Option<String>,
    #[arg(long, requires = "benchmark_runtime")]
    benchmark_model: Option<String>,
    #[arg(long)]
    cpu_benchmark: bool,
    #[arg(long,default_value_t=1,value_parser=clap::value_parser!(u32).range(1..=10))]
    benchmark_runs: u32,
    #[arg(
        long,
        alias = "db",
        value_name = "FILE",
        help = "Archivio SQLite; default LocalAppData/RUST-CPU/workspace.sqlite3"
    )]
    database: Option<PathBuf>,
    #[arg(
        long,
        value_name = "NOME",
        help = "Registra una sessione SQLite con nome, senza TUI"
    )]
    save_session: Option<String>,
    #[arg(long, help = "Elenca sessioni salvate; compatibile con --render")]
    sessions: bool,
    #[arg(long, value_name = "ID")]
    load_session: Option<i64>,
    #[arg(
        long,
        default_value_t = 0,
        help = "Indice nell'anteprima (max2000 campioni) della sessione caricata"
    )]
    sample_index: usize,
    #[arg(long, requires = "load_session", value_name = "FILE")]
    export_session: Option<PathBuf>,
    #[arg(long,num_args=2,value_names=["A","B"],help="Confronta due ID; B − A, statistiche complete")]
    compare: Vec<i64>,
    #[arg(long, requires = "compare", value_name = "FILE")]
    comparison_output: Option<PathBuf>,
    #[arg(long, value_name = "FILE")]
    import_session: Option<PathBuf>,
    #[arg(long, value_name = "FILE")]
    backup_db: Option<PathBuf>,
    #[arg(long)]
    db_check: bool,
    #[arg(long)]
    reset_settings: bool,
    #[arg(long, requires = "session_name", value_name = "ID")]
    rename_session: Option<i64>,
    #[arg(long, requires = "rename_session", value_name = "NOME")]
    session_name: Option<String>,
    #[arg(long,value_parser=["ollama","lm-studio","llama.cpp"],help="Proxy locale facoltativo: osserva SOLO le richieste instradate qui")]
    observe_runtime: Option<String>,
    #[arg(long,default_value_t=18434,value_parser=clap::value_parser!(u16).range(1..))]
    observe_port: u16,
    #[arg(long, requires = "observe_runtime", value_name = "ID")]
    observe_session: Option<i64>,
}
fn main() {
    if let Err(e) = run() {
        eprintln!("RUST-CPU: {e:#}");
        std::process::exit(1);
    }
}
fn default_database() -> PathBuf {
    if let Some(path) = std::env::var_os("RUST_CPU_DATABASE") {
        return path.into();
    }
    if let Some(base) = std::env::var_os("LOCALAPPDATA") {
        return PathBuf::from(base)
            .join("RUST-CPU")
            .join("workspace.sqlite3");
    }
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".local/share")
        });
    base.join("rust-cpu/workspace.sqlite3")
}
fn run() -> Result<()> {
    let args = Args::parse();
    // Reject supplied destinations before touching the database or issuing requests.
    for endpoint in [&args.ollama_url, &args.lm_studio_url, &args.llama_cpp_url]
        .into_iter()
        .flatten()
    {
        llm::validate_endpoint(endpoint)?;
    }
    let management_count = [
        args.sessions,
        args.load_session.is_some(),
        !args.compare.is_empty(),
        args.import_session.is_some(),
        args.backup_db.is_some(),
        args.db_check,
        args.reset_settings,
        args.rename_session.is_some(),
        args.observe_runtime.is_some(),
    ]
    .into_iter()
    .filter(|v| *v)
    .count();
    if management_count > 1 {
        bail!("Scegli una sola operazione archivio/proxy per comando.");
    }
    let benchmark = args.cpu_benchmark || args.benchmark_runtime.is_some();
    if management_count > 0 && (args.record.is_some() || args.save_session.is_some() || benchmark) {
        bail!("Le operazioni archivio/proxy non si combinano con nuove registrazioni o benchmark.");
    }
    if args.cpu_benchmark && args.benchmark_runtime.is_some() {
        bail!("Esegui benchmark CPU e LLM in comandi separati.");
    }
    let headless = management_count > 0
        || args.snapshot.is_some()
        || args.record.is_some()
        || args.render.is_some()
        || args.save_session.is_some()
        || benchmark;
    if !headless && (!std::io::stdin().is_terminal() || !std::io::stdout().is_terminal()) {
        bail!(
            "Serve un terminale interattivo. Usa Windows Terminal oppure --snapshot report.json / --render tui.svg."
        );
    }
    let database = args.database.clone().unwrap_or_else(default_database);
    let mut store = Store::open(&database)?;
    if args.reset_settings {
        store.save_preferences(&Preferences::default())?;
        println!("Preferenze ripristinate; sessioni conservate.");
        return Ok(());
    }
    let mut preferences = store.preferences()?;
    if let Some(interval) = args.interval_ms {
        preferences.interval_ms = interval;
    }
    if let Some(url) = &args.ollama_url {
        preferences.ollama_url = url.clone();
    }
    if let Some(url) = &args.lm_studio_url {
        preferences.lm_studio_url = url.clone();
    }
    if let Some(url) = &args.llama_cpp_url {
        preferences.llama_cpp_url = url.clone();
    }
    preferences.validate();
    let endpoints = Endpoints {
        ollama: preferences.ollama_url.clone(),
        lm_studio: preferences.lm_studio_url.clone(),
        llama_cpp: preferences.llama_cpp_url.clone(),
    };
    for url in [
        &endpoints.ollama,
        &endpoints.lm_studio,
        &endpoints.llama_cpp,
    ] {
        llm::validate_endpoint(url)?;
    }
    if args.interval_ms.is_some()
        || args.ollama_url.is_some()
        || args.lm_studio_url.is_some()
        || args.llama_cpp_url.is_some()
    {
        store.save_preferences(&preferences)?;
    }
    if let Some(path) = &args.backup_db {
        store.backup(path)?;
        println!("Backup SQLite: {}", path.display());
        return Ok(());
    }
    if args.db_check {
        println!("SQLite schema 1: {}", store.quick_check()?);
        return Ok(());
    }
    if let Some(path) = &args.import_session {
        let id = store.import_session(path)?;
        println!("Sessione importata: #{id}");
        return Ok(());
    }
    if let (Some(id), Some(name)) = (args.rename_session, args.session_name.as_deref()) {
        store.rename_session(id, name)?;
        println!("Sessione #{id} rinominata");
        return Ok(());
    }
    if args.sessions {
        let sessions = store.list_sessions(5000)?;
        println!("{}", serde_json::to_string_pretty(&sessions)?);
        if let Some(path) = &args.render {
            let mut state = UiState {
                active_tab: 10,
                ..Default::default()
            };
            preferences.apply(&mut state);
            state.workspace.sessions = sessions;
            state.workspace.db_path = database.display().to_string();
            state.workspace.benchmark_history = store.benchmarks(None, 100)?;
            render(
                path,
                &args,
                &Inventory::default(),
                &Snapshot::default(),
                &LlmState::default(),
                state,
                &History::default(),
            )?;
        }
        return Ok(());
    }
    if args.compare.len() == 2 {
        let value = store.compare(args.compare[0], args.compare[1])?;
        if let Some(path) = &args.comparison_output {
            write_json(path, &value)?;
        }
        println!("{}", serde_json::to_string_pretty(&value)?);
        if let Some(path) = &args.render {
            let snapshot = value.left_series.first().cloned().unwrap_or_default();
            let mut state = UiState {
                active_tab: 11,
                ..Default::default()
            };
            preferences.apply(&mut state);
            state.workspace.comparison = Some(value);
            state.workspace.db_path = database.display().to_string();
            render(
                path,
                &args,
                &Inventory::default(),
                &snapshot,
                &LlmState::default(),
                state,
                &History::default(),
            )?;
        }
        return Ok(());
    }
    if let Some(id) = args.load_session {
        let data = store.load_session(id, 2000)?;
        if let Some(path) = &args.export_session {
            store.export_session(id, path)?;
        }
        println!("{}", serde_json::to_string_pretty(&data.summary)?);
        let snapshot = data
            .samples
            .get(args.sample_index)
            .cloned()
            .or_else(|| data.samples.last().cloned())
            .unwrap_or_default();
        let llm = archived_llm(&data);
        let cpu = data.benchmarks.iter().find_map(|r| r.cpu.as_ref());
        if let Some(path) = &args.snapshot {
            export::export_snapshot_with_bench(path, &data.inventory, &snapshot, &llm, cpu)?;
        }
        if let Some(path) = &args.render {
            let mut state = UiState {
                active_tab: usize::from(args.tab - 1),
                cpu_benchmark: cpu.cloned(),
                ..Default::default()
            };
            preferences.apply(&mut state);
            state.workspace.replay_index =
                args.sample_index.min(data.samples.len().saturating_sub(1));
            state.workspace.loaded = Some(data.clone());
            let mut history = History::default();
            for s in data.samples.iter().take(state.workspace.replay_index + 1) {
                history.push(s);
            }
            render(
                path,
                &args,
                &data.inventory,
                &snapshot,
                &llm,
                state,
                &history,
            )?;
        }
        return Ok(());
    }
    if let Some(runtime) = &args.observe_runtime {
        if let Some(id) = args.observe_session
            && store.session_state(id)? != "open"
        {
            bail!(
                "La sessione observer deve essere aperta; avvia r nella TUI e usa --sessions per l'ID."
            );
        }
        let stop = Arc::new(AtomicBool::new(false));
        let signal = stop.clone();
        ctrlc::set_handler(move || signal.store(true, Ordering::Relaxed))?;
        let mut archive = ArchiveWorker::start(store);
        let sender = archive.sender();
        let endpoint = endpoint(runtime, &endpoints).to_owned();
        let runtime = runtime.clone();
        let port = args.observe_port;
        let session = args.observe_session;
        let flag = stop.clone();
        println!(
            "Observer locale http://127.0.0.1:{port} → {runtime}. Solo traffico instradato qui; nessun prompt/risposta salvato. Ctrl+C ferma."
        );
        let worker = thread::spawn(move || {
            rust_cpu::observer::serve(
                &runtime,
                &endpoint,
                port,
                move |value| {
                    if sender
                        .try_send(Command::Observed { value, session })
                        .is_err()
                    {
                        eprintln!("Observer: coda SQLite non disponibile; metrica non salvata");
                    }
                },
                flag,
            )
        });
        while !worker.is_finished() {
            for reply in archive.replies.try_iter() {
                match reply {
                    Reply::Saved(message) => println!("{message}"),
                    Reply::Error { message, .. } => eprintln!("{message}"),
                    _ => {}
                }
            }
            thread::sleep(Duration::from_millis(100));
        }
        let result = worker
            .join()
            .map_err(|_| anyhow::anyhow!("Observer interrotto"))?;
        archive.shutdown()?;
        return result;
    }
    let export_dir = args
        .export_dir
        .clone()
        .unwrap_or_else(app::default_export_dir);
    if !headless {
        drop(store);
        return app::run(
            endpoints,
            export_dir,
            preferences.interval_ms,
            args.no_llm,
            database,
            preferences,
        );
    }

    let cancel = Arc::new(AtomicBool::new(false));
    let signal = cancel.clone();
    ctrlc::set_handler(move || signal.store(true, Ordering::Relaxed))?;
    let inventory = collector::inventory();
    let mut collector = Collector::new();
    let _ = collector.sample();
    if !wait_interval(preferences.interval_ms, &cancel) {
        bail!("Operazione annullata prima del primo campione");
    }
    let mut snapshot = collector.sample();
    let mut llm_state = if args.no_llm {
        LlmState::default()
    } else {
        llm::probe_all(&endpoints)
    };
    let mut cpu_result = None;
    let mut history = History::default();
    history.push(&snapshot);
    let session_id = if args.save_session.is_some() || args.record.is_some() || benchmark {
        let name = args.save_session.clone().unwrap_or_else(|| {
            format!(
                "{} {}",
                if benchmark { "Benchmark" } else { "Sessione" },
                chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
            )
        });
        let id = store.begin_session_with_sample(
            &name,
            &inventory,
            preferences.interval_ms,
            &snapshot,
        )?;
        println!("Sessione SQLite: #{id} ({name})");
        Some(id)
    } else {
        None
    };
    let mut alert_engine = rust_cpu::settings::AlertEngine::default();
    persist_alerts(
        &store,
        session_id,
        &snapshot,
        &mut alert_engine,
        &preferences.alerts,
    )?;
    let result = (|| -> Result<()> {
        let mut recorder = if let Some(path) = &args.record {
            Some(Recorder::new(path)?)
        } else {
            None
        };
        if let Some(rec) = &mut recorder {
            rec.append(&snapshot)?;
        }
        if benchmark {
            enum Event {
                Cpu(crate_bench::CpuBenchmark),
                Llm(BenchmarkResult),
                Error(String),
                Done,
            }
            use rust_cpu::bench as crate_bench;
            let (tx, rx) = mpsc::channel();
            let runtime = args.benchmark_runtime.clone();
            let model = args.benchmark_model.clone();
            let endpoint = runtime
                .as_deref()
                .map(|r| endpoint(r, &endpoints).to_owned());
            let flag = cancel.clone();
            let runs = args.benchmark_runs;
            thread::spawn(move || {
                for _ in 0..runs {
                    let result = if let (Some(runtime), Some(model), Some(endpoint)) =
                        (&runtime, &model, &endpoint)
                    {
                        llm::benchmark(runtime, endpoint, model, 64, flag.clone()).map(Event::Llm)
                    } else {
                        crate_bench::run(flag.clone()).map(Event::Cpu)
                    };
                    match result {
                        Ok(value) => {
                            if tx.send(value).is_err() {
                                return;
                            }
                        }
                        Err(error) => {
                            let _ = tx.send(Event::Error(format!("{error:#}")));
                            break;
                        }
                    }
                }
                let _ = tx.send(Event::Done);
            });
            let mut next_sample =
                std::time::Instant::now() + Duration::from_millis(preferences.interval_ms);
            loop {
                match rx
                    .recv_timeout(next_sample.saturating_duration_since(std::time::Instant::now()))
                {
                    Ok(Event::Cpu(value)) => {
                        store.save_cpu(session_id, &value)?;
                        println!("{}", serde_json::to_string_pretty(&value)?);
                        cpu_result = Some(value);
                    }
                    Ok(Event::Llm(value)) => {
                        store.save_llm(session_id, &value, false)?;
                        println!("{}", serde_json::to_string_pretty(&value)?);
                        llm_state.benchmark = Some(value);
                    }
                    Ok(Event::Error(error)) => bail!("{error}"),
                    Ok(Event::Done) => break,
                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                        bail!("Worker benchmark interrotto")
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                }
                if std::time::Instant::now() >= next_sample {
                    snapshot = collector.sample();
                    history.push(&snapshot);
                    if let Some(id) = session_id {
                        store.append_sample(id, &snapshot)?;
                    }
                    persist_alerts(
                        &store,
                        session_id,
                        &snapshot,
                        &mut alert_engine,
                        &preferences.alerts,
                    )?;
                    if let Some(rec) = &mut recorder {
                        rec.append(&snapshot)?;
                    }
                    next_sample =
                        std::time::Instant::now() + Duration::from_millis(preferences.interval_ms);
                }
            }
            snapshot = collector.sample();
            history.push(&snapshot);
            if let Some(id) = session_id {
                store.append_sample(id, &snapshot)?;
            }
            persist_alerts(
                &store,
                session_id,
                &snapshot,
                &mut alert_engine,
                &preferences.alerts,
            )?;
            if let Some(rec) = &mut recorder {
                rec.append(&snapshot)?;
            }
        } else {
            for _ in 1..args.samples {
                if cancel.load(Ordering::Relaxed) {
                    break;
                }
                if !wait_interval(preferences.interval_ms, &cancel) {
                    break;
                }
                snapshot = collector.sample();
                history.push(&snapshot);
                if let Some(id) = session_id {
                    store.append_sample(id, &snapshot)?;
                }
                persist_alerts(
                    &store,
                    session_id,
                    &snapshot,
                    &mut alert_engine,
                    &preferences.alerts,
                )?;
                if let Some(rec) = &mut recorder {
                    rec.append(&snapshot)?;
                }
            }
        }
        if let Some(rec) = recorder {
            println!("CSV: {} ({} campioni)", rec.path.display(), rec.rows);
        }
        if let Some(path) = &args.snapshot {
            export::export_snapshot_with_bench(
                path,
                &inventory,
                &snapshot,
                &llm_state,
                cpu_result.as_ref(),
            )?;
            println!("JSON: {}", path.display());
        }
        if let Some(path) = &args.render {
            let mut state = UiState {
                active_tab: usize::from(args.tab - 1),
                cpu_benchmark: cpu_result.clone(),
                ..Default::default()
            };
            preferences.apply(&mut state);
            state.workspace.db_path = database.display().to_string();
            state.workspace.sessions = store.list_sessions(500)?;
            state.workspace.preferences = preferences.clone();
            render(
                path, &args, &inventory, &snapshot, &llm_state, state, &history,
            )?;
        }
        Ok(())
    })();
    if let Some(id) = session_id {
        store.finish_session(id)?;
    }
    result
}
fn wait_interval(ms: u64, cancel: &AtomicBool) -> bool {
    let deadline = std::time::Instant::now() + Duration::from_millis(ms);
    while std::time::Instant::now() < deadline {
        if cancel.load(Ordering::Relaxed) {
            return false;
        }
        thread::sleep(
            deadline
                .saturating_duration_since(std::time::Instant::now())
                .min(Duration::from_millis(50)),
        );
    }
    !cancel.load(Ordering::Relaxed)
}
fn persist_alerts(
    store: &Store,
    id: Option<i64>,
    snapshot: &Snapshot,
    engine: &mut rust_cpu::settings::AlertEngine,
    thresholds: &rust_cpu::settings::AlertThresholds,
) -> Result<()> {
    for event in engine.evaluate(snapshot, thresholds) {
        store.save_alert(id, &event)?;
        eprintln!("Avviso risorse: {}", event.message);
    }
    Ok(())
}
fn endpoint<'a>(runtime: &str, endpoints: &'a Endpoints) -> &'a str {
    match runtime {
        "ollama" => &endpoints.ollama,
        "lm-studio" => &endpoints.lm_studio,
        _ => &endpoints.llama_cpp,
    }
}
fn archived_llm(data: &SessionData) -> LlmState {
    LlmState {
        benchmark: data
            .benchmarks
            .iter()
            .filter(|r| r.kind == "llm")
            .find_map(|r| r.llm.clone()),
        observations: data
            .benchmarks
            .iter()
            .filter(|r| r.kind == "observed")
            .filter_map(|r| r.llm.clone())
            .collect(),
        ..Default::default()
    }
}
fn render(
    path: &Path,
    args: &Args,
    inventory: &Inventory,
    snapshot: &Snapshot,
    llm: &LlmState,
    mut state: UiState,
    history: &History,
) -> Result<()> {
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(args.width, args.height))?;
    terminal.draw(|frame| ui::render(frame, inventory, snapshot, llm, &mut state, history))?;
    let buffer = terminal.backend().buffer();
    let content = if path
        .extension()
        .is_some_and(|v| v.eq_ignore_ascii_case("svg"))
    {
        export::buffer_svg(buffer)
    } else {
        let mut text = String::new();
        for y in 0..args.height {
            for x in 0..args.width {
                text.push_str(buffer[(x, y)].symbol());
            }
            text.push('\n');
        }
        text
    };
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .context("Il file di rendering deve essere nuovo")?;
    file.write_all(content.as_bytes())?;
    file.sync_all()?;
    println!("TUI: {}", path.display());
    Ok(())
}
fn write_json(path: &Path, value: &SessionComparison) -> Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_vec_pretty(value)?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(&content)?;
    file.sync_all()?;
    Ok(())
}
