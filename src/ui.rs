//! Responsive, keyboard-first terminal workspace.
//!
//! Navigation contract: Tab/Shift-Tab or arrows switch pages, 1..9/0 select a
//! page, Up/Down/PgUp/PgDn/Home/End scroll. `/` searches processes; `s` cycles
//! sort and `a` reverses it. Space freezes the displayed sample, `t` changes
//! theme, `e` exports, `r` records. `+`/`-` change sampling except in Capacity
//! Lab, where they edit the field selected with `[`/`]`. `b` requests an
//! inference benchmark, Enter confirms, Esc returns, `?` opens help, `q` exits.

use crate::{
    model::*,
    settings::SETTINGS_FIELDS,
    storage::{ComparisonMetric, SavedBenchmark, SessionComparison, SessionSummary},
};
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
    let step = if height >= 34 { 2 } else { 1 };
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
        compact_navigation(frame, rows[0], state, p);
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
        9 => diagnostics_page(frame, main, inventory, snapshot, llm, state, p),
        10 => sessions_page(frame, main, state, p),
        11 => comparison_page(frame, main, state, p),
        12 => settings_page(frame, main, state, p),
        _ => {}
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
        cpu_benchmark_modal(frame, area, state, p);
    }
    if state.workspace.name_edit.is_some() {
        session_name_modal(frame, area, state, p);
    }
    if state.workspace.delete_confirm {
        session_delete_modal(frame, area, state, p);
    }
}

fn tab_shortcut(index: usize) -> String {
    match index {
        0..=8 => (index + 1).to_string(),
        9 => "0".into(),
        10 => "F2".into(),
        11 => "F3".into(),
        12 => "F4".into(),
        _ => String::new(),
    }
}

fn compact_navigation(frame: &mut Frame<'_>, area: Rect, state: &UiState, p: Palette) {
    let workspace = state.active_tab >= 10;
    let start = if workspace {
        10
    } else if area.width < 64 {
        state.active_tab.saturating_sub(2).min(5)
    } else {
        0
    };
    let end = if workspace {
        TABS.len()
    } else if area.width < 64 {
        (start + 5).min(10)
    } else {
        10
    };
    let titles = (start..end)
        .map(|index| {
            let label = match index {
                0 => "Vista",
                2 => "MB",
                3 => "RAM",
                5 => "I/O",
                6 => "Proc",
                7 => "LLM",
                8 => "Lab",
                9 => "Diag",
                _ => TABS[index],
            };
            Line::from(format!("{} {label}", tab_shortcut(index)))
        })
        .collect::<Vec<_>>();
    let rows = split(
        area,
        Direction::Vertical,
        &[Constraint::Length(1), Constraint::Min(1)],
    );
    frame.render_widget(
        Tabs::new(titles)
            .select(
                state
                    .active_tab
                    .saturating_sub(start)
                    .min(end.saturating_sub(start + 1)),
            )
            .divider(" ")
            .padding("", "")
            .style(p.muted())
            .highlight_style(p.accent().bg(p.elevated)),
        rows[0],
    );
    frame.render_widget(
        Paragraph::new(if workspace {
            "←→ / Tab: 13 viste · 1…9,0 hardware · F2/F3/F4 archivio"
        } else {
            "←→ / Tab: 13 viste · F2 Sessioni · F3 Confronto · F4 Opzioni"
        })
        .style(p.muted())
        .block(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_style(Style::default().fg(p.border)),
        ),
        rows[1],
    );
}

fn header(frame: &mut Frame<'_>, area: Rect, snapshot: &Snapshot, state: &UiState, p: Palette) {
    let workspace_page = state.active_tab >= 10;
    let mut line = vec![
        Span::styled(" RUST", p.accent()),
        Span::styled("-CPU ", p.text().add_modifier(Modifier::BOLD)),
        Span::styled(" / HARDWARE + LOCAL AI", p.muted()),
    ];
    if area.width >= 88 {
        line.push(Span::styled("       ", p.muted()));
    }
    line.push(Span::styled(
        if state.active_tab == 10 {
            "  ARCHIVIO "
        } else if state.active_tab == 11 {
            "  CONFRONTO "
        } else if state.active_tab == 12 {
            "  CONFIG "
        } else if state.workspace.loaded.is_some() {
            "  REPLAY "
        } else if state.paused {
            "  PAUSA "
        } else {
            "  LIVE "
        },
        Style::default()
            .fg(if workspace_page || state.workspace.loaded.is_some() {
                p.cyan
            } else if state.paused {
                p.amber
            } else {
                p.green
            })
            .add_modifier(Modifier::BOLD),
    ));
    if state.recording {
        line.push(Span::styled(" ● REC", Style::default().fg(p.red)));
    }
    if !state.workspace.alerts.is_empty() {
        line.push(Span::styled(
            format!(" !{} eventi", state.workspace.alerts.len()),
            Style::default().fg(p.amber).add_modifier(Modifier::BOLD),
        ));
    }
    if !workspace_page {
        line.push(Span::styled(
            format!("  {:.1}s", state.tick_ms as f64 / 1000.0),
            p.muted(),
        ));
    }
    if area.width >= 100 && !workspace_page {
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
        Paragraph::new(vec![Line::from(line),Line::from(Span::styled(if let Some(session)=&state.workspace.loaded{
            format!(" REPLAY #{} {} · campione {}/{} caricati ({} totali) · [ ] scorri · l live",session.summary.id,ellipsize(&clean(&session.summary.name),24),state.workspace.replay_index.saturating_add(1).min(session.samples.len()),session.samples.len(),session.total_samples)
        }else if let Some(id)=state.workspace.active_session{format!(" SQLite #{id} · sessione attiva · F2 archivio · F3 confronto · F4 preferenze")}
        else{" SQLite · F2 sessioni · F3 confronto · F4 preferenze · F6 backup database".into()},p.muted()))]).block(
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
            Span::styled(format!("{} ", tab_shortcut(index)), p.muted()),
            Span::styled(
                (*title).to_string(),
                if active {
                    p.accent().bg(p.elevated)
                } else {
                    p.text()
                },
            ),
        ]));
        if area.height >= 29 {
            lines.push(Line::default());
        }
    }
    frame.render_widget(Paragraph::new(lines), inner);
    if area.height >= 36 {
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
    } else if state.active_tab == 10 {
        "↑↓ sessione  Invio replay  a A  b B  c confronta  n nome  Del elimina"
    } else if state.active_tab == 11 {
        "↑↓ scorri  F2 scegli A/B  l live  F6 backup  ? guida  q esci"
    } else if state.active_tab == 12 {
        "↑↓ campo  + − modifica  salvataggio automatico  F6 backup  ? guida"
    } else if state.workspace.loaded.is_some() {
        "[ ] campione  l torna live  F2 archivio  Tab pagina  ? guida  q esci"
    } else if state.active_tab == 8 {
        "[ ] campo  + − modifica  Tab pagina  ? guida  q esci"
    } else if state.active_tab == 1 {
        "↑↓ scorri  x CPU  Shift+X ripeti  c annulla  Tab pagina  ? guida"
    } else if state.active_tab == 6 {
        "↑↓ seleziona  / cerca  s ordina  a verso  Invio dettagli  ? guida"
    } else if state.active_tab == 7 {
        "↑↓ modello  b misura  Shift+B ripeti  c annulla  F5 aggiorna  ? guida"
    } else {
        "Tab pagina  ↑↓ scorri  Spazio pausa  e esporta  r registra  ? guida  q esci"
    };
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(context, p.muted())),
            if let Some(alert) = state.workspace.alerts.last() {
                Line::from(vec![
                    Span::styled(
                        ellipsize(
                            &available(&state.status),
                            (area.width / 2).saturating_sub(2) as usize,
                        ),
                        p.text(),
                    ),
                    Span::styled(
                        format!(
                            " | ! {}",
                            ellipsize(
                                &clean(&alert.message),
                                (area.width / 2).saturating_sub(4) as usize
                            )
                        ),
                        Style::default().fg(p.amber),
                    ),
                ])
            } else {
                Line::from(Span::styled(available(&state.status), p.text()))
            },
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
                "{} core / {} thread · {} processi{}",
                inventory.physical_cores,
                inventory.logical_cores,
                snapshot.processes.len(),
                if state.workspace.loaded.is_some() {
                    " LLM archiviati"
                } else {
                    ""
                }
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
            "{} core · {} thread · {} processi{}",
            inventory.physical_cores,
            inventory.logical_cores,
            snapshot.processes.len(),
            if state.workspace.loaded.is_some() {
                " LLM archiviati"
            } else {
                ""
            }
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
    let mut cpu_details = inventory.cpu.clone();
    cpu_details.extend(snapshot.sensor_details.iter().cloned());
    cpu_details.extend(
        snapshot
            .sensors
            .iter()
            .filter(|sensor| {
                let name = sensor.name.to_lowercase();
                name.contains("cpu") || name.contains("package") || name.contains("tctl")
            })
            .map(|sensor| {
                Detail::new(
                    format!("Temperatura · {}", sensor.name),
                    format!("{:.1} °C (lettura live)", sensor.temperature),
                )
            }),
    );
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
            detail_max_offset(cols[0], &cpu_details)
                .max(core_max_offset(right[0], snapshot.per_core.len())),
        );
        detail_table(
            frame,
            cols[0],
            "Caratteristiche CPUID / sistema",
            &cpu_details,
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
        clamp_scroll(state, detail_max_offset(rows[2], &cpu_details));
        detail_table(
            frame,
            rows[2],
            "Caratteristiche CPU · ↑↓ scorri",
            &cpu_details,
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
        let panels = if rows[1].height >= 14 {
            split(
                rows[1],
                Direction::Vertical,
                &[Constraint::Length(8), Constraint::Min(4)],
            )
        } else {
            vec![rows[1]]
        };
        let gpu_max = snapshot
            .gpus
            .len()
            .saturating_sub(panels[0].height.saturating_sub(4).max(2) as usize / 2);
        let detail_max = panels
            .get(1)
            .map(|area| detail_max_offset(*area, &inventory.graphics))
            .unwrap_or(0);
        clamp_scroll(state, gpu_max.max(detail_max));
        let offset = (state.offset as usize).min(gpu_max);
        let gpu_rows = snapshot.gpus.iter().skip(offset).map(|g| {
            let vram = match (g.memory_used, g.memory_total) {
                (Some(u), Some(t)) => format!("{} / {}", bytes(u), bytes(t)),
                (Some(u), None) => format!("{} / n/d", bytes(u)),
                (_, Some(t)) => format!("totale {}", bytes(t)),
                _ => "n/d".into(),
            };
            Row::new(vec![
                clean(&g.name),
                optional_percent(g.utilization),
                vram,
                g.shared_memory_used
                    .map(bytes)
                    .unwrap_or_else(|| "n/d".into()),
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
                    Constraint::Length(if area.width >= 85 { 12 } else { 0 }),
                    Constraint::Length(8),
                    Constraint::Length(8),
                ],
            )
            .header(
                Row::new(["SCHEDA", "CARICO", "DEDICATA", "CONDIVISA", "TEMP", "POWER"])
                    .style(p.muted())
                    .bottom_margin(1),
            )
            .column_spacing(1)
            .block(panel("Adattatori e telemetria", p)),
            panels[0],
        );
        if let Some(details) = panels.get(1) {
            detail_table(
                frame,
                *details,
                "Driver e identità adattatori",
                &inventory.graphics,
                state.offset,
                p,
            );
        }
    }
    frame.render_widget(Paragraph::new("n/d = dato non esposto. WDDM/DXGI: carico e memoria Windows quando disponibili; NVIDIA può aggiungere temperatura e potenza. La RAM condivisa è distinta dalla memoria dedicata. Le attribuzioni ai PID non vanno sommate come memoria fisica.").style(p.muted()).wrap(Wrap{trim:true}),rows[2]);
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
        if state.workspace.loaded.is_some() {
            "Solo processi LLM archiviati; non è l'elenco completo del sistema operativo."
        } else {
            "CPU normalizzata sulla macchina · LLM = riconoscimento euristico del nome."
        },
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
            Cell::from(
                proc.gpu_memory_bytes
                    .map(bytes)
                    .unwrap_or_else(|| "n/d".into()),
            ),
            Cell::from(optional_percent(proc.gpu_percent)),
            Cell::from(
                proc.gpu_shared_memory_bytes
                    .map(bytes)
                    .unwrap_or_else(|| "n/d".into()),
            ),
        ])
    });
    let widths = vec![
        Constraint::Length(6),
        Constraint::Min(14),
        Constraint::Length(7),
        Constraint::Length(11),
        Constraint::Length(if area.width >= 130 { 11 } else { 0 }),
        Constraint::Length(if area.width >= 130 { 11 } else { 0 }),
        Constraint::Length(if area.width >= 88 { 10 } else { 0 }),
        Constraint::Length(if area.width >= 88 { 7 } else { 0 }),
        Constraint::Length(if area.width >= 105 { 10 } else { 0 }),
    ];
    let table = Table::new(body, widths)
        .column_spacing(1)
        .header(
            Row::new([
                "PID",
                "PROCESSO",
                "CPU",
                "RAM",
                "LETTURA",
                "SCRITTURA",
                "GPU DED",
                "GPU %",
                "GPU SH",
            ])
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
    let history_present =
        !state.workspace.benchmark_history.is_empty() || !llm.observations.is_empty();
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
            Constraint::Length(if history_present {
                if tiny {
                    2
                } else if area.height >= 28 {
                    12
                } else {
                    8
                }
            } else if tall {
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
        if state.workspace.loaded.is_some() {
            "Replay: dati API live non archiviati; benchmark salvati disponibili. l torna live."
        } else {
            "Endpoint localhost · elenco rilevato, nessun download automatico."
        },
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
        let summary = if state.workspace.loaded.is_some() {
            "Replay · API live non archiviate · l torna live".into()
        } else if llm.runtimes.is_empty() {
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
                if state.workspace.loaded.is_some() {
                    "Replay · API dei runtime non archiviate"
                } else if llm.runtimes.is_empty() {
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
            if state.workspace.loaded.is_some() {
                "Modelli API non archiviati · risultati storici sotto".into()
            } else {
                format!(
                    "{} modelli · ↑↓ seleziona · b misura inferenza",
                    models.len()
                )
            },
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
    let result_width = rows[3].width.saturating_sub(if tiny { 0 } else { 2 }) as usize;
    let mut results = wrap_diagnostic_lines(benchmark_lines, result_width);
    append_llm_history(&mut results, llm, state, result_width, p);
    let visible = rows[3].height.saturating_sub(if tiny { 0 } else { 2 }) as usize;
    clamp_scroll(state, results.len().saturating_sub(visible));
    if tiny {
        frame.render_widget(
            Paragraph::new(results)
                .scroll((state.offset, 0))
                .style(p.muted()),
            rows[3],
        );
    } else {
        frame.render_widget(
            Paragraph::new(results)
                .scroll((state.offset, 0))
                .block(panel("Risultati e cronologia · PgUp/PgDn scorri", p)),
            rows[3],
        );
    }
}

fn append_llm_history(
    lines: &mut Vec<Line<'static>>,
    llm: &LlmState,
    state: &UiState,
    width: usize,
    p: Palette,
) {
    if !llm.observations.is_empty() {
        lines.push(Line::default());
        push_prose(
            lines,
            "OSSERVAZIONI DEL SERVER · distinte dai benchmark volontari",
            p.accent(),
            width,
        );
        for value in llm.observations.iter().rev().take(3) {
            push_prose(
                lines,
                format!(
                    "{} / {} · {} · TTFT {}",
                    clean(&value.runtime),
                    clean(&value.model),
                    metric_value(value.generation_tokens_per_sec, "token/s", false),
                    metric_value(value.time_to_first_token_ms, "ms", false)
                ),
                p.text(),
                width,
            );
            push_prose(lines, clean(&value.note), p.muted(), width);
        }
    }
    if !state.workspace.benchmark_history.is_empty() {
        lines.push(Line::default());
        let mut history = state.workspace.benchmark_history.iter().collect::<Vec<_>>();
        history.sort_by_key(|entry| std::cmp::Reverse(entry.id));
        push_prose(
            lines,
            format!(
                "CRONOLOGIA SQLITE · ultime {} prove / osservazioni",
                history.len().min(100)
            ),
            p.accent(),
            width,
        );
        for entry in history.into_iter().take(100) {
            push_prose(
                lines,
                format!(
                    "#{} · {}",
                    entry.id,
                    entry
                        .session_id
                        .map(|id| format!("sessione #{id}"))
                        .unwrap_or_else(|| "senza sessione".into())
                ),
                p.accent(),
                width,
            );
            lines.extend(benchmark_lines(Some(entry), width, p));
            lines.push(Line::default());
        }
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
    lines.push(Line::default());
    lines.push(Line::from(Span::styled(
        "SENSORI DINAMICI · SOLO VALORI ESPOSTI",
        p.accent(),
    )));
    for detail in &snapshot.sensor_details {
        lines.push(Line::from(format!(
            "{}: {}",
            clean(&detail.label),
            available(&detail.value)
        )));
    }
    for sensor in &snapshot.sensors {
        lines.push(Line::from(format!(
            "{}: {:.1} °C",
            clean(&sensor.name),
            sensor.temperature
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

fn ellipsize(text: &str, width: usize) -> String {
    if Line::raw(text).width() <= width {
        return text.to_string();
    }
    if width == 0 {
        return String::new();
    }
    let line = Line::raw(text);
    let mut result = String::new();
    let mut used = 0;
    for grapheme in line.styled_graphemes(Style::default()) {
        let cells = Span::raw(grapheme.symbol).width();
        if used + cells > width.saturating_sub(1) {
            break;
        }
        result.push_str(grapheme.symbol);
        used += cells;
    }
    result.push('…');
    result
}

fn padded_cell(text: &str, width: usize) -> String {
    let text = ellipsize(text, width);
    let pad = width.saturating_sub(Line::raw(&text).width());
    format!("{text}{}", " ".repeat(pad))
}

fn duration_label(seconds: f64) -> String {
    if !seconds.is_finite() {
        return "n/d".into();
    }
    let seconds = seconds.max(0.0) as u64;
    if seconds >= 3600 {
        format!("{}h {:02}m", seconds / 3600, (seconds % 3600) / 60)
    } else if seconds >= 60 {
        format!("{}m {:02}s", seconds / 60, seconds % 60)
    } else {
        format!("{seconds}s")
    }
}

fn optional_number(value: Option<f64>, suffix: &str) -> String {
    value
        .filter(|v| v.is_finite())
        .map(|v| format!("{v:.1}{suffix}"))
        .unwrap_or_else(|| "n/d".into())
}
fn optional_bytes(value: Option<f64>) -> String {
    value
        .filter(|v| v.is_finite() && *v >= 0.0)
        .map(|v| bytes(v as u64))
        .unwrap_or_else(|| "n/d".into())
}
fn time_label(timestamp: u64) -> String {
    chrono::DateTime::from_timestamp(timestamp.min(i64::MAX as u64) as i64, 0)
        .map(|v| v.format("%H:%M:%S UTC").to_string())
        .unwrap_or_else(|| "n/d".into())
}
fn short_date(value: &str) -> String {
    ellipsize(&clean(value).replace('T', " ").replace('Z', ""), 16)
}

fn sessions_page(frame: &mut Frame<'_>, area: Rect, state: &mut UiState, p: Palette) {
    let compact = area.height < 19;
    let rows = split(
        area,
        Direction::Vertical,
        &[
            Constraint::Length(if compact { 1 } else { 2 }),
            Constraint::Length(if compact { 2 } else { 3 }),
            Constraint::Min(4),
            Constraint::Length(if compact { 2 } else { 6 }),
        ],
    );
    page_title(
        frame,
        rows[0],
        "Sessioni · il tuo archivio locale",
        "SQLite persistente · scegli due sessioni come riferimento A e candidata B.",
        p,
    );
    let workspace = &state.workspace;
    let status = format!(
        "{} sessioni · A {} · B {} · {}",
        workspace.sessions.len(),
        workspace
            .baseline_id
            .map(|v| format!("#{v}"))
            .unwrap_or_else(|| "—".into()),
        workspace
            .candidate_id
            .map(|v| format!("#{v}"))
            .unwrap_or_else(|| "—".into()),
        if workspace.loading {
            "caricamento…"
        } else if state.recording {
            "registrazione attiva"
        } else {
            "archivio pronto"
        }
    );
    let db = if let Some(error) = &workspace.last_error {
        format!("! {}", clean(error))
    } else {
        format!("DB {}", available(&workspace.db_path))
    };
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                status,
                if workspace.loading {
                    Style::default().fg(p.amber)
                } else {
                    p.accent()
                },
            )),
            Line::from(Span::styled(
                db,
                if workspace.last_error.is_some() {
                    Style::default().fg(p.red)
                } else {
                    p.muted()
                },
            )),
        ]),
        rows[1],
    );
    state.selected = state
        .selected
        .min(state.workspace.sessions.len().saturating_sub(1));
    let table_rows = state.workspace.sessions.iter().map(|session| {
        let markers = format!(
            "{}{}{}",
            if Some(session.id) == state.workspace.baseline_id {
                "A"
            } else {
                " "
            },
            if Some(session.id) == state.workspace.candidate_id {
                "B"
            } else {
                " "
            },
            if Some(session.id) == state.workspace.active_session {
                "●"
            } else {
                " "
            }
        );
        Row::new(vec![
            Cell::from(markers).style(p.accent()),
            Cell::from(format!("#{} {}", session.id, clean(&session.name))),
            Cell::from(short_date(&session.started_utc)).style(p.muted()),
            Cell::from(session.samples.to_string()),
            Cell::from(duration_label(session.duration_secs)),
            Cell::from(optional_number(session.cpu.average, "%")),
            Cell::from(optional_number(session.cpu.peak, "%")),
            Cell::from(optional_bytes(session.ram.peak)),
            Cell::from(optional_bytes(session.vram.peak)),
        ])
    });
    let widths = if area.width >= 95 {
        vec![
            Constraint::Length(3),
            Constraint::Min(16),
            Constraint::Length(16),
            Constraint::Length(7),
            Constraint::Length(8),
            Constraint::Length(7),
            Constraint::Length(7),
            Constraint::Length(11),
            Constraint::Length(11),
        ]
    } else {
        vec![
            Constraint::Length(3),
            Constraint::Min(16),
            Constraint::Length(0),
            Constraint::Length(6),
            Constraint::Length(6),
            Constraint::Length(7),
            Constraint::Length(0),
            Constraint::Length(9),
            Constraint::Length(0),
        ]
    };
    let title = if state.workspace.sessions.is_empty() {
        "Nessuna sessione · r avvia una registrazione"
    } else {
        "Sessioni salvate · Invio apre il replay"
    };
    let mut table_state = TableState::default()
        .with_selected((!state.workspace.sessions.is_empty()).then_some(state.selected));
    frame.render_stateful_widget(
        Table::new(table_rows, widths)
            .column_spacing(1)
            .header(
                Row::new([
                    "A/B",
                    "SESSIONE",
                    "INIZIO UTC",
                    "CAMP.",
                    "DURATA",
                    "CPU μ",
                    "CPU MAX",
                    "RAM MAX",
                    "VRAM MAX",
                ])
                .style(p.muted()),
            )
            .highlight_symbol("▌ ")
            .row_highlight_style(p.accent().bg(p.elevated))
            .block(panel(title, p)),
        rows[2],
        &mut table_state,
    );
    if let Some(session) = state.workspace.sessions.get(state.selected) {
        let lines = session_summary_lines(session, p);
        if compact {
            frame.render_widget(
                Paragraph::new(vec![
                    Line::from(Span::styled(
                        format!("#{} {}", session.id, clean(&session.name)),
                        p.accent(),
                    )),
                    Line::from(Span::styled(
                        format!(
                            "CPU max {} · RAM {} · VRAM {}",
                            optional_number(session.cpu.peak, "%"),
                            optional_bytes(session.ram.peak),
                            optional_bytes(session.vram.peak)
                        ),
                        p.muted(),
                    )),
                ]),
                rows[3],
            );
        } else {
            frame.render_widget(
                Paragraph::new(lines)
                    .wrap(Wrap { trim: true })
                    .block(panel("Sessione selezionata · n rinomina · Canc elimina", p)),
                rows[3],
            );
        }
    } else {
        frame.render_widget(Paragraph::new("Premi r durante un lavoro per conservare risorse e benchmark. F6 crea un backup del database. I report CSV/JSON restano esportabili.").style(p.muted()).wrap(Wrap{trim:true}),rows[3]);
    }
}

fn session_summary_lines(session: &SessionSummary, p: Palette) -> Vec<Line<'static>> {
    vec![
        Line::from(Span::styled(
            format!("#{} {}", session.id, clean(&session.name)),
            p.accent(),
        )),
        Line::from(format!(
            "UTC {} → {} · {} · {} campioni · {} benchmark",
            clean(&session.started_utc),
            session
                .ended_utc
                .as_deref()
                .map(clean)
                .unwrap_or_else(|| "in corso".into()),
            duration_label(session.duration_secs),
            session.samples,
            session.benchmark_count
        )),
        Line::from(format!(
            "CPU media {} / picco {} · RAM media {} / picco {}",
            optional_number(session.cpu.average, "%"),
            optional_number(session.cpu.peak, "%"),
            optional_bytes(session.ram.average),
            optional_bytes(session.ram.peak)
        )),
        Line::from(format!(
            "VRAM media {} / picco {} · RSS LLM media {} · stato {}",
            optional_bytes(session.vram.average),
            optional_bytes(session.vram.peak),
            optional_bytes(session.llm_ram.average),
            available(&session.state)
        )),
    ]
}

fn metric_value(value: Option<f64>, unit: &str, signed: bool) -> String {
    let Some(value) = value.filter(|v| v.is_finite()) else {
        return "n/d".into();
    };
    let sign = if signed {
        if value < 0.0 { "−" } else { "+" }
    } else if value < 0.0 {
        "−"
    } else {
        ""
    };
    let abs = value.abs();
    let formatted = match unit {
        "B" | "bytes" => bytes(abs as u64),
        "B/s" => rate(abs),
        "%" => format!("{abs:.1}%"),
        "s" => format!("{abs:.2}s"),
        "ms" => format!("{abs:.0}ms"),
        "mix/s" | "ops/s" => format!("{:.2} M {unit}", abs / 1e6),
        "token/s" => format!("{abs:.2} tok/s"),
        _ => format!("{abs:.2} {}", clean(unit)),
    };
    format!("{sign}{formatted}")
}

fn push_prose(lines: &mut Vec<Line<'static>>, text: impl Into<String>, style: Style, width: usize) {
    lines.extend(wrap_diagnostic_lines(
        vec![Line::styled(text.into(), style)],
        width,
    ));
}

fn comparison_page(frame: &mut Frame<'_>, area: Rect, state: &mut UiState, p: Palette) {
    let rows = split(
        area,
        Direction::Vertical,
        &[
            Constraint::Length(if area.height < 16 { 1 } else { 2 }),
            Constraint::Min(1),
        ],
    );
    page_title(
        frame,
        rows[0],
        "Confronto · A riferimento → B candidata",
        "Statistiche su tutti i campioni salvati. Δ = B − A; un delta non dimostra una causa.",
        p,
    );
    let Some(comparison) = &state.workspace.comparison else {
        let lines = vec![
            Line::from(Span::styled(
                "Scegli le sessioni da confrontare",
                p.accent(),
            )),
            Line::default(),
            Line::from(
                "F2 apre l'archivio. ↑↓ seleziona una sessione; a la imposta come A, b come B. Premi c per caricare il confronto.",
            ),
            Line::default(),
            Line::from(
                "Grafici: stessa progressione da 0 a 100% della sessione, con asse verticale in percentuale. Durate e configurazioni diverse richiedono cautela.",
            ),
            Line::from(if state.workspace.loading {
                "Caricamento in corso…"
            } else {
                "Nessun confronto caricato."
            }),
        ];
        frame.render_widget(
            Paragraph::new(lines)
                .wrap(Wrap { trim: true })
                .block(panel("Un confronto ripetibile", p)),
            rows[1],
        );
        state.offset = 0;
        return;
    };
    let width = rows[1].width.saturating_sub(2) as usize;
    let mut lines = comparison_lines(comparison, width, p);
    if let Some(error) = &state.workspace.last_error {
        push_prose(
            &mut lines,
            format!("! {}", clean(error)),
            Style::default().fg(p.red),
            width,
        );
    }
    let maximum = lines
        .len()
        .saturating_sub(rows[1].height.saturating_sub(2) as usize);
    state.offset = state.offset.min(maximum.min(u16::MAX as usize) as u16);
    frame.render_widget(
        Paragraph::new(lines)
            .scroll((state.offset, 0))
            .block(panel("Dati, progressione e prove · ↑↓ / PgUp PgDn", p)),
        rows[1],
    );
}

fn comparison_lines(
    comparison: &SessionComparison,
    width: usize,
    p: Palette,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    push_prose(
        &mut lines,
        format!(
            "A  #{} {}",
            comparison.left.id,
            clean(&comparison.left.name)
        ),
        p.accent(),
        width,
    );
    push_prose(
        &mut lines,
        format!(
            "B  #{} {}",
            comparison.right.id,
            clean(&comparison.right.name)
        ),
        Style::default().fg(p.amber).add_modifier(Modifier::BOLD),
        width,
    );
    push_prose(
        &mut lines,
        format!(
            "A: {} · {} campioni  |  B: {} · {} campioni",
            duration_label(comparison.left.duration_secs),
            comparison.left.samples,
            duration_label(comparison.right.duration_secs),
            comparison.right.samples
        ),
        p.muted(),
        width,
    );
    lines.push(Line::default());
    if width >= 86 {
        lines.push(Line::from(Span::styled(
            format!(
                "{}{}{}{}{}",
                padded_cell("METRICA", 26),
                padded_cell("A", 15),
                padded_cell("B", 15),
                padded_cell("Δ B − A", 17),
                "Δ %"
            ),
            p.muted(),
        )));
    }
    append_comparison_metrics(&mut lines, &comparison.metrics, width, p);
    push_prose(
        &mut lines,
        "n/d = dato assente; Δ% non è definito con A = 0. Le percentuali sono variazioni relative, Δ dei carichi è espresso in punti percentuali.",
        p.muted(),
        width,
    );
    lines.push(Line::default());
    push_prose(
        &mut lines,
        "BENCHMARK · GRUPPI CON METODO COMPATIBILE",
        p.accent(),
        width,
    );
    if comparison.benchmarks.is_empty() {
        push_prose(
            &mut lines,
            "Nessun gruppo statistico disponibile; le singole prove restano elencate sotto.",
            p.muted(),
            width,
        );
    }
    for group in &comparison.benchmarks {
        push_prose(&mut lines, clean(&group.label), p.accent(), width);
        push_prose(
            &mut lines,
            format!(
                "{} · metodo {} · A {} prove / B {} prove",
                clean(&group.kind),
                available(&group.method),
                group.runs_left,
                group.runs_right
            ),
            p.muted(),
            width,
        );
        append_comparison_metrics(&mut lines, &group.metrics, width, p);
        for warning in &group.warnings {
            push_prose(
                &mut lines,
                format!("! {}", clean(warning)),
                Style::default().fg(p.amber),
                width,
            );
        }
        lines.push(Line::default());
    }
    comparison_progress_and_benchmarks(&mut lines, comparison, width, p);
    lines
}

fn append_comparison_metrics(
    lines: &mut Vec<Line<'static>>,
    metrics: &[ComparisonMetric],
    width: usize,
    p: Palette,
) {
    for metric in metrics {
        let left = metric_value(metric.left, &metric.unit, false);
        let right = metric_value(metric.right, &metric.unit, false);
        let delta = metric_value(metric.delta, &metric.unit, true);
        let delta_percent = metric_value(metric.delta_percent, "%", true);
        let favorable = metric.delta.filter(|v| v.is_finite()).map(|v| {
            if metric.lower_is_better {
                v < 0.0
            } else {
                v > 0.0
            }
        });
        let color = if metric.delta == Some(0.0) {
            p.muted
        } else {
            match favorable {
                Some(true) => p.green,
                Some(false) => p.amber,
                None => p.muted,
            }
        };
        if width >= 86 {
            lines.push(Line::from(vec![
                Span::styled(padded_cell(&clean(&metric.label), 26), p.text()),
                Span::raw(padded_cell(&left, 15)),
                Span::raw(padded_cell(&right, 15)),
                Span::styled(padded_cell(&delta, 17), Style::default().fg(color)),
                Span::styled(delta_percent, Style::default().fg(color)),
            ]));
        } else {
            push_prose(
                lines,
                clean(&metric.label),
                p.text().add_modifier(Modifier::BOLD),
                width,
            );
            push_prose(
                lines,
                format!("A {left} | B {right} | Δ {delta} ({delta_percent})"),
                Style::default().fg(color),
                width,
            );
        }
    }
}

fn comparison_progress_and_benchmarks(
    lines: &mut Vec<Line<'static>>,
    comparison: &SessionComparison,
    width: usize,
    p: Palette,
) {
    lines.push(Line::default());
    push_prose(
        lines,
        "PROGRESSIONE NORMALIZZATA · asse Y 0–100%",
        p.accent(),
        width,
    );
    push_prose(
        lines,
        "Asse X: avanzamento 0→100% del tempo di ciascuna sessione; le durate possono differire. · indica telemetria assente. L'anteprima può essere sottocampionata.",
        p.muted(),
        width,
    );
    let points = (width.saturating_sub(9) / 2).clamp(4, 100);
    for (label, kind, color) in [
        ("CPU", 0, p.cyan),
        ("RAM", 1, p.green),
        ("GPU0", 2, p.amber),
    ] {
        lines.push(Line::from(Span::styled(
            label,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        )));
        let a = progress_series(&comparison.left_series, points, kind);
        let b = progress_series(&comparison.right_series, points, kind);
        lines.push(Line::from(vec![
            Span::styled("A ", p.muted()),
            Span::styled(spark_text(&a), Style::default().fg(color)),
            Span::styled("  B ", p.muted()),
            Span::styled(spark_text(&b), Style::default().fg(color)),
        ]));
    }
    lines.push(Line::default());
    push_prose(lines, "CONDIZIONI DEL CONFRONTO", p.accent(), width);
    if comparison.warnings.is_empty() {
        push_prose(
            lines,
            "Nessun avviso automatico. Verifica comunque stesso modello, quantizzazione, contesto, offload, cache e carico di fondo.",
            p.muted(),
            width,
        );
    }
    for warning in &comparison.warnings {
        push_prose(
            lines,
            format!("! {}", clean(warning)),
            Style::default().fg(p.amber),
            width,
        );
    }
    lines.push(Line::default());
    push_prose(
        lines,
        "BENCHMARK SALVATI · ogni prova rimane distinta",
        p.accent(),
        width,
    );
    push_prose(
        lines,
        "A e B sono affiancati nell'ordine salvato. Un modello/runtime uguale non prova una configurazione identica. Decode nativo e token/tempo totale non sono intercambiabili; osservazioni di server separate dai test volontari.",
        p.muted(),
        width,
    );
    let count = comparison
        .left_benchmarks
        .len()
        .max(comparison.right_benchmarks.len());
    if count == 0 {
        push_prose(
            lines,
            "Nessun benchmark presente nelle due sessioni.",
            p.muted(),
            width,
        );
    }
    for index in 0..count {
        let left = comparison.left_benchmarks.get(index);
        let right = comparison.right_benchmarks.get(index);
        push_prose(lines, format!("PROVA {}", index + 1), p.accent(), width);
        if width >= 86 {
            let col = (width.saturating_sub(3)) / 2;
            let a = benchmark_lines(left, col, p);
            let b = benchmark_lines(right, col, p);
            for row in 0..a.len().max(b.len()) {
                let a = a.get(row).map(ToString::to_string).unwrap_or_default();
                let b = b.get(row).map(ToString::to_string).unwrap_or_default();
                lines.push(Line::from(vec![
                    Span::raw(padded_cell(&a, col)),
                    Span::styled(" │ ", p.muted()),
                    Span::raw(b),
                ]));
            }
        } else {
            push_prose(lines, "A", p.accent(), width);
            lines.extend(benchmark_lines(left, width, p));
            push_prose(lines, "B", Style::default().fg(p.amber), width);
            lines.extend(benchmark_lines(right, width, p));
        }
        if let (Some(a), Some(b)) = (left, right) {
            push_prose(lines, benchmark_delta(a, b), p.muted(), width);
        }
        lines.push(Line::default());
    }
}

fn progress_series(samples: &[Snapshot], points: usize, kind: u8) -> Vec<Option<f64>> {
    if samples.is_empty() {
        return vec![None; points];
    }
    let first = samples.first().map(|s| s.elapsed_secs).unwrap_or(0.0);
    let last = samples.last().map(|s| s.elapsed_secs).unwrap_or(first);
    let elapsed = last - first;
    let coordinates = samples
        .iter()
        .enumerate()
        .map(|(index, s)| {
            if elapsed.is_finite() && elapsed > 0.0 {
                ((s.elapsed_secs - first) / elapsed).clamp(0.0, 1.0)
            } else if samples.len() > 1 {
                index as f64 / (samples.len() - 1) as f64
            } else {
                0.0
            }
        })
        .collect::<Vec<_>>();
    let value = |sample: &Snapshot| {
        match kind {
            0 => Some(sample.cpu_percent as f64),
            1 => {
                (sample.memory_total > 0).then(|| percent(sample.memory_used, sample.memory_total))
            }
            _ => sample
                .gpus
                .first()
                .and_then(|g| g.utilization)
                .map(f64::from),
        }
        .filter(|v| v.is_finite())
        .map(safe_percent)
    };
    let mut result = Vec::with_capacity(points);
    let mut right = 0;
    for point in 0..points {
        let target = point as f64 / points.saturating_sub(1).max(1) as f64;
        while right + 1 < samples.len() && coordinates[right] < target {
            right += 1;
        }
        if right == 0 {
            result.push(value(&samples[0]));
            continue;
        }
        let left = right - 1;
        let span = coordinates[right] - coordinates[left];
        result.push(match (value(&samples[left]), value(&samples[right])) {
            (Some(a), Some(b)) if span > 0.0 => {
                Some(a + (b - a) * ((target - coordinates[left]) / span).clamp(0.0, 1.0))
            }
            (_, b) if target >= coordinates[right] => b,
            (a, _) if target <= coordinates[left] => a,
            _ => None,
        });
    }
    result
}

fn spark_text(values: &[Option<f64>]) -> String {
    const BARS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    values
        .iter()
        .map(|value| {
            value
                .map(|v| BARS[((safe_percent(v) / 100.0) * 7.0).round() as usize])
                .unwrap_or('·')
        })
        .collect()
}

fn benchmark_lines(value: Option<&SavedBenchmark>, width: usize, p: Palette) -> Vec<Line<'static>> {
    let Some(value) = value else {
        return vec![Line::from(Span::styled("Nessuna prova", p.muted()))];
    };
    let mut lines = Vec::new();
    push_prose(
        &mut lines,
        format!("{} · {}", clean(&value.kind), clean(&value.recorded_utc)),
        p.muted(),
        width,
    );
    if let Some(cpu) = &value.cpu {
        push_prose(
            &mut lines,
            format!(
                "CPU {} · {} worker · build {}",
                clean(&cpu.algorithm),
                cpu.threads,
                available(&cpu.build_profile)
            ),
            p.text(),
            width,
        );
        push_prose(
            &mut lines,
            format!(
                "Single {} · Multi {}",
                metric_value(Some(cpu.single_ops_per_sec), "mix/s", false),
                metric_value(Some(cpu.multi_ops_per_sec), "mix/s", false)
            ),
            p.text(),
            width,
        );
        push_prose(
            &mut lines,
            format!(
                "Durata {}",
                metric_value(Some(cpu.duration_secs), "s", false)
            ),
            p.muted(),
            width,
        );
    }
    if let Some(llm) = &value.llm {
        push_prose(
            &mut lines,
            format!("{} / {}", clean(&llm.runtime), clean(&llm.model)),
            p.text(),
            width,
        );
        push_prose(
            &mut lines,
            format!(
                "{} token · {} · TTFT {}",
                llm.output_tokens
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "n/d".into()),
                metric_value(llm.generation_tokens_per_sec, "token/s", false),
                metric_value(llm.time_to_first_token_ms, "ms", false)
            ),
            p.text(),
            width,
        );
        push_prose(
            &mut lines,
            format!(
                "Durata {} · metodo {}",
                metric_value(Some(llm.wall_seconds), "s", false),
                available(&llm.throughput_kind),
            ),
            p.muted(),
            width,
        );
        push_prose(
            &mut lines,
            format!("Nota: {}", available(&llm.note)),
            p.muted(),
            width,
        );
    }
    lines
}

fn benchmark_delta(left: &SavedBenchmark, right: &SavedBenchmark) -> String {
    if left.kind != right.kind {
        return "Δ non calcolato: tipi di prova diversi.".into();
    }
    if let (Some(a), Some(b)) = (&left.cpu, &right.cpu)
        && a.algorithm == b.algorithm
        && a.threads == b.threads
        && !a.build_profile.is_empty()
        && a.build_profile == b.build_profile
    {
        return format!(
            "Stesso kernel/worker · Δ single {} · Δ multi {}. Verificare hardware e build.",
            metric_value(
                Some(b.single_ops_per_sec - a.single_ops_per_sec),
                "mix/s",
                true
            ),
            metric_value(
                Some(b.multi_ops_per_sec - a.multi_ops_per_sec),
                "mix/s",
                true
            )
        );
    }
    if let (Some(a), Some(b)) = (&left.llm, &right.llm)
        && a.runtime == b.runtime
        && a.model == b.model
        && a.note == b.note
        && !a.throughput_kind.is_empty()
        && a.throughput_kind == b.throughput_kind
        && a.throughput_kind != "unavailable"
    {
        return format!(
            "Stesso modello/runtime/metodo dichiarato · Δ throughput {} · Δ TTFT {}. Configurazione da verificare.",
            metric_value(
                a.generation_tokens_per_sec
                    .zip(b.generation_tokens_per_sec)
                    .map(|(a, b)| b - a),
                "token/s",
                true
            ),
            metric_value(
                a.time_to_first_token_ms
                    .zip(b.time_to_first_token_ms)
                    .map(|(a, b)| b - a),
                "ms",
                true
            )
        );
    }
    "Δ non calcolato: kernel, modello, runtime o metodo differenti/non verificabili.".into()
}

fn settings_page(frame: &mut Frame<'_>, area: Rect, state: &mut UiState, p: Palette) {
    let compact = area.height < 20;
    let rows = split(
        area,
        Direction::Vertical,
        &[
            Constraint::Length(if compact { 1 } else { 2 }),
            Constraint::Length(if compact { 5 } else { 12 }),
            Constraint::Min(2),
        ],
    );
    page_title(
        frame,
        rows[0],
        "Impostazioni · una postazione che ricorda",
        "Preferenze salvate automaticamente in SQLite · nessuna credenziale richiesta.",
        p,
    );
    let prefs = &state.workspace.preferences;
    let alerts = &prefs.alerts;
    let values = [
        if state.theme_light {
            "Chiaro".into()
        } else {
            "Scuro".into()
        },
        format!("{} ms", state.tick_ms),
        if alerts.enabled {
            "Sì".into()
        } else {
            "No".into()
        },
        format!("{:.0}%", alerts.cpu_percent),
        format!("{:.0}%", alerts.ram_percent),
        format!("{:.0}%", alerts.vram_percent),
        format!("{:.0} °C", alerts.gpu_temperature_c),
        format!("{:.0} °C", alerts.cpu_temperature_c),
        prefs.benchmark_repeats.to_string(),
    ];
    state.workspace.settings_field = state
        .workspace
        .settings_field
        .min(SETTINGS_FIELDS.len().saturating_sub(1));
    let table_rows = SETTINGS_FIELDS
        .iter()
        .zip(values)
        .map(|(label, value)| Row::new([(*label).to_string(), value]));
    let mut selected = TableState::default().with_selected(Some(state.workspace.settings_field));
    frame.render_stateful_widget(
        Table::new(
            table_rows,
            [Constraint::Percentage(67), Constraint::Percentage(33)],
        )
        .header(Row::new(["PREFERENZA", "VALORE"]).style(p.muted()))
        .highlight_symbol("▌ ")
        .row_highlight_style(p.accent().bg(p.elevated))
        .block(panel("↑↓ scegli · + − modifica", p)),
        rows[1],
        &mut selected,
    );
    let inner = rows[2].width.saturating_sub(2) as usize;
    let mut lines = Vec::new();
    push_prose(
        &mut lines,
        format!("DB {}", available(&state.workspace.db_path)),
        p.muted(),
        inner,
    );
    if let Some(error) = &state.workspace.last_error {
        push_prose(
            &mut lines,
            format!("! Salvataggio/database: {}", clean(error)),
            Style::default().fg(p.red),
            inner,
        );
    }
    push_prose(
        &mut lines,
        format!(
            "Salvataggio automatico · cooldown avvisi {}s · F6 backup SQLite",
            alerts.cooldown_seconds
        ),
        p.muted(),
        inner,
    );
    push_prose(
        &mut lines,
        "Tema, intervallo, ordinamento e Capacity Lab vengono ricordati. Gli avvisi usano solo sensori disponibili; non sono notifiche del sistema operativo.",
        p.muted(),
        inner,
    );
    push_prose(
        &mut lines,
        format!(
            "AVVISI · {} eventi · {}",
            state.workspace.alerts.len(),
            if alerts.enabled {
                "attivi"
            } else {
                "sospesi; eventi precedenti conservati"
            }
        ),
        p.accent(),
        inner,
    );
    if state.workspace.alerts.is_empty() {
        push_prose(
            &mut lines,
            "Nessun superamento registrato. Dato assente non equivale a valore sotto soglia.",
            p.muted(),
            inner,
        );
    }
    for alert in state.workspace.alerts.iter().rev() {
        push_prose(
            &mut lines,
            format!(
                "{}  {}",
                time_label(alert.timestamp_unix),
                clean(&alert.message)
            ),
            Style::default().fg(p.amber),
            inner,
        );
    }
    let max = lines
        .len()
        .saturating_sub(rows[2].height.saturating_sub(2) as usize);
    state.offset = state.offset.min(max.min(u16::MAX as usize) as u16);
    frame.render_widget(
        Paragraph::new(lines)
            .scroll((state.offset, 0))
            .block(panel("Archivio, salvataggio e avvisi · PgUp/PgDn", p)),
        rows[2],
    );
}

fn session_name_modal(frame: &mut Frame<'_>, area: Rect, state: &UiState, p: Palette) {
    let target = modal_area(area, 78, 12);
    frame.render_widget(Clear, target);
    let block = panel("Rinomina sessione", p);
    let inner = block.inner(target);
    frame.render_widget(block, target);
    let panes = split(
        inner,
        Direction::Vertical,
        &[
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(2),
        ],
    );
    let value = state.workspace.name_edit.as_deref().unwrap_or("");
    let width = target.width.saturating_sub(2) as usize;
    frame.render_widget(
        Paragraph::new("Un nome per riconoscere il lavoro svolto").style(p.accent()),
        panes[0],
    );
    let mut name = wrap_diagnostic_lines(
        vec![Line::styled(
            format!("{}▏", clean(value)),
            p.text().bg(p.elevated),
        )],
        width,
    );
    let visible = panes[1].height.max(1) as usize;
    if name.len() > visible {
        name.drain(..name.len() - visible);
    }
    frame.render_widget(Paragraph::new(name), panes[1]);
    frame.render_widget(
        Paragraph::new("Invio salva il nome · Esc annulla\nIl nome viene conservato in SQLite.")
            .style(Style::default().fg(p.amber)),
        panes[2],
    );
}

fn session_delete_modal(frame: &mut Frame<'_>, area: Rect, state: &UiState, p: Palette) {
    let target = modal_area(area, 78, 13);
    frame.render_widget(Clear, target);
    let block = panel("Conferma eliminazione", p);
    let inner = block.inner(target);
    frame.render_widget(block, target);
    let panes = split(
        inner,
        Direction::Vertical,
        &[Constraint::Min(1), Constraint::Length(2)],
    );
    let mut lines = vec![
        Line::from(Span::styled(
            "Eliminare questa sessione dall'archivio?",
            Style::default().fg(p.red).add_modifier(Modifier::BOLD),
        )),
        Line::default(),
    ];
    if let Some(session) = state.workspace.sessions.get(state.selected) {
        push_prose(
            &mut lines,
            format!("#{} {}", session.id, clean(&session.name)),
            p.accent(),
            target.width.saturating_sub(2) as usize,
        );
        push_prose(
            &mut lines,
            format!(
                "{} campioni · {} benchmark · {}",
                session.samples,
                session.benchmark_count,
                duration_label(session.duration_secs)
            ),
            p.text(),
            target.width.saturating_sub(2) as usize,
        );
    }
    lines.extend([
        Line::default(),
        Line::from("L'eliminazione rimuove i dati salvati della sessione."),
        Line::from("Un backup del database permette di conservarne una copia."),
    ]);
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), panes[0]);
    frame.render_widget(
        Paragraph::new("Invio elimina · Esc annulla\nL'operazione rimuove i dati dal database.")
            .style(Style::default().fg(p.amber)),
        panes[1],
    );
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
    let target = modal_area(area, 88, 30);
    frame.render_widget(Clear, target);
    if target.height < 24 {
        let lines = vec![
            Line::from(Span::styled("COMANDI · Esc / ? chiude", p.accent())),
            Line::from("Tab / Shift-Tab · ← →: 13 pagine · 1…9,0 hardware"),
            Line::from("F2 Sessioni · F3 Confronto · F4 Impostazioni"),
            Line::from("↑↓ / PgUp PgDn / Home End: scorri, seleziona"),
            Line::from("Processi: / cerca · s ordina · a verso · Invio dettagli"),
            Line::from("Spazio pausa · + − intervallo · t tema"),
            Line::from("e esporta · r registra · F6 backup · q esci"),
            Line::from("CPU: x / Shift+X poi Invio · c annulla"),
            Line::from("LLM: b / Shift+B misura · c annulla · F5 aggiorna"),
            Line::from("Sessioni: Invio replay · a A · b B · c confronta"),
            Line::from("n rinomina · Canc elimina · [ ] campione · l live"),
            Line::from("Capacity: [ ] campo · + − valore; Opzioni: ↑↓ + −"),
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
        Line::from("← → tutte le 13 pagine; 1…9,0 seleziona le viste hardware"),
        Line::from("F2 Sessioni · F3 Confronto · F4 Impostazioni · F6 backup SQLite"),
        Line::from("↑ ↓ · PgUp PgDn   Scorri dettagli / seleziona un processo o modello"),
        Line::from("Home / End        Primo / ultimo elemento"),
        Line::from("/                 Cerca processi per nome o PID; Invio applica"),
        Line::from("s / a             Cambia colonna di ordinamento / direzione"),
        Line::from("Invio             Dettagli del processo selezionato"),
        Line::from("Spazio            Congela / riprende la vista hardware"),
        Line::from("+ / −             Aumenta / riduce l'intervallo dei campioni"),
        Line::from("e / r             Esporta report / avvia o ferma registrazione locale"),
        Line::from("t                 Tema scuro / chiaro"),
        Line::default(),
        Line::from(Span::styled("LLM LOCALI E CAPACITY LAB", p.accent())),
        Line::from("LLM: ↑↓ modello, b poi Invio per inferenza locale (max 64 token)."),
        Line::from("Shift+B ripete il benchmark; c annulla; F5 aggiorna i runtime."),
        Line::from("CPU: x / Shift+X poi Invio per prova singola / ripetuta (~1,2s per prova)."),
        Line::from("Capacity: [ ] campo; + − modifica il parametro selezionato."),
        Line::from("La stima tratta RAM e VRAM separatamente. Non è un benchmark."),
        Line::from("Sessioni: Invio replay, a riferimento A, b candidata B, c confronto."),
        Line::from("n rinomina, Canc elimina con conferma; [ ] campione, l torna live."),
        Line::from("Impostazioni: ↑↓ sceglie un campo, + − cambia; salvataggio automatico."),
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
            if state.workspace.repeat_requested {
                "Eseguire la serie di inferenze locali?"
            } else {
                "Eseguire una breve inferenza locale?"
            },
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
            Line::from(format!(
                "{} prova/e · massimo 64 token per prova",
                if state.workspace.repeat_requested {
                    state.workspace.preferences.benchmark_repeats
                } else {
                    1
                }
            )),
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

fn cpu_benchmark_modal(frame: &mut Frame<'_>, area: Rect, state: &UiState, p: Palette) {
    let target = modal_area(area, 80, 19);
    frame.render_widget(Clear, target);
    let runs = if state.workspace.repeat_requested {
        state.workspace.preferences.benchmark_repeats
    } else {
        1
    };
    let lines = vec![
        Line::from(Span::styled(
            "Misurare la capacità di calcolo della CPU?",
            p.accent(),
        )),
        Line::default(),
        Line::from(format!(
            "{runs} prova/e · circa {:.1}s di hash mixing: una fase single",
            runs as f64 * 1.2
        )),
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
    let target = modal_area(area, 80, 19);
    frame.render_widget(Clear, target);
    let lines = if let Some(proc) = &state.process_detail_row {
        vec![
            Line::from(Span::styled(clean(&proc.name), p.accent())),
            Line::from(Span::styled("Istantanea all’apertura", p.muted())),
            Line::default(),
            Line::from(format!("PID             {}", proc.pid)),
            Line::from(format!("CPU macchina    {:.2}%", proc.cpu_percent)),
            Line::from(format!("RAM residente   {}", bytes(proc.memory_bytes))),
            Line::from(format!(
                "GPU dedicata    {}",
                proc.gpu_memory_bytes
                    .map(bytes)
                    .unwrap_or_else(|| "n/d".into())
            )),
            Line::from(format!(
                "GPU condivisa   {}",
                proc.gpu_shared_memory_bytes
                    .map(bytes)
                    .unwrap_or_else(|| "n/d".into())
            )),
            Line::from(format!(
                "GPU max motore  {}",
                optional_percent(proc.gpu_percent)
            )),
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
                "Istantanea. Attribuzioni GPU per PID; non sommare i PID come memoria fisica. Esc chiude.",
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
            for tab in 0..TABS.len() {
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
        assert_eq!(sidebar_tab_at(100, 24, 10, 18), Some(12));
        assert_eq!(sidebar_tab_at(120, 40, 10, 30), Some(12));
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

    fn workspace_fixture() -> UiState {
        use crate::storage::{ComparisonMetric, MetricSummary, SessionData};
        let mut state = UiState::default();
        state.workspace.db_path = "C:\\Dati\\RUST-CPU\\workspace.sqlite3".into();
        state.workspace.sessions = (1..=60)
            .map(|id| SessionSummary {
                id,
                name: format!("Sessione {id:02} · analisi locale"),
                started_utc: "2026-10-19T10:30:00Z".into(),
                ended_utc: Some("2026-10-19T10:40:00Z".into()),
                state: "complete".into(),
                samples: 600,
                duration_secs: 600.0,
                cpu: MetricSummary {
                    average: Some(42.0),
                    peak: Some(95.0),
                    p95: Some(90.0),
                },
                ram: MetricSummary {
                    average: Some((8u64 << 30) as f64),
                    peak: Some((12u64 << 30) as f64),
                    ..Default::default()
                },
                vram: MetricSummary {
                    peak: Some((4u64 << 30) as f64),
                    ..Default::default()
                },
                ..Default::default()
            })
            .collect();
        state.workspace.baseline_id = Some(1);
        state.workspace.candidate_id = Some(2);
        state.workspace.active_session = Some(3);
        let samples = vec![
            Snapshot {
                elapsed_secs: 0.0,
                cpu_percent: 0.0,
                memory_used: 2 << 30,
                memory_total: 8 << 30,
                ..Default::default()
            },
            Snapshot {
                elapsed_secs: 10.0,
                cpu_percent: 100.0,
                memory_used: 4 << 30,
                memory_total: 8 << 30,
                ..Default::default()
            },
        ];
        state.workspace.comparison = Some(SessionComparison {
            left: state.workspace.sessions[0].clone(),
            right: state.workspace.sessions[1].clone(),
            metrics: vec![
                ComparisonMetric {
                    label: "CPU media".into(),
                    unit: "%".into(),
                    left: Some(0.0),
                    right: Some(12.0),
                    delta: Some(12.0),
                    delta_percent: None,
                    lower_is_better: true,
                },
                ComparisonMetric {
                    label: "RAM picco".into(),
                    unit: "B".into(),
                    left: Some((8u64 << 30) as f64),
                    right: Some((6u64 << 30) as f64),
                    delta: Some(-((2u64 << 30) as f64)),
                    delta_percent: Some(-25.0),
                    lower_is_better: true,
                },
                ComparisonMetric {
                    label: "GPU mancante".into(),
                    unit: "%".into(),
                    ..Default::default()
                },
            ],
            warnings: vec!["Durate o contesti diversi: verificare prima di confrontare.".into()],
            left_series: samples.clone(),
            right_series: samples.clone(),
            left_benchmarks: vec![SavedBenchmark {
                kind: "llm".into(),
                recorded_utc: "2026-10-19T10:35:00Z".into(),
                llm: Some(BenchmarkResult {
                    runtime: "Ollama".into(),
                    model: "test:8b".into(),
                    note: "decode nativo".into(),
                    generation_tokens_per_sec: Some(30.0),
                    ..Default::default()
                }),
                ..Default::default()
            }],
            right_benchmarks: vec![SavedBenchmark {
                kind: "llm".into(),
                recorded_utc: "2026-10-19T10:35:00Z".into(),
                llm: Some(BenchmarkResult {
                    runtime: "Ollama".into(),
                    model: "test:8b".into(),
                    note: "tempo totale end-to-end".into(),
                    generation_tokens_per_sec: Some(20.0),
                    ..Default::default()
                }),
                ..Default::default()
            }],
            benchmarks: vec![crate::storage::BenchmarkComparison {
                label: "Ollama test:8b".into(),
                kind: "llm".into(),
                method: "native_decode".into(),
                runs_left: 3,
                runs_right: 3,
                metrics: vec![ComparisonMetric {
                    label: "Throughput mediana".into(),
                    unit: "token/s".into(),
                    left: Some(30.0),
                    right: Some(33.0),
                    delta: Some(3.0),
                    delta_percent: Some(10.0),
                    ..Default::default()
                }],
                warnings: vec!["Verificare contesto e cache.".into()],
            }],
        });
        state.workspace.loaded = Some(SessionData {
            summary: state.workspace.sessions[0].clone(),
            samples,
            total_samples: 600,
            ..Default::default()
        });
        state.workspace.replay_index = 1;
        state.workspace.alerts = vec![crate::settings::AlertEvent {
            timestamp_unix: 1792405800,
            kind: "ram".into(),
            message: "RAM oltre soglia: 95%".into(),
            value: 95.0,
            threshold: 90.0,
        }];
        state
    }

    #[test]
    fn workspace_pages_handle_empty_many_sessions_and_compact_navigation() {
        let (inv, snapshot, llm, history) = fixture();
        for (width, height) in [(40, 12), (64, 18), (100, 24), (140, 45)] {
            for tab in 10..TABS.len() {
                let mut state = workspace_fixture();
                state.active_tab = tab;
                state.selected = usize::MAX;
                state.offset = u16::MAX;
                state.workspace.settings_field = 8;
                let text = draw(width, height, &inv, &snapshot, &llm, &mut state, &history);
                assert!(
                    text.contains(if tab == 10 {
                        "Sessioni"
                    } else if tab == 11 {
                        "Confronto"
                    } else {
                        "Impostazioni"
                    }),
                    "active workspace tab must remain discoverable"
                );
                draw(
                    width,
                    height,
                    &Inventory::default(),
                    &Snapshot::default(),
                    &LlmState::default(),
                    &mut UiState {
                        active_tab: tab,
                        ..Default::default()
                    },
                    &History::default(),
                );
            }
        }
    }

    #[test]
    fn replay_session_markers_and_settings_show_real_state() {
        let (inv, snapshot, llm, history) = fixture();
        let mut state = workspace_fixture();
        state.active_tab = 10;
        let text = draw(140, 45, &inv, &snapshot, &llm, &mut state, &history);
        assert!(text.contains("REPLAY"));
        assert!(text.contains("campione 2/2"));
        assert!(text.contains("600 totali"));
        assert!(text.contains("A #1"));
        assert!(text.contains("B #2"));
        assert!(text.contains("Sessione 01"));
        state.active_tab = 12;
        state.workspace.settings_field = 8;
        let text = draw(120, 38, &inv, &snapshot, &llm, &mut state, &history);
        assert!(text.contains("Ripetizioni benchmark"));
        assert!(text.contains("workspace.sqlite3"));
        assert!(text.contains("RAM oltre soglia"));
    }

    #[test]
    fn comparison_preserves_unknown_zero_baseline_and_benchmark_method_caveats() {
        let state = workspace_fixture();
        let comparison = state.workspace.comparison.as_ref().unwrap();
        let lines = comparison_lines(comparison, 100, Palette::new(false));
        let text = lines
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("+12.0%"));
        assert!(text.contains("−2.0 GiB"));
        assert!(text.contains("−25.0%"));
        assert!(text.contains("n/d"));
        assert!(text.contains("A = 0"));
        assert!(text.contains("Δ non calcolato"));
        assert!(text.contains("decode nativo"));
        assert!(text.contains("end-to-end"));
        assert!(!text.contains("inf%"));
        assert!(lines.iter().all(|line| line.width() <= 100));
        let samples = &comparison.left_series;
        assert_eq!(
            progress_series(samples, 5, 0),
            vec![Some(0.0), Some(25.0), Some(50.0), Some(75.0), Some(100.0)]
        );
        assert_eq!(spark_text(&progress_series(samples, 5, 2)), "·····");
        assert_eq!(metric_value(Some(-0.0), "B", true), "+0 B");
    }

    #[test]
    fn workspace_modals_keep_actions_visible_for_long_unicode_names() {
        let (inv, snapshot, llm, history) = fixture();
        for (width, height) in [(64, 18), (100, 24), (140, 45)] {
            let mut state = workspace_fixture();
            state.active_tab = 10;
            state.workspace.sessions[0].name = "Sessione 日本語 🦀 con nome lungo ".repeat(20);
            state.workspace.name_edit = Some(state.workspace.sessions[0].name.clone());
            let text = draw(width, height, &inv, &snapshot, &llm, &mut state, &history);
            assert!(text.contains("Invio salva il nome"));
            assert!(text.contains("Esc annulla"));
            state.workspace.name_edit = None;
            state.workspace.delete_confirm = true;
            let text = draw(width, height, &inv, &snapshot, &llm, &mut state, &history);
            assert!(text.contains("Invio elimina"));
            assert!(text.contains("Esc annulla"));
        }
    }

    #[test]
    fn comparison_end_then_up_persists_physical_offset() {
        let (inv, snapshot, llm, history) = fixture();
        let mut state = workspace_fixture();
        state.active_tab = 11;
        state.offset = u16::MAX;
        let end = draw(64, 18, &inv, &snapshot, &llm, &mut state, &history);
        assert!(state.offset > 0 && state.offset < u16::MAX);
        let offset = state.offset;
        state.offset -= 1;
        let up = draw(64, 18, &inv, &snapshot, &llm, &mut state, &history);
        assert_eq!(state.offset, offset - 1);
        assert_ne!(end, up);
    }

    #[test]
    fn global_history_includes_standalone_observations_and_bounds_latest_entries() {
        let (inv, snapshot, llm, history) = fixture();
        let mut state = workspace_fixture();
        state.active_tab = 7;
        state.workspace.loaded = None;
        state.workspace.benchmark_history=(1..=105).map(|id|SavedBenchmark{id,session_id:None,kind:"observed".into(),recorded_utc:"2026-10-19T10:30:00Z".into(),llm:Some(BenchmarkResult{runtime:"Proxy locale".into(),model:format!("orphan-model-{id:03}"),throughput_kind:"end_to_end".into(),note:"Osservazione di una richiesta transitata dal proxy; non misura tutte le applicazioni.".into(),..Default::default()}),..Default::default()}).collect();
        let mut lines = Vec::new();
        append_llm_history(&mut lines, &llm, &state, 70, Palette::new(false));
        let text = lines
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("ultime 100"));
        assert!(text.contains("senza sessione"));
        assert!(text.contains("observed"));
        assert!(text.contains("orphan-model-105"));
        assert!(text.contains("orphan-model-006"));
        assert!(!text.contains("orphan-model-005"));
        assert!(lines.iter().all(|line| line.width() <= 70));
        state.offset = u16::MAX;
        let end = draw(64, 18, &inv, &snapshot, &llm, &mut state, &history);
        assert!(state.offset > 0 && state.offset < u16::MAX);
        let offset = state.offset;
        state.offset -= 1;
        let up = draw(64, 18, &inv, &snapshot, &llm, &mut state, &history);
        assert_eq!(state.offset, offset - 1);
        assert_ne!(end, up);
    }

    #[test]
    fn archive_comparison_and_configuration_headers_do_not_claim_live_samples() {
        let (inv, snapshot, llm, history) = fixture();
        for (tab, label) in [(10, "ARCHIVIO"), (11, "CONFRONTO"), (12, "CONFIG")] {
            let mut state = UiState {
                active_tab: tab,
                ..Default::default()
            };
            let text = draw(120, 38, &inv, &snapshot, &llm, &mut state, &history);
            let header = text.lines().next().unwrap();
            assert!(header.contains(label));
            assert!(!header.contains("LIVE"));
            assert!(!header.contains("CPU 42.5%"));
        }
    }

    #[test]
    fn repeated_benchmark_confirmation_shows_count_and_cpu_load_duration() {
        let (inv, snapshot, llm, history) = fixture();
        let mut state = UiState {
            active_tab: 1,
            cpu_benchmark_confirm: true,
            ..Default::default()
        };
        state.workspace.repeat_requested = true;
        state.workspace.preferences.benchmark_repeats = 4;
        let text = draw(120, 38, &inv, &snapshot, &llm, &mut state, &history);
        assert!(text.contains("4 prova/e"));
        assert!(text.contains("4.8s"));
        state.cpu_benchmark_confirm = false;
        state.active_tab = 7;
        state.benchmark_confirm = true;
        let text = draw(120, 38, &inv, &snapshot, &llm, &mut state, &history);
        assert!(text.contains("4 prova/e"));
        assert!(text.contains("massimo 64 token per prova"));
    }

    #[test]
    fn comparison_provenance_is_on_its_own_line_and_reachable_by_scrolling() {
        let (inv, snapshot, llm, history) = fixture();
        let mut state = workspace_fixture();
        state.active_tab = 11;
        let lines = comparison_lines(
            state.workspace.comparison.as_ref().unwrap(),
            60,
            Palette::new(false),
        );
        for note in ["Nota: decode nativo", "Nota: tempo totale end-to-end"] {
            let line = lines
                .iter()
                .position(|line| line.to_string().contains(note))
                .expect("method provenance must remain readable on a dedicated physical line");
            state.offset = line as u16;
            let text = draw(64, 18, &inv, &snapshot, &llm, &mut state, &history);
            assert!(
                text.contains(note),
                "method provenance must be reachable inside the real compact viewport"
            );
        }
    }
}
