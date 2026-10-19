//! Optional read-only consumer of an already-running LibreHardwareMonitor WMI provider.
//! No driver, service, executable, firmware query or write is installed or started.
use crate::model::{Detail, SensorRow};
use serde::Deserialize;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

#[derive(Clone, Default)]
pub struct Reading {
    pub temperatures: Vec<SensorRow>,
    pub details: Vec<Detail>,
    pub warnings: Vec<String>,
}
#[derive(Clone)]
struct Update {
    sampled: Instant,
    value: Reading,
}
struct Shared {
    latest: Mutex<Option<Update>>,
    subscribers: AtomicUsize,
}
static SHARED: OnceLock<Arc<Shared>> = OnceLock::new();
pub struct Worker {
    shared: Arc<Shared>,
    started: Instant,
}
impl Default for Worker {
    fn default() -> Self {
        Self::new()
    }
}
impl Worker {
    pub fn new() -> Self {
        // One process-wide worker: repeated Collector creation cannot accumulate
        // blocked COM calls. With no subscribers it performs no provider queries.
        let shared = Arc::clone(SHARED.get_or_init(|| {
            let shared = Arc::new(Shared { latest: Mutex::new(None), subscribers: AtomicUsize::new(0) });
            let output = Arc::clone(&shared);
            std::thread::spawn(move || {
            // COM and the WMI connection live only on this worker thread.
            loop {
                if output.subscribers.load(Ordering::Relaxed) == 0 {
                    std::thread::sleep(Duration::from_millis(100));
                    continue;
                }
                let reading = match wmi::WMIConnection::with_namespace_path(r"ROOT\LibreHardwareMonitor") {
                    Ok(connection) => match connection.raw_query::<RawSensor>("SELECT Name, SensorType, Value, Parent FROM Sensor WHERE SensorType = 'Temperature' OR SensorType = 'Voltage'") {
                        Ok(values) => convert(values),
                        Err(_) => Reading { warnings: vec!["LibreHardwareMonitor: namespace presente ma lettura sensori non disponibile.".into()], ..Default::default() },
                    },
                    Err(_) => Reading { warnings: vec!["Sensori avanzati opzionali: LibreHardwareMonitor WMI non attivo; nessun driver avviato da RUST-CPU.".into()], ..Default::default() },
                };
                let active = !reading.temperatures.is_empty() || !reading.details.is_empty();
                if let Ok(mut state) = output.latest.lock() {
                    *state = Some(Update {
                        sampled: Instant::now(),
                        value: reading,
                    });
                }
                for _ in 0..if active { 40 } else { 300 } {
                    if output.subscribers.load(Ordering::Relaxed) == 0 { break; }
                    std::thread::sleep(Duration::from_millis(50));
                }
            }
            });
            shared
        }));
        shared.subscribers.fetch_add(1, Ordering::Relaxed);
        Self {
            shared,
            started: Instant::now(),
        }
    }
    /// A blocked WMI provider cannot hold up a collector or UI frame.
    pub fn latest(&self) -> Reading {
        let value = self
            .shared
            .latest
            .try_lock()
            .ok()
            .and_then(|state| state.clone());
        match value {
            Some(value) if value.value.temperatures.is_empty() && value.value.details.is_empty() => value.value,
            Some(value) if value.sampled.elapsed() <= Duration::from_secs(4) => value.value,
            Some(_) => Reading { warnings: vec!["LibreHardwareMonitor: sensori scaduti (>4 s) / timeout WMI; valori omessi.".into()], ..Default::default() },
            None => Reading { warnings: vec![if self.started.elapsed() > Duration::from_secs(4) { "LibreHardwareMonitor: limite di attesa 4 s superato; sensori N/D, worker isolato." } else { "LibreHardwareMonitor: rilevamento provider sensori opzionale in corso." }.into()], ..Default::default() },
        }
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.shared.subscribers.fetch_sub(1, Ordering::Relaxed);
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RawSensor {
    name: Option<String>,
    sensor_type: Option<String>,
    value: Option<f32>,
    parent: Option<String>,
}
fn convert(values: Vec<RawSensor>) -> Reading {
    let mut out = Reading::default();
    for sensor in values {
        let Some(value) = sensor.value.filter(|value| value.is_finite()) else {
            continue;
        };
        let parent = sensor.parent.unwrap_or_default().to_ascii_lowercase();
        let hardware = if parent.contains("cpu") {
            "CPU"
        } else if parent.contains("gpu") {
            "GPU"
        } else {
            "Hardware"
        };
        let name = sensor
            .name
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| "Sensore".into());
        match sensor.sensor_type.as_deref() {
            Some("Temperature") => out.temperatures.push(SensorRow {
                name: format!("LHM · {hardware} · {name}"),
                temperature: value,
            }),
            Some("Voltage") => out.details.push(Detail::new(
                format!("LHM · {hardware} · {name}"),
                format!("{value:.3} V (lettura live)"),
            )),
            _ => {}
        }
    }
    if out.temperatures.is_empty() && out.details.is_empty() {
        out.warnings.push(
            "LibreHardwareMonitor attivo ma nessuna temperatura/tensione valida esposta.".into(),
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn temperatures_and_voltages_keep_units_and_source_separate() {
        let values = vec![
            RawSensor {
                name: Some("Package".into()),
                sensor_type: Some("Temperature".into()),
                value: Some(62.5),
                parent: Some("/amdcpu/0".into()),
            },
            RawSensor {
                name: Some("Core".into()),
                sensor_type: Some("Voltage".into()),
                value: Some(1.125),
                parent: Some("/amdcpu/0".into()),
            },
        ];
        let reading = convert(values);
        assert_eq!(reading.temperatures.len(), 1);
        assert_eq!(reading.temperatures[0].temperature, 62.5);
        assert!(reading.temperatures[0].name.starts_with("LHM · CPU"));
        assert_eq!(reading.details.len(), 1);
        assert!(reading.details[0].value.contains("1.125 V"));
    }
    #[test]
    fn absent_nan_and_unknown_types_are_not_fabricated() {
        let values = vec![
            RawSensor {
                name: None,
                sensor_type: Some("Temperature".into()),
                value: None,
                parent: None,
            },
            RawSensor {
                name: None,
                sensor_type: Some("Voltage".into()),
                value: Some(f32::NAN),
                parent: None,
            },
            RawSensor {
                name: None,
                sensor_type: Some("Fan".into()),
                value: Some(1200.0),
                parent: None,
            },
        ];
        let reading = convert(values);
        assert!(reading.temperatures.is_empty());
        assert!(reading.details.is_empty());
        assert!(!reading.warnings.is_empty());
    }
}
