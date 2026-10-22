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
    pub(super) state: SendState,
    pub(super) submitting: bool,
    pub(super) history: composer_history::History,
    recall: composer_history::Recall,
    search: Option<composer_history::Search>,
    /// The box width and label inset the editor last wrapped at.
    text_width: std::cell::Cell<(usize, usize)>,
}

/// The prompt's rows: the label prefix, then the text wrapped so that only the first row
/// starts after the prefix and every later row starts at the box's left edge.
struct PromptLayout {
    prefix: String,
    /// The byte length of the task name that starts the prefix; zero while the label shows
    /// something else.
    task_label_len: usize,
    inset: u16,
    lines: Vec<String>,
    /// The caret's column within the box and its row.
    caret: (u16, usize),
}

fn prompt_heading(label: &str, marker: &str) -> String {
    if label.is_empty() {
        marker.to_owned()
    } else {
        format!("{label} {marker}")
    }
}

/// Draws the task name in its task list color, then the marker and the rest in the box's style.
fn render_heading(
    area: Rect,
    buf: &mut Buffer,
    (name, rest): (&str, &str),
    name_style: Style,
    rest_style: Style,
) {
    let (x, _) = buf.set_stringn(area.x, area.y, name, usize::from(area.width), name_style);
    buf.set_stringn(x, area.y, rest, usize::from(area.right() - x), rest_style);
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
        // A failed send reverses the marker and keeps its reason after it until the next send.
        let failed_reason = match &self.state {
            SendState::Failed(reason) if self.search.is_none() => {
                Some(reason.lines().next().unwrap_or(""))
            }
            _ => None,
        };
        let label = search_label.as_deref().unwrap_or(label);
        let prefix_budget = usize::from(width.saturating_sub((width / 3).max(1)));
        let label_budget = if self.search.is_some() {
            prefix_budget.saturating_sub(3)
        } else {
            usize::from(width / 3).min(prefix_budget.saturating_sub(3))
        };
        let shows_task = self.search.is_none() && failed_reason.is_none();
        let label = clip(label.lines().next().unwrap_or(""), label_budget);
        let heading = match failed_reason {
            Some(reason) => format!(
                "{} {reason} ",
                prompt_heading(&label, DISABLED_PROMPT_MARKER)
            ),
            None => format!("{} ", prompt_heading(&label, PROMPT_MARKER)),
        };
        let prefix = clip(&heading, prefix_budget);
        let task_label_len = if shows_task {
            label.len().min(prefix.len())
        } else {
            0
        };
        let inset = prefix.width() as u16;
        self.text_width
            .set((usize::from(width), usize::from(inset)));
        let positions = self
            .editor
            .visual_positions(usize::from(width), usize::from(inset));
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
            task_label_len,
            inset,
            lines,
            caret,
        }
    }

    /// The rows the whole prompt takes; the layout bounds it.
    pub(super) fn height(&self, width: u16, label: &str) -> u16 {
        self.layout(width, label)
            .lines
            .len()
            .clamp(1, usize::from(u16::MAX)) as u16
    }

    pub(super) fn render(
        &self,
        area: Rect,
        buf: &mut Buffer,
        label: &str,
        label_style: Style,
        disabled: Option<&str>,
        focused: bool,
    ) {
        if area.is_empty() {
            return;
        }
        if let Some(reason) = disabled {
            let reason = reason.lines().next().unwrap_or("");
            let mut heading = prompt_heading(label, DISABLED_PROMPT_MARKER);
            if !reason.is_empty() {
                heading.push(' ');
                heading.push_str(reason);
            }
            render_heading(
                area,
                buf,
                heading.split_at(label.len()),
                label_style,
                Style::default().dim(),
            );
            return;
        }
        let layout = self.layout(area.width, label);
        let height = usize::from(area.height);
        let start = self.mouse_scroll_start(&layout, height);
        let style = if matches!(self.state, SendState::Failed(_)) && self.search.is_none() {
            style::error_style()
        } else if focused {
            Style::default().cyan()
        } else {
            Style::default().dim()
        };
        if start == 0 {
            render_heading(
                area,
                buf,
                layout.prefix.split_at(layout.task_label_len),
                label_style,
                style,
            );
        }
        for (row, text) in layout.lines.iter().skip(start).take(height).enumerate() {
            let inset = if start + row == 0 { layout.inset } else { 0 };
            buf.set_stringn(
                area.x + inset,
                area.y + row as u16,
                text,
                usize::from(area.width - inset),
                Style::default(),
            );
        }
        self.highlight_mouse_selection(area, buf, start);
    }

    pub(super) fn cursor(&self, area: Rect, label: &str) -> Option<(u16, u16)> {
        if area.is_empty() {
            return None;
        }
        let layout = self.layout(area.width, label);
        let height = usize::from(area.height);
        let start = self.mouse_scroll_start(&layout, height);
        let row = layout.caret.1.checked_sub(start)?;
        if row >= height {
            return None;
        }
        Some((
            area.x + layout.caret.0.min(area.width - 1),
            area.y + row as u16,
        ))
    }

    pub(super) fn searching_history(&self) -> bool {
        self.search.is_some()
    }

    pub(super) fn paste(&mut self, text: &str) {
        if self.search.is_some() {
            self.search_paste(text);
        } else {
            self.editor
                .insert(text.replace("\r\n", "\n").replace('\r', "\n"));
        }
    }

    pub(super) fn handle_key(&mut self, key: KeyEvent) -> Option<PromptAction> {
        let key = super::line_edit::normalize_key(key);
        if key.kind == KeyEventKind::Release {
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
            let (width, inset) = self.text_width.get();
            if !self.editor.move_vertical(width, inset, down) {
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
        PromptBox::default()
    }
    fn key(editor: &mut PromptBox, code: KeyCode) -> Option<PromptAction> {
        editor.handle_key(KeyEvent::new(code, KeyModifiers::NONE))
    }
    fn rendered(editor: &PromptBox, width: u16, disabled: Option<&str>) -> String {
        let area = Rect::new(0, 0, width, editor.height(width, "Task"));
        let mut buffer = Buffer::empty(area);
        editor.render(area, &mut buffer, "Task", Style::default(), disabled, true);
        buffer
            .content
            .chunks(usize::from(width))
            .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn empty_label_heading_starts_at_first_cell_and_moves_caret_and_clicks() {
        let mut editor = focused();
        let area = Rect::new(8, 4, 35, 1);
        let mut buffer = Buffer::empty(area);
        editor.render(area, &mut buffer, "", Style::default(), None, true);
        assert_eq!(buffer[(8, 4)].symbol(), "❯");
        assert_eq!(editor.cursor(area, ""), Some((10, 4)));
        editor.paste("abc");
        editor.render(area, &mut buffer, "", Style::default(), None, true);
        assert_eq!(buffer[(10, 4)].symbol(), "a");
        assert_eq!(editor.cursor(area, ""), Some((13, 4)));
        editor.select_mouse(
            area,
            "",
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 11,
                row: 4,
                modifiers: KeyModifiers::NONE,
            },
        );
        assert_eq!(editor.editor.cursor(), 1);
        assert_eq!(editor.cursor(area, ""), Some((11, 4)));
    }

    #[test]
    fn disabled_empty_label_heading_starts_at_first_cell() {
        let editor = focused();
        let area = Rect::new(8, 4, 35, 1);
        for (reason, expected) in [("", "❮"), ("/bg", "❮ /bg")] {
            let mut buffer = Buffer::empty(area);
            editor.render(area, &mut buffer, "", Style::default(), Some(reason), true);
            let screen: String = (area.x..area.right())
                .map(|column| buffer[(column, area.y)].symbol())
                .collect();
            assert_eq!(screen.trim_end(), expected);
        }
    }

    #[test]
    fn nonempty_label_heading_keeps_spacing() {
        let editor = focused();
        assert_eq!(rendered(&editor, 35, None).trim_end(), "Task ❯");
        assert_eq!(editor.layout(35, "Task").prefix, "Task ❯ ");
        assert_eq!(editor.cursor(Rect::new(8, 4, 35, 1), "Task"), Some((15, 4)));
        assert_eq!(rendered(&editor, 35, Some("/bg")).trim_end(), "Task ❮ /bg");
    }

    #[test]
    fn prompt_marker_keeps_one_cell_and_caret_after_unicode_text() {
        let mut editor = focused();
        let area = Rect::new(8, 4, 35, 5);
        let mut buffer = Buffer::empty(area);
        editor.render(area, &mut buffer, "Task", Style::default(), None, true);
        assert_eq!(buffer[(13, 4)].symbol(), "❯");
        assert_eq!(editor.cursor(area, "Task"), Some((15, 4)));
        editor.paste("你好");
        editor.render(area, &mut buffer, "Task", Style::default(), None, true);
        assert_eq!(buffer[(15, 4)].symbol(), "你");
        assert_eq!(buffer[(17, 4)].symbol(), "好");
        assert_eq!(editor.cursor(area, "Task"), Some((19, 4)));
        key(&mut editor, KeyCode::Left);
        assert_eq!(editor.cursor(area, "Task"), Some((17, 4)));
    }

    #[test]
    fn paste_preserves_real_multiline_content() {
        let mut editor = PromptBox::default();

        editor.paste(REAL_TEXT);
        assert_eq!(editor.editor.text(), REAL_TEXT);
        let height = editor.height(70, "Task");
        assert!(height > 5, "{height}");
        assert_eq!(height, editor.layout(70, "Task").lines.len() as u16);
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

        assert_eq!(editor.cursor(Rect::default(), "Task"), None);
    }

    #[test]
    fn failed_send_reverses_the_marker_and_shows_its_reason_after_it() {
        let mut editor = focused();
        editor.editor.set_text("retry me");
        editor.state = SendState::Failed("Claude attach timed out\nscreen".into());
        let area = Rect::new(0, 0, 80, 1);
        let mut buffer = Buffer::empty(area);
        editor.render(area, &mut buffer, "Task", Style::default(), None, true);
        let heading = "Task ❮ Claude attach timed out retry me";
        let screen: String = (0..heading.chars().count() as u16)
            .map(|column| buffer[(column, 0)].symbol().to_owned())
            .collect();
        assert_eq!(screen, heading);
        let reason_end = heading.find(" retry").unwrap();
        for column in 0..heading[..reason_end].chars().count() as u16 {
            assert_eq!(buffer[(column, 0)].fg, Color::Red, "column {column}");
        }
        assert_ne!(
            buffer[(heading.chars().count() as u16 - 1, 0)].fg,
            Color::Red
        );
        let caret = editor.cursor(area, "Task").unwrap();
        assert_eq!(caret, (heading.chars().count() as u16, 0));
        editor.state = SendState::Ready;
        assert!(!rendered(&editor, 80, None).contains("timed out"));
    }

    #[test]
    fn disabled_reason_is_visible() {
        let mut editor = focused();
        editor.state = SendState::Failed("denied".into());
        let screen = rendered(&editor, 100, Some("/bg"));
        assert!(screen.contains("Task ❮ /bg"), "{screen}");
        assert!(!screen.contains("denied"));
        let area = Rect::new(0, 0, 100, 1);
        for (reason, expected) in [("/bg", "升级命令中心 ❮ /bg"), ("", "升级命令中心 ❮")]
        {
            let mut buffer = Buffer::empty(area);
            editor.render(
                area,
                &mut buffer,
                "升级命令中心",
                Style::default(),
                Some(reason),
                true,
            );
            use unicode_segmentation::UnicodeSegmentation;
            let mut column = 0;
            for grapheme in expected.graphemes(true) {
                let cell = &buffer[(column as u16, 0)];
                assert_eq!(cell.symbol(), grapheme);
                // The task name keeps its own style; the marker and reason are dimmed.
                assert_eq!(
                    cell.modifier.contains(ratatui::style::Modifier::DIM),
                    column >= "升级命令中心".width()
                );
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
            assert_eq!(editor.height(width, "Long task target"), 1);
            let area = Rect::new(0, 0, width, 1);
            if let Some(cursor) = editor.cursor(area, "Long task target") {
                assert!(area.contains(cursor.into()));
            }
        }
        editor.paste("写工具详情计划");
        assert!(editor.height(2, "Task") >= 1);
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
        editor.render(area, &mut buffer, label, Style::default(), None, true);
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

    const CJK_PARAGRAPHS: &str = "同样，它是用已有的这个任务的名字做那个 placeholder。然后当用户开始打字的时候，这个 placeholder 就消失了，用户输入的东西会替换掉原来的名字。这个和现在的搜索框的行为是一样的，所以可以复用同一个组件，不需要再写一遍。\n\n然后它也支持 line editing，就是重命名。重命名的时候也是在同一个输入框里面做，按回车确认，按 Esc 取消。这个和搜索框的行为也是一样的，所以也可以复用。\n\n最后还有一个问题，就是这个输入框的高度。现在是固定五行，但是长的提示会被截掉。";

    fn screen_rows(editor: &PromptBox, width: u16, height: u16) -> Vec<String> {
        use unicode_width::UnicodeWidthStr;
        let area = Rect::new(0, 0, width, height);
        let mut buffer = Buffer::empty(area);
        editor.render(
            area,
            &mut buffer,
            "New prompt",
            Style::default(),
            None,
            true,
        );
        buffer
            .content
            .chunks(usize::from(width))
            .map(|row| {
                let mut text = String::new();
                let mut skip = 0;
                for cell in row {
                    if skip > 0 {
                        skip -= 1;
                        continue;
                    }
                    text.push_str(cell.symbol());
                    skip = cell.symbol().width().saturating_sub(1);
                }
                text.trim_end().to_owned()
            })
            .collect()
    }

    #[test]
    fn newline_after_a_full_row_adds_no_blank_row() {
        let mut editor = focused();
        editor.paste("AAAA\nB");
        // "Task ❯ " takes seven cells, so eleven columns leave four for the first row.
        let layout = editor.layout(11, "Task");
        assert_eq!(layout.lines, ["AAAA", "B"]);
        assert_eq!(layout.caret, (1, 1));
    }

    #[test]
    fn continuation_rows_flow_from_the_left_edge() {
        let mut editor = focused();
        editor.paste(CJK_PARAGRAPHS);
        let rows = screen_rows(&editor, 138, editor.height(138, "New prompt"));
        assert!(rows[0].starts_with("New prompt ❯ 同样"), "{}", rows[0]);
        let flowed = format!("{}{}", &rows[0]["New prompt ❯ ".len()..], rows[1]);
        assert!(CJK_PARAGRAPHS.starts_with(&flowed), "{flowed}");
        assert!(rows[rows.len() - 1].starts_with("最后"), "{:?}", rows);
        let area = Rect::new(0, 0, 138, rows.len() as u16);
        let caret = editor.cursor(area, "New prompt").unwrap();
        assert_eq!(caret.1, area.bottom() - 1);
        key(&mut editor, KeyCode::Home);
        assert_eq!(editor.cursor(area, "New prompt"), Some((13, 0)));
        key(&mut editor, KeyCode::Down);
        assert_eq!(editor.cursor(area, "New prompt"), Some((0, 1)));
    }

    #[test]
    fn box_grows_to_the_whole_prompt_and_shows_its_tail_when_clipped() {
        let mut editor = focused();
        editor.paste(CJK_PARAGRAPHS);
        let height = editor.height(138, "New prompt");
        assert_eq!(height, editor.layout(138, "New prompt").lines.len() as u16);
        assert!(height > 5, "{height}");
        let rows = screen_rows(&editor, 138, height);
        assert!(
            rows.iter().filter(|row| row.is_empty()).count() == 2,
            "{rows:?}"
        );
        let clipped = screen_rows(&editor, 138, 3);
        assert_eq!(clipped, rows[rows.len() - 3..]);
        let area = Rect::new(0, 0, 138, 3);
        assert_eq!(editor.cursor(area, "New prompt").map(|c| c.1), Some(2));
        for _ in 0..4 {
            key(&mut editor, KeyCode::Up);
        }
        let caret = editor.cursor(area, "New prompt").unwrap();
        assert_eq!(caret.1, 0);
        assert_eq!(
            screen_rows(&editor, 138, 3),
            rows[rows.len() - 5..rows.len() - 2]
        );
    }
}

#[cfg(test)]
#[path = "composer_keys_tests.rs"]
mod composer_keys_tests;
