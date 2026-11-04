//! Dense's project tree, backed by the same rows as Full's CPU panel.

use std::collections::BTreeSet;

use super::{paint, short_cmd, truncate, App, Buffer, Layout, Rect, Snapshot, Style, Theme};
use crate::app::TreeView;
use crate::process_tree::{self, GroupMode, TreeRow};
use crate::ui::tree::{Column, TreeTable};
use crate::ui::widgets::human_bytes;
use ratatui::widgets::Widget;

pub(super) fn draw(buf: &mut Buffer, layout: &Layout, theme: &Theme, app: &App, snap: &Snapshot) {
    if layout.procs.width == 0 || layout.procs.height == 0 {
        return;
    }
    let (groups, rows) = app.tree_rows(TreeView::Dense, snap);
    let selected = app.dense.tree.selected_index(&rows);
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
    let mut y = inner.y;
    if inner.height >= 4 {
        if let Some(row) = rows.get(selected) {
            details(buf, inner, y, theme, row);
        }
        y += 2;
    }
    TreeTable::new(&rows, "APPLICATION / PROJECT")
        .expanded(&app.dense.tree.expanded)
        .columns([
            Column::cpu().sorted(true),
            Column::memory(),
            Column::recent(app, GroupMode::Project),
        ])
        .selection(selected, &app.dense.scroll)
        .render(
            Rect {
                y,
                height: inner.bottom().saturating_sub(y),
                ..inner
            },
            buf,
        );
}

fn details(buf: &mut Buffer, inner: Rect, y: u16, theme: &Theme, row: &TreeRow<'_>) {
    paint::put(buf, inner.x + 1, y, "↳", Style::default().fg(theme.brand));
    let container = match &row.key {
        process_tree::RowKey::Container(id) => Some(id.as_str()),
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
            .map(|owner| {
                format!(
                    " · tool {} ({})",
                    process_tree::application(owner).1,
                    owner.pid
                )
            })
            .unwrap_or_default();
        let cwd = proc
            .cwd
            .as_deref()
            .map(process_tree::display_path)
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
            .map(process_tree::display_path)
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
