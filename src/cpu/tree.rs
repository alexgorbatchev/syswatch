//! Expand owning-tool groups into helper applications and individual PIDs.
use super::{
    application, machine_pct, ownership::Ownership, process_row, system_only, usage_order,
    CpuGroup, CpuRow, CpuState, GroupKey, RowKey,
};
use crate::collect::{ProcTick, Snapshot};
use std::collections::BTreeMap;

struct Branch<'a> {
    application: GroupKey,
    label: String,
    icon: char,
    path: Vec<String>,
    members: Vec<&'a ProcTick>,
    direct: Vec<&'a ProcTick>,
    children: BTreeMap<String, usize>,
}

pub(super) fn append<'a>(
    rows: &mut Vec<CpuRow<'a>>,
    snap: &Snapshot,
    group: &CpuGroup<'a>,
    context: &RowKey,
    depth: usize,
    state: &CpuState,
    ownership: &Ownership<'a>,
) {
    let mut branches = vec![Branch {
        application: group.key.clone(),
        label: group.label.clone(),
        icon: group.icon,
        path: Vec::new(),
        members: group.members.clone(),
        direct: Vec::new(),
        children: BTreeMap::new(),
    }];
    for &p in &group.members {
        let ancestry = ownership.branch(p);
        let mut previous = application(ancestry[0]).0;
        let mut index = 0;
        for ancestor in ancestry.into_iter().skip(1) {
            let (identity, label, icon) = application(ancestor);
            // Multiple generations of the same executable share one helper
            // group, while differing helper applications remain nested.
            if identity == previous {
                continue;
            }
            previous = identity.clone();
            let child = if let Some(&child) = branches[index].children.get(&identity) {
                child
            } else {
                let child = branches.len();
                let mut path = branches[index].path.clone();
                path.push(identity.clone());
                branches[index].children.insert(identity.clone(), child);
                branches.push(Branch {
                    application: GroupKey::Application { identity },
                    label,
                    icon,
                    path,
                    members: Vec::new(),
                    direct: Vec::new(),
                    children: BTreeMap::new(),
                });
                child
            };
            index = child;
            branches[index].members.push(p);
            if branches[index].label.ends_with(" (deleted)")
                && !application(ancestor).1.ends_with(" (deleted)")
            {
                let len = branches[index].label.len() - " (deleted)".len();
                branches[index].label.truncate(len);
            }
        }
        branches[index].direct.push(p);
    }

    let mut pending = entries(snap, &branches, 0, context, depth, ownership);
    while let Some((row, branch)) = pending.pop() {
        let expanded = state.expanded.contains(&row.key);
        let child_depth = row.depth + 1;
        rows.push(row);
        if let Some(branch) = branch.filter(|_| expanded) {
            pending.extend(entries(
                snap,
                &branches,
                branch,
                context,
                child_depth,
                ownership,
            ));
        }
    }
}

fn entries<'a>(
    snap: &Snapshot,
    branches: &[Branch<'a>],
    index: usize,
    context: &RowKey,
    depth: usize,
    ownership: &Ownership<'a>,
) -> Vec<(CpuRow<'a>, Option<usize>)> {
    let branch = &branches[index];
    let mut entries: Vec<_> = branch
        .direct
        .iter()
        .map(|p| {
            (
                process_row(snap, p, ownership.owner(p), &branch.application, depth),
                None,
            )
        })
        .collect();
    for &child in branch.children.values() {
        let branch = &branches[child];
        let row = CpuRow {
            key: RowKey::Helper(Box::new(context.clone()), branch.path.clone()),
            label: branch.label.clone(),
            icon: branch.icon,
            depth,
            cpu_pct: machine_pct(
                snap,
                branch.members.iter().map(|p| p.cpu_pct.max(0.0)).sum(),
            ),
            mem_rss: branch.members.iter().map(|p| p.mem_rss).sum(),
            count: branch.members.len(),
            process: None,
            owner: None,
            members: branch.members.clone(),
            group: branch.application.clone(),
        }
        .collapse_single(ownership);
        let expandable = row.process.is_none();
        entries.push((row, expandable.then_some(child)));
    }
    // Reverse order for the iterative depth-first stack; rows retain the same
    // system/CPU-band/name ordering used by top-level applications.
    entries.sort_by(|(a, _), (b, _)| {
        system_only(&b.members)
            .cmp(&system_only(&a.members))
            .then_with(|| usage_order(b.cpu_pct, &b.label, a.cpu_pct, &a.label))
            .then_with(|| b.process.map(|p| p.pid).cmp(&a.process.map(|p| p.pid)))
    });
    entries
}
