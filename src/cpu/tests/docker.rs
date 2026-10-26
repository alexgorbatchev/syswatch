use super::*;

fn docker_snapshot() -> Snapshot {
    let mut procs = vec![
        process(10, "/usr/bin/dockerd", 8.0),
        process(11, "/usr/bin/docker-proxy", 8.0),
        process(12, "/usr/bin/containerd", 8.0),
        process(20, "/usr/bin/containerd-shim-runc-v2", 8.0),
        process(21, "/usr/bin/chromium", 80.0),
        process(22, "/usr/bin/chromium", 8.0),
        process(30, "/usr/bin/postgres", 8.0),
        process(40, "/usr/bin/node", 8.0),
    ];
    procs[1].ppid = 10;
    for p in &mut procs[3..6] {
        p.docker_container = Some("a".repeat(64));
    }
    procs[4].ppid = 20;
    procs[5].ppid = 21;
    procs[6].docker_container = Some("b".repeat(64));
    for p in &mut procs {
        p.cwd = Some(format!("/work/{}", p.pid));
        p.mem_rss = 1024 * 1024;
    }
    procs[4].project = Some(crate::collect::ProjectIdentity {
        host: "github.com".into(),
        repo: "dev/browser".into(),
        checkout: "/work/browser".into(),
    });
    snapshot(procs)
}

#[test]
fn docker_overrides_locations_and_aggregates_without_absorbing_unrelated_tools() {
    let snap = docker_snapshot();
    for mode in [GroupMode::Project, GroupMode::Application] {
        let grouped = groups(&snap, mode);
        let docker = grouped
            .iter()
            .find(|g| g.label == "Docker")
            .expect("Docker root");
        let mut pids: Vec<_> = docker.members.iter().map(|p| p.pid).collect();
        pids.sort_unstable();
        assert_eq!(pids, [10, 11, 20, 21, 22, 30]);
        assert_eq!(docker.cpu_pct, Some(15.0));
        assert_eq!(GroupTotals::new(&snap).get(mode, &docker.key), 15.0);
        assert_eq!(grouped.iter().map(|g| g.members.len()).sum::<usize>(), 8);
        assert_eq!(grouped.len(), 3);
        let state = CpuState::default();
        let visible = rows(&snap, mode, &grouped, &state);
        assert_eq!(
            visible.iter().map(|r| r.mem_rss).sum::<u64>(),
            8 * 1024 * 1024
        );
        assert!(!grouped.iter().any(|g| g.label == "dev/browser"));
    }
}

#[test]
fn docker_expands_containers_to_unique_pid_leaves_and_preserves_real_parents() {
    let snap = docker_snapshot();
    let grouped = groups(&snap, GroupMode::Project);
    let docker = grouped
        .iter()
        .find(|g| g.label == "Docker")
        .expect("Docker root");
    let mut state = CpuState::default();
    state.expanded.insert(RowKey::Group(docker.key.clone()));
    let visible = rows(&snap, GroupMode::Project, &grouped, &state);
    let containers: Vec<_> = visible
        .iter()
        .filter(|r| r.depth == 1 && r.process.is_none())
        .collect();
    assert_eq!(containers.len(), 2);
    assert_eq!(containers.iter().map(|r| r.count).sum::<usize>(), 4);
    assert!(
        containers.iter().any(|r| r.count == 1),
        "single-PID container stays expandable"
    );
    state
        .expanded
        .extend(containers.iter().map(|r| r.key.clone()));
    let visible = rows(&snap, GroupMode::Project, &grouped, &state);
    let mut leaves: Vec<_> = visible
        .iter()
        .filter(|r| r.group == docker.key)
        .filter_map(|r| r.process.map(|p| p.pid))
        .collect();
    leaves.sort_unstable();
    assert_eq!(leaves, [10, 11, 20, 21, 22, 30]);
    let child = visible
        .iter()
        .find(|r| r.process.is_some_and(|p| p.pid == 21))
        .unwrap();
    assert_eq!(child.depth, 2);
    assert_eq!(child.process.unwrap().ppid, 20);
    assert_eq!(child.mem_rss, 1024 * 1024);
}

#[test]
fn docker_container_workloads_remain_tools_even_when_their_program_is_an_os_helper() {
    let mut snap = docker_snapshot();
    snap.procs.retain(|p| p.pid == 30);
    snap.procs[0].name = "sshd".into();
    snap.procs[0].exe = Some("/usr/bin/sshd".into());
    snap.procs[0].scope = ProcessScope::System;
    let grouped = groups(&snap, GroupMode::Project);
    assert_eq!(grouped[0].label, "Docker");
    assert!(!system_only(&grouped[0].members));
}
