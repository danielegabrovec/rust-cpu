use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Detail {
    pub label: String,
    pub value: String,
}
impl Detail {
    pub fn new(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Inventory {
    pub os: String,
    pub cpu_name: String,
    pub physical_cores: usize,
    pub logical_cores: usize,
    pub cpu: Vec<Detail>,
    pub board: Vec<Detail>,
    pub memory: Vec<Detail>,
    pub graphics: Vec<Detail>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ProcessRow {
    pub pid: u32,
    pub name: String,
    pub cpu_percent: f32,
    pub memory_bytes: u64,
    pub read_bytes_per_sec: f64,
    pub write_bytes_per_sec: f64,
    pub is_llm: bool,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DiskRow {
    pub name: String,
    pub mount: String,
    pub kind: String,
    pub total_bytes: u64,
    pub available_bytes: u64,
    pub read_bytes_per_sec: f64,
    pub write_bytes_per_sec: f64,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct NetworkRow {
    pub name: String,
    pub received_per_sec: f64,
    pub transmitted_per_sec: f64,
    pub total_received: u64,
    pub total_transmitted: u64,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct GpuRow {
    pub name: String,
    pub utilization: Option<f32>,
    pub memory_used: Option<u64>,
    pub memory_total: Option<u64>,
    pub temperature: Option<f32>,
    pub power_watts: Option<f32>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SensorRow {
    pub name: String,
    pub temperature: f32,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Snapshot {
    pub timestamp_unix: u64,
    pub elapsed_secs: f64,
    pub sampling_ms: u64,
    pub cpu_percent: f32,
    pub per_core: Vec<f32>,
    pub frequency_mhz: u64,
    pub memory_used: u64,
    pub memory_total: u64,
    pub swap_used: u64,
    pub swap_total: u64,
    pub processes: Vec<ProcessRow>,
    pub disks: Vec<DiskRow>,
    pub networks: Vec<NetworkRow>,
    pub gpus: Vec<GpuRow>,
    pub sensors: Vec<SensorRow>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub size_bytes: Option<u64>,
    pub vram_bytes: Option<u64>,
    pub context_length: Option<u64>,
    pub quantization: Option<String>,
    pub loaded: Option<bool>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RuntimeStatus {
    pub name: String,
    pub endpoint: String,
    pub status: String,
    pub message: String,
    pub models: Vec<ModelInfo>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct BenchmarkResult {
    pub runtime: String,
    pub model: String,
    pub output_tokens: Option<u64>,
    pub prompt_tokens: Option<u64>,
    pub wall_seconds: f64,
    pub generation_tokens_per_sec: Option<f64>,
    pub prompt_tokens_per_sec: Option<f64>,
    pub time_to_first_token_ms: Option<f64>,
    pub load_ms: Option<f64>,
    pub note: String,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LlmState {
    pub runtimes: Vec<RuntimeStatus>,
    pub benchmark: Option<BenchmarkResult>,
    pub last_probe_unix: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CapacitySettings {
    pub parameters_b: f64,
    pub bits_per_weight: f64,
    pub context: u64,
    pub layers: u64,
    pub kv_heads: u64,
    pub head_dim: u64,
}
impl Default for CapacitySettings {
    fn default() -> Self {
        Self {
            parameters_b: 8.0,
            bits_per_weight: 4.5,
            context: 8192,
            layers: 32,
            kv_heads: 8,
            head_dim: 128,
        }
    }
}
impl CapacitySettings {
    pub fn weights_bytes(&self) -> f64 {
        self.parameters_b * 1e9 * self.bits_per_weight / 8.0
    }
    pub fn kv_bytes(&self) -> f64 {
        2.0 * self.layers as f64
            * self.context as f64
            * self.kv_heads as f64
            * self.head_dim as f64
            * 2.0
    }
    pub fn estimated_bytes(&self) -> f64 {
        self.weights_bytes() * 1.15 + self.kv_bytes() + 512.0 * 1024.0 * 1024.0
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SortColumn {
    #[default]
    Cpu,
    Memory,
    Name,
    Pid,
}
impl SortColumn {
    pub fn next(self) -> Self {
        match self {
            Self::Cpu => Self::Memory,
            Self::Memory => Self::Name,
            Self::Name => Self::Pid,
            Self::Pid => Self::Cpu,
        }
    }
}
#[derive(Debug)]
pub struct UiState {
    pub process_detail_row: Option<ProcessRow>,
    pub cpu_benchmark: Option<crate::bench::CpuBenchmark>,
    pub cpu_benchmark_confirm: bool,
    pub cpu_benchmark_running: bool,
    pub active_tab: usize,
    pub selected: usize,
    pub offset: u16,
    pub query: String,
    pub filter_edit: bool,
    pub sort: SortColumn,
    pub ascending: bool,
    pub paused: bool,
    pub help: bool,
    pub theme_light: bool,
    pub status: String,
    pub capacity: CapacitySettings,
    pub capacity_field: usize,
    pub benchmark_confirm: bool,
    pub benchmark_running: bool,
    pub recording: bool,
    pub tick_ms: u64,
    pub process_detail: bool,
}
impl Default for UiState {
    fn default() -> Self {
        Self {
            process_detail_row: None,
            cpu_benchmark: None,
            cpu_benchmark_confirm: false,
            cpu_benchmark_running: false,
            active_tab: 0,
            selected: 0,
            offset: 0,
            query: String::new(),
            filter_edit: false,
            sort: SortColumn::Cpu,
            ascending: false,
            paused: false,
            help: false,
            theme_light: false,
            status: "Pronto | ? guida ai comandi".into(),
            capacity: CapacitySettings::default(),
            capacity_field: 0,
            benchmark_confirm: false,
            benchmark_running: false,
            recording: false,
            tick_ms: 1000,
            process_detail: false,
        }
    }
}

#[derive(Debug, Default)]
pub struct History {
    pub cpu: VecDeque<u64>,
    pub memory: VecDeque<u64>,
    pub gpu: VecDeque<u64>,
}
impl History {
    pub fn push(&mut self, s: &Snapshot) {
        for (q, v) in [
            (&mut self.cpu, s.cpu_percent as u64),
            (
                &mut self.memory,
                percent(s.memory_used, s.memory_total) as u64,
            ),
            (
                &mut self.gpu,
                s.gpus.first().and_then(|g| g.utilization).unwrap_or(0.0) as u64,
            ),
        ] {
            q.push_back(v);
            if q.len() > 180 {
                q.pop_front();
            }
        }
    }
}

pub fn percent(used: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        ((used as f64 / total as f64) * 100.0).clamp(0.0, 100.0)
    }
}
pub fn bytes(value: u64) -> String {
    let v = value as f64;
    if value >= 1 << 40 {
        format!("{:.1} TiB", v / (1u64 << 40) as f64)
    } else if value >= 1 << 30 {
        format!("{:.1} GiB", v / (1u64 << 30) as f64)
    } else if value >= 1 << 20 {
        format!("{:.1} MiB", v / (1u64 << 20) as f64)
    } else if value >= 1 << 10 {
        format!("{:.1} KiB", v / 1024.0)
    } else {
        format!("{value} B")
    }
}
pub const TABS: [&str; 10] = [
    "Overview",
    "CPU",
    "Mainboard",
    "Memoria",
    "GPU",
    "Dischi / Rete",
    "Processi",
    "LLM locali",
    "Capacity Lab",
    "Diagnostica",
];

pub fn filtered_processes<'a>(snapshot: &'a Snapshot, state: &UiState) -> Vec<&'a ProcessRow> {
    let query = state.query.to_lowercase();
    let mut rows: Vec<_> = snapshot
        .processes
        .iter()
        .filter(|p| {
            query.is_empty()
                || p.name.to_lowercase().contains(&query)
                || p.pid.to_string().contains(&query)
        })
        .collect();
    rows.sort_by(|a, b| {
        let ord = match state.sort {
            SortColumn::Cpu => a.cpu_percent.total_cmp(&b.cpu_percent),
            SortColumn::Memory => a.memory_bytes.cmp(&b.memory_bytes),
            SortColumn::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            SortColumn::Pid => a.pid.cmp(&b.pid),
        };
        let ord = if state.ascending { ord } else { ord.reverse() };
        ord.then(a.pid.cmp(&b.pid))
    });
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ratios_are_safe() {
        assert_eq!(percent(12, 0), 0.0);
        assert_eq!(percent(110, 100), 100.0);
    }
    #[test]
    fn capacity_accounts_for_gqa_kv() {
        let s = CapacitySettings::default();
        assert_eq!(s.kv_bytes(), 1073741824.0);
        assert!(s.estimated_bytes() > s.weights_bytes() + s.kv_bytes());
    }
    #[test]
    fn history_is_bounded() {
        let mut h = History::default();
        for _ in 0..1000 {
            h.push(&Snapshot::default());
        }
        assert_eq!(h.cpu.len(), 180);
    }
    #[test]
    fn search_and_sort_are_deterministic() {
        let s = Snapshot {
            processes: vec![
                ProcessRow {
                    pid: 2,
                    name: "ollama".into(),
                    cpu_percent: 20.0,
                    ..Default::default()
                },
                ProcessRow {
                    pid: 1,
                    name: "other".into(),
                    cpu_percent: 30.0,
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let mut u = UiState::default();
        assert_eq!(filtered_processes(&s, &u)[0].pid, 1);
        u.query = "OLL".into();
        assert_eq!(filtered_processes(&s, &u)[0].pid, 2);
        u.query = "no match".into();
        assert!(filtered_processes(&s, &u).is_empty());
    }
}
