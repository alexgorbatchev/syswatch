use super::*;
use crate::app::TabId;
use crate::collect::{CpuTick, ProcTick, ProjectIdentity};
use crate::config::SyswatchConfig;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{backend::TestBackend, Terminal};

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
fn memory_totals_and_cpu_colors_render_for_groups_and_processes() {
    for (raw_cpu, text, color) in [
        (80.0, "10%", p::status_good()),
        (240.0, "30%", p::status_warn()),
        (560.0, "70%", p::status_error()),
    ] {
        let mut app = app();
        let mut snap = app.snap.take().unwrap();
        snap.procs.truncate(1);
        snap.procs[0].cpu_pct = raw_cpu;
        snap.procs[0].mem_rss = 512 * 1024 * 1024;
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
            if line.contains("512.0 MB") {
                assert!(line.contains(text), "{line}");
                for x in 0..116 {
                    let value: String = (x..x + text.len() as u16)
                        .map(|x| buffer[(x, y)].symbol())
                        .collect();
                    if value == text {
                        assert_eq!(buffer[(x, y)].fg, color);
                        matches += 1;
                    }
                }
            }
        }
        assert_eq!(matches, 2);
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
    assert!(screen.contains("postgres  PID 42"), "{screen}");
    assert!(screen.contains("postgres  PID 43"), "{screen}");
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
    assert!(!screen.contains("PID 43"), "{screen}");
    app.handle_key(KeyEvent::from(KeyCode::Enter));
    app.handle_key(KeyEvent::from(KeyCode::Down));
    let screen = render(&app, 120, 35);
    assert!(screen.contains("PID 43"), "{screen}");
    assert!(screen.contains("working directory"), "{screen}");
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
    app.handle_key(KeyEvent::from(KeyCode::Down));
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
        .find(|line| line.contains("▸") && line.contains("Google Chrome"))
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
        if line.contains("▸") && line.contains("(deleted)") {
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
    app.handle_key(KeyEvent::from(KeyCode::Down));
    assert!(render(&app, 80, 19).contains("unlinked"));
}
