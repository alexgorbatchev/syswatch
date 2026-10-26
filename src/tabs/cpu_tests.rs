use super::*;
use crate::app::TabId;
use crate::collect::{CpuTick, ProcTick, ProjectIdentity};
use crate::config::SyswatchConfig;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{backend::TestBackend, Terminal};

#[path = "cpu_tests/directories.rs"]
mod directories;

fn app() -> App {
    let mut app = App::new(TabId::Cpu, SyswatchConfig::default());
    let snap = Snapshot {
        cpu: CpuTick {
            usage_pct: 60.0,
            per_core: vec![60.0; 8],
            ..Default::default()
        },
        procs: vec![
            ProcTick {
                pid: 42,
                name: "chrome".into(),
                exe: Some("/opt/google/chrome/chrome".into()),
                cpu_pct: 240.0,
                ..Default::default()
            },
            ProcTick {
                pid: 43,
                name: "node".into(),
                exe: Some("/usr/bin/node".into()),
                cpu_pct: 160.0,
                cwd: Some("/work/web/src".into()),
                project: Some(ProjectIdentity {
                    host: "github.com".into(),
                    repo: "acme/web".into(),
                    checkout: "/work/web".into(),
                }),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    app.history.push(&snap);
    app.snap = Some(snap);
    app
}

fn render(app: &App, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| draw(frame, frame.area(), app, app.displayed_snap().unwrap()))
        .unwrap();
    let buffer = terminal.backend().buffer();
    (0..height)
        .map(|y| {
            (0..width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn shows_application_totals_and_explanation_at_normal_and_small_sizes() {
    let app = app();
    for (w, h) in [(120, 35), (80, 24), (60, 18)] {
        let screen = render(&app, w, h);
        assert!(screen.contains("Google Chrome"), "{screen}");
        assert!(screen.contains("30%"), "{screen}");
        assert!(screen.contains("Application"), "{screen}");
        assert!(screen.contains("b:Group"), "{screen}");
    }
}

#[test]
fn expanded_processes_have_a_separate_pid_column() {
    for (width, process_pid) in [
        (60, 42),
        (80, 42),
        (140, 42),
        (60, u32::MAX),
        (140, u32::MAX),
    ] {
        let mut app = app();
        let mut snap = app.snap.take().unwrap();
        snap.procs[0].pid = process_pid;
        let mut second = snap.procs[0].clone();
        second.pid = 44;
        snap.procs.push(second);
        app.history.push(&snap);
        app.snap = Some(snap);
        app.handle_key(KeyEvent::from(KeyCode::Enter));
        let expected_pid = process_pid.to_string();
        let screen = render(&app, width, 35);
        if width == 140 {
            assert!(
                screen.contains("Expand to inspect its processes."),
                "{screen}"
            );
        }
        let lines: Vec<_> = screen.lines().collect();
        let header = lines
            .iter()
            .find(|l| l.contains("Procs") && l.contains("CPU"))
            .unwrap();
        let pid_column = header[..header.find("PID").expect("separate PID header")]
            .chars()
            .count();
        let count_column = header[..header.find("Procs").unwrap()].chars().count();
        let cpu_column = header[..header.find("CPU").unwrap()].chars().count();
        assert!(count_column < pid_column, "{header}");
        let group = lines
            .iter()
            .skip_while(|l| !l.contains("Procs") || !l.contains("CPU"))
            .find(|l| l.contains('%'))
            .unwrap();
        let leaf = lines
            .iter()
            .find(|l| {
                l.contains("chrome") && !l.contains("Google Chrome") && l.contains(&expected_pid)
            })
            .unwrap_or_else(|| panic!("{screen}"));
        assert!(!leaf.contains("PID"), "{leaf}");
        let pid: String = leaf
            .chars()
            .skip(pid_column)
            .take(cpu_column - pid_column)
            .collect();
        assert_eq!(pid.trim(), expected_pid, "{leaf}");
        assert!(
            group
                .chars()
                .skip(pid_column)
                .take(cpu_column - pid_column)
                .all(|c| c == ' '),
            "{group}"
        );
    }
}

#[test]
fn procs_column_hides_single_process_counts_but_keeps_group_counts() {
    for width in [60, 80, 140] {
        let mut app = app();
        let mut snap = app.snap.take().unwrap();
        let mut second = snap.procs[0].clone();
        second.pid = 44;
        snap.procs.push(second);
        app.snap = Some(snap);
        app.handle_key(KeyEvent::from(KeyCode::Enter));
        let screen = render(&app, width, 35);
        let header = screen
            .lines()
            .find(|l| l.contains("Procs") && l.contains("CPU"))
            .unwrap();
        let start = header[..header.find("Procs").unwrap()].chars().count();
        let end = header[..header.find("PID").unwrap()].chars().count();
        assert!(start < end, "{header}");
        let counts: Vec<_> = screen
            .lines()
            .skip_while(|line| *line != header)
            .skip(1)
            .take_while(|line| !line.starts_with('└'))
            .filter(|line| line.contains('%'))
            .map(|line| {
                line.chars()
                    .skip(start)
                    .take(end - start)
                    .collect::<String>()
                    .trim()
                    .to_owned()
            })
            .collect();
        assert_eq!(counts, ["2", "", "", ""], "{screen}");
    }
}

#[test]
fn owning_tool_expands_helpers_with_pids_and_explains_ownership_in_both_modes() {
    for mode in [cpu::GroupMode::Application, cpu::GroupMode::Project] {
        let mut app = app();
        let mut snap = app.snap.take().unwrap();
        snap.procs = vec![snap.procs[1].clone()];
        snap.procs[0].name = "pi".into();
        snap.procs[0].cpu_pct = 80.0;
        let mut helper = snap.procs[0].clone();
        helper.pid = 44;
        helper.ppid = 43;
        helper.name = "MainThread".into();
        helper.cpu_pct = 160.0;
        snap.procs.push(helper);
        app.snap = Some(snap);
        app.user_config.cpu_group = mode;
        for _ in 0..5 {
            let snap = app.displayed_snap().unwrap();
            let grouped = cpu::groups(snap, mode);
            let keys: Vec<_> = cpu::rows(snap, mode, &grouped, &app.cpu)
                .into_iter()
                .filter(|row| row.process.is_none())
                .map(|row| row.key)
                .collect();
            app.cpu.expanded.extend(keys);
        }
        let screen = render(&app, 140, 40);
        assert!(
            screen
                .lines()
                .any(|l| l.contains("pi") && l.contains("30%")),
            "{screen}"
        );
        assert!(
            screen
                .lines()
                .any(|l| l.contains("node") && l.contains("44") && l.contains("20%")),
            "{screen}"
        );
        assert!(
            screen.lines().any(|l| !l.contains("▾")
                && !l.contains("▸")
                && l.contains("node")
                && l.contains("20%")),
            "{screen}"
        );
        app.cpu.selected = Some(cpu::RowKey::Process(44, None));
        let screen = render(&app, 140, 40);
        assert!(screen.contains("Owning tool: pi · PID 43"), "{screen}");
    }
}

#[test]
fn singleton_project_expands_through_checkout_to_its_pid_and_details() {
    for width in [80, 140] {
        let mut app = app();
        let mut snap = app.snap.take().unwrap();
        snap.procs.remove(0);
        snap.procs[0].name = "yazi".into();
        snap.procs[0].exe = Some("/bin/yazi".into());
        snap.procs[0].pid = 1161583;
        app.snap = Some(snap);
        app.user_config.cpu_group = cpu::GroupMode::Project;
        let before = render(&app, width, 35);
        assert!(
            before.contains("Expand to inspect its processes."),
            "{before}"
        );
        assert!(
            before
                .lines()
                .any(|l| !l.contains('─') && l.contains("acme/web")),
            "{before}"
        );
        assert!(!before.contains("1161583"), "{before}");
        assert!(!before.contains("acme/web · yazi"), "{before}");
        app.handle_key(KeyEvent::from(KeyCode::Enter));
        let project = render(&app, width, 35);
        assert!(
            project
                .lines()
                .any(|l| l.contains("╰─") && l.contains("/work/web")),
            "{project}"
        );
        assert!(!project.contains("1161583"), "{project}");
        app.handle_key(KeyEvent::from(KeyCode::Down));
        app.handle_key(KeyEvent::from(KeyCode::Enter));
        app.handle_key(KeyEvent::from(KeyCode::Down));
        let expanded = render(&app, width, 35);
        let header = expanded
            .lines()
            .find(|l| l.contains("Procs") && l.contains("CPU"))
            .unwrap();
        let pid_start = header[..header.find("PID").unwrap()].chars().count();
        let count_start = header[..header.find("Procs").unwrap()].chars().count();
        let cpu_start = header[..header.find("CPU").unwrap()].chars().count();
        assert!(count_start < pid_start, "{header}");
        let table: Vec<_> = expanded
            .lines()
            .skip_while(|l| *l != header)
            .skip(1)
            .take_while(|l| !l.starts_with('└'))
            .collect();
        let processes: Vec<_> = table.iter().filter(|l| l.contains("1161583")).collect();
        assert_eq!(processes.len(), 1, "{expanded}");
        let row = processes[0];
        assert!(row.contains("yazi") && !row.contains("acme/web"), "{row}");
        assert!(!row.contains('▸') && !row.contains('▾'), "{row}");
        assert_eq!(
            row.chars()
                .skip(pid_start)
                .take(cpu_start - pid_start)
                .collect::<String>()
                .trim(),
            "1161583"
        );
        assert!(row
            .chars()
            .skip(count_start)
            .take(pid_start - count_start)
            .all(|c| c == ' '));
        assert!(
            expanded.contains("Working directory: /work/web/src"),
            "{expanded}"
        );
        let expansion = app.cpu.expanded.clone();
        app.handle_key(KeyEvent::from(KeyCode::Enter));
        assert_eq!(app.cpu.expanded, expansion);
        assert_eq!(app.cpu.selected, Some(cpu::RowKey::Process(1161583, None)));
        assert_eq!(render(&app, width, 35), expanded);
        app.handle_key(KeyEvent::from(KeyCode::PageDown));
        assert!(render(&app, width, 35).contains("github.com/acme/web · /work/web"));
    }
}

#[test]
fn memory_totals_and_cpu_colors_render_for_groups_and_processes() {
    for (raw_cpu, text, group_text, color) in [
        (80.0, "10%", "20%", p::status_good()),
        (240.0, "30%", "60%", p::status_warn()),
        (560.0, "70%", "140%", p::status_error()),
    ] {
        for count in [1, 2] {
            let mut app = app();
            let mut snap = app.snap.take().unwrap();
            snap.procs.truncate(1);
            snap.procs[0].cpu_pct = raw_cpu;
            snap.procs[0].mem_rss = 512 * 1024 * 1024;
            if count == 2 {
                let mut second = snap.procs[0].clone();
                second.pid = 44;
                snap.procs.push(second);
            }
            app.history.push(&snap);
            app.snap = Some(snap);
            app.handle_key(KeyEvent::from(KeyCode::Enter));
            let mut terminal = Terminal::new(TestBackend::new(120, 35)).unwrap();
            terminal
                .draw(|frame| draw(frame, frame.area(), &app, app.displayed_snap().unwrap()))
                .unwrap();
            let buffer = terminal.backend().buffer();
            let mut matches = 0;
            for y in 0..35 {
                let line: String = (0..120).map(|x| buffer[(x, y)].symbol()).collect();
                if line.contains("512.0 MB") || line.contains("1.0 GB") {
                    let expected = if line.contains("1.0 GB") {
                        group_text
                    } else {
                        text
                    };
                    assert!(line.contains(expected), "{line}");
                    for x in 0..116 {
                        let value: String = (x..x + expected.len() as u16)
                            .map(|x| buffer[(x, y)].symbol())
                            .collect();
                        if value == expected {
                            assert_eq!(buffer[(x, y)].fg, color);
                            matches += 1;
                        }
                    }
                }
            }
            assert_eq!(matches, if count == 1 { 1 } else { 3 });
        }
    }
}

#[test]
fn assessment_uses_actual_largest_consumer_after_banding() {
    let mut app = app();
    let mut snap = app.snap.take().unwrap();
    snap.procs[0].cpu_pct = 240.0;
    snap.procs[1].cpu_pct = 272.0;
    app.history.push(&snap);
    app.snap = Some(snap);
    let screen = render(&app, 120, 35);
    assert!(
        screen.contains("node is the largest observed CPU consumer (34%"),
        "{screen}"
    );
}

#[test]
fn system_rows_are_marked_and_follow_users_even_when_they_use_more_cpu() {
    let mut app = app();
    let mut snap = app.snap.take().unwrap();
    snap.procs[0].scope = crate::collect::ProcessScope::System;
    snap.procs[0].cpu_pct = 560.0;
    snap.procs[1].scope = crate::collect::ProcessScope::Tool;
    app.history.push(&snap);
    app.snap = Some(snap);
    for mode in [cpu::GroupMode::Application, cpu::GroupMode::Project] {
        app.user_config.cpu_group = mode;
        let screen = render(&app, 140, 40);
        let lines: Vec<_> = screen.lines().collect();
        let system = lines
            .iter()
            .position(|l| l.contains("Google Chrome [system]"))
            .unwrap();
        let user = lines
            .iter()
            .position(|l| {
                l.contains(if mode == cpu::GroupMode::Project {
                    "acme/web"
                } else {
                    "node"
                })
            })
            .unwrap();
        assert!(user < system, "{screen}");
        assert!(
            screen.contains("Google Chrome is the largest observed CPU consumer (70%"),
            "{screen}"
        );
    }
    app.user_config.cpu_group = cpu::GroupMode::Application;
    app.handle_key(KeyEvent::from(KeyCode::Down));
    app.handle_key(KeyEvent::from(KeyCode::Enter));
    let screen = render(&app, 140, 40);
    let leaf = screen
        .lines()
        .find(|l| l.contains("Google Chrome [system]"))
        .unwrap();
    assert!(leaf.contains("42"), "{screen}");
}

#[test]
fn mixed_and_unknown_scopes_remain_combined_and_are_explained_in_details() {
    let mut app = app();
    let mut snap = app.snap.take().unwrap();
    snap.procs.truncate(1);
    snap.procs[0].scope = crate::collect::ProcessScope::System;
    let mut user = snap.procs[0].clone();
    user.pid = 43;
    user.scope = crate::collect::ProcessScope::Tool;
    let mut unknown = user.clone();
    unknown.pid = 44;
    unknown.scope = crate::collect::ProcessScope::Unknown;
    snap.procs.extend([user, unknown]);
    app.history.push(&snap);
    app.snap = Some(snap);
    let screen = render(&app, 140, 40);
    assert!(!screen.contains("[system]"), "{screen}");
    assert!(screen.contains("Google Chrome · 3 processes"), "{screen}");
    assert!(
        screen.contains("Class: 1 system · 1 tools · 1 unknown"),
        "{screen}"
    );
}

#[test]
fn name_fallback_is_explained_and_expands_to_individual_processes() {
    let mut app = app();
    let mut snap = app.snap.take().unwrap();
    snap.procs[0].name = "postgres".into();
    snap.procs[0].exe = None;
    snap.procs[0].user = "?".into();
    snap.procs[1] = snap.procs[0].clone();
    snap.procs[1].pid = 43;
    app.history.push(&snap);
    app.snap = Some(snap);
    let screen = render(&app, 120, 35);
    assert!(screen.contains("postgres · 2 processes"), "{screen}");
    assert!(
        screen.contains("grouped by process name; executable path unavailable"),
        "{screen}"
    );
    app.handle_key(KeyEvent::from(KeyCode::Enter));
    let screen = render(&app, 120, 35);
    for pid in ["42", "43"] {
        assert!(
            screen
                .lines()
                .any(|l| l.contains("postgres") && l.contains(pid)),
            "{screen}"
        );
    }
}

#[test]
fn combined_application_details_show_owners_and_paths_and_can_scroll() {
    let mut app = app();
    let mut snap = app.snap.take().unwrap();
    snap.procs.clear();
    for i in 0..6 {
        snap.procs.push(ProcTick {
            pid: 100 + i,
            name: "node".into(),
            exe: Some(format!("/versions/{i}/node")),
            user: if i == 0 { "root" } else { "alex" }.into(),
            cpu_pct: 8.0,
            ..Default::default()
        });
    }
    app.history.push(&snap);
    app.snap = Some(snap);
    let screen = render(&app, 120, 35);
    assert!(screen.contains("node · 6 processes"), "{screen}");
    assert!(screen.contains("Owners: alex, root"), "{screen}");
    assert!(screen.contains("/versions/0/node"), "{screen}");
    for _ in 0..3 {
        app.handle_key(KeyEvent::from(KeyCode::PageDown));
    }
    let screen = render(&app, 120, 35);
    assert!(screen.contains("/versions/5/node"), "{screen}");
    for _ in 0..3 {
        app.handle_key(KeyEvent::from(KeyCode::PageUp));
    }
    assert!(render(&app, 120, 35).contains("Owners: alex, root"));
}

#[test]
fn project_expansion_reveals_checkout_then_application_then_process() {
    let mut app = app();
    let mut snap = app.snap.take().unwrap();
    let mut second = snap.procs[1].clone();
    second.pid = 44;
    second.cpu_pct = 0.0;
    snap.procs.push(second);
    app.snap = Some(snap);
    app.handle_key(KeyEvent::from(KeyCode::Char('b')));
    // Chrome remains first at 30%, project follows at 20%.
    app.handle_key(KeyEvent::from(KeyCode::Down));
    app.handle_key(KeyEvent::from(KeyCode::Enter));
    let screen = render(&app, 120, 35);
    assert!(screen.contains("acme/web"), "{screen}");
    assert!(screen.contains("/work/web"), "{screen}");
    app.handle_key(KeyEvent::from(KeyCode::Down));
    app.handle_key(KeyEvent::from(KeyCode::Enter));
    app.handle_key(KeyEvent::from(KeyCode::Down));
    let screen = render(&app, 120, 35);
    assert!(screen.contains("node"), "{screen}");
    assert!(
        !screen
            .lines()
            .any(|l| l.contains("node") && l.contains("43")),
        "{screen}"
    );
    app.handle_key(KeyEvent::from(KeyCode::Enter));
    app.handle_key(KeyEvent::from(KeyCode::Down));
    let screen = render(&app, 120, 35);
    assert!(
        screen
            .lines()
            .any(|l| l.contains("node") && l.contains("43")),
        "{screen}"
    );
    assert!(screen.contains("working directory"), "{screen}");
}

/// Moving up from the bottom of a long consumer list moves the cursor, not
/// the list — the same windowing contract as the Procs and Services tables.
#[test]
fn cursor_moves_up_without_scrolling_the_list() {
    let mut app = app();
    let mut snap = app.snap.take().unwrap();
    snap.procs = (0..40)
        .map(|i| ProcTick {
            pid: 1000 + i,
            name: format!("tool{i:02}"),
            exe: Some(format!("/opt/tool{i:02}")),
            cpu_pct: 80.0 - i as f32,
            ..Default::default()
        })
        .collect();
    app.history.push(&snap);
    app.snap = Some(snap);
    // The table row — the only line naming both the tool and its PID, unlike
    // the assessment and detail text.
    let row_of = |screen: &str, i: u32| {
        let (name, pid) = (format!("tool{i:02}"), (1000 + i).to_string());
        screen
            .lines()
            .position(|l| l.contains(&name) && l.contains(&pid))
    };
    // Draw after every press, like the event loop: only rendering knows how
    // many rows fit, so that is where the window moves.
    let press = |app: &mut App, key: KeyCode, n: usize| {
        for _ in 0..n {
            app.handle_key(KeyEvent::from(key));
            render(app, 120, 35);
        }
    };

    press(&mut app, KeyCode::Down, 39);
    let bottom = render(&app, 120, 35);
    let last = row_of(&bottom, 39).expect("last consumer on screen");

    press(&mut app, KeyCode::Up, 3);
    let after = render(&app, 120, 35);
    assert_eq!(
        row_of(&after, 39),
        Some(last),
        "the list scrolled under the cursor:\n{after}"
    );
    assert_eq!(row_of(&after, 36), Some(last - 3), "{after}");

    // Back up past the top of the window, the list does scroll — and the
    // selection becomes its first row.
    press(&mut app, KeyCode::Up, 36);
    let top = render(&app, 120, 35);
    let first = row_of(&top, 0).expect("first consumer on screen");
    assert_eq!(row_of(&top, 1), Some(first + 1), "{top}");
}

#[test]
fn scrubbing_uses_historical_groups_and_assessment() {
    let mut app = app();
    let mut later = app.snap.as_ref().unwrap().clone();
    later.t += std::time::Duration::from_secs(5);
    later.procs.clear();
    later.cpu.usage_pct = 99.0;
    app.history.push(&later);
    app.snap = Some(later);
    assert!(!render(&app, 120, 35).contains("Google Chrome"));
    app.handle_key(KeyEvent::from(KeyCode::Left));
    let screen = render(&app, 120, 35);
    assert!(screen.contains("Google Chrome"));
    assert!(screen.contains("contention measurement unavailable"));
}

#[test]
fn tiny_terminal_does_not_panic() {
    let app = app();
    for (w, h) in [(1, 1), (20, 5), (40, 10)] {
        render(&app, w, h);
    }
}

#[test]
fn compact_terminal_shows_selected_process_and_core_pages() {
    let mut app = app();
    app.handle_key(KeyEvent::from(KeyCode::Enter));
    assert!(render(&app, 80, 19).contains("PID 42 · parent"));
    app.handle_key(KeyEvent::from(KeyCode::Char('c')));
    assert!(render(&app, 80, 19).contains("c0"));
    let mut snap = app.snap.take().unwrap();
    snap.cpu.per_core = vec![10.0; 128];
    app.history.push(&snap);
    app.snap = Some(snap);
    app.handle_key(KeyEvent::from(KeyCode::PageDown));
    assert!(render(&app, 80, 19).contains("c10"));
    app.handle_key(KeyEvent::from(KeyCode::PageUp));
    assert!(render(&app, 80, 19).contains("c0"));
}

#[test]
fn recent_column_uses_more_history_on_wide_terminals() {
    let mut app = app();
    let snap = app.snap.as_ref().unwrap().clone();
    for _ in 0..40 {
        app.history.push(&snap);
    }
    let screen = render(&app, 120, 35);
    let row = screen
        .lines()
        .find(|line| line.contains("Google Chrome") && line.contains("42"))
        .unwrap();
    assert!(
        row.chars().filter(|c| "▁▂▃▄▅▆▇█".contains(*c)).count() >= 28,
        "{row}"
    );
}

#[test]
fn deleted_executable_is_muted_and_explained_even_when_selected() {
    let mut app = app();
    let mut snap = app.snap.take().unwrap();
    snap.procs[0].exe = Some("/opt/worker (deleted)".into());
    snap.procs[0].name = "worker".into();
    app.history.push(&snap);
    app.snap = Some(snap);
    let mut terminal = Terminal::new(TestBackend::new(120, 35)).unwrap();
    terminal
        .draw(|frame| draw(frame, frame.area(), &app, app.displayed_snap().unwrap()))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let mut found = false;
    for y in 0..35 {
        let line: String = (0..120).map(|x| buffer[(x, y)].symbol()).collect();
        if line.contains("42") && line.contains("(deleted)") {
            for x in 0..111 {
                let suffix: String = (x..x + 9).map(|x| buffer[(x, y)].symbol()).collect();
                if suffix == "(deleted)" {
                    assert_eq!(buffer[(x, y)].fg, p::text_muted());
                    found = true;
                }
            }
        }
    }
    assert!(found);
    assert!(render(&app, 120, 35).contains("unlinked"));
    app.handle_key(KeyEvent::from(KeyCode::Enter));
    assert!(render(&app, 80, 19).contains("unlinked"));
}
