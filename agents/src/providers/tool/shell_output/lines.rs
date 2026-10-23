#![allow(dead_code)]

use super::Stream;
use std::ops::Range;

/// A visible line's original byte range, or a truncation gap with a hidden line count.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Line {
    Text(Range<usize>),
    Gap { hidden: Option<usize> },
}

/// Build a line table into text, excluding prefixes and recording truncation gaps.
pub fn lines(_text: &str, _stream: &Stream) -> Vec<Line> {
    todo!()
}
