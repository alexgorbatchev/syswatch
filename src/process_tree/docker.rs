//! Snapshot-only Docker grouping, independent of OS parent relationships.

use std::collections::BTreeMap;

use super::{system_only, usage_order, Build, GroupKey, RowKey, Totals, TreeGroup, TreeRow};
use crate::collect::ProcTick;

pub(super) fn infrastructure(p: &ProcTick) -> bool {
    let name = p
        .exe
        .as_deref()
        .filter(|exe| !exe.is_empty())
        .and_then(|exe| exe.rsplit(['/', '\\']).next())
        .unwrap_or(&p.name);
    matches!(
        name.trim_end_matches(" (deleted)"),
        "docker" | "dockerd" | "docker-proxy"
    )
}

pub(super) fn append<'a>(
    rows: &mut Vec<TreeRow<'a>>,
    build: &Build<'a, '_>,
    group: &TreeGroup<'a>,
) {
    let mut containers: BTreeMap<&str, Vec<&ProcTick>> = BTreeMap::new();
    let mut children = Vec::new();
    for &p in &group.members {
        if let Some(id) = p.docker_container.as_deref() {
            containers.entry(id).or_default().push(p);
        } else {
            children.push(build.process_row(p, &group.key, 1));
        }
    }
    for (id, members) in containers {
        children.push(TreeRow {
            key: RowKey::Container(id.into()),
            label: format!("container {}", id.chars().take(12).collect::<String>()),
            icon: '\u{e7b0}',
            depth: 1,
            totals: Totals::of(build.snap, members.iter().copied()),
            count: members.len(),
            process: None,
            owner: None,
            members,
            group: GroupKey::Docker,
        });
    }
    children.sort_by(|a, b| {
        system_only(&a.members)
            .cmp(&system_only(&b.members))
            .then_with(|| usage_order(build.order, &a.totals, &a.label, &b.totals, &b.label))
            .then_with(|| a.process.map(|p| p.pid).cmp(&b.process.map(|p| p.pid)))
    });
    for row in children {
        let expanded = build.state.expanded.contains(&row.key);
        let members = if expanded && row.process.is_none() {
            row.members.clone()
        } else {
            Vec::new()
        };
        rows.push(row);
        for p in members {
            rows.push(build.process_row(p, &GroupKey::Docker, 2));
        }
    }
}
