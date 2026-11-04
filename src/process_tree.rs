//! Process grouping shared by every process view, independent of terminal rendering.
use std::cell::{OnceCell, RefCell};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::collect::{ProcTick, ProcessScope, Snapshot};

mod docker;
mod icons;
mod location;
mod ownership;
mod tree;

use ownership::{Ownership, OwnershipIndex};

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
    Docker,
    Application { identity: String },
    Directory { path: String },
    Project { host: String, repo: String },
}

/// The measure a view ranks rows by.
///
/// Rows sort into descending bands of the measure and then by name, so small
/// fluctuations within a band never reorder it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Metric {
    /// Whole-machine CPU, in 5% bands.
    Cpu,
    /// Resident memory, in 1 GiB bands.
    Rss,
    /// Each process's footprint, else PSS, else RSS, in 1 GiB bands.
    Memory,
    /// Disk read plus write, in 1 MiB/s bands.
    Io,
    /// GPU utilisation, in 5% bands.
    Gpu,
    /// Network receive plus transmit, in 128 KiB/s bands.
    Net,
    /// Estimated power, in 1 W bands.
    Power,
    /// Newest start time, newest first, in one-second bands.
    Start,
    /// Name alone.
    Name,
}

const MIB: f64 = 1024.0 * 1024.0;
const GIB: f64 = 1024.0 * MIB;

impl Metric {
    /// The measured value and the width of one band, in the same unit.
    fn measure(self, totals: &Totals) -> (f64, f64) {
        let positive = |value: Option<f32>| f64::from(value.unwrap_or(0.0).max(0.0));
        match self {
            Self::Cpu => (positive(totals.cpu_pct), 5.0),
            Self::Gpu => (positive(totals.gpu_pct), 5.0),
            Self::Power => (positive(totals.power_w), 1.0),
            Self::Rss => (totals.mem_rss as f64, GIB),
            Self::Memory => (totals.mem_attributed as f64, GIB),
            Self::Io => ((totals.io_read_rate + totals.io_write_rate).max(0.0), MIB),
            Self::Net => (
                (totals.net_rx_rate.unwrap_or(0.0) + totals.net_tx_rate.unwrap_or(0.0)).max(0.0),
                128.0 * 1024.0,
            ),
            // Unknown start times sort after every known one.
            Self::Start => (
                totals
                    .newest_start
                    .and_then(|start| start.duration_since(UNIX_EPOCH).ok())
                    .map_or(-1.0, |since| since.as_secs() as f64),
                1.0,
            ),
            Self::Name => (0.0, 1.0),
        }
    }
}

/// A metric and how finely rows rank by it.
#[derive(Debug, Clone, Copy)]
struct Order {
    metric: Metric,
    /// Navigable lists rank by band so rows hold still while values move;
    /// summaries, which cannot be navigated, rank by exact value.
    exact: bool,
}

impl Order {
    fn key(self, totals: &Totals) -> f64 {
        let (value, band) = self.metric.measure(totals);
        if self.exact {
            value
        } else {
            (value / band).floor()
        }
    }
}

/// Resource totals for one process or a group of processes.
///
/// Optional fields sum the members that report them and are `None` when none
/// do. Shared pages, peaks and virtual size are absent: they are not additive,
/// so a sum would overstate a group.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Totals {
    /// Whole-machine CPU; `None` while the core count is unknown.
    pub cpu_pct: Option<f32>,
    pub mem_rss: u64,
    /// Each member's footprint, else PSS, else RSS.
    pub mem_attributed: u64,
    pub mem_footprint: Option<u64>,
    pub mem_pss: Option<u64>,
    pub mem_private: Option<u64>,
    pub mem_swap: Option<u64>,
    pub io_read_rate: f64,
    pub io_write_rate: f64,
    pub net_rx_rate: Option<f64>,
    pub net_tx_rate: Option<f64>,
    pub gpu_pct: Option<f32>,
    pub gpu_mem_bytes: Option<u64>,
    pub power_w: Option<f32>,
    pub newest_start: Option<SystemTime>,
}

impl Totals {
    pub fn of<'a>(snap: &Snapshot, procs: impl IntoIterator<Item = &'a ProcTick>) -> Self {
        let mut totals = Self {
            cpu_pct: machine_pct(snap, 0.0),
            ..Self::default()
        };
        for p in procs {
            if let Some(total) = &mut totals.cpu_pct {
                *total += machine_pct(snap, p.cpu_pct.max(0.0)).unwrap_or(0.0);
            }
            totals.mem_rss += p.mem_rss;
            totals.mem_attributed += p.mem_footprint.or(p.mem_pss).unwrap_or(p.mem_rss);
            add(&mut totals.mem_footprint, p.mem_footprint);
            add(&mut totals.mem_pss, p.mem_pss);
            add(&mut totals.mem_private, p.mem_private);
            add(&mut totals.mem_swap, p.mem_swap);
            totals.io_read_rate += p.io_read_rate;
            totals.io_write_rate += p.io_write_rate;
            add(&mut totals.net_rx_rate, p.net_rx_rate);
            add(&mut totals.net_tx_rate, p.net_tx_rate);
            add(&mut totals.gpu_pct, p.gpu_pct);
            add(&mut totals.gpu_mem_bytes, p.gpu_mem_bytes);
            add(&mut totals.power_w, p.power_w);
            totals.newest_start = totals.newest_start.max(p.start_time);
        }
        totals
    }
}

fn add<T: std::ops::Add<Output = T> + Default + Copy>(total: &mut Option<T>, value: Option<T>) {
    if let Some(value) = value {
        *total = Some(total.unwrap_or_default() + value);
    }
}

pub struct TreeGroup<'s> {
    pub key: GroupKey,
    pub label: String,
    pub icon: char,
    pub totals: Totals,
    pub members: Vec<&'s ProcTick>,
}

/// Unknown or mixed membership must never move a group into the system section.
pub fn system_only(procs: &[&ProcTick]) -> bool {
    !procs.is_empty()
        && procs.iter().all(|p| {
            p.scope == ProcessScope::System
                && p.docker_container.is_none()
                && !docker::infrastructure(p)
        })
}

/// Case-insensitive substring match against name, command line or user.
/// `needle` must already be lowercase.
pub fn matches_filter(p: &ProcTick, needle: &str) -> bool {
    p.name.to_lowercase().contains(needle)
        || p.cmd.to_lowercase().contains(needle)
        || p.user.to_lowercase().contains(needle)
}

/// Compute each history point once on ingestion, not once per row/frame.
#[derive(Clone)]
pub struct GroupTotals {
    pub application: HashMap<GroupKey, f32>,
    pub project: HashMap<GroupKey, f32>,
}

impl GroupTotals {
    pub fn new(snap: &Snapshot) -> Self {
        let forest = Forest::new(snap);
        let totals = |mode| {
            forest
                .partition(snap, mode)
                .groups
                .iter()
                .filter_map(|group| {
                    let members = group.members.iter().map(|&i| &snap.procs[i]);
                    let cpu = Totals::of(snap, members).cpu_pct.filter(|v| *v > 0.0)?;
                    Some((group.key.clone(), cpu))
                })
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
    // Node tools can set their OS process title (for example Pi). Generic
    // runtime/thread titles retain the executable identity. Never parse cmd.
    let title = matches!(app_name, "node" | "nodejs")
        && !matches!(p.name.as_str(), "" | "node" | "nodejs" | "MainThread");
    let name = if title { p.name.as_str() } else { name };
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
        if title && basename.ends_with(" (deleted)") && !name.ends_with(" (deleted)") {
            format!("{name} (deleted)")
        } else {
            name.into()
        },
        icons::for_name(if title {
            basename.strip_suffix(" (deleted)").unwrap_or(basename)
        } else {
            lower.trim_end_matches(".exe")
        }),
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

/// Ownership and top-level partitions derived from one snapshot.
///
/// Both depend only on process identity and ancestry, never on the metric,
/// filter or expansion a view applies, so every view shares one copy per
/// displayed snapshot instead of rebuilding them for each frame and key.
pub struct Forest {
    fingerprint: u64,
    application: OnceCell<Partition>,
    project: OnceCell<Partition>,
}

struct Partition {
    ownership: OwnershipIndex,
    groups: Vec<Part<usize>>,
}

/// A group before ranking. Members are snapshot indices once cached.
struct Part<M> {
    key: GroupKey,
    label: String,
    icon: char,
    members: Vec<M>,
}

impl Forest {
    pub fn new(snap: &Snapshot) -> Self {
        Self::with_fingerprint(fingerprint(snap))
    }

    fn with_fingerprint(fingerprint: u64) -> Self {
        Self {
            fingerprint,
            application: OnceCell::new(),
            project: OnceCell::new(),
        }
    }

    fn partition(&self, snap: &Snapshot, mode: GroupMode) -> &Partition {
        debug_assert_eq!(self.fingerprint, fingerprint(snap));
        let cell = match mode {
            GroupMode::Application => &self.application,
            GroupMode::Project => &self.project,
        };
        cell.get_or_init(|| {
            let ownership = OwnershipIndex::new(snap, mode);
            let groups = partition(mode, snap.procs.iter().enumerate(), &ownership.read(snap));
            Partition { ownership, groups }
        })
    }
}

/// Hash every process field that ownership and partitioning read, in order,
/// so equal fingerprints index identical processes. A new grouping input must
/// be hashed here, or views would reuse a stale forest.
fn fingerprint(snap: &Snapshot) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    snap.procs.len().hash(&mut hasher);
    for p in &snap.procs {
        (p.pid, p.ppid, p.start_time, p.scope).hash(&mut hasher);
        (&p.name, &p.cmd, &p.exe, &p.cwd).hash(&mut hasher);
        (&p.project, &p.docker_container).hash(&mut hasher);
    }
    hasher.finish()
}

/// The forest for the most recently displayed process table.
#[derive(Default)]
pub struct ForestCache(RefCell<Option<Rc<Forest>>>);

impl ForestCache {
    pub fn get(&self, snap: &Snapshot) -> Rc<Forest> {
        let fingerprint = fingerprint(snap);
        let mut cached = self.0.borrow_mut();
        match cached.as_ref() {
            Some(forest) if forest.fingerprint == fingerprint => Rc::clone(forest),
            _ => {
                let forest = Rc::new(Forest::with_fingerprint(fingerprint));
                *cached = Some(Rc::clone(&forest));
                forest
            }
        }
    }
}

fn partition<'s, M>(
    mode: GroupMode,
    procs: impl IntoIterator<Item = (M, &'s ProcTick)>,
    ownership: &Ownership<'s, '_>,
) -> Vec<Part<M>> {
    // The flag records whether any member's owner is still on disk.
    let mut grouped: BTreeMap<GroupKey, (Part<M>, bool)> = BTreeMap::new();
    for (member, process) in procs {
        let owner = ownership.owner(process);
        let (identity, owner_label, owner_icon) = application(owner);
        let live_owner = !owner_label.ends_with(" (deleted)");
        let (key, label, icon) = if process.docker_container.is_some()
            || docker::infrastructure(process)
            || docker::infrastructure(owner)
        {
            (GroupKey::Docker, "Docker".into(), '\u{e7b0}')
        } else if let Some(location) = location::of(process).filter(|_| mode == GroupMode::Project)
        {
            location.group()
        } else {
            (GroupKey::Application { identity }, owner_label, owner_icon)
        };
        let (group, live) = grouped.entry(key.clone()).or_insert_with(|| {
            (
                Part {
                    key,
                    label,
                    icon,
                    members: Vec::new(),
                },
                false,
            )
        });
        group.members.push(member);
        *live |= live_owner;
    }
    grouped
        .into_values()
        .map(|(mut group, live)| {
            if live && group.label.ends_with(" (deleted)") {
                group.label.truncate(group.label.len() - " (deleted)".len());
            }
            group
        })
        .collect()
}

/// Total and order partitioned groups.
fn rank<'s>(
    snap: &Snapshot,
    order: Order,
    parts: impl IntoIterator<Item = Part<&'s ProcTick>>,
) -> Vec<TreeGroup<'s>> {
    let mut groups: Vec<_> = parts
        .into_iter()
        .map(|part| {
            let mut members: Vec<_> = part
                .members
                .into_iter()
                .map(|p| (order.key(&Totals::of(snap, [p])), p))
                .collect();
            members.sort_by(|(a_band, a), (b_band, b)| {
                (a.scope == ProcessScope::System)
                    .cmp(&(b.scope == ProcessScope::System))
                    .then_with(|| b_band.total_cmp(a_band))
                    .then_with(|| name_order(&a.name, &b.name))
                    .then(a.pid.cmp(&b.pid))
            });
            let members: Vec<_> = members.into_iter().map(|(_, p)| p).collect();
            TreeGroup {
                key: part.key,
                label: part.label,
                icon: part.icon,
                totals: Totals::of(snap, members.iter().copied()),
                members,
            }
        })
        .collect();
    groups.sort_by(|a, b| {
        system_only(&a.members)
            .cmp(&system_only(&b.members))
            .then_with(|| order.key(&b.totals).total_cmp(&order.key(&a.totals)))
            .then_with(|| {
                matches!(b.key, GroupKey::Project { .. })
                    .cmp(&matches!(a.key, GroupKey::Project { .. }))
            })
            .then_with(|| name_order(&a.label, &b.label))
            .then(a.key.cmp(&b.key))
    });
    groups
}

fn usage_order(
    order: Order,
    a: &Totals,
    a_name: &str,
    b: &Totals,
    b_name: &str,
) -> std::cmp::Ordering {
    order
        .key(b)
        .total_cmp(&order.key(a))
        .then_with(|| name_order(a_name, b_name))
}

fn name_order(a_name: &str, b_name: &str) -> std::cmp::Ordering {
    b_name
        .starts_with('~')
        .cmp(&a_name.starts_with('~'))
        .then_with(|| a_name.to_lowercase().cmp(&b_name.to_lowercase()))
        .then_with(|| a_name.cmp(b_name))
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RowKey {
    Container(String),
    Group(GroupKey),
    Directory(GroupKey, String),
    Application(Box<RowKey>, GroupKey),
    Helper(Box<RowKey>, Vec<String>),
    Process(u32, Option<SystemTime>),
}

/// One view's selection and expansion over a tree.
#[derive(Debug, Clone, Default)]
pub struct TreeState {
    pub selected: Option<RowKey>,
    pub expanded: HashSet<RowKey>,
    /// Previous visible order and selected membership for process-list churn.
    previous_rows: Vec<RowKey>,
    selected_members: Vec<RowKey>,
}

/// A selection resolved against freshly built rows, applied by
/// [`TreeState::apply`] once the rows no longer borrow the snapshot.
pub struct Reconciled {
    keys: Vec<RowKey>,
    selected: Option<RowKey>,
    members: Vec<RowKey>,
}

/// Where a navigation key moves the selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Previous(usize),
    Next(usize),
    First,
    Last,
    /// Expand or collapse the selected row.
    Toggle,
}

impl TreeState {
    pub fn selected_index(&self, rows: &[TreeRow<'_>]) -> usize {
        self.selected
            .as_ref()
            .and_then(|key| rows.iter().position(|r| &r.key == key))
            .unwrap_or(0)
    }

    /// Keep the selection when rows move. A singleton can become a group, and
    /// an exited process falls back to a nearby surviving row.
    pub fn reconcile(&self, rows: &[TreeRow<'_>]) -> Reconciled {
        let keys: Vec<_> = rows.iter().map(|row| row.key.clone()).collect();
        let current = self.selected.as_ref();
        let exact = current.and_then(|key| keys.iter().position(|row| row == key));
        let membership = || {
            rows.iter().rposition(|row| {
                row.members.iter().any(|p| {
                    let key = RowKey::Process(p.pid, p.start_time);
                    current == Some(&key) || self.selected_members.contains(&key)
                })
            })
        };
        let nearby = || {
            let previous = &self.previous_rows;
            let index = current.and_then(|key| previous.iter().position(|row| row == key))?;
            previous[index + 1..]
                .iter()
                .chain(previous[..index].iter().rev())
                .find_map(|key| keys.iter().position(|row| row == key))
                .or_else(|| (!keys.is_empty()).then(|| index.min(keys.len() - 1)))
        };
        let row = rows.get(exact.or_else(membership).or_else(nearby).unwrap_or(0));
        Reconciled {
            selected: row.map(|row| row.key.clone()),
            members: row
                .map(|row| {
                    row.members
                        .iter()
                        .map(|p| RowKey::Process(p.pid, p.start_time))
                        .collect()
                })
                .unwrap_or_default(),
            keys,
        }
    }

    /// Store a reconciled selection and return the visible row keys.
    pub fn apply(&mut self, reconciled: Reconciled) -> Vec<RowKey> {
        self.selected = reconciled.selected;
        self.selected_members = reconciled.members;
        self.previous_rows = reconciled.keys.clone();
        reconciled.keys
    }

    /// Return the selection to the first row, forgetting what it followed.
    pub fn clear_selection(&mut self) {
        self.selected = None;
        self.selected_members.clear();
        self.previous_rows.clear();
    }

    pub fn step(&mut self, keys: &[RowKey], step: Step) {
        let Some(last) = keys.len().checked_sub(1) else {
            return;
        };
        let current = self
            .selected
            .as_ref()
            .and_then(|key| keys.iter().position(|row| row == key))
            .unwrap_or(0);
        let index = match step {
            Step::Previous(n) => current.saturating_sub(n),
            Step::Next(n) => current.saturating_add(n).min(last),
            Step::First => 0,
            Step::Last => last,
            Step::Toggle => {
                let key = &keys[current];
                if !matches!(key, RowKey::Process(..)) && !self.expanded.remove(key) {
                    self.expanded.insert(key.clone());
                }
                current
            }
        };
        self.selected = Some(keys[index].clone());
    }
}

pub struct TreeRow<'s> {
    pub key: RowKey,
    pub label: String,
    pub icon: char,
    pub depth: usize,
    pub totals: Totals,
    pub count: usize,
    pub process: Option<&'s ProcTick>,
    pub owner: Option<&'s ProcTick>,
    pub members: Vec<&'s ProcTick>,
    pub group: GroupKey,
}

impl<'s> TreeRow<'s> {
    fn collapse_single(mut self, ownership: &Ownership<'s, '_>) -> Self {
        // Project and folder rows preserve navigation even for a single PID.
        if !matches!(self.group, GroupKey::Application { .. }) {
            return self;
        }
        if let [process] = self.members.as_slice() {
            self.key = RowKey::Process(process.pid, process.start_time);
            self.process = Some(process);
            self.owner = Some(ownership.owner(process));
        }
        self
    }
}

/// One view's reading of a snapshot: its grouping mode and ranking metric.
pub struct Tree<'s, 'f> {
    snap: &'s Snapshot,
    forest: &'f Forest,
    mode: GroupMode,
    order: Order,
}

impl<'s, 'f> Tree<'s, 'f> {
    pub fn new(snap: &'s Snapshot, forest: &'f Forest, mode: GroupMode, metric: Metric) -> Self {
        Self {
            snap,
            forest,
            mode,
            order: Order {
                metric,
                exact: false,
            },
        }
    }

    /// Rank by exact value rather than by band, for summaries that cannot be
    /// navigated: their first row is always the largest.
    pub fn exact(mut self) -> Self {
        self.order.exact = true;
        self
    }

    /// Top-level groups. A filter keeps whole groups that contain a matching
    /// process, with totals for all of their members.
    pub fn groups(&self, filter: Option<&str>) -> Vec<TreeGroup<'s>> {
        let procs = &self.snap.procs;
        let needle = filter.map(str::to_lowercase);
        let parts = self
            .forest
            .partition(self.snap, self.mode)
            .groups
            .iter()
            .filter(|group| {
                needle.as_deref().is_none_or(|needle| {
                    group
                        .members
                        .iter()
                        .any(|&i| matches_filter(&procs[i], needle))
                })
            })
            .map(|group| Part {
                key: group.key.clone(),
                label: group.label.clone(),
                icon: group.icon,
                members: group.members.iter().map(|&i| &procs[i]).collect(),
            });
        rank(self.snap, self.order, parts)
    }

    pub fn rows(&self, groups: &[TreeGroup<'s>], state: &TreeState) -> Vec<TreeRow<'s>> {
        let build = Build {
            snap: self.snap,
            order: self.order,
            ownership: self
                .forest
                .partition(self.snap, self.mode)
                .ownership
                .read(self.snap),
            state,
        };
        let mut rows = Vec::new();
        for group in groups {
            let key = RowKey::Group(group.key.clone());
            let row = TreeRow {
                key: key.clone(),
                label: group.label.clone(),
                icon: group.icon,
                depth: 0,
                totals: group.totals,
                count: group.members.len(),
                process: None,
                owner: None,
                members: group.members.clone(),
                group: group.key.clone(),
            }
            .collapse_single(&build.ownership);
            let expandable = row.process.is_none();
            rows.push(row);
            if !expandable || !state.expanded.contains(&key) {
                continue;
            }
            match group.key {
                GroupKey::Docker => docker::append(&mut rows, &build, group),
                GroupKey::Project { .. } => build.append_checkouts(&mut rows, group),
                GroupKey::Directory { .. } => {
                    build.append_applications(&mut rows, &group.members, &key, 1)
                }
                GroupKey::Application { .. } => tree::append(&mut rows, &build, group, &key, 1),
            }
        }
        if self.mode == GroupMode::Project {
            // Location boundaries affect grouping, while details retain the
            // owning tool across the complete process ancestry.
            let global = self
                .forest
                .partition(self.snap, GroupMode::Application)
                .ownership
                .read(self.snap);
            for row in &mut rows {
                row.owner = row.process.map(|p| global.owner(p));
            }
        }
        rows
    }
}

/// Inputs shared while expanding one tree's rows.
struct Build<'s, 'b> {
    snap: &'s Snapshot,
    order: Order,
    ownership: Ownership<'s, 'b>,
    state: &'b TreeState,
}

impl<'s> Build<'s, '_> {
    fn append_checkouts(&self, rows: &mut Vec<TreeRow<'s>>, group: &TreeGroup<'s>) {
        let mut checkouts: BTreeMap<&str, Vec<&ProcTick>> = BTreeMap::new();
        for p in &group.members {
            if let Some(project) = &p.project {
                checkouts.entry(&project.checkout).or_default().push(p);
            }
        }
        let mut checkouts: Vec<_> = checkouts
            .into_iter()
            .map(|(path, procs)| {
                let totals = Totals::of(self.snap, procs.iter().copied());
                (path, procs, totals, display_path(path))
            })
            .collect();
        checkouts.sort_by(|a, b| {
            system_only(&a.1)
                .cmp(&system_only(&b.1))
                .then_with(|| usage_order(self.order, &a.2, &a.3, &b.2, &b.3))
        });
        for (path, procs, totals, label) in checkouts {
            let key = RowKey::Directory(group.key.clone(), path.into());
            let row = TreeRow {
                key: key.clone(),
                label,
                icon: '\u{f07b}',
                depth: 1,
                totals,
                count: procs.len(),
                process: None,
                owner: None,
                members: procs.clone(),
                group: group.key.clone(),
            }
            .collapse_single(&self.ownership);
            let expandable = row.process.is_none();
            rows.push(row);
            if expandable && self.state.expanded.contains(&key) {
                self.append_applications(rows, &procs, &key, 2);
            }
        }
    }

    fn append_applications(
        &self,
        rows: &mut Vec<TreeRow<'s>>,
        procs: &[&'s ProcTick],
        context: &RowKey,
        depth: usize,
    ) {
        let parts = partition(
            GroupMode::Application,
            procs.iter().map(|&p| (p, p)),
            &self.ownership,
        );
        for app in rank(self.snap, self.order, parts) {
            let key = RowKey::Application(Box::new(context.clone()), app.key.clone());
            let row = TreeRow {
                key: key.clone(),
                label: app.label.clone(),
                icon: app.icon,
                depth,
                totals: app.totals,
                count: app.members.len(),
                process: None,
                owner: None,
                members: app.members.clone(),
                group: app.key.clone(),
            }
            .collapse_single(&self.ownership);
            let expandable = row.process.is_none();
            rows.push(row);
            if expandable && self.state.expanded.contains(&key) {
                tree::append(rows, self, &app, &key, depth + 1);
            }
        }
    }

    fn process_row(&self, p: &'s ProcTick, group: &GroupKey, depth: usize) -> TreeRow<'s> {
        TreeRow {
            key: RowKey::Process(p.pid, p.start_time),
            label: p.name.clone(),
            icon: application(p).2,
            depth,
            totals: Totals::of(self.snap, [p]),
            count: 1,
            process: Some(p),
            owner: Some(self.ownership.owner(p)),
            members: vec![p],
            group: group.clone(),
        }
    }
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
