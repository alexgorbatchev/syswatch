use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Cell, Paragraph, Row, Table, TableState},
    Frame,
};

use crate::app::{App, Snapshot, TreeView};
use crate::process_tree::{self, TreeGroup, TreeRow};
use crate::ui::{
    graph::GraphStyle,
    palette as p,
    widgets::{block_bar_styled, human_bytes, mem_pct, panel},
};

pub fn draw(f: &mut Frame, area: Rect, app: &App, snap: &Snapshot) {
    // Drop the SWAP panel entirely when no swap is configured — those 7 rows
    // go to the process list instead (issue #12).
    let has_swap = snap.mem.swap_total_bytes > 0;
    // The filter strip costs a process row, so it only appears once there
    // is a filter to show or edit — same reasoning as issue #12. The key
    // itself is advertised in the footer and help popup (issue #20).
    let show_filter = app.filter_input || app.filter_active.is_some();

    let mut constraints = vec![Constraint::Length(7)];
    if has_swap {
        constraints.push(Constraint::Length(7));
    }
    if show_filter {
        constraints.push(Constraint::Length(1));
    }
    constraints.push(Constraint::Min(0));

    let v = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    let (groups, rows) = app.tree_rows(TreeView::Memory, snap);
    let mut row = 0;
    draw_ram_bar(f, v[row], snap, app.graph_style);
    row += 1;
    if has_swap {
        draw_swap(f, v[row], snap, app.graph_style);
        row += 1;
    }
    if show_filter {
        draw_filter_strip(f, v[row], app, snap, &groups);
        row += 1;
    }
    draw_proc_breakdown(f, v[row], app, snap, &rows);
}

/// Filter prompt while typing; kept groups and match count once applied.
fn draw_filter_strip(
    f: &mut Frame,
    area: Rect,
    app: &App,
    snap: &Snapshot,
    groups: &[TreeGroup<'_>],
) {
    let line = if app.filter_input {
        crate::ui::widgets::filter_input_line(&app.filter_buf)
    } else {
        let filter = app.filter_active.as_deref().unwrap_or_default();
        let needle = filter.to_lowercase();
        let matching = snap
            .procs
            .iter()
            .filter(|p| process_tree::matches_filter(p, &needle))
            .count();
        let members: usize = groups.iter().map(|g| g.members.len()).sum();
        Line::from(Span::styled(
            format!(
                " {} groups ({members} procs) · {matching}/{} match \"{filter}\"   / f:edit",
                groups.len(),
                snap.procs.len(),
            ),
            Style::default().fg(p::brand()),
        ))
    };
    f.render_widget(
        Paragraph::new(line).style(Style::default().bg(p::bg())),
        area,
    );
}

fn draw_ram_bar(f: &mut Frame, area: Rect, snap: &Snapshot, style: GraphStyle) {
    let block = panel("RAM");
    let inner = block.inner(area);
    f.render_widget(block, area);

    let total = snap.mem.total_bytes.max(1);
    let used = snap.mem.used_bytes;
    let avail = snap.mem.available_bytes;
    let pct = used as f32 / total as f32;
    let color = if pct >= 0.9 {
        p::status_error()
    } else if pct >= 0.7 {
        p::status_warn()
    } else {
        p::status_good()
    };

    let mut header_spans = vec![
        Span::styled("used ", Style::default().fg(p::text_muted())),
        Span::styled(
            human_bytes(used),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" / ", Style::default().fg(p::text_muted())),
        Span::styled(human_bytes(total), Style::default().fg(p::text_primary())),
        Span::styled(
            format!("   ({:>4.1}%)", pct * 100.0),
            Style::default().fg(p::text_muted()),
        ),
        Span::styled("    available ", Style::default().fg(p::text_muted())),
        Span::styled(human_bytes(avail), Style::default().fg(p::text_primary())),
    ];
    // PSI: stall time is the honest pressure signal — %used can be
    // high while nothing is starved, and vice versa.
    if let Some(psi) = &snap.pressure {
        let stall_color = if psi.mem_full >= 5.0 {
            p::status_error()
        } else if psi.mem_some >= 10.0 {
            p::status_warn()
        } else {
            p::text_primary()
        };
        header_spans.push(Span::styled(
            "    stall ",
            Style::default().fg(p::text_muted()),
        ));
        header_spans.push(Span::styled(
            format!("{:.1}% some / {:.1}% full", psi.mem_some, psi.mem_full),
            Style::default().fg(stall_color),
        ));
    }
    let header = Line::from(header_spans);
    let bar = block_bar_styled(pct, inner.width, color, style);
    f.render_widget(
        Paragraph::new(vec![header, Line::from(""), bar]).style(Style::default().bg(p::bg())),
        inner,
    );
}

fn draw_swap(f: &mut Frame, area: Rect, snap: &Snapshot, style: GraphStyle) {
    let block = panel("Swap");
    let inner = block.inner(area);
    f.render_widget(block, area);

    let total = snap.mem.swap_total_bytes;
    let used = snap.mem.swap_used_bytes;
    let pct = if total > 0 {
        used as f32 / total as f32
    } else {
        0.0
    };
    let color = if pct >= 0.75 {
        p::status_error()
    } else if pct >= 0.25 {
        p::status_warn()
    } else {
        p::status_good()
    };

    let header = if total == 0 {
        Line::from(vec![Span::styled(
            "no swap configured",
            Style::default().fg(p::text_muted()),
        )])
    } else {
        Line::from(vec![
            Span::styled("used ", Style::default().fg(p::text_muted())),
            Span::styled(
                human_bytes(used),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(" / ", Style::default().fg(p::text_muted())),
            Span::styled(human_bytes(total), Style::default().fg(p::text_primary())),
            Span::styled(
                format!("   ({:>4.1}%)", pct * 100.0),
                Style::default().fg(p::text_muted()),
            ),
        ])
    };
    let bar = block_bar_styled(pct, inner.width, color, style);
    f.render_widget(
        Paragraph::new(vec![header, Line::from(""), bar]).style(Style::default().bg(p::bg())),
        inner,
    );
}

/// Per-process memory breakdown over the shared process tree. RSS
/// double-counts shared pages, so where the platform sampler delivered honest
/// accounting we lead with it: phys_footprint on macOS (Activity Monitor's
/// Memory column), PSS + private/swap from smaps_rollup on Linux. Detail is
/// sampled for the top procs by RSS and only for procs the user can inspect
/// without sudo — rows without it show "—" and rank by RSS instead.
///
/// Groups sum footprint, PSS, private and swap over the members that report
/// them. Shared pages, peaks and virtual sizes overlap between processes, so
/// only process rows show those.
fn draw_proc_breakdown(
    f: &mut Frame,
    area: Rect,
    app: &App,
    snap: &Snapshot,
    rows: &[TreeRow<'_>],
) {
    let total_mem = snap.mem.total_bytes.max(1);

    // Column sets keyed off what the sampler could actually read this
    // session — same pattern as the procs tab's NET / GPU columns.
    let show_footprint = snap.procs.iter().any(|p| p.mem_footprint.is_some());
    let show_smaps = snap.procs.iter().any(|p| p.mem_pss.is_some());
    let show_peak = snap.procs.iter().any(|p| p.mem_peak.is_some());
    let show_virt = !show_footprint && !show_smaps;

    let block = panel(format!(
        "{} · {} · system last",
        if show_footprint {
            "Process memory (footprint = real pressure; rss double-counts shared)"
        } else if show_smaps {
            "Process memory (pss sums to real use; private frees on exit)"
        } else {
            "Process memory (by RSS)"
        },
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

    let pid_width = rows
        .iter()
        .filter_map(|row| row.process.map(|p| p.pid.to_string().len()))
        .max()
        .unwrap_or(0)
        .max(3) as u16;
    let mut header = vec!["Application / project", "Procs", "PID", "%MEM"];
    let mut widths = vec![
        Constraint::Min(16),
        Constraint::Length(5),
        Constraint::Length(pid_width),
        Constraint::Length(5),
    ];
    if show_footprint {
        header.push("FOOTPRNT");
    }
    if show_smaps {
        header.extend(["PSS", "PRIVATE", "SHARED", "SWAP"]);
    }
    if show_peak {
        header.push("PEAK");
    }
    header.push("RSS");
    if show_virt {
        header.push("VIRT");
    }
    widths.resize(header.len(), Constraint::Length(10));

    // None → "—": the proc exists but detail wasn't readable (other
    // user without sudo) or it ranked below the sampler's top-N cap.
    let detail = |v: Option<u64>| {
        Cell::from(v.map(human_bytes).unwrap_or_else(|| "—".into())).style(Style::default().fg(
            if v.is_some() {
                p::brand()
            } else {
                p::text_muted()
            },
        ))
    };
    // Non-additive measures: blank for groups rather than a misleading sum.
    let leaf = |row: &TreeRow<'_>, v: fn(&crate::collect::ProcTick) -> Option<u64>| {
        row.process.map_or_else(|| Cell::from(""), |p| detail(v(p)))
    };

    let prefixes = crate::ui::tree::prefixes(rows);
    let table_rows = rows.iter().zip(prefixes).map(|(row, prefix)| {
        let totals = &row.totals;
        let mut name = Line::raw(format!(
            "{prefix}{} {}",
            crate::ui::tree::icon(row, app.memory_tree.expanded.contains(&row.key)),
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
            Cell::from(row.process.map(|p| p.pid.to_string()).unwrap_or_default()),
            Cell::from(format!("{:.1}", mem_pct(totals.mem_rss, total_mem))),
        ];
        if show_footprint {
            cells.push(detail(totals.mem_footprint));
        }
        if show_smaps {
            cells.push(detail(totals.mem_pss));
            cells.push(detail(totals.mem_private));
            cells.push(leaf(row, |p| p.mem_shared));
            cells.push(detail(totals.mem_swap));
        }
        if show_peak {
            cells.push(leaf(row, |p| p.mem_peak));
        }
        cells.push(
            Cell::from(human_bytes(totals.mem_rss)).style(Style::default().fg(if show_virt {
                p::brand()
            } else {
                p::text_muted()
            })),
        );
        if show_virt {
            cells.push(
                Cell::from(
                    row.process
                        .map(|p| human_bytes(p.mem_virt))
                        .unwrap_or_default(),
                )
                .style(Style::default().fg(p::text_muted())),
            );
        }
        Row::new(cells)
    });
    let selected = app.memory_tree.selected_index(rows);
    let table = Table::new(table_rows, widths)
        .header(
            Row::new(header).style(
                Style::default()
                    .fg(p::text_muted())
                    .add_modifier(Modifier::BOLD),
            ),
        )
        .block(block)
        .column_spacing(1)
        .row_highlight_style(Style::default().bg(p::selection_bg()).bold())
        .style(Style::default().fg(p::text_primary()).bg(p::bg()));
    let visible = inner.height.saturating_sub(1) as usize;
    let offset = app.memory_scroll.follow(selected, rows.len(), visible);
    let mut state = TableState::default()
        .with_offset(offset)
        .with_selected(Some(selected));
    f.render_stateful_widget(table, area, &mut state);
}

#[cfg(test)]
#[path = "memory_tests.rs"]
mod tests;
