use super::*;
use crate::app::TabId;
use crate::collect::{CpuTick, MemTick, ProcTick};
use crate::config::SyswatchConfig;
use crate::process_tree::RowKey;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{backend::TestBackend, Terminal};

const MIB: u64 = 1024 * 1024;
const GIB: u64 = 1024 * MIB;

fn proc(pid: u32, name: &str, rss: u64) -> ProcTick {
    ProcTick {
        pid,
        name: name.into(),
        exe: Some(format!("/bin/{name}")),
        cmd: format!("/bin/{name}"),
        user: "alex".into(),
        mem_rss: rss,
        ..Default::default()
    }
}

fn app() -> App {
    let mut app = App::new(TabId::Memory, SyswatchConfig::default());
    let mut browser = proc(10, "browser", 900 * MIB);
    browser.mem_pss = Some(300 * MIB);
    browser.mem_private = Some(200 * MIB);
    browser.mem_shared = Some(77 * MIB);
    browser.mem_peak = Some(999 * MIB);
    let mut helper = proc(11, "browser", 600 * MIB);
    helper.mem_pss = Some(100 * MIB);
    helper.mem_swap = Some(5 * MIB);
    let mut postgres = proc(20, "postgres", 2 * GIB);
    postgres.mem_pss = Some(3 * GIB / 2);
    postgres.user = "postgres".into();
    app.snap = Some(Snapshot {
        cpu: CpuTick {
            per_core: vec![0.0; 8],
            ..Default::default()
        },
        mem: MemTick {
            total_bytes: 16 * GIB,
            ..Default::default()
        },
        procs: vec![browser, helper, postgres, proc(30, "vim", MIB)],
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
fn groups_rank_by_attributed_memory_and_sum_only_additive_measures() {
    let app = app();
    let screen = render(&app, 140, 30);
    // PSS: postgres 1.5 GiB is in a higher band than the browser's 400 MiB.
    assert!(row_of(&screen, "postgres") < row_of(&screen, "browser"));
    let browser = &screen[row_of(&screen, "browser")];
    assert!(browser.contains("400.0 MB"), "summed PSS: {browser}");
    assert!(browser.contains("5.0 MB"), "summed swap: {browser}");
    assert!(browser.contains("1.5 GB"), "summed RSS: {browser}");
    // Shared pages and peaks overlap between processes.
    assert!(!browser.contains("77.0 MB") && !browser.contains("999.0 MB"));
    assert!(screen.iter().any(|l| l.contains("Process memory (pss")));
}

#[test]
fn memory_tree_navigates_and_expands_to_processes() {
    let mut app = app();
    key(&mut app, KeyCode::Down);
    assert_eq!(
        app.memory_tree.selected,
        Some(RowKey::Group(crate::process_tree::GroupKey::Application {
            identity: "name:browser".into()
        }))
    );
    key(&mut app, KeyCode::Enter);
    let screen = render(&app, 140, 30);
    let group = row_of(&screen, "browser");
    let leaf = &screen[group + 1];
    assert!(leaf.contains(" 10 "), "{screen:#?}");
    assert!(
        leaf.contains("77.0 MB") && leaf.contains("999.0 MB"),
        "{leaf}"
    );
    // A process without detail shows that it is unavailable.
    assert!(screen[group + 2].contains("—"), "{screen:#?}");
}

#[test]
fn filter_strip_counts_kept_groups_and_matches() {
    let mut app = app();
    app.filter_active = Some("postgres".into());
    let screen = render(&app, 140, 30);
    row_of(&screen, "1 groups (1 procs) · 1/4 match \"postgres\"");
    assert!(!screen.iter().any(|l| l.contains("browser")));

    app.filter_active = Some("absent".into());
    let screen = render(&app, 140, 30);
    row_of(&screen, "No matching processes.");
}

#[test]
fn rss_only_samples_show_virtual_size_on_process_rows() {
    let mut app = app();
    for p in &mut app.snap.as_mut().unwrap().procs {
        p.mem_pss = None;
        p.mem_peak = None;
        p.mem_virt = 3 * GIB;
    }
    let screen = render(&app, 140, 30);
    row_of(&screen, "Process memory (by RSS)");
    let header = &screen[row_of(&screen, "Application / project")];
    assert!(
        header.contains("VIRT") && !header.contains("PSS"),
        "{header}"
    );
    assert!(screen[row_of(&screen, "postgres")].contains("3.0 GB"));
    assert!(!screen[row_of(&screen, "browser")].contains("3.0 GB"));
}

#[test]
fn footprint_samples_lead_the_breakdown_and_mark_system_groups() {
    let mut app = app();
    for p in &mut app.snap.as_mut().unwrap().procs {
        p.mem_footprint = Some(p.mem_rss / 2);
        p.mem_pss = None;
    }
    app.snap.as_mut().unwrap().procs[2].scope = crate::collect::ProcessScope::System;
    let screen = render(&app, 140, 30);
    row_of(&screen, "footprint = real pressure");
    assert!(screen[row_of(&screen, "Application / project")].contains("FOOTPRNT"));
    let browser = &screen[row_of(&screen, "browser")];
    assert!(browser.contains("750.0 MB"), "summed footprint: {browser}");
    assert!(screen[row_of(&screen, "postgres")].contains("[system]"));
}
