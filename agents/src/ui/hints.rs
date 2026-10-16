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
        } else if self.rename_target.is_some() {
            hint_line(&[("esc", "back"), ("enter", "rename")])
        } else if self.log_search.editing || self.log_detail_search.editing {
            hint_line(&[("esc", "back"), ("enter", "keep")])
        } else if self
            .selected_row()
            .is_some_and(|session| !session.can_resume())
        {
            hint_line(&[
                ("?", "help"),
                ("esc", "back"),
                ("↑/↓", "move"),
                ("n", "new"),
            ])
        } else if self.searching {
            hint_line(&[("esc", "back"), ("enter", "open")])
        } else {
            hint_line(&[
                ("?", "help"),
                ("esc", "back"),
                ("↑/↓", "move"),
                ("enter", "open"),
                ("n", "new"),
            ])
        }
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
                    ("enter", "Open"),
                    ("pgup", "Page up"),
                    ("pgdn", "Page down"),
                    ("⌃c", "Quit"),
                    ("esc", "Back"),
                ],
            ),
            group(
                "Tasks",
                &[
                    ("n", "New"),
                    ("w", "New worktree"),
                    ("o", "Resume"),
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
                &[("tab/⇧tab", "Filter"), ("f3", "Search"), ("g", "Group")],
            ),
            group("Log", &[("⌃r", "Search")]),
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
