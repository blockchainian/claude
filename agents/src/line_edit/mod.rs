use crossterm::event::KeyEvent;

mod command;
pub(crate) mod keymap;
pub(crate) use command::{EditCommand, Motion, WordKind};
use unicode_segmentation::UnicodeSegmentation;

mod kill_buffer;
mod selection;

pub(crate) use kill_buffer::{Direction, KillBuffer};
mod layout;
pub(crate) use layout::{CaretStop, Vertical, Window, Wrap};

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
    fn left(&mut self) {
        self.preferred_column = None;
        self.cursor = self.text[..self.cursor]
            .grapheme_indices(true)
            .next_back()
            .map_or(0, |(i, _)| i);
        self.cursor = self.caret_boundary(self.cursor, false);
    }
    fn right(&mut self) {
        self.preferred_column = None;
        if let Some(g) = self.text[self.cursor..].graphemes(true).next() {
            self.cursor += g.len();
        }
        self.cursor = self.caret_boundary(self.cursor, true);
    }
    fn home(&mut self) {
        self.preferred_column = None;
        self.cursor = 0;
    }
    fn end(&mut self) {
        self.preferred_column = None;
        self.cursor = self.text.len();
    }
    /// Moves to the start of the caret's line, or of the previous line when already there.
    fn line_start(&mut self) {
        self.preferred_column = None;
        let line_start = |end: usize| self.text[..end].rfind('\n').map_or(0, |i| i + 1);
        let mut start = line_start(self.cursor);
        if start == self.cursor && start > 0 {
            start = line_start(start - 1);
        }
        self.cursor = self.caret_boundary(start, false);
    }
    /// Moves to the end of the caret's line, or of the next line when already there.
    fn line_end(&mut self) {
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
    fn word_left(&mut self, kind: WordKind) {
        self.preferred_column = None;
        let mut word = false;
        for (i, g) in self.text[..self.cursor].grapheme_indices(true).rev() {
            if !kind.contains(g) {
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
    fn word_right(&mut self, kind: WordKind) {
        self.preferred_column = None;
        let mut word = false;
        let start = self.cursor;
        for (i, g) in self.text[start..].grapheme_indices(true) {
            if !kind.contains(g) {
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
    fn kill_to_end(&mut self) {
        if !self.remove_empty_line(false) {
            let start = self.cursor;
            self.line_end();
            self.cursor = self.replace_placeholders(start..self.cursor, "").start;
            self.remove_empty_line(false);
        }
        self.align_cursor();
    }
    /// Deletes what Ctrl+A would move over.
    fn kill_to_start(&mut self) {
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
    /// Saves the removed text, joined in text order with earlier kills of this key.
    fn kill(&mut self, delete: impl FnOnce(&mut Self), kills: &mut KillBuffer) {
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
    pub(crate) fn move_caret(&mut self, motion: Motion) {
        self.clear_selection();
        match motion {
            Motion::Left => self.left(),
            Motion::Right => self.right(),
            Motion::WordLeft(kind) => self.word_left(kind),
            Motion::WordRight(kind) => self.word_right(kind),
            Motion::LineStart => self.line_start(),
            Motion::LineEnd => self.line_end(),
            Motion::TextStart => self.home(),
            Motion::TextEnd => self.end(),
        }
    }

    fn delete_motion(&mut self, motion: Motion) {
        let start = self.cursor;
        self.move_caret(motion);
        let range = start.min(self.cursor)..start.max(self.cursor);
        self.cursor = self.replace_placeholders(range, "").start;
        self.align_cursor();
    }

    pub(crate) fn apply(&mut self, command: EditCommand, kills: &mut KillBuffer) {
        match command {
            EditCommand::Move(motion) => self.move_caret(motion),
            EditCommand::Delete(motion) => {
                if !self.delete_selection() {
                    self.delete_motion(motion);
                }
            }
            EditCommand::Kill(motion) => {
                self.clear_selection();
                self.kill(
                    |editor| match motion {
                        Motion::LineStart => editor.kill_to_start(),
                        Motion::LineEnd => editor.kill_to_end(),
                        _ => editor.delete_motion(motion),
                    },
                    kills,
                );
            }
            EditCommand::Insert(text) => self.insert(text),
            EditCommand::Yank => self.insert(kills.text()),
        }
    }

    pub(crate) fn handle_key(&mut self, key: KeyEvent, kills: &mut KillBuffer) -> bool {
        let Some(command) = keymap::command_for_key(keymap::normalize_control_code(key)) else {
            return false;
        };
        self.apply(command, kills);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEventKind, KeyModifiers};

    fn edit(text: &str) -> LineEdit {
        let mut edit = LineEdit::default();
        edit.set_text(text);
        edit
    }

    #[test]
    fn graphemes_move_insert_and_delete() {
        let mut e = edit("你e\u{301}👨‍👩‍👧‍👦好");
        e.move_caret(Motion::Left);
        assert_eq!(e.cursor, "你e\u{301}👨‍👩‍👧‍👦".len());
        e.apply(
            EditCommand::Delete(Motion::Left),
            &mut KillBuffer::default(),
        );
        assert_eq!(e.text(), "你e\u{301}好");
        e.move_caret(Motion::Left);
        assert_eq!(e.cursor, "你".len());
        e.apply(
            EditCommand::Delete(Motion::Right),
            &mut KillBuffer::default(),
        );
        assert_eq!(e.text(), "你好");
        e.insert("e");
        e.insert('\u{301}');
        assert_eq!(e.text(), "你e\u{301}好");
        e.move_caret(Motion::Left);
        assert_eq!(e.cursor, "你".len());
        e.move_caret(Motion::Right);
        assert_eq!(e.cursor, "你e\u{301}".len());
        e.move_caret(Motion::TextStart);
        e.apply(
            EditCommand::Delete(Motion::Left),
            &mut KillBuffer::default(),
        );
        assert_eq!(e.cursor, 0);
        e.move_caret(Motion::TextEnd);
        e.apply(
            EditCommand::Delete(Motion::Right),
            &mut KillBuffer::default(),
        );
        assert_eq!(e.cursor, e.text.len());
        e.clear();
        assert_eq!((e.text(), e.cursor), ("", 0));
    }

    #[test]
    fn insertion_and_kills_preserve_grapheme_boundaries() {
        let mut e = edit("a\u{301}b");
        e.move_caret(Motion::TextStart);
        e.insert("x\u{200d}");
        assert!(e.text.grapheme_indices(true).any(|(i, _)| i == e.cursor));
        let mut e = edit("👩👧");
        e.move_caret(Motion::Left);
        e.insert('\u{200d}');
        assert_eq!(e.cursor, e.text.len());
        e.apply(
            EditCommand::Delete(Motion::Left),
            &mut KillBuffer::default(),
        );
        assert_eq!(e.text(), "");
    }

    #[test]
    fn words_and_all_kills() {
        let mut e = edit("one  你好 three  ");
        e.move_caret(Motion::WordLeft(WordKind::Whitespace));
        assert_eq!(&e.text[e.cursor..], "three  ");
        e.move_caret(Motion::WordLeft(WordKind::Whitespace));
        assert_eq!(&e.text[e.cursor..], "你好 three  ");
        e.move_caret(Motion::WordRight(WordKind::Whitespace));
        assert_eq!(&e.text[e.cursor..], " three  ");
        e.apply(
            EditCommand::Kill(Motion::WordRight(WordKind::Whitespace)),
            &mut KillBuffer::default(),
        );
        assert_eq!(e.text(), "one  你好  ");
        e.apply(
            EditCommand::Kill(Motion::WordLeft(WordKind::Whitespace)),
            &mut KillBuffer::default(),
        );
        assert_eq!(e.text(), "one    ");
        e.apply(
            EditCommand::Kill(Motion::LineEnd),
            &mut KillBuffer::default(),
        );
        assert_eq!(e.text(), "one  ");
        e.apply(
            EditCommand::Kill(Motion::LineStart),
            &mut KillBuffer::default(),
        );
        assert_eq!(e.text(), "");
        e.move_caret(Motion::WordLeft(WordKind::Whitespace));
        e.move_caret(Motion::WordRight(WordKind::Whitespace));
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
                e.move_caret(Motion::TextStart);
                e.move_caret(Motion::WordRight(WordKind::Whitespace));
                e.move_caret(Motion::Right);
                kills.begin_stroke();
                e.apply(
                    keymap::command_for_key(KeyEvent::new(code, modifiers)).unwrap(),
                    &mut kills,
                );
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
        let mut e = edit("one\r\ntwo\nthree");
        e.set_cursor("one\r\ntw".len());
        for cursor in ["one\r\n".len(), 0, 0] {
            e.move_caret(Motion::LineStart);
            assert_eq!(e.cursor, cursor);
        }
        for cursor in ["one".len(), "one\r\ntwo".len(), e.text.len(), e.text.len()] {
            e.move_caret(Motion::LineEnd);
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
            e.apply(
                EditCommand::Kill(Motion::LineStart),
                &mut KillBuffer::default(),
            );
            assert_eq!((e.text(), e.cursor), (text, cursor));
        }
    }

    #[test]
    fn kills_feed_the_shared_buffer_and_ctrl_y_pastes() {
        let mut kills = KillBuffer::default();
        let press = |kills: &mut KillBuffer, e: &mut LineEdit, c, modifiers| {
            assert!({
                kills.begin_stroke();
                e.handle_key(KeyEvent::new(KeyCode::Char(c), modifiers), kills)
            });
        };
        let mut e = edit("one two");
        press(&mut kills, &mut e, 'w', KeyModifiers::CONTROL);
        assert_eq!(e.text(), "one ");
        let mut other = edit("one ");
        press(&mut kills, &mut other, 'y', KeyModifiers::CONTROL);
        assert_eq!(other.text(), "one two");

        e.set_text("one two three");
        e.move_caret(Motion::TextEnd);
        kills.begin_stroke();
        e.apply(
            EditCommand::Kill(Motion::WordLeft(WordKind::Whitespace)),
            &mut kills,
        );
        e.apply(
            EditCommand::Kill(Motion::WordLeft(WordKind::Whitespace)),
            &mut kills,
        );
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
                e.handle_key(KeyEvent::new(code, modifiers), kills)
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
            e.apply(
                EditCommand::Kill(Motion::LineEnd),
                &mut KillBuffer::default(),
            );
            assert_eq!((e.text(), e.cursor), (text, 1));
        }
    }

    #[test]
    fn kills_that_empty_a_line_remove_it() {
        let mut e = edit("one\r\ntwo\nth");
        for (text, cursor) in [("one\r\ntwo", 8), ("one", 3), ("", 0), ("", 0)] {
            e.apply(
                EditCommand::Kill(Motion::LineStart),
                &mut KillBuffer::default(),
            );
            assert_eq!((e.text(), e.cursor), (text, cursor));
        }
        let mut e = edit("on\ntwo");
        e.set_cursor(2);
        e.apply(
            EditCommand::Kill(Motion::LineStart),
            &mut KillBuffer::default(),
        );
        assert_eq!((e.text(), e.cursor), ("two", 0));

        let mut e = edit("one\ntwo\r\nthree");
        e.move_caret(Motion::TextStart);
        for text in ["two\r\nthree", "three", "", ""] {
            e.apply(
                EditCommand::Kill(Motion::LineEnd),
                &mut KillBuffer::default(),
            );
            assert_eq!((e.text(), e.cursor), (text, 0));
        }
        let mut e = edit("one\ntwo");
        e.set_cursor(4);
        e.apply(
            EditCommand::Kill(Motion::LineEnd),
            &mut KillBuffer::default(),
        );
        assert_eq!((e.text(), e.cursor), ("one", 3));
    }

    #[test]
    fn kills_on_an_empty_line_remove_only_that_line() {
        let mut e = edit("1\n2\n");
        e.apply(
            EditCommand::Kill(Motion::LineStart),
            &mut KillBuffer::default(),
        );
        assert_eq!((e.text(), e.cursor), ("1\n2", 3));
        let mut e = edit("1\n\n2");
        e.set_cursor(2);
        e.apply(
            EditCommand::Kill(Motion::LineEnd),
            &mut KillBuffer::default(),
        );
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
                    edit("hello world").handle_key(KeyEvent::new(code, modifiers), &mut kills)
                },
                "{code:?}"
            );
        }
        for code in [KeyCode::Esc, KeyCode::Enter, KeyCode::Up, KeyCode::Down] {
            assert!(!{
                kills.begin_stroke();
                edit("").handle_key(KeyEvent::new(code, KeyModifiers::NONE), &mut kills)
            });
        }
        for modifiers in [
            KeyModifiers::CONTROL,
            KeyModifiers::SUPER,
            KeyModifiers::ALT | KeyModifiers::CONTROL,
        ] {
            assert!(!{
                kills.begin_stroke();
                edit("").handle_key(KeyEvent::new(KeyCode::Char('c'), modifiers), &mut kills)
            });
        }
        let mut e = edit("");
        assert!({
            kills.begin_stroke();
            e.handle_key(
                KeyEvent::new(KeyCode::Char('你'), KeyModifiers::SHIFT),
                &mut kills,
            )
        });
        assert_eq!(e.text(), "你");
        let mut release = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE);
        release.kind = KeyEventKind::Release;
        assert!(!{
            kills.begin_stroke();
            e.handle_key(release, &mut kills)
        });
    }

    #[test]
    fn readline_word_boundaries() {
        // Captured from GNU readline's emacs bindings in an en_US.UTF-8 Bash.
        // bind -x exposes READLINE_POINT in characters; editor offsets are bytes.
        let cases: serde_json::Value =
            serde_json::from_str(include_str!("../../tests/fixtures/readline-words.json")).unwrap();
        for case in cases.as_array().unwrap() {
            let text = case["input"].as_str().unwrap();
            let key = case["key"].as_str().unwrap();
            let expected = case["result"].as_str().unwrap();
            let point = case["point"].as_u64().unwrap() as usize;
            let mut e = edit(text);
            if key == "Alt+F" || key == "Alt+D" {
                e.set_cursor(0);
            }
            let (code, modifiers) = match key {
                "Alt+B" => (KeyCode::Char('b'), KeyModifiers::ALT),
                "Alt+F" => (KeyCode::Char('f'), KeyModifiers::ALT),
                "Alt+D" => (KeyCode::Char('d'), KeyModifiers::ALT),
                "Alt+Backspace" => (KeyCode::Backspace, KeyModifiers::ALT),
                "Ctrl+W" => (KeyCode::Char('w'), KeyModifiers::CONTROL),
                _ => panic!("unknown readline key: {key}"),
            };
            let mut kills = KillBuffer::default();
            kills.begin_stroke();
            assert!(e.handle_key(KeyEvent::new(code, modifiers), &mut kills));
            let expected_cursor = expected
                .chars()
                .take(point)
                .map(char::len_utf8)
                .sum::<usize>();
            assert_eq!(
                (e.text(), e.cursor()),
                (expected, expected_cursor),
                "{text:?} key {key}"
            );
        }
    }
    #[test]
    fn commands_replace_or_clear_selection() {
        let mut e = edit("foo-bar baz");
        let mut kills = KillBuffer::default();
        e.select(0, 3);
        e.apply(EditCommand::Delete(Motion::Right), &mut kills);
        assert_eq!((e.text(), e.cursor()), ("-bar baz", 0));
        assert_eq!(kills.text(), "");
        e.select(0, 4);
        e.apply(EditCommand::Insert("one".into()), &mut kills);
        assert_eq!(e.text(), "one baz");
        e.select(0, 3);
        e.apply(EditCommand::Move(Motion::Right), &mut kills);
        assert_eq!(e.selection_range(), None);
        e.select(0, 4);
        e.apply(
            EditCommand::Kill(Motion::WordRight(WordKind::Alphanumeric)),
            &mut kills,
        );
        assert_eq!(e.text(), "one ");
        assert_eq!(kills.text(), "baz");
        e.select(0, 3);
        e.apply(EditCommand::Yank, &mut kills);
        assert_eq!(e.text(), "baz ");
    }
}
