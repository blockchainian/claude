use super::*;
use crate::log::Event;

fn display_view() -> View {
    use crate::providers::Provider;
    let config = tempfile::tempdir().unwrap();
    crate::providers::claude::write_fixture_cache(config.path());
    std::fs::write(
        config.path().join("models_cache.json"),
        include_str!("../../tests/fixtures/codex-models-cache.json"),
    )
    .unwrap();
    let claude = crate::providers::claude::ClaudeProvider::new(config.path().into());
    let codex = crate::providers::codex::CodexProvider::new(
        config.path().into(),
        config.path().join("manager"),
    );
    let mut view = test_view(Grouping::Project, false);
    view.set_input_choices(HashMap::from([
        (ProviderId::Claude, claude.input_choices()),
        (ProviderId::Codex, codex.input_choices()),
    ]));
    view
}

fn claude_events(name: &str) -> Vec<Event> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    crate::providers::claude::log::tail(&path, usize::MAX).unwrap()
}

fn spawn_event(events: &[Event]) -> &Event {
    events
        .iter()
        .find(|event| {
            event.calls.iter().any(|call| {
                call.agent_message
                    .as_ref()
                    .is_some_and(|message| message.kind == "SPAWN_AGENT")
            })
        })
        .expect("real fixture must contain a spawn")
}

fn panel(view: &mut View, event: &Event) -> String {
    let row = session("spawn", "Spawn", SessionStatus::Ready, 0);
    view.set_sessions(vec![row], false);
    view.log_session = Some(view.selected_row().unwrap().id.clone());
    view.set_log(vec![event.clone()]);
    let area = Rect::new(0, 0, 240, 42);
    let mut buffer = Buffer::empty(area);
    view.render_at(area, &mut buffer, DateTime::UNIX_EPOCH);
    let detail = view.viewport.borrow().log_detail;
    buffer
        .content
        .chunks(240)
        .skip(usize::from(detail.y))
        .take(usize::from(detail.height))
        .map(|row| {
            let row = &row[usize::from(detail.x)..usize::from(detail.right())];
            let mut result = String::new();
            let mut cells = row.iter();
            while let Some(cell) = cells.next() {
                result.push_str(cell.symbol());
                for _ in 1..cell.symbol().width() {
                    cells.next();
                }
            }
            result.trim_end().to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim_end()
        .to_owned()
}

fn snapshot(name: &str, text: &str) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/ui_snapshots")
        .join(name);
    let actual = format!("{text}\n");
    if std::env::var_os("UPDATE_DETAIL_SNAPSHOT").is_some() {
        std::fs::write(&path, &actual).unwrap();
    }
    assert_eq!(actual, std::fs::read_to_string(path).unwrap());
}

#[test]
fn spawn_agent_claude_background_name_model_and_hidden_receipt() {
    let events = claude_events("claude-agent-launch.jsonl");
    let event = spawn_event(&events);
    let mut view = display_view();
    assert_eq!(event.summary(), "Spawn agent 查 Claude 前台转后台");
    let shown = view.display_event(event);
    assert_eq!(
        shown.calls[0].detail_title,
        "查 Claude 前台转后台 · Sonnet 5.5"
    );
    let body = view.detail_text(event);
    assert!(
        !body.contains("Async agent launched successfully"),
        "{body}"
    );
    assert!(!body.contains("agentId:"), "{body}");
    assert!(!body.contains("Default"), "{body}");
    let rendered = panel(&mut view, event);
    assert!(
        rendered.contains("查 Claude 前台转后台 · Sonnet 5.5"),
        "{rendered}"
    );
    snapshot("spawn_agent_claude_background.txt", &rendered);
}

#[test]
fn spawn_agent_codex_model_and_hidden_receipt() {
    use crate::providers::{Provider, codex::CodexProvider};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("parent.jsonl");
    std::fs::write(
        &path,
        include_str!("../../tests/fixtures/codex-interaction/parent.jsonl"),
    )
    .unwrap();
    let provider = CodexProvider::new(dir.path().into(), dir.path().join("manager"));
    let mut session = fixture().rows.remove(0);
    session.transcript = Some(path);
    let events = provider.log(&session, usize::MAX, false).unwrap();
    let event = spawn_event(&events);
    let mut view = display_view();
    assert_eq!(event.summary(), "Spawn agent demo_sub");
    let shown = view.display_event(event);
    assert_eq!(shown.calls[0].detail_title, "demo_sub · GPT-6.1-Sol · low");
    let body = view.detail_text(event);
    assert!(!body.contains("task_name"), "{body}");
    snapshot("spawn_agent_codex.txt", &panel(&mut view, event));
}
