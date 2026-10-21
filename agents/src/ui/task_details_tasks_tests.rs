use super::*;
use crate::model::{SessionTask, TaskStatus};

fn tasks_view() -> View {
    let mut row = session("tasks", "Session title", SessionStatus::NeedsInput, 0);
    row.last_message = Some("Task update".into());
    row.tasks = [
        ("1", "Pending first", TaskStatus::Pending),
        ("2", "Working first", TaskStatus::InProgress),
        ("3", "Finished", TaskStatus::Completed),
        ("4", "Removed", TaskStatus::Deleted),
        ("5", "Pending second", TaskStatus::Pending),
        ("6", "Working second", TaskStatus::InProgress),
    ]
    .into_iter()
    .map(|(id, subject, status)| SessionTask {
        id: id.into(),
        subject: subject.into(),
        status,
    })
    .collect();
    let mut view = test_view(Grouping::Project, true);
    view.set_sessions(vec![row], false);
    view
}

fn render(view: &View, width: u16, height: u16) -> (Buffer, Vec<String>) {
    let area = Rect::new(0, 0, width, height);
    let mut buf = Buffer::empty(area);
    view.render_task_details(area, &mut buf, Utc::now());
    let rows = (0..height)
        .map(|y| (0..width).map(|x| buf[(x, y)].symbol()).collect::<String>())
        .collect();
    (buf, rows)
}

#[test]
fn tasks_section_orders_filters_counts_and_reuses_status_styles() {
    let view = tasks_view();
    let (buf, rows) = render(&view, 40, 40);
    let find = |s: &str| rows.iter().position(|line| line.contains(s)).unwrap();
    assert!(find("Prompt") < find("Tasks"));
    assert!(find("Update") < find("Tasks"));
    assert!(find("Update") < find("Prompt"));
    assert!(find("Needs input") < find("Tasks"));
    assert!(!rows.iter().any(|row| row.trim() == "Project"));
    assert_eq!(rows[find("Tasks")].find("Tasks"), Some(17));
    assert!(rows[find("Tasks")].ends_with("2/4"));
    assert!(
        !rows
            .iter()
            .any(|row| row.contains("Finished") || row.contains("Removed"))
    );
    let first = find("Working first");
    assert_eq!(rows[first].trim_end(), "● Working first");
    assert_eq!(rows[first + 1].trim_end(), "● Working second");
    assert_eq!(rows[first + 2].trim_end(), "○ Pending first");
    assert_eq!(rows[first + 3].trim_end(), "○ Pending second");
    for (y, status) in [
        (first, SessionStatus::Working),
        (first + 2, SessionStatus::Ready),
    ] {
        let reference = session("style", "Style", status, 0);
        let dot = View::status(&reference).1;
        assert_eq!(buf[(0, y as u16)].symbol(), dot.content);
        assert_eq!(buf[(0, y as u16)].fg, dot.style.fg.unwrap());
    }
    assert!(rows[36].trim().is_empty());
}

#[test]
fn tasks_hide_empty_sections_and_truncate_subjects() {
    let mut view = tasks_view();
    view.rows[view.selected]
        .tasks
        .retain(|task| matches!(task.status, TaskStatus::Completed | TaskStatus::Deleted));
    assert!(
        !render(&view, 40, 40)
            .1
            .iter()
            .any(|row| row.contains("Tasks"))
    );
    view.rows[view.selected].tasks = vec![SessionTask {
        id: "1".into(),
        subject: "A long task subject that cannot fit".into(),
        status: TaskStatus::InProgress,
    }];
    let (_, rows) = render(&view, 20, 40);
    assert!(
        rows.iter().any(|row| row == "● A long task subje…"),
        "{rows:?}"
    );
}

#[test]
fn tasks_double_click_copies_visible_subjects() {
    let mut view = tasks_view();
    let (buf, rows) = render(&view, 40, 40);
    let y = rows
        .iter()
        .position(|row| row.contains("Working first"))
        .unwrap() as u16;
    let click = mouse(MouseEventKind::Down(MouseButton::Left), 3, y);
    assert_eq!(click_mouse(&mut view, click), None);
    assert_eq!(
        click_mouse(&mut view, click),
        Some(Action::Copy {
            text: "● Working first\n● Working second\n○ Pending first\n○ Pending second".into(),
            label: "Copied tasks".into(),
        })
    );
    assert_eq!(buf.area.height, 40);
}

#[test]
fn tasks_one_in_progress_and_one_pending_snapshot() {
    let mut view = tasks_view();
    view.rows[view.selected].tasks.truncate(4);
    let (_, rows) = render(&view, 40, 24);
    let text = rows
        .iter()
        .map(|row| row.trim_end())
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(
        text,
        include_str!("../../tests/ui_snapshots/task_details_tasks.txt")
    );
}

#[test]
fn tasks_long_list_uses_task_details_scroll_and_keeps_other_sections() {
    let mut view = tasks_view();
    view.rows[view.selected].tasks = (1..=30)
        .map(|id| SessionTask {
            id: id.to_string(),
            subject: format!("Task row {id}"),
            status: TaskStatus::Pending,
        })
        .collect();
    let (_, before) = render(&view, 40, 24);
    let y = before
        .iter()
        .position(|row| row.contains("Task row 1"))
        .unwrap() as u16;
    assert!(before.iter().any(|row| row.trim_end().ends_with('…')));
    assert!(!before.iter().any(|row| row.trim() == "Project"));
    view.scroll_content_row(true, false, y);
    let (_, after) = render(&view, 40, 24);
    assert_eq!(
        view.viewport
            .borrow()
            .task_details_scroll
            .reveal
            .iter()
            .sum::<usize>(),
        1
    );
    assert!(after.iter().any(|row| row.contains("Task row 2")));
    assert!(before.iter().any(|row| row.trim() == "Prompt"));
    assert_eq!(view.viewport.borrow().task_details_scroll.reveal[0], 0);
    assert_eq!(view.viewport.borrow().task_details_scroll.reveal[3], 1);
    assert_eq!(before[0].trim(), "Update");
    assert_eq!(after[0].trim(), "Task update");
    for label in ["Prompt", "Needs input"] {
        let position = |rows: &[String]| rows.iter().position(|row| row.trim() == label).unwrap();
        assert_eq!(position(&before), position(&after) + 1);
    }
    view.scroll_content_row(false, false, y.saturating_sub(1));
    assert_eq!(render(&view, 40, 24).1, before);
}
