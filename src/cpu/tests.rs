use super::*;
use crate::collect::{CpuTick, ProcTick, ProcessScope, ProjectIdentity, Snapshot};

fn process(pid: u32, exe: &str, cpu: f32) -> ProcTick {
    ProcTick {
        pid,
        name: exe.rsplit('/').next().unwrap().into(),
        exe: Some(exe.into()),
        cpu_pct: cpu,
        ..Default::default()
    }
}

fn snapshot(procs: Vec<ProcTick>) -> Snapshot {
    Snapshot {
        cpu: CpuTick {
            per_core: vec![0.0; 8],
            ..Default::default()
        },
        procs,
        ..Default::default()
    }
}

#[test]
fn helpers_combine_across_owners_below_development_services() {
    use std::{ffi::OsStr, path::Path};
    let mut procs = vec![
        process(1, "/usr/lib/systemd/systemd", 240.0),
        process(2, "/usr/lib/systemd/systemd", 240.0),
        process(3, "/usr/sbin/sshd", 8.0),
        process(4, "/usr/sbin/sshd", 8.0),
        process(5, "/usr/bin/ssh-agent", 8.0),
        process(6, "/usr/bin/gpg-agent", 8.0),
        process(7, "/usr/lib/postgresql/16/bin/postgres", 8.0),
        process(8, "/usr/bin/redis-server", 8.0),
        process(9, "/usr/bin/dockerd", 8.0),
        process(10, "/usr/bin/qemu-system-x86_64", 8.0),
    ];
    for p in &mut procs {
        p.user = if p.pid % 2 == 0 { "alex" } else { "root" }.into();
        p.scope = crate::collect::process_scope::classify(
            p.exe.as_deref().map(Path::new),
            OsStr::new(&p.name),
        );
    }
    let snap = snapshot(procs);
    for mode in [GroupMode::Application, GroupMode::Project] {
        let grouped = groups(&snap, mode);
        assert_eq!(
            grouped.iter().map(|g| g.label.as_str()).collect::<Vec<_>>(),
            [
                "dockerd",
                "postgres",
                "qemu-system-x86_64",
                "redis-server",
                "systemd",
                "gpg-agent",
                "ssh-agent",
                "sshd"
            ]
        );
        for name in ["systemd", "sshd"] {
            let group = grouped.iter().find(|g| g.label == name).unwrap();
            assert_eq!(group.members.len(), 2);
            assert!(system_only(&group.members));
        }
    }
}

#[test]
fn system_only_groups_follow_other_groups_before_cpu_banding() {
    let mut procs = vec![
        process(1, "/bin/user-zulu", 8.0),
        process(2, "/bin/user-alpha", 32.0),
        process(3, "/bin/user-busy", 80.0),
        process(4, "/bin/system-zulu", 240.0),
        process(5, "/bin/system-alpha", 272.0),
        process(6, "/bin/system-busy", 400.0),
        process(7, "/bin/node", 0.0),
        process(8, "/bin/node", 0.0),
        process(9, "/bin/unknown", 0.0),
        process(10, "/bin/dbus-daemon", 0.0),
        process(11, "/bin/dbus-daemon", 0.0),
    ];
    for p in &mut procs {
        p.scope = match p.pid {
            4..=7 | 10 => ProcessScope::System,
            9 | 11 => ProcessScope::Unknown,
            _ => ProcessScope::Tool,
        };
        // Account names cannot determine system/user classification.
        p.user = if p.scope == ProcessScope::System {
            "daemon"
        } else {
            "root"
        }
        .into();
    }
    let snap = snapshot(procs);
    for mode in [GroupMode::Application, GroupMode::Project] {
        let grouped = groups(&snap, mode);
        assert_eq!(
            grouped.iter().map(|g| g.label.as_str()).collect::<Vec<_>>(),
            [
                "user-busy",
                "dbus-daemon",
                "node",
                "unknown",
                "user-alpha",
                "user-zulu",
                "system-busy",
                "system-alpha",
                "system-zulu"
            ]
        );
        let node = grouped.iter().find(|g| g.label == "node").unwrap();
        let dbus = grouped.iter().find(|g| g.label == "dbus-daemon").unwrap();
        assert_eq!(dbus.members.len(), 2);
        assert!(!system_only(&dbus.members));
        assert_eq!(node.members.len(), 2);
        assert_eq!(
            node.members.iter().map(|p| p.pid).collect::<Vec<_>>(),
            [8, 7]
        );
        let mut state = CpuState::default();
        state.expanded.insert(RowKey::Group(node.key.clone()));
        let leaves: Vec<_> = rows(&snap, &grouped, &state)
            .iter()
            .filter_map(|row| row.process.map(|p| p.pid))
            .collect();
        assert_eq!(leaves, [8, 7]);
    }
}

#[test]
fn system_priority_applies_to_projects_checkouts_and_nested_applications() {
    let mut procs = vec![
        process(1, "/bin/zulu", 8.0),
        process(2, "/bin/alpha", 8.0),
        process(3, "/bin/busy", 400.0),
        process(4, "/bin/service", 480.0),
        process(5, "/bin/project-service", 560.0),
    ];
    for p in &mut procs {
        p.scope = if p.pid >= 3 {
            ProcessScope::System
        } else {
            ProcessScope::Tool
        };
        p.project = Some(project(
            "github.com",
            match p.pid {
                1..=3 => "/work/user",
                4 => "/work/system",
                _ => "/work/separate",
            },
        ));
        if p.pid == 5 {
            p.project.as_mut().unwrap().repo = "acme/system".into();
        }
    }
    let snap = snapshot(procs);
    let grouped = groups(&snap, GroupMode::Project);
    assert_eq!(grouped[0].members.len(), 4);
    assert_eq!(grouped[1].label, "acme/system");
    let mut state = CpuState::default();
    state.expanded.insert(RowKey::Group(grouped[0].key.clone()));
    for row in rows(&snap, &grouped, &state) {
        if row.depth == 1 {
            state.expanded.insert(row.key);
        }
    }
    let expanded = rows(&snap, &grouped, &state);
    assert_eq!(
        expanded
            .iter()
            .filter(|r| r.depth == 1)
            .map(|r| r.label.as_str())
            .collect::<Vec<_>>(),
        ["/work/user", "/work/system"]
    );
    assert_eq!(
        expanded
            .iter()
            .filter(|r| r.depth == 2)
            .map(|r| r.label.as_str())
            .collect::<Vec<_>>(),
        ["alpha", "zulu", "busy", "service"]
    );
    state.selected = Some(expanded[1].key.clone());
    let mut changed = snap.clone();
    changed.procs[0].scope = ProcessScope::System;
    changed.procs[1].scope = ProcessScope::System;
    let changed_groups = groups(&changed, GroupMode::Project);
    let changed_rows = rows(&changed, &changed_groups, &state);
    assert_eq!(
        changed_rows[selected_index(&changed_rows, &state)].key,
        expanded[1].key
    );
}

#[test]
fn claude_native_versions_and_helpers_share_the_application_name() {
    let mut procs = vec![
        process(1, "/home/alex/.local/share/claude/versions/2.1.292", 80.0),
        process(2, "/home/alex/.local/share/claude/versions/2.1.292", 80.0),
        process(3, "/home/other/.local/share/claude/versions/2.1.929", 80.0),
        process(4, "/usr/bin/claude", 80.0),
    ];
    procs[0].name = "claude".into();
    for p in &mut procs {
        p.user = if p.pid == 3 { "other" } else { "alex" }.into();
        p.mem_rss = u64::from(p.pid) * 1024;
    }
    let snap = snapshot(procs);
    for mode in [GroupMode::Application, GroupMode::Project] {
        let grouped = groups(&snap, mode);
        assert_eq!(grouped.len(), 1);
        assert_eq!(grouped[0].label, "claude");
        assert_eq!(grouped[0].members.len(), 4);
        assert_eq!(grouped[0].cpu_pct, Some(40.0));
        assert_eq!(
            rows(&snap, &grouped, &CpuState::default())[0].mem_rss,
            10 * 1024
        );
        assert_eq!(grouped[0].icon, application(&snap.procs[3]).2);
        assert!(grouped[0]
            .members
            .iter()
            .any(|p| p.exe.as_deref() == Some("/home/other/.local/share/claude/versions/2.1.929")));
    }
}

#[test]
fn claude_identification_uses_the_installation_layout_not_substrings() {
    for exe in [
        "/work/not-claude/versions/2.1.292",
        "/work/claude/versions/2.1.292/helper",
        "/bin/2.1.292",
    ] {
        let mut p = process(1, exe, 0.0);
        p.cmd = "claude --version".into();
        assert_ne!(application(&p).1, "claude", "{exe}");
    }
    let p = process(
        1,
        "/home/alex/.local/share/claude/versions/2.1.292 (deleted)",
        0.0,
    );
    assert_eq!(application(&p).1, "claude (deleted)");
}

#[test]
fn cpu_bands_keep_alphabetical_order_until_a_boundary_is_crossed() {
    let mut snap = snapshot(vec![
        process(1, "/bin/alpha", 80.0),
        process(2, "/bin/Zebra", 119.2),
        process(3, "/bin/beta", 120.0),
    ]);
    let labels = |snap: &Snapshot| {
        groups(snap, GroupMode::Application)
            .into_iter()
            .map(|g| g.label)
            .collect::<Vec<_>>()
    };
    assert_eq!(labels(&snap), ["beta", "alpha", "Zebra"]);
    snap.procs[0].cpu_pct = 112.0;
    snap.procs[1].cpu_pct = 81.0;
    assert_eq!(labels(&snap), ["beta", "alpha", "Zebra"]);
    snap.procs[1].cpu_pct = 160.0;
    assert_eq!(labels(&snap), ["Zebra", "beta", "alpha"]);
}

#[test]
fn process_rows_use_machine_cpu_bands_and_keep_selection_after_reordering() {
    let mut a = process(3, "/bin/worker", 80.0);
    a.name = "alpha".into();
    let mut b = process(2, "/bin/worker", 119.2);
    b.name = "Zebra".into();
    let mut snap = snapshot(vec![a, b]);
    let grouped = groups(&snap, GroupMode::Application);
    let mut state = CpuState::default();
    state.expanded.insert(RowKey::Group(grouped[0].key.clone()));
    state.selected = Some(RowKey::Process(2, None));
    let first = rows(&snap, &grouped, &state);
    assert_eq!(
        first
            .iter()
            .filter_map(|r| r.process.map(|p| p.pid))
            .collect::<Vec<_>>(),
        [3, 2]
    );
    assert_eq!(selected_index(&first, &state), 2);
    snap.procs[1].cpu_pct = 120.0;
    let grouped = groups(&snap, GroupMode::Application);
    let second = rows(&snap, &grouped, &state);
    assert_eq!(
        second
            .iter()
            .filter_map(|r| r.process.map(|p| p.pid))
            .collect::<Vec<_>>(),
        [2, 3]
    );
    assert_eq!(selected_index(&second, &state), 1);
}

#[test]
fn chrome_helpers_share_an_application_and_cpu_is_machine_normalized() {
    let snap = snapshot(vec![
        process(1, "/opt/google/chrome/chrome", 160.0),
        process(2, "/opt/google/chrome/chrome", 80.0),
    ]);
    let groups = groups(&snap, GroupMode::Application);
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].label, "Google Chrome");
    assert_eq!(groups[0].cpu_pct, Some(30.0));
    assert_eq!(groups[0].members.len(), 2);
    assert_eq!(groups[0].icon, '\u{f268}');
}

#[test]
fn mac_helpers_use_the_outer_bundle_not_command_substrings() {
    let chrome = process(1, "/Applications/Google Chrome.app/Contents/Frameworks/Google Chrome Framework.framework/Helpers/Google Chrome Helper.app/Contents/MacOS/Google Chrome Helper", 80.0);
    let mut shell = process(2, "/bin/sh", 8.0);
    shell.cmd = "sh -c echo chrome slack".into();
    let snap = snapshot(vec![chrome, shell]);
    let groups = groups(&snap, GroupMode::Application);
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].label, "Google Chrome");
    assert_eq!(groups[1].label, "sh");
}

fn project(host: &str, checkout: &str) -> ProjectIdentity {
    ProjectIdentity {
        host: host.into(),
        repo: "acme/web".into(),
        checkout: checkout.into(),
    }
}

#[test]
fn project_groups_merge_worktrees_but_separate_hosts_and_keep_unattributed_apps() {
    let mut a = process(1, "/bin/node", 160.0);
    a.project = Some(project("github.com", "/work/web"));
    let mut b = process(2, "/bin/node", 80.0);
    b.project = Some(project("github.com", "/work/web-feature"));
    let mut c = process(3, "/bin/node", 40.0);
    c.project = Some(project("git.example.com", "/work/other"));
    let snap = snapshot(vec![a, b, c, process(4, "/opt/google/chrome/chrome", 8.0)]);
    let groups = groups(&snap, GroupMode::Project);
    assert_eq!(groups.len(), 3);
    assert_eq!(groups[0].label, "acme/web");
    assert_eq!(groups[0].cpu_pct, Some(30.0));
    assert_ne!(groups[0].key, groups[1].key);
    assert_eq!(groups[2].label, "Google Chrome");
    assert_eq!(groups.iter().map(|g| g.members.len()).sum::<usize>(), 4);
}

#[test]
fn same_application_names_combine_installations_and_owners() {
    let mut a = process(1, "/one/worker", 8.0);
    a.mem_rss = 1024;
    let b = process(2, "/two/worker", 8.0);
    let mut c = a.clone();
    c.pid = 3;
    c.user = "other".into();
    let snap = snapshot(vec![a, b, c]);
    for mode in [GroupMode::Application, GroupMode::Project] {
        let grouped = groups(&snap, mode);
        assert_eq!(grouped.len(), 1);
        assert_eq!(grouped[0].members.len(), 3);
        assert_eq!(grouped[0].cpu_pct, Some(3.0));
        assert_eq!(rows(&snap, &grouped, &CpuState::default())[0].mem_rss, 2048);
        assert_eq!(GroupTotals::new(&snap).get(mode, &grouped[0].key), 3.0);
    }
    let key = groups(&snap, GroupMode::Application)[0].key.clone();
    let mut later = snap.clone();
    later.procs = vec![process(4, "/third/worker", 16.0)];
    later.procs[0].user = "new-owner".into();
    assert_eq!(groups(&later, GroupMode::Application)[0].key, key);
    assert_eq!(
        GroupTotals::new(&later).get(GroupMode::Application, &key),
        2.0
    );
}

#[test]
fn unavailable_executables_group_same_names_and_unknown_owners_in_both_modes() {
    for name in ["postgres", "sshd"] {
        let mut first = process(1, "/unused", 80.0);
        first.name = name.into();
        first.exe = None;
        first.user = "?".into();
        first.mem_rss = 1024;
        let mut second = first.clone();
        second.pid = 2;
        second.cpu_pct = 160.0;
        second.mem_rss = 2048;
        second.start_time = Some(SystemTime::UNIX_EPOCH);
        let snap = snapshot(vec![first, second]);
        for mode in [GroupMode::Application, GroupMode::Project] {
            let grouped = groups(&snap, mode);
            assert_eq!(grouped.len(), 1, "{name} in {mode:?}");
            assert_eq!(grouped[0].label, name);
            assert_eq!(grouped[0].members.len(), 2);
            assert_eq!(grouped[0].cpu_pct, Some(30.0));
            let mut state = CpuState::default();
            state.expanded.insert(RowKey::Group(grouped[0].key.clone()));
            let expanded = rows(&snap, &grouped, &state);
            assert_eq!(expanded[0].mem_rss, 3072);
            assert_eq!(expanded.len(), 3);
            assert_ne!(expanded[1].key, expanded[2].key);
        }
    }
}

#[test]
fn name_fallback_combines_owners_and_known_paths_but_separates_different_names() {
    let mut procs: Vec<_> = (1..=6)
        .map(|pid| process(pid, "/bin/postgres", 8.0))
        .collect();
    for p in &mut procs[..5] {
        p.exe = None;
        p.user = "postgres".into();
    }
    procs[1].exe = Some(String::new());
    procs[2].user = "?".into();
    procs[3].user = "root".into();
    procs[4].name = "sshd".into();
    procs[5].user = "postgres".into();
    let snap = snapshot(procs);
    let grouped = groups(&snap, GroupMode::Application);
    assert_eq!(grouped.len(), 2);
    let merged = grouped.iter().find(|g| g.label == "postgres").unwrap();
    assert_eq!(
        merged.members.iter().map(|p| p.pid).collect::<Vec<_>>(),
        [1, 2, 3, 4, 6]
    );
    assert_eq!(merged.cpu_pct, Some(5.0));
}

#[test]
fn live_and_deleted_installations_share_one_application() {
    let procs = vec![
        process(1, "/old/node (deleted)", 80.0),
        process(2, "/new/node", 160.0),
    ];
    for procs in [procs.clone(), procs.into_iter().rev().collect()] {
        let snap = snapshot(procs);
        let grouped = groups(&snap, GroupMode::Application);
        assert_eq!(grouped.len(), 1);
        assert_eq!(grouped[0].label, "node");
        assert_eq!(grouped[0].cpu_pct, Some(30.0));
    }
}

#[test]
fn missing_names_and_executables_remain_individual() {
    let snap = snapshot(
        (1..=2)
            .map(|pid| ProcTick {
                pid,
                user: "?".into(),
                ..Default::default()
            })
            .collect(),
    );
    assert_eq!(groups(&snap, GroupMode::Application).len(), 2);
}

#[test]
fn assessment_distinguishes_busy_waiting_and_unavailable() {
    let mut snap = snapshot(vec![]);
    snap.cpu.usage_pct = 60.0;
    assert!(!assessment(&snap).contains("low"));
    snap.cpu.usage_pct = 95.0;
    assert!(assessment(&snap).contains("unavailable"));
    snap.pressure = Some(crate::collect::PressureTick::default());
    assert!(assessment(&snap).contains("little CPU waiting"));
    snap.pressure.as_mut().unwrap().cpu_some = 28.0;
    assert!(assessment(&snap).contains("contention"));
}

#[test]
fn missing_core_count_does_not_invent_a_machine_percentage() {
    let snap = Snapshot {
        procs: vec![process(1, "/bin/node", 80.0)],
        ..Default::default()
    };
    assert_eq!(groups(&snap, GroupMode::Application)[0].cpu_pct, None);
}

#[test]
fn cached_history_preserves_group_totals_at_the_displayed_time() {
    let mut history = crate::app::History::new(3);
    history.push(&snapshot(vec![process(
        1,
        "/opt/google/chrome/chrome",
        240.0,
    )]));
    history.push(&snapshot(vec![process(
        1,
        "/opt/google/chrome/chrome",
        80.0,
    )]));
    let totals = history.cpu_groups.nth_back(1).unwrap();
    assert_eq!(totals.application.values().copied().sum::<f32>(), 30.0);
    assert_eq!(
        history
            .cpu_groups
            .last()
            .unwrap()
            .application
            .values()
            .copied()
            .sum::<f32>(),
        10.0
    );
}

#[test]
fn app_icons_follow_executable_or_outer_bundle_identity() {
    for (exe, icon) in [
        ("/usr/bin/node", '\u{e719}'),
        ("/usr/bin/docker", '\u{e7b0}'),
        ("C:\\Program Files\\Microsoft\\Edge\\msedge.exe", '\u{f282}'),
        ("/Applications/Safari.app/Contents/MacOS/Safari", '\u{f267}'),
        (
            "/Applications/IntelliJ IDEA.app/Contents/bin/java",
            '\u{e7b5}',
        ),
        ("/usr/bin/python3.12", '\u{e73c}'),
        ("/usr/bin/nvim", '\u{e6ae}'),
    ] {
        assert_eq!(application(&process(1, exe, 0.0)).2, icon, "{exe}");
    }
    let mut unknown = process(2, "/bin/not-docker", 0.0);
    unknown.cmd = "docker node python3.12".into();
    assert_eq!(application(&unknown).2, '\u{eb7f}');
    assert_eq!(
        application(&process(3, "/bin/python3.backup", 0.0)).2,
        '\u{eb7f}'
    );
}

#[test]
fn system_icons_do_not_merge_distinct_services() {
    let snap = snapshot(vec![
        process(1, "/usr/bin/NetworkManager", 8.0),
        process(2, "/usr/lib/systemd/systemd-resolved", 16.0),
        process(3, "/usr/bin/pipewire", 24.0),
    ]);
    let grouped = groups(&snap, GroupMode::Application);
    assert_eq!(grouped.len(), 3);
    assert_eq!(grouped[0].label, "NetworkManager");
    assert_eq!(grouped[0].icon, '\u{f06f3}');
    assert_eq!(grouped[1].icon, '\u{f028}');
    assert_eq!(grouped[2].icon, '\u{f06f3}');
    assert_ne!(grouped[1].key, grouped[2].key);
    let mut state = CpuState::default();
    state.expanded.insert(RowKey::Group(grouped[0].key.clone()));
    let expanded = rows(&snap, &grouped, &state);
    assert_eq!(expanded[1].icon, grouped[0].icon);
}

#[test]
fn checkout_applications_aggregate_and_expand_independently() {
    let mut procs = vec![
        process(1, "/bin/node", 160.0),
        process(2, "/versions/v2/node", 80.0),
        process(3, "/bin/git", 40.0),
        process(4, "/bin/node", 320.0),
    ];
    for p in &mut procs {
        p.mem_rss = u64::from(p.pid) * 1024;
        p.project = Some(project(
            "github.com",
            if p.pid == 4 {
                "/work/feature"
            } else {
                "/work/main"
            },
        ));
    }
    let snap = snapshot(procs);
    let grouped = groups(&snap, GroupMode::Project);
    let mut state = CpuState::default();
    state.expanded.insert(RowKey::Group(grouped[0].key.clone()));
    for row in rows(&snap, &grouped, &state) {
        if row.depth == 1 {
            state.expanded.insert(row.key);
        }
    }
    let collapsed = rows(&snap, &grouped, &state);
    assert_eq!(collapsed[0].mem_rss, 10 * 1024);
    let checkouts: Vec<_> = collapsed.iter().filter(|r| r.depth == 1).collect();
    assert_eq!(
        checkouts.iter().map(|r| r.mem_rss).collect::<Vec<_>>(),
        [4 * 1024, 6 * 1024]
    );
    let apps: Vec<_> = collapsed.iter().filter(|r| r.depth == 2).collect();
    assert_eq!(apps.len(), 3);
    assert!(apps.iter().all(|r| r.process.is_none()));
    let main_node = apps.iter().find(|r| r.count == 2).unwrap();
    assert_eq!(main_node.label, "node");
    assert_eq!(main_node.cpu_pct, Some(30.0));
    assert_eq!(main_node.mem_rss, 3 * 1024);
    assert_eq!(main_node.icon, application(&snap.procs[0]).2);
    state.expanded.insert(main_node.key.clone());
    let expanded = rows(&snap, &grouped, &state);
    let leaves: Vec<_> = expanded
        .iter()
        .filter_map(|r| r.process.map(|p| (r.depth, p.pid)))
        .collect();
    assert_eq!(leaves, vec![(3, 1), (3, 2)]);
    assert_eq!(
        expanded
            .iter()
            .filter(|r| r.process.is_some())
            .map(|r| r.mem_rss)
            .sum::<u64>(),
        main_node.mem_rss
    );
    // Application sorting and totals match the normal view for this checkout.
    let local = snapshot(snap.procs[..3].to_vec());
    let normal = groups(&local, GroupMode::Application);
    let start = apps.iter().position(|r| r.count == 2).unwrap();
    for (nested, app) in apps[start..].iter().zip(normal) {
        assert_eq!(
            (&nested.label, nested.cpu_pct, nested.count),
            (&app.label, app.cpu_pct, app.members.len())
        );
    }
}
