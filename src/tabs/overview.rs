use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use crate::app::{App, Snapshot};
use crate::insights::Severity;
use crate::process_tree::Metric;
use crate::ui::tree::{Column, TreeTable};
use crate::ui::{
    graph::{self, GraphStyle},
    palette as p,
    widgets::{block_bar_styled, human_rate, panel},
};

pub fn draw(f: &mut Frame, area: Rect, app: &App, snap: &Snapshot) {
    let v = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(7), // KPI strip
            Constraint::Min(0),    // body
            Constraint::Length(6), // insights strip
        ])
        .split(area);

    draw_kpi_strip(f, v[0], app, snap);
    draw_middle(f, v[1], app, snap);
    draw_insights_strip(f, v[2], app);
}

fn draw_kpi_strip(f: &mut Frame, area: Rect, app: &App, snap: &Snapshot) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Ratio(1, 6),
            Constraint::Ratio(1, 6),
            Constraint::Ratio(1, 6),
            Constraint::Ratio(1, 6),
            Constraint::Ratio(1, 6),
            Constraint::Ratio(1, 6),
        ])
        .split(area);

    let style = app.graph_style;
    kpi_tile(
        f,
        cols[0],
        "CPU",
        format!("{:.0}%", snap.cpu.usage_pct),
        kpi_color(snap.cpu.usage_pct, 60.0, 85.0),
        &app.history.cpu.to_vec(),
        100.0,
        style,
        app.graph_opts(),
    );
    let mem_pct = if snap.mem.total_bytes > 0 {
        100.0 * snap.mem.used_bytes as f32 / snap.mem.total_bytes as f32
    } else {
        0.0
    };
    kpi_tile(
        f,
        cols[1],
        "MEM",
        format!("{:.0}%", mem_pct),
        kpi_color(mem_pct, 70.0, 90.0),
        &app.history
            .mem
            .to_vec()
            .iter()
            .map(|v| v * 100.0)
            .collect::<Vec<f32>>(),
        100.0,
        style,
        app.graph_opts(),
    );
    let swap_pct = if snap.mem.swap_total_bytes > 0 {
        100.0 * snap.mem.swap_used_bytes as f32 / snap.mem.swap_total_bytes as f32
    } else {
        0.0
    };
    kpi_tile(
        f,
        cols[2],
        "SWAP",
        format!("{:.0}%", swap_pct),
        kpi_color(swap_pct, 25.0, 75.0),
        &[],
        100.0,
        style,
        app.graph_opts(),
    );
    let io = snap.disk_io.read_rate + snap.disk_io.write_rate;
    kpi_tile(
        f,
        cols[3],
        "DISK IO",
        human_rate(io),
        p::brand(),
        &app.history
            .io_rate
            .to_vec()
            .iter()
            .map(|v| *v as f32)
            .collect::<Vec<f32>>(),
        max_or(&app.history.io_rate.to_vec(), 1.0) as f32,
        style,
        app.graph_opts(),
    );
    let net: f64 = snap.net.iter().map(|i| i.rx_rate + i.tx_rate).sum();
    kpi_tile(
        f,
        cols[4],
        "NET",
        human_rate(net),
        p::brand(),
        &app.history
            .net_rate
            .to_vec()
            .iter()
            .map(|v| *v as f32)
            .collect::<Vec<f32>>(),
        max_or(&app.history.net_rate.to_vec(), 1.0) as f32,
        style,
        app.graph_opts(),
    );
    // Current GPU util = cross-device max, matching history.gpu_util. None
    // when no device exposes live util (so we show "—" + an empty chart
    // rather than a misleading flat-zero line).
    let gpu_cur: Option<f32> = snap
        .gpus
        .iter()
        .filter_map(|g| g.util_pct)
        .fold(None, |acc, u| Some(acc.map_or(u, |m: f32| m.max(u))));
    let gpu_series = if gpu_cur.is_some() {
        app.history.gpu_util.to_vec()
    } else {
        Vec::new()
    };
    kpi_tile(
        f,
        cols[5],
        "GPU",
        gpu_cur
            .map(|u| format!("{:.0}%", u))
            .unwrap_or_else(|| "—".into()),
        gpu_cur
            .map(|u| kpi_color(u, 60.0, 85.0))
            .unwrap_or(p::text_muted()),
        &gpu_series,
        100.0,
        style,
        app.graph_opts(),
    );
}

fn kpi_tile(
    f: &mut Frame,
    area: Rect,
    label: &str,
    value: String,
    accent: ratatui::style::Color,
    series: &[f32],
    series_max: f32,
    style: GraphStyle,
    opts: crate::ui::graph::GraphOpts,
) {
    let block = panel(label);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let h = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(inner);

    let val = Paragraph::new(Line::from(vec![Span::styled(
        value,
        Style::default().fg(accent).add_modifier(Modifier::BOLD),
    )]))
    .style(Style::default().bg(p::bg()));
    f.render_widget(val, h[0]);

    let bar = block_bar_styled(
        if series_max > 0.0 {
            series.last().copied().unwrap_or(0.0) / series_max
        } else {
            0.0
        },
        h[1].width,
        accent,
        style,
    );
    f.render_widget(
        Paragraph::new(bar).style(Style::default().bg(p::bg())),
        h[1],
    );

    if !series.is_empty() && series_max > 0.0 {
        let normalized: Vec<f32> = series.iter().map(|v| (v / series_max).min(1.0)).collect();
        graph::render(f, h[2], &normalized, style, accent, opts);
    }
}

fn draw_middle(f: &mut Frame, area: Rect, app: &App, snap: &Snapshot) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(area);

    draw_per_core(f, cols[0], snap, app.graph_style);
    draw_top_procs(f, cols[1], app, snap);
}

fn draw_per_core(f: &mut Frame, area: Rect, snap: &Snapshot, style: GraphStyle) {
    let block = panel("Per-core CPU");
    let inner = block.inner(area);
    f.render_widget(block, area);

    let mut lines: Vec<Line> = Vec::new();
    for (i, pct) in snap.cpu.per_core.iter().enumerate() {
        let color = kpi_color(*pct, 60.0, 85.0);
        let bar = block_bar_styled(*pct / 100.0, inner.width.saturating_sub(12), color, style);
        let mut spans = vec![Span::styled(
            format!("c{:>2} ", i),
            Style::default().fg(p::text_muted()),
        )];
        spans.extend(bar.spans);
        spans.push(Span::styled(
            format!(" {:>3.0}%", pct),
            Style::default().fg(p::text_primary()),
        ));
        lines.push(Line::from(spans));
    }
    f.render_widget(
        Paragraph::new(lines).style(Style::default().bg(p::bg())),
        inner,
    );
}

/// Top-level groups of the shared process tree, collapsed. The tree itself
/// is navigated on the CPU and Procs tabs.
fn draw_top_procs(f: &mut Frame, area: Rect, app: &App, snap: &Snapshot) {
    let block = panel(format!(
        "Top consumers (by CPU · {})",
        app.user_config.cpu_group.label()
    ));
    let inner = block.inner(area);
    f.render_widget(block, area);
    let rows = app.summary_rows(snap, Metric::Cpu);
    f.render_widget(
        TreeTable::new(&rows, "APPLICATION / PROJECT")
            .columns([Column::cpu().sorted(true), Column::memory()]),
        inner,
    );
}

fn draw_insights_strip(f: &mut Frame, area: Rect, app: &App) {
    let active = app
        .insights
        .iter()
        .filter(|i| i.severity != Severity::Info)
        .count();
    let title = if active == 0 {
        "Insights — system nominal".to_string()
    } else {
        format!("Insights — {} active (press [+] for detail)", active)
    };
    let block = panel(title);
    let inner = block.inner(area);
    f.render_widget(block, area);

    if app.insights.is_empty() {
        f.render_widget(
            Paragraph::new(Line::from(vec![Span::styled(
                "No anomalies in CPU, Memory, Disks, Procs, Net.",
                Style::default().fg(p::text_muted()),
            )]))
            .style(Style::default().bg(p::bg())),
            inner,
        );
        return;
    }

    let take = inner.height as usize;
    let lines: Vec<Line> = app
        .insights
        .iter()
        .take(take)
        .map(|ins| {
            let (color, label) = match ins.severity {
                Severity::Crit => (p::status_error(), "CRIT"),
                Severity::Warn => (p::status_warn(), "WARN"),
                Severity::Info => (p::brand(), "INFO"),
            };
            Line::from(vec![
                Span::styled(
                    format!(" {} ", label),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ),
                Span::raw(" "),
                Span::styled(ins.title.clone(), Style::default().fg(p::text_primary())),
                Span::styled(
                    format!("   \u{2192} {}", ins.suggested_tab.title()),
                    Style::default().fg(p::text_muted()),
                ),
            ])
        })
        .collect();

    f.render_widget(
        Paragraph::new(lines).style(Style::default().bg(p::bg())),
        inner,
    );
}

pub(super) fn kpi_color(v: f32, warn: f32, crit: f32) -> ratatui::style::Color {
    if v >= crit {
        p::status_error()
    } else if v >= warn {
        p::status_warn()
    } else {
        p::status_good()
    }
}

fn max_or(xs: &[f64], min: f64) -> f64 {
    xs.iter().cloned().fold(min, f64::max)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kpi_color_thresholds() {
        assert_eq!(kpi_color(0.0, 70.0, 90.0), p::status_good());
        assert_eq!(kpi_color(69.9, 70.0, 90.0), p::status_good());
        assert_eq!(kpi_color(70.0, 70.0, 90.0), p::status_warn());
        assert_eq!(kpi_color(89.9, 70.0, 90.0), p::status_warn());
        assert_eq!(kpi_color(90.0, 70.0, 90.0), p::status_error());
        assert_eq!(kpi_color(100.0, 70.0, 90.0), p::status_error());
    }

    #[test]
    fn kpi_color_crit_branch_runs_first() {
        // If a caller swaps warn/crit, crit still wins because the
        // branch order locks it in. Documents the contract.
        assert_eq!(kpi_color(95.0, 100.0, 90.0), p::status_error());
    }

    #[test]
    fn max_or_empty_slice_returns_min() {
        assert_eq!(max_or(&[], 5.0), 5.0);
    }

    #[test]
    fn max_or_picks_largest_above_min() {
        assert_eq!(max_or(&[1.0, 4.0, 2.0], 0.0), 4.0);
    }

    #[test]
    fn max_or_returns_min_when_all_values_lower() {
        // `min` acts as a floor — used to keep graph axes non-zero even
        // when no samples are big enough yet.
        assert_eq!(max_or(&[1.0, 2.0, 3.0], 100.0), 100.0);
    }

    #[test]
    fn max_or_single_element() {
        assert_eq!(max_or(&[42.0], 0.0), 42.0);
    }

    fn grouped_snapshot() -> Snapshot {
        let proc = |pid, name: &str, cpu, power| crate::collect::ProcTick {
            pid,
            name: name.into(),
            exe: Some(format!("/bin/{name}")),
            cpu_pct: cpu,
            mem_rss: 1024,
            power_w: Some(power),
            ..Default::default()
        };
        Snapshot {
            cpu: crate::collect::CpuTick {
                per_core: vec![0.0; 8],
                ..Default::default()
            },
            procs: vec![
                proc(10, "browser", 240.0, 0.6),
                proc(11, "browser", 80.0, 0.6),
                proc(20, "postgres", 8.0, 0.9),
                proc(30, "idle", 0.0, 0.001),
                proc(40, "alpha", 56.0, 0.0),
                proc(50, "zeta", 72.0, 0.0),
            ],
            ..Default::default()
        }
    }

    fn screen(width: u16, height: u16, draw: impl FnOnce(&mut Frame)) -> Vec<String> {
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
        terminal.draw(draw).unwrap();
        let buffer = terminal.backend().buffer();
        (0..height)
            .map(|y| (0..width).map(|x| buffer[(x, y)].symbol()).collect())
            .collect()
    }

    #[test]
    fn top_consumers_are_collapsed_groups_of_the_shared_tree() {
        let mut app = App::new(
            crate::app::TabId::Overview,
            crate::config::SyswatchConfig::default(),
        );
        let snap = grouped_snapshot();
        let rows = screen(80, 12, |f| draw_top_procs(f, f.area(), &app, &snap));
        assert!(rows[0].contains("Top consumers (by CPU · Application)"));
        let browser = rows.iter().find(|l| l.contains("browser")).unwrap();
        assert!(
            browser.contains(" 2 ") && browser.contains("40%"),
            "{browser}"
        );
        // 9% and 7% share a 5% band; the summary still puts the larger first.
        let zeta = rows.iter().position(|l| l.contains("zeta")).unwrap();
        let alpha = rows.iter().position(|l| l.contains("alpha")).unwrap();
        assert!(zeta < alpha, "{rows:#?}");
        let postgres = rows.iter().position(|l| l.contains("postgres")).unwrap();
        assert!(rows[postgres].contains(" 20 "), "{}", rows[postgres]);
        assert!(!rows.iter().any(|l| l.contains(" 10 ")), "{rows:#?}");

        let mut snap = snap;
        snap.procs[2].scope = crate::collect::ProcessScope::System;
        app.user_config.cpu_group = crate::process_tree::GroupMode::Project;
        let rows = screen(80, 12, |f| draw_top_procs(f, f.area(), &app, &snap));
        assert!(rows[0].contains("Top consumers (by CPU · Project)"));
        assert!(
            rows.iter().any(|l| l.contains("postgres [system]")),
            "{rows:#?}"
        );
    }
}
