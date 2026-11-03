use super::*;
use std::time::Duration;

const GIB: u64 = 1024 * 1024 * 1024;

fn ranked(snap: &Snapshot, metric: Metric) -> Vec<String> {
    Tree::new(snap, &Forest::new(snap), GroupMode::Application, metric)
        .groups(None)
        .into_iter()
        .map(|group| group.label)
        .collect()
}

fn named(pid: u32, name: &str) -> ProcTick {
    process(pid, &format!("/bin/{name}"), 0.0)
}

#[test]
fn memory_ranks_in_gib_bands_by_the_most_accurate_measure() {
    let mut procs = vec![
        named(1, "zulu"),
        named(2, "alpha"),
        named(3, "large"),
        named(4, "mixed"),
        named(5, "mixed"),
    ];
    // Footprint wins over RSS; within the 0 GiB band names decide.
    procs[0].mem_rss = 5 * GIB;
    procs[0].mem_footprint = Some(GIB / 2);
    procs[1].mem_rss = GIB / 4;
    procs[2].mem_pss = Some(3 * GIB);
    // One member reports PSS and one only RSS; their sum crosses 1 GiB.
    procs[3].mem_pss = Some(GIB / 2);
    procs[4].mem_rss = 3 * GIB / 4;
    let snap = snapshot(procs);
    assert_eq!(
        ranked(&snap, Metric::Memory),
        ["large", "mixed", "alpha", "zulu"]
    );
    // Resident memory alone ranks the same processes differently.
    assert_eq!(
        ranked(&snap, Metric::Rss),
        ["zulu", "alpha", "large", "mixed"]
    );
}

#[test]
fn power_io_net_and_gpu_rank_in_their_own_bands() {
    let mut procs = vec![named(1, "zulu"), named(2, "alpha"), named(3, "busy")];
    procs[0].power_w = Some(0.9);
    procs[1].power_w = Some(0.2);
    procs[2].power_w = Some(1.1);
    let snap = snapshot(procs.clone());
    assert_eq!(ranked(&snap, Metric::Power), ["busy", "alpha", "zulu"]);

    for p in &mut procs {
        p.power_w = None;
    }
    procs[0].io_read_rate = 900.0 * 1024.0;
    procs[2].io_write_rate = 1024.0 * 1024.0;
    assert_eq!(
        ranked(&snapshot(procs.clone()), Metric::Io),
        ["busy", "alpha", "zulu"]
    );

    procs[0].net_rx_rate = Some(100.0 * 1024.0);
    procs[2].net_tx_rate = Some(128.0 * 1024.0);
    assert_eq!(
        ranked(&snapshot(procs.clone()), Metric::Net),
        ["busy", "alpha", "zulu"]
    );

    procs[0].gpu_pct = Some(4.9);
    procs[2].gpu_pct = Some(5.0);
    assert_eq!(
        ranked(&snapshot(procs), Metric::Gpu),
        ["busy", "alpha", "zulu"]
    );
}

#[test]
fn start_ranks_newest_first_with_unknown_times_last_and_name_ignores_usage() {
    let at = |secs| Some(SystemTime::UNIX_EPOCH + Duration::from_secs(secs));
    let mut procs = vec![
        named(1, "unknown"),
        named(2, "older"),
        named(3, "newest"),
        named(4, "newest"),
    ];
    procs[1].start_time = at(100);
    procs[2].start_time = at(50);
    procs[3].start_time = at(200);
    procs[1].cpu_pct = 400.0;
    let snap = snapshot(procs);
    assert_eq!(ranked(&snap, Metric::Start), ["newest", "older", "unknown"]);
    assert_eq!(ranked(&snap, Metric::Name), ["newest", "older", "unknown"]);
    // Within a group, members follow the same metric.
    let groups = Tree::new(
        &snap,
        &Forest::new(&snap),
        GroupMode::Application,
        Metric::Start,
    )
    .groups(None);
    assert_eq!(
        groups[0].members.iter().map(|p| p.pid).collect::<Vec<_>>(),
        [4, 3]
    );
}

#[test]
fn totals_sum_reported_values_and_leave_unreported_ones_absent() {
    let mut procs = vec![named(1, "app"), named(2, "app"), named(3, "app")];
    procs[0].mem_footprint = Some(10);
    procs[1].mem_footprint = Some(5);
    procs[0].mem_swap = Some(7);
    procs[0].power_w = Some(1.5);
    procs[2].power_w = Some(0.5);
    procs[1].net_rx_rate = Some(3.0);
    procs[0].mem_rss = 100;
    procs[2].mem_rss = 50;
    procs[0].io_read_rate = 1.0;
    procs[1].io_write_rate = 2.0;
    procs[2].gpu_mem_bytes = Some(9);
    let snap = snapshot(procs);
    let totals = Totals::of(&snap, &snap.procs);
    assert_eq!(totals.mem_footprint, Some(15));
    assert_eq!(totals.mem_swap, Some(7));
    assert_eq!(totals.power_w, Some(2.0));
    assert_eq!(totals.net_rx_rate, Some(3.0));
    assert_eq!(totals.net_tx_rate, None);
    assert_eq!(totals.mem_pss, None);
    assert_eq!(totals.gpu_pct, None);
    assert_eq!(totals.gpu_mem_bytes, Some(9));
    assert_eq!(totals.mem_rss, 150);
    // Footprint where reported, RSS for the third process.
    assert_eq!(totals.mem_attributed, 10 + 5 + 50);
    assert_eq!((totals.io_read_rate, totals.io_write_rate), (1.0, 2.0));
}

#[test]
fn filter_keeps_whole_groups_with_totals_for_every_member() {
    let mut procs = vec![
        process(1, "/bin/browser", 80.0),
        process(2, "/bin/browser", 40.0),
        process(3, "/bin/postgres", 8.0),
        process(4, "/bin/vim", 8.0),
    ];
    procs[1].cmd = "/bin/browser --type=renderer".into();
    procs[3].user = "Deploy".into();
    let snap = snapshot(procs);
    let forest = Forest::new(&snap);
    let tree = Tree::new(&snap, &forest, GroupMode::Application, Metric::Cpu);

    let kept = tree.groups(Some("RENDERER"));
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].label, "browser");
    // The non-matching browser process stays, and so does its CPU.
    assert_eq!(kept[0].members.len(), 2);
    assert_eq!(kept[0].totals.cpu_pct, Some(15.0));

    let by_user = tree.groups(Some("deploy"));
    assert_eq!(
        by_user.iter().map(|g| g.label.as_str()).collect::<Vec<_>>(),
        ["vim"]
    );
    assert!(tree.groups(Some("nothing-matches")).is_empty());
    assert_eq!(tree.groups(None).len(), 3);
}

#[test]
fn cached_forest_survives_usage_changes_and_rebuilds_for_new_identities() {
    let cache = ForestCache::default();
    let mut snap = snapshot(vec![
        process(1, "/bin/browser", 80.0),
        process(2, "/bin/browser", 40.0),
    ]);
    let first = cache.get(&snap);
    assert!(Rc::ptr_eq(&first, &cache.get(&snap)));

    // Usage is ranked per call, so it never invalidates the grouping.
    snap.procs[0].cpu_pct = 0.0;
    snap.procs[1].mem_rss = GIB;
    let reused = cache.get(&snap);
    assert!(Rc::ptr_eq(&first, &reused));
    let groups = Tree::new(&snap, &reused, GroupMode::Application, Metric::Cpu).groups(None);
    assert_eq!(groups[0].totals.cpu_pct, Some(5.0));
    assert_eq!(groups[0].totals.mem_rss, GIB);

    // Identity and ancestry inputs do.
    for change in [
        |p: &mut ProcTick| p.ppid = 2,
        |p: &mut ProcTick| p.cwd = Some("/work".into()),
        |p: &mut ProcTick| p.cmd = "browser --type=gpu".into(),
        |p: &mut ProcTick| p.docker_container = Some("abc".into()),
        |p: &mut ProcTick| p.scope = ProcessScope::System,
    ] {
        let before = cache.get(&snap);
        let mut changed = snap.clone();
        change(&mut changed.procs[0]);
        assert!(!Rc::ptr_eq(&before, &cache.get(&changed)));
        snap = changed;
    }
    let mut renamed = snap.clone();
    renamed.procs[1].exe = Some("/bin/viewer".into());
    let before = cache.get(&snap);
    let after = cache.get(&renamed);
    assert!(!Rc::ptr_eq(&before, &after));
    let labels: Vec<_> = Tree::new(&renamed, &after, GroupMode::Application, Metric::Cpu)
        .groups(None)
        .into_iter()
        .map(|g| g.label)
        .collect();
    assert!(labels.contains(&"viewer".to_string()), "{labels:?}");
}

#[test]
fn steps_move_within_bounds_and_toggle_only_expandable_rows() {
    let group = RowKey::Group(GroupKey::Application {
        identity: "name:browser".into(),
    });
    let keys = vec![
        group.clone(),
        RowKey::Process(1, None),
        RowKey::Process(2, None),
    ];
    let mut state = TreeState::default();
    state.step(&keys, Step::Previous(1));
    assert_eq!(state.selected, Some(group.clone()));
    state.step(&keys, Step::Next(5));
    assert_eq!(state.selected, Some(RowKey::Process(2, None)));
    state.step(&keys, Step::Previous(1));
    assert_eq!(state.selected, Some(RowKey::Process(1, None)));
    state.step(&keys, Step::Toggle);
    assert!(
        state.expanded.is_empty(),
        "processes have nothing to expand"
    );
    state.step(&keys, Step::First);
    state.step(&keys, Step::Toggle);
    assert!(state.expanded.contains(&group));
    state.step(&keys, Step::Toggle);
    assert!(state.expanded.is_empty());
    state.step(&keys, Step::Last);
    assert_eq!(state.selected, Some(RowKey::Process(2, None)));
    // Nothing to select leaves the state alone.
    state.step(&[], Step::First);
    assert_eq!(state.selected, Some(RowKey::Process(2, None)));
}

#[test]
fn cleared_selection_forgets_the_membership_it_followed() {
    let snap = snapshot(vec![
        process(1, "/bin/alpha", 0.0),
        process(2, "/bin/zulu", 80.0),
    ]);
    let grouped = groups(&snap, GroupMode::Application);
    let visible = rows(
        &snap,
        GroupMode::Application,
        &grouped,
        &TreeState::default(),
    );
    let mut state = TreeState {
        selected: Some(RowKey::Process(1, None)),
        ..TreeState::default()
    };
    let keys = state.apply(state.reconcile(&visible));
    assert_eq!(keys.len(), 2);
    assert_eq!(state.selected, Some(RowKey::Process(1, None)));

    state.clear_selection();
    state.apply(state.reconcile(&visible));
    assert_eq!(state.selected, Some(visible[0].key.clone()));
}
