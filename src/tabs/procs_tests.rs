use super::*;
use crate::app::TabId;
use crate::collect::{CpuTick, MemTick, ProcTick};
use crate::config::SyswatchConfig;
use crate::process_tree::{GroupMode, RowKey};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{backend::TestBackend, Terminal};

const GIB: u64 = 1024 * 1024 * 1024;

fn proc(pid: u32, ppid: u32, name: &str, cpu: f32, rss: u64) -> ProcTick {
    ProcTick {
        pid,
        ppid,
        name: name.into(),
        exe: Some(format!("/bin/{name}")),
        cmd: format!("/bin/{name}"),
        user: "alex".into(),
        cpu_pct: cpu,
        mem_rss: rss,
        state: 'S',
        ..Default::default()
    }
}

/// Eight cores, so 80% of one core is 10% of the machine.
fn app() -> App {
    let mut app = App::new(TabId::Procs, SyswatchConfig::default());
    let mut helper = proc(11, 10, "browser", 80.0, GIB / 4);
    helper.cmd = "/bin/browser --type=renderer".into();
    app.snap = Some(Snapshot {
        cpu: CpuTick {
            per_core: vec![0.0; 8],
            ..Default::default()
        },
        mem: MemTick {
            total_bytes: 16 * GIB,
            ..Default::default()
        },
        procs: vec![
            proc(10, 1, "browser", 240.0, GIB),
            helper,
            ProcTick {
                user: "postgres".into(),
                ..proc(20, 1, "postgres", 8.0, 3 * GIB)
            },
        ],
        ..Default::default()
    });
    app
}

fn render(app: &App, width: u16, height: u16) -> Vec<String> {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| draw(frame, frame.area(), app, app.displayed_snap().unwrap()))
        .unwrap();
    let buffer = terminal.backend().buffer();
    (0..height)
        .map(|y| (0..width).map(|x| buffer[(x, y)].symbol()).collect())
        .collect()
}

fn row_of(screen: &[String], needle: &str) -> usize {
    screen
        .iter()
        .position(|line| line.contains(needle))
        .unwrap_or_else(|| panic!("{needle:?} not on screen:\n{}", screen.join("\n")))
}

fn key(app: &mut App, code: KeyCode) {
    app.handle_key(KeyEvent::from(code));
}

#[test]
fn processes_share_one_group_row_until_expanded() {
    let mut app = app();
    let screen = render(&app, 140, 30);
    let browser = &screen[row_of(&screen, "browser")];
    // Whole-machine CPU without decimals, the member count, and no PID.
    assert!(browser.contains("40%"), "{browser}");
    assert!(browser.contains(" 2 "), "{browser}");
    assert!(!screen.iter().any(|l| l.contains(" 10 ")), "{screen:#?}");
    // A lone process is its own row, with its PID and owner.
    let postgres = &screen[row_of(&screen, "postgres")];
    assert!(
        postgres.contains(" 20 ") && postgres.contains("1%"),
        "{postgres}"
    );

    key(&mut app, KeyCode::Enter);
    let screen = render(&app, 140, 30);
    let group = row_of(&screen, "browser");
    assert!(screen[group + 1].contains(" 10 "), "{screen:#?}");
    assert!(screen[group + 2].contains(" 11 "), "{screen:#?}");
    assert!(screen[group + 1].contains("30%"), "{screen:#?}");
}

#[test]
fn sort_keys_rank_groups_by_their_metric() {
    let mut app = app();
    let screen = render(&app, 140, 30);
    assert!(row_of(&screen, "browser") < row_of(&screen, "postgres"));

    key(&mut app, KeyCode::Char('s'));
    assert_eq!(app.proc_sort, ProcSort::Rss);
    let screen = render(&app, 140, 30);
    // 3 GiB lands in a higher 1 GiB band than the browser's 1.25 GiB.
    assert!(row_of(&screen, "postgres") < row_of(&screen, "browser"));
}

#[test]
fn filter_keeps_whole_groups_and_reports_matches() {
    let mut app = app();
    key(&mut app, KeyCode::Char('/'));
    for c in "renderer".chars() {
        key(&mut app, KeyCode::Char(c));
    }
    key(&mut app, KeyCode::Enter);
    let screen = render(&app, 160, 30);
    assert!(
        screen[0].contains("1 groups (2 procs) · 1/3 match \"renderer\""),
        "{}",
        screen[0]
    );
    assert!(screen[row_of(&screen, "browser")].contains("40%"));
    assert!(!screen.iter().any(|l| l.contains("postgres")));

    app.filter_active = Some("nothing".into());
    let screen = render(&app, 160, 30);
    row_of(&screen, "No matching processes.");
}

#[test]
fn drill_in_describes_groups_and_processes() {
    let mut app = app();
    let screen = render(&app, 140, 30);
    row_of(&screen, "browser  2 processes  -  drill-in");
    assert!(screen[row_of(&screen, "owners")].contains("alex"));

    key(&mut app, KeyCode::Enter);
    key(&mut app, KeyCode::Down);
    key(&mut app, KeyCode::Down);
    assert_eq!(app.procs_tree.selected, Some(RowKey::Process(11, None)));
    let screen = render(&app, 140, 30);
    row_of(&screen, "browser  pid 11  -  drill-in");
    assert!(screen[row_of(&screen, "cmd")].contains("--type=renderer"));
    assert!(screen[row_of(&screen, "cpu         ")].contains("10% of machine"));

    key(&mut app, KeyCode::Char('d'));
    let screen = render(&app, 140, 30);
    assert!(!screen.iter().any(|l| l.contains("drill-in")));
}

#[test]
fn group_key_switches_every_tree_and_resets_their_state() {
    let mut app = app();
    key(&mut app, KeyCode::Enter);
    assert!(!app.procs_tree.expanded.is_empty());
    app.cpu.tree.selected = Some(RowKey::Process(20, None));

    key(&mut app, KeyCode::Char('b'));
    assert_eq!(app.user_config.process_group, GroupMode::Project);
    assert!(app.procs_tree.expanded.is_empty());
    assert_eq!(app.procs_tree.selected, None);
    assert_eq!(app.cpu.tree.selected, None);
    let screen = render(&app, 140, 30);
    row_of(&screen, "PROCESSES · Project");

    // Tabs without a process tree leave the grouping alone.
    key(&mut app, KeyCode::Char('4'));
    key(&mut app, KeyCode::Char('b'));
    assert_eq!(app.user_config.process_group, GroupMode::Project);
    for tab in ['1', '2', '3', '8'] {
        let before = app.user_config.process_group;
        key(&mut app, KeyCode::Char(tab));
        key(&mut app, KeyCode::Char('b'));
        assert_eq!(app.user_config.process_group, before.next(), "tab {tab}");
    }
}

#[test]
fn optional_columns_follow_available_data() {
    let mut app = app();
    let screen = render(&app, 160, 30);
    assert!(screen[row_of(&screen, "APPLICATION / PROJECT")].contains("VIRT"));

    let snap = app.snap.as_mut().unwrap();
    snap.net_rates_estimated = true;
    snap.procs[0].net_rx_rate = Some(2048.0);
    snap.procs[1].gpu_pct = Some(40.0);
    snap.procs[1].gpu_mem_bytes = Some(GIB);
    snap.procs[2].state = 'Z';
    let screen = render(&app, 200, 30);
    let header = &screen[row_of(&screen, "APPLICATION / PROJECT")];
    assert!(
        header.contains("~NET ↓/s") && header.contains(" GPU "),
        "{header}"
    );
    assert!(!header.contains("VIRT"), "{header}");
    let browser = &screen[row_of(&screen, "browser")];
    assert!(
        browser.contains("2.0 KB/s") && browser.contains("40%"),
        "{browser}"
    );
    assert!(browser.contains("1.0 GB"), "{browser}");
    assert!(screen[row_of(&screen, "postgres")].contains(" Z "));
}

#[test]
fn proc_sort_cycle_visits_gpu_and_net() {
    let labels: Vec<&str> = ProcSort::ALL.iter().map(|s| s.label()).collect();
    assert!(labels.contains(&"gpu"));
    assert!(labels.contains(&"net"));
}

#[test]
fn selection_falls_back_to_a_nearby_row_when_its_process_exits() {
    let mut app = app();
    key(&mut app, KeyCode::Enter);
    key(&mut app, KeyCode::Down);
    key(&mut app, KeyCode::Down);
    assert_eq!(app.procs_tree.selected, Some(RowKey::Process(11, None)));
    app.snap.as_mut().unwrap().procs.retain(|p| p.pid != 11);
    if let Some(view) = app.tree_view() {
        app.prepare_tree(view);
    }
    // The next surviving row takes the cursor, rather than the top row.
    assert_eq!(app.procs_tree.selected, Some(RowKey::Process(20, None)));
}

#[test]
fn rows_mark_system_groups_shared_owners_states_and_measured_rates() {
    let mut app = app();
    {
        let snap = app.snap.as_mut().unwrap();
        snap.procs[1].user = "root".into();
        snap.procs[0].state = 'R';
        snap.procs[0].io_read_rate = 4096.0;
        snap.procs[0].net_rx_rate = Some(1.0);
        snap.net_rates_estimated = false;
        snap.procs[0].start_time =
            Some(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000));
        snap.procs[2].scope = crate::collect::ProcessScope::System;
    }
    let screen = render(&app, 200, 30);
    let header = &screen[row_of(&screen, "APPLICATION / PROJECT")];
    assert!(
        header.contains("NET ↓/s") && !header.contains("~NET"),
        "{header}"
    );
    let browser = &screen[row_of(&screen, "browser")];
    assert!(
        browser.contains("2 users") && browser.contains("4.0 KB/s"),
        "{browser}"
    );
    assert!(screen[row_of(&screen, "postgres")].contains("[system]"));

    key(&mut app, KeyCode::Enter);
    key(&mut app, KeyCode::Down);
    let screen = render(&app, 200, 30);
    assert!(screen[row_of(&screen, " 10 ")].contains(" R "));
    assert!(screen[row_of(&screen, "started")].contains("2023-11-1"));

    app.filter_input = true;
    app.filter_buf = "bro".into();
    let screen = render(&app, 200, 30);
    assert!(screen[0].contains("bro"), "{}", screen[0]);
}
