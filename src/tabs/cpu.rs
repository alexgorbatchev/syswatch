use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Cell, Paragraph, Row, Table, TableState, Wrap},
    Frame,
};

use crate::app::{App, Snapshot, TreeView};
use crate::process_tree::{self, GroupKey, TreeGroup, TreeRow, TreeState};
use crate::ui::{
    palette as p,
    widgets::{human_bytes, panel, sparkline_string},
};

#[cfg(test)]
#[path = "cpu_tests.rs"]
mod tests;

/// The CPU tab's process tree plus its core-activity and detail panes.
#[derive(Debug, Clone, Default)]
pub struct CpuState {
    pub tree: TreeState,
    pub show_cores: bool,
    pub core_page: usize,
    pub detail_scroll: usize,
}

pub fn draw(f: &mut Frame, area: Rect, app: &App, snap: &Snapshot) {
    let (groups, rows) = app.tree_rows(TreeView::Cpu, snap);
    let selected = app.cpu.tree.selected_index(&rows);
    let roomy = area.height >= 24;
    let show_detail = roomy || (!app.cpu.show_cores && rows.get(selected).is_some());
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(if roomy { 5 } else { 4 }),
            Constraint::Length(if roomy { 5 } else { 4 }),
            Constraint::Min(3),
            Constraint::Length(if show_detail { 6 } else { 0 }),
            Constraint::Length(if roomy {
                if app.cpu.show_cores {
                    6
                } else {
                    3
                }
            } else if app.cpu.show_cores {
                4
            } else {
                0
            }),
            Constraint::Length(1),
        ])
        .split(area);

    draw_assessment(f, sections[0], snap, &groups);
    let samples = displayed_history(app);
    draw_evidence(f, sections[1], app, snap, &samples);
    draw_groups(f, sections[2], app, &rows, selected);
    if show_detail {
        draw_detail(f, sections[3], rows.get(selected), app.cpu.detail_scroll);
    }
    if roomy || app.cpu.show_cores {
        draw_cores(f, sections[4], app, snap);
    }
    f.render_widget(
        Paragraph::new("b:Group  ↑↓:Select  Enter:Expand  c:Cores  ←→:History")
            .style(Style::default().fg(p::key_hint()).bg(p::bg())),
        sections[5],
    );
}

fn draw_assessment(f: &mut Frame, area: Rect, snap: &Snapshot, groups: &[TreeGroup<'_>]) {
    let mut lines = vec![Line::styled(
        assessment(snap),
        Style::default().fg(p::text_primary()).bold(),
    )];
    if let Some(top) = groups
        .iter()
        .max_by(|a, b| {
            a.totals
                .cpu_pct
                .unwrap_or(0.0)
                .total_cmp(&b.totals.cpu_pct.unwrap_or(0.0))
        })
        .filter(|g| g.totals.cpu_pct.unwrap_or(0.0) > 0.0)
    {
        lines.push(muted_deleted(format!(
            "{} {} is the largest observed CPU consumer ({:.0}% of machine).",
            top.icon,
            top.label,
            top.totals.cpu_pct.unwrap_or(0.0)
        )));
        lines.push(Line::styled(
            if top.members.len() == 1 && matches!(top.key, GroupKey::Application { .. }) {
                "Select its row to inspect process details. High usage alone does not establish a fault."
            } else {
                "Expand to inspect its processes. High usage alone does not establish a fault."
            },
            Style::default().fg(p::text_muted()),
        ));
    } else {
        lines.push(Line::raw(
            "No active CPU consumer identified in this sample.",
        ));
    }
    f.render_widget(
        Paragraph::new(lines)
            .block(panel("What's slowing things down?"))
            .style(Style::default().fg(p::text_primary()).bg(p::bg()))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn assessment(snap: &Snapshot) -> &'static str {
    if let Some(pressure) = &snap.pressure {
        if pressure.cpu_some >= 10.0 {
            return "CPU contention detected: tasks are waiting for processor time.";
        }
        if pressure.mem_some >= 10.0 {
            return "Memory pressure detected; inspect the Memory tab.";
        }
        if pressure.io_some >= 10.0 {
            return "I/O pressure detected; inspect the Disks tab.";
        }
        if snap.cpu.usage_pct >= 80.0 {
            return "CPU busy; little CPU waiting detected.";
        }
        "No clear CPU contention in the available measurements."
    } else if snap.cpu.usage_pct >= 80.0 {
        "CPU activity is high; contention measurement unavailable."
    } else {
        "CPU under 80%; contention measurement unavailable."
    }
}

/// Every chart ends at the displayed snapshot, including during replay/scrub.
fn displayed_history(app: &App) -> Vec<&Snapshot> {
    let count = app.history.session.len().saturating_sub(app.scrub_offset);
    app.history
        .session
        .iter()
        .take(count)
        .skip(count.saturating_sub(32))
        .collect()
}

fn draw_evidence(f: &mut Frame, area: Rect, app: &App, snap: &Snapshot, samples: &[&Snapshot]) {
    let elapsed = samples
        .first()
        .and_then(|first| snap.t.duration_since(first.t).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let block = panel(format!(
        "Evidence · last {elapsed}s · CPU across whole machine"
    ));
    let inner = block.inner(area);
    f.render_widget(block, area);
    let cpu: Vec<_> = samples.iter().map(|s| s.cpu.usage_pct / 100.0).collect();
    let width = inner.width.saturating_sub(26) as usize;
    let chart = |values: &[f32]| {
        sparkline_string(
            &values[values.len().saturating_sub(width)..],
            app.graph_style,
        )
    };
    let mut lines = vec![Line::from(vec![
        Span::styled(
            format!("CPU busy     {:>5.0}%    ", snap.cpu.usage_pct),
            Style::default().fg(p::brand()),
        ),
        Span::styled(chart(&cpu), Style::default().fg(p::brand())),
    ])];
    if let Some(pressure) = &snap.pressure {
        // Do not fill unavailable pressure samples with invented zeroes.
        let mut waiting: Vec<_> = samples
            .iter()
            .rev()
            .map_while(|s| s.pressure.as_ref().map(|p| p.cpu_some / 100.0))
            .collect();
        waiting.reverse();
        lines.push(Line::raw(format!(
            "CPU waiting  {:>5.0}%    {}",
            pressure.cpu_some,
            chart(&waiting)
        )));
        lines.push(Line::styled(
            format!(
                "Waiting: 10s average · memory {:.1}% · I/O {:.1}%",
                pressure.mem_some, pressure.io_some
            ),
            Style::default().fg(p::text_muted()),
        ));
    } else {
        lines.push(Line::styled(
            "CPU waiting: unavailable on this sample/platform",
            Style::default().fg(p::text_muted()),
        ));
    }
    f.render_widget(
        Paragraph::new(lines).style(Style::default().bg(p::bg())),
        inner,
    );
}

fn draw_groups(f: &mut Frame, area: Rect, app: &App, rows: &[TreeRow<'_>], selected: usize) {
    let block = panel(format!(
        "Largest CPU consumers · {} · system last",
        app.user_config.cpu_group.label()
    ));
    let inner = block.inner(area);
    if rows.is_empty() {
        f.render_widget(
            Paragraph::new("No process samples available.").block(block),
            area,
        );
        return;
    }
    let pid_width = rows
        .iter()
        .filter_map(|row| row.process.map(|p| p.pid.to_string().len()))
        .max()
        .unwrap_or(0)
        .max(3);
    // Give trends a quarter of the row, reserving space for names, fixed
    // columns and five separators so long PIDs remain visible.
    let reserved_width = 12 + 5 + pid_width + 5 + 10 + 5;
    let trend_width = (inner.width as usize / 4)
        .clamp(8, 48)
        .min((inner.width as usize).saturating_sub(reserved_width));
    let prefixes = crate::ui::tree::prefixes(rows);
    let table_rows = rows.iter().zip(prefixes).map(|(row, prefix)| {
        let name = format!(
            "{}{} {}",
            prefix,
            crate::ui::tree::icon(row, app.cpu.tree.expanded.contains(&row.key)),
            row.label
        );
        let mut name = muted_deleted(name);
        if process_tree::system_only(&row.members) {
            name.spans.push(Span::styled(
                " [system]",
                Style::default()
                    .fg(p::text_muted())
                    .add_modifier(Modifier::DIM),
            ));
        }
        let trend = if row.depth == 0 && row.totals.cpu_pct.is_some() {
            let mut values: Vec<_> = (app.scrub_offset..app.scrub_offset + trend_width)
                .map_while(|offset| app.history.cpu_groups.nth_back(offset))
                .map(|point| point.get(app.user_config.cpu_group, &row.group) / 100.0)
                .collect();
            values.reverse();
            sparkline_string(&values, app.graph_style)
        } else {
            String::new()
        };
        Row::new(vec![
            Cell::from(name),
            Cell::from(if row.count == 1 {
                String::new()
            } else {
                row.count.to_string()
            }),
            Cell::from(row.process.map(|p| p.pid.to_string()).unwrap_or_default()),
            Cell::from(
                row.totals
                    .cpu_pct
                    .map(|pct| format!("{pct:.0}%"))
                    .unwrap_or_else(|| "n/a".into()),
            )
            .style(
                Style::default().fg(row
                    .totals
                    .cpu_pct
                    .map(|pct| super::overview::kpi_color(pct, 30.0, 70.0))
                    .unwrap_or_else(p::text_muted)),
            ),
            Cell::from(human_bytes(row.totals.mem_rss)),
            Cell::from(trend),
        ])
    });
    let table = Table::new(
        table_rows,
        [
            Constraint::Min(12),
            Constraint::Length(5),
            Constraint::Length(pid_width as u16),
            Constraint::Length(5),
            Constraint::Length(10),
            Constraint::Length(trend_width as u16),
        ],
    )
    .header(
        Row::new([
            "Application / project",
            "Procs",
            "PID",
            "CPU",
            "Memory",
            "Recent",
        ])
        .style(Style::default().fg(p::text_muted())),
    )
    .block(block)
    .column_spacing(1)
    .row_highlight_style(Style::default().bg(p::selection_bg()).bold())
    .style(Style::default().fg(p::text_primary()).bg(p::bg()));
    // Rows are one line each, below the one-line header. Given a window that
    // already shows the selection, the table draws it unmoved.
    let visible = inner.height.saturating_sub(1) as usize;
    let offset = app.cpu_scroll.follow(selected, rows.len(), visible);
    let mut state = TableState::default()
        .with_offset(offset)
        .with_selected(Some(selected));
    f.render_stateful_widget(table, area, &mut state);
}

fn muted_deleted(text: String) -> Line<'static> {
    if let Some((before, after)) = text.split_once("(deleted)") {
        Line::from(vec![
            Span::raw(before.to_owned()),
            Span::styled("(deleted)", Style::default().fg(p::text_muted())),
            Span::raw(after.to_owned()),
        ])
    } else {
        Line::raw(text)
    }
}

const DELETED_EXPLANATION: &str =
    "(deleted): executable unlinked from disk; process still running.";

pub(crate) fn detail_line_count(selected: Option<&TreeRow<'_>>) -> usize {
    detail_lines(selected).len()
}

fn detail_lines(selected: Option<&TreeRow<'_>>) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    if let Some(row) = selected {
        let system = row
            .members
            .iter()
            .filter(|p| p.scope == crate::collect::ProcessScope::System)
            .count();
        let tools = row
            .members
            .iter()
            .filter(|p| p.scope == crate::collect::ProcessScope::Tool)
            .count();
        let scope = format!(
            "Class: {system} system · {tools} tools · {} unknown",
            row.members.len() - system - tools
        );
        if let Some(process) = row.process {
            lines.push(Line::raw(format!(
                "PID {} · parent {} · user {} · {scope}",
                process.pid, process.ppid, process.user
            )));
            if let Some(owner) = row.owner.filter(|owner| owner.pid != process.pid) {
                lines.push(Line::raw(format!(
                    "Owning tool: {} · PID {}",
                    process_tree::application(owner).1,
                    owner.pid
                )));
            }
            lines.push(Line::raw(format!("Command: {}", process.cmd)));
            lines.push(Line::raw(format!(
                "Working directory: {}",
                process
                    .cwd
                    .as_deref()
                    .map(process_tree::display_path)
                    .unwrap_or_else(|| "unavailable".into())
            )));
            lines.push(Line::raw(
                if process
                    .exe
                    .as_deref()
                    .is_some_and(|exe| exe.ends_with(" (deleted)"))
                {
                    DELETED_EXPLANATION.into()
                } else if let Some(project) = &process.project {
                    format!(
                        "Attributed by working directory · {}/{} · {}",
                        project.host,
                        project.repo,
                        process_tree::display_path(&project.checkout)
                    )
                } else {
                    "No Git project attributed from this process's working directory.".into()
                },
            ));
            lines.push(Line::raw(format!(
                "Executable: {}",
                process
                    .exe
                    .as_deref()
                    .filter(|exe| !exe.is_empty())
                    .map(process_tree::display_path)
                    .unwrap_or_else(|| "unavailable".into())
            )));
        } else {
            lines.push(Line::raw(format!(
                "{} {} · {} processes · {scope}",
                row.icon, row.label, row.count
            )));
            let owners: std::collections::BTreeSet<_> = row
                .members
                .iter()
                .map(|p| {
                    if p.user.is_empty() {
                        "?"
                    } else {
                        p.user.as_str()
                    }
                })
                .collect();
            lines.push(Line::raw(format!(
                "Owners: {} · totals include descendant helpers",
                owners.into_iter().collect::<Vec<_>>().join(", ")
            )));
            match &row.group {
                GroupKey::Docker => {
                    lines.push(Line::raw(
                        "Docker → containers → PIDs · overrides directory grouping",
                    ));
                    if let process_tree::RowKey::Container(id) = &row.key {
                        lines.push(Line::raw(format!("Container ID: {id}")));
                    }
                }
                GroupKey::Directory { path } => {
                    lines.push(Line::raw(format!(
                        "Working directory: {}",
                        process_tree::display_path(path)
                    )));
                    lines.push(Line::raw(
                        "No Git project discovered · expand: tool → helpers / PIDs.",
                    ));
                }
                GroupKey::Project { host, .. } => {
                    lines.push(Line::raw(format!(
                        "Git host: {}",
                        if host.is_empty() {
                            "local (no network origin)"
                        } else {
                            host
                        }
                    )));
                    lines.push(Line::raw(
                        "Process working directory · expand: checkout → tool → helpers / PIDs.",
                    ));
                }
                GroupKey::Application { .. } => {
                    let paths: std::collections::BTreeSet<_> = row
                        .members
                        .iter()
                        .filter_map(|p| p.exe.as_deref().filter(|exe| !exe.is_empty()))
                        .collect();
                    let unavailable = row
                        .members
                        .iter()
                        .filter(|p| p.exe.as_deref().is_none_or(str::is_empty))
                        .count();
                    if paths.is_empty() {
                        lines.push(Line::raw(
                            "grouped by process name; executable path unavailable",
                        ));
                    } else {
                        lines.push(Line::raw(format!(
                            "Executable paths ({} unavailable):",
                            unavailable
                        )));
                    }
                    if row.label.ends_with(" (deleted)") {
                        lines.push(Line::raw(DELETED_EXPLANATION));
                    }
                    lines.extend(
                        paths
                            .into_iter()
                            .map(|path| muted_deleted(process_tree::display_path(path))),
                    );
                    if row.label == "Google Chrome" {
                        lines.push(Line::raw("Use Chrome's Task Manager to identify tabs/extensions; tab titles are not collected."));
                    }
                }
            }
        }
    }
    lines
}

fn draw_detail(f: &mut Frame, area: Rect, selected: Option<&TreeRow<'_>>, scroll: usize) {
    let lines = detail_lines(selected);
    let scroll = scroll
        .min(lines.len().saturating_sub(1))
        .min(u16::MAX as usize) as u16;
    f.render_widget(
        Paragraph::new(lines)
            .scroll((scroll, 0))
            .block(panel(
                "Selected · Enter: expand · PgUp/PgDn: details (c: cores)",
            ))
            .style(Style::default().fg(p::text_muted()).bg(p::bg())),
        area,
    );
}

fn draw_cores(f: &mut Frame, area: Rect, app: &App, snap: &Snapshot) {
    let block = panel(if app.cpu.show_cores {
        "Core activity · PgUp/PgDn: pages · c: collapse"
    } else {
        "Core activity · c: expand"
    });
    let inner = block.inner(area);
    f.render_widget(block, area);
    if app.cpu.show_cores {
        let cols = (inner.width / 14).max(1) as usize;
        let lines: Vec<_> = snap
            .cpu
            .per_core
            .chunks(cols)
            .enumerate()
            .map(|(chunk, values)| {
                Line::raw(
                    values
                        .iter()
                        .enumerate()
                        .map(|(i, v)| format!("c{:<3} {:>5.0}%  ", chunk * cols + i, v))
                        .collect::<String>(),
                )
            })
            .collect();
        let page_height = inner.height.max(1) as usize;
        let max_page = lines.len().saturating_sub(1) / page_height;
        let start = app.cpu.core_page.min(max_page) * page_height;
        f.render_widget(
            Paragraph::new(
                lines
                    .into_iter()
                    .skip(start)
                    .take(page_height)
                    .collect::<Vec<_>>(),
            )
            .style(Style::default().fg(p::text_muted()).bg(p::bg())),
            inner,
        );
        return;
    }
    let busiest = snap.cpu.per_core.iter().copied().fold(0.0f32, f32::max);
    let busy = snap.cpu.per_core.iter().filter(|v| **v >= 85.0).count();
    f.render_widget(
        Paragraph::new(format!(
            "{} logical cores · busiest {:.0}% · {} at ≥85%",
            snap.cpu.per_core.len(),
            busiest,
            busy
        ))
        .style(Style::default().fg(p::text_muted()).bg(p::bg())),
        inner,
    );
}
