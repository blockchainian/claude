#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Motion {
    Left,
    Right,
    WordLeft(WordKind),
    WordRight(WordKind),
    LineStart,
    LineEnd,
    TextStart,
    TextEnd,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WordKind {
    Whitespace,
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

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum EditCommand {
    Move(Motion),
    Delete(Motion),
    Kill(Motion),
    Insert(String),
    Yank,
}
