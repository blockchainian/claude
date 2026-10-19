use super::shortcut_help::{Group, Shortcut};
use super::*;

pub(super) fn hint_line(items: &[(&str, &str)]) -> Line<'static> {
    let mut line = Line::default();
    for (key, label) in items {
        if !line.spans.is_empty() {
            line.spans.push("  ".dim());
        }
        line.spans.push(key.to_string().bold());
        line.spans.push(format!(" {label}").dim().not_bold());
    }
    line
}
impl View {
    pub(super) fn center_footer_hints(&self) -> Line<'static> {
        if self.confirm_dialog.is_some() {
            hint_line(&[("↑/↓", "move"), ("enter", "select"), ("esc", "back")])
        } else if self.help {
            hint_line(&[("esc", "back")])
        } else if self.title_editor().is_some() {
            if self.name_target.is_some() {
                hint_line(&[("esc", "cancel"), ("enter", "next")])
            } else {
                hint_line(&[("esc", "back"), ("enter", "rename")])
            }
        } else if self.active_question().is_some() {
            hint_line(&[("↑/↓", "move"), ("enter", "confirm"), ("esc", "close")])
        } else if self.selected_draft().is_some()
            && (self.input_box.focused || self.selector.is_some())
        {
            if self.input_box.focused {
                hint_line(&[("esc", "cancel"), ("enter", "next")])
            } else if self.selector.as_ref().is_some_and(Selector::input_active) {
                hint_line(&[("esc", "back"), ("enter", "select")])
            } else {
                hint_line(&[("↑/↓", "move"), ("esc", "back"), ("enter", "select")])
            }
        } else if self.log_search.editing || self.log_detail_search.editing {
            hint_line(&[("esc", "back"), ("enter", "keep")])
        } else if self
            .selected_row()
            .is_some_and(|session| !session.can_resume())
        {
            hint_line(&[
                ("?", "help"),
                ("esc", "Previous"),
                ("↑/↓", "move"),
                ("n", "new"),
            ])
        } else if self.searching {
            hint_line(&[("esc", "back"), ("enter", "Terminal")])
        } else {
            hint_line(&[
                ("?", "help"),
                ("esc", "Previous"),
                ("↑/↓", "move"),
                ("enter", "Terminal"),
                ("n", "new"),
            ])
        }
    }
    /// Whether the footer shows the task hints rather than a dialog's, an editor's or a search's.
    pub(super) fn footer_shows_task_hints(&self) -> bool {
        self.confirm_dialog.is_none()
            && !self.help
            && self.title_editor().is_none()
            && self.active_question().is_none()
            && !(self.selected_draft().is_some()
                && (self.input_box.focused || self.selector.is_some()))
            && !self.log_search.editing
            && !self.log_detail_search.editing
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
                    ("enter", "Terminal"),
                    ("→/⌃l", "Terminal"),
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
                    ("o", "Terminal"),
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
                "Log",
                &[
                    ("/", "Search"),
                    ("t", "Sort"),
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

    #[test]
    fn log_help_includes_duration_sort_key() {
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
                0 => view.input_box.focused = true,
                1 => {
                    view.confirm_dialog = Some(ConfirmDialog {
                        action: ConfirmAction::Delete(id),
                        title: "Task".into(),
                        confirm: false,
                    });
                }
                2 => view.help = true,
                3 => view.rename_target = Some(id),
                4 => view.log_search.editing = true,
                5 => view.log_detail_search.editing = true,
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
