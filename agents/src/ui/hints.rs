use super::shortcut_help::{Group, Shortcut};
use super::*;

pub(super) fn hint_line(items: &[(&str, &str)]) -> Line<'static> {
    let mut line = Line::default();
    for (key, label) in items {
        if !line.spans.is_empty() {
            line.spans.push("  ".dim());
        }
        line.spans.push(key_span(key));
        line.spans.push(format!(" {label}").dim().not_bold());
    }
    line
}

pub(super) fn key_span(key: &str) -> Span<'static> {
    super::key_name::display(key).bold()
}

#[derive(Clone, Copy, Debug)]
pub(super) struct KeyTarget {
    pub area: Rect,
    pub key: KeyEvent,
    pub panel: Option<Panel>,
}

/// Records only the visible key cells, splitting arrow combinations at their slash.
pub(super) fn key_targets(line: &Line<'_>, area: Rect, panel: Option<Panel>) -> Vec<KeyTarget> {
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
                if let Some(key) = super::key_name::event(part) {
                    let rect =
                        Rect::new(x.saturating_add(offset), area.y, width, 1).intersection(area);
                    if !rect.is_empty() {
                        targets.push(KeyTarget {
                            area: rect,
                            key,
                            panel,
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
        if self.confirm_dialog.is_some() {
            hint_line(&[("↑/↓", "move"), ("enter", "select"), ("esc", "back")])
        } else if self.help {
            hint_line(&[("esc", "back")])
        } else if self.active_panel == Panel::TaskDetails && self.title_editor().is_some() {
            if self.name_target.is_some() {
                hint_line(&[("esc", "cancel"), ("enter", "next")])
            } else {
                hint_line(&[("esc", "back"), ("enter", "rename")])
            }
        } else if self.active_panel == Panel::Prompt && self.active_question().is_some() {
            hint_line(&[("↑/↓", "move"), ("enter", "confirm"), ("esc", "close")])
        } else if self.selected_draft().is_some()
            && (self.prompt_focused()
                || (self.active_panel == Panel::TaskDetails && self.selector.is_some()))
        {
            if self.prompt_focused() {
                hint_line(&[("esc", "cancel"), ("enter", "next")])
            } else if self.selector.as_ref().is_some_and(Selector::input_active) {
                hint_line(&[("esc", "back"), ("enter", "select")])
            } else {
                hint_line(&[("↑/↓", "move"), ("esc", "back"), ("enter", "select")])
            }
        } else if self.task_search_editing() {
            hint_line(&[("esc", "back"), ("enter", "keep")])
        } else if self
            .selected_row()
            .is_some_and(|session| !session.can_resume())
        {
            hint_line(&[
                ("?", "help"),
                ("esc", "previous"),
                ("↑/↓", "move"),
                ("n", "new"),
            ])
        } else if self.searching {
            hint_line(&[("esc", "back"), ("enter", "open")])
        } else {
            hint_line(&[
                ("?", "help"),
                ("esc", "previous"),
                ("↑/↓", "move"),
                ("enter", "open"),
                ("n", "new"),
            ])
        }
    }
    /// Whether the footer shows the task hints rather than a dialog's, an editor's or a search's.
    pub(super) fn footer_shows_task_hints(&self) -> bool {
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
    fn hint_keys_capitalize_special_names() {
        assert_eq!(
            hint_line(&[("esc", "back"), ("enter", "open"), ("n", "new")]).to_string(),
            "Esc back  Enter open  n new"
        );
    }

    #[test]
    fn footer_clicks_follow_keyboard_for_escape_enter_and_letter() {
        for code in [KeyCode::Esc, KeyCode::Enter, KeyCode::Char('n')] {
            let prepare = || {
                let mut view = View::default();
                if code == KeyCode::Esc {
                    view.help = true;
                }
                if code == KeyCode::Enter {
                    view.confirm_dialog = Some(ConfirmDialog {
                        action: ConfirmAction::Delete("claude:footer".parse().unwrap()),
                        title: "Footer test".into(),
                        confirm: false,
                    });
                }
                view
            };
            let mut mouse = prepare();
            let mut keyboard = prepare();
            let buffer = render(&mouse);
            let footer = mouse.center_layout(buffer.area).footer;
            let text = (0..buffer.area.width)
                .map(|x| buffer[(x, footer.y)].symbol())
                .collect::<String>();
            let key = match code {
                KeyCode::Esc => "esc",
                KeyCode::Enter => "enter",
                _ => "n new",
            };
            let start = text.to_lowercase().find(key).unwrap();
            let x = text[..start].width() as u16;
            assert_eq!(
                click(&mut mouse, x, footer.y),
                keyboard.handle_key(code.into())
            );
            assert_eq!(mouse.help, keyboard.help);
            assert_eq!(
                mouse.confirm_dialog.is_some(),
                keyboard.confirm_dialog.is_some()
            );
            assert_eq!(
                mouse.selected_draft().is_some(),
                keyboard.selected_draft().is_some()
            );
            assert_eq!(mouse.active_panel, keyboard.active_panel);
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
    fn task_footer_uses_open_and_lowercase_descriptions() {
        let mut view = View::default();
        assert_eq!(
            view.center_footer_hints().to_string(),
            "? help  Esc previous  ↑/↓ move  Enter open  n new"
        );
        view.searching = true;
        assert_eq!(
            view.center_footer_hints().to_string(),
            "Esc back  Enter open"
        );
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
            assert!(
                !view
                    .center_footer_hints()
                    .to_string()
                    .contains("g group by")
            );
            assert_eq!(
                view.handle_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE)),
                Some(Action::PersistGrouping(grouping.next()))
            );
        }
    }

    #[test]
    fn grouping_hint_is_hidden_when_the_grouping_key_is_consumed() {
        for mode in 0..7 {
            let mut view = View::new(Grouping::Project, false);
            let id = SessionKey {
                provider: ProviderId::Claude,
                id: "task".into(),
            };
            match mode {
                0 => view.active_panel = Panel::Prompt,
                1 => {
                    view.confirm_dialog = Some(ConfirmDialog {
                        action: ConfirmAction::Delete(id),
                        title: "Task".into(),
                        confirm: false,
                    });
                }
                2 => view.help = true,
                3 => {
                    view.rename_target = Some(id);
                    view.set_active_panel(Panel::TaskDetails);
                }
                4 => {
                    view.set_active_panel(Panel::EventList);
                    view.event_list.search.editing = true;
                }
                5 => {
                    view.set_active_panel(Panel::EventDetail);
                    view.event_detail.search.editing = true;
                }
                6 => view.searching = true,
                _ => unreachable!(),
            }
            assert!(
                !view
                    .center_footer_hints()
                    .to_string()
                    .contains("g group by"),
                "mode {mode}"
            );
            assert_eq!(
                view.handle_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE)),
                None,
                "mode {mode}"
            );
            assert_eq!(view.grouping(), Grouping::Project, "mode {mode}");
        }
    }
}
