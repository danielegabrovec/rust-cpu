use crate::model::*;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use std::path::PathBuf;

#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    ReloadArchive,
    LoadSession,
    CompareSessions,
    RenameSession,
    DeleteSession,
    ReturnLive,
    BackupDatabase,
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
    if state.workspace.name_edit.is_some() {
        match key.code {
            KeyCode::Esc => state.workspace.name_edit = None,
            KeyCode::Enter => return Action::RenameSession,
            KeyCode::Backspace => {
                state.workspace.name_edit.as_mut().unwrap().pop();
            }
            KeyCode::Char(c) if !c.is_control() => {
                let name = state.workspace.name_edit.as_mut().unwrap();
                if name.chars().count() < 120 {
                    name.push(c);
                }
            }
            _ => {}
        }
        return Action::None;
    }
    if state.workspace.delete_confirm {
        return match key.code {
            KeyCode::Enter => {
                state.workspace.delete_confirm = false;
                Action::DeleteSession
            }
            KeyCode::Esc | KeyCode::Char('q') => {
                state.workspace.delete_confirm = false;
                Action::None
            }
            _ => Action::None,
        };
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
        if key.code == KeyCode::Char('q') {
            return Action::Quit;
        }
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
        KeyCode::F(2) => {
            state.active_tab = 10;
            action = Action::ReloadArchive;
        }
        KeyCode::F(3) => {
            state.active_tab = 11;
            action = Action::CompareSessions;
        }
        KeyCode::F(4) => state.active_tab = 12,
        KeyCode::F(6) => action = Action::BackupDatabase,
        KeyCode::Char('l') => action = Action::ReturnLive,
        KeyCode::Esc if state.workspace.loaded.is_some() => action = Action::ReturnLive,
        KeyCode::Up | KeyCode::Char('k') if state.active_tab == 12 => {
            state.workspace.settings_field = state.workspace.settings_field.saturating_sub(1)
        }
        KeyCode::Down | KeyCode::Char('j') if state.active_tab == 12 => {
            state.workspace.settings_field =
                (state.workspace.settings_field + 1).min(crate::settings::SETTINGS_FIELDS.len() - 1)
        }
        KeyCode::Home if state.active_tab == 12 => state.workspace.settings_field = 0,
        KeyCode::End if state.active_tab == 12 => {
            state.workspace.settings_field = crate::settings::SETTINGS_FIELDS.len() - 1
        }
        KeyCode::Enter if state.active_tab == 10 => action = Action::LoadSession,
        KeyCode::Char('a') if state.active_tab == 10 => {
            state.workspace.baseline_id =
                state.workspace.sessions.get(state.selected).map(|s| s.id);
            state.status = "Baseline A selezionata; scegli B con b, poi c confronta".into();
        }
        KeyCode::Char('b') if state.active_tab == 10 => {
            state.workspace.candidate_id =
                state.workspace.sessions.get(state.selected).map(|s| s.id);
            state.status = "Sessione B selezionata; c apre il confronto".into();
        }
        KeyCode::Char('c') if state.active_tab == 10 || state.active_tab == 11 => {
            action = Action::CompareSessions
        }
        KeyCode::Char('n') if state.active_tab == 10 => {
            state.workspace.name_edit = state
                .workspace
                .sessions
                .get(state.selected)
                .map(|s| s.name.clone());
        }
        KeyCode::Delete if state.active_tab == 10 => {
            if state
                .workspace
                .sessions
                .get(state.selected)
                .is_some_and(|s| Some(s.id) != state.workspace.active_session)
            {
                state.workspace.delete_confirm = true;
            } else {
                state.status =
                    "Ferma la registrazione prima di eliminare la sessione attiva".into();
            }
        }
        KeyCode::Char('[') if state.active_tab != 8 && state.workspace.loaded.is_some() => {
            state.workspace.replay_index = state.workspace.replay_index.saturating_sub(1);
        }
        KeyCode::Char(']') if state.active_tab != 8 && state.workspace.loaded.is_some() => {
            let n = state
                .workspace
                .loaded
                .as_ref()
                .map(|s| s.samples.len())
                .unwrap_or(0);
            state.workspace.replay_index =
                (state.workspace.replay_index + 1).min(n.saturating_sub(1));
        }
        KeyCode::Char('q') | KeyCode::Esc => action = Action::Quit,
        KeyCode::Char('?') | KeyCode::F(1) => state.help = true,
        KeyCode::Tab | KeyCode::Right => state.active_tab = (state.active_tab + 1) % TABS.len(),
        KeyCode::BackTab | KeyCode::Left => {
            state.active_tab = (state.active_tab + TABS.len() - 1) % TABS.len()
        }
        KeyCode::Char(c @ '1'..='9') => state.active_tab = c.to_digit(10).unwrap() as usize - 1,
        KeyCode::Char('0') => state.active_tab = 9,
        KeyCode::Up | KeyCode::Char('k') if state.active_tab == 7 => {
            state.selected = state.selected.saturating_sub(1)
        }
        KeyCode::Down | KeyCode::Char('j') if state.active_tab == 7 => {
            state.selected = state.selected.saturating_add(1)
        }
        KeyCode::PageUp if state.active_tab == 7 => state.offset = state.offset.saturating_sub(10),
        KeyCode::PageDown if state.active_tab == 7 => {
            state.offset = state.offset.saturating_add(10)
        }
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
        KeyCode::Char('x' | 'X') if state.active_tab == 1 => {
            if state.workspace.loaded.is_some() {
                state.status = "Premi l per tornare live prima di avviare un benchmark".into();
                return Action::None;
            }
            if !state.benchmark_running && !state.cpu_benchmark_running {
                state.workspace.repeat_requested = key.code == KeyCode::Char('X');
                state.cpu_benchmark_confirm = true;
            }
        }
        KeyCode::Char('b' | 'B') if state.active_tab == 7 => {
            if state.benchmark_running || state.cpu_benchmark_running {
                state.status = "Benchmark in corso; c annulla, q esce".into();
            } else if model_count == 0 {
                state.status =
                    "Nessun modello disponibile. Avvia un runtime locale e premi F5".into();
            } else {
                state.workspace.repeat_requested = key.code == KeyCode::Char('B');
                state.benchmark_confirm = true;
            }
        }
        KeyCode::Enter if state.active_tab == 6 => {
            state.process_detail_row = filtered_processes(snapshot, state)
                .get(state.selected)
                .map(|p| (*p).clone());
            state.process_detail = state.process_detail_row.is_some();
        }
        KeyCode::F(5) if state.active_tab >= 10 => action = Action::ReloadArchive,
        KeyCode::F(5) => action = Action::RefreshLlm,
        KeyCode::Char('[') if state.active_tab == 8 => {
            state.capacity_field = (state.capacity_field + 5) % 6
        }
        KeyCode::Char(']') if state.active_tab == 8 => {
            state.capacity_field = (state.capacity_field + 1) % 6
        }
        KeyCode::Char('+') | KeyCode::Char('=') => {
            if state.active_tab == 12 {
                crate::settings::adjust(state, true);
            } else if state.active_tab == 8 {
                adjust_capacity(state, true);
            } else {
                state.tick_ms = (state.tick_ms + 500).min(10000);
            }
        }
        KeyCode::Char('-') => {
            if state.active_tab == 12 {
                crate::settings::adjust(state, false);
            } else if state.active_tab == 8 {
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
        10 => state.workspace.sessions.len(),
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

pub use crate::app_runtime::run;

pub(crate) fn modal_open(state: &UiState) -> bool {
    state.help
        || state.filter_edit
        || state.benchmark_confirm
        || state.cpu_benchmark_confirm
        || state.process_detail
        || state.workspace.name_edit.is_some()
        || state.workspace.delete_confirm
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
            active_tab: TABS.len() - 1,
            selected: 80,
            ..Default::default()
        };
        handle_key(key(KeyCode::Tab), &mut s, &Snapshot::default(), 0);
        assert_eq!((s.active_tab, s.selected), (0, 0));
        handle_key(key(KeyCode::BackTab), &mut s, &Snapshot::default(), 0);
        assert_eq!(s.active_tab, TABS.len() - 1);
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
    #[test]
    fn workspace_keys_rename_confirmation_and_help_are_consistent() {
        let mut s = UiState {
            help: true,
            ..Default::default()
        };
        assert_eq!(
            handle_key(key(KeyCode::Char('q')), &mut s, &Snapshot::default(), 0),
            Action::Quit
        );
        s.help = false;
        assert_eq!(
            handle_key(key(KeyCode::F(2)), &mut s, &Snapshot::default(), 0),
            Action::ReloadArchive
        );
        assert_eq!(s.active_tab, 10);
        s.workspace.sessions.push(crate::storage::SessionSummary {
            id: 7,
            name: "A".into(),
            ..Default::default()
        });
        handle_key(key(KeyCode::Char('a')), &mut s, &Snapshot::default(), 0);
        handle_key(key(KeyCode::Char('b')), &mut s, &Snapshot::default(), 0);
        assert_eq!(
            (s.workspace.baseline_id, s.workspace.candidate_id),
            (Some(7), Some(7))
        );
        handle_key(key(KeyCode::Char('n')), &mut s, &Snapshot::default(), 0);
        assert!(modal_open(&s));
        assert_eq!(
            handle_key(key(KeyCode::Char('q')), &mut s, &Snapshot::default(), 0),
            Action::None
        );
        assert_eq!(s.workspace.name_edit.as_deref(), Some("Aq"));
        handle_key(key(KeyCode::Esc), &mut s, &Snapshot::default(), 0);
        handle_key(key(KeyCode::Delete), &mut s, &Snapshot::default(), 0);
        assert!(s.workspace.delete_confirm);
        assert_eq!(
            handle_key(key(KeyCode::Enter), &mut s, &Snapshot::default(), 0),
            Action::DeleteSession
        );
        s.active_tab = 7;
        s.selected = 4;
        s.offset = 15;
        handle_key(key(KeyCode::PageDown), &mut s, &Snapshot::default(), 20);
        assert_eq!(s.selected, 4);
        assert_eq!(s.offset, 25);
    }
}
