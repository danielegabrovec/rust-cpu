//! Local, read-only telemetry. No host identifiers, process arguments or serial numbers.
use crate::model::{
    Detail, DiskRow, GpuRow, Inventory, NetworkRow, ProcessRow, SensorRow, Snapshot, bytes,
};
use std::collections::HashMap;
use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::{OnceLock, mpsc};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use sysinfo::{
    Components, CpuRefreshKind, Disks, Networks, ProcessRefreshKind, ProcessesToUpdate, System,
};

#[derive(Clone, Default)]
struct Hardware {
    inventory: Inventory,
    gpu_names: Vec<String>,
}
static HARDWARE: OnceLock<Hardware> = OnceLock::new();

/// Inventory is cached: WMI is never queried on the sampling path.
pub fn inventory() -> Inventory {
    HARDWARE.get_or_init(collect_inventory).inventory.clone()
}

fn collect_inventory() -> Hardware {
    let mut system = System::new();
    system.refresh_cpu_specifics(CpuRefreshKind::nothing().with_frequency());
    system.refresh_memory();
    let mut hw = Hardware::default();
    let inv = &mut hw.inventory;
    inv.os = System::long_os_version().unwrap_or_else(|| std::env::consts::OS.into());
    inv.cpu_name = system
        .cpus()
        .first()
        .map(|c| c.brand().trim().to_owned())
        .unwrap_or_default();
    inv.logical_cores = system.cpus().len();
    inv.physical_cores = System::physical_core_count().unwrap_or(0);
    inv.cpu
        .push(Detail::new("Architettura", std::env::consts::ARCH));
    inv.cpu.push(Detail::new(
        "Core fisici / thread logici",
        format!("{} / {}", inv.physical_cores, inv.logical_cores),
    ));
    inv.memory.push(Detail::new(
        "RAM utilizzabile dal sistema",
        bytes(system.total_memory()),
    ));
    inv.memory
        .push(Detail::new("Swap / pagefile", bytes(system.total_swap())));
    cpuid_details(inv);
    #[cfg(windows)]
    {
        // COM connections belong to their creating thread. This single inventory worker
        // prevents a stalled WMI provider from blocking startup indefinitely.
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(windows_inventory());
        });
        match rx.recv_timeout(Duration::from_secs(4)) {
            Ok(Ok(wmi)) => {
                inv.cpu.extend(wmi.cpu);
                inv.board.extend(wmi.board);
                inv.memory.extend(wmi.memory);
                inv.graphics.extend(wmi.graphics);
                inv.warnings.extend(wmi.warnings);
                hw.gpu_names = wmi.gpu_names;
            }
            Ok(Err(error)) => inv
                .warnings
                .push(format!("Inventario WMI non disponibile: {error}")),
            Err(_) => inv
                .warnings
                .push("Inventario WMI: superato il limite di 4 secondi; dati parziali.".into()),
        }
    }
    #[cfg(not(windows))]
    inv.warnings.push(
        "Inventario scheda madre, BIOS e moduli DIMM esteso disponibile su Windows via WMI.".into(),
    );
    inv.warnings.push("SPD diretto, timing RAM e clock per core effettivi richiedono provider specifici: non misurati. Temperature e tensioni avanzate sono opzionali tramite LibreHardwareMonitor già in esecuzione.".into());
    inv.warnings.push("Le feature CPUID descrivono l'hardware; il sistema operativo può limitarne l'utilizzo. La frequenza OS può essere nominale.".into());
    hw
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
fn cpuid_details(inv: &mut Inventory) {
    let cpuid = raw_cpuid::CpuId::new();
    if let Some(vendor) = cpuid.get_vendor_info() {
        inv.cpu.push(Detail::new("Vendor CPUID", vendor.as_str()));
    }
    if let Some(brand) = cpuid.get_processor_brand_string() {
        inv.cpu_name = brand.as_str().trim().into();
    }
    let mut features = Vec::new();
    if let Some(f) = cpuid.get_feature_info() {
        inv.cpu.push(Detail::new(
            "Family / Model / Stepping",
            format!("{} / {} / {}", f.family_id(), f.model_id(), f.stepping_id()),
        ));
        for (supported, label) in [
            (f.has_sse2(), "SSE2"),
            (f.has_sse3(), "SSE3"),
            (f.has_ssse3(), "SSSE3"),
            (f.has_sse41(), "SSE4.1"),
            (f.has_sse42(), "SSE4.2"),
            (f.has_avx(), "AVX"),
            (f.has_fma(), "FMA"),
            (f.has_f16c(), "F16C"),
            (f.has_aesni(), "AES-NI"),
            (f.has_vmx(), "VT-x"),
            (f.has_hypervisor(), "Hypervisor"),
        ] {
            if supported {
                features.push(label);
            }
        }
    }
    if let Some(f) = cpuid.get_extended_feature_info() {
        for (supported, label) in [
            (f.has_avx2(), "AVX2"),
            (f.has_avx512f(), "AVX-512F"),
            (f.has_avx512bw(), "AVX-512BW"),
            (f.has_avx512vl(), "AVX-512VL"),
            (f.has_avx512vnni(), "AVX-512 VNNI"),
            (f.has_avx_vnni(), "AVX VNNI"),
            (f.has_avx512_bf16(), "AVX-512 BF16"),
            (f.has_amx_tile(), "AMX TILE"),
            (f.has_amx_int8(), "AMX INT8"),
            (f.has_amx_bf16(), "AMX BF16"),
            (f.has_sha(), "SHA"),
            (f.has_bmi2(), "BMI2"),
        ] {
            if supported {
                features.push(label);
            }
        }
    }
    inv.cpu.push(Detail::new(
        "Istruzioni / accelerazione hardware",
        features.join(" · "),
    ));
    let mut cache_count = 0;
    if let Some(caches) = cpuid.get_cache_parameters() {
        for c in caches {
            if matches!(
                c.cache_type(),
                raw_cpuid::CacheType::Null | raw_cpuid::CacheType::Reserved
            ) {
                continue;
            }
            let size = (c.coherency_line_size() as u64)
                .saturating_mul(c.physical_line_partitions() as u64)
                .saturating_mul(c.associativity() as u64)
                .saturating_mul(c.sets() as u64);
            inv.cpu.push(Detail::new(
                format!("Cache L{} {:?}", c.level(), c.cache_type()),
                format!(
                    "{} / istanza · {} vie · linea {} B",
                    bytes(size),
                    c.associativity(),
                    c.coherency_line_size()
                ),
            ));
            cache_count += 1;
        }
    }
    // AMD processors can expose legacy extended cache leaves without leaf 4.
    if cache_count == 0 {
        if let Some(c) = cpuid.get_l1_cache_and_tlb_info() {
            inv.cpu.push(Detail::new(
                "Cache L1 dati / istruzioni per core",
                format!("{} / {} KiB", c.dcache_size(), c.icache_size()),
            ));
        }
        if let Some(c) = cpuid.get_l2_l3_cache_and_tlb_info() {
            if c.l2cache_size() > 0 {
                inv.cpu.push(Detail::new(
                    "Cache L2 CPUID",
                    format!("{} KiB", c.l2cache_size()),
                ));
            }
            if c.l3cache_size() > 0 {
                inv.cpu.push(Detail::new(
                    "Cache L3 CPUID (limite inferiore)",
                    format!("{} KiB", c.l3cache_size() as u64 * 512),
                ));
            }
        }
    }
    if let Some(f) = cpuid.get_processor_frequency_info() {
        if f.processor_base_frequency() > 0 {
            inv.cpu.push(Detail::new(
                "Base dichiarata CPUID",
                format!("{} MHz", f.processor_base_frequency()),
            ));
        }
        if f.processor_max_frequency() > 0 {
            inv.cpu.push(Detail::new(
                "Massima dichiarata CPUID",
                format!("{} MHz", f.processor_max_frequency()),
            ));
        }
        if f.bus_frequency() > 0 {
            inv.cpu.push(Detail::new(
                "Bus dichiarato CPUID",
                format!("{} MHz", f.bus_frequency()),
            ));
        }
    }
    if let Some(c) = cpuid.get_processor_capacity_feature_info() {
        inv.cpu.push(Detail::new(
            "Indirizzamento fisico / virtuale",
            format!(
                "{} / {} bit",
                c.physical_address_bits(),
                c.linear_address_bits()
            ),
        ));
    }
}
#[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
fn cpuid_details(inv: &mut Inventory) {
    inv.warnings
        .push("CPUID dettagliato disponibile solo su x86/x86-64.".into());
}

#[cfg(windows)]
#[derive(Default)]
struct WmiInventory {
    cpu: Vec<Detail>,
    board: Vec<Detail>,
    memory: Vec<Detail>,
    graphics: Vec<Detail>,
    gpu_names: Vec<String>,
    warnings: Vec<String>,
}

#[cfg(windows)]
fn windows_inventory() -> Result<WmiInventory, String> {
    use serde::Deserialize;
    use wmi::WMIConnection;
    #[derive(Deserialize)]
    #[serde(rename_all = "PascalCase")]
    struct Cpu {
        name: Option<String>,
        socket_designation: Option<String>,
        max_clock_speed: Option<u32>,
        l2_cache_size: Option<u32>,
        l3_cache_size: Option<u32>,
        address_width: Option<u16>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "PascalCase")]
    struct Board {
        manufacturer: Option<String>,
        product: Option<String>,
        version: Option<String>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "PascalCase")]
    struct Bios {
        manufacturer: Option<String>,
        #[serde(rename = "SMBIOSBIOSVersion")]
        version: Option<String>,
        release_date: Option<String>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "PascalCase")]
    struct Dimm {
        device_locator: Option<String>,
        bank_label: Option<String>,
        manufacturer: Option<String>,
        part_number: Option<String>,
        capacity: Option<u64>,
        speed: Option<u32>,
        configured_clock_speed: Option<u32>,
        #[serde(rename = "SMBIOSMemoryType")]
        memory_type: Option<u32>,
        data_width: Option<u16>,
        total_width: Option<u16>,
        configured_voltage: Option<u32>,
        form_factor: Option<u16>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "PascalCase")]
    struct MemoryArray {
        memory_devices: Option<u16>,
        memory_error_correction: Option<u16>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "PascalCase")]
    struct Gpu {
        name: Option<String>,
        driver_version: Option<String>,
        video_processor: Option<String>,
    }

    let connection = WMIConnection::new().map_err(|e| e.to_string())?;
    let mut out = WmiInventory::default();
    match connection.raw_query::<Cpu>("SELECT Name, SocketDesignation, MaxClockSpeed, L2CacheSize, L3CacheSize, AddressWidth FROM Win32_Processor") {
        Ok(rows) => for (i, c) in rows.into_iter().enumerate() {
            let label = format!("CPU socket {}", i + 1);
            add_opt(&mut out.cpu, &label, c.name);
            add_opt(&mut out.cpu, "Socket / package WMI", c.socket_designation);
            if let Some(v) = c.max_clock_speed.filter(|v| *v > 0) { out.cpu.push(Detail::new("Clock massimo firmware", format!("{v} MHz (dichiarato)"))); }
            if let Some(v) = c.address_width { out.cpu.push(Detail::new("Larghezza indirizzi OS", format!("{v} bit"))); }
            if let Some(v) = c.l2_cache_size.filter(|v| *v > 0) { out.cpu.push(Detail::new("L2 firmware / socket", format!("{v} KiB"))); }
            if let Some(v) = c.l3_cache_size.filter(|v| *v > 0) { out.cpu.push(Detail::new("L3 firmware / socket", format!("{v} KiB"))); }
        },
        Err(_) => out.warnings.push("WMI: dettagli del processore non disponibili.".into()),
    }
    match connection
        .raw_query::<Board>("SELECT Manufacturer, Product, Version FROM Win32_BaseBoard")
    {
        Ok(rows) => {
            for b in rows {
                add_opt(&mut out.board, "Produttore", b.manufacturer);
                add_opt(&mut out.board, "Scheda madre", b.product);
                add_opt(&mut out.board, "Revisione", b.version);
            }
        }
        Err(_) => out
            .warnings
            .push("WMI: scheda madre non disponibile.".into()),
    }
    match connection
        .raw_query::<Bios>("SELECT Manufacturer, SMBIOSBIOSVersion, ReleaseDate FROM Win32_BIOS")
    {
        Ok(rows) => {
            for b in rows {
                add_opt(&mut out.board, "BIOS / UEFI vendor", b.manufacturer);
                add_opt(&mut out.board, "Versione BIOS / UEFI", b.version);
                if let Some(date) = b.release_date {
                    add_opt(&mut out.board, "Data firmware", wmi_date(&date));
                }
            }
        }
        Err(_) => out
            .warnings
            .push("WMI: informazioni BIOS non disponibili.".into()),
    }
    match connection.raw_query::<Dimm>("SELECT DeviceLocator, BankLabel, Manufacturer, PartNumber, Capacity, Speed, ConfiguredClockSpeed, SMBIOSMemoryType, DataWidth, TotalWidth, ConfiguredVoltage, FormFactor FROM Win32_PhysicalMemory") {
        Ok(rows) => for (i, d) in rows.into_iter().enumerate() {
            let slot = d.device_locator.as_deref().filter(|s| !s.trim().is_empty()).unwrap_or("slot non indicato");
            out.memory.push(Detail::new(format!("DIMM {} · {}", i + 1, slot.trim()), format!("{} · {}", d.capacity.map(bytes).unwrap_or_else(|| "capacità N/D".into()), memory_type(d.memory_type.unwrap_or(0)))));
            add_opt(&mut out.memory, "Produttore modulo", d.manufacturer);
            add_opt(&mut out.memory, "Part number", d.part_number);
            add_opt(&mut out.memory, "Banco firmware", d.bank_label);
            if let Some(v) = d.speed.filter(|v| *v > 0) { out.memory.push(Detail::new("Velocità dichiarata SMBIOS", format!("{v} MT/s"))); }
            if let Some(v) = d.configured_clock_speed.filter(|v| *v > 0) { out.memory.push(Detail::new("Velocità configurata SMBIOS", format!("{v} MT/s (non misurata)"))); }
            if let (Some(data), Some(total)) = (d.data_width, d.total_width) { out.memory.push(Detail::new("Bus dati / totale", format!("{data} / {total} bit"))); }
            if let Some(v) = d.configured_voltage.filter(|v| *v > 0) { out.memory.push(Detail::new("Tensione configurata firmware", format!("{:.3} V", v as f64 / 1000.0))); }
            if let Some(v) = d.form_factor { out.memory.push(Detail::new("Formato modulo", match v { 8 => "DIMM".into(), 12 => "SO-DIMM".into(), _ => format!("SMBIOS codice {v}") })); }
        },
        Err(_) => out.warnings.push("WMI: moduli di memoria non disponibili.".into()),
    }
    if let Ok(rows) = connection.raw_query::<MemoryArray>(
        "SELECT MemoryDevices, MemoryErrorCorrection FROM Win32_PhysicalMemoryArray",
    ) {
        for a in rows {
            if let Some(n) = a.memory_devices {
                out.memory.push(Detail::new(
                    "Slot dichiarati firmware / array",
                    n.to_string(),
                ));
            }
            if let Some(ecc) = a.memory_error_correction {
                out.memory.push(Detail::new(
                    "Correzione errori array (firmware)",
                    match ecc {
                        3 => "Nessuna".into(),
                        4 => "Parità".into(),
                        5 => "ECC single-bit".into(),
                        6 => "ECC multi-bit".into(),
                        _ => format!("N/D (codice {ecc})"),
                    },
                ));
            }
        }
    }
    match connection
        .raw_query::<Gpu>("SELECT Name, DriverVersion, VideoProcessor FROM Win32_VideoController")
    {
        Ok(rows) => {
            for g in rows {
                if let Some(name) = g.name {
                    out.gpu_names.push(name.trim().into());
                    add_opt(&mut out.graphics, "Adattatore", Some(name));
                }
                add_opt(&mut out.graphics, "Driver", g.driver_version);
                add_opt(&mut out.graphics, "Processore video", g.video_processor);
            }
        }
        Err(_) => out
            .warnings
            .push("WMI: inventario GPU non disponibile.".into()),
    }
    out.graphics.push(Detail::new(
        "Carico e memoria GPU",
        "Windows WDDM / PDH con mappatura DXGI; memoria dedicata e condivisa separate. Sensori NVIDIA via nvidia-smi se disponibile; AdapterRAM WMI non usata (limite 32 bit).",
    ));
    Ok(out)
}

#[cfg(windows)]
fn add_opt(rows: &mut Vec<Detail>, label: &str, value: Option<String>) {
    if let Some(value) = value
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    {
        rows.push(Detail::new(label, value));
    }
}
#[cfg(any(windows, test))]
fn wmi_date(input: &str) -> Option<String> {
    let prefix = input.get(..8)?;
    if !prefix.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(format!(
        "{}-{}-{}",
        &prefix[..4],
        &prefix[4..6],
        &prefix[6..8]
    ))
}
#[cfg(any(windows, test))]
fn memory_type(value: u32) -> String {
    match value {
        20 => "DDR".into(),
        21 => "DDR2".into(),
        24 => "DDR3".into(),
        26 => "DDR4".into(),
        27 => "LPDDR".into(),
        28 => "LPDDR2".into(),
        29 => "LPDDR3".into(),
        30 => "LPDDR4".into(),
        34 => "DDR5".into(),
        35 => "LPDDR5".into(),
        _ => format!("Tipo SMBIOS {value} (N/D)"),
    }
}

type IoCounters = (u64, u64);

pub struct Collector {
    system: System,
    disks: Disks,
    networks: Networks,
    components: Components,
    started: Instant,
    previous_sample: Instant,
    process_io: HashMap<(u32, u64), IoCounters>,
    disk_io: HashMap<String, IoCounters>,
    network_io: HashMap<String, IoCounters>,
    gpu_worker: GpuWorker,
    gpu_rows: Vec<GpuRow>,
    gpu_warning: String,
    gpu_updated: Option<Instant>,
    #[cfg(windows)]
    wddm_worker: crate::gpu_windows::Worker,
    #[cfg(windows)]
    sensor_worker: crate::sensors::Worker,
}
impl Default for Collector {
    fn default() -> Self {
        Self::new()
    }
}

fn process_refresh() -> ProcessRefreshKind {
    ProcessRefreshKind::nothing()
        .with_cpu()
        .with_memory()
        .with_disk_usage()
        .without_tasks()
}

impl Collector {
    pub fn new() -> Self {
        #[cfg(windows)]
        let wddm_worker = crate::gpu_windows::Worker::new();
        #[cfg(windows)]
        let sensor_worker = crate::sensors::Worker::new();
        let hw = HARDWARE.get_or_init(collect_inventory);
        let mut system = System::new();
        system.refresh_cpu_all();
        system.refresh_memory();
        system.refresh_processes_specifics(ProcessesToUpdate::All, true, process_refresh());
        let disks = Disks::new_with_refreshed_list();
        let networks = Networks::new_with_refreshed_list();
        let components = Components::new_with_refreshed_list();
        let process_io = system
            .processes()
            .iter()
            .map(|(pid, p)| {
                let d = p.disk_usage();
                (
                    (pid.as_u32(), p.start_time()),
                    (d.total_read_bytes, d.total_written_bytes),
                )
            })
            .collect();
        let disk_io = disks
            .iter()
            .map(|d| {
                let u = d.usage();
                (disk_key(d), (u.total_read_bytes, u.total_written_bytes))
            })
            .collect();
        let network_io = networks
            .iter()
            .map(|(n, d)| (n.clone(), (d.total_received(), d.total_transmitted())))
            .collect();
        let now = Instant::now();
        Self {
            system,
            disks,
            networks,
            components,
            started: now,
            previous_sample: now,
            process_io,
            disk_io,
            network_io,
            gpu_worker: GpuWorker::new(),
            gpu_rows: fallback_gpus(&hw.gpu_names),
            gpu_warning:
                "Telemetria GPU in inizializzazione; sensori non disponibili indicati N/D.".into(),
            gpu_updated: None,
            #[cfg(windows)]
            wddm_worker,
            #[cfg(windows)]
            sensor_worker,
        }
    }

    pub fn sample(&mut self) -> Snapshot {
        let collect_started = Instant::now();
        let elapsed = collect_started
            .duration_since(self.previous_sample)
            .as_secs_f64();
        self.previous_sample = collect_started;
        self.system.refresh_cpu_all();
        self.system.refresh_memory();
        self.system
            .refresh_processes_specifics(ProcessesToUpdate::All, true, process_refresh());
        self.disks.refresh(true);
        self.networks.refresh(true);
        self.components.refresh(true);
        let warm = elapsed >= sysinfo::MINIMUM_CPU_UPDATE_INTERVAL.as_secs_f64();
        let logical = self.system.cpus().len().max(1);
        let mut process_io = HashMap::new();
        let mut processes: Vec<_> = self
            .system
            .processes()
            .iter()
            .map(|(pid, p)| {
                let key = (pid.as_u32(), p.start_time());
                let io = p.disk_usage();
                let counters = (io.total_read_bytes, io.total_written_bytes);
                let (read, write) = rates(counters, self.process_io.get(&key).copied(), elapsed);
                process_io.insert(key, counters);
                let name = p.name().to_string_lossy().into_owned();
                let is_llm = is_llm_process(&name);
                ProcessRow {
                    pid: pid.as_u32(),
                    name,
                    cpu_percent: if warm {
                        normalized_cpu(p.cpu_usage(), logical)
                    } else {
                        0.0
                    },
                    memory_bytes: p.memory(),
                    read_bytes_per_sec: read,
                    write_bytes_per_sec: write,
                    is_llm,
                    gpu_memory_bytes: None,
                    gpu_shared_memory_bytes: None,
                    gpu_percent: None,
                }
            })
            .collect();
        self.process_io = process_io;
        processes.sort_by(|a, b| {
            b.cpu_percent
                .total_cmp(&a.cpu_percent)
                .then(a.pid.cmp(&b.pid))
        });

        let mut disk_io = HashMap::new();
        let disks = self
            .disks
            .iter()
            .map(|d| {
                let key = disk_key(d);
                let u = d.usage();
                let counters = (u.total_read_bytes, u.total_written_bytes);
                let (read, write) = rates(counters, self.disk_io.get(&key).copied(), elapsed);
                disk_io.insert(key, counters);
                DiskRow {
                    name: d.name().to_string_lossy().into_owned(),
                    mount: d.mount_point().to_string_lossy().into_owned(),
                    kind: format!("{:?} · {}", d.kind(), d.file_system().to_string_lossy()),
                    total_bytes: d.total_space(),
                    available_bytes: d.available_space(),
                    read_bytes_per_sec: read,
                    write_bytes_per_sec: write,
                }
            })
            .collect();
        self.disk_io = disk_io;
        let mut network_io = HashMap::new();
        let mut networks: Vec<_> = self
            .networks
            .iter()
            .map(|(name, n)| {
                let counters = (n.total_received(), n.total_transmitted());
                let (received, transmitted) =
                    rates(counters, self.network_io.get(name).copied(), elapsed);
                network_io.insert(name.clone(), counters);
                NetworkRow {
                    name: name.clone(),
                    received_per_sec: received,
                    transmitted_per_sec: transmitted,
                    total_received: counters.0,
                    total_transmitted: counters.1,
                }
            })
            .collect();
        self.network_io = network_io;
        networks.sort_by(|a, b| a.name.cmp(&b.name));
        #[allow(unused_mut)]
        let mut sensors: Vec<_> = self
            .components
            .iter()
            .filter_map(|c| {
                c.temperature()
                    .filter(|v| v.is_finite())
                    .map(|temperature| SensorRow {
                        name: c.label().into(),
                        temperature,
                    })
            })
            .collect();

        while let Ok(update) = self.gpu_worker.updates.try_recv() {
            match update {
                Ok(rows) => {
                    // Preserve non-NVIDIA adapters as inventory-only rows.
                    let names = &HARDWARE.get_or_init(collect_inventory).gpu_names;
                    self.gpu_rows = rows;
                    for name in names {
                        if !name.to_ascii_lowercase().contains("nvidia")
                            && !self.gpu_rows.iter().any(|g| g.name == *name)
                        {
                            self.gpu_rows.push(GpuRow {
                                name: name.clone(),
                                source: "WMI inventory".into(),
                                ..Default::default()
                            });
                        }
                    }
                    self.gpu_warning.clear();
                    self.gpu_updated = Some(Instant::now());
                }
                Err(message) => {
                    self.gpu_warning = message;
                    // Never retain a measured value as if it were current after a failed poll.
                    for gpu in &mut self.gpu_rows {
                        gpu.utilization = None;
                        gpu.memory_used = None;
                        gpu.temperature = None;
                        gpu.power_watts = None;
                    }
                    self.gpu_updated = None;
                }
            }
        }
        let mut warnings = Vec::new();
        #[cfg(windows)]
        let sensor_details;
        #[cfg(not(windows))]
        let sensor_details = Vec::new();
        if !warm {
            warnings.push(
                "Campione CPU in riscaldamento: attendere almeno 250 ms tra i campioni.".into(),
            );
        }
        if self
            .gpu_updated
            .is_some_and(|t| t.elapsed() > Duration::from_secs(6))
        {
            warnings.push("Campione GPU scaduto (>6 s): sensori non correnti indicati N/D.".into());
            for g in &mut self.gpu_rows {
                g.utilization = None;
                g.memory_used = None;
                g.temperature = None;
                g.power_watts = None;
            }
        }
        #[allow(unused_mut)]
        let mut gpus = self.gpu_rows.clone();
        #[cfg(windows)]
        {
            let native = self.wddm_worker.latest();
            let has_native_data = native
                .gpus
                .iter()
                .any(|gpu| gpu.utilization.is_some() || gpu.memory_used.is_some());
            for process in &mut processes {
                if let Some(value) = native.processes.get(&process.pid) {
                    process.gpu_memory_bytes = value.dedicated_bytes;
                    process.gpu_shared_memory_bytes = value.shared_bytes;
                    process.gpu_percent = value.utilization;
                }
            }
            if !native.gpus.is_empty() {
                gpus = merge_gpus(native.gpus, &gpus);
            }
            warnings.extend(native.warnings);
            if has_native_data {
                warnings.push("GPU Windows: carico = motore più occupato (WDDM), memoria dedicata/condivisa = contatori OS. Temperatura/potenza NVIDIA da nvidia-smi quando disponibili.".into());
            } else if !self.gpu_warning.is_empty() {
                warnings.push(self.gpu_warning.clone());
            }
            let extended = self.sensor_worker.latest();
            sensors.extend(extended.temperatures);
            sensor_details = extended.details;
            warnings.extend(extended.warnings);
        }
        #[cfg(not(windows))]
        if !self.gpu_warning.is_empty() {
            warnings.push(self.gpu_warning.clone());
        }
        if sensors.is_empty() {
            warnings.push("Temperature CPU / scheda madre non esposte dal sistema o dal provider opzionale; nessun valore simulato.".into());
        }
        if gpus.iter().any(|g| g.utilization.is_none()) {
            warnings.push(
                "Una o più GPU non espongono un carico corrente: valore N/D, non zero.".into(),
            );
        }
        let cpus = self.system.cpus();
        let frequency_mhz = if cpus.is_empty() {
            0
        } else {
            cpus.iter().map(|c| c.frequency()).sum::<u64>() / cpus.len() as u64
        };
        Snapshot {
            timestamp_unix: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            elapsed_secs: self.started.elapsed().as_secs_f64(),
            sampling_ms: collect_started.elapsed().as_millis().min(u64::MAX as u128) as u64,
            cpu_percent: if warm {
                finite_percent(self.system.global_cpu_usage())
            } else {
                0.0
            },
            per_core: cpus
                .iter()
                .map(|c| {
                    if warm {
                        finite_percent(c.cpu_usage())
                    } else {
                        0.0
                    }
                })
                .collect(),
            frequency_mhz,
            memory_used: self.system.used_memory(),
            memory_total: self.system.total_memory(),
            swap_used: self.system.used_swap(),
            swap_total: self.system.total_swap(),
            processes,
            disks,
            networks,
            gpus,
            sensors,
            sensor_details,
            warnings,
        }
    }
}

fn disk_key(d: &sysinfo::Disk) -> String {
    format!(
        "{}|{}",
        d.name().to_string_lossy(),
        d.mount_point().to_string_lossy()
    )
}
fn finite_percent(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.0, 100.0)
    } else {
        0.0
    }
}
fn normalized_cpu(value: f32, logical: usize) -> f32 {
    finite_percent(value / logical.max(1) as f32)
}
fn rates(current: IoCounters, previous: Option<IoCounters>, seconds: f64) -> (f64, f64) {
    let Some(previous) = previous else {
        return (0.0, 0.0);
    };
    if !seconds.is_finite() || seconds <= 0.0 {
        return (0.0, 0.0);
    }
    (
        current.0.saturating_sub(previous.0) as f64 / seconds,
        current.1.saturating_sub(previous.1) as f64 / seconds,
    )
}
fn is_llm_process(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    [
        "ollama",
        "llama-server",
        "llama-cli",
        "llama.cpp",
        "lm studio",
        "lmstudio",
        "lms.exe",
        "vllm",
        "text-generation",
        "koboldcpp",
        "gpt4all",
        "jan.exe",
    ]
    .iter()
    .any(|part| name.contains(part))
}
fn fallback_gpus(names: &[String]) -> Vec<GpuRow> {
    names
        .iter()
        .map(|n| GpuRow {
            name: n.clone(),
            source: if cfg!(windows) {
                "WMI inventory"
            } else {
                "n/d"
            }
            .into(),
            ..Default::default()
        })
        .collect()
}

/// Exact name association only when unambiguous. Two identical adapters must not
/// inherit the same NVIDIA sensor values because a display name is not an ID.
#[cfg(any(windows, test))]
fn merge_gpus(mut native: Vec<GpuRow>, secondary: &[GpuRow]) -> Vec<GpuRow> {
    let key = |name: &str| name.trim().to_ascii_lowercase();
    let native_names: Vec<_> = native.iter().map(|gpu| key(&gpu.name)).collect();
    for gpu in &mut native {
        let name = key(&gpu.name);
        let matches: Vec<_> = secondary
            .iter()
            .filter(|other| key(&other.name) == name)
            .collect();
        if matches.len() == 1 && native_names.iter().filter(|other| **other == name).count() == 1 {
            let other = matches[0];
            // The primary label follows the utilization provider; sensor origin
            // remains documented separately so missing sensors do not change it.
            if gpu.utilization.is_none() && other.utilization.is_some() {
                gpu.source = other.source.clone();
            }
            gpu.temperature = other.temperature;
            gpu.power_watts = other.power_watts;
            gpu.utilization = gpu.utilization.or(other.utilization);
            gpu.memory_used = gpu.memory_used.or(other.memory_used);
            gpu.memory_total = gpu.memory_total.or(other.memory_total);
        }
    }
    for gpu in secondary {
        if !native_names.iter().any(|name| *name == key(&gpu.name)) {
            native.push(gpu.clone());
        }
    }
    native
}

struct GpuWorker {
    stop: mpsc::Sender<()>,
    updates: mpsc::Receiver<Result<Vec<GpuRow>, String>>,
}
impl GpuWorker {
    fn new() -> Self {
        let (stop, stop_rx) = mpsc::channel();
        let (tx, updates) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            loop {
                let result = nvidia_snapshot();
                let unavailable = result
                    .as_ref()
                    .err()
                    .is_some_and(|e| e.contains("non trovato"));
                if let Err(mpsc::TrySendError::Disconnected(_)) = tx.try_send(result) {
                    break;
                }
                if unavailable || stop_rx.recv_timeout(Duration::from_secs(2)).is_ok() {
                    break;
                }
                if matches!(stop_rx.try_recv(), Err(mpsc::TryRecvError::Disconnected)) {
                    break;
                }
            }
        });
        Self { stop, updates }
    }
}
impl Drop for GpuWorker {
    fn drop(&mut self) {
        let _ = self.stop.send(());
    }
}

fn nvidia_snapshot() -> Result<Vec<GpuRow>, String> {
    let mut command = Command::new("nvidia-smi");
    command
        .args([
            "--query-gpu=name,utilization.gpu,memory.used,memory.total,temperature.gpu,power.draw",
            "--format=csv,noheader,nounits",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound { "nvidia-smi non trovato: GPU NVIDIA non rilevabile; AMD / Intel restano in modalità inventario.".into() }
        else { format!("Avvio nvidia-smi non riuscito: {e}") }
    })?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return Err("nvidia-smi: driver / metriche non disponibili.".into());
                }
                let mut output = String::new();
                // A fixed, small set of GPU columns; cap accepted output at 64 KiB.
                if let Some(stdout) = child.stdout.take() {
                    stdout
                        .take(65536)
                        .read_to_string(&mut output)
                        .map_err(|_| "Risposta nvidia-smi non leggibile.".to_string())?;
                }
                return parse_nvidia_csv(&output);
            }
            Ok(None) if started.elapsed() < Duration::from_millis(800) => {
                std::thread::sleep(Duration::from_millis(10))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("nvidia-smi: timeout / errore (limite 800 ms); sensori N/D.".into());
            }
        }
    }
}

fn parse_nvidia_csv(output: &str) -> Result<Vec<GpuRow>, String> {
    let mut rows = Vec::new();
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .trim(csv::Trim::All)
        .flexible(false)
        .from_reader(output.as_bytes());
    for record in reader.records() {
        let r = record.map_err(|_| "Formato risposta nvidia-smi non valido.".to_string())?;
        if r.len() != 6 || r[0].trim().is_empty() {
            return Err("Colonne risposta nvidia-smi non valide.".into());
        }
        let number = |index| {
            r.get(index)
                .and_then(|s| s.parse::<f32>().ok())
                .filter(|v| v.is_finite() && *v >= 0.0)
        };
        let memory = |index| number(index).map(|v| (v as f64 * 1024.0 * 1024.0) as u64);
        rows.push(GpuRow {
            source: "nvidia-smi".into(),
            name: r[0].trim().into(),
            utilization: number(1).filter(|v| *v <= 100.0),
            memory_used: memory(2),
            memory_total: memory(3),
            temperature: number(4),
            power_watts: number(5),
            shared_memory_used: None,
        });
    }
    if rows.is_empty() {
        Err("nvidia-smi non ha restituito GPU utilizzabili.".into())
    } else {
        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gpu_merge_preserves_os_metrics_and_refuses_ambiguous_name_sensors() {
        let native = vec![GpuRow {
            name: "NVIDIA GPU".into(),
            utilization: Some(5.0),
            memory_used: Some(100),
            shared_memory_used: Some(200),
            ..Default::default()
        }];
        let secondary = vec![GpuRow {
            name: "NVIDIA GPU".into(),
            utilization: Some(90.0),
            temperature: Some(60.0),
            ..Default::default()
        }];
        let merged = merge_gpus(native.clone(), &secondary);
        assert_eq!(merged[0].utilization, Some(5.0));
        assert_eq!(merged[0].temperature, Some(60.0));
        assert_eq!(merged[0].shared_memory_used, Some(200));
        let ambiguous = merge_gpus(vec![native[0].clone(), native[0].clone()], &secondary);
        assert!(ambiguous.iter().all(|gpu| gpu.temperature.is_none()));
    }
    #[test]
    fn io_rates_use_actual_elapsed_and_handle_reset_and_first_sample() {
        assert_eq!(
            rates((2000, 5000), Some((1000, 3000)), 2.0),
            (500.0, 1000.0)
        );
        assert_eq!(rates((100, 200), Some((1000, 3000)), 0.5), (0.0, 0.0));
        assert_eq!(rates((1000, 3000), None, 1.0), (0.0, 0.0));
        assert_eq!(rates((1000, 3000), Some((0, 0)), 0.0), (0.0, 0.0));
        assert_eq!(rates((1000, 3000), Some((0, 0)), f64::NAN), (0.0, 0.0));
    }
    #[test]
    fn process_cpu_is_fraction_of_entire_host() {
        assert_eq!(normalized_cpu(1600.0, 16), 100.0);
        assert_eq!(normalized_cpu(100.0, 16), 6.25);
        assert_eq!(normalized_cpu(f32::NAN, 16), 0.0);
        assert_eq!(normalized_cpu(-10.0, 16), 0.0);
    }
    #[test]
    fn gpu_parser_preserves_unavailable_and_uses_binary_mib() {
        let rows = parse_nvidia_csv("NVIDIA RTX 5090, 42, 2048, 32768, 66, 125.50\nNVIDIA Tesla, [N/A], [Not Supported], 16384, [N/A], [N/A]\n").unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].memory_used, Some(2 * 1024 * 1024 * 1024));
        assert_eq!(rows[0].utilization, Some(42.0));
        assert_eq!(rows[0].source, "nvidia-smi");
        assert_eq!(rows[1].utilization, None);
        assert_eq!(rows[1].memory_used, None);
        assert_eq!(rows[1].memory_total, Some(16 * 1024 * 1024 * 1024));
        assert_eq!(rows[1].temperature, None);
    }
    #[test]
    fn gpu_parser_rejects_bad_rows_and_non_finite_values() {
        assert!(parse_nvidia_csv("").is_err());
        assert!(parse_nvidia_csv("bad,1,2").is_err());
        let row = parse_nvidia_csv("GPU,101,NaN,4096,inf,-1")
            .unwrap()
            .remove(0);
        assert_eq!(row.utilization, None);
        assert_eq!(row.memory_used, None);
        assert_eq!(row.temperature, None);
        assert_eq!(row.power_watts, None);
    }
    #[test]
    fn firmware_values_are_labelled_and_dates_are_not_blindly_sliced() {
        assert_eq!(
            wmi_date("20260918000000.000000+000"),
            Some("2026-09-18".into())
        );
        assert_eq!(wmi_date("??"), None);
        assert_eq!(wmi_date("ééééé"), None);
        assert_eq!(memory_type(34), "DDR5");
        assert!(memory_type(999).contains("N/D"));
    }
    #[test]
    fn llm_process_detection_avoids_general_python_false_positives() {
        assert!(is_llm_process("ollama.exe"));
        assert!(is_llm_process("llama-server.exe"));
        assert!(!is_llm_process("python.exe"));
        assert!(!is_llm_process("chrome.exe"));
    }
}
