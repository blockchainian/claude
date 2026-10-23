#![allow(dead_code)]

use super::lines::Line;
use super::shape::OutputShape;
use super::solve::Solution;
use std::ops::Range;

/// Owners of contiguous line-table ranges, including gaps.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ownership(pub Vec<(Range<usize>, Owner)>);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Owner {
    Command(usize),
    Block(Range<usize>),
    Plain,
}

/// Assign line-table ranges using feasible cuts, outcome kinds and original text.
pub fn ownership(
    _shapes: &[OutputShape],
    _solution: &Solution,
    _lines: &[Line],
    _text: &str,
) -> Ownership {
    todo!()
}
