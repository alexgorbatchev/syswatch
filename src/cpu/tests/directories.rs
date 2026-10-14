use super::*;

fn in_directory(pid: u32, exe: &str, cwd: Option<&str>, cpu: f32) -> ProcTick {
    let mut p = process(pid, exe, cpu);
    p.cwd = cwd.map(str::to_owned);
    p.mem_rss = 1024 * u64::from(pid);
    p
}

#[test]
fn directory_groups_combine_different_applications_and_owners() {
    let mut other_user = in_directory(2, "/bin/node", Some("/work/scratch"), 160.0);
    other_user.user = "other".into();
    let snap = snapshot(vec![
        in_directory(1, "/bin/pi", Some("/work/scratch"), 80.0),
        other_user,
        in_directory(3, "/bin/node", Some("/work/another"), 8.0),
    ]);
    let grouped = groups(&snap, GroupMode::Project);
    assert_eq!(grouped.len(), 2);
    let directory = grouped.iter().find(|g| g.label == "/work/scratch").unwrap();
    assert_eq!(
        (directory.members.len(), directory.cpu_pct),
        (2, Some(30.0))
    );
    let history = GroupTotals::new(&snap);
    assert_eq!(history.get(GroupMode::Project, &directory.key), 30.0);
    let mut state = CpuState::default();
    state.expanded.insert(RowKey::Group(directory.key.clone()));
    let visible = rows(&snap, GroupMode::Project, &grouped, &state);
    let applications: Vec<_> = visible.iter().filter(|r| r.depth == 1).collect();
    assert_eq!(
        applications
            .iter()
            .map(|r| r.label.as_str())
            .collect::<Vec<_>>(),
        ["node", "pi"]
    );
    assert!(applications.iter().all(|r| r.process.is_some()));
    assert_eq!(applications.iter().map(|r| r.mem_rss).sum::<u64>(), 3072);
    let root = visible.iter().find(|r| r.label == "/work/scratch").unwrap();
    assert_eq!(root.mem_rss, 3072);
    let single = visible.iter().find(|r| r.label == "/work/another").unwrap();
    assert_eq!(single.depth, 0);
    assert!(single.process.is_none());
    state.expanded.insert(single.key.clone());
    let visible = rows(&snap, GroupMode::Project, &grouped, &state);
    let process = visible
        .iter()
        .find(|r| r.process.is_some_and(|p| p.pid == 3))
        .unwrap();
    assert_eq!((process.depth, process.label.as_str()), (1, "node"));
    // Application mode remains focused on owning tools across directories.
    let apps = groups(&snap, GroupMode::Application);
    assert_eq!(apps.len(), 2);
    assert_eq!(
        apps.iter()
            .find(|g| g.label == "node")
            .unwrap()
            .members
            .len(),
        2
    );
}

#[test]
fn empty_and_missing_directories_fall_back_to_applications() {
    let snap = snapshot(vec![
        in_directory(1, "/bin/node", None, 8.0),
        in_directory(2, "/bin/node", Some(""), 8.0),
        in_directory(3, "/bin/node", Some("/work/known"), 8.0),
    ]);
    let grouped = groups(&snap, GroupMode::Project);
    assert_eq!(grouped.len(), 2);
    let unknown = grouped.iter().find(|g| g.label == "node").unwrap();
    assert_eq!(unknown.members.len(), 2);
    let known = grouped.iter().find(|g| g.label == "/work/known").unwrap();
    assert_eq!(known.members[0].pid, 3);
}

#[test]
fn directory_groups_keep_system_last_and_sort_by_cpu_band_then_path() {
    let mut procs = vec![
        in_directory(1, "/bin/node", Some("/work/zulu"), 80.0),
        in_directory(2, "/bin/pi", Some("/work/alpha"), 112.0),
        in_directory(3, "/bin/daemon", Some("/system/busy"), 400.0),
        in_directory(4, "/bin/node", Some("/mixed"), 0.0),
        in_directory(5, "/bin/daemon", Some("/mixed"), 0.0),
    ];
    for p in &mut procs {
        p.scope = if matches!(p.pid, 3 | 5) {
            ProcessScope::System
        } else {
            ProcessScope::Tool
        };
    }
    let snap = snapshot(procs);
    let grouped = groups(&snap, GroupMode::Project);
    assert_eq!(
        grouped.iter().map(|g| g.label.as_str()).collect::<Vec<_>>(),
        ["/work/alpha", "/work/zulu", "/mixed", "/system/busy"]
    );
    assert!(!system_only(&grouped[2].members));
    assert!(system_only(&grouped[3].members));
}

#[test]
fn changing_directories_updates_current_groups_without_rewriting_history() {
    let first = snapshot(vec![
        in_directory(1, "/bin/node", Some("/work/first"), 80.0),
        in_directory(2, "/bin/node", Some("/work/first"), 8.0),
    ]);
    let before = groups(&first, GroupMode::Project);
    let history = GroupTotals::new(&first);
    let mut second = first.clone();
    second.procs[1].cwd = Some("/work/second".into());
    let after = groups(&second, GroupMode::Project);
    assert_eq!((before.len(), after.len()), (1, 2));
    assert_eq!(history.get(GroupMode::Project, &before[0].key), 11.0);
    let moved = after.iter().find(|g| g.label == "/work/second").unwrap();
    assert_eq!(history.get(GroupMode::Project, &moved.key), 0.0);
    assert_eq!(
        GroupTotals::new(&second).get(GroupMode::Project, &before[0].key),
        10.0
    );
    assert_eq!(moved.members[0].pid, 2);
}

#[test]
fn git_projects_combine_working_subdirectories_under_the_checkout() {
    let mut procs = vec![
        in_directory(1, "/bin/node", Some("/work/main/src"), 8.0),
        in_directory(2, "/bin/git", Some("/work/main/src"), 8.0),
        in_directory(3, "/bin/yazi", Some("/work/main/docs"), 8.0),
        in_directory(4, "/bin/pi", None, 8.0),
    ];
    for p in &mut procs {
        p.project = Some(project("github.com", "/work/main"));
    }
    procs[1].user = "other".into();
    let snap = snapshot(procs);
    let grouped = groups(&snap, GroupMode::Project);
    assert_eq!(
        (
            grouped.len(),
            grouped[0].label.as_str(),
            grouped[0].members.len()
        ),
        (1, "acme/web", 4)
    );
    let mut state = CpuState::default();
    state.expanded.insert(RowKey::Group(grouped[0].key.clone()));
    let visible = rows(&snap, GroupMode::Project, &grouped, &state);
    let directories: Vec<_> = visible.iter().filter(|r| r.depth == 1).collect();
    assert_eq!(
        directories
            .iter()
            .map(|r| r.label.as_str())
            .collect::<Vec<_>>(),
        ["/work/main"]
    );
    assert_eq!(directories.iter().map(|r| r.count).sum::<usize>(), 4);
    assert_eq!(directories[0].cpu_pct, Some(4.0));
    state.expanded.insert(directories[0].key.clone());
    let visible = rows(&snap, GroupMode::Project, &grouped, &state);
    assert_eq!(
        visible
            .iter()
            .filter(|r| r.depth == 2)
            .map(|r| r.label.as_str())
            .collect::<Vec<_>>(),
        ["git", "node", "pi", "yazi"]
    );
    assert_eq!(visible.iter().filter(|r| r.process.is_some()).count(), 4);
}

#[test]
fn helpers_use_their_own_directories_and_preserve_ancestry_inside_a_directory() {
    let owner = in_directory(100, "/bin/pi", Some("/work/tool"), 8.0);
    let mut helper = in_directory(101, "/bin/node", Some("/work/helper"), 80.0);
    helper.ppid = 100;
    let mut nested = in_directory(102, "/bin/git", Some("/work/helper"), 8.0);
    nested.ppid = 101;
    let mut unknown = in_directory(103, "/bin/python", None, 8.0);
    unknown.ppid = 100;
    let snap = snapshot(vec![owner, helper, nested, unknown]);
    let grouped = groups(&snap, GroupMode::Project);
    assert_eq!(grouped.len(), 3);
    let directory = grouped.iter().find(|g| g.label == "/work/helper").unwrap();
    assert_eq!(
        (directory.members.len(), directory.cpu_pct),
        (2, Some(11.0))
    );
    let mut state = CpuState::default();
    state.expanded.insert(RowKey::Group(directory.key.clone()));
    let visible = rows(&snap, GroupMode::Project, &grouped, &state);
    let apps: Vec<_> = visible.iter().filter(|r| r.depth == 1).collect();
    assert_eq!(apps.len(), 1);
    assert_eq!((apps[0].label.as_str(), apps[0].count), ("node", 2));
    state.expanded.insert(apps[0].key.clone());
    let visible = rows(&snap, GroupMode::Project, &grouped, &state);
    let leaf = visible
        .iter()
        .find(|r| r.process.is_some_and(|p| p.pid == 101))
        .unwrap();
    assert_eq!(leaf.owner.unwrap().pid, 100);
    let missing = grouped.iter().find(|g| g.label == "python").unwrap();
    assert_eq!(missing.members[0].pid, 103);
    let normal = groups(&snap, GroupMode::Application);
    assert_eq!(
        (
            normal.len(),
            normal[0].label.as_str(),
            normal[0].members.len()
        ),
        (1, "pi", 4)
    );
}

#[test]
fn application_expansion_is_independent_in_each_directory_and_survives_reordering() {
    let mut snap = snapshot(vec![
        in_directory(1, "/bin/node", Some("/work/alpha"), 80.0),
        in_directory(2, "/bin/node", Some("/work/alpha"), 8.0),
        in_directory(3, "/bin/node", Some("/work/beta"), 80.0),
        in_directory(4, "/bin/node", Some("/work/beta"), 8.0),
    ]);
    let grouped = groups(&snap, GroupMode::Project);
    let mut state = CpuState::default();
    state
        .expanded
        .extend(grouped.iter().map(|g| RowKey::Group(g.key.clone())));
    let visible = rows(&snap, GroupMode::Project, &grouped, &state);
    let apps: Vec<_> = visible.iter().filter(|r| r.depth == 1).collect();
    assert_eq!(apps.len(), 2);
    assert_ne!(apps[0].key, apps[1].key);
    let selected = apps[0].key.clone();
    state.expanded.insert(selected.clone());
    state.selected = Some(selected.clone());
    // Move beta into a higher CPU band so alpha's selected app changes position.
    snap.procs[2].cpu_pct = 160.0;
    let grouped = groups(&snap, GroupMode::Project);
    let later = rows(&snap, GroupMode::Project, &grouped, &state);
    assert_eq!(later[selected_index(&later, &state)].key, selected);
    assert_eq!(later[0].label, "/work/beta");
    let leaves: Vec<_> = later
        .iter()
        .filter_map(|r| r.process.map(|p| p.pid))
        .collect();
    assert_eq!(leaves, [1, 2]);
}

#[test]
fn home_directories_sort_before_alphabetic_names_within_each_cpu_band() {
    let home = dirs::home_dir().unwrap();
    let mut procs = vec![
        in_directory(1, "/bin/node", home.to_str(), 8.0),
        in_directory(2, "/bin/node", home.join("alpha").to_str(), 8.0),
        in_directory(3, "/bin/node", home.join("Zulu").to_str(), 8.0),
        in_directory(4, "/bin/node", Some("/work/main"), 8.0),
        in_directory(5, "/bin/node", Some("/work/busy"), 80.0),
        in_directory(6, "/bin/daemon", home.join("system").to_str(), 400.0),
    ];
    procs[3].project = Some(project("github.com", "/work/main"));
    procs[4].project = Some(project("github.com", "/work/busy"));
    procs[4].project.as_mut().unwrap().repo = "busy/repo".into();
    procs[5].scope = ProcessScope::System;
    let snap = snapshot(procs);
    let grouped = groups(&snap, GroupMode::Project);
    assert_eq!(
        grouped.iter().map(|g| g.label.as_str()).collect::<Vec<_>>(),
        [
            "busy/repo",
            "~",
            "~/alpha",
            "~/Zulu",
            "acme/web",
            "~/system"
        ]
    );
}

#[test]
fn checkout_sorting_uses_displayed_home_paths_and_cpu_bands() {
    let home = dirs::home_dir().unwrap().join("checkout");
    let home = home.to_str().unwrap();
    let mut procs = vec![
        in_directory(1, "/bin/node", Some(home), 8.0),
        in_directory(2, "/bin/node", Some("/a-checkout"), 8.0),
    ];
    procs[0].project = Some(project("github.com", home));
    procs[1].project = Some(project("github.com", "/a-checkout"));
    let mut snap = snapshot(procs);
    let grouped = groups(&snap, GroupMode::Project);
    let mut state = CpuState::default();
    state.expanded.insert(RowKey::Group(grouped[0].key.clone()));
    let visible = rows(&snap, GroupMode::Project, &grouped, &state);
    let paths = |rows: &[CpuRow<'_>]| {
        rows.iter()
            .filter(|r| r.depth == 1)
            .map(|r| r.label.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(paths(&visible), ["~/checkout", "/a-checkout"]);
    snap.procs[1].cpu_pct = 80.0;
    let grouped = groups(&snap, GroupMode::Project);
    let visible = rows(&snap, GroupMode::Project, &grouped, &state);
    assert_eq!(paths(&visible), ["/a-checkout", "~/checkout"]);
}
