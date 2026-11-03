use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Cell, Paragraph, Row, Table, TableState},
    Frame,
};

use crate::app::{App, ProcSort, Snapshot, TreeView};
use crate::process_tree::{self, TreeRow};
use crate::ui::{
    palette as p,
    widgets::{human_bytes, human_rate, mem_pct, panel},
};

pub fn draw(f: &mut Frame, area: Rect, app: &App, snap: &Snapshot) {
    // The drill-in pane is toggleable (`d`); hidden, its 9 rows go to the
    // process list (issue #13).
    let mut constraints = vec![
        Constraint::Length(1), // sort strip / filter input
        Constraint::Min(0),    // process table
    ];
    if app.proc_show_detail {
        constraints.push(Constraint::Length(9)); // drill-in
    }
    let v = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    let (groups, rows) = app.tree_rows(TreeView::Procs, snap);
    let selected = app.procs_tree.selected_index(&rows);
    let members: usize = groups.iter().map(|g| g.members.len()).sum();
    draw_sort_strip(f, v[0], app, snap, groups.len(), members);
    draw_table(f, v[1], app, snap, &rows, selected);
    if app.proc_show_detail {
        draw_drill_in(f, v[2], snap, rows.get(selected));
    }
}

fn draw_sort_strip(
    f: &mut Frame,
    area: Rect,
    app: &App,
    snap: &Snapshot,
    groups: usize,
    members: usize,
) {
    // While typing into the filter, the strip becomes a single-line
    // input box — no sort chips, just the prompt and a cursor.
    if app.filter_input {
        f.render_widget(
            Paragraph::new(crate::ui::widgets::filter_input_line(&app.filter_buf))
                .style(Style::default().bg(p::bg())),
            area,
        );
        return;
    }

    let mut spans: Vec<Span> = Vec::new();
    spans.push(Span::styled(" sort ", Style::default().fg(p::text_muted())));
    for s in ProcSort::ALL.iter() {
        let active = *s == app.proc_sort;
        let label = format!(" {} ", s.label());
        if active {
            spans.push(Span::styled(
                label,
                Style::default()
                    .fg(p::brand())
                    .bg(p::selection_bg())
                    .add_modifier(Modifier::BOLD),
            ));
            spans.push(Span::styled("\u{25BC} ", Style::default().fg(p::brand())));
        } else {
            spans.push(Span::styled(label, Style::default().fg(p::text_primary())));
            spans.push(Span::raw(" "));
        }
    }
    let keys = "s:sort  b:group  d:detail  ↑↓:select  ↵:expand";
    // A filter keeps whole groups, so report both the groups it kept and
    // how many of their processes actually match.
    let count_text = if let Some(f) = app.filter_active.as_deref() {
        let needle = f.to_lowercase();
        let matching = snap
            .procs
            .iter()
            .filter(|p| process_tree::matches_filter(p, &needle))
            .count();
        format!(
            "    {groups} groups ({members} procs) · {matching}/{} match \"{f}\"   / f:edit  {keys}",
            snap.procs.len(),
        )
    } else {
        format!("    {} procs   / f:filter  {keys}", snap.procs.len())
    };
    spans.push(Span::styled(
        count_text,
        Style::default().fg(p::text_muted()),
    ));
    f.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::default().bg(p::bg())),
        area,
    );
}

fn draw_table(
    f: &mut Frame,
    area: Rect,
    app: &App,
    snap: &Snapshot,
    rows: &[TreeRow<'_>],
    selected: usize,
) {
    let block = panel(format!(
        "PROCESSES · {} · system last",
        app.user_config.cpu_group.label()
    ));
    let inner = block.inner(area);
    if rows.is_empty() {
        f.render_widget(
            Paragraph::new("No matching processes.")
                .block(block)
                .style(Style::default().fg(p::text_muted()).bg(p::bg())),
            area,
        );
        return;
    }

    // Show NET / GPU columns only when at least one proc has data —
    // saves columns on platforms where the source isn't available.
    let show_net = snap.procs.iter().any(|p| p.net_rx_rate.is_some());
    let show_gpu = snap
        .procs
        .iter()
        .any(|p| p.gpu_pct.is_some() || p.gpu_mem_bytes.is_some());
    // VIRT only when neither extras are shown — keeps the row from
    // sprawling past 120 cols when both NET and GPU are populated.
    let show_virt = !show_net && !show_gpu;
    let total_mem = snap.mem.total_bytes.max(1);
    let pid_width = rows
        .iter()
        .filter_map(|row| row.process.map(|p| p.pid.to_string().len()))
        .max()
        .unwrap_or(0)
        .max(3) as u16;

    let mut header = vec!["Application / project", "Procs", "PID", "User", "CPU"];
    let mut widths = vec![
        Constraint::Min(16),
        Constraint::Length(5),
        Constraint::Length(pid_width),
        Constraint::Length(10),
        Constraint::Length(4),
        Constraint::Length(CPU_SPARK_W as u16),
        Constraint::Length(5),
    ];
    header.extend(["CPU hist", "%MEM"]);
    if show_virt {
        header.push("VIRT");
        widths.push(Constraint::Length(9));
    }
    header.extend(["State", "R/s", "W/s"]);
    widths.extend([
        Constraint::Length(5),
        Constraint::Length(10),
        Constraint::Length(10),
    ]);
    if show_net {
        // `~` marks the connection-count estimate; measured rates
        // (nettop on macOS) render unmarked.
        header.extend(if snap.net_rates_estimated {
            ["~NET ↓/s", "~NET ↑/s"]
        } else {
            ["NET ↓/s", "NET ↑/s"]
        });
        widths.extend([Constraint::Length(10), Constraint::Length(10)]);
    }
    if show_gpu {
        header.extend(["%GPU", "GPU MEM"]);
        widths.extend([Constraint::Length(5), Constraint::Length(9)]);
    }

    let prefixes = crate::ui::tree::prefixes(rows);
    let table_rows = rows.iter().zip(prefixes).map(|(row, prefix)| {
        let totals = &row.totals;
        let process = row.process;
        let rate = |value: f64, active| {
            Cell::from(human_rate(value)).style(Style::default().fg(if value > 0.0 {
                active
            } else {
                p::text_muted()
            }))
        };
        let mut name = Line::raw(format!(
            "{prefix}{} {}",
            crate::ui::tree::icon(row, app.procs_tree.expanded.contains(&row.key)),
            row.label
        ));
        if process_tree::system_only(&row.members) {
            name.spans.push(Span::styled(
                " [system]",
                Style::default()
                    .fg(p::text_muted())
                    .add_modifier(Modifier::DIM),
            ));
        }
        let mut cells = vec![
            Cell::from(name),
            Cell::from(if row.count == 1 {
                String::new()
            } else {
                row.count.to_string()
            }),
            Cell::from(process.map(|p| p.pid.to_string()).unwrap_or_default()),
            Cell::from(owners(row)).style(Style::default().fg(p::text_muted())),
            cpu_cell(totals.cpu_pct),
            Cell::from(cpu_history(app, row)).style(Style::default().fg(p::brand())),
            Cell::from(format!("{:.1}", mem_pct(totals.mem_rss, total_mem))),
        ];
        if show_virt {
            // Virtual sizes overlap between processes; only leaves show one.
            cells.push(
                Cell::from(process.map(|p| human_bytes(p.mem_virt)).unwrap_or_default())
                    .style(Style::default().fg(p::text_muted())),
            );
        }
        cells.push(match process {
            Some(p_) => Cell::from(p_.state.to_string()).style(
                Style::default()
                    .fg(state_color(p_.state))
                    .add_modifier(Modifier::BOLD),
            ),
            None => Cell::from(""),
        });
        cells.push(rate(totals.io_read_rate, p::brand()));
        cells.push(rate(totals.io_write_rate, p::brand()));
        if show_net {
            cells.push(rate(totals.net_rx_rate.unwrap_or(0.0), p::status_good()));
            cells.push(rate(totals.net_tx_rate.unwrap_or(0.0), p::tx_rate()));
        }
        if show_gpu {
            let pct = totals.gpu_pct.unwrap_or(0.0);
            cells.push(Cell::from(format!("{pct:.0}%")).style(Style::default().fg(
                if pct >= 30.0 {
                    p::status_warn()
                } else if pct > 0.0 {
                    p::brand()
                } else {
                    p::text_muted()
                },
            )));
            cells.push(
                Cell::from(match totals.gpu_mem_bytes {
                    Some(b) if b > 0 => human_bytes(b),
                    _ => "—".into(),
                })
                .style(Style::default().fg(if totals.gpu_mem_bytes.is_some() {
                    p::brand()
                } else {
                    p::text_muted()
                })),
            );
        }
        Row::new(cells)
    });

    let table = Table::new(table_rows, widths)
        .header(Row::new(header).style(header_style()))
        .block(block)
        .column_spacing(1)
        .row_highlight_style(Style::default().bg(p::selection_bg()).bold())
        .style(Style::default().fg(p::text_primary()).bg(p::bg()));
    // Rows are one line each, below the one-line header.
    let visible = inner.height.saturating_sub(1) as usize;
    let offset = app.proc_scroll.follow(selected, rows.len(), visible);
    let mut state = TableState::default()
        .with_offset(offset)
        .with_selected(Some(selected));
    f.render_stateful_widget(table, area, &mut state);
}

/// Whole-machine CPU without decimals, coloured like the CPU tab.
fn cpu_cell(pct: Option<f32>) -> Cell<'static> {
    Cell::from(
        pct.map(|pct| format!("{pct:.0}%"))
            .unwrap_or_else(|| "n/a".into()),
    )
    .style(
        Style::default().fg(pct
            .map(|pct| super::overview::kpi_color(pct, 30.0, 70.0))
            .unwrap_or_else(p::text_muted)),
    )
}

/// The single owner of a row's processes, or how many owners share it.
fn owners(row: &TreeRow<'_>) -> String {
    let owners: std::collections::BTreeSet<_> = row.members.iter().map(|p| &p.user).collect();
    match owners.len() {
        1 => owners.into_iter().next().cloned().unwrap_or_default(),
        n => format!("{n} users"),
    }
}

fn state_color(state: char) -> ratatui::style::Color {
    match state {
        'R' => p::status_good(),
        'S' | 'I' => p::text_primary(),
        'Z' => p::status_error(),
        _ => p::text_muted(),
    }
}

/// Top-level groups chart their recorded share of the machine; process rows
/// chart their own samples. Intermediate rows have no recorded history.
fn cpu_history(app: &App, row: &TreeRow<'_>) -> String {
    if let Some(process) = row.process {
        let samples = app
            .history
            .proc_cpu_history
            .get(&process.pid)
            .map(|r| r.to_vec())
            .unwrap_or_default();
        return spark_cell(&samples, app.graph_style);
    }
    if row.depth > 0 {
        return String::new();
    }
    let mut samples: Vec<_> = (app.scrub_offset..app.scrub_offset + CPU_SPARK_W)
        .map_while(|offset| app.history.cpu_groups.nth_back(offset))
        .map(|point| point.get(app.user_config.cpu_group, &row.group) / 100.0)
        .collect();
    samples.reverse();
    spark_cell(&samples, app.graph_style)
}

fn draw_drill_in(f: &mut Frame, area: Rect, snap: &Snapshot, row: Option<&TreeRow<'_>>) {
    let Some(row) = row else {
        let block = panel("DRILL-IN");
        f.render_widget(block, area);
        return;
    };
    let total_mem = snap.mem.total_bytes.max(1);
    let totals = &row.totals;
    let title = match row.process {
        Some(p_) => format!("{}  pid {}  -  drill-in", p_.name, p_.pid),
        None => format!("{}  {} processes  -  drill-in", row.label, row.count),
    };
    let block = panel(&title);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let cpu_line = format!(
        "{}{}",
        totals
            .cpu_pct
            .map(|pct| format!("{pct:.0}% of machine"))
            .unwrap_or_else(|| "n/a".into()),
        totals
            .power_w
            .map(|w| format!("   ~{:.2} W", w))
            .unwrap_or_default()
    );
    let io_total = totals.io_read_rate + totals.io_write_rate;
    let net_line = match (totals.net_rx_rate, totals.net_tx_rate) {
        (None, None) => "—".to_string(),
        (rx, tx) => format!(
            "↓ {} / ↑ {}",
            human_rate(rx.unwrap_or(0.0)),
            human_rate(tx.unwrap_or(0.0))
        ),
    };
    let io = kv(
        "io r/w",
        format!(
            "read {} / write {}",
            human_rate(totals.io_read_rate),
            human_rate(totals.io_write_rate)
        ),
        if io_total > 0.0 {
            p::brand()
        } else {
            p::text_muted()
        },
    );
    let lines = if let Some(p_) = row.process {
        let cmd = if p_.cmd.is_empty() {
            p_.name.clone()
        } else {
            p_.cmd.clone()
        };
        let started = p_
            .start_time
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| {
                chrono::DateTime::<chrono::Local>::from(
                    std::time::UNIX_EPOCH + std::time::Duration::from_secs(d.as_secs()),
                )
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
            })
            .unwrap_or_else(|| "?".into());
        let owner = row
            .owner
            .filter(|owner| owner.pid != p_.pid)
            .map(|owner| {
                format!(
                    "   tool {} ({})",
                    process_tree::application(owner).1,
                    owner.pid
                )
            })
            .unwrap_or_default();
        let user_line = format!(
            "{}   ppid {}{}{owner}",
            p_.user,
            p_.ppid,
            p_.threads
                .map(|t| format!("   threads {}", t))
                .unwrap_or_default()
        );
        let mem_line = format!(
            "{:.1}%  ({} rss / {} virt){}",
            mem_pct(p_.mem_rss, total_mem),
            human_bytes(p_.mem_rss),
            human_bytes(p_.mem_virt),
            p_.mem_peak
                .map(|b| format!("   peak {}", human_bytes(b)))
                .unwrap_or_default()
        );
        vec![
            kv("cmd", cmd, p::text_primary()),
            kv("user", user_line, p::text_primary()),
            kv("mem", mem_line, p::text_primary()),
            kv("cpu", cpu_line, p::text_primary()),
            io,
            kv("net", net_line, p::text_primary()),
            kv("started", started, p::text_muted()),
        ]
    } else {
        let mut users: Vec<_> = row.members.iter().map(|p| p.user.as_str()).collect();
        users.sort_unstable();
        users.dedup();
        vec![
            kv("owners", users.join(", "), p::text_primary()),
            kv(
                "mem",
                format!(
                    "{:.1}%  ({} summed rss; shared pages count in every process)",
                    mem_pct(totals.mem_rss, total_mem),
                    human_bytes(totals.mem_rss)
                ),
                p::text_primary(),
            ),
            kv("cpu", cpu_line, p::text_primary()),
            io,
            kv("net", net_line, p::text_primary()),
            kv(
                "expand",
                "↵ shows checkouts, tools, helpers and PIDs".into(),
                p::text_muted(),
            ),
        ]
    };
    f.render_widget(
        Paragraph::new(lines).style(Style::default().bg(p::bg())),
        inner,
    );
}

fn kv(k: &str, v: String, val_color: ratatui::style::Color) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{:<11} ", k), Style::default().fg(p::text_muted())),
        Span::styled(v, Style::default().fg(val_color)),
    ])
}

/// Width of the `CPU hist` column.
const CPU_SPARK_W: usize = 8;

/// Render the `CPU hist` cell: exactly `CPU_SPARK_W` chars, newest sample
/// last, left-padded while history is still filling. History is kept
/// `PROC_CPU_SPARK_LEN` deep for the Dense view, so only the most recent
/// samples fit here.
fn spark_cell(samples: &[f32], style: crate::ui::graph::GraphStyle) -> String {
    let tail = &samples[samples.len().saturating_sub(CPU_SPARK_W)..];
    let glyphs = crate::ui::widgets::sparkline_string(tail, style);
    format!("{}{}", " ".repeat(CPU_SPARK_W - tail.len()), glyphs)
}

fn header_style() -> Style {
    Style::default()
        .fg(p::text_muted())
        .add_modifier(Modifier::BOLD)
}

#[cfg(test)]
#[path = "procs_tests.rs"]
mod tests;
