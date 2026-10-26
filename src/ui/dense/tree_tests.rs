use super::*;
use crate::app::{TabId, ViewMode};
use crate::collect::{ProcTick, ProcessScope, ProjectIdentity};
use crate::config::SyswatchConfig;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{backend::TestBackend, Terminal};

fn app() -> App {
    let mut app = App::new(TabId::Overview, SyswatchConfig::default());
    app.view_mode = ViewMode::Dense;
    app.last_area = Rect::new(0, 0, GRID_W, GRID_H);
    let mut snap = Snapshot::default();
    snap.cpu.per_core = vec![0.0; 8];
    snap.procs = vec![
        ProcTick {
            pid: 290376,
            name: "pi".into(),
            exe: Some("/bin/pi".into()),
            user: "dev".into(),
            cpu_pct: 320.0,
            mem_rss: 100 * 1024 * 1024,
            cwd: Some("/work/assistant".into()),
            scope: ProcessScope::Tool,
            ..Default::default()
        },
        ProcTick {
            pid: 3845642,
            ppid: 290376,
            name: "MainThread".into(),
            exe: Some("/bin/node".into()),
            user: "dev".into(),
            cpu_pct: 320.0,
            mem_rss: 50 * 1024 * 1024,
            cwd: Some("/work/assistant/src".into()),
            scope: ProcessScope::Tool,
            ..Default::default()
        },
    ];
    for proc in &mut snap.procs {
        proc.project = Some(ProjectIdentity {
            host: "github.com".into(),
            repo: "dev/assistant".into(),
            checkout: "/work/assistant".into(),
        });
    }
    for _ in 0..8 {
        app.history.push(&snap);
    }
    app.snap = Some(snap);
    app
}

fn draw(app: &App) -> Buffer {
    let mut terminal =
        Terminal::new(TestBackend::new(app.last_area.width, app.last_area.height)).unwrap();
    terminal
        .draw(|f| render(f, app, app.displayed_snap().unwrap()))
        .unwrap();
    terminal.backend().buffer().clone()
}

fn lines(buffer: &Buffer) -> Vec<String> {
    (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect()
        })
        .collect()
}

fn press(app: &mut App, code: KeyCode) {
    app.handle_key(KeyEvent::from(code));
}

fn column(header: &str, label: &str) -> u16 {
    header[..header.find(label).unwrap()].width() as u16
}

#[test]
fn dense_folder_icon_tracks_expansion_and_collapse() {
    let _theme = theme::exclusive_theme();
    let mut app = app();
    for proc in &mut app.snap.as_mut().unwrap().procs {
        proc.project = None;
        proc.cwd = Some("/work/assistant".into());
    }
    assert!(lines(&draw(&app))[36].contains('\u{f07b}'));
    press(&mut app, KeyCode::Enter);
    let expanded = lines(&draw(&app));
    assert!(expanded[36].contains('\u{f07c}'), "{}", expanded[36]);
    assert!(expanded[37].contains("╰─"), "{}", expanded[37]);
    press(&mut app, KeyCode::Enter);
    assert!(lines(&draw(&app))[36].contains('\u{f07b}'));
}

#[test]
fn dense_tree_aggregates_projects_and_keeps_dotted_history() {
    let _theme = theme::exclusive_theme();
    let app = app();
    let buffer = draw(&app);
    let lines = lines(&buffer);
    let root = lines
        .iter()
        .find(|line| line.contains("dev/assistant") && line.contains("80%"))
        .expect("collapsed project row");
    assert!(root.contains("80%"), "{root}");
    assert!(root.contains(&human_bytes(150 * 1024 * 1024)), "{root}");
    assert!(
        !root.contains("290376") && !root.contains("3845642"),
        "{root}"
    );
    assert!(
        root.chars().any(|c| ('\u{2801}'..='\u{28ff}').contains(&c)),
        "dotted history: {root}"
    );
    assert!(lines[32].contains("┤ procs ├"));
    assert!(lines[12].contains("┤ mem ├") && lines[12].contains("┤ net ├"));
    let header = &lines[35];
    assert!(header.find("PROCS").unwrap() < header.find("PID").unwrap());
    assert!(
        !header.contains("MEMRECENT"),
        "column headers touch: {header}"
    );
    let cpu_x = column(header, "CPU");
    assert_eq!(buffer[(cpu_x, 36)].fg, theme::active().status_error);
}

#[test]
fn dense_tree_drills_through_checkout_and_parent_tool_to_full_pid() {
    let _theme = theme::exclusive_theme();
    let mut app = app();
    // Zoom retains the same tree and gives it room for all descendants.
    press(&mut app, KeyCode::Char('6'));
    press(&mut app, KeyCode::Enter);
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Enter);
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Enter);
    let screen = lines(&draw(&app)).join("\n");
    assert!(!screen.contains('▸') && !screen.contains('▾'), "{screen}");
    for (label, branch) in [
        ("/work/assistant", "╰─"),
        ("290376", "╰─"),
        ("3845642", "├─"),
    ] {
        let row = screen
            .lines()
            .find(|line| line.contains(label) && line.contains('%'))
            .unwrap();
        assert!(row.contains(branch), "{row}");
    }
    let root = screen
        .lines()
        .find(|line| line.contains("dev/assistant") && line.contains('%'))
        .unwrap();
    assert!(!root.contains('─'), "{root}");
    assert!(
        screen.contains("╰─") && screen.contains("dev/assistant"),
        "{screen}"
    );
    assert!(screen.contains("/work/assistant"), "{screen}");
    assert!(
        screen.contains("290376") && screen.contains("3845642"),
        "{screen}"
    );
    let leaf = screen
        .lines()
        .find(|line| line.contains("3845642"))
        .unwrap();
    assert!(
        leaf.contains("node") || leaf.contains("MainThread"),
        "{leaf}"
    );
    press(&mut app, KeyCode::Esc);
    let screen = lines(&draw(&app)).join("\n");
    assert!(screen.contains("dev/assistant") && screen.contains("/work/assistant"));
    // Collapsing the selected tool keeps its parent hierarchy visible.
    press(&mut app, KeyCode::Enter);
    let screen = lines(&draw(&app)).join("\n");
    assert!(!screen.contains("3845642"), "{screen}");
}

#[test]
fn dense_tree_orders_tools_before_system_then_cpu_bands_and_names() {
    let _theme = theme::exclusive_theme();
    let mut app = app();
    let snap = app.snap.as_mut().unwrap();
    snap.procs = [("zulu", 112.0), ("alpha", 80.0), ("systemd", 800.0)]
        .into_iter()
        .enumerate()
        .map(|(i, (name, cpu_pct))| ProcTick {
            pid: 100 + i as u32,
            name: name.into(),
            cpu_pct,
            scope: if name == "systemd" {
                ProcessScope::System
            } else {
                ProcessScope::Tool
            },
            ..Default::default()
        })
        .collect();
    let buffer = draw(&app);
    let lines = lines(&buffer);
    assert!(lines[36].contains("alpha"), "{}", lines[36]);
    assert!(lines[37].contains("zulu"), "{}", lines[37]);
    assert!(
        lines[38].contains("systemd") && lines[38].contains("[system]"),
        "{}",
        lines[38]
    );
}

#[test]
fn dense_tree_uses_displayed_snapshot_for_navigation() {
    let _theme = theme::exclusive_theme();
    let mut app = app();
    let mut live = app.snap.clone().unwrap();
    live.procs.clear();
    app.history.push(&live);
    app.snap = Some(live);
    app.scrub_offset = 1;
    press(&mut app, KeyCode::Enter);
    let screen = lines(&draw(&app)).join("\n");
    assert!(screen.contains("/work/assistant"), "{screen}");
}

#[test]
fn dense_tree_keeps_leaf_pids_and_blank_single_counts_on_narrow_screens() {
    let _theme = theme::exclusive_theme();
    let mut app = app();
    app.last_area = Rect::new(0, 0, 80, 24);
    press(&mut app, KeyCode::Char('3'));
    for _ in 0..3 {
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Down);
    }
    let buffer = draw(&app);
    let screen = lines(&buffer);
    let header = &screen[3];
    let procs_x = column(header, "PROCS");
    let cpu_x = column(header, "CPU");
    let y = screen
        .iter()
        .enumerate()
        .skip(4)
        .find(|(_, line)| line.contains("3845642"))
        .unwrap()
        .0 as u16;
    assert!((procs_x..procs_x + 5).all(|x| buffer[(x, y)].symbol() == " "));
    assert!(screen[y as usize].contains("40%"), "{}", screen[y as usize]);
    assert_eq!(buffer[(cpu_x, y)].fg, theme::active().status_warn);
    assert!(!screen[y as usize].contains("40.0"));
}

#[test]
fn dense_tree_selection_follows_group_when_cpu_changes_its_position() {
    let _theme = theme::exclusive_theme();
    let mut app = app();
    app.snap.as_mut().unwrap().procs.push(ProcTick {
        pid: 99,
        name: "yazi".into(),
        cwd: Some("/work/other".into()),
        cpu_pct: 8.0,
        ..Default::default()
    });
    press(&mut app, KeyCode::End);
    let before = app.dense.tree.selected.clone();
    app.snap.as_mut().unwrap().procs.last_mut().unwrap().cpu_pct = 800.0;
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.dense.tree.selected, before);
    let buffer = draw(&app);
    let screen = lines(&buffer);
    assert!(
        !screen[36].contains("─") && screen[36].contains("/work/other"),
        "{}",
        screen[36]
    );
    assert!(screen[37].contains("yazi"), "{}", screen[37]);
    assert!(
        !screen[38].contains("─") && screen[38].contains("dev/assistant"),
        "{}",
        screen[38]
    );
}

#[test]
fn dense_tree_history_distinguishes_low_and_high_machine_usage() {
    let _theme = theme::exclusive_theme();
    let mut app = app();
    let mut snap = app.snap.take().unwrap();
    for (proc, (name, cpu)) in snap.procs.iter_mut().zip([("quiet", 8.0), ("busy", 320.0)]) {
        proc.name = name.into();
        proc.exe = None;
        proc.cwd = None;
        proc.project = None;
        proc.ppid = 0;
        proc.cpu_pct = cpu;
    }
    app.history = History::new(64);
    for _ in 0..8 {
        app.history.push(&snap);
    }
    app.snap = Some(snap);
    let screen = lines(&draw(&app));
    let dots = |line: &str| {
        line.chars()
            .filter(|c| ('\u{2801}'..='\u{28ff}').contains(c))
            .collect::<String>()
    };
    assert_ne!(
        dots(&screen[36]),
        dots(&screen[37]),
        "1% and 40% must have different history heights:\n{}",
        screen.join("\n")
    );
}

#[test]
fn dense_tree_compact_border_keeps_the_pid_total_readable() {
    let _theme = theme::exclusive_theme();
    let mut app = app();
    app.last_area = Rect::new(0, 0, 56, 24);
    let base = app.snap.as_ref().unwrap().procs[0].clone();
    app.snap.as_mut().unwrap().procs = (0..128)
        .map(|i| ProcTick {
            pid: 1000 + i,
            cpu_pct: 0.0,
            ..base.clone()
        })
        .collect();
    press(&mut app, KeyCode::Char('3'));
    let screen = lines(&draw(&app));
    assert!(screen[0].contains("128 PIDs"), "{}", screen[0]);
    assert!(screen[0].contains("system last"), "{}", screen[0]);
}

#[test]
fn dense_tree_memory_is_gray_below_one_gb_and_colored_at_the_boundary() {
    let _theme = theme::exclusive_theme();
    let theme = theme::active();
    let ramps = Ramps::from_theme(&theme);
    for bytes in [0, 150 * 1024 * 1024, (1 << 30) - 1, 1 << 30, 2 << 30] {
        let mut app = app();
        let procs = &mut app.snap.as_mut().unwrap().procs;
        procs[0].mem_rss = bytes;
        procs[1].mem_rss = 0;
        let buffer = draw(&app);
        let screen = lines(&buffer);
        let mem_end = column(&screen[35], "MEM") + 2;
        assert!(screen[36].contains(&human_bytes(bytes)), "{}", screen[36]);
        assert_eq!(
            buffer[(mem_end, 36)].fg,
            if bytes < 1 << 30 {
                theme.text_secondary
            } else {
                ramps.secondary.mid()
            },
            "memory color for {bytes} bytes"
        );
    }
}

#[test]
fn dense_tree_memory_color_uses_each_group_and_pid_total() {
    let _theme = theme::exclusive_theme();
    let mut app = app();
    for proc in &mut app.snap.as_mut().unwrap().procs {
        proc.mem_rss = 512 * 1024 * 1024;
    }
    for _ in 0..3 {
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Down);
    }
    let buffer = draw(&app);
    let screen = lines(&buffer);
    let mem_end = column(&screen[35], "MEM") + 2;
    for y in 36..39 {
        assert!(screen[y].contains("1.0 GB"), "{}", screen[y]);
        assert_eq!(
            buffer[(mem_end, y as u16)].fg,
            Ramps::from_theme(&theme::active()).secondary.mid()
        );
    }
    for y in 39..41 {
        assert!(screen[y].contains("512.0 MB"), "{}", screen[y]);
        assert_eq!(
            buffer[(mem_end, y as u16)].fg,
            if screen[y].contains('▶') {
                theme::active().text_secondary
            } else {
                theme::active().text_muted
            }
        );
    }
}

fn changing_process_list() -> App {
    let mut app = app();
    app.snap.as_mut().unwrap().procs = ["alpha", "beta", "zulu"]
        .into_iter()
        .enumerate()
        .map(|(i, name)| ProcTick {
            pid: 100 + i as u32,
            name: name.into(),
            mem_rss: 1024,
            ..Default::default()
        })
        .collect();
    press(&mut app, KeyCode::Down);
    app
}

#[test]
fn dense_tree_keeps_selection_when_a_single_pid_becomes_a_group() {
    let _theme = theme::exclusive_theme();
    let mut app = changing_process_list();
    let mut helper = app.snap.as_ref().unwrap().procs[1].clone();
    helper.pid = 200;
    app.snap.as_mut().unwrap().procs.push(helper);
    press(&mut app, KeyCode::Enter);
    let screen = lines(&draw(&app));
    let selected = screen.iter().find(|line| line.contains('▶')).unwrap();
    assert!(
        selected.contains("beta") && !selected.contains('─'),
        "{selected}"
    );
    assert!(
        screen.iter().any(|line| line.contains("200")),
        "{}",
        screen.join("\n")
    );
}

#[test]
fn dense_tree_keeps_a_nearby_selection_when_the_selected_process_exits() {
    let _theme = theme::exclusive_theme();
    let mut app = changing_process_list();
    app.snap.as_mut().unwrap().procs.retain(|p| p.pid != 101);
    press(&mut app, KeyCode::Char('p'));
    let screen = lines(&draw(&app));
    let selected = screen.iter().find(|line| line.contains('▶')).unwrap();
    assert!(selected.contains("zulu"), "{selected}");
}

#[test]
fn dense_tree_docker_expands_a_single_pid_container_and_shows_its_identity() {
    let _theme = theme::exclusive_theme();
    let mut app = app();
    let id = "abcdef01".repeat(8);
    app.snap.as_mut().unwrap().procs.truncate(1);
    let proc = &mut app.snap.as_mut().unwrap().procs[0];
    proc.name = "postgres".into();
    proc.exe = Some("/bin/postgres".into());
    proc.docker_container = Some(id.clone());
    press(&mut app, KeyCode::Enter);
    let screen = lines(&draw(&app));
    assert!(screen[36].contains("Docker"), "{}", screen[36]);
    assert!(
        screen[37].contains("╰─") && screen[37].contains(&id[..12]),
        "{}",
        screen[37]
    );
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Enter);
    let screen = lines(&draw(&app));
    assert!(screen[34].contains(&id), "{}", screen[34]);
    assert!(
        screen[38].contains("postgres") && screen[38].contains("290376"),
        "{}",
        screen[38]
    );
    assert!(!screen.iter().any(|line| line.contains("dev/assistant")));
}

#[test]
fn dense_tree_container_id_stays_gray_while_busy_container_label_is_red() {
    let _theme = theme::exclusive_theme();
    for width in [130, 96, 56] {
        for selected in [false, true] {
            let mut app = app();
            app.last_area = Rect::new(0, 0, width, 44);
            app.dense.zoom = Some(if width < 100 { 3 } else { 6 });
            let id = "abcdef01".repeat(8);
            app.snap.as_mut().unwrap().procs.truncate(1);
            app.snap.as_mut().unwrap().procs[0].docker_container = Some(id.clone());
            let root = crate::cpu::RowKey::Group(crate::cpu::GroupKey::Docker);
            app.dense.tree.expanded.insert(root.clone());
            app.dense.tree.selected = Some(if selected {
                crate::cpu::RowKey::Container(id.clone())
            } else {
                root
            });
            let buffer = draw(&app);
            let screen = lines(&buffer);
            let (y, line) = screen
                .iter()
                .enumerate()
                .find(|(_, line)| line.contains("╰─") && line.contains("container "))
                .unwrap();
            let label = column(line, "container ");
            assert_eq!(buffer[(label, y as u16)].fg, theme::active().status_error);
            let id_start = label + "container ".width() as u16;
            assert_eq!(buffer[(id_start, y as u16)].symbol(), "a", "{line}");
            for (offset, ch) in id[..12].chars().enumerate() {
                let cell = &buffer[(id_start + offset as u16, y as u16)];
                if cell.symbol() != ch.to_string() {
                    break; // Narrow rows clip the ID before the numeric columns.
                }
                assert_eq!(
                    cell.fg,
                    if selected {
                        theme::active().text_secondary
                    } else {
                        theme::active().text_muted
                    },
                    "container ID at width {width}, selected {selected}"
                );
            }
        }
    }
}

#[test]
fn dense_tree_system_badge_is_gray_on_selected_and_unselected_rows() {
    let _theme = theme::exclusive_theme();
    for selected in [false, true] {
        let mut app = app();
        app.snap.as_mut().unwrap().procs = vec![
            ProcTick {
                pid: 1,
                name: "alpha".into(),
                ..Default::default()
            },
            ProcTick {
                pid: 2,
                name: "systemd".into(),
                cpu_pct: 240.0,
                scope: ProcessScope::System,
                ..Default::default()
            },
        ];
        app.dense.tree.selected = Some(crate::cpu::RowKey::Process(
            if selected { 2 } else { 1 },
            None,
        ));
        let buffer = draw(&app);
        let screen = lines(&buffer);
        let (y, line) = screen
            .iter()
            .enumerate()
            .find(|(_, line)| line.contains("[system]"))
            .unwrap();
        assert_eq!(
            buffer[(column(line, "systemd"), y as u16)].fg,
            theme::active().status_error
        );
        let start = column(line, "[system]");
        for offset in 0.."[system]".len() as u16 {
            assert_eq!(
                buffer[(start + offset, y as u16)].fg,
                if selected {
                    theme::active().text_secondary
                } else {
                    theme::active().text_muted
                },
                "system badge selected {selected}"
            );
        }
    }
}

#[test]
fn dense_tree_label_colors_follow_machine_cpu_thresholds_and_selection() {
    let _theme = theme::exclusive_theme();
    let theme = theme::active();
    for (pct, expected) in [
        (0.0, theme.text_muted),
        (9.99, theme.text_muted),
        (10.0, theme.text_primary),
        (29.99, theme.text_primary),
        (30.0, theme.status_error),
        (70.0, theme.status_error),
    ] {
        for selected in [false, true] {
            let mut app = app();
            app.snap.as_mut().unwrap().procs = vec![
                ProcTick {
                    pid: 1,
                    name: "target".into(),
                    cpu_pct: pct * 8.0,
                    ..Default::default()
                },
                ProcTick {
                    pid: 2,
                    name: "other".into(),
                    ..Default::default()
                },
            ];
            app.dense.tree.selected = Some(crate::cpu::RowKey::Process(
                if selected { 1 } else { 2 },
                None,
            ));
            let buffer = draw(&app);
            let screen = lines(&buffer);
            let (y, line) = screen
                .iter()
                .enumerate()
                .skip(36)
                .find(|(_, line)| line.contains("target"))
                .unwrap();
            let start = column(line, "target");
            let expected = if selected && pct < 10.0 {
                theme.text_secondary
            } else {
                expected
            };
            for x in start..start + "target".len() as u16 {
                assert_eq!(
                    buffer[(x, y as u16)].fg,
                    expected,
                    "CPU {pct}, selected {selected}: {line}"
                );
            }
        }
    }
}

#[test]
fn dense_tree_label_colors_use_aggregate_cpu_for_groups_and_own_cpu_for_pids() {
    let _theme = theme::exclusive_theme();
    let mut app = app();
    app.snap.as_mut().unwrap().procs[0].cpu_pct = 72.0;
    app.snap.as_mut().unwrap().procs[1].cpu_pct = 168.0;
    press(&mut app, KeyCode::Char('6'));
    for _ in 0..3 {
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Down);
    }
    app.dense.tree.selected = Some(crate::cpu::RowKey::Process(290376, None));
    let buffer = draw(&app);
    let screen = lines(&buffer);
    for (offset, label, expected) in [
        (0, "dev/assistant", theme::active().status_error),
        (1, "/work/assistant", theme::active().status_error),
        (2, "pi", theme::active().status_error),
        (3, "node", theme::active().text_primary),
        (4, "pi", theme::active().text_secondary),
    ] {
        let y = 4 + offset;
        assert!(screen[y].contains(label), "{}", screen[y]);
        assert_eq!(
            buffer[(column(&screen[y], label), y as u16)].fg,
            expected,
            "{}",
            screen[y]
        );
    }
}
