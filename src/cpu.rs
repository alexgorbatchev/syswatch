//! CPU explanations and grouping, independent of terminal rendering.
use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::SystemTime;

use crate::collect::{ProcTick, ProcessScope, Snapshot};

mod icons;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GroupMode {
    #[default]
    Application,
    Project,
}

impl GroupMode {
    pub fn next(self) -> Self {
        match self {
            Self::Application => Self::Project,
            Self::Project => Self::Application,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Application => "Application",
            Self::Project => "Project",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GroupKey {
    Application {
        identity: String,
    },
    Project {
        host: String,
        repo: String,
        user: String,
    },
}

pub struct CpuGroup<'a> {
    pub key: GroupKey,
    pub label: String,
    pub icon: char,
    pub cpu_pct: Option<f32>,
    pub members: Vec<&'a ProcTick>,
}

/// Unknown or mixed membership must never move a group into the system section.
pub fn system_only(procs: &[&ProcTick]) -> bool {
    !procs.is_empty() && procs.iter().all(|p| p.scope == ProcessScope::System)
}

/// Compute each history point once on ingestion, not once per row/frame.
#[derive(Clone)]
pub struct GroupTotals {
    pub application: HashMap<GroupKey, f32>,
    pub project: HashMap<GroupKey, f32>,
}

impl GroupTotals {
    pub fn new(snap: &Snapshot) -> Self {
        let totals = |mode| {
            groups(snap, mode)
                .into_iter()
                .filter_map(|g| g.cpu_pct.filter(|v| *v > 0.0).map(|v| (g.key, v)))
                .collect()
        };
        Self {
            application: totals(GroupMode::Application),
            project: totals(GroupMode::Project),
        }
    }
    pub fn get(&self, mode: GroupMode, key: &GroupKey) -> f32 {
        let totals = match mode {
            GroupMode::Application => &self.application,
            GroupMode::Project => &self.project,
        };
        totals.get(key).copied().unwrap_or(0.0)
    }
}

pub fn machine_pct(snap: &Snapshot, raw: f32) -> Option<f32> {
    let cores = snap.cpu.per_core.len().max(snap.host.cpu_cores as usize);
    (cores > 0).then(|| raw / cores as f32)
}

/// Prefer executable/bundle names; fall back to the OS-reported process name.
/// Never infer identity from arbitrary command-line substrings.
pub fn application(p: &ProcTick) -> (String, String, char) {
    let exe = p.exe.as_deref().filter(|s| !s.is_empty());
    let bundle = exe.and_then(|s| s.split('/').find(|part| part.ends_with(".app")));
    let basename = exe
        .and_then(|s| s.rsplit(['/', '\\']).next())
        .unwrap_or(&p.name);
    let name = bundle
        .and_then(|s| s.strip_suffix(".app"))
        .or_else(|| exe.and_then(claude_native_name))
        .unwrap_or(basename);
    let app_name = name.strip_suffix(" (deleted)").unwrap_or(name);
    let lower = app_name.to_ascii_lowercase();
    let known = icons::known_application(lower.trim_end_matches(".exe"));
    if let Some((label, icon)) = known {
        return (format!("known:{label}"), label.into(), icon);
    }
    // Installations and owners are details, not application identities.
    let identity = if app_name.is_empty() {
        format!("pid:{}:{:?}", p.pid, p.start_time)
    } else {
        format!("name:{app_name}")
    };
    (
        identity,
        name.into(),
        icons::for_name(lower.trim_end_matches(".exe")),
    )
}

fn claude_native_name(exe: &str) -> Option<&'static str> {
    // The native launcher resolves to .../claude/versions/<version>.
    // Helpers can report the version as their process name too.
    let mut parts = exe.rsplit(['/', '\\']);
    let filename = parts.next()?;
    if parts.next() != Some("versions") || parts.next() != Some("claude") {
        return None;
    }
    Some(if filename.ends_with(" (deleted)") {
        "claude (deleted)"
    } else {
        "claude"
    })
}

pub fn groups(snap: &Snapshot, mode: GroupMode) -> Vec<CpuGroup<'_>> {
    group_processes(snap, mode, snap.procs.iter())
}

fn group_processes<'a>(
    snap: &Snapshot,
    mode: GroupMode,
    procs: impl IntoIterator<Item = &'a ProcTick>,
) -> Vec<CpuGroup<'a>> {
    let mut grouped = BTreeMap::new();
    for process in procs {
        let (key, label, icon) = if let Some(project) = process
            .project
            .as_ref()
            .filter(|_| mode == GroupMode::Project)
        {
            let repo_key = if project.host.is_empty() {
                project.checkout.clone()
            } else {
                project.repo.clone()
            };
            (
                GroupKey::Project {
                    host: project.host.clone(),
                    repo: repo_key,
                    user: process.user.clone(),
                },
                project.repo.clone(),
                if project.host == "github.com" {
                    '\u{f402}'
                } else {
                    '\u{e702}'
                },
            )
        } else {
            let (identity, label, icon) = application(process);
            (GroupKey::Application { identity }, label, icon)
        };
        let group = grouped.entry(key.clone()).or_insert_with(|| CpuGroup {
            key,
            label,
            icon,
            cpu_pct: machine_pct(snap, 0.0),
            members: Vec::new(),
        });
        if let Some(total) = &mut group.cpu_pct {
            *total += machine_pct(snap, process.cpu_pct.max(0.0)).unwrap_or(0.0);
        }
        group.members.push(process);
    }
    let mut groups: Vec<_> = grouped.into_values().collect();
    for group in &mut groups {
        if group.label.ends_with(" (deleted)")
            && group
                .members
                .iter()
                .any(|p| !application(p).1.ends_with(" (deleted)"))
        {
            group.label.truncate(group.label.len() - " (deleted)".len());
        }
        group.members.sort_by(|a, b| {
            (a.scope == ProcessScope::System)
                .cmp(&(b.scope == ProcessScope::System))
                .then_with(|| {
                    usage_order(
                        machine_pct(snap, a.cpu_pct),
                        &a.name,
                        machine_pct(snap, b.cpu_pct),
                        &b.name,
                    )
                })
                .then(a.pid.cmp(&b.pid))
        });
    }
    groups.sort_by(|a, b| {
        system_only(&a.members)
            .cmp(&system_only(&b.members))
            .then_with(|| usage_order(a.cpu_pct, &a.label, b.cpu_pct, &b.label))
            .then(a.key.cmp(&b.key))
    });
    groups
}

fn usage_order(a: Option<f32>, a_name: &str, b: Option<f32>, b_name: &str) -> std::cmp::Ordering {
    let band = |pct: Option<f32>| (pct.unwrap_or(0.0).max(0.0) / 5.0).floor();
    band(b)
        .total_cmp(&band(a))
        .then_with(|| a_name.to_lowercase().cmp(&b_name.to_lowercase()))
        .then_with(|| a_name.cmp(b_name))
}

pub fn assessment(snap: &Snapshot) -> &'static str {
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

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RowKey {
    Group(GroupKey),
    Checkout(GroupKey, String),
    CheckoutApplication(GroupKey, String, GroupKey),
    Process(u32, Option<SystemTime>),
}

#[derive(Default)]
pub struct CpuState {
    pub selected: Option<RowKey>,
    pub expanded: HashSet<RowKey>,
    pub show_cores: bool,
    pub core_page: usize,
    pub detail_scroll: usize,
}

pub struct CpuRow<'a> {
    pub key: RowKey,
    pub label: String,
    pub icon: char,
    pub depth: usize,
    pub cpu_pct: Option<f32>,
    pub mem_rss: u64,
    pub count: usize,
    pub process: Option<&'a ProcTick>,
    pub members: Vec<&'a ProcTick>,
    pub group: GroupKey,
}

pub fn rows<'a>(snap: &'a Snapshot, groups: &[CpuGroup<'a>], state: &CpuState) -> Vec<CpuRow<'a>> {
    let mut rows = Vec::new();
    for group in groups {
        let key = RowKey::Group(group.key.clone());
        rows.push(CpuRow {
            key: key.clone(),
            label: group.label.clone(),
            icon: group.icon,
            depth: 0,
            cpu_pct: group.cpu_pct,
            mem_rss: group.members.iter().map(|p| p.mem_rss).sum(),
            count: group.members.len(),
            process: None,
            members: group.members.clone(),
            group: group.key.clone(),
        });
        if !state.expanded.contains(&key) {
            continue;
        }
        if matches!(group.key, GroupKey::Project { .. }) {
            let mut checkouts: BTreeMap<&str, Vec<&ProcTick>> = BTreeMap::new();
            for p in &group.members {
                if let Some(project) = &p.project {
                    checkouts.entry(&project.checkout).or_default().push(p);
                }
            }
            let mut checkouts: Vec<_> = checkouts
                .into_iter()
                .map(|(path, procs)| {
                    let cpu = machine_pct(snap, procs.iter().map(|p| p.cpu_pct.max(0.0)).sum());
                    (path, procs, cpu)
                })
                .collect();
            checkouts.sort_by(|a, b| {
                system_only(&a.1)
                    .cmp(&system_only(&b.1))
                    .then_with(|| usage_order(a.2, a.0, b.2, b.0))
            });
            for (checkout, procs, cpu_pct) in checkouts {
                let key = RowKey::Checkout(group.key.clone(), checkout.into());
                rows.push(CpuRow {
                    key: key.clone(),
                    label: display_path(checkout),
                    icon: '\u{e702}',
                    depth: 1,
                    cpu_pct,
                    mem_rss: procs.iter().map(|p| p.mem_rss).sum(),
                    count: procs.len(),
                    process: None,
                    members: procs.clone(),
                    group: group.key.clone(),
                });
                if state.expanded.contains(&key) {
                    for app in group_processes(snap, GroupMode::Application, procs.iter().copied())
                    {
                        let key = RowKey::CheckoutApplication(
                            group.key.clone(),
                            checkout.into(),
                            app.key.clone(),
                        );
                        rows.push(CpuRow {
                            key: key.clone(),
                            label: app.label,
                            icon: app.icon,
                            depth: 2,
                            cpu_pct: app.cpu_pct,
                            mem_rss: app.members.iter().map(|p| p.mem_rss).sum(),
                            count: app.members.len(),
                            process: None,
                            members: app.members.clone(),
                            group: app.key.clone(),
                        });
                        if state.expanded.contains(&key) {
                            append_processes(&mut rows, snap, &app.members, &app.key, 3);
                        }
                    }
                }
            }
        } else {
            append_processes(&mut rows, snap, &group.members, &group.key, 1);
        }
    }
    rows
}

fn append_processes<'a>(
    rows: &mut Vec<CpuRow<'a>>,
    snap: &Snapshot,
    procs: &[&'a ProcTick],
    group: &GroupKey,
    depth: usize,
) {
    for p in procs {
        rows.push(CpuRow {
            key: RowKey::Process(p.pid, p.start_time),
            label: p.name.clone(),
            icon: application(p).2,
            depth,
            cpu_pct: machine_pct(snap, p.cpu_pct.max(0.0)),
            mem_rss: p.mem_rss,
            count: 1,
            process: Some(p),
            members: vec![p],
            group: group.clone(),
        });
    }
}

pub fn selected_index(rows: &[CpuRow<'_>], state: &CpuState) -> usize {
    state
        .selected
        .as_ref()
        .and_then(|key| rows.iter().position(|r| &r.key == key))
        .unwrap_or(0)
}

pub fn display_path(path: &str) -> String {
    if let Some(home) = dirs::home_dir() {
        if let Ok(rest) = std::path::Path::new(path).strip_prefix(home) {
            return if rest.as_os_str().is_empty() {
                "~".into()
            } else {
                format!("~/{}", rest.display())
            };
        }
    }
    path.into()
}

#[cfg(test)]
mod tests;
