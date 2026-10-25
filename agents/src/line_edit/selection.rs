use super::*;
use std::ops::Range;

impl<P: Placeholder> Editor<P> {
    fn assert_boundary(&self, offset: usize) {
        assert!(
            offset == self.text.len()
                || self
                    .text
                    .grapheme_indices(true)
                    .any(|(byte, _)| byte == offset),
            "caret must be a grapheme boundary"
        );
    }

    /// Places the caret at a grapheme boundary, snaps outside placeholders and clears selection.
    /// Panics if `byte` is not a grapheme boundary in the text.
    pub fn set_cursor(&mut self, byte: usize) {
        self.assert_boundary(byte);
        self.home();
        self.cursor = self.placeholder_caret(byte);
        self.clear_selection();
    }

    /// Clears the selection without moving the caret.
    pub fn clear_selection(&mut self) {
        self.selection = None;
    }

    /// Selects from anchor to head and places the caret outside any placeholder at head.
    /// Panics if either offset is not a grapheme boundary in the text.
    pub fn select(&mut self, anchor: usize, head: usize) {
        self.assert_boundary(anchor);
        self.set_cursor(head);
        self.selection = Some((anchor, head));
    }

    /// Returns the nonempty selection range, widened to include whole placeholders.
    pub fn selection(&self) -> Option<Range<usize>> {
        let (anchor, end) = self.selection?;
        (anchor != end).then(|| {
            self.placeholder_boundary(anchor.min(end), false)
                ..self.placeholder_boundary(anchor.max(end), true)
        })
    }

    /// Returns the selected text, or None when the selection is empty.
    pub fn selected_text(&self) -> Option<&str> {
        Some(&self.text[self.selection()?])
    }

    pub(super) fn delete_selection(&mut self) -> bool {
        self.replace_selection("")
    }

    pub(super) fn replace_selection(&mut self, text: impl ToString) -> bool {
        let Some(range) = self.selection() else {
            return false;
        };
        let text = text.to_string();
        let range = self.replace_placeholders(range, &text);
        self.cursor = range.start + text.len();
        self.align_cursor();
        self.set_cursor(self.cursor);
        true
    }
}
