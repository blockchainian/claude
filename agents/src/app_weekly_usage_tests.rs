use super::*;
use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::backend::TestBackend;
use unicode_width::UnicodeWidthStr;

fn app(directory: &std::path::Path) -> App {
    App::new(
        Hub::new(vec![
            Box::new(crate::providers::claude::ClaudeProvider::new(
                directory.into(),
            )),
            Box::new(crate::providers::codex::CodexProvider::new(
                directory.into(),
                directory.join("manager"),
            )),
        ]),
        directory.into(),
    )
}

fn draw(app: &mut App, width: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, 40)).unwrap();
    app.draw(&mut terminal).unwrap();
    terminal
        .backend()
        .buffer()
        .content
        .chunks(usize::from(width))
        .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
        .find(|row| row.contains("Weekly "))
        .expect("weekly row on first render")
}

#[test]
fn weekly_usage_first_frame_click_persistence_and_immediate_refresh() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("agents.json"),
        r#"{
        "config":{"grouping":"project"},
        "state":{"usage_view":"compact","usage":{"weekly":{
            "claude":{"used_percent":21,"resets_at":"2026-10-23T15:59:59.837Z","reset_credits":1},
            "codex":{"used_percent":16,"resets_at":"2026-10-25T08:32:50Z","reset_credits":2}
        }}}
    }"#,
    )
    .unwrap();
    let mut app = app(directory.path());
    let first = draw(&mut app, 200);
    assert!(
        first.contains("Weekly Claude 79% left · Codex 84% left"),
        "{first}"
    );
    // Loading cached values does not schedule or throttle the first account requests.
    assert_eq!(app.hub.weekly_usage_jobs(Instant::now()).len(), 2);
    let start = first.find("Weekly ").unwrap() as u16;
    let y = 0;
    for kind in [
        MouseEventKind::Down(MouseButton::Left),
        MouseEventKind::Up(MouseButton::Left),
    ] {
        app.handle_event(Event::Mouse(MouseEvent {
            kind,
            column: start,
            row: y,
            modifiers: crossterm::event::KeyModifiers::NONE,
        }));
    }
    assert_eq!(
        Preferences::load(directory.path()).usage_view,
        crate::state::UsageView::Detailed
    );
    let detailed = draw(&mut app, 200);
    let claude_reset: chrono::DateTime<chrono::Utc> = "2026-10-23T16:00:00Z".parse().unwrap();
    let codex_reset: chrono::DateTime<chrono::Utc> = "2026-10-25T08:33:00Z".parse().unwrap();
    let expected = format!(
        "Weekly Claude 79% left until {}, 1 reset · Codex 84% left until {}, 2 resets",
        claude_reset
            .with_timezone(&chrono::Local)
            .format("%a %H:%M"),
        codex_reset.with_timezone(&chrono::Local).format("%a %H:%M")
    );
    assert!(detailed.contains(&expected), "{detailed}");
    let mut restarted = self::app(directory.path());
    assert!(draw(&mut restarted, 200).contains(&expected));
    let end = detailed.find(&expected).unwrap() + expected.width() - 1;
    for kind in [
        MouseEventKind::Down(MouseButton::Left),
        MouseEventKind::Up(MouseButton::Left),
    ] {
        app.handle_event(Event::Mouse(MouseEvent {
            kind,
            column: end as u16,
            row: y,
            modifiers: crossterm::event::KeyModifiers::NONE,
        }));
    }
    assert_eq!(
        Preferences::load(directory.path()).usage_view,
        crate::state::UsageView::Compact
    );
    if let Some(path) = std::env::var_os("AGENTS_WEEKLY_PROBE_RESULT") {
        std::fs::write(path, serde_json::to_vec(&serde_json::json!({"verdict":"passed","compact":first.trim(),"detailed":detailed.trim()})).unwrap()).unwrap();
    }
}

#[test]
fn weekly_usage_refresh_updates_display_and_only_its_provider_state() {
    let directory = tempfile::tempdir().unwrap();
    let mut app = app(directory.path());
    let mut other_process = Preferences::load(directory.path());
    other_process.usage_view = crate::state::UsageView::Detailed;
    other_process.save_usage_view(directory.path()).unwrap();
    let mut claude = crate::providers::WeeklyUsage::from_used_percent(21.4, None).unwrap();
    claude.reset_credits = Some(1);
    let codex = crate::providers::WeeklyUsage::from_used_percent(16.0, None).unwrap();
    app.complete_weekly_usage(ProviderId::Claude, Ok(Some(claude.clone())));
    app.complete_weekly_usage(ProviderId::Codex, Ok(Some(codex.clone())));
    assert!(draw(&mut app, 200).contains("Weekly Claude 79% left · Codex 84% left"));
    let saved = Preferences::load(directory.path());
    assert_eq!(saved.usage_view, crate::state::UsageView::Detailed);
    assert_eq!(saved.usage.get(&ProviderId::Claude), Some(&claude));
    assert_eq!(saved.usage.get(&ProviderId::Codex), Some(&codex));
    app.complete_weekly_usage(ProviderId::Claude, Err(io::Error::other("unavailable")));
    app.complete_weekly_usage(ProviderId::Codex, Ok(None));
    assert_eq!(Preferences::load(directory.path()), saved);
}
