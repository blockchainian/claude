use super::line_edit::LineEdit;
use super::*;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) enum SendState {
    #[default]
    Ready,
    Sending,
    Failed(String),
}

#[derive(Debug, Eq, PartialEq)]
pub(super) enum PromptAction {
    Submit(String),
    Quit,
}

#[derive(Default)]
pub(super) struct PromptBox {
    pub(super) editor: LineEdit,
    pub(super) focused: bool,
    pub(super) state: SendState,
}

struct PromptLayout {
    prefix: String,
    inset: u16,
    lines: Vec<String>,
    caret: (u16, usize),
}

impl PromptBox {
    fn layout(&self, width: u16, label: &str) -> PromptLayout {
        use unicode_segmentation::UnicodeSegmentation;
        use unicode_width::UnicodeWidthStr;
        let status = match &self.state {
            SendState::Ready => String::new(),
            SendState::Sending => " · sending".into(),
            SendState::Failed(reason) => {
                format!(" · failed: {}", reason.lines().next().unwrap_or(""))
            }
        };
        let clip = |text: &str, limit: usize| {
            let mut clipped = String::new();
            let mut columns = 0;
            for grapheme in text.graphemes(true) {
                if columns + grapheme.width() > limit {
                    break;
                }
                clipped.push_str(grapheme);
                columns += grapheme.width();
            }
            clipped
        };
        let prefix_budget = usize::from(width.saturating_sub((width / 3).max(1)));
        let status = clip(&status, prefix_budget.saturating_sub(6));
        let label_budget =
            usize::from(width / 3).min(prefix_budget.saturating_sub(status.width() + 3));
        let label = clip(label.lines().next().unwrap_or(""), label_budget);
        let heading = format!("{label}{status} {PROMPT_MARKER} ");
        let prefix = clip(&heading, prefix_budget);
        let inset = prefix.width() as u16;
        let available = usize::from(width.saturating_sub(inset).max(1));
        let mut lines = vec![String::new()];
        let mut column = 0;
        let mut caret = (0, 0);
        for (index, grapheme) in self.editor.text().grapheme_indices(true) {
            let columns = grapheme.width();
            if column > 0 && column + columns.max(1) > available {
                lines.push(String::new());
                column = 0;
            }
            if index == self.editor.cursor() {
                caret = (column as u16, lines.len() - 1);
            }
            if grapheme == "\n" || grapheme == "\r\n" {
                lines.push(String::new());
                column = 0;
            } else {
                lines.last_mut().expect("one prompt row").push_str(grapheme);
                column += columns.min(available);
            }
        }
        if self.editor.cursor() == self.editor.text().len() {
            if column >= available {
                lines.push(String::new());
                column = 0;
            }
            caret = (column as u16, lines.len() - 1);
        }
        PromptLayout {
            prefix,
            inset,
            lines,
            caret,
        }
    }

    pub(super) fn height(&self, width: u16, label: &str) -> u16 {
        self.layout(width, label).lines.len().clamp(1, 5) as u16
    }

    pub(super) fn render(&self, area: Rect, buf: &mut Buffer, label: &str, disabled: Option<&str>) {
        if area.is_empty() {
            return;
        }
        if let Some(reason) = disabled {
            let reason = reason.lines().next().unwrap_or("");
            buf.set_stringn(
                area.x,
                area.y,
                format!("{label} · {reason}"),
                usize::from(area.width),
                Style::default().dim(),
            );
            return;
        }
        let layout = self.layout(area.width, label);
        let height = usize::from(area.height.min(5));
        let start = layout.caret.1.saturating_sub(height.saturating_sub(1));
        let style = if self.focused {
            Style::default().cyan()
        } else {
            Style::default().dim()
        };
        buf.set_stringn(
            area.x,
            area.y,
            &layout.prefix,
            usize::from(area.width),
            style,
        );
        for (row, text) in layout.lines.iter().skip(start).take(height).enumerate() {
            buf.set_stringn(
                area.x + layout.inset,
                area.y + row as u16,
                text,
                usize::from(area.width - layout.inset),
                Style::default(),
            );
        }
    }

    pub(super) fn cursor(&self, area: Rect, label: &str) -> Option<(u16, u16)> {
        if !self.focused || area.is_empty() {
            return None;
        }
        let layout = self.layout(area.width, label);
        let row = layout
            .caret
            .1
            .min(usize::from(area.height.min(5).saturating_sub(1)));
        Some((
            area.x + (layout.inset + layout.caret.0).min(area.width - 1),
            area.y + row as u16,
        ))
    }

    pub(super) fn paste(&mut self, text: &str) {
        if self.focused {
            self.editor.insert(text);
        }
    }

    pub(super) fn handle_key(&mut self, key: KeyEvent) -> Option<PromptAction> {
        if !self.focused || key.kind == KeyEventKind::Release {
            return None;
        }
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            if self.editor.is_empty() {
                return Some(PromptAction::Quit);
            }
            self.editor.clear();
        } else if key.code == KeyCode::Esc {
            self.focused = false;
        } else if key.code == KeyCode::Enter {
            if !self.editor.is_empty() && self.state != SendState::Sending {
                self.state = SendState::Sending;
                return Some(PromptAction::Submit(self.editor.text().to_owned()));
            }
        } else {
            self.editor.handle(key);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const REAL_TEXT: &str = include_str!("../../tests/fixtures/claude-peer-message.txt");

    fn focused() -> PromptBox {
        PromptBox {
            focused: true,
            ..PromptBox::default()
        }
    }
    fn key(editor: &mut PromptBox, code: KeyCode) -> Option<PromptAction> {
        editor.handle_key(KeyEvent::new(code, KeyModifiers::NONE))
    }
    fn rendered(editor: &PromptBox, width: u16, disabled: Option<&str>) -> String {
        let area = Rect::new(0, 0, width, editor.height(width, "Task"));
        let mut buffer = Buffer::empty(area);
        editor.render(area, &mut buffer, "Task", disabled);
        buffer
            .content
            .chunks(usize::from(width))
            .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn prompt_marker_keeps_one_cell_and_caret_after_unicode_text() {
        let mut editor = focused();
        let area = Rect::new(8, 4, 35, 5);
        let mut buffer = Buffer::empty(area);
        editor.render(area, &mut buffer, "Task", None);
        assert_eq!(buffer[(13, 4)].symbol(), "❯");
        assert_eq!(editor.cursor(area, "Task"), Some((15, 4)));
        editor.paste("你好");
        editor.render(area, &mut buffer, "Task", None);
        assert_eq!(buffer[(15, 4)].symbol(), "你");
        assert_eq!(buffer[(17, 4)].symbol(), "好");
        assert_eq!(editor.cursor(area, "Task"), Some((19, 4)));
        key(&mut editor, KeyCode::Left);
        assert_eq!(editor.cursor(area, "Task"), Some((17, 4)));
    }

    #[test]
    fn paste_preserves_real_multiline_content_only_when_focused() {
        let mut editor = PromptBox::default();
        editor.paste(REAL_TEXT);
        assert!(editor.editor.is_empty());
        editor.focused = true;
        editor.paste(REAL_TEXT);
        assert_eq!(editor.editor.text(), REAL_TEXT);
        assert_eq!(editor.height(70, "Task"), 5);
        assert!(rendered(&editor, 70, None).contains("No reply needed."));
    }

    #[test]
    fn edits_real_unicode_prompt_at_grapheme_boundaries() {
        let record: serde_json::Value = serde_json::from_str(
            include_str!("../../tests/fixtures/claude-tool-calls.jsonl")
                .lines()
                .nth(10)
                .unwrap(),
        )
        .unwrap();
        let description = record["message"]["content"][0]["input"]["description"]
            .as_str()
            .unwrap();
        assert_eq!(description, "写工具详情计划");
        let mut editor = focused();
        editor.paste(description);
        key(&mut editor, KeyCode::Left);
        key(&mut editor, KeyCode::Backspace);
        assert_eq!(editor.editor.text(), "写工具详情划");
        key(&mut editor, KeyCode::Home);
        key(&mut editor, KeyCode::Char('查'));
        assert_eq!(editor.editor.text(), "查写工具详情划");
        key(&mut editor, KeyCode::Delete);
        assert_eq!(editor.editor.text(), "查工具详情划");
    }

    #[test]
    fn submit_keeps_text_and_refuses_duplicate_sends() {
        let mut editor = focused();
        assert_eq!(key(&mut editor, KeyCode::Enter), None);
        let text = REAL_TEXT.lines().next().unwrap();
        editor.paste(text);
        assert_eq!(
            key(&mut editor, KeyCode::Enter),
            Some(PromptAction::Submit(text.into()))
        );
        assert_eq!(editor.editor.text(), text);
        assert_eq!(editor.state, SendState::Sending);
        assert_eq!(key(&mut editor, KeyCode::Enter), None);
        editor.state = SendState::Failed("permission denied".into());
        assert_eq!(
            key(&mut editor, KeyCode::Enter),
            Some(PromptAction::Submit(text.into()))
        );
    }

    #[test]
    fn control_c_clears_then_quits_and_escape_unfocuses() {
        let mut editor = focused();
        editor.paste(REAL_TEXT);
        let clear = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(editor.handle_key(clear), None);
        assert!(editor.editor.is_empty());
        assert_eq!(editor.handle_key(clear), Some(PromptAction::Quit));
        key(&mut editor, KeyCode::Esc);
        assert!(!editor.focused);
        key(&mut editor, KeyCode::Char('x'));
        assert!(editor.editor.is_empty());
    }

    #[test]
    fn wrapped_cursor_follows_home_and_end_in_real_text() {
        let mut editor = focused();
        editor.paste(REAL_TEXT);
        let area = Rect::new(8, 4, 35, 5);
        let end = editor.cursor(area, "Task").unwrap();
        assert!(area.contains(end.into()));
        assert!(rendered(&editor, 35, None).contains("No reply needed."));
        key(&mut editor, KeyCode::Home);
        assert!(rendered(&editor, 35, None).contains("FYI: VS Code"));
        assert_eq!(editor.cursor(area, "Task").unwrap().1, area.y);
        key(&mut editor, KeyCode::End);
        assert_eq!(editor.cursor(area, "Task"), Some(end));
        editor.focused = false;
        assert_eq!(editor.cursor(area, "Task"), None);
        assert_eq!(editor.cursor(Rect::default(), "Task"), None);
    }

    #[test]
    fn target_send_states_and_disabled_reason_are_visible() {
        let mut editor = focused();
        for (state, expected) in [
            (SendState::Ready, "Task"),
            (SendState::Sending, "sending"),
            (SendState::Failed("denied".into()), "denied"),
        ] {
            editor.state = state;
            let screen = rendered(&editor, 80, None);
            assert!(screen.contains("Task"));
            assert!(screen.contains(expected), "{screen}");
        }
        let screen = rendered(&editor, 100, Some("Running in a terminal"));
        assert!(screen.contains("Running in a terminal"));
        assert!(!screen.contains("denied"));
    }

    #[test]
    fn empty_editor_and_narrow_terminal_keep_valid_height_and_cursor() {
        let mut editor = focused();
        assert_eq!(editor.height(80, "Task"), 1);
        for width in [0, 1, 2, 8] {
            assert!((1..=5).contains(&editor.height(width, "Long task target")));
            let area = Rect::new(0, 0, width, 1);
            if let Some(cursor) = editor.cursor(area, "Long task target") {
                assert!(area.contains(cursor.into()));
            }
        }
        editor.paste("写工具详情计划");
        assert!((1..=5).contains(&editor.height(2, "Task")));
    }

    #[test]
    fn long_real_target_leaves_room_for_text_and_send_state() {
        let mut editor = focused();
        editor.paste("No reply needed.");
        editor.state = SendState::Sending;
        let label = REAL_TEXT.lines().next().unwrap();
        assert_eq!(editor.height(60, label), 1);
        let area = Rect::new(0, 0, 60, 1);
        let mut buffer = Buffer::empty(area);
        editor.render(area, &mut buffer, label, None);
        let text = buffer
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(text.contains("FYI:"));
        assert!(text.contains("sending"));
        assert!(text.contains("No reply needed."));
    }

    #[test]
    fn release_events_do_not_edit_or_submit() {
        let mut editor = focused();
        editor.paste(REAL_TEXT);
        let mut release = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        release.kind = KeyEventKind::Release;
        assert_eq!(editor.handle_key(release), None);
        assert_eq!(editor.state, SendState::Ready);
        release.code = KeyCode::Backspace;
        editor.handle_key(release);
        assert_eq!(editor.editor.text(), REAL_TEXT);
    }
}
