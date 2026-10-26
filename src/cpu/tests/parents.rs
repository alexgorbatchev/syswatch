use super::*;

fn child(pid: u32, ppid: u32, exe: &str, cpu: f32) -> ProcTick {
    let mut p = process(pid, exe, cpu);
    p.ppid = ppid;
    p.scope = ProcessScope::Tool;
    p.mem_rss = u64::from(pid);
    p.start_time = Some(SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(u64::from(pid)));
    p
}

fn expand_all<'a>(
    snap: &'a Snapshot,
    mode: GroupMode,
    grouped: &[CpuGroup<'a>],
) -> Vec<CpuRow<'a>> {
    let mut state = CpuState::default();
    loop {
        let visible = rows(snap, mode, grouped, &state);
        let before = state.expanded.len();
        for row in &visible {
            if row.process.is_none() {
                state.expanded.insert(row.key.clone());
            }
        }
        if before == state.expanded.len() {
            return visible;
        }
    }
}

#[test]
fn owning_tool_includes_descendants_and_keeps_unrelated_runtimes_separate() {
    let mut pi = child(100, 0, "/bin/node", 8.0);
    pi.name = "pi".into();
    let mut other_pi = child(200, 0, "/other/node", 8.0);
    other_pi.name = "pi".into();
    other_pi.user = "other".into();
    let snap = snapshot(vec![
        pi,
        child(101, 100, "/bin/node", 80.0),
        child(102, 101, "/bin/node", 40.0),
        child(103, 102, "/bin/git", 24.0),
        other_pi,
        child(201, 200, "/bin/node", 8.0),
        child(300, 0, "/bin/node", 8.0),
    ]);
    let grouped = groups(&snap, GroupMode::Application);
    assert_eq!(grouped.len(), 2);
    let pi = grouped.iter().find(|g| g.label == "pi").unwrap();
    assert_eq!(pi.members.len(), 6);
    assert_eq!(pi.cpu_pct, Some(21.0));
    let node = grouped.iter().find(|g| g.label == "node").unwrap();
    assert_eq!(
        node.members.iter().map(|p| p.pid).collect::<Vec<_>>(),
        [300]
    );
    let visible = expand_all(&snap, GroupMode::Application, &grouped);
    let helpers: Vec<_> = visible
        .iter()
        .filter(|r| r.depth > 0 && r.process.is_none())
        .collect();
    assert_eq!(helpers.len(), 1);
    let node = helpers.iter().find(|r| r.label == "node").unwrap();
    assert_eq!((node.depth, node.count, node.cpu_pct), (1, 4, Some(19.0)));
    assert_eq!(node.mem_rss, 101 + 102 + 103 + 201);
    let git = visible.iter().find(|r| r.label == "git").unwrap();
    assert_eq!((git.depth, git.count), (2, 1));
    assert_eq!(git.process.unwrap().pid, 103);
    let mut leaves: Vec<_> = visible
        .iter()
        .filter_map(|r| r.process.map(|p| p.pid))
        .collect();
    leaves.sort_unstable();
    assert_eq!(leaves, [100, 101, 102, 103, 200, 201, 300]);
    assert_eq!(
        visible
            .iter()
            .filter(|r| r.depth == 0)
            .map(|r| r.mem_rss)
            .sum::<u64>(),
        snap.procs.iter().map(|p| p.mem_rss).sum::<u64>()
    );
    assert_eq!(
        GroupTotals::new(&snap).application.values().sum::<f32>(),
        22.0
    );
}

#[test]
fn session_launchers_do_not_own_the_tools_they_launch_but_tool_shells_are_transparent() {
    let mut procs = vec![
        child(100, 0, "/lib/systemd/systemd", 8.0),
        child(101, 100, "/bin/sshd", 8.0),
        child(102, 101, "/bin/bash", 8.0),
        child(103, 102, "/bin/tmux", 8.0),
        child(104, 103, "/bin/zsh", 8.0),
        child(105, 104, "/bin/pi", 8.0),
        child(106, 105, "/bin/sh", 8.0),
        child(107, 106, "/bin/node", 8.0),
        child(108, 104, "/bin/postgres", 8.0),
        child(109, 108, "/bin/postgres", 8.0),
        child(110, 100, "/bin/redis-server", 8.0),
        child(111, 100, "/bin/dockerd", 8.0),
        child(112, 100, "/bin/qemu-system-x86_64", 8.0),
    ];
    procs[0].scope = ProcessScope::System;
    procs[1].scope = ProcessScope::System;
    procs[6].cmd = "/bin/sh -c node helper.js".into();
    let snap = snapshot(procs);
    let grouped = groups(&snap, GroupMode::Application);
    let pi = grouped.iter().find(|g| g.label == "pi").unwrap();
    let mut members: Vec<_> = pi.members.iter().map(|p| p.pid).collect();
    members.sort_unstable();
    assert_eq!(members, [105, 106, 107]);
    for name in [
        "bash",
        "zsh",
        "tmux",
        "systemd",
        "sshd",
        "redis-server",
        "Docker",
        "qemu-system-x86_64",
    ] {
        assert_eq!(
            grouped
                .iter()
                .find(|g| g.label == name)
                .unwrap()
                .members
                .len(),
            1,
            "{name}"
        );
    }
    assert_eq!(
        grouped
            .iter()
            .find(|g| g.label == "postgres")
            .unwrap()
            .members
            .len(),
        2
    );
    assert!(grouped
        .iter()
        .rev()
        .take(2)
        .all(|g| system_only(&g.members)));
    assert_eq!(
        grouped.iter().map(|g| g.members.len()).sum::<usize>(),
        snap.procs.len()
    );
}

#[test]
fn command_prompt_shells_separate_tools_from_their_session_launcher() {
    let mut procs = vec![
        child(100, 0, "/bin/herdr", 8.0),
        child(101, 100, "/bin/zsh", 8.0),
        child(102, 101, "/bin/pi", 8.0),
        child(103, 102, "/bin/zsh", 8.0),
        child(104, 103, "/bin/node", 8.0),
        child(105, 101, "/bin/claude", 8.0),
    ];
    procs[3].cmd = "/bin/zsh -lc node helper.js".into();
    procs[1].cmd = "/bin/zsh".into();
    let snap = snapshot(procs);
    let grouped = groups(&snap, GroupMode::Application);
    for (name, expected) in [
        ("herdr", vec![100]),
        ("zsh", vec![101]),
        ("pi", vec![102, 103, 104]),
        ("claude", vec![105]),
    ] {
        let group = grouped.iter().find(|g| g.label == name).unwrap();
        let mut members: Vec<_> = group.members.iter().map(|p| p.pid).collect();
        members.sort_unstable();
        assert_eq!(members, expected, "{name}");
    }
    assert_eq!(
        expand_all(&snap, GroupMode::Application, &grouped)
            .iter()
            .filter(|r| r.process.is_some())
            .count(),
        6
    );
    // Login-style argv[0] and unavailable command lines are also boundaries.
    for cmd in ["-zsh", "zsh", ""] {
        let mut login = snap.clone();
        login.procs[1].cmd = cmd.into();
        let grouped = groups(&login, GroupMode::Application);
        assert_eq!(
            grouped
                .iter()
                .find(|g| g.label == "pi")
                .unwrap()
                .members
                .len(),
            3
        );
    }
}

#[test]
fn missing_parents_newer_parents_and_cycles_never_invent_ownership() {
    let snap = snapshot(vec![
        child(100, 999, "/bin/orphan", 8.0),
        child(101, 200, "/bin/old-child", 8.0),
        child(200, 0, "/bin/reused-pid", 8.0),
        child(300, 300, "/bin/self-cycle", 8.0),
        child(400, 401, "/bin/cycle-a", 8.0),
        child(401, 400, "/bin/cycle-b", 8.0),
    ]);
    assert_eq!(groups(&snap, GroupMode::Application).len(), 6);
    let mut cycle = snap.clone();
    cycle.procs[4].start_time = None;
    cycle.procs[5].start_time = None;
    for procs in [cycle.procs.clone(), cycle.procs.into_iter().rev().collect()] {
        let snap = snapshot(procs);
        let grouped = groups(&snap, GroupMode::Application);
        assert_eq!(grouped.len(), 6);
        assert_eq!(
            expand_all(&snap, GroupMode::Application, &grouped)
                .iter()
                .filter(|r| r.process.is_some())
                .count(),
            6
        );
    }
}

#[test]
fn project_view_places_helpers_by_their_own_checkout_or_missing_location() {
    let mut owner = child(100, 0, "/bin/pi", 8.0);
    owner.project = Some(project("github.com", "/work/main"));
    let mut different = child(101, 100, "/bin/node", 80.0);
    different.project = Some(ProjectIdentity {
        host: "github.com".into(),
        repo: "other/helper".into(),
        checkout: "/work/helpers".into(),
    });
    let missing = child(102, 101, "/bin/git", 8.0);
    let mut another = child(200, 0, "/bin/pi", 8.0);
    another.project = Some(project("github.com", "/work/feature"));
    another.cwd = Some("/work/feature/tool".into());
    let mut same_checkout = child(201, 200, "/bin/node", 8.0);
    same_checkout.project = another.project.clone();
    same_checkout.cwd = Some("/work/feature/helpers".into());
    let snap = snapshot(vec![owner, different, missing, another, same_checkout]);
    let grouped = groups(&snap, GroupMode::Project);
    assert_eq!(grouped.len(), 3);
    let main = grouped.iter().find(|g| g.label == "acme/web").unwrap();
    assert_eq!(
        (main.label.as_str(), main.members.len(), main.cpu_pct),
        ("acme/web", 3, Some(3.0))
    );
    let visible = expand_all(&snap, GroupMode::Project, &grouped);
    let checkouts: Vec<_> = visible
        .iter()
        .filter(|r| r.depth == 1 && r.group == main.key)
        .collect();
    assert_eq!(checkouts.len(), 2);
    assert_eq!(checkouts.iter().map(|r| r.count).sum::<usize>(), 3);
    let apps: Vec<_> = visible.iter().filter(|r| r.depth == 2).collect();
    assert_eq!(
        apps.iter().map(|r| r.label.as_str()).collect::<Vec<_>>(),
        ["node", "pi", "pi"]
    );
    let helper = visible
        .iter()
        .find(|r| r.process.is_some_and(|p| p.pid == 101))
        .unwrap();
    assert_eq!((helper.depth, helper.label.as_str()), (2, "node"));
    assert_eq!(helper.owner.unwrap().pid, 100);
    let missing = visible
        .iter()
        .find(|r| r.process.is_some_and(|p| p.pid == 102))
        .unwrap();
    assert_eq!(missing.label, "git");
    let mut pids: Vec<_> = visible
        .iter()
        .filter_map(|r| r.process.map(|p| p.pid))
        .collect();
    pids.sort_unstable();
    assert_eq!(pids, [100, 101, 102, 200, 201]);
}

#[test]
fn ownership_updates_on_reparenting_without_rewriting_past_totals_or_selection_keys() {
    let mut snap = snapshot(vec![
        child(100, 0, "/bin/pi", 8.0),
        child(101, 100, "/bin/node", 80.0),
    ]);
    let before = GroupTotals::new(&snap);
    let grouped = groups(&snap, GroupMode::Application);
    let key = grouped
        .iter()
        .find(|g| g.label == "pi")
        .unwrap()
        .key
        .clone();
    let selected = RowKey::Process(101, snap.procs[1].start_time);
    assert_eq!(before.get(GroupMode::Application, &key), 11.0);
    snap.procs[1].ppid = 0;
    let grouped = groups(&snap, GroupMode::Application);
    let expanded = expand_all(&snap, GroupMode::Application, &grouped);
    let state = CpuState {
        selected: Some(selected.clone()),
        ..Default::default()
    };
    assert_eq!(expanded[selected_index(&expanded, &state)].key, selected);
    assert_eq!(
        GroupTotals::new(&snap).get(GroupMode::Application, &key),
        1.0
    );
    assert_eq!(before.get(GroupMode::Application, &key), 11.0);
}

#[test]
fn helper_groups_keep_expansion_and_selection_when_cpu_moves_within_a_band() {
    let mut snap = snapshot(vec![
        child(100, 0, "/bin/pi", 0.0),
        child(101, 100, "/bin/zulu", 112.0),
        child(102, 100, "/bin/alpha", 80.0),
        child(200, 0, "/bin/claude", 0.0),
        child(201, 200, "/bin/alpha", 80.0),
        child(103, 100, "/bin/alpha", 0.0),
        child(104, 100, "/bin/zulu", 0.0),
        child(202, 200, "/bin/alpha", 0.0),
    ]);
    let grouped = groups(&snap, GroupMode::Application);
    let mut state = CpuState::default();
    for group in &grouped {
        state.expanded.insert(RowKey::Group(group.key.clone()));
    }
    let initial = rows(&snap, GroupMode::Application, &grouped, &state);
    let helpers: Vec<_> = initial
        .iter()
        .filter(|r| r.depth == 1 && r.process.is_none())
        .collect();
    assert_eq!(
        helpers.iter().map(|r| r.label.as_str()).collect::<Vec<_>>(),
        ["alpha", "zulu", "alpha"]
    );
    assert_ne!(helpers[0].key, helpers[2].key);
    let selected = helpers[0].key.clone();
    state.selected = Some(selected.clone());
    state.expanded.insert(selected.clone());
    snap.procs[1].cpu_pct = 80.0;
    snap.procs[2].cpu_pct = 112.0;
    let grouped = groups(&snap, GroupMode::Application);
    let later = rows(&snap, GroupMode::Application, &grouped, &state);
    assert_eq!(later[selected_index(&later, &state)].key, selected);
    assert_eq!(
        later
            .iter()
            .filter_map(|r| r.process.map(|p| p.pid))
            .collect::<Vec<_>>(),
        [102, 103, 100, 200]
    );
}

#[test]
fn chains_with_repeated_application_names_preserve_each_pid_once() {
    let snap = snapshot(vec![
        child(100, 0, "/bin/pi", 8.0),
        child(101, 100, "/bin/node", 8.0),
        child(102, 101, "/bin/pi", 8.0),
        child(103, 102, "/bin/node", 8.0),
    ]);
    let grouped = groups(&snap, GroupMode::Application);
    assert_eq!(grouped.len(), 1);
    let visible = expand_all(&snap, GroupMode::Application, &grouped);
    let helpers: Vec<_> = visible
        .iter()
        .filter(|r| r.depth > 0 && r.process.is_none())
        .collect();
    assert_eq!(
        helpers
            .iter()
            .map(|r| (r.label.as_str(), r.depth, r.count))
            .collect::<Vec<_>>(),
        [("node", 1, 3), ("pi", 2, 2)]
    );
    let last = visible
        .iter()
        .find(|r| r.process.is_some_and(|p| p.pid == 103))
        .unwrap();
    assert_eq!(
        (last.label.as_str(), last.depth, last.count),
        ("node", 3, 1)
    );
    let mut pids: Vec<_> = visible
        .iter()
        .filter_map(|r| r.process.map(|p| p.pid))
        .collect();
    pids.sort_unstable();
    assert_eq!(pids, [100, 101, 102, 103]);
}

#[test]
fn a_live_helper_does_not_remove_the_deleted_marker_from_its_owner() {
    let snap = snapshot(vec![
        child(100, 0, "/bin/pi (deleted)", 8.0),
        child(101, 100, "/bin/node", 8.0),
    ]);
    let grouped = groups(&snap, GroupMode::Application);
    assert_eq!(grouped[0].label, "pi (deleted)");
    let mut titled = child(200, 0, "/bin/node (deleted)", 8.0);
    titled.name = "pi".into();
    assert_eq!(application(&titled).1, "pi (deleted)");
    assert_eq!(application(&titled).2, '\u{e719}');
}

#[test]
fn runtime_process_titles_identify_tools_without_matching_command_substrings() {
    let mut p = child(100, 0, "/bin/node", 0.0);
    p.name = "pi".into();
    assert_eq!(application(&p).1, "pi");
    assert_eq!(application(&p).2, '\u{e719}');
    p.name = "MainThread".into();
    p.cmd = "node --eval 'console.log(\"pi\")'".into();
    assert_eq!(application(&p).1, "node");
    p.exe = Some("/bin/not-node".into());
    p.name = "pi".into();
    assert_eq!(application(&p).1, "not-node");
}
