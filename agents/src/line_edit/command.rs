/// A caret movement over text, treating placeholders as atomic spans.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Motion {
    /// Move one grapheme backward.
    Left,
    /// Move one grapheme forward.
    Right,
    /// Move to the start of the preceding word.
    WordLeft(WordKind),
    /// Move to the end of the following word.
    WordRight(WordKind),
    /// Move to this line's start, or the preceding line's start when already there.
    LineStart,
    /// Move to this line's end, or the following line's end when already there.
    LineEnd,
    /// Move to the start of all text.
    TextStart,
    /// Move to the end of all text.
    TextEnd,
}

/// Character classes used to find word boundaries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WordKind {
    /// Treat runs of non-whitespace graphemes as words.
    Whitespace,
    /// Treat graphemes starting with alphanumeric characters as words.
    Alphanumeric,
}

impl WordKind {
    pub(super) fn contains(self, grapheme: &str) -> bool {
        match self {
            Self::Whitespace => !grapheme.chars().all(char::is_whitespace),
            Self::Alphanumeric => grapheme.chars().next().is_some_and(char::is_alphanumeric),
        }
    }
}

/// A movement, edit or yank applied to an editor.
#[derive(Debug, PartialEq, Eq)]
pub enum EditCommand {
    /// Move the caret and clear selection.
    Move(Motion),
    /// Delete selection or text crossed by the motion without saving it.
    Delete(Motion),
    /// Clear selection and save text removed by the motion; line kills also remove empty lines.
    Kill(Motion),
    /// Insert text, replacing selection.
    Insert(String),
    /// Insert the kill buffer text, replacing selection.
    Yank,
}
