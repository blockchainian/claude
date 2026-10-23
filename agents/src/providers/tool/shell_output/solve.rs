use super::lines::Line;
use super::shape::{OutputShape, Pattern, Strength};
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
    /// Whether each cut can fall inside a truncation gap, where highlighting cannot start.
    pub in_gap: Vec<bool>,
    /// The lexicographically earliest complete partition, in line-table positions.
    pub earliest: Vec<usize>,
}

/// Solve output constraints and filter hints; return None when no partition fits.
pub fn solve(shapes: &[OutputShape], lines: &[Line], text: &str) -> Option<Solution> {
    let table = Table::new(lines, text);
    let reversed = table.reversed();
    let forward_rules = rules(shapes, &table, false);
    let backward_rules = rules(shapes, &reversed, true);
    let n = shapes.len();
    let mut masks = vec![table.full(); n + 1];
    masks[0] = table.point(0, 0);
    masks[n] = table.point(table.cells.len() - 1, table.total);
    if n == 0 && table.total != 0 {
        return None;
    }
    loop {
        let forward = layers(&table, &forward_rules, &masks);
        let reverse_masks: Vec<_> = masks.iter().rev().map(|d| table.mirror(d)).collect();
        let backward: Vec<Layer> = layers(&reversed, &backward_rules, &reverse_masks)
            .into_iter()
            .rev()
            .map(|layer| layer.iter().map(|d| table.mirror(d)).collect())
            .collect();
        let mut feasible = Vec::new();
        let mut outcomes: Vec<_> = shapes
            .iter()
            .map(|s| BitSet::new(s.outcomes.len()))
            .collect();
        for i in 0..=n {
            let mut domain = vec![None; table.cells.len()];
            for (previous, prefix) in forward[i].iter().enumerate() {
                let left = i
                    .checked_sub(1)
                    .map(|j| (&forward_rules[j][previous / 2], previous % 2 == 1));
                let complete =
                    intersect(prefix, &suffix(&table, &forward_rules, &backward, i, left));
                if i > 0 && !empty(&complete) {
                    outcomes[i - 1].insert(previous / 2);
                }
                union(&mut domain, &complete);
            }
            if empty(&domain) {
                return None;
            }
            feasible.push(domain);
        }
        let mut changed = false;
        for i in 1..n {
            if feasible[i].iter().flatten().count() <= 1 {
                continue;
            }
            let mut votes = Vec::new();
            for (command, opening) in [(i, true), (i - 1, false)] {
                for index in outcomes[command].iter() {
                    let outcome = &shapes[command].outcomes[index];
                    let edge = if opening {
                        &outcome.first
                    } else {
                        &outcome.last
                    };
                    for (pattern, feature) in edge
                        .iter()
                        .filter(|p| p.strength == Strength::Hint)
                        .map(|p| (p, true))
                        .chain(outcome.every.iter().map(|p| (p, false)))
                    {
                        let mut candidates: Vec<_> = feasible[i]
                            .iter()
                            .enumerate()
                            .filter_map(|(cell, domain)| {
                                if domain.is_none() || table.cells[cell].gap {
                                    return None;
                                }
                                let position = table.cells[cell].position;
                                let row = if opening {
                                    table.next[position]
                                } else {
                                    table.previous[position]
                                };
                                let matched = row.is_some_and(|row| pattern.regex.is_match(row));
                                let previous_matches = feature
                                    && opening
                                    && table.previous[position]
                                        .is_some_and(|row| pattern.regex.is_match(row));
                                (matched && !previous_matches).then_some(cell)
                            })
                            .collect();
                        candidates.dedup_by_key(|cell| table.cells[*cell].lo);
                        if let [cell] = candidates.as_slice() {
                            votes.push(*cell);
                        }
                    }
                }
            }
            if let Some(&cell) = votes.first()
                && votes.iter().all(|vote| *vote == cell)
            {
                masks[i] = table.point(cell, table.cells[cell].lo);
                changed = true;
                break;
            }
        }
        if changed {
            continue;
        }
        let mut cuts = Vec::with_capacity(n + 1);
        let mut in_gap = Vec::with_capacity(n + 1);
        for domain in &feasible {
            let mut set = BitSet::new(lines.len() + 1);
            let mut hidden = false;
            for (cell, value) in table.cells.iter().zip(domain) {
                if value.is_some() {
                    set.insert(cell.position);
                    hidden |= cell.gap;
                }
            }
            cuts.push(set);
            in_gap.push(hidden);
        }
        let mut earliest = vec![0];
        let mut selected = vec![table.point(0, 0)];
        for i in 0..n {
            let mut next = Vec::new();
            for rule in &forward_rules[i] {
                for nonempty in [false, true] {
                    let mut input = vec![None; table.cells.len()];
                    for (previous, domain) in selected.iter().enumerate() {
                        let left = i
                            .checked_sub(1)
                            .map(|j| (&forward_rules[j][previous / 2], previous % 2 == 1));
                        union(&mut input, &join(domain, left, Some((rule, nonempty))));
                    }
                    next.push(intersect(
                        &intersect(&rule.apply(&table, &input, nonempty), &masks[i + 1]),
                        &suffix(
                            &table,
                            &forward_rules,
                            &backward,
                            i + 1,
                            Some((rule, nonempty)),
                        ),
                    ));
                }
            }
            let position = next
                .iter()
                .flat_map(|domain| domain.iter().zip(&table.cells))
                .filter_map(|(value, cell)| value.map(|_| cell.position))
                .min()?;
            earliest.push(position);
            // Keep every outcome tied at this cut, so selecting an outcome now
            // cannot force a later cut past the lexicographically earliest path.
            for domain in &mut next {
                for (value, cell) in domain.iter_mut().zip(&table.cells) {
                    if cell.position != position {
                        *value = None;
                    }
                }
            }
            selected = next;
        }
        return Some(Solution {
            cuts: FeasibleCuts(cuts),
            outcomes,
            in_gap,
            earliest,
        });
    }
}

type Domain = Vec<Option<(usize, usize)>>;

#[derive(Clone)]
struct Cell {
    lo: usize,
    hi: usize,
    position: usize,
    gap: bool,
    unknown: bool,
}

struct Table<'a> {
    rows: Vec<Option<&'a str>>,
    cells: Vec<Cell>,
    previous: Vec<Option<&'a str>>,
    next: Vec<Option<&'a str>>,
    total: usize,
}

impl<'a> Table<'a> {
    fn new(lines: &[Line], text: &'a str) -> Self {
        let mut rows = Vec::with_capacity(lines.len());
        let mut cells = Vec::with_capacity(lines.len() * 2 + 1);
        let mut offset = 0usize;
        for (position, line) in lines.iter().enumerate() {
            cells.push(Cell {
                lo: offset,
                hi: offset,
                position,
                gap: false,
                unknown: false,
            });
            match line {
                Line::Text(range) => {
                    rows.push(Some(
                        text[range.clone()]
                            .trim_end_matches('\n')
                            .trim_end_matches('\r'),
                    ));
                    offset = offset.saturating_add(1);
                }
                Line::Gap { hidden } => {
                    rows.push(None);
                    let end = offset.saturating_add(hidden.unwrap_or(0));
                    if *hidden != Some(0) {
                        cells.push(Cell {
                            lo: offset,
                            hi: end,
                            position,
                            gap: true,
                            unknown: hidden.is_none(),
                        });
                    }
                    offset = end;
                }
            }
        }
        cells.push(Cell {
            lo: offset,
            hi: offset,
            position: lines.len(),
            gap: false,
            unknown: false,
        });
        let mut previous = vec![None; rows.len() + 1];
        for i in 0..rows.len() {
            previous[i + 1] = match lines[i] {
                Line::Text(_) => rows[i],
                Line::Gap { hidden: Some(0) } => previous[i],
                Line::Gap { .. } => None,
            };
        }
        let mut next = vec![None; rows.len() + 1];
        for i in (0..rows.len()).rev() {
            next[i] = match lines[i] {
                Line::Text(_) => rows[i],
                Line::Gap { hidden: Some(0) } => next[i + 1],
                Line::Gap { .. } => None,
            };
        }
        Self {
            rows,
            cells,
            previous,
            next,
            total: offset,
        }
    }

    fn reversed(&self) -> Self {
        Self {
            rows: self.rows.iter().rev().copied().collect(),
            previous: self.next.iter().rev().copied().collect(),
            next: self.previous.iter().rev().copied().collect(),
            cells: self
                .cells
                .iter()
                .rev()
                .map(|c| Cell {
                    lo: self.total - c.hi,
                    hi: self.total - c.lo,
                    position: self.rows.len() - c.position - usize::from(c.gap),
                    gap: c.gap,
                    unknown: c.unknown,
                })
                .collect(),
            total: self.total,
        }
    }

    fn full(&self) -> Domain {
        self.cells.iter().map(|c| Some((c.lo, c.hi))).collect()
    }

    fn point(&self, cell: usize, offset: usize) -> Domain {
        let mut result = vec![None; self.cells.len()];
        result[cell] = Some((offset, offset));
        result
    }

    fn mirror(&self, domain: &Domain) -> Domain {
        domain
            .iter()
            .rev()
            .map(|v| v.map(|(a, b)| (self.total - b, self.total - a)))
            .collect()
    }
}

fn empty(domain: &Domain) -> bool {
    domain.iter().all(Option::is_none)
}

fn intersect(a: &Domain, b: &Domain) -> Domain {
    a.iter()
        .zip(b)
        .map(|(a, b)| match (a, b) {
            (Some((a, b)), Some((c, d))) if a.max(c) <= b.min(d) => Some((*a.max(c), *b.min(d))),
            _ => None,
        })
        .collect()
}

fn include(target: &mut Option<(usize, usize)>, lo: usize, hi: usize) {
    if lo > hi {
        return;
    }
    // A gap uses a convex hull deliberately: enlarging its feasible set preserves
    // uncertainty, so a hidden boundary cannot falsely become a highlighting start.
    *target = Some(match *target {
        Some((a, b)) => (a.min(lo), b.max(hi)),
        None => (lo, hi),
    });
}

struct Rule {
    min: usize,
    max: usize,
    start: Vec<bool>,
    end: Vec<bool>,
    stop: Vec<usize>,
    unknown: Vec<Option<usize>>,
    known: Option<Vec<String>>,
    empty_known: bool,
    mode: Option<String>,
    edge_start: Vec<bool>,
    edge_end: Vec<bool>,
}

fn required(pattern: &Option<Pattern>) -> Option<&Pattern> {
    pattern
        .as_ref()
        .filter(|p| p.strength == Strength::Required)
}

fn rules(shapes: &[OutputShape], table: &Table<'_>, reverse: bool) -> Vec<Vec<Rule>> {
    let mut failures =
        std::collections::HashMap::<&str, std::collections::HashSet<(usize, usize)>>::new();
    for (command, shape) in shapes.iter().enumerate() {
        for (index, outcome) in shape.outcomes.iter().enumerate().skip(1) {
            if let Some(known) = &outcome.known {
                for line in known.lines() {
                    failures.entry(line).or_default().insert((command, index));
                }
            }
        }
    }
    let n = shapes.len();
    (0..n)
        .map(|step| {
            let index = if reverse { n - 1 - step } else { step };
            shapes[index]
                .outcomes
                .iter()
                .enumerate()
                .map(|(outcome_index, outcome)| {
                    let first = required(if reverse {
                        &outcome.last
                    } else {
                        &outcome.first
                    });
                    let last = required(if reverse {
                        &outcome.first
                    } else {
                        &outcome.last
                    });
                    let every = required(&outcome.every);
                    let matches: Vec<_> = table
                        .rows
                        .iter()
                        .map(|row| {
                            row.is_none_or(|row| {
                                failures
                                    .get(row)
                                    .is_none_or(|owners| owners.contains(&(index, outcome_index)))
                                    && (!outcome.blank_only || row.trim().is_empty())
                                    && every.is_none_or(|p| p.regex.is_match(row))
                            })
                        })
                        .collect();
                    let start = table
                        .cells
                        .iter()
                        .map(|c| {
                            c.gap
                                || table
                                    .rows
                                    .get(c.position)
                                    .copied()
                                    .flatten()
                                    .is_none_or(|s| first.is_none_or(|p| p.regex.is_match(s)))
                        })
                        .collect();
                    let end = table
                        .cells
                        .iter()
                        .map(|c| {
                            c.gap
                                || c.position == 0
                                || table.rows[c.position - 1]
                                    .is_none_or(|s| last.is_none_or(|p| p.regex.is_match(s)))
                        })
                        .collect();
                    let edge_start = table
                        .cells
                        .iter()
                        .map(|c| {
                            c.gap
                                || c.position == 0
                                || table.rows[c.position - 1]
                                    .is_none_or(|s| every.is_none_or(|p| !p.regex.is_match(s)))
                        })
                        .collect();
                    let edge_end = table
                        .cells
                        .iter()
                        .map(|c| {
                            c.gap
                                || table
                                    .rows
                                    .get(c.position)
                                    .copied()
                                    .flatten()
                                    .is_none_or(|s| every.is_none_or(|p| !p.regex.is_match(s)))
                        })
                        .collect();
                    let mut coordinates = vec![0; table.rows.len() + 1];
                    for c in &table.cells {
                        if !c.gap {
                            coordinates[c.position] = c.lo;
                        }
                    }
                    let mut stops = vec![table.total; table.rows.len() + 1];
                    for i in (0..table.rows.len()).rev() {
                        stops[i] = if matches[i] {
                            stops[i + 1]
                        } else {
                            coordinates[i]
                        };
                    }
                    let stop = table
                        .cells
                        .iter()
                        .map(|c| stops[c.position + usize::from(c.gap)])
                        .collect();
                    let mut next = None;
                    let mut unknown = vec![None; table.cells.len()];
                    for i in (0..table.cells.len()).rev() {
                        if table.cells[i].unknown {
                            next = Some(i);
                        }
                        unknown[i] = next;
                    }
                    let known = outcome.known.as_ref().map(|s| {
                        let mut lines: Vec<_> = s.lines().map(str::to_owned).collect();
                        if reverse {
                            lines.reverse();
                        }
                        lines
                    });
                    Rule {
                        min: outcome.lines.min,
                        max: outcome.lines.max.unwrap_or(usize::MAX),
                        start,
                        end,
                        stop,
                        unknown,
                        known,
                        empty_known: shapes[index].conditional || outcome.blank_only,
                        mode: every.map(|p| p.regex.regex_str().to_owned()),
                        edge_start,
                        edge_end,
                    }
                })
                .collect()
        })
        .collect()
}

/// Difference-array expansion visits visible line positions once, rather than
/// enumerating all pairs of starts and ends. Gap offsets are never expanded.
struct Expansion<'a> {
    table: &'a Table<'a>,
    changes: Vec<i64>,
    partial: Domain,
    cursors: [(usize, usize); 2],
}

impl<'a> Expansion<'a> {
    fn new(table: &'a Table<'a>) -> Self {
        Self {
            table,
            changes: vec![0; table.cells.len() + 1],
            partial: vec![None; table.cells.len()],
            cursors: [(0, 0); 2],
        }
    }

    fn add(&mut self, lo: usize, hi: usize, first: usize, stream: usize) {
        if lo > hi {
            return;
        }
        let cells = &self.table.cells;
        let (mut a, mut b) = self.cursors[stream];
        while a < cells.len() && cells[a].hi < lo {
            a += 1;
        }
        a = a.max(first);
        while b < cells.len() && cells[b].lo <= hi {
            b += 1;
        }
        self.cursors[stream] = (a, b);
        if a >= b {
            return;
        }
        let mut start = a;
        let mut end = b;
        if cells[a].lo < lo {
            include(&mut self.partial[a], lo, cells[a].hi.min(hi));
            start += 1;
        }
        if end > start && cells[b - 1].hi > hi {
            include(&mut self.partial[b - 1], cells[b - 1].lo.max(lo), hi);
            end -= 1;
        }
        if start < end {
            self.changes[start] += 1;
            self.changes[end] -= 1;
        }
    }

    fn finish(mut self) -> Domain {
        let mut active = 0;
        for (i, cell) in self.table.cells.iter().enumerate() {
            active += self.changes[i];
            if active > 0 {
                include(&mut self.partial[i], cell.lo, cell.hi);
            }
        }
        self.partial
    }
}

impl Rule {
    fn expand(&self, table: &Table<'_>, input: &Domain, min: usize, max: usize) -> Domain {
        let mut expansion = Expansion::new(table);
        if min > max {
            return expansion.finish();
        }
        for (i, value) in input.iter().enumerate() {
            let Some((lo, hi)) = *value else {
                continue;
            };
            let stop = self.stop[i];
            if let Some(lower) = lo.checked_add(min) {
                expansion.add(lower, hi.saturating_add(max).min(stop), i, 0);
            }
            if let Some(gap) = self.unknown[i] {
                let coordinate = table.cells[gap].lo;
                if coordinate <= stop && coordinate <= hi.saturating_add(max) {
                    expansion.add(coordinate.max(lo), hi.saturating_add(max).min(stop), gap, 1);
                }
            }
        }
        expansion.finish()
    }

    fn apply(&self, table: &Table<'_>, input: &Domain, nonempty: bool) -> Domain {
        if !nonempty {
            return if self.min == 0
                && (self.empty_known || self.known.as_ref().is_none_or(Vec::is_empty))
            {
                input.clone()
            } else {
                vec![None; input.len()]
            };
        }
        if self.known.as_ref().is_some_and(Vec::is_empty) {
            return vec![None; input.len()];
        }
        let mut starts = input.clone();
        for (value, allowed) in starts.iter_mut().zip(&self.start) {
            if !allowed {
                *value = None;
            }
        }
        let mut result = if let Some(known) = &self.known {
            if known.len() < self.min || known.len() > self.max {
                return vec![None; input.len()];
            }
            let mut current = starts;
            for expected in known {
                current = self.expand(table, &current, 1, 1);
                for (cell, value) in table.cells.iter().zip(&mut current) {
                    if value.is_some()
                        && !cell.gap
                        && cell.position > 0
                        && table.previous[cell.position].is_some_and(|row| row != expected)
                    {
                        *value = None;
                    }
                }
                if empty(&current) {
                    break;
                }
            }
            current
        } else {
            self.expand(table, &starts, self.min.max(1), self.max)
        };
        for (value, allowed) in result.iter_mut().zip(&self.end) {
            if !allowed {
                *value = None;
            }
        }
        result
    }
}

// Each layer retains the selected outcome and whether its segment is empty.
// This keeps equal-every runs conditional on the outcomes of the complete path.
type Layer = Vec<Domain>;

fn union(target: &mut Domain, source: &Domain) {
    for (a, b) in target.iter_mut().zip(source) {
        if let Some((lo, hi)) = b {
            include(a, *lo, *hi);
        }
    }
}

fn join(domain: &Domain, left: Option<(&Rule, bool)>, right: Option<(&Rule, bool)>) -> Domain {
    let shared =
        matches!((left,right), (Some((a,_)),Some((b,_))) if a.mode.is_some() && a.mode == b.mode);
    domain
        .iter()
        .enumerate()
        .map(|(i, v)| {
            if shared
                || (left.is_none_or(|(r, nonempty)| !nonempty || r.edge_end[i])
                    && right.is_none_or(|(r, nonempty)| !nonempty || r.edge_start[i]))
            {
                *v
            } else {
                None
            }
        })
        .collect()
}

fn layers(table: &Table<'_>, rules: &[Vec<Rule>], masks: &[Domain]) -> Vec<Layer> {
    let mut result = vec![vec![masks[0].clone()]];
    for (i, group) in rules.iter().enumerate() {
        let mut next = Vec::new();
        for rule in group {
            for nonempty in [false, true] {
                let mut input = vec![None; table.cells.len()];
                for (previous, domain) in result[i].iter().enumerate() {
                    let left = i
                        .checked_sub(1)
                        .map(|j| (&rules[j][previous / 2], previous % 2 == 1));
                    union(&mut input, &join(domain, left, Some((rule, nonempty))));
                }
                next.push(intersect(
                    &rule.apply(table, &input, nonempty),
                    &masks[i + 1],
                ));
            }
        }
        result.push(next);
    }
    result
}

fn suffix(
    table: &Table<'_>,
    rules: &[Vec<Rule>],
    backward: &[Layer],
    cut: usize,
    left: Option<(&Rule, bool)>,
) -> Domain {
    let mut result = vec![None; table.cells.len()];
    for (index, domain) in backward[cut].iter().enumerate() {
        let right = rules.get(cut).map(|r| (&r[index / 2], index % 2 == 1));
        union(&mut result, &join(domain, left, right));
    }
    result
}
