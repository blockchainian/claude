use syntect::parsing::Regex;

/// The possible outputs of one command, with success first.
pub struct OutputShape {
    pub outcomes: Vec<Outcome>,
    pub conditional: bool,
}

/// One possible command result and its output constraints.
pub struct Outcome {
    pub lines: LineRange,
    pub kind: Kind,
    pub first: Option<Pattern>,
    pub last: Option<Pattern>,
    pub every: Option<Pattern>,
    pub known: Option<String>,
    pub blank_only: bool,
}

/// A line pattern that constrains or suggests a boundary.
pub struct Pattern {
    pub regex: Regex,
    pub strength: Strength,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Strength {
    Required,
    Hint,
}

/// Inclusive line counts; no maximum means unbounded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LineRange {
    pub min: usize,
    pub max: Option<usize>,
}

#[derive(Clone, PartialEq, Eq)]
pub enum Kind {
    File { path: String, numbered: bool },
    Diff,
    Plain,
}
