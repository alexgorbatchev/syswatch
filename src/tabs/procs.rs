use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use crate::app::{App, ProcSort, Snapshot, TreeView};
use crate::process_tree::{self, TreeRow};
use crate::ui::{
    palette as p,
    tree::{Cell, Column, TreeTable},
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
        app.user_config.process_group.label()
    ));
    let inner = block.inner(area);
    f.render_widget(block, area);

    // Show NET / GPU columns only when at least one proc has data —
    // saves columns on platforms where the source isn't available.
    let show_net = snap.procs.iter().any(|p| p.net_rx_rate.is_some());
    let show_gpu = snap
        .procs
        .iter()
        .any(|p| p.gpu_pct.is_some() || p.gpu_mem_bytes.is_some());
    let sort = app.proc_sort;
    let mut columns = vec![
        Column::new("USER", 10, |row, tone| Cell::Left(owners(row), tone.muted)).left(),
        Column::cpu().sorted(sort == ProcSort::Cpu),
        Column::memory().sorted(sort == ProcSort::Rss),
        Column::recent(app, app.user_config.process_group),
    ];
    // VIRT only when neither extra is shown. Virtual sizes overlap between
    // processes, so only process rows carry one.
    if !show_net && !show_gpu {
        columns.push(Column::new("VIRT", 9, |row, tone| {
            row.process
                .map_or(Cell::Blank, |p| tone.bytes(Some(p.mem_virt)))
        }));
    }
    columns.push(Column::new("STATE", 5, |row, tone| {
        row.process.map_or(Cell::Blank, |p| {
            Cell::Text(
                p.state.to_string(),
                match p.state {
                    'R' => tone.theme.status_good,
                    'Z' => tone.theme.status_error,
                    _ => tone.muted,
                },
            )
        })
    }));
    let io = sort == ProcSort::Io;
    columns.extend([
        Column::new("R/s", 10, |row, tone| {
            tone.rate(row.totals.io_read_rate, tone.ramps.primary.mid())
        })
        .sorted(io),
        Column::new("W/s", 10, |row, tone| {
            tone.rate(row.totals.io_write_rate, tone.ramps.secondary.mid())
        })
        .sorted(io),
    ]);
    if show_net {
        // `~` marks the connection-count estimate; measured rates
        // (nettop on macOS) render unmarked.
        let (rx, tx) = if snap.net_rates_estimated {
            ("~NET ↓/s", "~NET ↑/s")
        } else {
            ("NET ↓/s", "NET ↑/s")
        };
        let net = sort == ProcSort::Net;
        columns.extend([
            Column::new(rx, 10, |row, tone| {
                tone.rate(
                    row.totals.net_rx_rate.unwrap_or(0.0),
                    tone.ramps.primary.mid(),
                )
            })
            .sorted(net),
            Column::new(tx, 10, |row, tone| {
                tone.rate(
                    row.totals.net_tx_rate.unwrap_or(0.0),
                    tone.ramps.secondary.mid(),
                )
            })
            .sorted(net),
        ]);
    }
    if show_gpu {
        columns.extend([
            Column::new("GPU", 5, |row, tone| match row.totals.gpu_pct {
                Some(pct) => Cell::Text(format!("{pct:.0}%"), tone.usage(pct)),
                None => Cell::Text("—".into(), tone.muted),
            })
            .sorted(sort == ProcSort::Gpu),
            Column::new("GPU MEM", 9, |row, tone| {
                tone.bytes(row.totals.gpu_mem_bytes)
            }),
        ]);
    }
    f.render_widget(
        TreeTable::new(rows, "APPLICATION / PROJECT")
            .expanded(&app.procs_tree.expanded)
            .columns(columns)
            .selection(selected, &app.proc_scroll)
            .empty("No matching processes."),
        inner,
    );
}

/// The single owner of a row's processes, or how many owners share it.
fn owners(row: &TreeRow<'_>) -> String {
    let owners: std::collections::BTreeSet<_> = row.members.iter().map(|p| &p.user).collect();
    match owners.len() {
        1 => owners.into_iter().next().cloned().unwrap_or_default(),
        n => format!("{n} users"),
    }
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

#[cfg(test)]
#[path = "procs_tests.rs"]
mod tests;
