mod describe;
mod lines;
mod ownership;
mod sed;
mod shape;
mod solve;
#[cfg(test)]
mod solve_tests;
#[cfg(test)]
mod tests;

use super::{file_language, file_section, shell_summary, text_section};
use crate::event::Section;
use describe::{Segment, Size, pipeline};
use shape::Kind;
#[cfg(test)]
use std::sync::LazyLock;
use syntect::parsing::Regex;

#[derive(Clone, Copy, Default)]
pub(super) struct Stream {
    pub merged_stderr: bool,
    pub trimmed: bool,
    pub exit_code: Option<i64>,
    pub is_error: bool,
}

fn syntax(path: &str) -> Option<syntect::parsing::Scope> {
    crate::ui::highlight::find_syntax(file_language(path)).map(|s| s.scope)
}

fn section(label: &str, segment: Option<&Segment>, text: &str) -> Section {
    match segment.map(|s| &s.kind) {
        Some(Kind::Diff) => Section::Diff {
            lang: String::new(),
            label: String::new(),
            text: text.into(),
        },
        Some(Kind::File { path, numbered }) if syntax(path).is_some() => {
            let mut section = file_section(label, path, text);
            if *numbered && let Section::Code { lang, .. } = &mut section {
                *lang = format!("numbered:{lang}");
            }
            section
        }
        _ => text_section(label, text),
    }
}

fn same(a: &Kind, b: &Kind) -> bool {
    match (a, b) {
        (Kind::Diff, Kind::Diff) => true,
        (
            Kind::File {
                path: a,
                numbered: x,
            },
            Kind::File {
                path: b,
                numbered: y,
            },
        ) => x == y && syntax(a).is_some() && syntax(a) == syntax(b),
        _ => false,
    }
}

fn boundary(
    segment: &Segment,
    lines: &[&str],
    start: usize,
    end: usize,
    forward: bool,
) -> Option<usize> {
    match segment.size {
        Size::Silent => Some(if forward { start } else { end }),
        Size::Exact(n) => {
            if forward {
                start.checked_add(n).filter(|n| *n <= end)
            } else {
                end.checked_sub(n).filter(|n| *n >= start)
            }
        }
        _ => {
            let regex = segment.lines?;
            let matches = |line: &&&str| {
                let line = line.strip_suffix('\n').unwrap_or(line);
                let line = line.strip_suffix('\r').unwrap_or(line);
                regex.is_match(line)
            };
            let n = if forward {
                lines[start..end].iter().take_while(matches).count()
            } else {
                lines[start..end].iter().rev().take_while(matches).count()
            };
            (n > 0).then_some(if forward { start + n } else { end - n })
        }
    }
}

fn block(segments: &[Segment], count: usize, truncated: bool) -> Option<&Segment> {
    let active: Vec<_> = segments
        .iter()
        .filter(|s| !matches!(s.size, Size::Silent | Size::Exact(0)))
        .collect();
    let first = *active.first()?;
    if !active.iter().all(|s| same(&first.kind, &s.kind)) {
        return None;
    }
    if !truncated {
        let cap = active.iter().try_fold(0usize, |sum, s| match s.size {
            Size::Exact(n) | Size::AtMost(n) => sum.checked_add(n),
            _ => None,
        });
        if cap.is_some_and(|cap| count > cap) {
            return None;
        }
    }
    Some(first)
}

type Span = (usize, usize, Option<usize>);

struct Splitter<'a> {
    segments: &'a [Segment],
    lines: &'a [&'a str],
    markers: &'a [usize],
    truncated: bool,
    spans: Vec<Span>,
}

impl Splitter<'_> {
    // A capped command without a line pattern is taken at its full cap only when
    // the commands after it confirm that boundary; `limit` is the far end of the
    // remaining commands (exclusive going forward, inclusive index going back).
    fn full(
        &self,
        k: usize,
        limit: usize,
        start: usize,
        end: usize,
        whole: usize,
        forward: bool,
    ) -> Option<usize> {
        let segment = &self.segments[k];
        let Size::AtMost(n) = segment.size else {
            return None;
        };
        if segment.lines.is_some() || n == 0 {
            return None;
        }
        let t = if forward {
            start.checked_add(n).filter(|t| *t <= end)?
        } else {
            end.checked_sub(n).filter(|t| *t >= start)?
        };
        let ok = if forward {
            self.starts(k + 1, limit, t, end, whole)
        } else {
            self.ends(k, limit, start, t, whole)
        };
        ok.then_some(t)
    }

    // Whether segment `k` (of `k..limit`) can start at line `t`.
    fn starts(&self, k: usize, limit: usize, t: usize, end: usize, whole: usize) -> bool {
        if k >= limit {
            return t == end && end == whole;
        }
        let s = &self.segments[k];
        match s.size {
            Size::Silent | Size::Exact(0) => return self.starts(k + 1, limit, t, end, whole),
            Size::Exact(m) => return t + m <= end && self.starts(k + 1, limit, t + m, end, whole),
            _ => {}
        }
        if t >= end {
            return false;
        }
        let line = |x: usize| self.lines[x].trim_end_matches('\n').trim_end_matches('\r');
        if let Some(regex) = s.lines {
            return regex.is_match(line(t)) && (t == 0 || !regex.is_match(line(t - 1)));
        }
        if let Some(known) = s.known.as_ref().filter(|_| !s.blank) {
            let expected: Vec<_> = known.split_inclusive('\n').collect();
            return !expected.is_empty()
                && t + expected.len() <= end
                && self.lines[t..t + expected.len()]
                    .iter()
                    .zip(&expected)
                    .all(|(a, b)| a.trim_end_matches('\n') == b.trim_end_matches('\n'));
        }
        if matches!(s.kind, Kind::Diff) {
            return line(t).starts_with("diff --git ");
        }
        if let Size::AtMost(m) = s.size {
            return t + m <= end && self.starts(k + 1, limit, t + m, end, whole);
        }
        false
    }

    // Whether segment `k` (scanning back to index `limit`) can end at line `t`.
    fn ends(&self, k: usize, limit: usize, start: usize, t: usize, whole: usize) -> bool {
        if k <= limit {
            return t == start && start == whole;
        }
        let s = &self.segments[k - 1];
        match s.size {
            Size::Silent | Size::Exact(0) => return self.ends(k - 1, limit, start, t, whole),
            Size::Exact(m) => {
                return t >= start + m && self.ends(k - 1, limit, start, t - m, whole);
            }
            _ => {}
        }
        if t <= start {
            return false;
        }
        let line = |x: usize| self.lines[x].trim_end_matches('\n').trim_end_matches('\r');
        if let Some(regex) = s.lines {
            return regex.is_match(line(t - 1))
                && (t == self.lines.len() || !regex.is_match(line(t)));
        }
        if let Some(known) = s.known.as_ref().filter(|_| !s.blank) {
            let expected: Vec<_> = known.split_inclusive('\n').collect();
            return !expected.is_empty()
                && t >= start + expected.len()
                && self.lines[t - expected.len()..t]
                    .iter()
                    .zip(&expected)
                    .all(|(a, b)| a.trim_end_matches('\n') == b.trim_end_matches('\n'));
        }
        if let Size::AtMost(m) = s.size {
            return t >= start + m && self.ends(k - 1, limit, start, t - m, whole);
        }
        false
    }

    fn split(&mut self, commands: std::ops::Range<usize>, output: std::ops::Range<usize>) {
        let (mut i, mut j, mut lo, mut hi) =
            (commands.start, commands.end, output.start, output.end);
        let markers: Vec<_> = self
            .markers
            .iter()
            .copied()
            .filter(|m| output.contains(m))
            .collect();
        let front = markers.first().copied().unwrap_or(hi);
        let back = markers.last().map_or(lo, |m| m + 1);
        while i < j {
            let Some(next) = boundary(&self.segments[i], self.lines, lo, front.min(hi), true)
                .or_else(|| self.full(i, j, lo, front.min(hi), hi, true))
            else {
                break;
            };
            self.spans.push((lo, next, Some(i)));
            lo = next;
            i += 1;
        }
        while j > i {
            let Some(next) = boundary(&self.segments[j - 1], self.lines, back.max(lo), hi, false)
                .or_else(|| self.full(j - 1, i, back.max(lo), hi, lo, false))
            else {
                break;
            };
            self.spans.push((next, hi, Some(j - 1)));
            hi = next;
            j -= 1;
        }

        // Only unique complete matches can anchor a command inside this block.
        // All anchors must agree with command order, including non-overlap.
        let mut anchors = Vec::new();
        for index in i..j {
            let segment = &self.segments[index];
            let Some(known) = segment.known.as_ref().filter(|_| !segment.blank) else {
                continue;
            };
            let expected: Vec<_> = known.split_inclusive('\n').collect();
            if expected.is_empty() || expected.len() > hi - lo {
                continue;
            }
            let mut matches = self.lines[lo..hi]
                .windows(expected.len())
                .enumerate()
                .filter(|(_, actual)| {
                    actual
                        .iter()
                        .zip(&expected)
                        .all(|(a, b)| a.trim_end_matches('\n') == b.trim_end_matches('\n'))
                });
            if let Some((offset, _)) = matches.next()
                && matches.next().is_none()
            {
                anchors.push((index, lo + offset, lo + offset + expected.len()));
            }
        }
        if !anchors.is_empty() && anchors.windows(2).all(|pair| pair[0].2 <= pair[1].1) {
            for (index, start, end) in anchors {
                self.split(i..index, lo..start);
                self.spans.push((start, end, Some(index)));
                i = index + 1;
                lo = end;
            }
            self.split(i..j, lo..hi);
            return;
        }

        // A unique longest maximal run supplies an interior format anchor.
        // Adjacent commands with the same format share one plain-text anchor.
        let mut formats = Vec::new();
        let mut index = i;
        while index < j {
            let first = index;
            index += 1;
            let Some(regex) = self.segments[first].lines else {
                continue;
            };
            while index < j
                && self.segments[index]
                    .lines
                    .is_some_and(|r| std::ptr::eq(r, regex))
            {
                index += 1;
            }
            let mut runs = Vec::new();
            let mut at = lo;
            while at < hi {
                let matches = |at: usize| {
                    !self.markers.contains(&at)
                        && regex.is_match(self.lines[at].trim_end_matches(['\n', '\r']))
                };
                if !matches(at) {
                    at += 1;
                    continue;
                }
                let start = at;
                while at < hi && matches(at) {
                    at += 1;
                }
                runs.push((start, at));
            }
            let longest = runs.iter().map(|(a, b)| b - a).max().unwrap_or(0);
            let mut best = runs.into_iter().filter(|(a, b)| b - a == longest);
            if let Some((start, end)) = best.next()
                && best.next().is_none()
                && (first != i || start == lo)
                && (index != j || end == hi)
            {
                formats.push((first, index, start, end));
            }
        }
        if !formats.is_empty() && formats.windows(2).all(|pair| pair[0].3 <= pair[1].2) {
            for (first, limit, start, end) in formats {
                self.split(i..first, lo..start);
                self.spans.push((start, end, Some(first)));
                i = limit;
                lo = end;
            }
            self.split(i..j, lo..hi);
            return;
        }

        // A language opening can bound a preceding block of a different language.
        let fits = |commands: std::ops::Range<usize>, count: usize| {
            let mut min = 0usize;
            let mut max = Some(0usize);
            for s in &self.segments[commands] {
                match s.size {
                    Size::Silent => {}
                    Size::Exact(n) => {
                        min = min.saturating_add(n);
                        max = max.and_then(|v| v.checked_add(n));
                    }
                    Size::AtMost(n) => max = max.and_then(|v| v.checked_add(n)),
                    Size::Unknown => max = None,
                }
            }
            count >= min && max.is_none_or(|max| count <= max)
        };
        let mut openings = Vec::new();
        for index in i + 1..j {
            let s = &self.segments[index];
            if !s.beginning || same(&s.kind, &self.segments[index - 1].kind) {
                continue;
            }
            let Kind::File { path, .. } = &s.kind else {
                continue;
            };
            let pattern = match file_language(path) {
                "py" => r"^(?:import |from \S+ import |#!)",
                "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" => r"^import ",
                "go" => r"^package ",
                "rs" => r"^(?:use |//!|#!\[)",
                _ => continue,
            };
            let regex = Regex::new(pattern.into());
            let mut candidates = (lo + 1..hi).filter(|&at| {
                regex.is_match(self.lines[at])
                    && fits(i..index, at - lo)
                    && fits(index..j, hi - at)
                    && !self.markers.contains(&at)
            });
            if let Some(start) = candidates.next()
                && candidates.next().is_none()
            {
                openings.push((index, start));
            }
        }
        if !openings.is_empty() && openings.windows(2).all(|pair| pair[0].1 < pair[1].1) {
            let (index, start) = openings[0];
            self.split(i..index, lo..start);
            self.split(index..j, start..hi);
            return;
        }

        let chosen = block(&self.segments[i..j], hi - lo, self.truncated).map(|s| {
            self.segments
                .iter()
                .position(|v| std::ptr::eq(s, v))
                .unwrap()
        });
        let mut at = lo;
        for marker in markers.into_iter().filter(|m| *m >= lo && *m < hi) {
            self.spans.push((at, marker, chosen));
            self.spans.push((marker, marker + 1, None));
            at = marker + 1;
        }
        self.spans.push((at, hi, chosen));
    }
}

// The shared splitter keeps OR lists together. Track their optional right-hand
// sides here; any asynchronous list invalidates the complete output stream.
fn alternatives(source: &str) -> Option<Vec<&str>> {
    let bytes = source.as_bytes();
    let (mut i, mut start, mut quote) = (0, 0, None);
    let mut parts = Vec::new();
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'\\' && quote != Some(b'\'') {
            i += 2;
            continue;
        }
        if matches!(b, b'\'' | b'"') && (quote.is_none() || quote == Some(b)) {
            quote = if quote.is_some() { None } else { Some(b) };
        } else if quote.is_none() && b == b'&' {
            // Descriptor duplication is consumed by the redirection parser.
            if i == 0 || bytes[i - 1] != b'>' {
                return None;
            }
        } else if quote.is_none() && b == b'|' && bytes.get(i + 1) == Some(&b'|') {
            parts.push(source[start..i].trim());
            i += 2;
            start = i;
            continue;
        }
        i += 1;
    }
    parts.push(source[start..].trim());
    (!parts.iter().any(|p| p.is_empty())).then_some(parts)
}

pub(super) fn sections(
    command: &str,
    label: &str,
    text: &str,
    stream: Stream,
) -> Option<Vec<Section>> {
    let commands = crate::ui::shell_commands::split(command)?;
    let mut segments = Vec::new();
    let mut programs = Vec::new();
    for c in commands {
        let source = &command[c.range];
        let alternatives = alternatives(source)?;
        for (index, source) in alternatives.into_iter().enumerate() {
            let mut segment = pipeline(source, &mut programs);
            if index > 0 {
                segment.size = Size::Unknown;
                segment.lines = None;
                segment.known = None;
            }
            segments.push(segment);
        }
    }
    if stream.merged_stderr && stream.exit_code.is_some_and(|n| n != 0) {
        let last = segments.last_mut()?;
        if last.no_match_exit == stream.exit_code {
            last.size = Size::Silent;
            last.kind = Kind::Plain;
        } else {
            return None;
        }
    }
    if !stream.merged_stderr && stream.is_error {
        for s in &mut segments {
            if matches!(s.size, Size::Silent | Size::Exact(_)) {
                s.size = Size::Unknown;
                s.known = None;
            }
        }
    }
    if stream.trimmed {
        for s in &mut segments {
            if s.blank && matches!(s.size, Size::Exact(_)) {
                s.size = Size::Silent;
            } else if !matches!(s.size, Size::Silent) {
                break;
            }
        }
        for s in segments.iter_mut().rev() {
            if s.blank && matches!(s.size, Size::Exact(_)) {
                s.size = Size::Silent;
            } else if !matches!(s.size, Size::Silent) {
                break;
            }
        }
    }
    let mut prefix = 0;
    for line in text.split_inclusive('\n') {
        if line.starts_with("Warning: truncated output (original token count: ")
            || line
                .strip_prefix("Total output lines: ")
                .is_some_and(|n| n.trim().parse::<usize>().is_ok())
            || (prefix > 0 && line.trim().is_empty())
        {
            prefix += line.len();
        } else {
            break;
        }
    }
    let body = &text[prefix..];
    // Merged stderr diagnostics keep their physical position, but do not
    // contribute to stdout counts or line-format boundaries.
    let mut lines = Vec::new();
    let mut ranges = Vec::new();
    let mut offset = prefix;
    for line in body.split_inclusive('\n') {
        let end = offset + line.len();
        let cat_error = stream.merged_stderr
            && segments.iter_mut().any(|segment| {
                let Kind::File { path, .. } = &segment.kind else {
                    return false;
                };
                if !segment.cat
                    || line.trim_end_matches(['\n', '\r'])
                        != format!("cat: {path}: No such file or directory")
                {
                    return false;
                }
                segment.kind = Kind::Plain;
                segment.size = Size::Exact(1);
                segment.known = Some(line.to_string());
                segment.beginning = false;
                true
            });
        let diagnostic = !cat_error
            && stream.merged_stderr
            && programs.iter().any(|program| {
                line.strip_prefix(program)
                    .is_some_and(|rest| rest.starts_with(':'))
            });
        if !diagnostic {
            lines.push(line);
            ranges.push(offset..end);
        }
        offset = end;
    }
    let markers: Vec<_> = lines
        .iter()
        .enumerate()
        .filter_map(|(i, l)| {
            (prefix > 0
                && l.split_once(" tokens truncated").is_some_and(|(p, _)| {
                    p.trim_end_matches(|c: char| c.is_ascii_digit())
                        .ends_with('…')
                        && p.ends_with(|c: char| c.is_ascii_digit())
                }))
            .then_some(i)
        })
        .collect();
    let truncated = !markers.is_empty() || prefix > 0;
    let mut splitter = Splitter {
        segments: &segments,
        lines: &lines,
        markers: &markers,
        truncated,
        spans: Vec::new(),
    };
    splitter.split(0..segments.len(), 0..lines.len());
    let mut spans = splitter.spans;
    spans.sort_by_key(|s| (s.0, s.1));
    let mut output = Vec::new();
    if prefix > 0 {
        output.push(text_section(label, &text[..prefix]));
    }
    let mut cursor = prefix;
    for (a, b, s) in spans {
        let mut index = a;
        while index < b {
            let start = ranges[index].start;
            if cursor < start {
                output.push(text_section(label, &text[cursor..start]));
            }
            let mut end = ranges[index].end;
            index += 1;
            while index < b && ranges[index].start == end {
                end = ranges[index].end;
                index += 1;
            }
            output.push(section(label, s.map(|s| &segments[s]), &text[start..end]));
            cursor = end;
        }
    }
    if cursor < text.len() {
        output.push(text_section(label, &text[cursor..]));
    }
    Some(output)
}

#[cfg(test)]
mod boundary_tests {
    use super::*;

    #[test]
    fn shell_output_boundary_matches_line_contents() {
        static FORMAT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^format$".into()));
        let segment = Segment {
            lines: Some(&FORMAT),
            ..Segment::unknown()
        };
        for line in ["format", "format\n", "format\r\n"] {
            for forward in [false, true] {
                assert_eq!(
                    boundary(&segment, &[line], 0, 1, forward),
                    Some(usize::from(forward)),
                    "{line:?}"
                );
            }
        }
        assert_eq!(boundary(&segment, &["format \n"], 0, 1, true), None);
    }
}

#[cfg(test)]
mod sed_tests {
    use super::tests::{Span, actual};
    use super::*;

    #[test]
    fn sed_row_selection_commands() {
        for (command, language) in [
            ("sed -n 1,140p src/sessions/mod.rs", "rs"),
            (
                "cd /x/agents/src/ui && sed -n '690,730p;790,815p;885,905p' render.rs",
                "rs",
            ),
            ("cat a.rs | sed -n '15,25p;125,140p;555,575p'", "rs"),
            ("sed -n '/## Install/,/^## /p' memory/agents.md", "md"),
            (r"sed -n '/#\[cfg(test)\]/q;p' src/rename/walker.rs", "rs"),
            (r#"sed -n "$((n-12)),$((n+12))p" claude.strings"#, "strings"),
            (r#"sed -n "${n}p" claude.strings"#, "strings"),
            ("sed -n '18,$p' log.diff", "diff"),
            ("sed -n 40p screen.ansi", "ansi"),
            ("sed -e '1,3p' -e '9p' -n a.rs", "rs"),
            ("sed '/^$/d' notes.md", "md"),
            ("sed -E -r -s -u -i -n --expression='1,3p' a.rs", "rs"),
            ("sed -ne '1,3p' a.rs", "rs"),
            ("sed -n '1~2p;2,+3p;3,~4p' a.rs", "rs"),
            (r"sed -n '\cfoo\ccIM!p' a.rs", "rs"),
            (r"sed -n '/[;/]/p;/foo;bar/Ip' a.rs", "rs"),
            (r"sed -n '/[[:alpha:]/]/p' a.rs", "rs"),
            (r"sed -n '/[]/]/p' a.rs", "rs"),
            (r#"sed -n "${n} p" a.rs"#, "rs"),
            (
                "sed -n '1{p;h;H;P;D;N;};b done;:done;t done;T done;w out;W out;q;Q;n;d' a.rs",
                "rs",
            ),
            ("sed -n '1p\n2p' a.rs b.rs", "rs"),
            (r#"sed -n "$n,3p" a.rs"#, "rs"),
        ] {
            // Unknown syntaxes still retain their extension in the parsed file kind.
            let stages = pipeline(command, &mut Vec::new());
            if matches!(language, "strings" | "ansi") {
                assert!(
                    matches!(&stages.kind, Kind::File { path, .. } if path.ends_with(language)),
                    "{command}"
                );
            } else {
                assert_eq!(
                    actual(command, "line\n", Stream::default()),
                    vec![Span {
                        start: 0,
                        end: 1,
                        language: language.into()
                    }],
                    "{command}"
                );
            }
        }
        for command in [
            "sed 's/ *$//' lr.out",
            "sed -n '1,5p;=' f.rs",
            "sed -f script.sed f.rs",
            "sed --file=script.sed f.rs",
            r"sed '1a\foo' f.rs",
            "sed -n '1p;s/x/y/' a.rs",
            "sed -n 'g' a.rs",
            "sed -n 'v' a.rs",
            "sed -n '1p' a.rs b.py",
        ] {
            assert!(
                actual(command, "line\n", Stream::default()).is_empty(),
                "{command}"
            );
        }
    }

    #[test]
    fn sed_row_selection_bounds() {
        let command = "sed -n '1,3p;60,90p' a.rs";
        assert!(matches!(
            pipeline(command, &mut Vec::new()).size,
            Size::AtMost(34)
        ));
        assert_eq!(
            actual(
                "head -n 1 a.rs | sed -n '1p;1p'",
                "a\na\n",
                Stream::default()
            ),
            vec![Span {
                start: 0,
                end: 2,
                language: "rs".into()
            }]
        );
        assert_eq!(
            actual("head -n 1 a.rs | sed 'p'", "a\na\n", Stream::default()),
            vec![Span {
                start: 0,
                end: 2,
                language: "rs".into()
            }]
        );
        for n in [1, 34, 35] {
            assert_eq!(
                !actual(command, &"line\n".repeat(n), Stream::default()).is_empty(),
                n <= 34
            );
        }
        for command in [
            "sed -n '18,$p' log.diff",
            "sed -n '$p' a.rs",
            "sed '1,3p' a.rs",
            "sed -n '1!p' a.rs",
            "sed -n '/foo/p' a.rs",
            "sed -n '1~2p' a.rs",
            "sed -n 'p' a.rs",
            "sed -n '1,3p;h' a.rs",
            "sed -n '1,3p' a.rs b.rs",
            r#"sed -n "${n}p" a.rs"#,
        ] {
            assert!(
                matches!(pipeline(command, &mut Vec::new()).size, Size::Unknown),
                "{command}"
            );
        }
    }
}

#[cfg(test)]
mod path_tests {
    use super::*;

    #[test]
    fn shell_output_real_rg_path_formats() {
        let cases: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/shell-output-boundaries.json"
        ))
        .unwrap();
        let paths: Vec<_> = cases["2"]["output"]
            .as_str()
            .unwrap()
            .lines()
            .skip(212)
            .take(5)
            .collect();
        for command in [
            "rg --files agents/scratchpad",
            "rg --files agents/scratchpad | rg '\\.(py|sh)$'",
            "rg -l needle agents/scratchpad",
        ] {
            let segment = pipeline(command, &mut Vec::new());
            let regex = segment.lines.expect("path listing format");
            assert!(paths.iter().all(|line| regex.is_match(line)), "{command}");
        }
    }
}
