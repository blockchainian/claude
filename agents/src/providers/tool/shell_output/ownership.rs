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
    shapes: &[OutputShape],
    solution: &Solution,
    lines: &[Line],
    text: &str,
) -> Ownership {
    assert_eq!(solution.cuts.0.len(), shapes.len() + 1);
    assert_eq!(solution.in_gap.len(), solution.cuts.0.len());
    assert_eq!(solution.outcomes.len(), shapes.len());

    let cuts = &solution.earliest;
    assert_eq!(cuts.len(), solution.cuts.0.len());
    assert_eq!(cuts.first(), Some(&0));
    assert_eq!(cuts.last(), Some(&lines.len()));

    let mut nonblank = vec![0; lines.len() + 1];
    for (index, line) in lines.iter().enumerate() {
        let blank = match line {
            Line::Text(range) => text[range.clone()].trim().is_empty(),
            Line::Gap { hidden } => *hidden == Some(0),
        };
        nonblank[index + 1] = nonblank[index] + usize::from(!blank);
    }
    let certain: Vec<_> = solution
        .cuts
        .0
        .iter()
        .zip(&solution.in_gap)
        .map(|(positions, in_gap)| {
            if *in_gap {
                return false;
            }
            let mut positions = positions.iter();
            let first = positions.next().expect("feasible cut cannot be empty");
            let last = positions.last().unwrap_or(first);
            nonblank[first] == nonblank[last]
        })
        .collect();
    let kinds: Vec<_> = shapes
        .iter()
        .zip(&solution.outcomes)
        .map(|(shape, outcomes)| {
            let mut outcomes = outcomes
                .iter()
                .filter(|&index| shape.outcomes[index].lines.max != Some(0));
            let first = &shape.outcomes[outcomes.next()?].kind;
            outcomes
                .all(|index| shape.outcomes[index].kind == *first)
                .then_some(first)
        })
        .collect();

    let mut output = Ownership(Vec::new());
    let mut start = 0;
    for end in 1..=shapes.len() {
        if !certain[end] && end < shapes.len() {
            continue;
        }
        let owner = if certain[start] && certain[end] {
            match kinds[start] {
                Some(_) if end == start + 1 => Owner::Command(start),
                Some(kind)
                    if kinds[start + 1..end]
                        .iter()
                        .all(|other| other.is_some_and(|other| super::same(kind, other))) =>
                {
                    Owner::Block(start..end)
                }
                _ => Owner::Plain,
            }
        } else {
            Owner::Plain
        };
        for (index, line) in lines.iter().enumerate().take(cuts[end]).skip(cuts[start]) {
            let owner = if matches!(line, Line::Gap { .. }) {
                Owner::Plain
            } else {
                owner.clone()
            };
            if let Some((range, previous)) = output.0.last_mut()
                && *previous == owner
                && range.end == index
            {
                range.end = index + 1;
            } else {
                output.0.push((index..index + 1, owner));
            }
        }
        start = end;
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::tool::shell_output::shape::{Kind, LineRange, Outcome};
    use crate::providers::tool::shell_output::solve::{BitSet, FeasibleCuts};

    fn file(path: &str) -> Kind {
        Kind::File {
            path: path.into(),
            numbered: false,
        }
    }

    fn shape(kinds: Vec<Kind>) -> OutputShape {
        OutputShape {
            outcomes: kinds
                .into_iter()
                .map(|kind| Outcome {
                    lines: LineRange { min: 0, max: None },
                    kind,
                    first: None,
                    last: None,
                    every: None,
                    known: None,
                    blank_only: false,
                })
                .collect(),
            conditional: false,
        }
    }

    fn set(indices: &[usize], len: usize) -> BitSet {
        let mut set = BitSet::new(len);
        for &index in indices {
            set.insert(index);
        }
        set
    }

    fn solution(
        shapes: &[OutputShape],
        cuts: &[&[usize]],
        len: usize,
        earliest: &[usize],
    ) -> Solution {
        Solution {
            cuts: FeasibleCuts(cuts.iter().map(|indices| set(indices, len + 1)).collect()),
            outcomes: shapes
                .iter()
                .map(|shape| {
                    set(
                        &(0..shape.outcomes.len()).collect::<Vec<_>>(),
                        shape.outcomes.len(),
                    )
                })
                .collect(),
            in_gap: vec![false; cuts.len()],
            earliest: earliest.to_vec(),
        }
    }

    fn lines(text: &str) -> Vec<Line> {
        let mut offset = 0;
        text.split_inclusive('\n')
            .map(|line| {
                let start = offset;
                offset += line.len();
                Line::Text(start..offset)
            })
            .collect()
    }

    #[test]
    fn unique_cuts_assign_commands() {
        // head -n 1 a.rs; head -n 1 b.py
        let text = "use std::io;\nimport os\n";
        let lines = lines(text);
        let shapes = [shape(vec![file("a.rs")]), shape(vec![file("b.py")])];
        let solution = solution(&shapes, &[&[0], &[1], &[2]], lines.len(), &[0, 1, 2]);
        assert_eq!(
            ownership(&shapes, &solution, &lines, text),
            Ownership(vec![(0..1, Owner::Command(0)), (1..2, Owner::Command(1)),])
        );
    }

    #[test]
    fn blank_floating_cuts_choose_earliest_complete_path() {
        // cat a.rs; printf '\n'; cat b.py
        let text = "use std::io;\n\n \t\r\nimport os\n";
        let lines = lines(text);
        let shapes = [
            shape(vec![file("a.rs")]),
            shape(vec![Kind::Plain]),
            shape(vec![file("b.py")]),
        ];
        let solution = solution(
            &shapes,
            &[&[0], &[1, 2, 3], &[2, 3], &[4]],
            lines.len(),
            &[0, 1, 2, 4],
        );
        assert_eq!(
            ownership(&shapes, &solution, &lines, text),
            Ownership(vec![
                (0..1, Owner::Command(0)),
                (1..2, Owner::Command(1)),
                (2..4, Owner::Command(2)),
            ])
        );
    }

    #[test]
    fn blank_floating_cuts_use_solver_complete_solution() {
        // head -n 2 a.rs; printf '\n\n\n' (or no output); head -n 3 b.py
        let text = "fn main() {}\n\n\n\nimport os\n";
        let lines = lines(text);
        let mut shapes = [
            shape(vec![file("a.rs")]),
            shape(vec![Kind::Plain, Kind::Plain]),
            shape(vec![file("b.py")]),
        ];
        shapes[0].outcomes[0].lines = LineRange {
            min: 1,
            max: Some(2),
        };
        shapes[1].outcomes[0].lines = LineRange {
            min: 0,
            max: Some(0),
        };
        shapes[1].outcomes[1].lines = LineRange {
            min: 3,
            max: Some(3),
        };
        shapes[2].outcomes[0].lines = LineRange {
            min: 1,
            max: Some(3),
        };
        let solution = solution(
            &shapes,
            &[&[0], &[1, 2], &[2, 4], &[5]],
            lines.len(),
            &[0, 1, 4, 5],
        );
        // The complete solutions are (0, 1, 4, 5) and (0, 2, 2, 5).
        // Choosing the cut minima would assign one line to a zero-or-three-line command.
        assert_eq!(
            ownership(&shapes, &solution, &lines, text),
            Ownership(vec![
                (0..1, Owner::Command(0)),
                (1..4, Owner::Command(1)),
                (4..5, Owner::Command(2)),
            ])
        );
    }

    #[test]
    fn adjacent_sed_outputs_with_same_language_form_block() {
        // sed -n '/fn /,$p' a.rs; sed -n '/fn /,$p' b.rs
        let text = "fn a() {}\nfn b() {}\nfn c() {}\n";
        let lines = lines(text);
        let shapes = [shape(vec![file("a.rs")]), shape(vec![file("b.rs")])];
        let solution = solution(&shapes, &[&[0], &[1, 2], &[3]], lines.len(), &[0, 1, 3]);
        assert_eq!(
            ownership(&shapes, &solution, &lines, text),
            Ownership(vec![(0..3, Owner::Block(0..2))])
        );
    }

    #[test]
    fn silent_failure_does_not_change_a_file_blocks_language() {
        let text = "fn a() {}\nfn b() {}\n";
        let lines = lines(text);
        let mut shapes = [
            shape(vec![file("a.rs"), Kind::Plain]),
            shape(vec![file("b.rs"), Kind::Plain]),
        ];
        for shape in &mut shapes {
            shape.outcomes[1].lines.max = Some(0);
        }
        let solution = solution(&shapes, &[&[0], &[0, 1, 2], &[2]], lines.len(), &[0, 0, 2]);
        assert_eq!(
            ownership(&shapes, &solution, &lines, text),
            Ownership(vec![(0..2, Owner::Block(0..2))])
        );
    }

    #[test]
    fn different_languages_and_numbering_do_not_form_blocks() {
        let text = "fn a() {}\nfn b() {}\nfn c() {}\n";
        let lines = lines(text);
        for kind in [
            file("b.py"),
            Kind::File {
                path: "b.rs".into(),
                numbered: true,
            },
            Kind::Diff,
            Kind::Plain,
        ] {
            let shapes = [shape(vec![file("a.rs")]), shape(vec![kind])];
            let solution = solution(&shapes, &[&[0], &[1, 2], &[3]], lines.len(), &[0, 1, 3]);
            assert_eq!(
                ownership(&shapes, &solution, &lines, text),
                Ownership(vec![(0..3, Owner::Plain)])
            );
        }
    }

    #[test]
    fn conflicting_feasible_outcomes_stay_plain() {
        // cat a.rs may print file contents or a diagnostic.
        let text = "cat: a.rs: No such file or directory\n";
        let lines = lines(text);
        let shapes = [shape(vec![file("a.rs"), Kind::Plain])];
        let mut solution = solution(&shapes, &[&[0], &[1]], lines.len(), &[0, 1]);
        assert_eq!(
            ownership(&shapes, &solution, &lines, text),
            Ownership(vec![(0..1, Owner::Plain)])
        );
        solution.outcomes[0] = set(&[0], 2);
        assert_eq!(
            ownership(&shapes, &solution, &lines, text),
            Ownership(vec![(0..1, Owner::Command(0))])
        );
    }

    #[test]
    fn gaps_stay_plain_and_ranges_cover_every_line_once() {
        let text = "use std::io;\nfn main() {}\nimport os\n";
        let mut lines = lines(text);
        lines.insert(1, Line::Gap { hidden: Some(20) });
        let shapes = [shape(vec![file("a.rs")]), shape(vec![file("b.py")])];
        let solution = solution(&shapes, &[&[0], &[3], &[4]], lines.len(), &[0, 3, 4]);
        let actual = ownership(&shapes, &solution, &lines, text);
        assert_eq!(
            actual,
            Ownership(vec![
                (0..1, Owner::Command(0)),
                (1..2, Owner::Plain),
                (2..3, Owner::Command(0)),
                (3..4, Owner::Command(1)),
            ])
        );
        assert_eq!(
            actual
                .0
                .iter()
                .flat_map(|(range, _)| range.clone())
                .collect::<Vec<_>>(),
            (0..lines.len()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn cut_inside_gap_is_uncertain_even_with_single_projection() {
        let text = "fn a() {}\nfn b() {}\n";
        let mut lines = lines(text);
        lines.insert(1, Line::Gap { hidden: None });
        let shapes = [shape(vec![file("a.rs")]), shape(vec![file("b.py")])];
        let mut solution = solution(&shapes, &[&[0], &[1], &[3]], lines.len(), &[0, 1, 3]);
        solution.in_gap[1] = true;
        assert_eq!(
            ownership(&shapes, &solution, &lines, text),
            Ownership(vec![(0..3, Owner::Plain)])
        );
    }

    #[test]
    fn empty_output_has_no_ranges() {
        let shapes = [shape(vec![Kind::Plain])];
        let solution = solution(&shapes, &[&[0], &[0]], 0, &[0, 0]);
        assert_eq!(ownership(&shapes, &solution, &[], ""), Ownership(vec![]));
    }
}
