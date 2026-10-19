use crate::model::{CapacitySettings, Snapshot, SortColumn, UiState, percent};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct AlertThresholds {
    pub enabled: bool,
    pub cpu_percent: f64,
    pub ram_percent: f64,
    pub vram_percent: f64,
    pub gpu_temperature_c: f64,
    pub cpu_temperature_c: f64,
    pub cooldown_seconds: u64,
}
impl Default for AlertThresholds {
    fn default() -> Self {
        Self {
            enabled: true,
            cpu_percent: 95.0,
            ram_percent: 90.0,
            vram_percent: 95.0,
            gpu_temperature_c: 85.0,
            cpu_temperature_c: 90.0,
            cooldown_seconds: 30,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub theme_light: bool,
    pub sort: SortColumn,
    pub ascending: bool,
    pub interval_ms: u64,
    pub capacity: CapacitySettings,
    pub alerts: AlertThresholds,
    pub benchmark_repeats: u32,
    pub ollama_url: String,
    pub lm_studio_url: String,
    pub llama_cpp_url: String,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            theme_light: false,
            sort: SortColumn::Cpu,
            ascending: false,
            interval_ms: 1000,
            capacity: CapacitySettings::default(),
            alerts: AlertThresholds::default(),
            benchmark_repeats: 3,
            ollama_url: "http://127.0.0.1:11434".into(),
            lm_studio_url: "http://127.0.0.1:1234".into(),
            llama_cpp_url: "http://127.0.0.1:8080".into(),
        }
    }
}
impl Preferences {
    pub fn validate(&mut self) {
        self.interval_ms = self.interval_ms.clamp(500, 10000);
        self.benchmark_repeats = self.benchmark_repeats.clamp(1, 10);
        for value in [
            &mut self.alerts.cpu_percent,
            &mut self.alerts.ram_percent,
            &mut self.alerts.vram_percent,
        ] {
            if !value.is_finite() {
                *value = 90.0;
            }
            *value = value.clamp(1.0, 100.0);
        }
        for value in [
            &mut self.alerts.gpu_temperature_c,
            &mut self.alerts.cpu_temperature_c,
        ] {
            if !value.is_finite() {
                *value = 85.0;
            }
            *value = value.clamp(30.0, 120.0);
        }
        self.alerts.cooldown_seconds = self.alerts.cooldown_seconds.clamp(5, 3600);
        let c = &mut self.capacity;
        if !c.parameters_b.is_finite() {
            c.parameters_b = 8.0;
        }
        if !c.bits_per_weight.is_finite() {
            c.bits_per_weight = 4.5;
        }
        c.parameters_b = c.parameters_b.clamp(0.5, 1000.0);
        c.bits_per_weight = c.bits_per_weight.clamp(1.0, 32.0);
        c.context = c.context.clamp(1024, 1048576);
        c.layers = c.layers.clamp(1, 256);
        c.kv_heads = c.kv_heads.clamp(1, 256);
        c.head_dim = c.head_dim.clamp(16, 1024);
    }
    pub fn apply(&self, state: &mut UiState) {
        state.theme_light = self.theme_light;
        state.sort = self.sort;
        state.ascending = self.ascending;
        state.tick_ms = self.interval_ms;
        state.capacity = self.capacity.clone();
    }
    pub fn capture(state: &UiState) -> Self {
        let mut p = state.workspace.preferences.clone();
        p.theme_light = state.theme_light;
        p.sort = state.sort;
        p.ascending = state.ascending;
        p.interval_ms = state.tick_ms;
        p.capacity = state.capacity.clone();
        p.validate();
        p
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AlertEvent {
    pub timestamp_unix: u64,
    pub kind: String,
    pub message: String,
    pub value: f64,
    pub threshold: f64,
}
#[derive(Default)]
pub struct AlertEngine {
    last: HashMap<String, u64>,
}
impl AlertEngine {
    pub fn evaluate(&mut self, s: &Snapshot, t: &AlertThresholds) -> Vec<AlertEvent> {
        if !t.enabled {
            return Vec::new();
        }
        let mut values = Vec::new();
        if !s.per_core.is_empty() {
            values.push((
                "cpu".to_string(),
                "Carico CPU".to_string(),
                s.cpu_percent as f64,
                t.cpu_percent,
                "%",
            ));
        }
        if s.memory_total > 0 {
            values.push((
                "ram".into(),
                "Memoria RAM".into(),
                percent(s.memory_used, s.memory_total),
                t.ram_percent,
                "%",
            ));
        }
        for (i, g) in s.gpus.iter().enumerate() {
            if let (Some(u), Some(total)) = (g.memory_used, g.memory_total)
                && total > 0
            {
                values.push((
                    format!("vram-{i}"),
                    format!("VRAM {}", g.name),
                    percent(u, total),
                    t.vram_percent,
                    "%",
                ));
            }
            if let Some(temp) = g.temperature {
                values.push((
                    format!("gpu-temp-{i}"),
                    format!("Temperatura {}", g.name),
                    f64::from(temp),
                    t.gpu_temperature_c,
                    "°C",
                ));
            }
        }
        for (i, sensor) in s.sensors.iter().enumerate() {
            let name = sensor.name.to_lowercase();
            if name.contains("cpu") || name.contains("package") || name.contains("tctl") {
                values.push((
                    format!("cpu-temp-{i}"),
                    sensor.name.clone(),
                    f64::from(sensor.temperature),
                    t.cpu_temperature_c,
                    "°C",
                ));
            }
        }
        let mut result = Vec::new();
        for (kind, label, value, threshold, unit) in values {
            if value.is_finite()
                && value >= threshold
                && self
                    .last
                    .get(&kind)
                    .is_none_or(|last| s.timestamp_unix.saturating_sub(*last) >= t.cooldown_seconds)
            {
                self.last.insert(kind.clone(), s.timestamp_unix);
                result.push(AlertEvent {
                    timestamp_unix: s.timestamp_unix,
                    kind,
                    message: format!("{label}: {value:.1}{unit} (soglia {threshold:.1}{unit})"),
                    value,
                    threshold,
                });
            }
        }
        result
    }
}
pub const SETTINGS_FIELDS: [&str; 9] = [
    "Tema chiaro",
    "Intervallo ms",
    "Avvisi attivi",
    "CPU soglia %",
    "RAM soglia %",
    "VRAM soglia %",
    "GPU soglia °C",
    "CPU soglia °C",
    "Ripetizioni benchmark",
];
pub fn adjust(state: &mut UiState, increase: bool) {
    let f = state.workspace.settings_field;
    let d = if increase { 1.0 } else { -1.0 };
    match f {
        0 => state.theme_light = !state.theme_light,
        1 => {
            state.tick_ms =
                (state.tick_ms as i64 + if increase { 500 } else { -500 }).clamp(500, 10000) as u64
        }
        2 => {
            state.workspace.preferences.alerts.enabled = !state.workspace.preferences.alerts.enabled
        }
        3 => state.workspace.preferences.alerts.cpu_percent += d,
        4 => state.workspace.preferences.alerts.ram_percent += d,
        5 => state.workspace.preferences.alerts.vram_percent += d,
        6 => state.workspace.preferences.alerts.gpu_temperature_c += d,
        7 => state.workspace.preferences.alerts.cpu_temperature_c += d,
        _ => {
            let v = state.workspace.preferences.benchmark_repeats as i32
                + if increase { 1 } else { -1 };
            state.workspace.preferences.benchmark_repeats = v.clamp(1, 10) as u32;
        }
    }
    state.workspace.preferences = Preferences::capture(state);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_sensors_do_not_alert_and_cooldown_is_real() {
        let mut e = AlertEngine::default();
        let t = AlertThresholds::default();
        assert!(e.evaluate(&Snapshot::default(), &t).is_empty());
        let mut s = Snapshot {
            timestamp_unix: 100,
            cpu_percent: 99.0,
            per_core: vec![99.0],
            ..Default::default()
        };
        assert_eq!(e.evaluate(&s, &t).len(), 1);
        s.timestamp_unix = 110;
        assert!(e.evaluate(&s, &t).is_empty());
        s.timestamp_unix = 131;
        assert_eq!(e.evaluate(&s, &t).len(), 1);
    }
    #[test]
    fn preferences_validate_and_roundtrip() {
        let mut p = Preferences {
            interval_ms: 1,
            benchmark_repeats: 999,
            ..Default::default()
        };
        p.validate();
        assert_eq!((p.interval_ms, p.benchmark_repeats), (500, 10));
        let v = serde_json::to_string(&p).unwrap();
        let p: Preferences = serde_json::from_str(&v).unwrap();
        assert_eq!(p.interval_ms, 500);
    }
}
