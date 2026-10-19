//! Responsive, keyboard-first terminal workspace.
//!
//! Navigation contract: Tab/Shift-Tab or arrows switch pages, 1..9/0 select a
//! page, Up/Down/PgUp/PgDn/Home/End scroll. `/` searches processes; `s` cycles
//! sort and `a` reverses it. Space freezes the displayed sample, `t` changes
//! theme, `e` exports, `r` records. `+`/`-` change sampling except in Capacity
//! Lab, where they edit the field selected with `[`/`]`. `b` requests an
//! inference benchmark, Enter confirms, Esc returns, `?` opens help, `q` exits.

use crate::model::*;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, BorderType, Borders, Cell, Clear, Gauge, Paragraph, Row, Sparkline, Table,
        TableState, Tabs, Wrap,
    },
};

#[derive(Clone, Copy)]
struct Palette {
    bg: Color,
    surface: Color,
    elevated: Color,
    border: Color,
    text: Color,
    muted: Color,
    cyan: Color,
    green: Color,
    amber: Color,
    red: Color,
}

impl Palette {
    fn new(light: bool) -> Self {
        if light {
            Self {
                bg: Color::Rgb(236, 241, 247),
                surface: Color::Rgb(248, 250, 253),
                elevated: Color::Rgb(218, 230, 240),
                border: Color::Rgb(164, 184, 201),
                text: Color::Rgb(25, 42, 61),
                muted: Color::Rgb(83, 104, 126),
                cyan: Color::Rgb(0, 106, 135),
                green: Color::Rgb(18, 121, 73),
                amber: Color::Rgb(149, 93, 0),
                red: Color::Rgb(177, 41, 61),
            }
        } else {
            Self {
                bg: Color::Rgb(9, 15, 26),
                surface: Color::Rgb(15, 24, 39),
                elevated: Color::Rgb(24, 39, 57),
                border: Color::Rgb(42, 61, 82),
                text: Color::Rgb(222, 233, 243),
                muted: Color::Rgb(126, 151, 176),
                cyan: Color::Rgb(67, 211, 229),
                green: Color::Rgb(113, 222, 161),
                amber: Color::Rgb(249, 194, 105),
                red: Color::Rgb(246, 113, 130),
            }
        }
    }
    fn text(self) -> Style {
        Style::default().fg(self.text)
    }
    fn muted(self) -> Style {
        Style::default().fg(self.muted)
    }
    fn accent(self) -> Style {
        Style::default().fg(self.cyan).add_modifier(Modifier::BOLD)
    }
}

fn panel(title: impl Into<String>, p: Palette) -> Block<'static> {
    Block::default()
        .title(Line::from(Span::styled(
            format!(" {} ", title.into()),
            p.accent(),
        )))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(p.border))
        .style(Style::default().bg(p.surface).fg(p.text))
}

fn split(area: Rect, direction: Direction, constraints: &[Constraint]) -> Vec<Rect> {
    Layout::default()
        .direction(direction)
        .constraints(constraints)
        .split(area)
        .to_vec()
}

fn padded(area: Rect, horizontal: u16, vertical: u16) -> Rect {
    Rect {
        x: area.x.saturating_add(horizontal.min(area.width / 2)),
        y: area.y.saturating_add(vertical.min(area.height / 2)),
        width: area.width.saturating_sub(horizontal.saturating_mul(2)),
        height: area.height.saturating_sub(vertical.saturating_mul(2)),
    }
}

fn usage_color(value: f64, p: Palette) -> Color {
    if value >= 90.0 {
        p.red
    } else if value >= 75.0 {
        p.amber
    } else {
        p.cyan
    }
}

fn safe_percent(value: f64) -> f64 {
    if value.is_finite() {
        value.clamp(0.0, 100.0)
    } else {
        0.0
    }
}
/// Process/model/SMBIOS strings are untrusted, including terminal escape bytes.
fn clean(value: &str) -> String {
    value
        .chars()
        .map(|c| if c.is_control() { '�' } else { c })
        .collect()
}
fn available(value: &str) -> String {
    if value.trim().is_empty() {
        "Non disponibile".into()
    } else {
        clean(value)
    }
}
fn rate(value: f64) -> String {
    format!("{}/s", bytes(value.max(0.0) as u64))
}
fn optional_percent(value: Option<f32>) -> String {
    value
        .map(|v| format!("{v:.0}%"))
        .unwrap_or_else(|| "n/d".into())
}

/// Sidebar hit testing uses the exact same geometry as [`render`].
pub fn sidebar_tab_at(width: u16, height: u16, x: u16, y: u16) -> Option<usize> {
    let step = if height >= 28 { 2 } else { 1 };
    if width >= 100
        && height >= 24
        && (1..21).contains(&x)
        && y >= 6
        && (y - 6).is_multiple_of(step)
    {
        let index = ((y - 6) / step) as usize;
        (index < TABS.len() && y < height.saturating_sub(3)).then_some(index)
    } else {
        None
    }
}

/// Draw one application frame. This function performs no I/O and never samples
/// hardware, allowing continuous, deterministic UI regression tests.
pub fn render(
    frame: &mut Frame<'_>,
    inventory: &Inventory,
    snapshot: &Snapshot,
    llm: &LlmState,
    state: &mut UiState,
    history: &History,
) {
    let p = Palette::new(state.theme_light);
    let area = frame.area();
    frame.render_widget(
        Block::default().style(Style::default().bg(p.bg).fg(p.text)),
        area,
    );
    if area.width < 40 || area.height < 12 {
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(Span::styled("RUST-CPU", p.accent())),
                Line::from("Amplia il terminale"),
                Line::from("Minimo utile: 64 × 18"),
                Line::from("q esci · ? guida"),
            ])
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true }),
            area,
        );
        return;
    }
    let areas = split(
        area,
        Direction::Vertical,
        &[
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(2),
        ],
    );
    header(frame, areas[0], snapshot, state, p);
    let wide = area.width >= 100 && area.height >= 24;
    let main = if wide {
        let cols = split(
            areas[1],
            Direction::Horizontal,
            &[Constraint::Length(22), Constraint::Min(1)],
        );
        sidebar(frame, cols[0], state, p);
        cols[1]
    } else {
        let rows = split(
            areas[1],
            Direction::Vertical,
            &[Constraint::Length(3), Constraint::Min(1)],
        );
        let titles = TABS
            .iter()
            .enumerate()
            .map(|(i, label)| {
                let label = if area.width < 90 {
                    match i {
                        0 => "Vista",
                        2 => "MB",
                        3 => "RAM",
                        5 => "I/O",
                        6 => "Proc",
                        7 => "LLM",
                        8 => "Lab",
                        9 => "Diag",
                        _ => label,
                    }
                } else {
                    label
                };
                Line::from(format!("{} {label}", (i + 1) % 10))
            })
            .collect::<Vec<_>>();
        frame.render_widget(
            Tabs::new(titles)
                .select(state.active_tab.min(9))
                .divider(" ")
                .padding("", "")
                .style(p.muted())
                .highlight_style(p.accent().bg(p.elevated))
                .block(
                    Block::default()
                        .borders(Borders::BOTTOM)
                        .border_style(Style::default().fg(p.border)),
                ),
            rows[0],
        );
        rows[1]
    };
    let main = padded(main, 1, 0);
    match state.active_tab {
        0 => overview(frame, main, inventory, snapshot, state, history, p),
        1 => cpu_page(frame, main, inventory, snapshot, state, history, p),
        2 => board_page(frame, main, inventory, state, p),
        3 => memory_page(frame, main, inventory, snapshot, state, history, p),
        4 => gpu_page(frame, main, inventory, snapshot, state, p),
        5 => io_page(frame, main, snapshot, state, p),
        6 => process_page(frame, main, snapshot, state, p),
        7 => llm_page(frame, main, llm, state, p),
        8 => capacity_page(frame, main, snapshot, state, p),
        _ => diagnostics_page(frame, main, inventory, snapshot, llm, state, p),
    }
    footer(frame, areas[2], state, p);
    if state.help {
        help_modal(frame, area, p);
    }
    if state.process_detail {
        process_modal(frame, area, state, p);
    }
    if state.benchmark_confirm {
        benchmark_modal(frame, area, llm, state, p);
    }
    if state.cpu_benchmark_confirm {
        cpu_benchmark_modal(frame, area, p);
    }
}

fn header(frame: &mut Frame<'_>, area: Rect, snapshot: &Snapshot, state: &UiState, p: Palette) {
    let mut line = vec![
        Span::styled(" RUST", p.accent()),
        Span::styled("-CPU ", p.text().add_modifier(Modifier::BOLD)),
        Span::styled(" / HARDWARE + LOCAL AI", p.muted()),
    ];
    if area.width >= 88 {
        line.push(Span::styled("       ", p.muted()));
    }
    line.push(Span::styled(
        if state.paused { "  PAUSA " } else { "  LIVE " },
        Style::default()
            .fg(if state.paused { p.amber } else { p.green })
            .add_modifier(Modifier::BOLD),
    ));
    if state.recording {
        line.push(Span::styled(" ● REC", Style::default().fg(p.red)));
    }
    line.push(Span::styled(
        format!("  {:.1}s", state.tick_ms as f64 / 1000.0),
        p.muted(),
    ));
    if area.width >= 100 {
        line.push(Span::styled(
            format!(
                "  |  CPU {:4.1}%  RAM {:4.1}%",
                snapshot.cpu_percent,
                percent(snapshot.memory_used, snapshot.memory_total)
            ),
            p.text(),
        ));
    }
    frame.render_widget(
        Paragraph::new(Line::from(line)).block(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_style(Style::default().fg(p.border)),
        ),
        area,
    );
}

fn sidebar(frame: &mut Frame<'_>, area: Rect, state: &UiState, p: Palette) {
    frame.render_widget(
        Block::default()
            .borders(Borders::RIGHT)
            .border_style(Style::default().fg(p.border)),
        area,
    );
    let inner = padded(area, 1, 0);
    let mut lines = vec![
        Line::from(Span::styled(" WORKSPACE", p.muted())),
        Line::default(),
        Line::default(),
    ];
    for (index, title) in TABS.iter().enumerate() {
        let active = index == state.active_tab;
        lines.push(Line::from(vec![
            Span::styled(
                if active { "▌ " } else { "  " },
                Style::default().fg(p.cyan),
            ),
            Span::styled(format!("{} ", (index + 1) % 10), p.muted()),
            Span::styled(
                (*title).to_string(),
                if active {
                    p.accent().bg(p.elevated)
                } else {
                    p.text()
                },
            ),
        ]));
        if area.height >= 23 {
            lines.push(Line::default());
        }
    }
    frame.render_widget(Paragraph::new(lines), inner);
    if area.height >= 31 {
        let info = Rect::new(inner.x, area.bottom().saturating_sub(6), inner.width, 5);
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(Span::styled(
                    " TUTTO IN LOCALE",
                    Style::default().fg(p.green),
                )),
                Line::from(" Nessun account"),
                Line::from(" ?  comandi e metodo"),
            ])
            .style(p.muted()),
            info,
        );
    }
}

fn footer(frame: &mut Frame<'_>, area: Rect, state: &UiState, p: Palette) {
    let context = if state.filter_edit {
        "Cerca: digita · Invio applica · Esc chiude"
    } else if state.active_tab == 8 {
        "[ ] campo  + − modifica  Tab pagina  ? guida  q esci"
    } else if state.active_tab == 1 {
        "↑↓ scorri  x benchmark CPU  Tab pagina  Spazio pausa  ? guida  q esci"
    } else if state.active_tab == 6 {
        "↑↓ seleziona  / cerca  s ordina  a verso  Invio dettagli  ? guida"
    } else if state.active_tab == 7 {
        "↑↓ modello  b benchmark  Tab pagina  e esporta  ? guida  q esci"
    } else {
        "Tab pagina  ↑↓ scorri  Spazio pausa  e esporta  r registra  ? guida  q esci"
    };
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(context, p.muted())),
            Line::from(Span::styled(available(&state.status).to_string(), p.text())),
        ]),
        area,
    );
}

fn page_title(
    frame: &mut Frame<'_>,
    area: Rect,
    title: &str,
    subtitle: impl Into<String>,
    p: Palette,
) {
    let lines = if area.height > 1 {
        vec![
            Line::from(Span::styled(title.to_string(), p.accent())),
            Line::from(Span::styled(subtitle.into(), p.muted())),
        ]
    } else {
        vec![Line::from(Span::styled(title.to_string(), p.accent()))]
    };
    frame.render_widget(Paragraph::new(lines), area);
}

fn metric(frame: &mut Frame<'_>, area: Rect, title: &str, value: f64, label: String, p: Palette) {
    let value = safe_percent(value);
    frame.render_widget(
        Gauge::default()
            .block(panel(title, p))
            .gauge_style(Style::default().fg(usage_color(value, p)).bg(p.elevated))
            .label(Span::styled(label, p.text().add_modifier(Modifier::BOLD)))
            .ratio(value / 100.0)
            .use_unicode(true),
        area,
    );
}

fn graph(
    frame: &mut Frame<'_>,
    area: Rect,
    title: &str,
    values: &std::collections::VecDeque<u64>,
    color: Color,
    p: Palette,
) {
    let available = area.width.saturating_sub(2) as usize;
    let data = values
        .iter()
        .skip(values.len().saturating_sub(available))
        .copied()
        .collect::<Vec<_>>();
    frame.render_widget(
        Sparkline::default()
            .block(panel(title, p))
            .data(&data)
            .max(100)
            .style(Style::default().fg(color)),
        area,
    );
}

fn overview(
    frame: &mut Frame<'_>,
    area: Rect,
    inventory: &Inventory,
    snapshot: &Snapshot,
    state: &mut UiState,
    history: &History,
    p: Palette,
) {
    let compact = area.height < 19;
    let rows = split(
        area,
        Direction::Vertical,
        if compact {
            &[
                Constraint::Length(2),
                Constraint::Length(3),
                Constraint::Min(1),
            ][..]
        } else {
            &[
                Constraint::Length(3),
                Constraint::Length(5),
                Constraint::Length(6),
                Constraint::Min(3),
            ][..]
        },
    );
    page_title(
        frame,
        rows[0],
        "Panoramica della macchina",
        available(&inventory.cpu_name),
        p,
    );
    let cards = split(
        rows[1],
        Direction::Horizontal,
        &[
            Constraint::Percentage(34),
            Constraint::Percentage(33),
            Constraint::Percentage(33),
        ],
    );
    metric(
        frame,
        cards[0],
        "CPU",
        snapshot.cpu_percent as f64,
        format!(
            "{:.1}% · {} MHz",
            snapshot.cpu_percent, snapshot.frequency_mhz
        ),
        p,
    );
    metric(
        frame,
        cards[1],
        "Memoria",
        percent(snapshot.memory_used, snapshot.memory_total),
        format!(
            "{} / {}",
            bytes(snapshot.memory_used),
            bytes(snapshot.memory_total)
        ),
        p,
    );
    let gpu = snapshot.gpus.first();
    if let Some(value) = gpu.and_then(|g| g.utilization) {
        metric(
            frame,
            cards[2],
            "GPU",
            value as f64,
            format!("{value:.1}%"),
            p,
        );
    } else {
        frame.render_widget(
            Paragraph::new("Telemetria n/d")
                .alignment(Alignment::Center)
                .block(panel("GPU", p))
                .style(p.muted()),
            cards[2],
        );
    }
    if compact {
        let text = vec![
            Line::from(format!(
                "{} core / {} thread · {} processi",
                inventory.physical_cores,
                inventory.logical_cores,
                snapshot.processes.len()
            )),
            Line::from(format!(
                "GPU: {}",
                gpu.map(|g| clean(&g.name))
                    .unwrap_or_else(|| "Non rilevata".into())
            )),
            Line::from(format!(
                "LLM attivi: {} processi · raccolta {} ms",
                snapshot.processes.iter().filter(|r| r.is_llm).count(),
                snapshot.sampling_ms
            )),
            Line::from(Span::styled(
                "CPU 2 · Memoria 4 · Processi 7 · LLM 8 · Capacity 9",
                p.muted(),
            )),
        ];
        clamp_scroll(
            state,
            text.len()
                .saturating_sub(rows[2].height.saturating_sub(2) as usize),
        );
        frame.render_widget(
            Paragraph::new(text)
                .block(panel("Risorse", p))
                .scroll((state.offset, 0)),
            rows[2],
        );
        return;
    }
    let charts = split(
        rows[2],
        Direction::Horizontal,
        &[
            Constraint::Percentage(34),
            Constraint::Percentage(33),
            Constraint::Percentage(33),
        ],
    );
    graph(
        frame,
        charts[0],
        "CPU · ultimi 180 campioni",
        &history.cpu,
        p.cyan,
        p,
    );
    graph(
        frame,
        charts[1],
        "RAM · % della fisica",
        &history.memory,
        p.green,
        p,
    );
    if snapshot.gpus.first().and_then(|g| g.utilization).is_some() {
        graph(
            frame,
            charts[2],
            "GPU · utilizzo %",
            &history.gpu,
            p.amber,
            p,
        );
    } else {
        frame.render_widget(
            Paragraph::new("Utilizzo GPU non esposto")
                .wrap(Wrap { trim: true })
                .style(p.muted())
                .block(panel("GPU · telemetria n/d", p)),
            charts[2],
        );
    }
    let bottom = split(
        rows[3],
        Direction::Horizontal,
        &[Constraint::Percentage(55), Constraint::Percentage(45)],
    );
    clamp_scroll(state, core_max_offset(bottom[0], snapshot.per_core.len()));
    core_grid(
        frame,
        bottom[0],
        &snapshot.per_core,
        state.offset as usize,
        p,
    );
    let llm_processes = snapshot
        .processes
        .iter()
        .filter(|r| r.is_llm)
        .collect::<Vec<_>>();
    let llm_ram = llm_processes.iter().map(|r| r.memory_bytes).sum::<u64>();
    let network_rx = snapshot
        .networks
        .iter()
        .map(|n| n.received_per_sec)
        .sum::<f64>();
    let network_tx = snapshot
        .networks
        .iter()
        .map(|n| n.transmitted_per_sec)
        .sum::<f64>();
    let mut lines = vec![
        Line::from(vec![
            Span::styled("SISTEMA  ", p.muted()),
            Span::raw(available(&inventory.os).to_string()),
        ]),
        Line::from(format!(
            "{} core · {} thread · {} processi",
            inventory.physical_cores,
            inventory.logical_cores,
            snapshot.processes.len()
        )),
        Line::from(format!(
            "LLM: {} processi · RAM {}",
            llm_processes.len(),
            bytes(llm_ram)
        )),
        Line::from(format!(
            "Rete: ↓ {}   ↑ {}",
            rate(network_rx),
            rate(network_tx)
        )),
        Line::from(format!(
            "Swap: {} / {}",
            bytes(snapshot.swap_used),
            bytes(snapshot.swap_total)
        )),
    ];
    if let Some(sensor) = snapshot.sensors.first() {
        lines.push(Line::from(format!(
            "{}: {:.0} °C",
            clean(&sensor.name),
            sensor.temperature
        )));
    } else {
        lines.push(Line::from(Span::styled(
            "Sensori termici: non disponibili",
            p.muted(),
        )));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel("In questo momento", p))
            .wrap(Wrap { trim: true }),
        bottom[1],
    );
}

fn core_grid(frame: &mut Frame<'_>, area: Rect, cores: &[f32], offset: usize, p: Palette) {
    let block = panel(format!("Thread logici · {}", cores.len()), p);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if cores.is_empty() {
        frame.render_widget(
            Paragraph::new("Dati per-core non disponibili").style(p.muted()),
            inner,
        );
        return;
    }
    let cols = (inner.width / 17).clamp(1, 8) as usize;
    let visible_rows = inner.height as usize;
    let start = offset.min(core_max_offset(area, cores.len()));
    for (relative, value) in cores
        .iter()
        .enumerate()
        .skip(start)
        .take(cols * visible_rows)
    {
        let index = relative - start;
        let x = inner.x + (index % cols) as u16 * (inner.width / cols as u16);
        let width = if index % cols == cols - 1 {
            inner.right().saturating_sub(x)
        } else {
            inner.width / cols as u16
        };
        let target = Rect::new(x, inner.y + (index / cols) as u16, width, 1);
        let safe = safe_percent(*value as f64);
        frame.render_widget(
            Gauge::default()
                .gauge_style(Style::default().fg(usage_color(safe, p)).bg(p.elevated))
                .label(Span::styled(
                    format!(" {relative:02} {safe:4.0}% "),
                    p.text(),
                ))
                .ratio(safe / 100.0),
            target,
        );
    }
}

fn clamp_scroll(state: &mut UiState, max_offset: usize) {
    state.offset = state.offset.min(max_offset.min(u16::MAX as usize) as u16);
}

fn core_max_offset(area: Rect, count: usize) -> usize {
    let width = area.width.saturating_sub(2);
    let height = area.height.saturating_sub(2);
    let columns = (width / 17).clamp(1, 8) as usize;
    count.saturating_sub((columns * height as usize).max(1))
}

fn detail_max_offset(area: Rect, details: &[Detail]) -> usize {
    detail_lines(area, details, Palette::new(false))
        .len()
        .saturating_sub(area.height.saturating_sub(2).max(1) as usize)
}

/// Physical lines keep long instructions and DIMM/firmware values reachable,
/// including values taller than the entire viewport. Label and value columns
/// wrap independently using terminal-cell widths rather than byte counts.
fn detail_lines(area: Rect, details: &[Detail], p: Palette) -> Vec<Line<'static>> {
    let width = area.width.saturating_sub(2) as usize;
    let label_width = (width.saturating_sub(2) * 35 / 100).max(1);
    let value_width = width.saturating_sub(label_width + 2).max(1);
    let mut result = Vec::new();
    for (index, detail) in details.iter().enumerate() {
        let labels = wrap_diagnostic_lines(vec![Line::raw(clean(&detail.label))], label_width);
        let values = wrap_diagnostic_lines(vec![Line::raw(available(&detail.value))], value_width);
        let background = if index.is_multiple_of(2) {
            p.surface
        } else {
            p.bg
        };
        for row in 0..labels.len().max(values.len()) {
            let label = labels.get(row).map(ToString::to_string).unwrap_or_default();
            let value = values.get(row).map(ToString::to_string).unwrap_or_default();
            let padding = label_width.saturating_sub(Line::raw(&label).width());
            result.push(
                Line::from(vec![
                    Span::styled(label, p.muted()),
                    Span::raw(" ".repeat(padding + 2)),
                    Span::styled(value, p.text()),
                ])
                .style(Style::default().bg(background)),
            );
        }
    }
    result
}

fn detail_table(
    frame: &mut Frame<'_>,
    area: Rect,
    title: &str,
    details: &[Detail],
    offset: u16,
    p: Palette,
) {
    if details.is_empty() {
        frame.render_widget(
            Paragraph::new("Nessun dato disponibile dalla piattaforma.")
                .wrap(Wrap { trim: true })
                .block(panel(title, p))
                .style(p.muted()),
            area,
        );
    } else {
        frame.render_widget(
            Paragraph::new(detail_lines(area, details, p))
                .scroll((
                    offset.min(detail_max_offset(area, details).min(u16::MAX as usize) as u16),
                    0,
                ))
                .block(panel(format!("{title} · {} voci", details.len()), p)),
            area,
        );
    }
}

fn cpu_page(
    frame: &mut Frame<'_>,
    area: Rect,
    inventory: &Inventory,
    snapshot: &Snapshot,
    state: &mut UiState,
    history: &History,
    p: Palette,
) {
    let rows = split(
        area,
        Direction::Vertical,
        &[
            Constraint::Length(if area.height < 14 { 1 } else { 2 }),
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(3),
        ],
    );
    page_title(
        frame,
        rows[0],
        "CPU · identità e attività",
        available(&inventory.cpu_name),
        p,
    );
    metric(
        frame,
        rows[1],
        "Carico complessivo",
        snapshot.cpu_percent as f64,
        format!(
            "{:.1}% · {} MHz · {} core / {} thread",
            snapshot.cpu_percent,
            snapshot.frequency_mhz,
            inventory.physical_cores,
            inventory.logical_cores
        ),
        p,
    );
    if area.width >= 88 && area.height >= 18 {
        let cols = split(
            rows[2],
            Direction::Horizontal,
            &[Constraint::Percentage(55), Constraint::Percentage(45)],
        );
        let right = split(
            cols[1],
            Direction::Vertical,
            &[Constraint::Min(4), Constraint::Length(5)],
        );
        clamp_scroll(
            state,
            detail_max_offset(cols[0], &inventory.cpu)
                .max(core_max_offset(right[0], snapshot.per_core.len())),
        );
        detail_table(
            frame,
            cols[0],
            "Caratteristiche CPUID / sistema",
            &inventory.cpu,
            state.offset,
            p,
        );
        core_grid(
            frame,
            right[0],
            &snapshot.per_core,
            state.offset as usize,
            p,
        );
        graph(frame, right[1], "Storico CPU %", &history.cpu, p.cyan, p);
    } else {
        clamp_scroll(state, detail_max_offset(rows[2], &inventory.cpu));
        detail_table(
            frame,
            rows[2],
            "Caratteristiche CPU · ↑↓ scorri",
            &inventory.cpu,
            state.offset,
            p,
        );
    }
    let result = if state.cpu_benchmark_running {
        "Benchmark CPU in corso… carico locale attivo".to_string()
    } else if let Some(b) = &state.cpu_benchmark {
        format!(
            "Single {:.2} M mix/s · Multi {:.2} M mix/s · {} worker · {:.2}s",
            b.single_ops_per_sec / 1e6,
            b.multi_ops_per_sec / 1e6,
            b.threads,
            b.duration_secs
        )
    } else {
        "x avvia una misura single / multi thread · conferma prima del carico".to_string()
    };
    frame.render_widget(
        Paragraph::new(result)
            .wrap(Wrap { trim: true })
            .block(panel("Benchmark CPU · hash mixing nativo", p)),
        rows[3],
    );
}

fn board_page(
    frame: &mut Frame<'_>,
    area: Rect,
    inventory: &Inventory,
    state: &mut UiState,
    p: Palette,
) {
    let rows = split(
        area,
        Direction::Vertical,
        &[Constraint::Length(3), Constraint::Min(1)],
    );
    page_title(
        frame,
        rows[0],
        "Mainboard & firmware",
        "Identità SMBIOS, sistema operativo e firmware esposti dal sistema.",
        p,
    );
    clamp_scroll(state, detail_max_offset(rows[1], &inventory.board));
    detail_table(
        frame,
        rows[1],
        "Scheda madre / BIOS / sistema",
        &inventory.board,
        state.offset,
        p,
    );
}

fn memory_page(
    frame: &mut Frame<'_>,
    area: Rect,
    inventory: &Inventory,
    snapshot: &Snapshot,
    state: &mut UiState,
    history: &History,
    p: Palette,
) {
    let compact = area.height < 18;
    let rows = split(
        area,
        Direction::Vertical,
        if compact {
            &[
                Constraint::Length(2),
                Constraint::Length(3),
                Constraint::Min(1),
            ][..]
        } else {
            &[
                Constraint::Length(3),
                Constraint::Length(4),
                Constraint::Min(1),
                Constraint::Length(3),
            ][..]
        },
    );
    page_title(
        frame,
        rows[0],
        "Memoria · capacità e pressione",
        "RAM fisica, moduli rilevati e swap. I contatori descrivono istanti diversi.",
        p,
    );
    let cols = split(
        rows[1],
        Direction::Horizontal,
        &[Constraint::Percentage(65), Constraint::Percentage(35)],
    );
    metric(
        frame,
        cols[0],
        "RAM fisica",
        percent(snapshot.memory_used, snapshot.memory_total),
        format!(
            "{} / {} · {:.1}%",
            bytes(snapshot.memory_used),
            bytes(snapshot.memory_total),
            percent(snapshot.memory_used, snapshot.memory_total)
        ),
        p,
    );
    metric(
        frame,
        cols[1],
        "Swap",
        percent(snapshot.swap_used, snapshot.swap_total),
        format!(
            "{} / {}",
            bytes(snapshot.swap_used),
            bytes(snapshot.swap_total)
        ),
        p,
    );
    if !compact && area.width >= 88 {
        let cols = split(
            rows[2],
            Direction::Horizontal,
            &[Constraint::Percentage(65), Constraint::Percentage(35)],
        );
        clamp_scroll(state, detail_max_offset(cols[0], &inventory.memory));
        detail_table(
            frame,
            cols[0],
            "Moduli / configurazione",
            &inventory.memory,
            state.offset,
            p,
        );
        graph(
            frame,
            cols[1],
            "RAM · storico %",
            &history.memory,
            p.green,
            p,
        );
    } else {
        clamp_scroll(state, detail_max_offset(rows[2], &inventory.memory));
        detail_table(
            frame,
            rows[2],
            "Moduli / configurazione",
            &inventory.memory,
            state.offset,
            p,
        );
    }
    if !compact {
        frame.render_widget(Paragraph::new("La lettura SPD completa e i timing in tempo reale richiedono driver dedicati. I valori non esposti non sono stimati.").wrap(Wrap{trim:true}).style(p.muted()),rows[3]);
    }
}

fn gpu_page(
    frame: &mut Frame<'_>,
    area: Rect,
    inventory: &Inventory,
    snapshot: &Snapshot,
    state: &mut UiState,
    p: Palette,
) {
    let rows = split(
        area,
        Direction::Vertical,
        &[
            Constraint::Length(2),
            Constraint::Min(1),
            Constraint::Length(if area.height >= 18 { 4 } else { 2 }),
        ],
    );
    page_title(
        frame,
        rows[0],
        "GPU · accelerazione locale",
        "Utilizzo, memoria dedicata, temperatura e potenza quando esposti dal driver.",
        p,
    );
    if snapshot.gpus.is_empty() {
        clamp_scroll(state, detail_max_offset(rows[1], &inventory.graphics));
        detail_table(
            frame,
            rows[1],
            "Schede video rilevate",
            &inventory.graphics,
            state.offset,
            p,
        );
    } else {
        clamp_scroll(
            state,
            snapshot
                .gpus
                .len()
                .saturating_sub(rows[1].height.saturating_sub(4).max(2) as usize / 2),
        );
        let offset = state.offset as usize;
        let gpu_rows = snapshot.gpus.iter().skip(offset).map(|g| {
            let vram = match (g.memory_used, g.memory_total) {
                (Some(u), Some(t)) => format!("{} / {}", bytes(u), bytes(t)),
                (_, Some(t)) => format!("totale {}", bytes(t)),
                _ => "n/d".into(),
            };
            Row::new(vec![
                clean(&g.name),
                optional_percent(g.utilization),
                vram,
                g.temperature
                    .map(|v| format!("{v:.0} °C"))
                    .unwrap_or_else(|| "n/d".into()),
                g.power_watts
                    .map(|v| format!("{v:.0} W"))
                    .unwrap_or_else(|| "n/d".into()),
            ])
            .height(2)
        });
        frame.render_widget(
            Table::new(
                gpu_rows,
                [
                    Constraint::Min(15),
                    Constraint::Length(7),
                    Constraint::Length(if area.width >= 85 { 23 } else { 16 }),
                    Constraint::Length(8),
                    Constraint::Length(8),
                ],
            )
            .header(
                Row::new(["SCHEDA", "CARICO", "VRAM", "TEMP", "POWER"])
                    .style(p.muted())
                    .bottom_margin(1),
            )
            .column_spacing(1)
            .block(panel("Adattatori e telemetria", p)),
            rows[1],
        );
    }
    frame.render_widget(Paragraph::new("n/d = dato non esposto. La telemetria NVIDIA usa nvidia-smi; altri adattatori possono esporre solo l'identità. La memoria condivisa non equivale alla VRAM dedicata.").style(p.muted()).wrap(Wrap{trim:true}),rows[2]);
}

fn io_page(
    frame: &mut Frame<'_>,
    area: Rect,
    snapshot: &Snapshot,
    state: &mut UiState,
    p: Palette,
) {
    let rows = split(
        area,
        Direction::Vertical,
        &[
            Constraint::Length(if area.height < 14 { 1 } else { 2 }),
            Constraint::Percentage(52),
            Constraint::Min(1),
        ],
    );
    page_title(
        frame,
        rows[0],
        "Dischi & rete",
        "Spazio disponibile e traffico misurato tra campioni; non è un test di velocità.",
        p,
    );
    let max_disk = snapshot
        .disks
        .len()
        .saturating_sub(rows[1].height.saturating_sub(3).max(1) as usize);
    let max_network = snapshot
        .networks
        .len()
        .saturating_sub(rows[2].height.saturating_sub(3).max(1) as usize);
    clamp_scroll(state, max_disk.max(max_network));
    let disk_offset = (state.offset as usize).min(
        snapshot
            .disks
            .len()
            .saturating_sub(rows[1].height.saturating_sub(3).max(1) as usize),
    );
    let disk_rows = snapshot.disks.iter().skip(disk_offset).map(|d| {
        let used = d.total_bytes.saturating_sub(d.available_bytes);
        Row::new(vec![
            clean(&d.mount),
            clean(&d.name),
            clean(&d.kind),
            bytes(d.available_bytes),
            format!("{:.0}%", percent(used, d.total_bytes)),
            rate(d.read_bytes_per_sec),
            rate(d.write_bytes_per_sec),
        ])
    });
    frame.render_widget(
        Table::new(
            disk_rows,
            [
                Constraint::Length(8),
                Constraint::Min(8),
                Constraint::Length(5),
                Constraint::Length(12),
                Constraint::Length(6),
                Constraint::Length(12),
                Constraint::Length(12),
            ],
        )
        .column_spacing(1)
        .header(
            Row::new([
                "VOLUME",
                "DISCO",
                "TIPO",
                "LIBERO",
                "USATO",
                "LETTURA",
                "SCRITTURA",
            ])
            .style(p.muted()),
        )
        .block(panel(
            if snapshot.disks.is_empty() {
                "Dischi · nessun volume rilevato"
            } else {
                "Dischi / volumi"
            },
            p,
        )),
        rows[1],
    );
    let net_offset = (state.offset as usize).min(
        snapshot
            .networks
            .len()
            .saturating_sub(rows[2].height.saturating_sub(3).max(1) as usize),
    );
    let net_rows = snapshot.networks.iter().skip(net_offset).map(|n| {
        Row::new(vec![
            clean(&n.name),
            rate(n.received_per_sec),
            rate(n.transmitted_per_sec),
            bytes(n.total_received),
            bytes(n.total_transmitted),
        ])
    });
    frame.render_widget(
        Table::new(
            net_rows,
            [
                Constraint::Min(10),
                Constraint::Length(13),
                Constraint::Length(13),
                Constraint::Length(13),
                Constraint::Length(13),
            ],
        )
        .column_spacing(1)
        .header(
            Row::new([
                "INTERFACCIA",
                "↓ RICEZIONE",
                "↑ INVIO",
                "RX TOTALE",
                "TX TOTALE",
            ])
            .style(p.muted()),
        )
        .block(panel(
            if snapshot.networks.is_empty() {
                "Rete · nessuna interfaccia rilevata"
            } else {
                "Rete · totali dall'avvio del sistema / interfaccia"
            },
            p,
        )),
        rows[2],
    );
}

fn process_page(
    frame: &mut Frame<'_>,
    area: Rect,
    snapshot: &Snapshot,
    state: &mut UiState,
    p: Palette,
) {
    let rows = split(
        area,
        Direction::Vertical,
        &[
            Constraint::Length(2),
            Constraint::Length(3),
            Constraint::Min(1),
        ],
    );
    page_title(
        frame,
        rows[0],
        "Processi · dove vanno le risorse",
        "CPU normalizzata sulla macchina · LLM = riconoscimento euristico del nome.",
        p,
    );
    let sort = match state.sort {
        SortColumn::Cpu => "CPU",
        SortColumn::Memory => "RAM",
        SortColumn::Name => "Nome",
        SortColumn::Pid => "PID",
    };
    let search = if state.query.is_empty() {
        if state.filter_edit {
            "▏".into()
        } else {
            "/ per cercare nome o PID".into()
        }
    } else {
        format!(
            "{}{}",
            clean(&state.query),
            if state.filter_edit { "▏" } else { "" }
        )
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                search,
                if state.filter_edit {
                    p.accent()
                } else {
                    p.text()
                },
            ),
            Span::styled(
                format!("   |   {sort} {}", if state.ascending { "↑" } else { "↓" }),
                p.muted(),
            ),
        ]))
        .block(panel("Filtro processi", p)),
        rows[1],
    );
    let filtered = filtered_processes(snapshot, state);
    state.selected = state.selected.min(filtered.len().saturating_sub(1));
    let body = filtered.iter().map(|proc| {
        Row::new(vec![
            Cell::from(proc.pid.to_string()).style(p.muted()),
            Cell::from(format!(
                "{}{}",
                if proc.is_llm { "◆ " } else { "" },
                clean(&proc.name)
            ))
            .style(if proc.is_llm {
                Style::default().fg(p.green)
            } else {
                p.text()
            }),
            Cell::from(format!("{:.1}%", proc.cpu_percent))
                .style(Style::default().fg(usage_color(proc.cpu_percent as f64, p))),
            Cell::from(bytes(proc.memory_bytes)),
            Cell::from(rate(proc.read_bytes_per_sec)),
            Cell::from(rate(proc.write_bytes_per_sec)),
        ])
    });
    let widths = if area.width >= 88 {
        vec![
            Constraint::Length(7),
            Constraint::Min(14),
            Constraint::Length(8),
            Constraint::Length(12),
            Constraint::Length(12),
            Constraint::Length(12),
        ]
    } else {
        vec![
            Constraint::Length(6),
            Constraint::Min(10),
            Constraint::Length(7),
            Constraint::Length(10),
            Constraint::Length(0),
            Constraint::Length(0),
        ]
    };
    let table = Table::new(body, widths)
        .column_spacing(1)
        .header(
            Row::new(["PID", "PROCESSO", "CPU", "RAM", "LETTURA", "SCRITTURA"])
                .style(p.muted())
                .bottom_margin(1),
        )
        .row_highlight_style(Style::default().bg(p.elevated).add_modifier(Modifier::BOLD))
        .highlight_symbol("▌ ")
        .block(panel(
            format!("{} risultati · Invio per i dettagli", filtered.len()),
            p,
        ));
    let mut table_state =
        TableState::default().with_selected((!filtered.is_empty()).then_some(state.selected));
    frame.render_stateful_widget(table, rows[2], &mut table_state);
}

fn llm_page(frame: &mut Frame<'_>, area: Rect, llm: &LlmState, state: &mut UiState, p: Palette) {
    let tall = area.height >= 21;
    let tiny = area.height < 14;
    let rows = split(
        area,
        Direction::Vertical,
        &[
            Constraint::Length(if tiny { 1 } else { 2 }),
            Constraint::Length(if tall {
                6
            } else if tiny {
                3
            } else {
                4
            }),
            Constraint::Min(3),
            Constraint::Length(if tall {
                7
            } else if tiny {
                1
            } else {
                3
            }),
        ],
    );
    page_title(
        frame,
        rows[0],
        "LLM locali · runtime, modelli e misure",
        "Endpoint localhost · elenco rilevato, nessun download automatico.",
        p,
    );
    let runtime_rows = llm.runtimes.iter().map(|r| {
        let ok = r.status.eq_ignore_ascii_case("online")
            || r.status.eq_ignore_ascii_case("ready")
            || r.status.eq_ignore_ascii_case("ok");
        Row::new(vec![
            Cell::from(clean(&r.name)),
            Cell::from(clean(&r.status)).style(Style::default().fg(if ok {
                p.green
            } else {
                p.amber
            })),
            Cell::from(clean(&r.endpoint)),
            Cell::from(clean(&r.message)),
        ])
    });
    if tiny {
        let summary = if llm.runtimes.is_empty() {
            "Rilevamento in corso / non disponibile".into()
        } else {
            llm.runtimes
                .iter()
                .map(|r| format!("{}: {}", clean(&r.name), clean(&r.status)))
                .collect::<Vec<_>>()
                .join(" · ")
        };
        frame.render_widget(
            Paragraph::new(summary)
                .style(p.text())
                .block(panel("Runtime locali", p)),
            rows[1],
        );
    } else {
        frame.render_widget(
            Table::new(
                runtime_rows,
                [
                    Constraint::Length(12),
                    Constraint::Length(12),
                    Constraint::Length(if area.width >= 90 { 30 } else { 22 }),
                    Constraint::Min(8),
                ],
            )
            .header(Row::new(["RUNTIME", "STATO", "ENDPOINT", "DETTAGLIO"]).style(p.muted()))
            .column_spacing(1)
            .block(panel(
                if llm.runtimes.is_empty() {
                    "Runtime · rilevamento in corso / non disponibile"
                } else {
                    "Runtime locali"
                },
                p,
            )),
            rows[1],
        );
    }
    let models = llm
        .runtimes
        .iter()
        .flat_map(|r| r.models.iter().map(move |m| (r, m)))
        .collect::<Vec<_>>();
    state.selected = state.selected.min(models.len().saturating_sub(1));
    let model_rows = models.iter().map(|(r, m)| {
        Row::new(vec![
            clean(&r.name),
            clean(&m.id),
            m.size_bytes.map(bytes).unwrap_or_else(|| "n/d".into()),
            m.vram_bytes.map(bytes).unwrap_or_else(|| "n/d".into()),
            m.context_length
                .map(|v| v.to_string())
                .unwrap_or_else(|| "n/d".into()),
            m.quantization
                .as_deref()
                .map(clean)
                .unwrap_or_else(|| "n/d".into()),
            match m.loaded {
                Some(true) => "caricato",
                Some(false) => "su disco",
                None => "n/d",
            }
            .into(),
        ])
    });
    let mut table_state =
        TableState::default().with_selected((!models.is_empty()).then_some(state.selected));
    let model_widths = if area.width >= 88 {
        vec![
            Constraint::Length(10),
            Constraint::Min(14),
            Constraint::Length(10),
            Constraint::Length(10),
            Constraint::Length(8),
            Constraint::Length(8),
            Constraint::Length(9),
        ]
    } else {
        vec![
            Constraint::Length(8),
            Constraint::Min(16),
            Constraint::Length(9),
            Constraint::Length(9),
            Constraint::Length(0),
            Constraint::Length(0),
            Constraint::Length(8),
        ]
    };
    let table = Table::new(model_rows, model_widths)
        .column_spacing(1)
        .header(
            Row::new([
                "RUNTIME", "MODELLO", "PESO", "VRAM", "CONTESTO", "QUANT", "STATO",
            ])
            .style(p.muted()),
        )
        .highlight_symbol("▌ ")
        .row_highlight_style(
            Style::default()
                .bg(p.elevated)
                .fg(p.cyan)
                .add_modifier(Modifier::BOLD),
        )
        .block(panel(
            format!(
                "{} modelli · ↑↓ seleziona · b misura inferenza",
                models.len()
            ),
            p,
        ));
    frame.render_stateful_widget(table, rows[2], &mut table_state);
    let benchmark_lines = if state.benchmark_running {
        vec![
            Line::from(Span::styled(
                "Misura in corso… il runtime sta eseguendo inferenza locale.",
                Style::default().fg(p.amber),
            )),
            Line::from("L'interfaccia continua a ricevere campioni hardware."),
        ]
    } else if let Some(b) = &llm.benchmark {
        vec![
            Line::from(Span::styled(
                format!("{} / {}", clean(&b.runtime), clean(&b.model)),
                p.accent(),
            )),
            Line::from(format!(
                "{} token output · {:.2}s totali · generazione {}",
                b.output_tokens
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "n/d".into()),
                b.wall_seconds,
                b.generation_tokens_per_sec
                    .map(|v| format!("{v:.2} token/s"))
                    .unwrap_or_else(|| "n/d".into())
            )),
            Line::from(format!(
                "TTFT {} · caricamento {} · prompt {}",
                b.time_to_first_token_ms
                    .map(|v| format!("{v:.0} ms"))
                    .unwrap_or_else(|| "n/d".into()),
                b.load_ms
                    .map(|v| format!("{v:.0} ms"))
                    .unwrap_or_else(|| "n/d".into()),
                b.prompt_tokens_per_sec
                    .map(|v| format!("{v:.1} token/s"))
                    .unwrap_or_else(|| "n/d".into())
            )),
            Line::from(Span::styled(clean(&b.note), p.muted())),
        ]
    } else {
        vec![
            Line::from("Seleziona un modello, poi b: richiesta locale con massimo 64 token."),
            Line::from(Span::styled(
                "Nessun benchmark eseguito. Disponibilità e velocità dipendono dal runtime.",
                p.muted(),
            )),
        ]
    };
    if tiny {
        frame.render_widget(
            Paragraph::new(if state.benchmark_running {
                "Benchmark in corso…"
            } else {
                "b: benchmark locale · max 64 token"
            })
            .style(p.muted()),
            rows[3],
        );
    } else {
        frame.render_widget(
            Paragraph::new(benchmark_lines)
                .wrap(Wrap { trim: true })
                .block(panel("Ultimo benchmark", p)),
            rows[3],
        );
    }
}

fn capacity_page(
    frame: &mut Frame<'_>,
    area: Rect,
    snapshot: &Snapshot,
    state: &UiState,
    p: Palette,
) {
    let compact = area.height < 21;
    let tiny = area.height < 14;
    let rows = split(
        area,
        Direction::Vertical,
        &[
            Constraint::Length(if tiny { 1 } else { 2 }),
            Constraint::Min(1),
            Constraint::Length(if tiny {
                1
            } else if compact {
                3
            } else {
                5
            }),
        ],
    );
    page_title(
        frame,
        rows[0],
        "Capacity Lab · prima di caricare un modello",
        "Stima modificabile · [ ] scegli il campo · + − cambia valore.",
        p,
    );
    let cols = split(
        rows[1],
        Direction::Horizontal,
        &[Constraint::Percentage(45), Constraint::Percentage(55)],
    );
    let c = &state.capacity;
    let parameters = [
        ("Parametri", format!("{:.1} B", c.parameters_b)),
        ("Bit / peso", format!("{:.1}", c.bits_per_weight)),
        ("Contesto", format!("{} token", c.context)),
        ("Layer", c.layers.to_string()),
        ("Head KV", c.kv_heads.to_string()),
        ("Dimensione head", c.head_dim.to_string()),
    ];
    let visible_parameters = cols[0].height.saturating_sub(3).max(1) as usize;
    let parameter_offset = (state.capacity_field.min(5) + 1).saturating_sub(visible_parameters);
    let parameter_rows =
        parameters
            .iter()
            .enumerate()
            .skip(parameter_offset)
            .map(|(i, (name, value))| {
                Row::new(vec![
                    format!(
                        "{} {name}",
                        if i == state.capacity_field {
                            "›"
                        } else {
                            " "
                        }
                    ),
                    value.clone(),
                ])
                .style(if i == state.capacity_field {
                    p.accent().bg(p.elevated)
                } else {
                    p.text()
                })
            });
    frame.render_widget(
        Table::new(
            parameter_rows,
            [Constraint::Percentage(58), Constraint::Percentage(42)],
        )
        .column_spacing(1)
        .header(Row::new(["PARAMETRO", "VALORE"]).style(p.muted()))
        .block(panel("Architettura modello", p)),
        cols[0],
    );
    let required = c.estimated_bytes().max(0.0) as u64;
    let free_ram = snapshot.memory_total.saturating_sub(snapshot.memory_used);
    let ram_known = snapshot.memory_total > 0;
    let ram_ok = ram_known && required <= free_ram;
    let ram_color = if !ram_known {
        p.muted
    } else if ram_ok {
        p.green
    } else {
        p.amber
    };
    let (vram_total, vram_free) = snapshot
        .gpus
        .iter()
        .filter_map(|g| {
            g.memory_total
                .zip(g.memory_used)
                .map(|(total, used)| (total, total.saturating_sub(used)))
        })
        .max_by_key(|(_, free)| *free)
        .unwrap_or((0, 0));
    let vram_status = if vram_total == 0 {
        "n/d · VRAM non esposta".to_string()
    } else if required <= vram_free {
        "Compatibile con VRAM libera".to_string()
    } else {
        "VRAM insufficiente per offload completo".to_string()
    };
    let mut lines = vec![
        Line::from(vec![
            Span::styled("STIMA TOTALE  ", p.muted()),
            Span::styled(bytes(required), p.accent()),
        ]),
        Line::from(format!(
            "Pesi: {}",
            bytes(c.weights_bytes().max(0.0) as u64)
        )),
        Line::from(format!(
            "Cache KV FP16: {}",
            bytes(c.kv_bytes().max(0.0) as u64)
        )),
        Line::from("Overhead: 15% dei pesi + 512 MiB"),
        Line::default(),
        Line::from(Span::styled(
            format!(
                "RAM libera: {}",
                if ram_known {
                    bytes(free_ram)
                } else {
                    "n/d".into()
                }
            ),
            p.text(),
        )),
        Line::from(Span::styled(
            if !ram_known {
                "RAM: n/d · campione non disponibile"
            } else if ram_ok {
                "RAM: capienza stimata sufficiente"
            } else {
                "RAM: capienza stimata insufficiente"
            },
            Style::default().fg(ram_color),
        )),
        Line::from(format!(
            "VRAM libera (una GPU): {}",
            if vram_total == 0 {
                "n/d".into()
            } else {
                bytes(vram_free)
            }
        )),
        Line::from(Span::styled(
            vram_status,
            Style::default().fg(if vram_total > 0 && required <= vram_free {
                p.green
            } else {
                p.amber
            }),
        )),
    ];
    if !compact {
        lines.extend([
            Line::default(),
            Line::from(Span::styled(
                "RAM e VRAM sono vincoli separati.",
                p.accent(),
            )),
            Line::from("L'offload parziale dipende dal motore."),
            Line::from("Il multi-GPU non è sommato automaticamente."),
        ]);
    }
    if tiny {
        lines = vec![
            Line::from(Span::styled(
                format!("Stima {}", bytes(required)),
                p.accent(),
            )),
            Line::from(Span::styled(
                format!(
                    "RAM {} · {}",
                    if ram_known {
                        bytes(free_ram)
                    } else {
                        "n/d".into()
                    },
                    if !ram_known {
                        "n/d"
                    } else if ram_ok {
                        "OK"
                    } else {
                        "insuff."
                    }
                ),
                Style::default().fg(ram_color),
            )),
            Line::from(Span::styled(
                format!(
                    "VRAM {} · {}",
                    if vram_total == 0 {
                        "n/d".into()
                    } else {
                        bytes(vram_free)
                    },
                    if vram_total == 0 {
                        "n/d"
                    } else if required <= vram_free {
                        "OK"
                    } else {
                        "insuff."
                    }
                ),
                Style::default().fg(if vram_total > 0 && required <= vram_free {
                    p.green
                } else {
                    p.amber
                }),
            )),
            Line::from(Span::styled("RAM / VRAM separate", p.muted())),
        ];
    }
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .block(panel("Budget memoria · stima", p)),
        cols[1],
    );
    frame.render_widget(Paragraph::new(if tiny{"Stima batch 1, KV FP16 · RAM e VRAM sono vincoli separati"}else{"Stima orientativa per modelli transformer densi, batch 1, KV FP16. Quantizzazione, cache, batch, MoE e backend cambiano l'uso reale. Non prevede token/s e non garantisce il caricamento. Il contesto configurato è distinto da quello effettivamente allocato dal runtime."}).wrap(Wrap{trim:true}).style(p.muted()),rows[2]);
}

fn diagnostics_page(
    frame: &mut Frame<'_>,
    area: Rect,
    inventory: &Inventory,
    snapshot: &Snapshot,
    llm: &LlmState,
    state: &mut UiState,
    p: Palette,
) {
    let rows = split(
        area,
        Direction::Vertical,
        &[Constraint::Length(2), Constraint::Min(1)],
    );
    page_title(
        frame,
        rows[0],
        "Diagnostica · fonti e limiti",
        "Dati mancanti sempre espliciti. Nessun valore sintetico spacciato per una misura.",
        p,
    );
    let mut lines = vec![
        Line::from(Span::styled("COPERTURA DEL CAMPIONE", p.accent())),
        Line::from(format!(
            "CPU {} · RAM {} · thread {} · processi {}",
            if snapshot.per_core.is_empty() {
                "n/d"
            } else {
                "attiva"
            },
            if snapshot.memory_total == 0 {
                "n/d"
            } else {
                "attiva"
            },
            snapshot.per_core.len(),
            snapshot.processes.len()
        )),
        Line::from(format!(
            "GPU {} · sensori {} · dischi {} · interfacce {}",
            snapshot.gpus.len(),
            snapshot.sensors.len(),
            snapshot.disks.len(),
            snapshot.networks.len()
        )),
        Line::from(format!(
            "Costo raccolta: {} ms · runtime verificati: {}",
            snapshot.sampling_ms,
            llm.runtimes.len()
        )),
        Line::default(),
        Line::from(Span::styled("AVVISI DEI COLLETTORI", p.accent())),
    ];
    if inventory.warnings.is_empty() && snapshot.warnings.is_empty() {
        lines.push(Line::from(Span::styled(
            "Nessun avviso segnalato dai collettori.",
            Style::default().fg(p.green),
        )));
    }
    for warning in inventory.warnings.iter().chain(snapshot.warnings.iter()) {
        lines.push(Line::from(Span::styled(
            format!("• {}", clean(warning)),
            Style::default().fg(p.amber),
        )));
    }
    lines.extend([
        Line::default(),Line::from(Span::styled("COME LEGGERE I DATI",p.accent())),
        Line::from("• CPU processi: percentuale normalizzata sulla capacità totale della macchina."),
        Line::from("• Frequenza: lettura fornita dal sistema; può differire dal clock effettivo istantaneo."),
        Line::from("• RAM processo: memoria residente; la somma include pagine condivise."),
        Line::from("• Primo campione I/O: può essere zero; le velocità richiedono due rilevazioni."),
        Line::from("• LLM: ricerca dei runtime configurati; il nome processo è un indizio, non una prova."),
        Line::from("• SPD completo, timing RAM e tensioni richiedono accesso hardware / driver non inclusi."),
        Line::from("• n/d: sorgente assente o dato non esposto; non equivale a zero."),
        Line::default(),Line::from(Span::styled("PRIVACY E REPORT",p.accent())),
        Line::from("Monitoraggio locale. I report esportati possono contenere nomi del dispositivo, processi e modelli: controllali prima di condividerli."),
        Line::from("Spazio congela la vista. La registrazione, quando attiva, continua a ricevere i campioni."),
    ]);
    let wrapped = wrap_diagnostic_lines(lines, rows[1].width.saturating_sub(2) as usize);
    let max_offset = wrapped
        .len()
        .saturating_sub(rows[1].height.saturating_sub(2) as usize)
        .min(u16::MAX as usize) as u16;
    let paragraph = Paragraph::new(wrapped).block(panel("Trasparenza delle misure · ↑↓ scorri", p));
    state.offset = state.offset.min(max_offset);
    frame.render_widget(paragraph.scroll((state.offset, 0)), rows[1]);
}

/// Pre-wrap diagnostic prose using stable terminal-cell widths. Keeping the
/// resulting physical lines also makes End exact without an unstable Ratatui API.
fn wrap_diagnostic_lines(lines: Vec<Line<'_>>, width: usize) -> Vec<Line<'static>> {
    let width = width.max(1);
    let mut result = Vec::new();
    for line in lines {
        let text = line
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect::<String>();
        let style = line
            .spans
            .first()
            .map(|span| line.style.patch(span.style))
            .unwrap_or(line.style);
        let mut current = String::new();
        let mut current_width = 0;
        for word in text.split_whitespace() {
            let word_line = Line::raw(word);
            let word_width = word_line.width();
            if current_width > 0 && current_width + 1 + word_width > width {
                result.push(Line::styled(std::mem::take(&mut current), style));
                current_width = 0;
            }
            if word_width <= width {
                if current_width > 0 {
                    current.push(' ');
                    current_width += 1;
                }
                current.push_str(word);
                current_width += word_width;
            } else {
                for grapheme in word_line.styled_graphemes(Style::default()) {
                    let cell_width = Span::raw(grapheme.symbol).width();
                    if current_width > 0 && current_width + cell_width > width {
                        result.push(Line::styled(std::mem::take(&mut current), style));
                        current_width = 0;
                    }
                    if cell_width <= width {
                        current.push_str(grapheme.symbol);
                        current_width += cell_width;
                    }
                }
            }
        }
        result.push(Line::styled(current, style));
    }
    result
}

fn modal_area(area: Rect, width: u16, height: u16) -> Rect {
    let width = width
        .min(area.width.saturating_sub(2))
        .max(1)
        .min(area.width);
    let height = height
        .min(area.height.saturating_sub(2))
        .max(1)
        .min(area.height);
    Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    )
}

fn help_modal(frame: &mut Frame<'_>, area: Rect, p: Palette) {
    let target = modal_area(area, 88, 28);
    frame.render_widget(Clear, target);
    if target.height < 24 {
        let lines = vec![
            Line::from(Span::styled("COMANDI · Esc / ? chiude", p.accent())),
            Line::from("Tab / Shift-Tab · ← → · 1…9,0: pagine"),
            Line::from("↑↓ / PgUp PgDn / Home End: scorri, seleziona"),
            Line::from("Processi: / cerca · s ordina · a verso · Invio dettagli"),
            Line::from("Spazio pausa · + − intervallo · t tema"),
            Line::from("e esporta · r registra · q esci"),
            Line::from("CPU: x poi Invio benchmark (~1,2s di carico)"),
            Line::from("LLM: ↑↓ modello · b poi Invio (max 64 token)"),
            Line::from("Capacity: [ ] campo · + − valore"),
            Line::from(Span::styled(
                "RAM e VRAM: vincoli separati · n/d: dato assente",
                p.muted(),
            )),
        ];
        frame.render_widget(
            Paragraph::new(lines)
                .wrap(Wrap { trim: true })
                .block(panel("Guida rapida", p)),
            target,
        );
        return;
    }
    let lines = vec![
        Line::from(Span::styled(
            "Una postazione di osservazione, tutta da tastiera.",
            p.accent(),
        )),
        Line::default(),
        Line::from("Tab / Shift-Tab   Pagina successiva / precedente"),
        Line::from("← →  oppure 1…9,0 Seleziona le 10 pagine"),
        Line::from("↑ ↓ · PgUp PgDn   Scorri dettagli / seleziona un processo o modello"),
        Line::from("Home / End        Primo / ultimo elemento"),
        Line::from("/                 Cerca processi per nome o PID; Invio applica"),
        Line::from("s / a             Cambia colonna di ordinamento / direzione"),
        Line::from("Invio             Dettagli del processo selezionato"),
        Line::from("Spazio            Congela / riprende la vista hardware"),
        Line::from("+ / −             Aumenta / riduce l'intervallo dei campioni"),
        Line::from("e / r             Esporta report / avvia o ferma registrazione CSV"),
        Line::from("t                 Tema scuro / chiaro"),
        Line::default(),
        Line::from(Span::styled("LLM LOCALI E CAPACITY LAB", p.accent())),
        Line::from("LLM: ↑↓ modello, b poi Invio per inferenza locale (max 64 token)."),
        Line::from("CPU: x poi Invio per carico hash mixing single / multi thread (~1,2s)."),
        Line::from("Capacity: [ ] campo; + − modifica il parametro selezionato."),
        Line::from("La stima tratta RAM e VRAM separatamente. Non è un benchmark."),
        Line::default(),
        Line::from(Span::styled(
            "Esc / ? chiude la guida · q esce dall'applicazione",
            Style::default().fg(p.amber),
        )),
    ];
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .block(panel("Guida rapida · RUST-CPU", p)),
        target,
    );
}

fn benchmark_modal(frame: &mut Frame<'_>, area: Rect, llm: &LlmState, state: &UiState, p: Palette) {
    let selected = llm
        .runtimes
        .iter()
        .flat_map(|r| r.models.iter().map(move |m| (r, m)))
        .nth(state.selected);
    let target = modal_area(area, 76, 17);
    frame.render_widget(Clear, target);
    let mut lines = vec![
        Line::from(Span::styled(
            "Eseguire una breve inferenza locale?",
            p.accent(),
        )),
        Line::default(),
    ];
    if let Some((runtime, model)) = selected {
        lines.extend([
            Line::from(format!(
                "Runtime: {} · {}",
                clean(&runtime.name),
                clean(&runtime.endpoint)
            )),
            Line::from(format!("Modello: {}", clean(&model.id))),
            Line::default(),
            Line::from("Invia un prompt di prova e richiede al massimo 64 token di output."),
            Line::from("Il runtime può caricare il modello e usare CPU, RAM e GPU."),
            Line::from("Non scarica modelli. Le misure dipendono dalle metriche del runtime."),
            Line::default(),
            Line::from(Span::styled(
                "Invio  esegui benchmark    Esc  annulla",
                Style::default().fg(p.amber),
            )),
        ]);
    } else {
        lines.extend([
            Line::from("Nessun modello selezionabile. Avvia un runtime e installa un modello."),
            Line::from(Span::styled(
                "Esc  torna ai runtime",
                Style::default().fg(p.amber),
            )),
        ]);
    }
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .block(panel("Conferma benchmark", p)),
        target,
    );
}

fn cpu_benchmark_modal(frame: &mut Frame<'_>, area: Rect, p: Palette) {
    let target = modal_area(area, 76, 16);
    frame.render_widget(Clear, target);
    let lines = vec![
        Line::from(Span::styled(
            "Misurare la capacità di calcolo della CPU?",
            p.accent(),
        )),
        Line::default(),
        Line::from("Esegue circa 1,2 secondi di hash mixing nativo: una fase single"),
        Line::from("thread e una fase multi thread. Aumenta temporaneamente il carico"),
        Line::from("e il consumo del processore; le altre attività possono rallentare."),
        Line::default(),
        Line::from("Il risultato è espresso in milioni di mix al secondo."),
        Line::from("È un test di RUST-CPU; non è un punteggio CPU-Z e non è"),
        Line::from("direttamente confrontabile con altri benchmark."),
        Line::default(),
        Line::from(Span::styled(
            "Invio  esegui benchmark    Esc  annulla",
            Style::default().fg(p.amber),
        )),
    ];
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .block(panel("Conferma benchmark CPU", p)),
        target,
    );
}

fn process_modal(frame: &mut Frame<'_>, area: Rect, state: &UiState, p: Palette) {
    let target = modal_area(area, 76, 16);
    frame.render_widget(Clear, target);
    let lines = if let Some(proc) = &state.process_detail_row {
        vec![
            Line::from(Span::styled(clean(&proc.name), p.accent())),
            Line::from(Span::styled("Istantanea all’apertura", p.muted())),
            Line::default(),
            Line::from(format!("PID             {}", proc.pid)),
            Line::from(format!("CPU macchina    {:.2}%", proc.cpu_percent)),
            Line::from(format!("RAM residente   {}", bytes(proc.memory_bytes))),
            Line::from(format!("Lettura         {}", rate(proc.read_bytes_per_sec))),
            Line::from(format!(
                "Scrittura       {}",
                rate(proc.write_bytes_per_sec)
            )),
            Line::from(format!(
                "Indizio LLM     {}",
                if proc.is_llm {
                    "Sì · identificazione dal nome"
                } else {
                    "Non rilevato"
                }
            )),
            Line::default(),
            Line::from(Span::styled(
                "Valori fissati all'apertura del dettaglio. Esc per chiudere.",
                p.muted(),
            )),
        ]
    } else {
        vec![
            Line::from("Il processo selezionato non è più disponibile."),
            Line::from("Esc per chiudere."),
        ]
    };
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .block(panel("Dettaglio processo", p)),
        target,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    fn fixture() -> (Inventory, Snapshot, LlmState, History) {
        let inv = Inventory {
            cpu_name: "CPU di prova · 日本語 🦀 con un nome molto lungo".repeat(4),
            os: "Windows 11".into(),
            physical_cores: 8,
            logical_cores: 16,
            cpu: vec![
                Detail::new("Vendor", "GenuineIntel"),
                Detail::new("ISA", "SSE AVX AVX2"),
            ],
            board: vec![Detail::new("Produttore", "Test board")],
            memory: vec![Detail::new("DIMM 0", "16 GiB DDR5")],
            graphics: vec![Detail::new("Adattatore", "Test GPU")],
            warnings: vec!["SPD non disponibile".into()],
        };
        let snapshot = Snapshot {
            cpu_percent: 42.5,
            per_core: vec![12.0, 50.0, 98.0, 100.0],
            frequency_mhz: 4200,
            memory_used: 8 << 30,
            memory_total: 32 << 30,
            swap_total: 8 << 30,
            processes: vec![ProcessRow {
                pid: 42,
                name: "ollama.exe".into(),
                cpu_percent: 10.0,
                memory_bytes: 2 << 30,
                is_llm: true,
                ..Default::default()
            }],
            disks: vec![DiskRow {
                name: "NVMe".into(),
                mount: "C:".into(),
                total_bytes: 1 << 40,
                available_bytes: 500 << 30,
                ..Default::default()
            }],
            networks: vec![NetworkRow {
                name: "Ethernet".into(),
                received_per_sec: 1024.0,
                ..Default::default()
            }],
            gpus: vec![GpuRow {
                name: "Test GPU".into(),
                utilization: Some(50.0),
                memory_used: Some(2 << 30),
                memory_total: Some(8 << 30),
                ..Default::default()
            }],
            sampling_ms: 1000,
            ..Default::default()
        };
        let llm = LlmState {
            runtimes: vec![RuntimeStatus {
                name: "Ollama".into(),
                endpoint: "http://127.0.0.1:11434".into(),
                status: "online".into(),
                models: vec![ModelInfo {
                    id: "modello-di-prova:8b".into(),
                    size_bytes: Some(4 << 30),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        let mut h = History::default();
        for _ in 0..30 {
            h.push(&snapshot);
        }
        (inv, snapshot, llm, h)
    }

    fn draw(
        width: u16,
        height: u16,
        inv: &Inventory,
        snapshot: &Snapshot,
        llm: &LlmState,
        state: &mut UiState,
        history: &History,
    ) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| render(frame, inv, snapshot, llm, state, history))
            .expect("frame renders");
        let buffer = terminal.backend().buffer();
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn every_page_survives_resize_and_empty_collectors() {
        let (inv, snapshot, llm, history) = fixture();
        for (width, height) in [
            (0, 0),
            (1, 1),
            (20, 8),
            (40, 12),
            (64, 18),
            (100, 32),
            (160, 48),
        ] {
            for tab in 0..10 {
                let mut state = UiState {
                    active_tab: tab,
                    selected: usize::MAX,
                    offset: u16::MAX,
                    ..Default::default()
                };
                draw(width, height, &inv, &snapshot, &llm, &mut state, &history);
                draw(
                    width,
                    height,
                    &Inventory::default(),
                    &Snapshot::default(),
                    &LlmState::default(),
                    &mut state,
                    &History::default(),
                );
            }
        }
    }

    #[test]
    fn content_has_real_values_and_truthful_missing_data() {
        let (inv, snapshot, llm, history) = fixture();
        let mut state = UiState::default();
        let text = draw(120, 38, &inv, &snapshot, &llm, &mut state, &history);
        assert!(text.contains("RUST-CPU"));
        assert!(text.contains("42.5%"));
        assert!(text.contains("8.0 GiB / 32.0 GiB"));
        state.active_tab = 6;
        let text = draw(120, 38, &inv, &snapshot, &llm, &mut state, &history);
        assert!(text.contains("ollama.exe"));
        assert!(text.contains("42"));
        state.active_tab = 7;
        let text = draw(140, 38, &inv, &snapshot, &llm, &mut state, &history);
        assert!(text.contains("modello-di-prova:8b"));
        assert!(text.contains("Nessun benchmark eseguito"));
        state.active_tab = 8;
        let text = draw(120, 38, &inv, &snapshot, &llm, &mut state, &history);
        assert!(text.contains("Cache KV FP16"));
        assert!(text.contains("RAM e VRAM sono vincoli separati"));
        state.active_tab = 4;
        let text = draw(
            120,
            38,
            &Inventory::default(),
            &Snapshot::default(),
            &llm,
            &mut state,
            &history,
        );
        assert!(text.contains("Nessun dato disponibile"));
    }

    #[test]
    fn overlays_themes_and_long_unicode_names_render() {
        let (inv, mut snapshot, llm, history) = fixture();
        snapshot.processes[0].name = "🦀 日本語 e accenti àèìòù".repeat(40);
        for theme in [false, true] {
            for (width, height) in [(40, 12), (64, 18), (120, 40)] {
                let mut state = UiState {
                    theme_light: theme,
                    help: true,
                    ..Default::default()
                };
                draw(width, height, &inv, &snapshot, &llm, &mut state, &history);
                state.help = false;
                state.process_detail = true;
                draw(width, height, &inv, &snapshot, &llm, &mut state, &history);
                state.process_detail = false;
                state.benchmark_confirm = true;
                draw(width, height, &inv, &snapshot, &llm, &mut state, &history);
                state.benchmark_confirm = false;
                state.cpu_benchmark_confirm = true;
                draw(width, height, &inv, &snapshot, &llm, &mut state, &history);
            }
        }
    }

    #[test]
    fn sidebar_mouse_geometry_matches_tab_rows() {
        assert_eq!(sidebar_tab_at(120, 40, 10, 6), Some(0));
        assert_eq!(sidebar_tab_at(120, 40, 10, 24), Some(9));
        assert_eq!(sidebar_tab_at(120, 40, 10, 7), None);
        assert_eq!(sidebar_tab_at(80, 24, 10, 6), None);
        assert_eq!(sidebar_tab_at(120, 40, 50, 6), None);
        assert_eq!(sidebar_tab_at(100, 24, 10, 15), Some(9));
    }

    #[test]
    fn terminal_control_bytes_are_removed_and_end_keeps_last_details_visible() {
        let (mut inv, mut snapshot, mut llm, history) = fixture();
        inv.cpu = vec![Detail::new("Vendor\x1b[2J", "Acme\r\nCPU")];
        snapshot.processes[0].name = "unsafe\x1b[2J\x07.exe".into();
        llm.runtimes[0].models[0].id = "unsafe\x1b[2Jmodel".into();
        let mut state = UiState {
            active_tab: 1,
            offset: u16::MAX,
            ..Default::default()
        };
        let text = draw(120, 38, &inv, &snapshot, &llm, &mut state, &history);
        assert!(text.contains("Acme��CPU"));
        assert!(!text.contains('\x1b'));
        state.active_tab = 6;
        let text = draw(120, 38, &inv, &snapshot, &llm, &mut state, &history);
        assert!(!text.contains('\x1b'));
        assert!(!text.contains('\x07'));
        state.active_tab = 7;
        let text = draw(120, 38, &inv, &snapshot, &llm, &mut state, &history);
        assert!(!text.contains('\x1b'));
        state.active_tab = 9;
        state.offset = u16::MAX;
        let text = draw(120, 38, &inv, &snapshot, &llm, &mut state, &history);
        assert!(text.contains("registrazione"));
        state.offset = u16::MAX;
        let text = draw(64, 18, &inv, &snapshot, &llm, &mut state, &history);
        assert!(
            text.contains("campioni."),
            "End must reach the last wrapped line"
        );
    }

    #[test]
    fn end_then_up_moves_immediately_on_scrollable_pages() {
        let (mut inv, mut snapshot, llm, history) = fixture();
        let details = (0..40)
            .map(|index| Detail::new(format!("Voce {index:02}"), format!("Dato {index:02}")))
            .collect::<Vec<_>>();
        inv.cpu = details.clone();
        inv.board = details.clone();
        inv.memory = details.clone();
        inv.graphics = details;
        snapshot.gpus = (0..30)
            .map(|index| GpuRow {
                name: format!("GPU {index:02}"),
                ..Default::default()
            })
            .collect();
        snapshot.disks = (0..30)
            .map(|index| DiskRow {
                mount: format!("V{index:02}"),
                ..Default::default()
            })
            .collect();
        snapshot.networks = (0..30)
            .map(|index| NetworkRow {
                name: format!("NET {index:02}"),
                ..Default::default()
            })
            .collect();
        for tab in [1, 2, 3, 4, 5, 9] {
            let mut state = UiState {
                active_tab: tab,
                offset: u16::MAX,
                ..Default::default()
            };
            let end = draw(64, 18, &inv, &snapshot, &llm, &mut state, &history);
            assert!(
                state.offset < u16::MAX,
                "render must persist effective offset for tab {tab}"
            );
            assert!(state.offset > 0, "fixture must overflow tab {tab}");
            let end_offset = state.offset;
            state.offset -= 1;
            let up = draw(64, 18, &inv, &snapshot, &llm, &mut state, &history);
            assert_eq!(state.offset, end_offset - 1);
            assert_ne!(end, up, "one Up must move tab {tab}");
        }
    }

    #[test]
    fn compact_llm_shows_runtime_status_and_models() {
        let (inv, snapshot, llm, history) = fixture();
        let mut state = UiState {
            active_tab: 7,
            ..Default::default()
        };
        let text = draw(64, 18, &inv, &snapshot, &llm, &mut state, &history);
        assert!(text.contains("Ollama: online"));
        assert!(text.contains("modello"));
    }

    #[test]
    fn process_detail_keeps_opening_snapshot_when_live_rows_change() {
        let (inv, mut snapshot, llm, history) = fixture();
        let mut state = UiState {
            active_tab: 6,
            process_detail: true,
            process_detail_row: Some(snapshot.processes[0].clone()),
            ..Default::default()
        };
        snapshot.processes = vec![ProcessRow {
            pid: 999,
            name: "processo-diverso.exe".into(),
            ..Default::default()
        }];
        let text = draw(120, 38, &inv, &snapshot, &llm, &mut state, &history);
        assert!(text.contains("Istantanea all’apertura"));
        assert!(text.contains("ollama.exe"));
        assert!(text.contains("PID             42"));
    }

    #[test]
    fn capacity_does_not_call_missing_ram_insufficient() {
        let mut state = UiState {
            active_tab: 8,
            ..Default::default()
        };
        for (width, height) in [(64, 18), (120, 38)] {
            let text = draw(
                width,
                height,
                &Inventory::default(),
                &Snapshot::default(),
                &LlmState::default(),
                &mut state,
                &History::default(),
            );
            assert!(
                !text.contains("insuff"),
                "missing telemetry is unknown, not insufficient"
            );
            assert!(text.contains(if width == 64 {
                "RAM n/d"
            } else {
                "RAM libera: n/d"
            }));
        }
    }

    #[test]
    fn long_cpu_and_dimm_values_scroll_to_their_last_physical_line() {
        let (mut inv, snapshot, llm, history) = fixture();
        inv.cpu = vec![Detail::new(
            "Istruzioni",
            format!("{}AMX-LAST", "SSE AVX AVX2 FMA ".repeat(70)),
        )];
        inv.memory = vec![Detail::new(
            "DIMM / part number",
            format!("{}DIMM-LAST", "DDR5 SERIAL VENDOR BANK CHANNEL ".repeat(45)),
        )];
        for (width, height, tab, tail) in [
            (64, 18, 1, "AMX-LAST"),
            (80, 24, 1, "AMX-LAST"),
            (64, 18, 3, "DIMM-LAST"),
            (80, 24, 3, "DIMM-LAST"),
        ] {
            let mut state = UiState {
                active_tab: tab,
                offset: u16::MAX,
                ..Default::default()
            };
            let end = draw(width, height, &inv, &snapshot, &llm, &mut state, &history);
            assert!(
                end.contains(tail),
                "last value token must remain reachable at {width}x{height}"
            );
            assert!(state.offset > 0 && state.offset < u16::MAX);
            let end_offset = state.offset;
            state.offset -= 1;
            let up = draw(width, height, &inv, &snapshot, &llm, &mut state, &history);
            assert_eq!(state.offset, end_offset - 1);
            assert!(
                !up.contains(tail),
                "Up after End must move one physical line"
            );
        }
        let wrapped = detail_lines(
            Rect::new(0, 0, 34, 10),
            &[Detail::new(
                "日本語 long label repeated",
                "Unicode 🦀 日本語 and averylongunbrokenvalue AMX-LAST",
            )],
            Palette::new(false),
        );
        assert!(wrapped.iter().all(|line| line.width() <= 32));
        assert!(
            wrapped
                .iter()
                .any(|line| line.to_string().contains("AMX-LAST"))
        );
    }

    #[test]
    fn diagnostic_wrapping_counts_terminal_cells_and_preserves_text() {
        let lines = wrap_diagnostic_lines(
            vec![
                Line::from("日本語 unità 🦀 lunghissimap arola"),
                Line::from(""),
            ],
            8,
        );
        assert!(lines.iter().all(|line| line.width() <= 8));
        let text = lines
            .iter()
            .flat_map(|line| line.spans.iter().map(|span| span.content.to_string()))
            .collect::<Vec<_>>()
            .join("");
        assert!(text.contains("日本語"));
        assert!(text.contains("🦀"));
        assert!(text.contains("lunghissimap"));
    }
}
