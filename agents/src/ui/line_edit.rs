use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// CSI-u can report an ASCII control code instead of its letter and Control modifier.
pub(super) fn normalize_key(mut key: KeyEvent) -> KeyEvent {
    if let KeyCode::Char(character @ '\u{1}'..='\u{1a}') = key.code {
        key.code = KeyCode::Char(char::from(character as u8 - 1 + b'a'));
        key.modifiers |= KeyModifiers::CONTROL;
    }
    key
}

#[derive(Clone, Default)]
pub(super) struct LineEdit {
    text: String,
    /// Byte offset at a grapheme boundary.
    cursor: usize,
    preferred_column: Option<usize>,
}

impl LineEdit {
    /// Caret stops as (byte offset, display column, visual row).
    pub(super) fn visual_positions(&self, width: usize) -> Vec<(usize, usize, usize)> {
        let width = width.max(1);
        let mut positions = Vec::new();
        let (mut column, mut row) = (0, 0);
        for (index, grapheme) in self.text.grapheme_indices(true) {
            let columns = grapheme.width();
            if column > 0 && column + columns.max(1) > width {
                row += 1;
                column = 0;
            }
            positions.push((index, column, row));
            if grapheme == "\n" || grapheme == "\r\n" {
                row += 1;
                column = 0;
            } else {
                column += columns.min(width);
            }
        }
        if column >= width {
            row += 1;
            column = 0;
        }
        positions.push((self.text.len(), column, row));
        positions
    }

    /// Returns false when the caret is already on the requested boundary row.
    pub(super) fn move_vertical(&mut self, width: usize, down: bool) -> bool {
        let positions = self.visual_positions(width);
        let &(_, column, row) = positions
            .iter()
            .find(|p| p.0 == self.cursor)
            .expect("caret boundary");
        let target_row = if down {
            if row == positions.last().expect("end caret").2 {
                return false;
            }
            row + 1
        } else {
            let Some(row) = row.checked_sub(1) else {
                return false;
            };
            row
        };
        let column = *self.preferred_column.get_or_insert(column);
        self.cursor = positions
            .iter()
            .rfind(|p| p.2 == target_row && p.1 <= column)
            .or_else(|| positions.iter().find(|p| p.2 == target_row))
            .expect("visual row has caret")
            .0;
        true
    }

    pub(super) fn cursor(&self) -> usize {
        self.cursor
    }

    pub(super) fn text(&self) -> &str {
        &self.text
    }
    pub(super) fn is_empty(&self) -> bool {
        self.text.is_empty()
    }
    pub(super) fn set_text(&mut self, text: &str) {
        self.preferred_column = None;
        self.text = text.into();
        self.end();
    }
    pub(super) fn clear(&mut self) {
        self.set_text("");
    }
    pub(super) fn insert(&mut self, text: impl ToString) {
        self.preferred_column = None;
        let text = text.to_string();
        self.text.insert_str(self.cursor, &text);
        self.cursor += text.len();
        self.align_cursor();
    }
    // Insertion or deletion can join the graphemes on either side of the edit.
    fn align_cursor(&mut self) {
        self.cursor = self
            .text
            .grapheme_indices(true)
            .map(|(i, _)| i)
            .find(|&i| i >= self.cursor)
            .unwrap_or(self.text.len());
    }
    pub(super) fn left(&mut self) {
        self.preferred_column = None;
        self.cursor = self.text[..self.cursor]
            .grapheme_indices(true)
            .next_back()
            .map_or(0, |(i, _)| i);
    }
    pub(super) fn right(&mut self) {
        self.preferred_column = None;
        if let Some(g) = self.text[self.cursor..].graphemes(true).next() {
            self.cursor += g.len();
        }
    }
    pub(super) fn home(&mut self) {
        self.preferred_column = None;
        self.cursor = 0;
    }
    pub(super) fn end(&mut self) {
        self.preferred_column = None;
        self.cursor = self.text.len();
    }
    pub(super) fn backspace(&mut self) {
        let end = self.cursor;
        self.left();
        self.text.drain(self.cursor..end);
        self.align_cursor();
    }
    pub(super) fn delete(&mut self) {
        let start = self.cursor;
        self.right();
        self.text.drain(start..self.cursor);
        self.cursor = start;
        self.align_cursor();
    }
    pub(super) fn word_left(&mut self) {
        self.preferred_column = None;
        let mut word = false;
        for (i, g) in self.text[..self.cursor].grapheme_indices(true).rev() {
            if g.chars().all(char::is_whitespace) {
                if word {
                    break;
                }
            } else {
                word = true;
            }
            self.cursor = i;
        }
    }
    pub(super) fn word_right(&mut self) {
        self.preferred_column = None;
        let mut word = false;
        let start = self.cursor;
        for (i, g) in self.text[start..].grapheme_indices(true) {
            if g.chars().all(char::is_whitespace) {
                if word {
                    break;
                }
            } else {
                word = true;
            }
            self.cursor = start + i + g.len();
        }
    }
    pub(super) fn kill_to_end(&mut self) {
        self.preferred_column = None;
        self.text.truncate(self.cursor);
    }
    pub(super) fn kill_to_start(&mut self) {
        self.text.drain(..self.cursor);
        self.home();
    }
    pub(super) fn kill_word_back(&mut self) {
        let end = self.cursor;
        self.word_left();
        self.text.drain(self.cursor..end);
        self.align_cursor();
    }
    pub(super) fn kill_word_forward(&mut self) {
        let start = self.cursor;
        self.word_right();
        self.text.drain(start..self.cursor);
        self.cursor = start;
        self.align_cursor();
    }
    pub(super) fn window(&self, available_cols: usize) -> (&str, u16) {
        let cols = available_cols.min(usize::from(u16::MAX));
        if cols == 0 {
            return ("", 0);
        }
        let caret_width = self.text[self.cursor..]
            .graphemes(true)
            .next()
            .map_or(1, |g| g.width().max(1));
        let mut start = self.cursor;
        let mut caret = 0;
        for (i, g) in self.text[..self.cursor].grapheme_indices(true).rev() {
            if caret + g.width() > cols.saturating_sub(caret_width) {
                break;
            }
            caret += g.width();
            start = i;
        }
        let mut end = start;
        let mut width = 0;
        for g in self.text[start..].graphemes(true) {
            if width + g.width() > cols {
                break;
            }
            width += g.width();
            end += g.len();
        }
        (&self.text[start..end], caret as u16)
    }
    pub(super) fn handle(&mut self, key: KeyEvent) -> bool {
        if key.kind == KeyEventKind::Release {
            return false;
        }
        let modifiers = key.modifiers - KeyModifiers::SHIFT;
        match (key.code, modifiers) {
            (KeyCode::Char('h'), KeyModifiers::CONTROL)
            | (KeyCode::Backspace, KeyModifiers::NONE) => self.backspace(),
            (KeyCode::Char('d'), KeyModifiers::CONTROL) | (KeyCode::Delete, KeyModifiers::NONE) => {
                self.delete()
            }
            (KeyCode::Char('b'), KeyModifiers::CONTROL) | (KeyCode::Left, KeyModifiers::NONE) => {
                self.left()
            }
            (KeyCode::Char('f'), KeyModifiers::CONTROL) | (KeyCode::Right, KeyModifiers::NONE) => {
                self.right()
            }
            (KeyCode::Char('a'), KeyModifiers::CONTROL) | (KeyCode::Home, KeyModifiers::NONE) => {
                self.home()
            }
            (KeyCode::Char('e'), KeyModifiers::CONTROL) | (KeyCode::End, KeyModifiers::NONE) => {
                self.end()
            }
            (KeyCode::Char('b'), KeyModifiers::ALT) => self.word_left(),
            (KeyCode::Char('f'), KeyModifiers::ALT) => self.word_right(),
            (KeyCode::Char('k'), KeyModifiers::CONTROL) => self.kill_to_end(),
            (KeyCode::Char('u'), KeyModifiers::CONTROL) => self.kill_to_start(),
            (KeyCode::Char('w'), KeyModifiers::CONTROL) => self.kill_word_back(),
            (KeyCode::Char('d'), KeyModifiers::ALT) => self.kill_word_forward(),
            (KeyCode::Char(c), KeyModifiers::NONE) if !c.is_control() => self.insert(c),
            _ => return false,
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(text: &str) -> LineEdit {
        let mut edit = LineEdit::default();
        edit.set_text(text);
        edit
    }

    #[test]
    fn graphemes_move_insert_and_delete() {
        let mut e = edit("你e\u{301}👨‍👩‍👧‍👦好");
        e.left();
        assert_eq!(e.cursor, "你e\u{301}👨‍👩‍👧‍👦".len());
        e.backspace();
        assert_eq!(e.text(), "你e\u{301}好");
        e.left();
        assert_eq!(e.cursor, "你".len());
        e.delete();
        assert_eq!(e.text(), "你好");
        e.insert("e");
        e.insert('\u{301}');
        assert_eq!(e.text(), "你e\u{301}好");
        e.left();
        assert_eq!(e.cursor, "你".len());
        e.right();
        assert_eq!(e.cursor, "你e\u{301}".len());
        e.home();
        e.backspace();
        assert_eq!(e.cursor, 0);
        e.end();
        e.delete();
        assert_eq!(e.cursor, e.text.len());
        e.clear();
        assert_eq!((e.text(), e.cursor), ("", 0));
    }

    #[test]
    fn insertion_and_kills_preserve_grapheme_boundaries() {
        let mut e = edit("a\u{301}b");
        e.home();
        e.insert("x\u{200d}");
        assert!(e.text.grapheme_indices(true).any(|(i, _)| i == e.cursor));
        let mut e = edit("👩👧");
        e.left();
        e.insert('\u{200d}');
        assert_eq!(e.cursor, e.text.len());
        e.backspace();
        assert_eq!(e.text(), "");
    }

    #[test]
    fn words_and_all_kills() {
        let mut e = edit("one  你好 three  ");
        e.word_left();
        assert_eq!(&e.text[e.cursor..], "three  ");
        e.word_left();
        assert_eq!(&e.text[e.cursor..], "你好 three  ");
        e.word_right();
        assert_eq!(&e.text[e.cursor..], " three  ");
        e.kill_word_forward();
        assert_eq!(e.text(), "one  你好  ");
        e.kill_word_back();
        assert_eq!(e.text(), "one    ");
        e.kill_to_end();
        assert_eq!(e.text(), "one  ");
        e.kill_to_start();
        assert_eq!(e.text(), "");
        e.word_left();
        e.word_right();
        assert_eq!(e.cursor, 0);
    }

    #[test]
    fn windows_keep_wide_caret_visible() {
        let mut e = edit("a你好b");
        assert_eq!(e.window(4), ("好b", 3));
        e.left();
        assert_eq!(e.window(4), ("好b", 2));
        e.left();
        assert_eq!(e.window(4), ("你好", 2));
        e.home();
        assert_eq!(e.window(4), ("a你", 0));
        assert_eq!(e.window(0), ("", 0));
        e.set_text("你好");
        assert_eq!(e.window(1), ("", 0));
        e.set_text("e\u{301}👨‍👩‍👧‍👦");
        assert_eq!(e.window(4), ("e\u{301}👨‍👩‍👧‍👦", 3));
    }

    #[test]
    fn key_map_applies_each_edit() {
        for (keys, modifiers, text, cursor) in [
            (vec![KeyCode::Backspace], KeyModifiers::NONE, "onetwo", 3),
            (vec![KeyCode::Char('h')], KeyModifiers::CONTROL, "onetwo", 3),
            (vec![KeyCode::Delete], KeyModifiers::NONE, "one wo", 4),
            (vec![KeyCode::Char('d')], KeyModifiers::CONTROL, "one wo", 4),
            (vec![KeyCode::Left], KeyModifiers::NONE, "one two", 3),
            (
                vec![KeyCode::Char('b')],
                KeyModifiers::CONTROL,
                "one two",
                3,
            ),
            (vec![KeyCode::Right], KeyModifiers::NONE, "one two", 5),
            (
                vec![KeyCode::Char('f')],
                KeyModifiers::CONTROL,
                "one two",
                5,
            ),
            (vec![KeyCode::Home], KeyModifiers::NONE, "one two", 0),
            (
                vec![KeyCode::Char('a')],
                KeyModifiers::CONTROL,
                "one two",
                0,
            ),
            (vec![KeyCode::End], KeyModifiers::NONE, "one two", 7),
            (
                vec![KeyCode::Char('e')],
                KeyModifiers::CONTROL,
                "one two",
                7,
            ),
            (vec![KeyCode::Char('b')], KeyModifiers::ALT, "one two", 0),
            (vec![KeyCode::Char('f')], KeyModifiers::ALT, "one two", 7),
            (vec![KeyCode::Char('k')], KeyModifiers::CONTROL, "one ", 4),
            (
                vec![KeyCode::Char('u'), KeyCode::Char('w')],
                KeyModifiers::CONTROL,
                "two",
                0,
            ),
            (vec![KeyCode::Char('d')], KeyModifiers::ALT, "one ", 4),
        ] {
            for code in keys {
                let mut e = edit("one two");
                e.home();
                e.word_right();
                e.right();
                assert!(e.handle(KeyEvent::new(code, modifiers)));
                assert_eq!(
                    (e.text(), e.cursor),
                    (text, cursor),
                    "{code:?} {modifiers:?}"
                );
            }
        }
    }

    #[test]
    fn key_map_consumes_editing_only() {
        for (code, modifiers) in [
            (KeyCode::Backspace, KeyModifiers::NONE),
            (KeyCode::Delete, KeyModifiers::NONE),
            (KeyCode::Left, KeyModifiers::NONE),
            (KeyCode::Right, KeyModifiers::NONE),
            (KeyCode::Home, KeyModifiers::NONE),
            (KeyCode::End, KeyModifiers::NONE),
        ]
        .into_iter()
        .chain(
            "hdbfaekuw"
                .chars()
                .map(|c| (KeyCode::Char(c), KeyModifiers::CONTROL)),
        )
        .chain("bfd".chars().map(|c| (KeyCode::Char(c), KeyModifiers::ALT)))
        {
            assert!(
                edit("hello world").handle(KeyEvent::new(code, modifiers)),
                "{code:?}"
            );
        }
        for code in [KeyCode::Esc, KeyCode::Enter, KeyCode::Up, KeyCode::Down] {
            assert!(!edit("").handle(KeyEvent::new(code, KeyModifiers::NONE)));
        }
        for modifiers in [
            KeyModifiers::CONTROL,
            KeyModifiers::SUPER,
            KeyModifiers::ALT | KeyModifiers::CONTROL,
        ] {
            assert!(!edit("").handle(KeyEvent::new(KeyCode::Char('c'), modifiers)));
        }
        let mut e = edit("");
        assert!(e.handle(KeyEvent::new(KeyCode::Char('你'), KeyModifiers::SHIFT)));
        assert_eq!(e.text(), "你");
        let mut release = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE);
        release.kind = KeyEventKind::Release;
        assert!(!e.handle(release));
    }
}
