use crate::model::*;
use anyhow::{Context, Result};
use chrono::Utc;
use serde::Serialize;
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Serialize)]
pub struct Report<'a> {
    pub cpu_benchmark: Option<&'a crate::bench::CpuBenchmark>,
    pub schema_version: u32,
    pub app_version: &'static str,
    pub captured_utc: String,
    pub inventory: &'a Inventory,
    pub snapshot: &'a Snapshot,
    pub llm: &'a LlmState,
}

pub fn export_snapshot(
    path: &Path,
    inventory: &Inventory,
    snapshot: &Snapshot,
    llm: &LlmState,
) -> Result<()> {
    export_snapshot_with_bench(path, inventory, snapshot, llm, None)
}
pub fn export_snapshot_with_bench(
    path: &Path,
    inventory: &Inventory,
    snapshot: &Snapshot,
    llm: &LlmState,
    cpu_benchmark: Option<&crate::bench::CpuBenchmark>,
) -> Result<()> {
    let report = Report {
        cpu_benchmark,
        schema_version: 1,
        app_version: env!("CARGO_PKG_VERSION"),
        captured_utc: Utc::now().to_rfc3339(),
        inventory,
        snapshot,
        llm,
    };
    let data = serde_json::to_vec_pretty(&report)?;
    ensure_parent(path)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .with_context(|| {
            format!(
                "Impossibile creare {} (il file non deve esistere)",
                path.display()
            )
        })?;
    if let Err(error) = file.write_all(&data).and_then(|()| file.sync_all()) {
        drop(file);
        let _ = fs::remove_file(path);
        return Err(error.into());
    }
    Ok(())
}

fn ensure_parent(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    Ok(())
}
/// Vector capture of the actual terminal cell buffer (no browser or external fonts needed).
pub fn buffer_svg(buffer: &ratatui::buffer::Buffer) -> String {
    use ratatui::style::Color;
    fn color(c: Color, default: &str) -> String {
        match c {
            Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
            Color::Black => "#000000".into(),
            Color::White => "#ffffff".into(),
            Color::Red | Color::LightRed => "#f67182".into(),
            Color::Green | Color::LightGreen => "#71dea1".into(),
            Color::Blue | Color::LightBlue => "#5a9deb".into(),
            Color::Cyan | Color::LightCyan => "#43d3e5".into(),
            Color::Yellow | Color::LightYellow => "#f9c269".into(),
            Color::Magenta | Color::LightMagenta => "#c590e8".into(),
            Color::Gray | Color::DarkGray => "#7e97b0".into(),
            _ => default.into(),
        }
    }
    fn xml(s: &str) -> String {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
    }
    let area = buffer.area;
    let width = u32::from(area.width) * 9 + 24;
    let height = u32::from(area.height) * 19 + 24;
    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" viewBox=\"0 0 {width} {height}\"><title>RUST-CPU terminal capture</title><rect width=\"100%\" height=\"100%\" rx=\"12\" fill=\"#090f1a\"/><g font-family=\"Cascadia Mono,DejaVu Sans Mono,Consolas,monospace\" font-size=\"14\" xml:space=\"preserve\">"
    );
    for y in area.y..area.bottom() {
        for x in area.x..area.right() {
            let c = &buffer[(x, y)];
            let px = 12 + u32::from(x - area.x) * 9;
            let py = 12 + u32::from(y - area.y) * 19;
            svg.push_str(&format!(
                "<rect x=\"{px}\" y=\"{py}\" width=\"9\" height=\"19\" fill=\"{}\"/>",
                color(c.bg, "#090f1a")
            ));
            if c.symbol() != " " {
                let bold = if c.modifier.contains(ratatui::style::Modifier::BOLD) {
                    " font-weight=\"700\""
                } else {
                    ""
                };
                svg.push_str(&format!(
                    "<text x=\"{px}\" y=\"{}\" fill=\"{}\"{bold}>{}</text>",
                    py + 15,
                    color(c.fg, "#dee9f3"),
                    xml(c.symbol())
                ));
            }
        }
    }
    svg.push_str("</g></svg>");
    svg
}
pub fn unique_path(directory: &Path, stem: &str, extension: &str) -> PathBuf {
    directory.join(format!(
        "{stem}-{}.{}",
        Utc::now().format("%Y%m%dT%H%M%S%.9fZ"),
        extension
    ))
}

pub struct Recorder {
    writer: csv::Writer<File>,
    pub path: PathBuf,
    pub rows: u64,
}
impl Recorder {
    pub fn new(path: &Path) -> Result<Self> {
        ensure_parent(path)?;
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .with_context(|| {
                format!(
                    "Impossibile creare {} (il file non deve esistere)",
                    path.display()
                )
            })?;
        let mut writer = csv::Writer::from_writer(file);
        writer.write_record([
            "timestamp_unix",
            "elapsed_secs",
            "sample_ms",
            "cpu_percent",
            "ram_used_bytes",
            "ram_total_bytes",
            "swap_used_bytes",
            "gpu0_percent",
            "gpu0_vram_used_bytes",
            "gpu0_vram_total_bytes",
            "gpu0_temperature_c",
            "gpu0_power_w",
            "llm_process_count",
            "llm_process_rss_sum_bytes",
            "disk_read_bytes_sec",
            "disk_write_bytes_sec",
            "net_rx_bytes_sec",
            "net_tx_bytes_sec",
        ])?;
        writer.flush()?;
        Ok(Self {
            writer,
            path: path.to_owned(),
            rows: 0,
        })
    }
    pub fn append(&mut self, s: &Snapshot) -> Result<()> {
        let gpu = s.gpus.first();
        self.writer.write_record([
            s.timestamp_unix.to_string(),
            format!("{:.3}", s.elapsed_secs),
            s.sampling_ms.to_string(),
            format!("{:.2}", s.cpu_percent),
            s.memory_used.to_string(),
            s.memory_total.to_string(),
            s.swap_used.to_string(),
            gpu.and_then(|g| g.utilization)
                .map(|v| format!("{v:.2}"))
                .unwrap_or_default(),
            gpu.and_then(|g| g.memory_used)
                .map(|v| v.to_string())
                .unwrap_or_default(),
            gpu.and_then(|g| g.memory_total)
                .map(|v| v.to_string())
                .unwrap_or_default(),
            gpu.and_then(|g| g.temperature)
                .map(|v| v.to_string())
                .unwrap_or_default(),
            gpu.and_then(|g| g.power_watts)
                .map(|v| v.to_string())
                .unwrap_or_default(),
            s.processes.iter().filter(|p| p.is_llm).count().to_string(),
            s.processes
                .iter()
                .filter(|p| p.is_llm)
                .map(|p| p.memory_bytes)
                .sum::<u64>()
                .to_string(),
            format!(
                "{:.2}",
                s.disks.iter().map(|d| d.read_bytes_per_sec).sum::<f64>()
            ),
            format!(
                "{:.2}",
                s.disks.iter().map(|d| d.write_bytes_per_sec).sum::<f64>()
            ),
            format!(
                "{:.2}",
                s.networks.iter().map(|n| n.received_per_sec).sum::<f64>()
            ),
            format!(
                "{:.2}",
                s.networks
                    .iter()
                    .map(|n| n.transmitted_per_sec)
                    .sum::<f64>()
            ),
        ])?;
        self.writer.flush()?;
        self.rows += 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exports_versioned_data_without_overwriting() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("snapshot.json");
        export_snapshot(
            &p,
            &Inventory::default(),
            &Snapshot::default(),
            &LlmState::default(),
        )
        .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&fs::read(&p).unwrap()).unwrap();
        assert_eq!(v["schema_version"], 1);
        assert!(
            export_snapshot(
                &p,
                &Inventory::default(),
                &Snapshot::default(),
                &LlmState::default()
            )
            .is_err()
        );
    }
    #[test]
    fn csv_is_parseable_and_missing_gpu_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("session.csv");
        let mut rec = Recorder::new(&p).unwrap();
        rec.append(&Snapshot {
            cpu_percent: 42.0,
            ..Default::default()
        })
        .unwrap();
        drop(rec);
        let mut csv = csv::Reader::from_path(p).unwrap();
        let row = csv.records().next().unwrap().unwrap();
        assert_eq!(&row[3], "42.00");
        assert_eq!(&row[7], "");
        assert_eq!(row.len(), 18);
    }
}
