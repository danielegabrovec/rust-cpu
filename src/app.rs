use crate::{
    collector::{self, Collector},
    export::{self, Recorder},
    llm::{self, Endpoints},
    model::*,
    ui,
};
use anyhow::Result;
use crossterm::{
    event::{
        self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
    },
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

#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    None,
    Quit,
    Export,
    ToggleRecording,
    Benchmark,
    CpuBenchmark,
    CancelBenchmark,
    RefreshLlm,
}

pub fn handle_key(
    key: KeyEvent,
    state: &mut UiState,
    snapshot: &Snapshot,
    model_count: usize,
) -> Action {
    if key.kind == KeyEventKind::Release {
        return Action::None;
    }
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return Action::Quit;
    }
    if state.filter_edit {
        match key.code {
            KeyCode::Esc | KeyCode::Enter => state.filter_edit = false,
            KeyCode::Backspace => {
                state.query.pop();
                state.selected = 0;
            }
            KeyCode::Char(c) if state.query.len() < 128 && !c.is_control() => {
                state.query.push(c);
                state.selected = 0;
            }
            _ => {}
        }
        return Action::None;
    }
    if state.help {
        if matches!(key.code, KeyCode::Esc | KeyCode::Char('?') | KeyCode::Enter) {
            state.help = false;
        }
        return Action::None;
    }
    if state.process_detail {
        if matches!(key.code, KeyCode::Esc | KeyCode::Enter) {
            state.process_detail = false;
        }
        return Action::None;
    }
    if state.benchmark_confirm {
        return match key.code {
            KeyCode::Enter => {
                state.benchmark_confirm = false;
                Action::Benchmark
            }
            KeyCode::Esc | KeyCode::Char('q') => {
                state.benchmark_confirm = false;
                Action::None
            }
            _ => Action::None,
        };
    }
    if state.cpu_benchmark_confirm {
        return match key.code {
            KeyCode::Enter => {
                state.cpu_benchmark_confirm = false;
                Action::CpuBenchmark
            }
            KeyCode::Esc | KeyCode::Char('q') => {
                state.cpu_benchmark_confirm = false;
                Action::None
            }
            _ => Action::None,
        };
    }
    let before_tab = state.active_tab;
    let mut action = Action::None;
    match key.code {
        KeyCode::Char('q') | KeyCode::Esc => action = Action::Quit,
        KeyCode::Char('?') | KeyCode::F(1) => state.help = true,
        KeyCode::Tab | KeyCode::Right => state.active_tab = (state.active_tab + 1) % TABS.len(),
        KeyCode::BackTab | KeyCode::Left => {
            state.active_tab = (state.active_tab + TABS.len() - 1) % TABS.len()
        }
        KeyCode::Char(c @ '1'..='9') => state.active_tab = c.to_digit(10).unwrap() as usize - 1,
        KeyCode::Char('0') => state.active_tab = 9,
        KeyCode::Up | KeyCode::Char('k') => {
            state.selected = state.selected.saturating_sub(1);
            state.offset = state.offset.saturating_sub(1);
        }
        KeyCode::Down | KeyCode::Char('j') => {
            state.selected = state.selected.saturating_add(1);
            state.offset = state.offset.saturating_add(1);
        }
        KeyCode::PageUp => {
            state.offset = state.offset.saturating_sub(10);
            state.selected = state.selected.saturating_sub(10);
        }
        KeyCode::PageDown => {
            state.offset = state.offset.saturating_add(10);
            state.selected = state.selected.saturating_add(10);
        }
        KeyCode::Home => {
            state.offset = 0;
            state.selected = 0;
        }
        KeyCode::End => {
            state.offset = u16::MAX;
            state.selected = usize::MAX;
        }
        KeyCode::Char('/') => {
            state.active_tab = 6;
            state.filter_edit = true;
            state.selected = 0;
        }
        KeyCode::Char('s') => {
            state.sort = state.sort.next();
            state.selected = 0;
        }
        KeyCode::Char('a') => state.ascending = !state.ascending,
        KeyCode::Char(' ') => {
            state.paused = !state.paused;
            state.status = if state.paused {
                "Vista in pausa; raccolta e CSV continuano"
            } else {
                "Vista live ripresa"
            }
            .into();
        }
        KeyCode::Char('t') => state.theme_light = !state.theme_light,
        KeyCode::Char('e') => action = Action::Export,
        KeyCode::Char('r') => action = Action::ToggleRecording,
        KeyCode::Char('c') if state.benchmark_running || state.cpu_benchmark_running => {
            action = Action::CancelBenchmark
        }
        KeyCode::Char('x') if state.active_tab == 1 => {
            if !state.benchmark_running && !state.cpu_benchmark_running {
                state.cpu_benchmark_confirm = true;
            }
        }
        KeyCode::Char('b') if state.active_tab == 7 => {
            if state.benchmark_running || state.cpu_benchmark_running {
                state.status = "Benchmark in corso; c annulla, q esce".into();
            } else if model_count == 0 {
                state.status =
                    "Nessun modello disponibile. Avvia un runtime locale e premi F5".into();
            } else {
                state.benchmark_confirm = true;
            }
        }
        KeyCode::Enter if state.active_tab == 6 => {
            state.process_detail_row = filtered_processes(snapshot, state)
                .get(state.selected)
                .map(|p| (*p).clone());
            state.process_detail = state.process_detail_row.is_some();
        }
        KeyCode::F(5) => action = Action::RefreshLlm,
        KeyCode::Char('[') if state.active_tab == 8 => {
            state.capacity_field = (state.capacity_field + 5) % 6
        }
        KeyCode::Char(']') if state.active_tab == 8 => {
            state.capacity_field = (state.capacity_field + 1) % 6
        }
        KeyCode::Char('+') | KeyCode::Char('=') => {
            if state.active_tab == 8 {
                adjust_capacity(state, true);
            } else {
                state.tick_ms = (state.tick_ms + 500).min(10000);
            }
        }
        KeyCode::Char('-') => {
            if state.active_tab == 8 {
                adjust_capacity(state, false);
            } else {
                state.tick_ms = state.tick_ms.saturating_sub(500).max(500);
            }
        }
        _ => {}
    }
    if state.active_tab != before_tab {
        state.selected = 0;
        state.offset = 0;
    }
    let count = match state.active_tab {
        6 => filtered_processes(snapshot, state).len(),
        7 => model_count,
        _ => usize::MAX,
    };
    if count != usize::MAX {
        state.selected = state.selected.min(count.saturating_sub(1));
    }
    action
}

fn adjust_capacity(state: &mut UiState, increase: bool) {
    let c = &mut state.capacity;
    let direction = if increase { 1.0 } else { -1.0 };
    match state.capacity_field {
        0 => c.parameters_b = (c.parameters_b + direction * 0.5).clamp(0.5, 1000.0),
        1 => c.bits_per_weight = (c.bits_per_weight + direction * 0.5).clamp(1.0, 32.0),
        2 => {
            c.context = (c.context as i64 + if increase { 1024 } else { -1024 })
                .clamp(1024, 1_048_576) as u64
        }
        3 => c.layers = (c.layers as i64 + if increase { 1 } else { -1 }).clamp(1, 256) as u64,
        4 => c.kv_heads = (c.kv_heads as i64 + if increase { 1 } else { -1 }).clamp(1, 256) as u64,
        _ => {
            c.head_dim =
                (c.head_dim as i64 + if increase { 16 } else { -16 }).clamp(16, 1024) as u64
        }
    }
}

pub fn default_export_dir() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Documents")
        .join("RUST-CPU")
        .join("exports")
}

enum Message {
    Inventory(Inventory),
    Sample(Snapshot),
    Llm(LlmState),
    Benchmark(std::result::Result<BenchmarkResult, String>),
    CpuBenchmark(std::result::Result<crate::bench::CpuBenchmark, String>),
}
struct WorkerGuard {
    stop: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
}
impl Drop for WorkerGuard {
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
) -> Result<()> {
    let stop = Arc::new(AtomicBool::new(false));
    let cancel = Arc::new(AtomicBool::new(false));
    let interval = Arc::new(AtomicU64::new(interval_ms));
    let _workers = WorkerGuard {
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
                    let sample = collector.sample();
                    if tx.send(Message::Sample(sample)).is_err() {
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
        let endpoints = endpoints.clone();
        thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                let state = llm::probe_all(&endpoints);
                if tx.send(Message::Llm(state)).is_err() {
                    break;
                }
                match probe_rx.recv_timeout(Duration::from_secs(5)) {
                    Ok(()) | Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(_) => break,
                }
            }
        });
    }
    let mut terminal = ratatui::init();
    let _terminal = TerminalGuard;
    execute!(std::io::stdout(), event::EnableMouseCapture)?;
    let mut inventory = Inventory {
        cpu_name: "Raccolta inventario in corso…".into(),
        ..Default::default()
    };
    let mut snapshot = Snapshot::default();
    let mut latest = Snapshot::default();
    let mut llm_state = LlmState::default();
    let mut history = History::default();
    let mut state = UiState {
        tick_ms: interval_ms,
        ..Default::default()
    };
    let mut recorder: Option<Recorder> = None;
    loop {
        for message in receiver.try_iter() {
            match message {
                Message::Inventory(value) => inventory = value,
                Message::Sample(value) => {
                    if let Some(rec) = &mut recorder
                        && let Err(error) = rec.append(&value)
                    {
                        state.status = format!("CSV interrotto: {error}");
                        recorder = None;
                        state.recording = false;
                    }
                    latest = value;
                    if !state.paused {
                        snapshot = latest.clone();
                        history.push(&snapshot);
                    }
                }
                Message::Llm(value) => {
                    if !state.benchmark_confirm {
                        let benchmark = llm_state.benchmark.take();
                        llm_state = value;
                        llm_state.benchmark = benchmark;
                    }
                }
                Message::CpuBenchmark(value) => {
                    state.cpu_benchmark_running = false;
                    match value {
                        Ok(result) => {
                            state.status="Benchmark CPU completato. e include il risultato nello snapshot JSON".into();
                            state.cpu_benchmark = Some(result);
                        }
                        Err(error) => state.status = format!("Benchmark CPU: {error}"),
                    }
                }
                Message::Benchmark(value) => {
                    state.benchmark_running = false;
                    match value {
                        Ok(result) => {
                            state.status = format!(
                                "Benchmark completato: {} token, {:.2} s",
                                result
                                    .output_tokens
                                    .map(|v| v.to_string())
                                    .unwrap_or_else(|| "n/d".into()),
                                result.wall_seconds
                            );
                            llm_state.benchmark = Some(result);
                        }
                        Err(error) => state.status = format!("Benchmark: {error}"),
                    }
                }
            }
        }
        if state.active_tab == 6 {
            state.selected = state.selected.min(
                filtered_processes(&snapshot, &state)
                    .len()
                    .saturating_sub(1),
            );
        }
        terminal.draw(|frame| {
            ui::render(
                frame, &inventory, &snapshot, &llm_state, &mut state, &history,
            )
        })?;
        if !event::poll(Duration::from_millis(100))? {
            continue;
        }
        let was_paused = state.paused;
        let action = match event::read()? {
            Event::Key(key) => handle_key(
                key,
                &mut state,
                &snapshot,
                llm_state.runtimes.iter().map(|r| r.models.len()).sum(),
            ),
            Event::Mouse(mouse) if !modal_open(&state) => {
                match mouse.kind {
                    MouseEventKind::Down(MouseButton::Left) => {
                        let size = terminal.size()?;
                        if let Some(tab) =
                            ui::sidebar_tab_at(size.width, size.height, mouse.column, mouse.row)
                        {
                            state.active_tab = tab;
                            state.selected = 0;
                            state.offset = 0;
                        }
                    }
                    MouseEventKind::ScrollDown => {
                        state.offset = state.offset.saturating_add(3);
                        state.selected = state.selected.saturating_add(1);
                    }
                    MouseEventKind::ScrollUp => {
                        state.offset = state.offset.saturating_sub(3);
                        state.selected = state.selected.saturating_sub(1);
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
        match action {
            Action::Quit => break,
            Action::Export => {
                if snapshot.memory_total == 0 {
                    state.status = "Attendi il primo campione prima di esportare".into();
                    continue;
                }
                let path = export::unique_path(&export_dir, "snapshot", "json");
                state.status = match export::export_snapshot_with_bench(
                    &path,
                    &inventory,
                    &snapshot,
                    &llm_state,
                    state.cpu_benchmark.as_ref(),
                ) {
                    Ok(()) => format!("JSON salvato: {}", path.display()),
                    Err(error) => format!("Esportazione fallita: {error}"),
                };
            }
            Action::ToggleRecording => {
                if let Some(rec) = recorder.take() {
                    state.status = format!(
                        "Sessione salvata: {} ({} campioni)",
                        rec.path.display(),
                        rec.rows
                    );
                    state.recording = false;
                } else {
                    let path = export::unique_path(&export_dir, "session", "csv");
                    match Recorder::new(&path) {
                        Ok(rec) => {
                            state.status = format!("Registrazione: {}", path.display());
                            recorder = Some(rec);
                            state.recording = true;
                        }
                        Err(error) => state.status = format!("Registrazione fallita: {error}"),
                    }
                }
            }
            Action::RefreshLlm => {
                if no_llm {
                    state.status = "Sonde LLM disattivate da --no-llm".into();
                } else {
                    let _ = probe_tx.try_send(());
                    state.status = "Aggiornamento runtime richiesto…".into();
                }
            }
            Action::Benchmark => {
                let selected = llm_state
                    .runtimes
                    .iter()
                    .flat_map(|r| r.models.iter().map(move |m| (r, m)))
                    .nth(state.selected);
                if let Some((runtime, model)) = selected {
                    let runtime_name = match runtime.name.to_lowercase().as_str() {
                        "ollama" => "ollama",
                        "lm studio" | "lm-studio" => "lm-studio",
                        _ => "llama.cpp",
                    }
                    .to_string();
                    let endpoint = runtime.endpoint.clone();
                    let model = model.id.clone();
                    let tx = sender.clone();
                    let cancel = cancel.clone();
                    cancel.store(false, Ordering::Relaxed);
                    state.benchmark_running = true;
                    state.status = "Benchmark attivo | max 64 token | timeout 120 s".into();
                    thread::spawn(move || {
                        let result = llm::benchmark(&runtime_name, &endpoint, &model, 64, cancel)
                            .map_err(|e| format!("{e:#}"));
                        let _ = tx.send(Message::Benchmark(result));
                    });
                }
            }
            Action::CpuBenchmark => {
                let tx = sender.clone();
                let cancel = cancel.clone();
                cancel.store(false, Ordering::Relaxed);
                state.cpu_benchmark_running = true;
                state.status = "Benchmark CPU ~1,2 s | c annulla | continua a navigare".into();
                thread::spawn(move || {
                    let value = crate::bench::run(cancel).map_err(|e| format!("{e:#}"));
                    let _ = tx.send(Message::CpuBenchmark(value));
                });
            }
            Action::CancelBenchmark => {
                cancel.store(true, Ordering::Relaxed);
                state.status = "Annullamento benchmark richiesto…".into();
            }
            Action::None => {}
        }
        if !state.paused && snapshot.elapsed_secs != latest.elapsed_secs {
            snapshot = latest.clone();
        }
    }
    Ok(())
}

fn modal_open(state: &UiState) -> bool {
    state.help
        || state.filter_edit
        || state.benchmark_confirm
        || state.cpu_benchmark_confirm
        || state.process_detail
}

#[cfg(test)]
mod tests {
    use super::*;
    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }
    #[test]
    fn navigation_wraps_and_resets_selection() {
        let mut s = UiState {
            active_tab: 9,
            selected: 80,
            ..Default::default()
        };
        handle_key(key(KeyCode::Tab), &mut s, &Snapshot::default(), 0);
        assert_eq!((s.active_tab, s.selected), (0, 0));
        handle_key(key(KeyCode::BackTab), &mut s, &Snapshot::default(), 0);
        assert_eq!(s.active_tab, 9);
    }
    #[test]
    fn search_is_modal_and_unicode_safe() {
        let mut s = UiState::default();
        let snap = Snapshot::default();
        handle_key(key(KeyCode::Char('/')), &mut s, &snap, 0);
        for c in ['λ', 'q', 'é'] {
            assert_eq!(
                handle_key(key(KeyCode::Char(c)), &mut s, &snap, 0),
                Action::None
            );
        }
        assert_eq!(s.query, "λqé");
        handle_key(key(KeyCode::Backspace), &mut s, &snap, 0);
        assert_eq!(s.query, "λq");
        assert_eq!(
            handle_key(key(KeyCode::Esc), &mut s, &snap, 0),
            Action::None
        );
        assert!(!s.filter_edit);
    }
    #[test]
    fn benchmark_requires_confirmation_and_model() {
        let mut s = UiState {
            active_tab: 7,
            ..Default::default()
        };
        assert_eq!(
            handle_key(key(KeyCode::Char('b')), &mut s, &Snapshot::default(), 0),
            Action::None
        );
        assert!(!s.benchmark_confirm);
        handle_key(key(KeyCode::Char('b')), &mut s, &Snapshot::default(), 1);
        assert!(s.benchmark_confirm);
        assert_eq!(
            handle_key(key(KeyCode::Enter), &mut s, &Snapshot::default(), 1),
            Action::Benchmark
        );
    }
    #[test]
    fn interval_and_capacity_are_bounded() {
        let mut s = UiState::default();
        for _ in 0..100 {
            handle_key(key(KeyCode::Char('-')), &mut s, &Snapshot::default(), 0);
        }
        assert_eq!(s.tick_ms, 500);
        s.active_tab = 8;
        for _ in 0..100 {
            handle_key(key(KeyCode::Char('-')), &mut s, &Snapshot::default(), 0);
        }
        assert_eq!(s.capacity.parameters_b, 0.5);
    }
    #[test]
    fn cpu_benchmark_is_explicit_and_cancel_is_available() {
        let mut state = UiState {
            active_tab: 1,
            ..Default::default()
        };
        assert_eq!(
            handle_key(key(KeyCode::Char('x')), &mut state, &Snapshot::default(), 0),
            Action::None
        );
        assert!(state.cpu_benchmark_confirm);
        assert_eq!(
            handle_key(key(KeyCode::Enter), &mut state, &Snapshot::default(), 0),
            Action::CpuBenchmark
        );
        state.cpu_benchmark_running = true;
        assert_eq!(
            handle_key(key(KeyCode::Char('c')), &mut state, &Snapshot::default(), 0),
            Action::CancelBenchmark
        );
    }
    #[test]
    fn process_detail_keeps_the_selected_snapshot_when_sort_changes() {
        let mut state = UiState {
            active_tab: 6,
            ..Default::default()
        };
        let snapshot = Snapshot {
            processes: vec![
                ProcessRow {
                    pid: 7,
                    name: "first".into(),
                    cpu_percent: 90.0,
                    ..Default::default()
                },
                ProcessRow {
                    pid: 8,
                    name: "other".into(),
                    cpu_percent: 5.0,
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        handle_key(key(KeyCode::Enter), &mut state, &snapshot, 0);
        assert_eq!(state.process_detail_row.as_ref().unwrap().pid, 7);
        assert!(modal_open(&state));
        handle_key(key(KeyCode::Esc), &mut state, &snapshot, 0);
        assert!(!modal_open(&state));
    }
}
