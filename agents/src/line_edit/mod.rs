use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use unicode_segmentation::UnicodeSegmentation;

mod kill_buffer;
mod selection;

pub(crate) use kill_buffer::{Direction, KillBuffer};
mod layout;
pub(crate) use layout::{CaretStop, Vertical, Window, Wrap};

/// CSI-u can report an ASCII control code instead of its letter and Control modifier.
pub(crate) fn normalize_key(mut key: KeyEvent) -> KeyEvent {
    if let KeyCode::Char(character @ '\u{1}'..='\u{1a}') = key.code {
        key.code = KeyCode::Char(char::from(character as u8 - 1 + b'a'));
        key.modifiers |= KeyModifiers::CONTROL;
    }
    key
}

mod placeholders;

pub(crate) trait Placeholder: Clone {
    /// Label of the `ordinal`-th placeholder in text order, counting from 1.
    fn label(ordinal: usize) -> String;
}

#[derive(Clone)]
pub(crate) enum NoPlaceholder {}

impl Placeholder for NoPlaceholder {
    fn label(_: usize) -> String {
        unreachable!()
    }
}

#[derive(Clone)]
pub(crate) struct LineEdit<P: Placeholder = NoPlaceholder> {
    placed: Vec<placeholders::Placed<P>>,
    selection: Option<(usize, usize)>,
    text: String,
    /// Byte offset at a grapheme boundary.
    cursor: usize,
    preferred_column: Option<usize>,
}

impl<P: Placeholder> Default for LineEdit<P> {
    fn default() -> Self {
        Self {
            placed: Vec::new(),
            selection: None,
            text: String::new(),
            cursor: 0,
            preferred_column: None,
        }
    }
}

impl<P: Placeholder> LineEdit<P> {
    pub(crate) fn cursor(&self) -> usize {
        self.cursor
    }

    pub(crate) fn text(&self) -> &str {
        &self.text
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.text.is_empty()
    }
    pub(crate) fn set_text(&mut self, text: &str) {
        self.preferred_column = None;
        self.placed.clear();
        self.text = text.into();
        self.end();
        self.clear_selection();
    }
    pub(crate) fn clear(&mut self) {
        self.set_text("");
    }
    pub(crate) fn insert(&mut self, text: impl ToString) {
        if self.selection_range().is_some() {
            self.replace_selection(text);
            return;
        }
        self.preferred_column = None;
        let text = text.to_string();
        let range = self.replace_placeholders(self.cursor..self.cursor, &text);
        self.cursor = range.start + text.len();
        self.align_cursor();
    }
    // Insertion or deletion can join the graphemes on either side of the edit.
    fn align_cursor(&mut self) {
        self.relabel_placeholders();
        self.cursor = self.caret_boundary(self.cursor, true);
    }
    pub(crate) fn left(&mut self) {
        self.preferred_column = None;
        self.cursor = self.text[..self.cursor]
            .grapheme_indices(true)
            .next_back()
            .map_or(0, |(i, _)| i);
        self.cursor = self.caret_boundary(self.cursor, false);
    }
    pub(crate) fn right(&mut self) {
        self.preferred_column = None;
        if let Some(g) = self.text[self.cursor..].graphemes(true).next() {
            self.cursor += g.len();
        }
        self.cursor = self.caret_boundary(self.cursor, true);
    }
    pub(crate) fn home(&mut self) {
        self.preferred_column = None;
        self.cursor = 0;
    }
    pub(crate) fn end(&mut self) {
        self.preferred_column = None;
        self.cursor = self.text.len();
    }
    /// Moves to the start of the caret's line, or of the previous line when already there.
    pub(crate) fn line_start(&mut self) {
        self.preferred_column = None;
        let line_start = |end: usize| self.text[..end].rfind('\n').map_or(0, |i| i + 1);
        let mut start = line_start(self.cursor);
        if start == self.cursor && start > 0 {
            start = line_start(start - 1);
        }
        self.cursor = self.caret_boundary(start, false);
    }
    /// Moves to the end of the caret's line, or of the next line when already there.
    pub(crate) fn line_end(&mut self) {
        self.preferred_column = None;
        let line_end = |from: usize| match self.text[from..].find('\n') {
            Some(i) => from + i - usize::from(self.text[..from + i].ends_with('\r')),
            None => self.text.len(),
        };
        let mut end = line_end(self.cursor);
        if end == self.cursor && end < self.text.len() {
            end = line_end(
                self.cursor + self.text[self.cursor..].find('\n').expect("line break") + 1,
            );
        }
        self.cursor = self.caret_boundary(end, true);
    }
    pub(crate) fn backspace(&mut self) {
        let end = self.cursor;
        self.left();
        self.cursor = self.replace_placeholders(self.cursor..end, "").start;
        self.align_cursor();
    }
    pub(crate) fn delete(&mut self) {
        let start = self.cursor;
        self.right();
        self.cursor = self.replace_placeholders(start..self.cursor, "").start;
        self.align_cursor();
    }
    pub(crate) fn word_left(&mut self) {
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
        self.cursor = self.caret_boundary(self.cursor, false);
    }
    pub(crate) fn word_right(&mut self) {
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
        self.cursor = self.caret_boundary(self.cursor, true);
    }
    /// Deletes what Ctrl+E would move over.
    pub(crate) fn kill_to_end(&mut self) {
        if !self.remove_empty_line(false) {
            let start = self.cursor;
            self.line_end();
            self.cursor = self.replace_placeholders(start..self.cursor, "").start;
            self.remove_empty_line(false);
        }
        self.align_cursor();
    }
    /// Deletes what Ctrl+A would move over.
    pub(crate) fn kill_to_start(&mut self) {
        if !self.remove_empty_line(true) {
            let end = self.cursor;
            self.line_start();
            self.cursor = self.replace_placeholders(self.cursor..end, "").start;
            self.remove_empty_line(true);
        }
        self.align_cursor();
    }
    /// Removes the caret's line when it is empty, by deleting its line break before the
    /// caret when `before` is set and there is one, or else the one after. Returns whether
    /// a line was removed.
    fn remove_empty_line(&mut self, before: bool) -> bool {
        let (head, tail) = self.text.split_at(self.cursor);
        let break_len = |crlf: bool, lf: bool| {
            if crlf { Some(2) } else { lf.then_some(1) }
        };
        let previous = break_len(head.ends_with("\r\n"), head.ends_with('\n'));
        let next = break_len(tail.starts_with("\r\n"), tail.starts_with('\n'));
        let empty = (head.is_empty() || previous.is_some()) && (tail.is_empty() || next.is_some());
        if empty {
            let range = match (previous, next) {
                (Some(len), _) if before || next.is_none() => self.cursor - len..self.cursor,
                (_, Some(len)) => self.cursor..self.cursor + len,
                _ => return false,
            };
            self.cursor = self.replace_placeholders(range, "").start;
            return true;
        }
        false
    }
    pub(crate) fn kill_word_back(&mut self) {
        let end = self.cursor;
        self.word_left();
        self.cursor = self.replace_placeholders(self.cursor..end, "").start;
        self.align_cursor();
    }
    pub(crate) fn kill_word_forward(&mut self) {
        let start = self.cursor;
        self.word_right();
        self.cursor = self.replace_placeholders(start..self.cursor, "").start;
        self.align_cursor();
    }
    /// Saves the removed text, joined in text order with earlier kills of this key.
    fn kill(&mut self, delete: fn(&mut Self), kills: &mut KillBuffer) {
        let (before, cursor) = (self.text.clone(), self.cursor);
        delete(self);
        // Every kill leaves the caret where its removed text started.
        let removed = &before[self.cursor..self.cursor + before.len() - self.text.len()];
        let direction = if self.cursor < cursor {
            Direction::Backward
        } else {
            Direction::Forward
        };
        kills.push(removed, direction);
    }
    pub(crate) fn handle(&mut self, key: KeyEvent, kills: &mut KillBuffer) -> bool {
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
            (KeyCode::Char('a'), KeyModifiers::CONTROL) => self.line_start(),
            (KeyCode::Char('e'), KeyModifiers::CONTROL) => self.line_end(),
            (KeyCode::Home, KeyModifiers::NONE) => self.home(),
            (KeyCode::End, KeyModifiers::NONE) => self.end(),
            (KeyCode::Char('b'), KeyModifiers::ALT) => self.word_left(),
            (KeyCode::Char('f'), KeyModifiers::ALT) => self.word_right(),
            (KeyCode::Char('k'), KeyModifiers::CONTROL) => self.kill(Self::kill_to_end, kills),
            (KeyCode::Char('u'), KeyModifiers::CONTROL) => self.kill(Self::kill_to_start, kills),
            (KeyCode::Char('w'), KeyModifiers::CONTROL) => self.kill(Self::kill_word_back, kills),
            (KeyCode::Char('d'), KeyModifiers::ALT) => self.kill(Self::kill_word_forward, kills),
            (KeyCode::Char('y'), KeyModifiers::CONTROL) => {
                self.insert(kills.text());
            }
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
    fn key_map_applies_each_edit() {
        let mut kills = KillBuffer::default();
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
                assert!({
                    kills.begin_stroke();
                    e.handle(KeyEvent::new(code, modifiers), &mut kills)
                });
                assert_eq!(
                    (e.text(), e.cursor),
                    (text, cursor),
                    "{code:?} {modifiers:?}"
                );
            }
        }
    }

    #[test]
    fn line_keys_step_through_lines() {
        let mut kills = KillBuffer::default();
        let mut control = |e: &mut LineEdit, c| {
            kills.begin_stroke();
            e.handle(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL), &mut kills)
        };
        let mut e = edit("one\r\ntwo\nthree");
        e.set_cursor("one\r\ntw".len());
        for cursor in ["one\r\n".len(), 0, 0] {
            control(&mut e, 'a');
            assert_eq!(e.cursor, cursor);
        }
        for cursor in ["one".len(), "one\r\ntwo".len(), e.text.len(), e.text.len()] {
            control(&mut e, 'e');
            assert_eq!(e.cursor, cursor);
        }
    }

    #[test]
    fn kill_to_start_removes_one_line_at_a_time() {
        let mut e = edit("one\ntwo\nthree");
        e.set_cursor("one\ntwo\nth".len());
        for (text, cursor) in [
            ("one\ntwo\nree", "one\ntwo\n".len()),
            ("one\nree", "one\n".len()),
            ("ree", 0),
            ("ree", 0),
        ] {
            e.kill_to_start();
            assert_eq!((e.text(), e.cursor), (text, cursor));
        }
    }

    #[test]
    fn kills_feed_the_shared_buffer_and_ctrl_y_pastes() {
        let mut kills = KillBuffer::default();
        let press = |kills: &mut KillBuffer, e: &mut LineEdit, c, modifiers| {
            assert!({
                kills.begin_stroke();
                e.handle(KeyEvent::new(KeyCode::Char(c), modifiers), kills)
            });
        };
        let mut e = edit("one two");
        press(&mut kills, &mut e, 'w', KeyModifiers::CONTROL);
        assert_eq!(e.text(), "one ");
        let mut other = edit("one ");
        press(&mut kills, &mut other, 'y', KeyModifiers::CONTROL);
        assert_eq!(other.text(), "one two");

        e.set_text("one two three");
        e.end();
        kills.begin_stroke();
        e.kill(LineEdit::kill_word_back, &mut kills);
        e.kill(LineEdit::kill_word_back, &mut kills);
        assert_eq!(e.text(), "one ");
        press(&mut kills, &mut e, 'y', KeyModifiers::CONTROL);
        assert_eq!(e.text(), "one two three");

        let mut empty = edit("keep");
        kills = KillBuffer::default();
        press(&mut kills, &mut empty, 'y', KeyModifiers::CONTROL);
        assert_eq!(empty.text(), "keep");
    }

    #[test]
    fn consecutive_kills_join_until_a_non_kill_key() {
        let mut kills = KillBuffer::default();
        let press = |kills: &mut KillBuffer, e: &mut LineEdit, code, modifiers| {
            assert!({
                kills.begin_stroke();
                e.handle(KeyEvent::new(code, modifiers), kills)
            });
        };
        let ctrl = KeyModifiers::CONTROL;
        let alt = KeyModifiers::ALT;
        let mut e = edit("foo bar baz");
        press(&mut kills, &mut e, KeyCode::Char('w'), ctrl);
        press(&mut kills, &mut e, KeyCode::Char('w'), ctrl);
        press(&mut kills, &mut e, KeyCode::Char('y'), ctrl);
        assert_eq!(e.text(), "foo bar baz");

        let mut e = edit("one two three");
        e.set_cursor("one ".len());
        press(&mut kills, &mut e, KeyCode::Char('k'), ctrl);
        press(&mut kills, &mut e, KeyCode::Char('k'), ctrl);
        press(&mut kills, &mut e, KeyCode::Char('y'), ctrl);
        assert_eq!(e.text(), "one two three");

        let mut e = edit("foo bar baz");
        e.set_cursor("foo ".len());
        press(&mut kills, &mut e, KeyCode::Char('w'), ctrl);
        press(&mut kills, &mut e, KeyCode::Char('d'), alt);
        press(&mut kills, &mut e, KeyCode::Char('y'), ctrl);
        assert_eq!(e.text(), "foo bar baz");

        let mut e = edit("foo bar baz");
        press(&mut kills, &mut e, KeyCode::Char('w'), ctrl);
        press(&mut kills, &mut e, KeyCode::Home, KeyModifiers::NONE);
        for _ in 0..4 {
            press(&mut kills, &mut e, KeyCode::Right, KeyModifiers::NONE);
        }
        press(&mut kills, &mut e, KeyCode::Char('w'), ctrl);
        press(&mut kills, &mut e, KeyCode::Char('y'), ctrl);
        assert_eq!(e.text(), "foo bar ");

        let mut e = edit("foo bar baz");
        press(&mut kills, &mut e, KeyCode::Char('w'), ctrl);
        press(&mut kills, &mut e, KeyCode::Char('x'), KeyModifiers::NONE);
        press(&mut kills, &mut e, KeyCode::Char('w'), ctrl);
        press(&mut kills, &mut e, KeyCode::Char('y'), ctrl);
        assert_eq!(e.text(), "foo bar x");

        let mut e = edit("foo bar baz");
        press(&mut kills, &mut e, KeyCode::Char('w'), ctrl);
        press(&mut kills, &mut e, KeyCode::Char('y'), ctrl);
        press(&mut kills, &mut e, KeyCode::Char('w'), ctrl);
        press(&mut kills, &mut e, KeyCode::Char('y'), ctrl);
        assert_eq!(e.text(), "foo bar baz");
    }

    #[test]
    fn kill_to_end_removes_one_line_at_a_time() {
        let mut e = edit("one\ntwo\nthree");
        e.set_cursor(1);
        for text in ["o\ntwo\nthree", "o\nthree", "o", "o"] {
            e.kill_to_end();
            assert_eq!((e.text(), e.cursor), (text, 1));
        }
    }

    #[test]
    fn kills_that_empty_a_line_remove_it() {
        let mut e = edit("one\r\ntwo\nth");
        for (text, cursor) in [("one\r\ntwo", 8), ("one", 3), ("", 0), ("", 0)] {
            e.kill_to_start();
            assert_eq!((e.text(), e.cursor), (text, cursor));
        }
        let mut e = edit("on\ntwo");
        e.set_cursor(2);
        e.kill_to_start();
        assert_eq!((e.text(), e.cursor), ("two", 0));

        let mut e = edit("one\ntwo\r\nthree");
        e.home();
        for text in ["two\r\nthree", "three", "", ""] {
            e.kill_to_end();
            assert_eq!((e.text(), e.cursor), (text, 0));
        }
        let mut e = edit("one\ntwo");
        e.set_cursor(4);
        e.kill_to_end();
        assert_eq!((e.text(), e.cursor), ("one", 3));
    }

    #[test]
    fn kills_on_an_empty_line_remove_only_that_line() {
        let mut e = edit("1\n2\n");
        e.kill_to_start();
        assert_eq!((e.text(), e.cursor), ("1\n2", 3));
        let mut e = edit("1\n\n2");
        e.set_cursor(2);
        e.kill_to_end();
        assert_eq!((e.text(), e.cursor), ("1\n2", 2));
    }

    #[test]
    fn key_map_consumes_editing_only() {
        let mut kills = KillBuffer::default();
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
                {
                    kills.begin_stroke();
                    edit("hello world").handle(KeyEvent::new(code, modifiers), &mut kills)
                },
                "{code:?}"
            );
        }
        for code in [KeyCode::Esc, KeyCode::Enter, KeyCode::Up, KeyCode::Down] {
            assert!(!{
                kills.begin_stroke();
                edit("").handle(KeyEvent::new(code, KeyModifiers::NONE), &mut kills)
            });
        }
        for modifiers in [
            KeyModifiers::CONTROL,
            KeyModifiers::SUPER,
            KeyModifiers::ALT | KeyModifiers::CONTROL,
        ] {
            assert!(!{
                kills.begin_stroke();
                edit("").handle(KeyEvent::new(KeyCode::Char('c'), modifiers), &mut kills)
            });
        }
        let mut e = edit("");
        assert!({
            kills.begin_stroke();
            e.handle(KeyEvent::new(KeyCode::Char('你'), KeyModifiers::SHIFT), &mut kills)
        });
        assert_eq!(e.text(), "你");
        let mut release = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE);
        release.kind = KeyEventKind::Release;
        assert!(!{
            kills.begin_stroke();
            e.handle(release, &mut kills)
        });
    }
}
