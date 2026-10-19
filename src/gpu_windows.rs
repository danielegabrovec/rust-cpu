//! Windows WDDM telemetry, collected on a single background worker.
//!
//! PDH English counter paths remain independent of the Windows display language.
//! Private adapter LUIDs are used only for DXGI correlation and are not exported.
//! GPU usage = busiest engine; per-process dedicated/shared memory stay separate.
use crate::model::GpuRow;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, IDXGIFactory1,
};
use windows::Win32::System::Performance::{
    PDH_FMT, PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY, PDH_MORE_DATA,
    PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW,
    PdhOpenQueryW,
};
use windows::core::PCWSTR;

#[derive(Clone, Copy, Debug, Default)]
pub struct ProcessGpu {
    pub dedicated_bytes: Option<u64>,
    pub shared_bytes: Option<u64>,
    pub utilization: Option<f32>,
}
#[derive(Clone, Default)]
pub struct GpuTelemetry {
    pub gpus: Vec<GpuRow>,
    pub processes: HashMap<u32, ProcessGpu>,
    pub warnings: Vec<String>,
}
#[derive(Clone)]
struct Update {
    captured: Instant,
    value: GpuTelemetry,
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
        let shared = Arc::clone(SHARED.get_or_init(|| {
            let shared = Arc::new(Shared {
                latest: Mutex::new(None),
                subscribers: AtomicUsize::new(0),
            });
            let output = Arc::clone(&shared);
            std::thread::spawn(move || {
                let mut provider = match Provider::new() {
                    Ok(value) => value,
                    Err(message) => {
                        publish(
                            &output.latest,
                            GpuTelemetry {
                                warnings: vec![message],
                                ..Default::default()
                            },
                        );
                        return;
                    }
                };
                loop {
                    if output.subscribers.load(Ordering::Relaxed) == 0 {
                        provider.first = true;
                        std::thread::sleep(Duration::from_millis(100));
                        continue;
                    }
                    publish(&output.latest, provider.sample());
                    for _ in 0..20 {
                        if output.subscribers.load(Ordering::Relaxed) == 0 {
                            break;
                        }
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

    /// Never waits on PDH, DXGI, a provider lock, or a blocked driver.
    pub fn latest(&self) -> GpuTelemetry {
        let update = self
            .shared
            .latest
            .try_lock()
            .ok()
            .and_then(|value| value.clone());
        match update {
            Some(value) if value.captured.elapsed() <= Duration::from_secs(5) => value.value,
            Some(_) => GpuTelemetry {
                warnings: vec!["WDDM: campione scaduto (>5 s), carico e memoria GPU N/D.".into()],
                ..Default::default()
            },
            None => GpuTelemetry {
                warnings: vec![
                    if self.started.elapsed() > Duration::from_secs(4) {
                        "WDDM: inizializzazione oltre 4 s; provider isolato, dati GPU N/D."
                    } else {
                        "WDDM: inizializzazione / warmup dei contatori GPU."
                    }
                    .into(),
                ],
                ..Default::default()
            },
        }
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.shared.subscribers.fetch_sub(1, Ordering::Relaxed);
    }
}
fn publish(output: &Mutex<Option<Update>>, value: GpuTelemetry) {
    if let Ok(mut latest) = output.lock() {
        *latest = Some(Update {
            captured: Instant::now(),
            value,
        });
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Luid {
    high: u32,
    low: u32,
}
#[derive(Clone, Debug)]
struct Adapter {
    luid: Luid,
    name: String,
    dedicated_total: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Instance {
    pid: Option<u32>,
    luid: Luid,
    physical: u32,
    engine: Option<u32>,
}

fn instance_name(name: &str) -> Option<Instance> {
    let parts: Vec<_> = name.split('_').collect();
    let pos = parts.iter().position(|part| *part == "luid")?;
    let parse_hex = |s: &str| {
        u32::from_str_radix(s.strip_prefix("0x").or_else(|| s.strip_prefix("0X"))?, 16).ok()
    };
    let luid = Luid {
        high: parse_hex(parts.get(pos + 1)?)?,
        low: parse_hex(parts.get(pos + 2)?)?,
    };
    let decimal_after = |field: &str| -> Option<u32> {
        let i = parts.iter().position(|part| *part == field)?;
        parts.get(i + 1)?.split('#').next()?.parse().ok()
    };
    Some(Instance {
        pid: decimal_after("pid"),
        luid,
        physical: decimal_after("phys")?,
        engine: decimal_after("eng"),
    })
}

struct Query(PDH_HQUERY);
impl Drop for Query {
    fn drop(&mut self) {
        unsafe {
            PdhCloseQuery(self.0);
        }
    }
}
struct Provider {
    query: Query,
    adapters: Vec<Adapter>,
    engines: Option<PDH_HCOUNTER>,
    dedicated: Option<PDH_HCOUNTER>,
    shared: Option<PDH_HCOUNTER>,
    process_memory: Option<PDH_HCOUNTER>,
    process_shared: Option<PDH_HCOUNTER>,
    first: bool,
    warnings: Vec<String>,
}
impl Provider {
    fn new() -> Result<Self, String> {
        let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1() }
            .map_err(|_| "DXGI: enumerazione adattatori GPU non disponibile.".to_string())?;
        let mut adapters = Vec::new();
        for index in 0..32 {
            let Ok(adapter) = (unsafe { factory.EnumAdapters1(index) }) else {
                break;
            };
            let Ok(desc) = (unsafe { adapter.GetDesc1() }) else {
                continue;
            };
            if desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0 {
                continue;
            }
            let end = desc
                .Description
                .iter()
                .position(|c| *c == 0)
                .unwrap_or(desc.Description.len());
            adapters.push(Adapter {
                luid: Luid {
                    high: desc.AdapterLuid.HighPart as u32,
                    low: desc.AdapterLuid.LowPart,
                },
                name: String::from_utf16_lossy(&desc.Description[..end]),
                dedicated_total: desc
                    .DedicatedVideoMemory
                    .saturating_add(desc.DedicatedSystemMemory)
                    as u64,
            });
        }
        let mut query = PDH_HQUERY::default();
        let status = unsafe { PdhOpenQueryW(PCWSTR::null(), 0, &mut query) };
        if status != 0 {
            return Err(format!(
                "WDDM: apertura query PDH non riuscita (0x{status:08x})."
            ));
        }
        let query = Query(query);
        let mut warnings = Vec::new();
        let mut add = |path: &str| {
            let wide: Vec<u16> = path.encode_utf16().chain([0]).collect();
            let mut counter = PDH_HCOUNTER::default();
            let status =
                unsafe { PdhAddEnglishCounterW(query.0, PCWSTR(wide.as_ptr()), 0, &mut counter) };
            if status == 0 {
                Some(counter)
            } else {
                warnings.push(format!(
                    "WDDM: contatore {path} non disponibile (0x{status:08x})."
                ));
                None
            }
        };
        let engines = add(r"\GPU Engine(*)\Utilization Percentage");
        let dedicated = add(r"\GPU Adapter Memory(*)\Dedicated Usage");
        let shared = add(r"\GPU Adapter Memory(*)\Shared Usage");
        let process_memory = add(r"\GPU Process Memory(*)\Dedicated Usage");
        let process_shared = add(r"\GPU Process Memory(*)\Shared Usage");
        Ok(Self {
            query,
            adapters,
            engines,
            dedicated,
            shared,
            process_memory,
            process_shared,
            first: true,
            warnings,
        })
    }

    fn sample(&mut self) -> GpuTelemetry {
        let status = unsafe { PdhCollectQueryData(self.query.0) };
        let mut out = GpuTelemetry {
            warnings: self.warnings.clone(),
            ..Default::default()
        };
        let get = |counter: Option<PDH_HCOUNTER>| counter.and_then(|c| formatted_values(c).ok());
        let mut engines = if status == 0 && !self.first {
            get(self.engines)
        } else {
            None
        };
        let mut dedicated = if status == 0 {
            get(self.dedicated)
        } else {
            None
        };
        let mut shared = if status == 0 { get(self.shared) } else { None };
        let mut process_memory = if status == 0 {
            get(self.process_memory)
        } else {
            None
        };
        let mut process_shared = if status == 0 {
            get(self.process_shared)
        } else {
            None
        };
        // Some drivers expose NPUs through GPU Engine. Only DXGI GPU adapters
        // participate in these GPU fields; LUIDs remain internal correlation keys.
        for series in [
            &mut engines,
            &mut dedicated,
            &mut shared,
            &mut process_memory,
            &mut process_shared,
        ]
        .into_iter()
        .flatten()
        {
            series.retain(|(name, _)| {
                instance_name(name).is_some_and(|instance| {
                    self.adapters
                        .iter()
                        .any(|adapter| adapter.luid == instance.luid)
                })
            });
        }
        self.first = false;
        if status != 0 {
            out.warnings.push(format!(
                "WDDM: raccolta PDH non riuscita (0x{status:08x}); metriche N/D."
            ));
        }
        if engines.is_none() {
            out.warnings.push(
                "WDDM: carico GPU in warmup o contatore non disponibile; non equivale a 0%.".into(),
            );
        }
        if dedicated.is_none() || shared.is_none() {
            out.warnings.push("WDDM: uno o più contatori memoria GPU dedicata/condivisa non disponibili; campi N/D.".into());
        }
        if process_memory.is_none() || process_shared.is_none() {
            out.warnings
                .push("WDDM: memoria GPU per processo non disponibile; attribuzione N/D.".into());
        }
        let usage = aggregate_engines(engines.as_deref().unwrap_or_default());
        let dedicated = aggregate_memory(dedicated.as_deref().unwrap_or_default(), false);
        let shared = aggregate_memory(shared.as_deref().unwrap_or_default(), false);
        let process_memory = aggregate_memory(process_memory.as_deref().unwrap_or_default(), true);
        let process_shared = aggregate_memory(process_shared.as_deref().unwrap_or_default(), true);
        for adapter in &self.adapters {
            out.gpus.push(GpuRow {
                name: adapter.name.clone(),
                source: "Windows WDDM PDH/DXGI".into(),
                utilization: usage.adapter.get(&adapter.luid).copied(),
                memory_used: dedicated.adapters.get(&adapter.luid).copied(),
                memory_total: Some(adapter.dedicated_total),
                shared_memory_used: shared.adapters.get(&adapter.luid).copied(),
                ..Default::default()
            });
        }
        for (pid, value) in usage.process {
            out.processes.entry(pid).or_default().utilization = Some(value);
        }
        for (pid, value) in process_memory.process {
            out.processes.entry(pid).or_default().dedicated_bytes = Some(value);
        }
        for (pid, value) in process_shared.process {
            out.processes.entry(pid).or_default().shared_bytes = Some(value);
        }
        if !out.processes.is_empty() {
            out.warnings.push("WDDM processo: memoria dedicata attribuita, risorse condivise possono comparire in più PID; non sommare per ricavare la VRAM fisica.".into());
        }
        out
    }
}

/// PDH returns item structs and UTF-16 strings in one caller-owned buffer.
/// Allocation is 8-byte aligned, sizes and name pointers are validated before reads.
fn formatted_values(counter: PDH_HCOUNTER) -> Result<Vec<(String, f64)>, u32> {
    // PDH_FMT_NOCAP100 (Pdh.h): memory byte counters must never be capped to 100.
    // Engine percentages are bounded explicitly after aggregation instead.
    let format = PDH_FMT(PDH_FMT_DOUBLE.0 | 0x0000_8000);
    for _ in 0..3 {
        let mut bytes = 0_u32;
        let mut count = 0_u32;
        let status =
            unsafe { PdhGetFormattedCounterArrayW(counter, format, &mut bytes, &mut count, None) };
        if status == 0 && bytes == 0 {
            return Ok(Vec::new());
        }
        if status != PDH_MORE_DATA {
            return Err(status);
        }
        if bytes == 0 || bytes > 16 * 1024 * 1024 {
            return Err(PDH_MORE_DATA);
        }
        let mut buffer = vec![0_u64; (bytes as usize).div_ceil(8)];
        let status = unsafe {
            PdhGetFormattedCounterArrayW(
                counter,
                format,
                &mut bytes,
                &mut count,
                Some(buffer.as_mut_ptr().cast()),
            )
        };
        if status == PDH_MORE_DATA {
            continue;
        }
        if status != 0 {
            return Err(status);
        }
        let capacity = buffer.len() * 8;
        if count as usize > capacity / std::mem::size_of::<PDH_FMT_COUNTERVALUE_ITEM_W>() {
            return Err(PDH_MORE_DATA);
        }
        let items = unsafe {
            std::slice::from_raw_parts(
                buffer.as_ptr().cast::<PDH_FMT_COUNTERVALUE_ITEM_W>(),
                count as usize,
            )
        };
        let begin = buffer.as_ptr() as usize;
        let end = begin + capacity;
        let mut result = Vec::with_capacity(items.len());
        for item in items {
            if !matches!(item.FmtValue.CStatus, 0 | 1) {
                continue;
            }
            let pointer = item.szName.0 as usize;
            if pointer < begin || pointer >= end || !pointer.is_multiple_of(2) {
                continue;
            }
            let max_len = ((end - pointer) / 2).min(2048);
            let name = unsafe { std::slice::from_raw_parts(item.szName.0, max_len) };
            let Some(length) = name.iter().position(|ch| *ch == 0) else {
                continue;
            };
            let value = unsafe { item.FmtValue.Anonymous.doubleValue };
            if value.is_finite() && value >= 0.0 {
                result.push((String::from_utf16_lossy(&name[..length]), value));
            }
        }
        return Ok(result);
    }
    Err(PDH_MORE_DATA)
}

#[derive(Default)]
struct EngineUsage {
    adapter: HashMap<Luid, f32>,
    process: HashMap<u32, f32>,
}
fn aggregate_engines(values: &[(String, f64)]) -> EngineUsage {
    let mut instances: HashMap<Instance, f64> = HashMap::new();
    for (name, value) in values {
        if !value.is_finite() || *value < 0.0 {
            continue;
        }
        if let Some(key) = instance_name(name).filter(|i| i.pid.is_some() && i.engine.is_some()) {
            instances
                .entry(key)
                .and_modify(|v| *v = v.max(*value))
                .or_insert(*value);
        }
    }
    let mut engines: HashMap<(Luid, u32, u32), f64> = HashMap::new();
    let mut out = EngineUsage::default();
    for (key, value) in instances {
        let value = value.clamp(0.0, 100.0) as f32;
        out.process
            .entry(key.pid.unwrap())
            .and_modify(|v| *v = v.max(value))
            .or_insert(value);
        *engines
            .entry((key.luid, key.physical, key.engine.unwrap()))
            .or_default() += value as f64;
    }
    for ((luid, _, _), value) in engines {
        let value = value.clamp(0.0, 100.0) as f32;
        out.adapter
            .entry(luid)
            .and_modify(|v| *v = v.max(value))
            .or_insert(value);
    }
    out
}
#[derive(Default)]
struct MemoryUsage {
    adapters: HashMap<Luid, u64>,
    process: HashMap<u32, u64>,
}
fn aggregate_memory(values: &[(String, f64)], per_process: bool) -> MemoryUsage {
    let mut unique: HashMap<Instance, u64> = HashMap::new();
    for (name, value) in values {
        if !value.is_finite() || *value < 0.0 || *value > u64::MAX as f64 {
            continue;
        }
        let Some(mut key) = instance_name(name) else {
            continue;
        };
        if per_process && key.pid.is_none() {
            continue;
        }
        key.engine = None;
        unique
            .entry(key)
            .and_modify(|v| *v = (*v).max(*value as u64))
            .or_insert(*value as u64);
    }
    let mut out = MemoryUsage::default();
    for (key, bytes) in unique {
        if per_process {
            let value = out.process.entry(key.pid.unwrap()).or_default();
            *value = value.saturating_add(bytes);
        } else {
            let value = out.adapters.entry(key.luid).or_default();
            *value = value.saturating_add(bytes);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    fn row(pid: u32, engine: u32, value: f64) -> (String, f64) {
        (
            format!("pid_{pid}_luid_0x00000001_0x12345678_phys_0_eng_{engine}_engtype_Compute"),
            value,
        )
    }
    #[test]
    fn parses_luid_without_exporting_identifiers() {
        let i =
            instance_name("pid_123_luid_0x80000000_0x0011aabb_phys_0_eng_2_engtype_3D#1").unwrap();
        assert_eq!(i.pid, Some(123));
        assert_eq!(i.luid.high, 0x80000000);
        assert_eq!(i.luid.low, 0x11aabb);
        assert_eq!(i.engine, Some(2));
        assert!(instance_name("GPU0").is_none());
    }
    #[test]
    fn busiest_engine_sums_pids_but_never_sums_different_engines() {
        let usage = aggregate_engines(&[
            row(1, 0, 30.0),
            row(2, 0, 40.0),
            row(1, 1, 90.0),
            row(2, 1, 0.0),
        ]);
        assert_eq!(usage.adapter.values().next(), Some(&90.0));
        assert_eq!(usage.process.get(&1), Some(&90.0));
        assert_eq!(usage.process.get(&2), Some(&40.0));
    }
    #[test]
    fn duplicates_invalid_and_missing_are_not_double_counted_or_zero_filled() {
        let usage = aggregate_engines(&[
            row(1, 0, 75.0),
            row(1, 0, 75.0),
            row(2, 0, 80.0),
            row(3, 1, f64::NAN),
        ]);
        assert_eq!(usage.adapter.values().next(), Some(&100.0));
        assert_eq!(usage.process.get(&3), None);
        assert!(aggregate_engines(&[]).adapter.is_empty());
        let zero = aggregate_engines(&[row(9, 0, 0.0)]);
        assert_eq!(zero.process.get(&9), Some(&0.0));
    }
    #[test]
    fn memory_is_deduplicated_per_physical_adapter_and_pid() {
        let values = vec![
            ("pid_7_luid_0x0_0x1_phys_0".into(), 1024.0),
            ("pid_7_luid_0x0_0x1_phys_0#1".into(), 1024.0),
            ("pid_7_luid_0x0_0x2_phys_0".into(), 2048.0),
        ];
        assert_eq!(aggregate_memory(&values, true).process.get(&7), Some(&3072));
        assert!(aggregate_memory(&[], false).adapters.is_empty());
    }
}
