//! Dense's project tree, backed by the same rows as Full's CPU panel.

use std::collections::BTreeSet;

use super::{
    paint, short_cmd, truncate, App, Buffer, Color, Layout, Modifier, Ramps, Rect, Series,
    Snapshot, Style, Theme,
};
use crate::cpu::{self, CpuRow, GroupMode};
use crate::ui::widgets::human_bytes;
use ratatui::text::{Line, Span};

struct Columns {
    name: u16,
    name_width: u16,
    procs: u16,
    pid: u16,
    pid_width: u16,
    cpu: u16,
    mem: u16,
    spark: u16,
    spark_width: u16,
}

impl Columns {
    fn new(inner: Rect, rows: &[CpuRow<'_>]) -> Self {
        let pid_width = rows
            .iter()
            .filter_map(|row| row.process.map(|proc| proc.pid.to_string().len()))
            .max()
            .unwrap_or(3)
            .max(3) as u16;
        let width = inner.width.saturating_sub(4);
        let fixed = 5 + pid_width + 5 + 10 + 5;
        let spark_width = (width / 4).min(28).min(width.saturating_sub(fixed + 18));
        let name = inner.x + 3;
        let name_width = width.saturating_sub(fixed + spark_width);
        let procs = name + name_width + 1;
        let pid = procs + 6;
        let cpu = pid + pid_width + 1;
        let mem = cpu + 6;
        let spark = mem + 11;
        Self {
            name,
            name_width,
            procs,
            pid,
            pid_width,
            cpu,
            mem,
            spark,
            spark_width,
        }
    }
}

pub(super) fn draw(
    buf: &mut Buffer,
    layout: &Layout,
    theme: &Theme,
    ramps: &Ramps,
    app: &App,
    snap: &Snapshot,
) {
    if layout.procs.width == 0 || layout.procs.height == 0 {
        return;
    }
    let groups = cpu::groups(snap, GroupMode::Project);
    let rows = cpu::rows(snap, GroupMode::Project, &groups, &app.dense.tree);
    let prefixes = crate::ui::tree::prefixes(&rows);
    let selected = cpu::selected_index(&rows, &app.dense.tree);
    let visible = layout.visible_procs().max(1);
    let first = app.dense.scroll.follow(selected, rows.len(), visible);
    let last = (first + visible).min(rows.len());
    let range = format!(
        "{}-{last} of {}",
        if rows.is_empty() { 0 } else { first + 1 },
        rows.len()
    );
    let compact = layout.procs.width < 100;
    let sub = if compact {
        format!("{} PIDs", snap.procs.len())
    } else {
        format!("{} groups · {} PIDs", groups.len(), snap.procs.len())
    };
    let binds: &[paint::Bind] = &[
        ("q", "uit"),
        ("↑↓", " select"),
        ("↵", " expand"),
        ("V", " view"),
        (",", " menu"),
        if layout.zoom.is_some() {
            ("esc", " back")
        } else {
            ("1-6", " zoom")
        },
        ("?", " keys"),
    ];
    let inner = paint::panel(
        buf,
        layout.procs,
        theme,
        &paint::PanelOpts {
            key: Some(if layout.shape == super::Shape::Compact {
                "3"
            } else {
                "6"
            }),
            title: Some("procs"),
            sub: Some(&sub),
            right: Some(if compact {
                "project · system last"
            } else {
                "project · system last · ↓ cpu"
            }),
            foot_left: binds,
            foot_right: Some(&range),
            ..Default::default()
        },
    );
    if inner.height < 2 || inner.width < 40 {
        return;
    }
    let cols = Columns::new(inner, &rows);
    let mut y = inner.y;
    if inner.height >= 4 {
        if let Some(row) = rows.get(selected) {
            details(buf, inner, y, theme, row);
        }
        y += 2;
    }
    let header = Style::default().fg(theme.border);
    paint::put(
        buf,
        cols.name,
        y,
        &truncate("APPLICATION / PROJECT", cols.name_width as usize),
        header,
    );
    paint::put_right(buf, cols.procs + 4, y, "PROCS", header);
    paint::put_right(buf, cols.pid + cols.pid_width - 1, y, "PID", header);
    paint::put_right(
        buf,
        cols.cpu + 4,
        y,
        "CPU",
        Style::default()
            .fg(theme.brand)
            .add_modifier(Modifier::BOLD),
    );
    paint::put_right(buf, cols.mem + 9, y, "MEM", header);
    if cols.spark_width >= 6 {
        paint::put(buf, cols.spark, y, "RECENT", header);
    }
    y += 1;
    for (index, row) in rows.iter().enumerate().take(last).skip(first) {
        if y >= inner.bottom() {
            break;
        }
        let is_selected = index == selected;
        let muted = if is_selected {
            theme.text_secondary
        } else {
            theme.text_muted
        };
        if is_selected {
            for x in inner.x..inner.right() {
                if let Some(cell) = buf.cell_mut((x, y)) {
                    cell.set_bg(theme.selection_bg);
                }
            }
            paint::put(buf, inner.x + 1, y, "▶", Style::default().fg(theme.brand));
        }
        let mut name = Line::from(Span::raw(format!(
            "{}{} ",
            prefixes[index],
            crate::ui::tree::icon(row, app.dense.tree.expanded.contains(&row.key))
        )))
        .style(Style::default().fg(label_color(row.cpu_pct, theme, muted)));
        if let (cpu::RowKey::Container(_), Some((label, id))) =
            (&row.key, row.label.split_once(' '))
        {
            name.spans.push(Span::raw(format!("{label} ")));
            name.spans
                .push(Span::styled(id, Style::default().fg(muted)));
        } else {
            name.spans.push(Span::raw(&row.label));
        }
        if cpu::system_only(&row.members) {
            name.spans.push(Span::styled(
                " [system]",
                Style::default().fg(muted).add_modifier(Modifier::DIM),
            ));
        }
        buf.set_line(cols.name, y, &name, cols.name_width);
        if row.count > 1 {
            paint::put_right(
                buf,
                cols.procs + 4,
                y,
                &row.count.to_string(),
                Style::default().fg(muted),
            );
        }
        if let Some(proc) = row.process {
            paint::put_right(
                buf,
                cols.pid + cols.pid_width - 1,
                y,
                &proc.pid.to_string(),
                Style::default().fg(muted),
            );
        }
        let cpu = row
            .cpu_pct
            .map(|pct| format!("{pct:.0}%"))
            .unwrap_or_else(|| "n/a".into());
        paint::put_right(
            buf,
            cols.cpu + 4,
            y,
            &cpu,
            Style::default().fg(cpu_color(row.cpu_pct, theme, muted)),
        );
        paint::put_right(
            buf,
            cols.mem + 9,
            y,
            &human_bytes(row.mem_rss),
            Style::default().fg(if row.mem_rss < 1024 * 1024 * 1024 {
                muted
            } else {
                ramps.secondary.mid()
            }),
        );
        if cols.spark_width >= 6 && row.depth == 0 && row.cpu_pct.is_some() {
            let mut values: Vec<u64> = (app.scrub_offset
                ..app.scrub_offset + cols.spark_width as usize)
                .map_while(|offset| app.history.cpu_groups.nth_back(offset))
                .map(|point| {
                    paint::perceptual(point.get(GroupMode::Project, &row.group) / 100.0, 64)
                })
                .collect();
            values.reverse();
            if values.len() > 1 {
                let series = Series::fixed(values, cols.spark_width, 64);
                paint::spark(
                    buf,
                    cols.spark,
                    y,
                    cols.spark_width,
                    &series.plot(),
                    series.max,
                    &ramps.primary,
                );
            } else {
                paint::put(buf, cols.spark, y, "·", Style::default().fg(muted));
            }
        }
        y += 1;
    }
}

fn label_color(pct: Option<f32>, theme: &Theme, muted: Color) -> Color {
    match pct {
        Some(pct) if pct >= 30.0 => theme.status_error,
        Some(pct) if pct >= 10.0 => theme.text_primary,
        _ => muted,
    }
}

fn cpu_color(pct: Option<f32>, theme: &Theme, muted: Color) -> Color {
    match pct {
        Some(pct) if pct >= 70.0 => theme.status_error,
        Some(pct) if pct >= 30.0 => theme.status_warn,
        Some(_) => theme.status_good,
        None => muted,
    }
}

fn details(buf: &mut Buffer, inner: Rect, y: u16, theme: &Theme, row: &CpuRow<'_>) {
    paint::put(buf, inner.x + 1, y, "↳", Style::default().fg(theme.brand));
    let container = match &row.key {
        cpu::RowKey::Container(id) => Some(id.as_str()),
        _ => row.process.and_then(|p| p.docker_container.as_deref()),
    }
    .map(|id| format!("container {id} · "))
    .unwrap_or_default();
    let (summary, context) = if let Some(proc) = row.process {
        let summary = format!(
            "{} · pid {} · ppid {} · user {} · {} threads · state {} · virt {}",
            row.label,
            proc.pid,
            proc.ppid,
            proc.user,
            proc.threads
                .map(|n| n.to_string())
                .unwrap_or_else(|| "·".into()),
            proc.state,
            human_bytes(proc.mem_virt)
        );
        let owner = row
            .owner
            .filter(|owner| owner.pid != proc.pid)
            .map(|owner| format!(" · tool {} ({})", cpu::application(owner).1, owner.pid))
            .unwrap_or_default();
        let cwd = proc
            .cwd
            .as_deref()
            .map(cpu::display_path)
            .unwrap_or_else(|| "·".into());
        (
            summary,
            format!("{container}cwd {cwd}{owner} · {}", short_cmd(&proc.cmd)),
        )
    } else {
        let owners: BTreeSet<_> = row
            .members
            .iter()
            .map(|proc| {
                if proc.user.is_empty() {
                    "?"
                } else {
                    proc.user.as_str()
                }
            })
            .collect();
        let paths: BTreeSet<_> = row
            .members
            .iter()
            .filter_map(|proc| proc.exe.as_deref())
            .map(cpu::display_path)
            .collect();
        (
            format!(
                "{} · {} processes · owners {}",
                row.label,
                row.count,
                owners.into_iter().collect::<Vec<_>>().join(", ")
            ),
            format!(
                "{container}executables {}",
                if paths.is_empty() {
                    "·".into()
                } else {
                    paths.into_iter().collect::<Vec<_>>().join(", ")
                }
            ),
        )
    };
    let width = inner.width.saturating_sub(4) as usize;
    paint::put(
        buf,
        inner.x + 3,
        y,
        &truncate(&summary, width),
        Style::default().fg(theme.text_primary),
    );
    paint::put(
        buf,
        inner.x + 3,
        y + 1,
        &truncate(&context, width),
        Style::default().fg(theme.text_muted),
    );
}
