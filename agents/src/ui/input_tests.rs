use super::*;
use crate::providers::Provider;

fn fixture() -> (tempfile::TempDir, View) {
    let directory = tempfile::tempdir().unwrap();
    let project = directory.path().join("config/projects/probe");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        project.join("4165651d-b9e3-458d-8612-f8114e312363.jsonl"),
        include_bytes!("../../tests/fixtures/transcript.jsonl"),
    )
    .unwrap();
    let mut provider =
        crate::providers::claude::ClaudeProvider::new(directory.path().join("config"));
    let mut sessions = provider.refresh(1).sessions;
    assert_eq!(sessions.len(), 1);
    sessions[0].cwd = directory.path().into();
    sessions[0].status = SessionStatus::Ready;
    let mut view = View::default();
    view.set_sessions(sessions, false);
    let mut buffer = Buffer::empty(Rect::new(0, 0, 120, 40));
    view.render_at(buffer.area, &mut buffer, Utc::now());
    (directory, view)
}
fn click(view: &mut View, x: u16, y: u16) -> Option<Action> {
    let event = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: x,
        row: y,
        modifiers: KeyModifiers::NONE,
    };
    assert!(view.handle_mouse(event).is_none());
    view.handle_mouse(MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        ..event
    })
}
#[test]
fn heading_only_path_opens_but_entire_row_double_clicks() {
    let (directory, mut view) = fixture();
    let mut buffer = Buffer::empty(Rect::new(0, 0, 300, 40));
    view.render_at(buffer.area, &mut buffer, Utc::now());
    let (path, _) = view.viewport.borrow().list_links[0].clone();
    let edge = view.viewport.borrow().list_rows.right() - 1;
    assert!(edge >= path.right());
    assert!(click(&mut view, edge, path.y).is_none());
    assert_eq!(
        view.flush_pending_click(Instant::now() + view.double_click_interval),
        None
    );
    assert!(click(&mut view, edge, path.y).is_none());
    assert_eq!(
        click(&mut view, path.x, path.y),
        Some(Action::NewDraft {
            cwd: directory.path().into()
        })
    );
    assert!(click(&mut view, path.x, path.y).is_none());
    assert_eq!(
        view.flush_pending_click(Instant::now() + view.double_click_interval),
        Some(Action::OpenPath(directory.path().into()))
    );
}
#[test]
fn different_target_runs_pending_path_before_new_click() {
    let (directory, mut view) = fixture();
    let (path, _) = view.viewport.borrow().list_links[0].clone();
    let (_, tab) = view.viewport.borrow().tabs[1];
    assert!(click(&mut view, path.x, path.y).is_none());
    assert_eq!(
        click(&mut view, tab.x, tab.y),
        Some(Action::OpenPath(directory.path().into()))
    );
    assert_eq!(view.status_filter, 1);
}
#[test]
fn links_without_double_action_open_each_click_immediately() {
    let (_directory, mut view) = fixture();
    let (rect, target) = view
        .viewport
        .borrow()
        .details_links
        .iter()
        .find(|(rect, _)| {
            !view
                .viewport
                .borrow()
                .details_copy
                .iter()
                .any(|(rows, _, _)| rows.contains(&rect.y))
        })
        .cloned()
        .expect("directory link outside copy bodies");
    assert_eq!(
        click(&mut view, rect.x, rect.y),
        Some(Action::OpenPath(target.clone()))
    );
    assert_eq!(
        click(&mut view, rect.x, rect.y),
        Some(Action::OpenPath(target))
    );
    assert!(view.pending_click.is_none());
}

#[test]
fn pending_path_precedes_immediate_link_action() {
    let (directory, mut view) = fixture();
    let (heading, _) = view.viewport.borrow().list_links[0].clone();
    let (link, target) = view
        .viewport
        .borrow()
        .details_links
        .iter()
        .find(|(rect, _)| {
            !view
                .viewport
                .borrow()
                .details_copy
                .iter()
                .any(|(rows, _, _)| rows.contains(&rect.y))
        })
        .cloned()
        .expect("directory link outside copy bodies");
    assert!(click(&mut view, heading.x, heading.y).is_none());
    assert_eq!(
        click(&mut view, link.x, link.y),
        Some(Action::Batch(vec![
            Action::OpenPath(directory.path().into()),
            Action::OpenPath(target)
        ]))
    );
}
