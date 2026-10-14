use super::*;

#[test]
fn known_directory_expands_into_apps_and_pids_with_totals_and_details() {
    let mut app = app();
    let mut snap = app.snap.take().unwrap();
    let path = dirs::home_dir().unwrap().join("syswatch-directory-fixture");
    let path = path.to_str().unwrap();
    for proc in &mut snap.procs {
        proc.cwd = Some(path.into());
        proc.project = None;
    }
    snap.procs[0].name = "node".into();
    snap.procs[0].exe = Some("/bin/node".into());
    snap.procs[0].mem_rss = 512 * 1024 * 1024;
    snap.procs[1].name = "git".into();
    snap.procs[1].exe = Some("/bin/git".into());
    snap.procs[1].mem_rss = 256 * 1024 * 1024;
    let mut second_node = snap.procs[0].clone();
    second_node.pid = 44;
    second_node.cpu_pct = 0.0;
    second_node.mem_rss = 128 * 1024 * 1024;
    snap.procs.push(second_node);
    app.history.push(&snap);
    app.snap = Some(snap);
    app.user_config.cpu_group = cpu::GroupMode::Project;
    let before = render(&app, 140, 40);
    assert!(before.contains("~/syswatch-directory-fixture"), "{before}");
    assert!(before.contains("No Git project discovered"), "{before}");
    assert!(!before.contains(path), "{before}");
    let directory = before
        .lines()
        .find(|l| l.contains('▸') && l.contains("~/syswatch-directory-fixture"))
        .unwrap();
    assert!(
        directory.contains("50%") && directory.contains("896.0 MB"),
        "{directory}"
    );
    app.handle_key(KeyEvent::from(KeyCode::Enter));
    let apps = render(&app, 140, 40);
    assert!(
        apps.lines()
            .any(|l| l.contains('▸') && l.contains("node") && l.contains("640.0 MB")),
        "{apps}"
    );
    assert!(
        apps.lines()
            .any(|l| l.contains("git") && l.contains("43") && l.contains("20%")),
        "{apps}"
    );
    app.handle_key(KeyEvent::from(KeyCode::Down));
    app.handle_key(KeyEvent::from(KeyCode::Enter));
    let processes = render(&app, 140, 40);
    for pid in [42, 43, 44] {
        assert_eq!(
            processes
                .lines()
                .filter(|l| l.contains(&pid.to_string()) && l.contains('%'))
                .count(),
            1,
            "{processes}"
        );
    }
}
