//! Snapshot-only Docker grouping, independent of OS parent relationships.

use std::collections::BTreeMap;

use super::{
    machine_pct, ownership::Ownership, process_row, system_only, usage_order, CpuGroup, CpuRow,
    CpuState, GroupKey, RowKey,
};
use crate::collect::{ProcTick, Snapshot};

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
    rows: &mut Vec<CpuRow<'a>>,
    snap: &Snapshot,
    group: &CpuGroup<'a>,
    state: &CpuState,
    ownership: &Ownership<'a>,
) {
    let mut containers: BTreeMap<&str, Vec<&ProcTick>> = BTreeMap::new();
    let mut children = Vec::new();
    for &p in &group.members {
        if let Some(id) = p.docker_container.as_deref() {
            containers.entry(id).or_default().push(p);
        } else {
            children.push(process_row(snap, p, ownership.owner(p), &group.key, 1));
        }
    }
    for (id, members) in containers {
        children.push(CpuRow {
            key: RowKey::Container(id.into()),
            label: format!("container {}", id.chars().take(12).collect::<String>()),
            icon: '\u{e7b0}',
            depth: 1,
            cpu_pct: machine_pct(snap, members.iter().map(|p| p.cpu_pct.max(0.0)).sum()),
            mem_rss: members.iter().map(|p| p.mem_rss).sum(),
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
            .then_with(|| usage_order(a.cpu_pct, &a.label, b.cpu_pct, &b.label))
            .then_with(|| a.process.map(|p| p.pid).cmp(&b.process.map(|p| p.pid)))
    });
    for row in children {
        let expanded = state.expanded.contains(&row.key);
        let members = if expanded && row.process.is_none() {
            row.members.clone()
        } else {
            Vec::new()
        };
        rows.push(row);
        for p in members {
            rows.push(process_row(
                snap,
                p,
                ownership.owner(p),
                &GroupKey::Docker,
                2,
            ));
        }
    }
}
