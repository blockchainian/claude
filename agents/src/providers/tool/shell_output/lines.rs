use super::Stream;
use std::ops::Range;

/// A visible line's original byte range, or a truncation gap with a hidden line count.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Line {
    Text(Range<usize>),
    Gap { hidden: Option<usize> },
}

/// Build a line table into text, excluding prefixes and recording truncation gaps.
pub fn lines(text: &str, stream: &Stream, programs: &[&str], known: &[&str]) -> Vec<Line> {
    let diagnostic = |line: &str| {
        !known.iter().any(|expected| {
            expected.trim_end_matches(['\n', '\r']) == line.trim_end_matches(['\n', '\r'])
        }) && stream.merged_stderr
            && programs.iter().any(|program| {
                let matches = |line: &str| {
                    if *program == "git" {
                        line.starts_with("fatal: ") || line.starts_with("error: ")
                    } else {
                        line.strip_prefix(program)
                            .is_some_and(|rest| rest.starts_with(": "))
                    }
                };
                matches(line) || line.split_once(": ").is_some_and(|(_, rest)| matches(rest))
            })
    };
    let mut prefix = 0;
    let mut total = None;
    for line in text.split_inclusive('\n') {
        if let Some(count) = line
            .strip_prefix("Total output lines: ")
            .and_then(|count| count.trim().parse::<usize>().ok())
        {
            total = Some(count);
        } else if !(line.starts_with("Warning: truncated output (original token count: ")
            || prefix > 0 && line.trim().is_empty())
        {
            break;
        }
        prefix += line.len();
    }

    let mut table = Vec::new();
    let mut offset = prefix;
    let mut visible = 0;
    let mut gaps = 0;
    for line in text[prefix..].split_inclusive('\n') {
        let end = offset + line.len();
        let marker = prefix > 0
            && line
                .split_once(" tokens truncated")
                .is_some_and(|(head, _)| {
                    head.trim_end_matches(|c: char| c.is_ascii_digit())
                        .ends_with('…')
                        && head.ends_with(|c: char| c.is_ascii_digit())
                });
        if diagnostic(line) {
            table.push(Line::Gap { hidden: Some(0) });
            visible += 1;
        } else if marker {
            table.push(Line::Gap { hidden: None });
            gaps += 1;
        } else {
            table.push(Line::Text(offset..end));
            visible += 1;
        }
        offset = end;
    }
    // A total cannot determine the individual sizes of multiple gaps.
    if gaps == 1 {
        let hidden = total.and_then(|count| count.checked_sub(visible));
        for line in &mut table {
            if let Line::Gap {
                hidden: count @ None,
            } = line
            {
                *count = hidden;
            }
        }
    }
    table
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merged_diagnostics_are_zero_length_gaps_except_known_failures() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/shell-output-boundaries.json"
        ))
        .unwrap();
        let text = fixture["8"]["output"].as_str().unwrap();
        let stream = Stream {
            merged_stderr: true,
            ..Stream::default()
        };
        let known = "cat: agents/.cargo/config.toml: No such file or directory\n";
        assert!(matches!(
            lines(text, &stream, &["cat"], &[])[61],
            Line::Gap { hidden: Some(0) }
        ));
        assert!(matches!(
            lines(text, &stream, &["cat"], &[known])[61],
            Line::Text(_)
        ));
        assert!(matches!(
            lines(text, &Stream::default(), &["cat"], &[])[61],
            Line::Text(_)
        ));
    }

    #[test]
    fn records_original_byte_ranges_including_unterminated_lines() {
        for (text, expected) in [
            ("", vec![]),
            ("fn a() {}\n", vec![Line::Text(0..10)]),
            (
                "a\nb\nc",
                vec![Line::Text(0..2), Line::Text(2..4), Line::Text(4..5)],
            ),
            (
                "中\r\n\n文",
                vec![Line::Text(0..5), Line::Text(5..6), Line::Text(6..9)],
            ),
        ] {
            assert_eq!(
                lines(text, &Stream::default(), &[], &[]),
                expected,
                "{text}"
            );
        }
    }

    #[test]
    fn excludes_prefixes_and_counts_only_visible_output() {
        for (prefix, hidden) in [
            ("Total output lines: 5\n\n", Some(2)),
            (
                "Warning: truncated output (original token count: 10000)\nTotal output lines: 50\n\n",
                Some(47),
            ),
            (
                "Warning: truncated output (original token count: 10000)\n\n",
                None,
            ),
            ("Total output lines: 2\n\n", None),
        ] {
            let text = format!("{prefix}a\nb\n…238 tokens truncated…\nc");
            let start = prefix.len();
            assert_eq!(
                lines(&text, &Stream::default(), &[], &[]),
                vec![
                    Line::Text(start..start + 2),
                    Line::Text(start + 2..start + 4),
                    Line::Gap { hidden },
                    Line::Text(text.len() - 1..text.len()),
                ]
            );
        }
    }

    #[test]
    fn retains_literal_markers_without_a_truncation_prefix() {
        for text in [
            "a\n…238 tokens truncated…\nb",
            "1:contains(\"…133 tokens truncated…\")\n2:next hit\nfn b() {}\n",
            "let text = \"…133 tokens truncated…\";\n",
            "a\n[... omitted end of long line]\nb",
            "Total output lines: invalid\na\n",
            "\na\n\n",
        ] {
            let table = lines(text, &Stream::default(), &[], &[]);
            let restored: String = table
                .iter()
                .map(|line| match line {
                    Line::Text(range) => &text[range.clone()],
                    Line::Gap { .. } => panic!("literal output became a gap"),
                })
                .collect();
            assert_eq!(restored, text);
        }
    }

    #[test]
    fn leaves_multiple_gap_lengths_unknown() {
        let text =
            "Total output lines: 100\n\na\n…238 tokens truncated…\nb\n…133 tokens truncated…\nc";
        let table = lines(text, &Stream::default(), &[], &[]);
        assert_eq!(table.len(), 5);
        assert_eq!(table[1], Line::Gap { hidden: None });
        assert_eq!(table[3], Line::Gap { hidden: None });
        let text = "Total output lines: 2\n\na\n…238 tokens truncated…\nb";
        assert_eq!(
            lines(text, &Stream::default(), &[], &[])[1],
            Line::Gap { hidden: Some(0) }
        );
    }

    #[test]
    fn prefix_without_output_and_marker_at_end() {
        assert!(lines("Total output lines: 0\n\n", &Stream::default(), &[], &[]).is_empty());
        assert_eq!(
            lines(
                "Total output lines: 7\n\n…238 tokens truncated…",
                &Stream::default(),
                &[],
                &[],
            ),
            vec![Line::Gap { hidden: Some(7) }]
        );
    }

    #[test]
    fn captured_boundary_output_keeps_every_byte() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/shell-output-boundaries.json"
        ))
        .unwrap();
        let text = fixture["2"]["output"].as_str().unwrap();
        assert!(text.starts_with("use super::super::render::ScreenLayout;\n"));
        let table = lines(text, &Stream::default(), &[], &[]);
        let Line::Text(first) = &table[0] else {
            panic!("expected captured source line")
        };
        assert_eq!(
            &text[first.clone()],
            "use super::super::render::ScreenLayout;\n"
        );
        let restored: String = table
            .iter()
            .map(|line| match line {
                Line::Text(range) => &text[range.clone()],
                Line::Gap { .. } => panic!("captured source became a gap"),
            })
            .collect();
        assert_eq!(restored, text);
    }
}
