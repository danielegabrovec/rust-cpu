use crate::{
    settings::{AlertEvent, Preferences},
    storage::{SavedBenchmark, SessionComparison, SessionData, SessionSummary},
};

#[derive(Debug, Default)]
pub struct WorkspaceUi {
    pub repeat_requested: bool,
    pub sessions: Vec<SessionSummary>,
    pub loaded: Option<SessionData>,
    pub comparison: Option<SessionComparison>,
    pub baseline_id: Option<i64>,
    pub candidate_id: Option<i64>,
    pub active_session: Option<i64>,
    pub replay_index: usize,
    pub loading: bool,
    pub name_edit: Option<String>,
    pub delete_confirm: bool,
    pub db_path: String,
    pub preferences: Preferences,
    pub settings_field: usize,
    pub alerts: Vec<AlertEvent>,
    pub benchmark_history: Vec<SavedBenchmark>,
    pub last_error: Option<String>,
}
