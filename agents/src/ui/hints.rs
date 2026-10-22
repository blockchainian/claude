use super::shortcut_help::{Group, Shortcut};
use super::*;

pub(super) fn key_span(key: &str) -> Span<'static> {
    super::key_name::display(key).bold()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum HintAction {
    Help,
    Question(question_view::FooterAction),
}

#[derive(Clone, Copy, Debug)]
pub(super) struct HintTarget {
    pub area: Rect,
    pub action: HintAction,
}

/// Records only the visible key cells, splitting arrow combinations at their slash.
pub(super) fn action_targets(
    line: &Line<'_>,
    area: Rect,
    actions: &[(&str, HintAction)],
) -> Vec<HintTarget> {
    let mut targets = Vec::new();
    let mut x = area.x;
    for span in &line.spans {
        if span
            .style
            .add_modifier
            .contains(ratatui::style::Modifier::BOLD)
        {
            let parts: Vec<_> = if span.content.contains('/') && span.content.as_ref() != "/" {
                span.content.split('/').collect()
            } else {
                vec![span.content.as_ref()]
            };
            let mut offset = 0;
            for part in parts {
                let width = part.width() as u16;
                if let Some((_, action)) = actions.iter().find(|(label, _)| *label == part) {
                    let rect =
                        Rect::new(x.saturating_add(offset), area.y, width, 1).intersection(area);
                    if !rect.is_empty() {
                        targets.push(HintTarget {
                            area: rect,
                            action: *action,
                        });
                    }
                }
                offset += width + 1;
            }
        }
        x = x.saturating_add(span.width() as u16);
    }
    targets
}

impl View {
    pub(super) fn center_footer_hints(&self) -> Line<'static> {
        Line::from(vec!["?".bold(), " help".dim().not_bold()])
    }
    /// Whether the footer shows the task title outside dialogs, editors and searches.
    pub(super) fn footer_shows_task_title(&self) -> bool {
        !(self.confirm_dialog.is_some()
            || self.help
            || (self.active_panel == Panel::TaskDetails && self.title_editor().is_some())
            || (self.active_panel == Panel::Prompt && self.active_question().is_some())
            || (self.selected_draft().is_some()
                && (self.prompt_focused()
                    || (self.active_panel == Panel::TaskDetails && self.selector.is_some())))
            || self.task_search_editing())
    }

    pub(super) fn center_help_lines(&self, width: u16) -> Vec<Line<'static>> {
        let group = |title, entries: &[(&str, &'static str)]| Group {
            title,
            entries: entries
                .iter()
                .map(|(key, action)| Shortcut {
                    key: (*key).into(),
                    action,
                })
                .collect(),
        };
        let groups = [
            group(
                "Navigate",
                &[
                    ("↑", "Up"),
                    ("↓", "Down"),
                    ("⌃w h/j/k/l", "Focus left/down/up/right"),
                    ("⌃w _", "Max height"),
                    ("⌃w |", "Max width"),
                    ("⌃w =", "Reset size"),
                    ("enter", "Open"),
                    ("→", "Open"),
                    ("pgup", "Page up"),
                    ("pgdn", "Page down"),
                    ("⌃c", "Quit"),
                    ("esc", "Previous"),
                ],
            ),
            group(
                "Tasks",
                &[
                    ("n", "New"),
                    ("o", "Open"),
                    ("f", "Fork"),
                    ("r", "Rename"),
                    ("x", "Stop"),
                    ("a", "Archive"),
                    ("h", "Hide"),
                    ("backspace", "Delete"),
                    ("p", "Pin/unpin"),
                ],
            ),
            group(
                "View",
                &[("tab/⇧tab", "Filter"), ("/", "Search"), ("g", "Group")],
            ),
            group(
                "Events",
                &[
                    ("/", "Search"),
                    ("t", "Sort"),
                    ("space", "Messages/tools"),
                    ("2× click top border", "Earliest"),
                    ("2× click bottom border", "Latest"),
                ],
            ),
        ];
        let mut lines = vec!["Task shortcuts".bold().into(), Line::default()];
        for line in super::shortcut_help::group_lines(groups, width) {
            if line.width() <= usize::from(width) {
                lines.push(line);
            } else {
                lines.extend(
                    textwrap::wrap(&line.to_string(), usize::from(width.max(1)))
                        .into_iter()
                        .map(|s| Line::from(s.into_owned())),
                );
            }
        }
        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(view: &View) -> Buffer {
        let area = Rect::new(0, 0, 160, 50);
        let mut buffer = Buffer::empty(area);
        view.render(area, &mut buffer);
        buffer
    }

    fn click(view: &mut View, x: u16, y: u16) -> Option<Action> {
        let event = |kind| MouseEvent {
            kind,
            column: x,
            row: y,
            modifiers: KeyModifiers::NONE,
        };
        view.handle_mouse(event(MouseEventKind::Down(MouseButton::Left)));
        view.handle_mouse(event(MouseEventKind::Up(MouseButton::Left)))
    }

    #[test]
    fn footer_help_click_preserves_focused_input() {
        for panel in [
            Panel::Prompt,
            Panel::TaskDetails,
            Panel::TaskList,
            Panel::EventList,
            Panel::EventDetail,
        ] {
            let mut view = View::default();
            view.add_draft(DraftSetup {
                provider: ProviderId::Claude,
                cwd: crate::test_project(),
                model: None,
                effort: None,
                branch: "main".into(),
            });
            if panel != Panel::TaskDetails {
                view.name_target = None;
            }
            view.set_active_panel(panel);
            view.searching = panel == Panel::TaskList;
            view.event_list.search.editing = panel == Panel::EventList;
            view.event_detail.search.editing = panel == Panel::EventDetail;
            view.input_box.editor.set_text("draft");
            view.rename.set_text("title");
            view.search.set_text("search");
            view.event_list.search.query.set_text("event");
            view.event_detail.search.query.set_text("detail");
            render(&view);
            let target = view
                .viewport
                .borrow()
                .hint_targets
                .iter()
                .find(|target| {
                    target.area.y == view.center_layout(Rect::new(0, 0, 160, 50)).footer.y
                })
                .copied()
                .expect("help target");
            click(&mut view, target.area.x, target.area.y);
            assert!(view.help, "panel {panel:?}");
            assert_eq!(view.input_box.editor.text(), "draft");
            assert_eq!(view.rename.text(), "title");
            assert_eq!(view.search.text(), "search");
            assert_eq!(view.event_list.search.query.text(), "event");
            assert_eq!(view.event_detail.search.query.text(), "detail");
            assert_eq!(view.active_panel, panel);
            let buffer = render(&view);
            let text = buffer
                .content
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>();
            assert!(text.contains("Task shortcuts"), "panel {panel:?}");
            view.handle_key(KeyCode::Esc.into());
            assert!(!view.help, "help did not close in {panel:?}");
            assert_eq!(view.input_box.editor.text(), "draft");
            assert_eq!(view.rename.text(), "title");
            assert_eq!(view.search.text(), "search");
            assert_eq!(view.event_list.search.query.text(), "event");
            assert_eq!(view.event_detail.search.query.text(), "detail");
        }
    }

    #[test]
    fn help_key_columns_do_not_handle_clicks() {
        let mut view = View {
            help: true,
            ..View::default()
        };
        let buffer = render(&view);
        let (row, x) = (0..buffer.area.height - 1)
            .find_map(|y| {
                let line = (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>();
                line.to_lowercase()
                    .find("enter")
                    .map(|start| (y, line[..start].width() as u16))
            })
            .expect("help Enter key");
        assert!(click(&mut view, x, row).is_none());
        assert!(view.help);
        assert_eq!(render(&view), buffer);
    }

    #[test]
    fn footer_only_shows_help_in_every_state() {
        for mode in 0..8 {
            let mut view = View::default();
            match mode {
                0 => {}
                1 => view.help = true,
                2 => view.searching = true,
                3 => view.active_panel = Panel::Prompt,
                4 => {
                    view.confirm_dialog = Some(ConfirmDialog {
                        action: ConfirmAction::Delete("claude:footer".parse().unwrap()),
                        title: "Task".into(),
                        confirm: false,
                    })
                }
                5 => {
                    view.rename_target = Some("claude:footer".parse().unwrap());
                    view.set_active_panel(Panel::TaskDetails);
                }
                6 => {
                    view.set_active_panel(Panel::EventList);
                    view.event_list.search.editing = true;
                }
                7 => {
                    view.set_active_panel(Panel::EventDetail);
                    view.event_detail.search.editing = true;
                }
                _ => unreachable!(),
            }
            let line = view.center_footer_hints();
            assert_eq!(line.to_string(), "? help", "mode {mode}");
            assert!(
                line.spans[0]
                    .style
                    .add_modifier
                    .contains(ratatui::style::Modifier::BOLD)
            );
            assert!(
                line.spans[1]
                    .style
                    .add_modifier
                    .contains(ratatui::style::Modifier::DIM)
            );
        }
    }

    #[test]
    fn task_help_describes_open_keys_without_control_l() {
        let view = View::default();
        let lines = view.center_help_lines(120);
        for key in ["Enter", "→", "o"] {
            assert!(
                lines.iter().any(|line| {
                    let text = line.to_string();
                    text.split_whitespace()
                        .collect::<Vec<_>>()
                        .windows(2)
                        .any(|words| words == [key, "Open"])
                }),
                "missing {key} Open"
            );
        }
        assert!(lines.iter().all(|line| !line.to_string().contains("⌃l")));
    }

    #[test]
    fn event_help_includes_duration_sort_key() {
        let view = View::default();
        assert!(view.center_help_lines(120).iter().any(|line| {
            let text = line.to_string();
            text.contains("t") && text.contains("Sort")
        }));
    }

    #[test]
    fn grouping_key_works_without_a_footer_hint() {
        let mut view = View::new(Grouping::Project, false);
        for grouping in [Grouping::Project, Grouping::Status, Grouping::Model] {
            assert_eq!(view.grouping(), grouping);
            assert!(view.center_help_lines(120).iter().any(|line| {
                let text = line.to_string();
                text.contains("g") && text.contains("Group")
            }));
            assert_eq!(
                view.handle_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE)),
                Some(Action::PersistGrouping(grouping.next()))
            );
        }
    }
}
