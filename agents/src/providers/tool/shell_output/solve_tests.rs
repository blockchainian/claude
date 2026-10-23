use super::solve::BitSet;

#[test]
fn bitset_tracks_word_boundaries_in_order() {
    let mut set = BitSet::new(130);
    assert_eq!(set.iter().collect::<Vec<_>>(), Vec::<usize>::new());
    for index in [129, 64, 0, 63, 65, 64] {
        set.insert(index);
    }
    assert_eq!(set.iter().collect::<Vec<_>>(), vec![0, 63, 64, 65, 129]);
    for index in [0, 63, 64, 65, 129] {
        assert!(set.contains(index));
    }
    for index in [1, 62, 66, 128, 192] {
        assert!(!set.contains(index));
    }
}

#[test]
fn bitset_intersection_handles_empty_and_different_capacities() {
    let mut left = BitSet::new(130);
    let mut right = BitSet::new(65);
    for index in [0, 63, 64, 129] {
        left.insert(index);
    }
    for index in [1, 63, 64] {
        right.insert(index);
    }
    assert_eq!((&left & &right).iter().collect::<Vec<_>>(), vec![63, 64]);
    assert_eq!((&right & &left).iter().collect::<Vec<_>>(), vec![63, 64]);
    assert!((&left & &BitSet::new(0)).iter().next().is_none());
    assert!(!BitSet::new(0).contains(0));
}

use super::lines::Line;
use super::shape::{Kind, LineRange, Outcome, OutputShape, Pattern, Strength};
use super::solve::solve;
use syntect::parsing::Regex;

fn output(min: usize, max: Option<usize>) -> Outcome {
    Outcome {
        lines: LineRange { min, max },
        kind: Kind::Plain,
        first: None,
        last: None,
        every: None,
        known: None,
        blank_only: false,
    }
}

fn shape(outcomes: Vec<Outcome>) -> OutputShape {
    OutputShape {
        outcomes,
        conditional: false,
    }
}

fn pattern(regex: &str, strength: Strength) -> Pattern {
    Pattern {
        regex: Regex::new(regex.into()),
        strength,
    }
}

fn table(text: &str) -> Vec<Line> {
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
fn sed_code_then_rg_is_a_maximal_search_run() {
    let text =
        "fn main() {\n    run();\n}\nsrc/main.rs:2:    run();\nsrc/lib.rs:8:pub fn run() {}\n";
    let mut rg = output(1, None);
    rg.every = Some(pattern(r"^src/[^:]+:\d+:", Strength::Required));
    let result = solve(
        &[shape(vec![output(0, Some(20))]), shape(vec![rg])],
        &table(text),
        text,
    )
    .unwrap();
    assert_eq!(result.cuts.0[1].iter().collect::<Vec<_>>(), vec![3]);
}

#[test]
fn cat_error_union_retains_only_outcomes_in_complete_solutions() {
    let text = "cat: missing.rs: No such file or directory\nfinished\n";
    let mut error = output(1, Some(1));
    error.known = Some("cat: missing.rs: No such file or directory\n".into());
    let mut success = output(3, None);
    success.kind = Kind::File {
        path: "missing.rs".into(),
        numbered: false,
    };
    let mut echo = output(1, Some(1));
    echo.known = Some("finished\n".into());
    let result = solve(
        &[shape(vec![success, error]), shape(vec![echo])],
        &table(text),
        text,
    )
    .unwrap();
    assert_eq!(result.outcomes[0].iter().collect::<Vec<_>>(), vec![1]);
    assert_eq!(result.cuts.0[1].iter().collect::<Vec<_>>(), vec![1]);
}

#[test]
fn two_sed_ranges_keep_the_middle_cut_floating() {
    let text = "use std::io;\nfn first() {}\nfn second() {}\n";
    let result = solve(
        &[
            shape(vec![output(0, Some(3))]),
            shape(vec![output(0, Some(3))]),
        ],
        &table(text),
        text,
    )
    .unwrap();
    assert_eq!(
        result.cuts.0[1].iter().collect::<Vec<_>>(),
        vec![0, 1, 2, 3]
    );
}

#[test]
fn mismatched_sed_line_count_has_no_solution() {
    let text = "fn one() {}\nfn two() {}\nfn three() {}\n";
    assert!(solve(&[shape(vec![output(0, Some(2))])], &table(text), text).is_none());
}

#[test]
fn equal_every_patterns_share_a_maximal_run() {
    let text = "src/a.rs:1:a\nsrc/b.rs:2:b\nsrc/c.rs:3:c\n";
    let rg = || {
        let mut o = output(1, None);
        o.every = Some(pattern(r"^src/", Strength::Required));
        shape(vec![o])
    };
    let result = solve(&[rg(), rg()], &table(text), text).unwrap();
    assert_eq!(result.cuts.0[1].iter().collect::<Vec<_>>(), vec![1, 2]);
}

#[test]
fn required_edges_blank_only_and_empty_outputs() {
    let text = "diff --git a/a b/a\n+hello\nend\n\n";
    let mut diff = output(1, None);
    diff.first = Some(pattern("^diff --git", Strength::Required));
    diff.last = Some(pattern("^end$", Strength::Required));
    let mut blank = output(1, None);
    blank.blank_only = true;
    let result = solve(
        &[
            shape(vec![output(0, Some(0))]),
            shape(vec![diff]),
            shape(vec![blank]),
        ],
        &table(text),
        text,
    )
    .unwrap();
    assert_eq!(result.cuts.0[1].iter().collect::<Vec<_>>(), vec![0]);
    assert_eq!(result.cuts.0[2].iter().collect::<Vec<_>>(), vec![3]);
}

#[test]
fn opening_hint_fixes_only_a_unique_feasible_start() {
    let text = "heading\nuse std::io;\nuse std::fs;\nfn main() {}\n";
    let mut file = output(1, None);
    file.first = Some(pattern("^use ", Strength::Hint));
    let result = solve(
        &[shape(vec![output(0, None)]), shape(vec![file])],
        &table(text),
        text,
    )
    .unwrap();
    assert_eq!(result.cuts.0[1].iter().collect::<Vec<_>>(), vec![1]);
}

#[test]
fn hints_cannot_create_positions_or_discard_ambiguous_candidates() {
    let text = "use std::io;\nfn a() {}\nuse std::fs;\nfn b() {}\n";
    let mut file = output(1, None);
    file.first = Some(pattern("^use ", Strength::Hint));
    let result = solve(
        &[shape(vec![output(0, None)]), shape(vec![file])],
        &table(text),
        text,
    )
    .unwrap();
    assert_eq!(
        result.cuts.0[1].iter().collect::<Vec<_>>(),
        vec![0, 1, 2, 3]
    );
    let mut file = output(1, Some(1));
    file.first = Some(pattern("^use ", Strength::Hint));
    let result = solve(
        &[shape(vec![output(3, Some(3))]), shape(vec![file])],
        &table(text),
        text,
    )
    .unwrap();
    assert_eq!(result.cuts.0[1].iter().collect::<Vec<_>>(), vec![3]);
}

#[test]
fn truncation_hides_multiple_cuts_without_expanding_hidden_lines() {
    let text = "first\nlast\n";
    let lines = vec![
        Line::Text(0..6),
        Line::Gap {
            hidden: Some(1_000_000_000),
        },
        Line::Text(6..11),
    ];
    let shapes = [
        shape(vec![output(2, Some(2))]),
        shape(vec![output(999_999_998, Some(999_999_998))]),
        shape(vec![output(2, Some(2))]),
    ];
    let result = solve(&shapes, &lines, text).unwrap();
    assert!(result.in_gap[1] && result.in_gap[2]);
    assert!(result.cuts.0[1].contains(1) && result.cuts.0[2].contains(1));
    assert!(!result.in_gap[0] && !result.in_gap[3]);
    assert!(solve(&[shape(vec![output(1, Some(1))])], &lines, text).is_none());
}

#[test]
fn unknown_gap_can_hide_zero_or_many_lines_and_known_text() {
    let text = "first\nlast\n";
    let lines = vec![
        Line::Text(0..6),
        Line::Gap { hidden: None },
        Line::Text(6..11),
    ];
    for count in [2, 100_000_000] {
        assert!(solve(&[shape(vec![output(count, Some(count))])], &lines, text).is_some());
    }
    let mut known = output(3, Some(3));
    known.known = Some("first\nhidden\nlast\n".into());
    assert!(solve(&[shape(vec![known])], &lines, text).is_some());
}

#[test]
fn hundred_thousand_lines_do_not_enumerate_partitions() {
    let text = "src/main.rs:17:run();\n".repeat(100_000);
    let rg = || {
        let mut o = output(0, None);
        o.every = Some(pattern(r"^src/[^:]+:\d+:", Strength::Required));
        shape(vec![o])
    };
    let started = std::time::Instant::now();
    let result = solve(&[rg(), rg(), rg()], &table(&text), &text).unwrap();
    assert_eq!(result.cuts.0[1].iter().count(), 100_001);
    assert!(
        started.elapsed() < std::time::Duration::from_secs(15),
        "solver took {:?}",
        started.elapsed()
    );
}

#[test]
fn earliest_is_one_complete_solution_across_exact_length_commands() {
    let text = "one\ntwo\nthree\nfour\nfive\n";
    let result = solve(
        &[
            shape(vec![output(1, None)]),
            shape(vec![output(3, Some(3))]),
            shape(vec![output(0, None)]),
        ],
        &table(text),
        text,
    )
    .unwrap();
    assert_eq!(result.earliest, vec![0, 1, 4, 5]);
    assert_eq!(result.earliest[2] - result.earliest[1], 3);
}

#[test]
fn every_merging_depends_on_the_outcome_that_actually_participates() {
    let text = "src/a.rs:1:a\nsrc/b.rs:2:b\n";
    let mut first = output(1, Some(1));
    first.every = Some(pattern("^src/", Strength::Required));
    let mut impossible = output(3, Some(3));
    impossible.every = Some(pattern("^src/", Strength::Required));
    let ordinary = output(1, Some(1));
    assert!(
        solve(
            &[shape(vec![first]), shape(vec![impossible, ordinary])],
            &table(text),
            text
        )
        .is_none()
    );
}

#[test]
fn empty_command_list_only_accepts_empty_output() {
    assert!(solve(&[], &[], "").is_some());
    assert!(solve(&[], &table("output\n"), "output\n").is_none());
}

#[test]
fn known_text_rejects_a_different_line_at_the_same_length() {
    let text = "cat: other.rs: No such file or directory\n";
    let mut error = output(1, Some(1));
    error.known = Some("cat: missing.rs: No such file or directory\n".into());
    assert!(solve(&[shape(vec![error])], &table(text), text).is_none());
}

#[test]
fn every_does_not_check_across_a_truncation_gap() {
    let text = "src/a.rs:1:a\nsrc/b.rs:2:b\n";
    let lines = vec![
        Line::Text(0..13),
        Line::Gap { hidden: Some(0) },
        Line::Text(13..26),
    ];
    let mut rg = output(1, Some(1));
    rg.every = Some(pattern("^src/", Strength::Required));
    assert!(
        solve(
            &[shape(vec![output(1, Some(1))]), shape(vec![rg])],
            &lines,
            text
        )
        .is_some()
    );
}

#[test]
fn earliest_keeps_outcome_ties_until_the_next_cut() {
    let text = "src/a.rs:1:a\nsrc/b.rs:2:b\nsrc/c.rs:3:c\nsrc/d.rs:4:d\n";
    let rg = |min, max| {
        let mut o = output(min, max);
        o.every = Some(pattern("^src/", Strength::Required));
        o
    };
    let shapes = [
        shape(vec![output(1, Some(1)), rg(1, Some(1))]),
        shape(vec![output(2, Some(2)), rg(1, Some(1))]),
        shape(vec![output(1, None), rg(1, None)]),
    ];
    let result = solve(&shapes, &table(text), text).unwrap();
    assert_eq!(result.earliest, vec![0, 1, 2, 4]);
}

#[test]
fn each_unique_hint_is_propagated_before_considering_the_next() {
    let text = "header\nuse std::io;\n";
    let mut middle = output(1, Some(1));
    middle.first = Some(pattern("^use ", Strength::Hint));
    let mut last = output(0, None);
    last.first = Some(pattern("^use ", Strength::Hint));
    let result = solve(
        &[
            shape(vec![output(0, None)]),
            shape(vec![middle]),
            shape(vec![last]),
        ],
        &table(text),
        text,
    )
    .unwrap();
    assert_eq!(result.earliest, vec![0, 1, 2, 2]);
}

#[test]
fn required_every_hints_prefer_nonempty_matching_output() {
    let text = "export const value = 1;\n72:const foo = 1;\n";
    let mut matches = output(0, Some(25));
    matches.every = Some(pattern(r"^\d+:", Strength::Required));
    let shapes = [
        shape(vec![output(0, Some(51))]),
        shape(vec![matches, output(0, Some(0))]),
    ];
    let solved = solve(&shapes, &table(text), text).unwrap();
    assert_eq!(solved.cuts.0[1].iter().collect::<Vec<_>>(), vec![1]);
}

#[test]
fn conflicting_unique_hints_do_not_fix_a_cut() {
    let text = "end\nmiddle\nstart\n";
    let mut left = output(0, None);
    left.last = Some(pattern("^end$", Strength::Hint));
    let mut right = output(0, None);
    right.first = Some(pattern("^start$", Strength::Hint));
    let solved = solve(&[shape(vec![left]), shape(vec![right])], &table(text), text).unwrap();
    assert_eq!(
        solved.cuts.0[1].iter().collect::<Vec<_>>(),
        vec![0, 1, 2, 3]
    );
}

#[test]
fn known_text_is_checked_across_a_zero_length_gap() {
    let text = "code\n";
    let mut expected = output(1, Some(1));
    expected.known = Some("error\n".into());
    let mut lines = table(text);
    lines.push(Line::Gap { hidden: Some(0) });
    assert!(solve(&[shape(vec![expected])], &lines, text).is_none());
}
