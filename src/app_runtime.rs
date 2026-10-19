use crate::{
    app::{self, Action},
    collector::{self, Collector},
    export::{self, Recorder},
    llm::{self, Endpoints},
    model::*,
    settings::{AlertEngine, Preferences},
    storage::Store,
    ui,
    workspace_runtime::{ArchiveWorker, Command, Reply},
};
use anyhow::{Result, bail};
use crossterm::{
    event::{self, Event, MouseButton, MouseEventKind},
    execute,
};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

enum Message {
    Inventory(Inventory),
    Sample(Snapshot),
    Llm(LlmState),
    Cpu {
        value: std::result::Result<crate::bench::CpuBenchmark, String>,
        run: u32,
        total: u32,
        last: bool,
    },
    Benchmark {
        value: std::result::Result<BenchmarkResult, String>,
        run: u32,
        total: u32,
        last: bool,
    },
}
struct Workers {
    stop: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
}
impl Drop for Workers {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.cancel.store(true, Ordering::Relaxed);
    }
}
struct TerminalGuard;
impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = execute!(std::io::stdout(), event::DisableMouseCapture);
        ratatui::restore();
    }
}

pub fn run(
    endpoints: Endpoints,
    export_dir: PathBuf,
    interval_ms: u64,
    no_llm: bool,
    database: PathBuf,
    preferences: Preferences,
) -> Result<()> {
    let mut archive = ArchiveWorker::start(Store::open(&database)?);
    archive.submit(Command::Refresh)?;
    let stop = Arc::new(AtomicBool::new(false));
    let cancel = Arc::new(AtomicBool::new(false));
    let interval = Arc::new(AtomicU64::new(interval_ms));
    let _workers = Workers {
        stop: stop.clone(),
        cancel: cancel.clone(),
    };
    let (sender, receiver) = mpsc::channel();
    let (probe_tx, probe_rx) = mpsc::sync_channel(1);
    {
        let tx = sender.clone();
        thread::spawn(move || {
            let _ = tx.send(Message::Inventory(collector::inventory()));
        });
    }
    {
        let tx = sender.clone();
        let stop = stop.clone();
        let interval = interval.clone();
        thread::spawn(move || {
            let mut collector = Collector::new();
            let _ = collector.sample();
            let mut next = Instant::now() + Duration::from_millis(500);
            while !stop.load(Ordering::Relaxed) {
                if Instant::now() >= next {
                    if tx.send(Message::Sample(collector.sample())).is_err() {
                        break;
                    }
                    next = Instant::now() + Duration::from_millis(interval.load(Ordering::Relaxed));
                }
                thread::sleep(Duration::from_millis(40));
            }
        });
    }
    if !no_llm {
        let tx = sender.clone();
        let stop = stop.clone();
        thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                if tx.send(Message::Llm(llm::probe_all(&endpoints))).is_err() {
                    break;
                }
                if matches!(
                    probe_rx.recv_timeout(Duration::from_secs(5)),
                    Err(mpsc::RecvTimeoutError::Disconnected)
                ) {
                    break;
                }
            }
        });
    }
    let mut terminal = ratatui::init();
    let terminal_guard = TerminalGuard;
    execute!(std::io::stdout(), event::EnableMouseCapture)?;
    let mut state = UiState::default();
    preferences.apply(&mut state);
    state.tick_ms = interval_ms;
    state.workspace.preferences = preferences;
    state.workspace.db_path = database.display().to_string();
    let mut inventory = Inventory {
        cpu_name: "Raccolta inventario in corso…".into(),
        ..Default::default()
    };
    let mut snapshot = Snapshot::default();
    let mut latest = Snapshot::default();
    let mut llm_state = LlmState::default();
    let mut history = History::default();
    let mut recorder: Option<Recorder> = None;
    let mut auto_session = false;
    let mut live_cpu = None;
    let mut alerts = AlertEngine::default();
    let mut last_preferences = serde_json::to_string(&Preferences::capture(&state))?;
    let mut last_refresh = Instant::now();
    let result = (|| -> Result<()> {
        loop {
            for reply in archive.replies.try_iter() {
                match reply {
                    Reply::Started(id) => {
                        state.workspace.active_session = Some(id);
                        state.workspace.loading = false;
                        state.status =
                            format!("Registrazione SQLite #{id} avviata | r ferma | F2 archivio");
                    }
                    Reply::Finished => {
                        state.workspace.active_session = None;
                        state.workspace.loading = false;
                    }
                    Reply::Refreshed {
                        sessions,
                        benchmarks,
                        alerts: stored_alerts,
                    } => {
                        let selected_id = if state.active_tab == 10 {
                            state.workspace.sessions.get(state.selected).map(|s| s.id)
                        } else {
                            None
                        };
                        if state.workspace.name_edit.is_none() && !state.workspace.delete_confirm {
                            state.workspace.sessions = sessions;
                            if state.active_tab == 10 {
                                state.selected = selected_id
                                    .and_then(|id| {
                                        state.workspace.sessions.iter().position(|s| s.id == id)
                                    })
                                    .unwrap_or(state.selected)
                                    .min(state.workspace.sessions.len().saturating_sub(1));
                            }
                        }
                        state.workspace.benchmark_history = benchmarks;
                        state.workspace.alerts = stored_alerts;
                        llm_state.observations = state
                            .workspace
                            .benchmark_history
                            .iter()
                            .filter(|r| r.kind == "observed")
                            .filter_map(|r| r.llm.clone())
                            .take(8)
                            .collect();
                        state.workspace.loading = false;
                    }
                    Reply::Loaded(mut data) => {
                        for sample in &mut data.samples {
                            sample.warnings.push("REPLAY SQLite: i processi archiviati includono solo quelli riconosciuti come LLM; il monitor live resta completo.".into());
                        }
                        state.cpu_benchmark = data.benchmarks.iter().find_map(|b| b.cpu.clone());
                        state.workspace.loaded = Some(*data);
                        state.workspace.replay_index = 0;
                        state.workspace.loading = false;
                        state.active_tab = 0;
                        state.offset = 0;
                        state.selected = 0;
                        state.status="Sessione caricata | [ ] scorri campioni | l torna live | raccolta live indipendente".into();
                    }
                    Reply::Compared(data) => {
                        state.workspace.comparison = Some(*data);
                        state.workspace.loading = false;
                        state.active_tab = 11;
                        state.offset = 0;
                        state.status =
                            "Confronto caricato: B − A | e esporta JSON | F2 scegli sessioni"
                                .into();
                    }
                    Reply::Saved(message) => {
                        state.status = message;
                        state.workspace.loading = false;
                    }
                    Reply::Error {
                        message,
                        recording_stopped,
                    } => {
                        state.workspace.last_error = Some(message.clone());
                        state.status = message;
                        state.workspace.loading = false;
                        if recording_stopped {
                            state.recording = false;
                            state.workspace.active_session = None;
                            auto_session = false;
                            recorder = None;
                        }
                    }
                }
            }
            for message in receiver.try_iter() {
                match message {
                    Message::Inventory(value) => inventory = value,
                    Message::Sample(value) => {
                        if state.recording
                            && let Err(error) = archive.submit(Command::Append(value.clone()))
                        {
                            state.status = format!(
                                "Registrazione SQLite interrotta: {error}; dati accettati conservati"
                            );
                            state.workspace.last_error = Some(state.status.clone());
                            state.recording = false;
                            auto_session = false;
                            recorder = None;
                            let _ = archive.submit(Command::Finish);
                        }
                        if let Some(csv) = &mut recorder
                            && let Err(error) = csv.append(&value)
                        {
                            state.status =
                                format!("CSV interrotto ({error}); archivio SQLite continua");
                            recorder = None;
                        }
                        for event in alerts.evaluate(&value, &state.workspace.preferences.alerts) {
                            let _ = archive.submit(Command::Alert(event.clone()));
                            state.status = format!("AVVISO: {}", event.message);
                            state.workspace.alerts.insert(0, event);
                            state.workspace.alerts.truncate(100);
                        }
                        latest = value;
                        if !state.paused {
                            snapshot = latest.clone();
                            history.push(&snapshot);
                        }
                    }
                    Message::Llm(value) => {
                        if !state.benchmark_confirm {
                            let previous = llm_state.benchmark.take();
                            let observations = std::mem::take(&mut llm_state.observations);
                            llm_state = value;
                            llm_state.benchmark = previous;
                            llm_state.observations = observations;
                        }
                    }
                    Message::Cpu {
                        value,
                        run,
                        total,
                        last,
                    } => {
                        if last {
                            state.cpu_benchmark_running = false;
                        }
                        match value {
                            Ok(value) => {
                                archive.submit(Command::Cpu(value.clone()))?;
                                live_cpu = Some(value.clone());
                                state.cpu_benchmark = Some(value);
                                state.status =
                                    format!("Benchmark CPU {run}/{total} salvato SQLite");
                            }
                            Err(error) => {
                                state.status = format!("Benchmark CPU {run}/{total}: {error}")
                            }
                        };
                        if last && auto_session {
                            archive.submit(Command::Finish)?;
                            state.recording = false;
                            auto_session = false;
                            recorder = None;
                        }
                    }
                    Message::Benchmark {
                        value,
                        run,
                        total,
                        last,
                    } => {
                        if last {
                            state.benchmark_running = false;
                        }
                        match value {
                            Ok(value) => {
                                archive.submit(Command::Llm(value.clone()))?;
                                state.status = format!(
                                    "Benchmark LLM {run}/{total} salvato SQLite | {} token",
                                    value
                                        .output_tokens
                                        .map(|n| n.to_string())
                                        .unwrap_or_else(|| "n/d".into())
                                );
                                llm_state.benchmark = Some(value);
                            }
                            Err(error) => {
                                state.status = format!("Benchmark LLM {run}/{total}: {error}")
                            }
                        };
                        if last && auto_session {
                            archive.submit(Command::Finish)?;
                            state.recording = false;
                            auto_session = false;
                            recorder = None;
                        }
                    }
                }
            }
            if last_refresh.elapsed() >= Duration::from_secs(5) {
                let _ = archive.submit(Command::Refresh);
                last_refresh = Instant::now();
            }
            if state.active_tab == 6 {
                state.selected = state.selected.min(
                    filtered_processes(&snapshot, &state)
                        .len()
                        .saturating_sub(1),
                );
            }
            let (view_inventory, view_snapshot, view_history, view_llm) =
                display_data(&state, &inventory, &snapshot, &history, &llm_state);
            terminal.draw(|frame| {
                ui::render(
                    frame,
                    &view_inventory,
                    &view_snapshot,
                    &view_llm,
                    &mut state,
                    &view_history,
                )
            })?;
            if !event::poll(Duration::from_millis(100))? {
                continue;
            }
            let was_paused = state.paused;
            let action = match event::read()? {
                Event::Key(key) => app::handle_key(
                    key,
                    &mut state,
                    &view_snapshot,
                    view_llm.runtimes.iter().map(|r| r.models.len()).sum(),
                ),
                Event::Mouse(mouse) if !app::modal_open(&state) => {
                    match mouse.kind {
                        MouseEventKind::Down(MouseButton::Left) => {
                            let size = terminal.size()?;
                            if let Some(tab) =
                                ui::sidebar_tab_at(size.width, size.height, mouse.column, mouse.row)
                            {
                                state.active_tab = tab;
                                state.selected = 0;
                                state.offset = 0;
                                if tab == 10 {
                                    let _ = archive.submit(Command::Refresh);
                                }
                            }
                        }
                        MouseEventKind::ScrollDown => {
                            if state.active_tab == 12 {
                                state.workspace.settings_field = (state.workspace.settings_field
                                    + 1)
                                .min(crate::settings::SETTINGS_FIELDS.len() - 1);
                            } else {
                                state.offset = state.offset.saturating_add(3);
                                state.selected = state.selected.saturating_add(1);
                            }
                        }
                        MouseEventKind::ScrollUp => {
                            if state.active_tab == 12 {
                                state.workspace.settings_field =
                                    state.workspace.settings_field.saturating_sub(1);
                            } else {
                                state.offset = state.offset.saturating_sub(3);
                                state.selected = state.selected.saturating_sub(1);
                            }
                        }
                        _ => {}
                    }
                    Action::None
                }
                _ => Action::None,
            };
            if was_paused && !state.paused {
                snapshot = latest.clone();
            }
            interval.store(state.tick_ms, Ordering::Relaxed);
            let operation = (|| -> Result<()> {
                match action {
                    Action::Quit => return Ok(()),
                    Action::ReloadArchive => {
                        state.workspace.loading = true;
                        archive.submit(Command::Refresh)?;
                    }
                    Action::LoadSession => {
                        let id = selected_id(&state)?;
                        state.workspace.loading = true;
                        archive.submit(Command::Load(id))?;
                    }
                    Action::CompareSessions => {
                        if let (Some(a), Some(b)) =
                            (state.workspace.baseline_id, state.workspace.candidate_id)
                        {
                            state.workspace.loading = true;
                            archive.submit(Command::Compare(a, b))?;
                        } else {
                            bail!(
                                "F2: scegli baseline A con a e sessione B con b, poi c confronta"
                            );
                        }
                    }
                    Action::RenameSession => {
                        let id = selected_id(&state)?;
                        if let Some(name) = state.workspace.name_edit.take() {
                            archive.submit(Command::Rename(id, name))?;
                        }
                    }
                    Action::DeleteSession => {
                        archive.submit(Command::Delete(selected_id(&state)?))?;
                    }
                    Action::ReturnLive => {
                        state.workspace.loaded = None;
                        state.workspace.replay_index = 0;
                        state.cpu_benchmark = live_cpu.clone();
                        state.status = "Monitor live ripristinato".into();
                    }
                    Action::BackupDatabase => {
                        archive.submit(Command::Backup(export::unique_path(
                            &export_dir,
                            "backup",
                            "sqlite3",
                        )))?;
                        state.status = "Backup SQLite richiesto…".into();
                    }
                    Action::Export => {
                        if state.active_tab == 10 {
                            archive.submit(Command::ExportSession(
                                selected_id(&state)?,
                                export::unique_path(&export_dir, "session", "json"),
                            ))?;
                        } else if state.active_tab == 11 {
                            let comparison = state
                                .workspace
                                .comparison
                                .as_ref()
                                .ok_or_else(|| anyhow::anyhow!("Nessun confronto caricato"))?;
                            archive.submit(Command::ExportFrozenComparison(
                                Box::new(comparison.clone()),
                                export::unique_path(&export_dir, "comparison", "json"),
                            ))?;
                        } else {
                            if view_snapshot.memory_total == 0 {
                                bail!("Attendi un campione valido prima di esportare");
                            }
                            let path = export::unique_path(&export_dir, "snapshot", "json");
                            export::export_snapshot_with_bench(
                                &path,
                                &view_inventory,
                                &view_snapshot,
                                &view_llm,
                                state.cpu_benchmark.as_ref(),
                            )?;
                            state.status = format!("JSON salvato: {}", path.display());
                        }
                    }
                    Action::ToggleRecording => {
                        auto_session = false;
                        if state.recording {
                            archive.submit(Command::Finish)?;
                            state.recording = false;
                            recorder = None;
                            state.status = "Sessione fermata; salvataggio SQLite in corso…".into();
                        } else {
                            start_session(&archive, &mut state, &inventory, &latest, "Sessione")?;
                            let path = export::unique_path(&export_dir, "session", "csv");
                            match Recorder::new(&path) {
                                Ok(csv) => recorder = Some(csv),
                                Err(error) => {
                                    state.status =
                                        format!("SQLite attivo; CSV non disponibile: {error}")
                                }
                            };
                        }
                    }
                    Action::RefreshLlm => {
                        if no_llm {
                            state.status = "Sonde LLM disattivate da --no-llm".into();
                        } else {
                            let _ = probe_tx.try_send(());
                            state.status = "Aggiornamento runtime richiesto…".into();
                        }
                        let _ = archive.submit(Command::Refresh);
                    }
                    Action::Benchmark => {
                        if state.workspace.loaded.is_some() {
                            bail!("Premi l per tornare live prima di eseguire un benchmark");
                        }
                        let selected = llm_state
                            .runtimes
                            .iter()
                            .flat_map(|r| r.models.iter().map(move |m| (r, m)))
                            .nth(state.selected)
                            .ok_or_else(|| anyhow::anyhow!("Modello non disponibile"))?;
                        let runtime = selected.0.name.clone();
                        let endpoint = selected.0.endpoint.clone();
                        let model = selected.1.id.clone();
                        let total = if state.workspace.repeat_requested {
                            state.workspace.preferences.benchmark_repeats
                        } else {
                            1
                        };
                        if !state.recording {
                            start_session(
                                &archive,
                                &mut state,
                                &inventory,
                                &latest,
                                "Benchmark LLM",
                            )?;
                            auto_session = true;
                        }
                        cancel.store(false, Ordering::Relaxed);
                        state.benchmark_running = true;
                        state.status =
                            format!("Benchmark LLM 1/{total} | c annulla | archivio SQLite attivo");
                        let tx = sender.clone();
                        let cancel = cancel.clone();
                        thread::spawn(move || {
                            for run in 1..=total {
                                let value =
                                    llm::benchmark(&runtime, &endpoint, &model, 64, cancel.clone())
                                        .map_err(|e| format!("{e:#}"));
                                let last = run == total || value.is_err();
                                if tx
                                    .send(Message::Benchmark {
                                        value,
                                        run,
                                        total,
                                        last,
                                    })
                                    .is_err()
                                    || last
                                {
                                    break;
                                }
                            }
                        });
                    }
                    Action::CpuBenchmark => {
                        if state.workspace.loaded.is_some() {
                            bail!("Premi l per tornare live prima di eseguire un benchmark");
                        }
                        let total = if state.workspace.repeat_requested {
                            state.workspace.preferences.benchmark_repeats
                        } else {
                            1
                        };
                        if !state.recording {
                            start_session(
                                &archive,
                                &mut state,
                                &inventory,
                                &latest,
                                "Benchmark CPU",
                            )?;
                            auto_session = true;
                        }
                        cancel.store(false, Ordering::Relaxed);
                        state.cpu_benchmark_running = true;
                        let tx = sender.clone();
                        let cancel = cancel.clone();
                        thread::spawn(move || {
                            for run in 1..=total {
                                let value =
                                    crate::bench::run(cancel.clone()).map_err(|e| format!("{e:#}"));
                                let last = run == total || value.is_err();
                                if tx
                                    .send(Message::Cpu {
                                        value,
                                        run,
                                        total,
                                        last,
                                    })
                                    .is_err()
                                    || last
                                {
                                    break;
                                }
                            }
                        });
                    }
                    Action::CancelBenchmark => {
                        cancel.store(true, Ordering::Relaxed);
                        state.status = "Annullamento benchmark richiesto…".into();
                    }
                    Action::None => {}
                }
                Ok(())
            })();
            if let Err(error) = operation {
                state.status = format!("{error:#}");
                state.workspace.loading = false;
                state.workspace.last_error = Some(state.status.clone());
            }
            if action == Action::Quit {
                break;
            }
            let preferences = Preferences::capture(&state);
            let serialized = serde_json::to_string(&preferences)?;
            if serialized != last_preferences {
                match archive.submit(Command::Preferences(preferences.clone())) {
                    Ok(()) => {
                        state.workspace.preferences = preferences;
                        last_preferences = serialized;
                    }
                    Err(error) => state.status = format!("Preferenze non salvate: {error}"),
                }
            }
        }
        Ok(())
    })();
    cancel.store(true, Ordering::Relaxed);
    stop.store(true, Ordering::Relaxed);
    let _ = archive.submit(Command::Preferences(Preferences::capture(&state)));
    drop(terminal_guard); // Restore the console before waiting for durable SQLite shutdown.
    let saved = archive.shutdown();
    result?;
    saved
}

fn start_session(
    archive: &ArchiveWorker,
    state: &mut UiState,
    inventory: &Inventory,
    snapshot: &Snapshot,
    prefix: &str,
) -> Result<()> {
    if snapshot.memory_total == 0 || inventory.logical_cores == 0 {
        bail!("Attendi il primo campione prima di avviare la sessione");
    }
    let name = format!(
        "{prefix} {}",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
    );
    archive.submit(Command::Start {
        name,
        inventory: Box::new(inventory.clone()),
        interval_ms: state.tick_ms,
        first: snapshot.clone(),
    })?;
    state.recording = true;
    state.workspace.loading = true;
    Ok(())
}
fn selected_id(state: &UiState) -> Result<i64> {
    state
        .workspace
        .sessions
        .get(state.selected)
        .map(|s| s.id)
        .ok_or_else(|| anyhow::anyhow!("Nessuna sessione selezionata"))
}
fn display_data(
    state: &UiState,
    inventory: &Inventory,
    snapshot: &Snapshot,
    history: &History,
    llm: &LlmState,
) -> (Inventory, Snapshot, History, LlmState) {
    if let Some(data) = &state.workspace.loaded {
        let mut h = History::default();
        for sample in data.samples.iter().take(state.workspace.replay_index + 1) {
            h.push(sample);
        }
        let sample = data
            .samples
            .get(state.workspace.replay_index)
            .cloned()
            .unwrap_or_default();
        let archive_llm = LlmState {
            benchmark: data
                .benchmarks
                .iter()
                .filter(|b| b.kind == "llm")
                .find_map(|b| b.llm.clone()),
            observations: data
                .benchmarks
                .iter()
                .filter(|b| b.kind == "observed")
                .filter_map(|b| b.llm.clone())
                .collect(),
            ..Default::default()
        };
        (data.inventory.clone(), sample, h, archive_llm)
    } else {
        (
            inventory.clone(),
            snapshot.clone(),
            History {
                cpu: history.cpu.clone(),
                memory: history.memory.clone(),
                gpu: history.gpu.clone(),
            },
            llm.clone(),
        )
    }
}
