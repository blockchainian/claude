use super::line_edit::LineEdit;
use super::*;

#[path = "prompt_images.rs"]
mod prompt_images;

#[cfg(test)]
#[path = "composer_images_tests.rs"]
mod image_tests;

#[path = "prompt_mouse.rs"]
mod prompt_mouse;

#[path = "composer_history.rs"]
mod composer_history;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) enum SendState {
    #[default]
    Ready,
    Failed(String),
}

#[derive(Debug, Eq, PartialEq)]
pub(super) enum PromptAction {
    Submit(String),
    PasteImage,
    Quit,
    Stop,
}

#[derive(Default)]
pub(super) struct PromptBox {
    pending_editor: Option<LineEdit>,
    mouse: prompt_mouse::PromptMouse,
    pub(super) editor: LineEdit,
    pub(super) focused: bool,
    pub(super) state: SendState,
    pub(super) submitting: bool,
    pub(super) history: composer_history::History,
    recall: composer_history::Recall,
    search: Option<composer_history::Search>,
    text_width: std::cell::Cell<usize>,
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
        let search_label = self
            .search
            .as_ref()
            .map(|search| format!("History: {}", search.query.text()));
        let label = search_label.as_deref().unwrap_or(label);
        let prefix_budget = usize::from(width.saturating_sub((width / 3).max(1)));
        let label_budget = if self.search.is_some() {
            prefix_budget.saturating_sub(3)
        } else {
            usize::from(width / 3).min(prefix_budget.saturating_sub(3))
        };
        let label = clip(label.lines().next().unwrap_or(""), label_budget);
        let heading = format!("{label} {PROMPT_MARKER} ");
        let prefix = clip(&heading, prefix_budget);
        let inset = prefix.width() as u16;
        let available = usize::from(width.saturating_sub(inset).max(1));
        self.text_width.set(available);
        let positions = self.editor.visual_positions(available);
        let row_count = positions.last().expect("end caret").2 + 1;
        let mut lines = vec![String::new(); row_count];
        let mut caret = (0, 0);
        for &(index, column, row) in &positions {
            if index == self.editor.cursor() {
                caret = (column as u16, row);
            }
            if let Some(grapheme) = self.editor.text()[index..].graphemes(true).next()
                && grapheme != "\n"
                && grapheme != "\r\n"
            {
                lines[row].push_str(grapheme);
            }
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
            let mut heading = format!("{label} {DISABLED_PROMPT_MARKER}");
            if !reason.is_empty() {
                heading.push(' ');
                heading.push_str(reason);
            }
            buf.set_stringn(
                area.x,
                area.y,
                heading,
                usize::from(area.width),
                Style::default().dim(),
            );
            return;
        }
        let layout = self.layout(area.width, label);
        let height = usize::from(area.height.min(5));
        let start = self.mouse_scroll_start(&layout, height);
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
        self.highlight_mouse_selection(area, buf, &layout, start);
    }

    pub(super) fn cursor(&self, area: Rect, label: &str) -> Option<(u16, u16)> {
        if !self.focused || area.is_empty() {
            return None;
        }
        let layout = self.layout(area.width, label);
        let height = usize::from(area.height.min(5));
        let start = self.mouse_scroll_start(&layout, height);
        let row = layout.caret.1.checked_sub(start)?;
        if row >= height {
            return None;
        }
        Some((
            area.x + (layout.inset + layout.caret.0).min(area.width - 1),
            area.y + row as u16,
        ))
    }

    pub(super) fn searching_history(&self) -> bool {
        self.search.is_some()
    }

    pub(super) fn paste(&mut self, text: &str) {
        if self.focused && self.search.is_some() {
            self.search_paste(text);
        } else if self.focused {
            self.editor
                .insert(text.replace("\r\n", "\n").replace('\r', "\n"));
        }
    }

    pub(super) fn handle_key(&mut self, key: KeyEvent) -> Option<PromptAction> {
        let key = super::line_edit::normalize_key(key);
        if !self.focused || key.kind == KeyEventKind::Release {
            return None;
        }
        if key.code == KeyCode::Char('v')
            && key.modifiers == KeyModifiers::CONTROL
            && self.search.is_none()
        {
            return Some(PromptAction::PasteImage);
        }
        if self.editor.selection_key(key) {
            return None;
        }
        if self.history_key(key) {
            return None;
        }
        let vertical = match (key.code, key.modifiers) {
            (KeyCode::Up, KeyModifiers::NONE) | (KeyCode::Char('p'), KeyModifiers::CONTROL) => {
                Some(false)
            }
            (KeyCode::Down, KeyModifiers::NONE) | (KeyCode::Char('n'), KeyModifiers::CONTROL) => {
                Some(true)
            }
            _ => None,
        };
        if let Some(down) = vertical {
            if !self.editor.move_vertical(self.text_width.get(), down) {
                self.recall_history(down);
            }
            return None;
        }
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            if self.editor.is_empty() {
                return Some(PromptAction::Quit);
            }
            self.editor.clear();
        } else if key.code == KeyCode::Esc {
            if self.editor.is_empty() {
                return Some(PromptAction::Stop);
            }
            self.editor.clear();
        } else if (key.code == KeyCode::Enter
            && key
                .modifiers
                .intersects(KeyModifiers::SHIFT | KeyModifiers::ALT))
            || (key.code == KeyCode::Char('j') && key.modifiers.contains(KeyModifiers::CONTROL))
        {
            self.editor.insert('\n');
        } else if key.code == KeyCode::Enter {
            if !self.editor.is_empty()
                && (!self.submitting || matches!(self.state, SendState::Failed(_)))
            {
                self.record_submission();
                self.submitting = true;
                self.state = SendState::Ready;
                return Some(PromptAction::Submit(self.editor.prompt_text()));
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
        let before = rendered(&editor, 120, None);
        assert_eq!(
            key(&mut editor, KeyCode::Enter),
            Some(PromptAction::Submit(text.into()))
        );
        assert_eq!(rendered(&editor, 120, None), before);
        assert_eq!(editor.editor.text(), text);
        assert_eq!(editor.state, SendState::Ready);
        assert_eq!(key(&mut editor, KeyCode::Enter), None);
        assert_eq!(editor.history.borrow().len(), 1);
        editor.state = SendState::Failed("permission denied".into());
        assert_eq!(
            key(&mut editor, KeyCode::Enter),
            Some(PromptAction::Submit(text.into()))
        );
    }

    #[test]
    fn control_c_clears_then_quits() {
        let mut editor = focused();
        editor.paste(REAL_TEXT);
        let clear = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(editor.handle_key(clear), None);
        assert!(editor.editor.is_empty());
        assert_eq!(editor.handle_key(clear), Some(PromptAction::Quit));
    }

    #[test]
    fn escape_clears_then_stops_and_keeps_focus() {
        let mut editor = focused();
        editor.paste(REAL_TEXT);
        let escape = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(editor.handle_key(escape), None);
        assert!(editor.editor.is_empty());
        assert_eq!(editor.handle_key(escape), Some(PromptAction::Stop));
        assert!(editor.focused);
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
    fn send_feedback_does_not_change_prompt_and_disabled_reason_is_visible() {
        let mut editor = focused();
        for (state, expected) in [
            (SendState::Ready, "Task"),
            (SendState::Failed("denied".into()), "Task ❯"),
        ] {
            editor.state = state;
            let screen = rendered(&editor, 80, None);
            assert!(screen.contains("Task"));
            assert!(screen.contains(expected), "{screen}");
            assert!(
                !screen.contains("sending") && !screen.contains("failed"),
                "{screen}"
            );
        }
        let screen = rendered(&editor, 100, Some("Running in a terminal"));
        assert!(screen.contains("Task ❮ Running in a terminal"), "{screen}");
        assert!(!screen.contains("denied"));
        let area = Rect::new(0, 0, 100, 1);
        for (reason, expected) in [
            (
                "Running in a terminal",
                "升级命令中心 ❮ Running in a terminal",
            ),
            ("", "升级命令中心 ❮"),
        ] {
            let mut buffer = Buffer::empty(area);
            editor.render(area, &mut buffer, "升级命令中心", Some(reason));
            use unicode_segmentation::UnicodeSegmentation;
            let mut column = 0;
            for grapheme in expected.graphemes(true) {
                let cell = &buffer[(column as u16, 0)];
                assert_eq!(cell.symbol(), grapheme);
                assert!(cell.modifier.contains(ratatui::style::Modifier::DIM));
                column += grapheme.width();
            }
            assert_eq!(column, expected.width());
            assert_eq!(
                buffer[(column as u16, 0)].modifier,
                ratatui::style::Modifier::empty()
            );
        }
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
    fn long_real_target_leaves_room_for_text() {
        let mut editor = focused();
        editor.paste("No reply needed.");
        editor.submitting = true;
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
        assert!(!text.contains("sending"));
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

#[cfg(test)]
#[path = "composer_keys_tests.rs"]
mod composer_keys_tests;
