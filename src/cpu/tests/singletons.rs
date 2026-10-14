use super::*;

fn checked_out(pid: u32, exe: &str, checkout: &str) -> ProcTick {
    let mut p = process(pid, exe, 80.0);
    p.cwd = Some(checkout.into());
    p.project = Some(project("github.com", checkout));
    p.mem_rss = 4096;
    p
}

fn expand_all<'a>(snap: &'a Snapshot, mode: GroupMode) -> Vec<CpuRow<'a>> {
    let grouped = groups(snap, mode);
    let mut state = CpuState::default();
    loop {
        let visible = rows(snap, mode, &grouped, &state);
        let before = state.expanded.len();
        state.expanded.extend(
            visible
                .iter()
                .filter(|r| r.process.is_none())
                .map(|r| r.key.clone()),
        );
        if state.expanded.len() == before {
            return visible;
        }
    }
}

#[test]
fn singleton_application_is_a_direct_pid_row() {
    let snap = snapshot(vec![checked_out(1161583, "/bin/yazi", "/work/main")]);
    let visible = expand_all(&snap, GroupMode::Application);
    assert_eq!(visible.len(), 1);
    let row = &visible[0];
    assert_eq!(row.process.map(|p| p.pid), Some(1161583));
    assert_eq!(row.key, RowKey::Process(1161583, None));
    assert_eq!(
        (row.depth, row.count, row.cpu_pct, row.mem_rss),
        (0, 1, Some(10.0), 4096)
    );
    assert_eq!(row.label, "yazi");
}

#[test]
fn singleton_project_keeps_separate_expandable_checkout_and_process_rows() {
    let snap = snapshot(vec![checked_out(1161583, "/bin/yazi", "/work/main")]);
    let grouped = groups(&snap, GroupMode::Project);
    let mut state = CpuState::default();
    let initial = rows(&snap, GroupMode::Project, &grouped, &state);
    assert_eq!(initial.len(), 1);
    assert!(initial[0].process.is_none());
    assert_eq!(initial[0].label, "acme/web");
    assert_eq!(
        (initial[0].count, initial[0].cpu_pct, initial[0].mem_rss),
        (1, Some(10.0), 4096)
    );
    state.expanded.insert(initial[0].key.clone());
    let project = rows(&snap, GroupMode::Project, &grouped, &state);
    assert_eq!(project.len(), 2);
    assert_eq!(project[1].label, "/work/main");
    assert!(project[1].process.is_none());
    state.expanded.insert(project[1].key.clone());
    let checkout = rows(&snap, GroupMode::Project, &grouped, &state);
    assert_eq!(checkout.len(), 3);
    assert_eq!(checkout[2].label, "yazi");
    assert_eq!(
        (checkout[2].depth, checkout[2].process.unwrap().pid),
        (2, 1161583)
    );
    assert_eq!(checkout[2].key, RowKey::Process(1161583, None));
}

#[test]
fn singleton_directory_keeps_its_process_beneath_an_expandable_folder() {
    let mut p = checked_out(100, "/bin/yazi", "/work/scratch");
    p.project = None;
    let snap = snapshot(vec![p]);
    let grouped = groups(&snap, GroupMode::Project);
    let initial = rows(&snap, GroupMode::Project, &grouped, &CpuState::default());
    assert_eq!(initial.len(), 1);
    assert_eq!(initial[0].label, "/work/scratch");
    assert!(initial[0].process.is_none());
    let visible = expand_all(&snap, GroupMode::Project);
    assert_eq!(visible.len(), 2);
    assert_eq!(
        (
            visible[1].label.as_str(),
            visible[1].depth,
            visible[1].process.unwrap().pid
        ),
        ("yazi", 1, 100)
    );
}

#[test]
fn singleton_checkouts_keep_their_paths_and_separate_process_rows() {
    let snap = snapshot(vec![
        checked_out(100, "/bin/yazi", "/work/main"),
        checked_out(200, "/bin/node", "/work/feature"),
    ]);
    let visible = expand_all(&snap, GroupMode::Project);
    assert_eq!(visible.len(), 5);
    assert!(visible[0].process.is_none());
    let checkouts: Vec<_> = visible.iter().filter(|r| r.depth == 1).collect();
    assert!(checkouts.iter().all(|r| r.process.is_none()));
    assert_eq!(
        checkouts
            .iter()
            .map(|r| r.label.as_str())
            .collect::<Vec<_>>(),
        ["/work/feature", "/work/main"]
    );
    assert_eq!(
        visible
            .iter()
            .filter_map(|r| r.process.map(|p| (r.depth, p.pid)))
            .collect::<Vec<_>>(),
        [(2, 200), (2, 100)]
    );
}

#[test]
fn singleton_applications_and_helpers_do_not_add_duplicate_rows() {
    let mut owner = checked_out(100, "/bin/pi", "/work/main");
    owner.name = "pi".into();
    let mut helper = process(101, "/bin/node", 160.0);
    helper.name = "MainThread".into();
    helper.ppid = 100;
    helper.cwd = owner.cwd.clone();
    helper.project = owner.project.clone();
    let snap = snapshot(vec![
        owner,
        helper,
        checked_out(200, "/bin/yazi", "/work/main"),
    ]);
    let visible = expand_all(&snap, GroupMode::Project);
    let yazi: Vec<_> = visible.iter().filter(|r| r.label == "yazi").collect();
    assert_eq!(yazi.len(), 1);
    assert_eq!((yazi[0].depth, yazi[0].process.unwrap().pid), (2, 200));
    let node: Vec<_> = visible.iter().filter(|r| r.label == "node").collect();
    assert_eq!(node.len(), 1);
    assert_eq!((node[0].depth, node[0].process.unwrap().pid), (3, 101));
    assert_eq!(node[0].owner.unwrap().pid, 100);
    let mut pids: Vec<_> = visible
        .iter()
        .filter_map(|r| r.process.map(|p| p.pid))
        .collect();
    pids.sort_unstable();
    assert_eq!(pids, [100, 101, 200]);
    assert!(visible
        .iter()
        .filter(|r| r.process.is_none())
        .all(|r| r.count > 1));
}
