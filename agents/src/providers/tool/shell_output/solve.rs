#![allow(dead_code)]

use super::lines::Line;
use super::shape::OutputShape;
use std::ops::BitAnd;

/// Packed membership bits for nonnegative indices.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BitSet(Vec<u64>);

impl BitSet {
    /// Create an empty set with capacity for indices below len.
    pub fn new(len: usize) -> Self {
        Self(vec![0; len.div_ceil(64)])
    }

    /// Include index in the set.
    pub fn insert(&mut self, index: usize) {
        self.0[index / 64] |= 1u64 << (index % 64);
    }

    /// Return whether index belongs to the set.
    pub fn contains(&self, index: usize) -> bool {
        self.0
            .get(index / 64)
            .is_some_and(|word| word & (1u64 << (index % 64)) != 0)
    }

    /// Yield member indices in ascending order.
    pub fn iter(&self) -> BitSetIter<'_> {
        BitSetIter {
            words: &self.0,
            word: 0,
            remaining: self.0.first().copied().unwrap_or(0),
        }
    }
}

impl BitAnd for &BitSet {
    type Output = BitSet;

    /// Return the intersection of two membership sets.
    fn bitand(self, rhs: Self) -> Self::Output {
        BitSet(
            self.0
                .iter()
                .zip(&rhs.0)
                .map(|(left, right)| left & right)
                .collect(),
        )
    }
}

/// An ascending, allocation-free traversal of a packed membership set.
pub struct BitSetIter<'a> {
    words: &'a [u64],
    word: usize,
    remaining: u64,
}

impl Iterator for BitSetIter<'_> {
    type Item = usize;

    /// Return the next member index, or None when all words are exhausted.
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if self.remaining != 0 {
                let bit = self.remaining.trailing_zeros() as usize;
                self.remaining &= self.remaining - 1;
                return Some(self.word * 64 + bit);
            }
            self.word += 1;
            self.remaining = *self.words.get(self.word)?;
        }
    }
}

/// Feasible line-table positions for c0..cn, including the start and end cuts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FeasibleCuts(pub Vec<BitSet>);

/// Feasible cuts and, per command, the outcome indices participating in a solution.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Solution {
    pub cuts: FeasibleCuts,
    pub outcomes: Vec<BitSet>,
    /// Whether each cut can fall inside a truncation gap.
    pub in_gap: Vec<bool>,
}

/// Solve output constraints and filter hints; return None when no partition fits.
pub fn solve(_shapes: &[OutputShape], _lines: &[Line], _text: &str) -> Option<Solution> {
    todo!()
}
