use super::{file_language, file_section, shell_summary, text_section};
use crate::event::Section;
use std::sync::LazyLock;
use syntect::parsing::{Regex, Region};

#[derive(Clone, Copy, Default)]
pub(super) struct Stream {
    pub merged_stderr: bool,
    pub trimmed: bool,
    pub exit_code: Option<i64>,
    pub is_error: bool,
}

#[derive(Clone, Copy)]
enum Count {
    Fixed(usize),
    Number,
    Range,
    Echo,
    Printf,
    Files,
}
#[derive(Clone, Copy)]
enum Length {
    Silent,
    Exact(Count),
    AtMost(Count),
    Unknown,
}
#[derive(Clone, Copy)]
enum Content {
    File,
    Numbered,
    Revision,
    Diff,
    Plain,
    Inherit,
    Verbatim,
}
struct Row {
    shape: Regex,
    flags: &'static [&'static str],
    length: Length,
    lines: Option<&'static LazyLock<Regex>>,
    content: Content,
    no_match_exit: Option<i64>,
    filter: bool,
}
impl Row {
    fn lines(mut self, regex: &'static LazyLock<Regex>) -> Self {
        self.lines = Some(regex);
        self
    }
}
static SEARCH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(?:[^\s:]+:)?\d+[:-]|^--$".into()));
static STATUS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(?:[ MADRCU?!]{2} \S|## )".into()));
static LOG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[0-9a-f]{7,40} ".into()));

// Shapes match shell words separated by NUL. Capture 1 supplies the count or
// arguments; capture 2 supplies file names. Flags are an exhaustive allowlist.
static ROWS: LazyLock<Vec<Row>> = LazyLock::new(|| {
    use Content::*;
    use Count::*;
    use Length::*;
    let row = |shape: &str, flags, length, content, no_match_exit, filter| Row {
        shape: Regex::new(format!("(?s)\\A(?:{shape})\\z")),
        flags,
        length,
        lines: None,
        content,
        no_match_exit,
        filter,
    };
    vec![
        row(r"cd\x00([^\x00-][^\x00]*)", &[], Silent, Plain, None, false),
        row(
            r"mkdir\x00(.+)",
            &["-p", "--parents", "--"],
            Silent,
            Plain,
            None,
            false,
        ),
        row(r"sleep\x00([0-9.]+)", &[], Silent, Plain, None, false),
        row(
            r"sed\x00-n\x00([0-9]+(?:,(?:[0-9]+|\$))?p|\$p)\x00([^\x00]+)",
            &[],
            AtMost(Range),
            File,
            None,
            false,
        ),
        row(
            r"(?:head|tail)\x00-n\x00([0-9]+)\x00([^\x00]+)",
            &[],
            AtMost(Number),
            File,
            None,
            false,
        ),
        row(
            r"(?:head|tail)\x00-n?([0-9]+)\x00([^\x00]+)",
            &[],
            AtMost(Number),
            File,
            None,
            false,
        ),
        row(
            r"tail\x00-n\x00(\+[0-9]+)\x00([^\x00]+)",
            &[],
            Unknown,
            File,
            None,
            false,
        ),
        row(
            r"(?:head|tail)\x00()([^\x00]+)",
            &[],
            AtMost(Fixed(10)),
            File,
            None,
            false,
        ),
        row(
            r"cat\x00-n\x00()([^\x00]+)",
            &[],
            Unknown,
            Numbered,
            None,
            false,
        ),
        row(
            r"nl\x00-ba\x00()([^\x00]+)",
            &[],
            Unknown,
            Numbered,
            None,
            false,
        ),
        row(r"cat\x00()(.+)", &[], Unknown, File, None, false),
        row(
            r"git\x00show\x00()([^\x00:]+:[^\x00]+)",
            &[],
            Unknown,
            Revision,
            None,
            false,
        ),
        row(
            r"git\x00(?:diff|show)(?:\x00([^:]*))?",
            &[
                "--",
                "--cached",
                "--staged",
                "--no-ext-diff",
                "--no-index",
                "--no-color",
                "--color=never",
                "-w",
                "--ignore-space-at-eol",
                "--ignore-all-space",
                "--no-renames",
                "--patch",
                "-p",
                "--unified=#",
                "-U#",
            ],
            Unknown,
            Diff,
            None,
            false,
        ),
        row(
            r"git\x00log\x00((?:-p|--patch)(?:\x00.*)?)",
            &["-p", "--patch", "-#", "-n#", "--", "--no-color"],
            Unknown,
            Diff,
            None,
            false,
        ),
        row(r"diff\x00-u\x00(.+)", &["--"], Unknown, Diff, None, false),
        row(
            r"(?:rg|grep)\x00((?:[^\x00]+\x00)*-n(?:\x00.*)?)",
            &[
                "-n",
                "-r",
                "-R",
                "-i",
                "-I",
                "-F",
                "-E",
                "-w",
                "-s",
                "-S",
                "--no-heading",
                "--line-number",
                "--hidden",
                "--no-ignore",
                "--",
                "-e=",
                "-g=",
                "--glob=",
                "--type=",
                "--max-count=#",
                "-m#",
            ],
            Unknown,
            Plain,
            Some(1),
            false,
        )
        .lines(&SEARCH),
        row(
            r"git\x00status\x00((?:-s|--short|-sb)(?:\x00.*)?)",
            &[
                "-s",
                "--short",
                "-sb",
                "--branch",
                "--",
                "--untracked-files=all",
                "--untracked-files=no",
            ],
            Unknown,
            Plain,
            None,
            false,
        )
        .lines(&STATUS),
        row(
            r"git\x00log\x00((?:[^\x00]+\x00)*--oneline(?:\x00.*)?)",
            &[
                "--oneline",
                "-#",
                "-n#",
                "--max-count=#",
                "--no-decorate",
                "--",
            ],
            Unknown,
            Plain,
            None,
            false,
        )
        .lines(&LOG),
        row(
            r"wc\x00(-[lcwcm])\x00(.+)",
            &[],
            Exact(Files),
            Plain,
            None,
            false,
        ),
        row(
            r"echo(?:\x00((?!-[neE]+(?:\x00|\z)).*))?",
            &[],
            Exact(Echo),
            Plain,
            None,
            false,
        ),
        row(r"printf\x00(.*)", &[], Exact(Printf), Plain, None, false),
        row(
            r"(?:head|tail)\x00-n\x00([0-9]+)",
            &[],
            AtMost(Number),
            Inherit,
            None,
            true,
        ),
        row(
            r"(?:head|tail)\x00-n?([0-9]+)",
            &[],
            AtMost(Number),
            Inherit,
            None,
            true,
        ),
        row(
            r"(?:head|tail)",
            &[],
            AtMost(Fixed(10)),
            Inherit,
            None,
            true,
        ),
        row(
            r"tail\x00-n\x00(\+[0-9]+)",
            &[],
            Unknown,
            Inherit,
            None,
            true,
        ),
        row(
            r"sed\x00-n\x00([0-9]+(?:,(?:[0-9]+|\$))?p|\$p)",
            &[],
            AtMost(Range),
            Inherit,
            None,
            true,
        ),
        row(r"cat", &[], Unknown, Verbatim, None, true),
    ]
});

#[derive(Clone, PartialEq, Eq)]
enum Kind {
    File { path: String, numbered: bool },
    Diff,
    Plain,
}
#[derive(Clone)]
enum Size {
    Silent,
    Exact(usize),
    AtMost(usize),
    Unknown,
}
#[derive(Clone)]
struct Segment {
    size: Size,
    lines: Option<&'static LazyLock<Regex>>,
    kind: Kind,
    blank: bool,
    no_match_exit: Option<i64>,
}
impl Segment {
    fn unknown() -> Self {
        Self {
            size: Size::Unknown,
            lines: None,
            kind: Kind::Plain,
            blank: false,
            no_match_exit: None,
        }
    }
}

fn allowed(args: &str, flags: &[&str]) -> bool {
    let mut words = args.split('\0');
    while let Some(word) = words.next() {
        if word == "--" {
            return flags.contains(&"--");
        }
        if !word.starts_with('-') {
            continue;
        }
        if !flags.iter().any(|flag| {
            if let Some(prefix) = flag.strip_suffix('#') {
                word.strip_prefix(prefix)
                    .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
            } else if let Some(prefix) = flag.strip_suffix('=') {
                word == prefix || word.starts_with(&format!("{prefix}="))
            } else {
                word == *flag
            }
        }) {
            return false;
        }
        if flags
            .iter()
            .any(|flag| flag.strip_suffix('=') == Some(word))
            && words.next().is_none()
        {
            return false;
        }
    }
    true
}

fn count(rule: Count, arg: &str, files: &str) -> Option<(Option<usize>, bool)> {
    let number = |s: &str| s.parse::<usize>().ok();
    Some(match rule {
        Count::Fixed(n) => (Some(n), false),
        Count::Number => (Some(number(arg)?), false),
        Count::Range => {
            let range = arg.strip_suffix('p')?;
            if range == "$" {
                (Some(1), false)
            } else if let Some((a, b)) = range.split_once(',') {
                let a = number(a)?.checked_sub(1)?;
                (
                    if b == "$" {
                        None
                    } else {
                        Some(number(b)?.checked_sub(a).filter(|n| *n > 0)?)
                    },
                    false,
                )
            } else {
                number(range)?.checked_sub(1)?;
                (Some(1), false)
            }
        }
        Count::Files => {
            let names: Vec<_> = files.split('\0').collect();
            if names.iter().any(|s| s.is_empty() || s.starts_with('-')) {
                return None;
            }
            (Some(names.len() + usize::from(names.len() > 1)), false)
        }
        Count::Echo => {
            let args: Vec<_> = if arg.is_empty() {
                vec![]
            } else {
                arg.split('\0').collect()
            };
            if args.iter().any(|s| s.contains('\\')) {
                return None;
            }
            let output = format!("{}\n", args.join(" "));
            (
                Some(output.lines().count()),
                output.chars().all(|c| c == '\n'),
            )
        }
        Count::Printf => {
            let mut words = arg.split('\0');
            let output = super::printf_separator(words.next()?, &words.collect::<Vec<_>>())?;
            if !output.is_empty() && !output.ends_with('\n') {
                return None;
            }
            (
                Some(output.lines().count()),
                output.chars().all(|c| c == '\n'),
            )
        }
    })
}

fn describe(words: &[String], filter: bool) -> Option<(Segment, bool)> {
    let joined = words.join("\0");
    for row in ROWS.iter().filter(|r| r.filter == filter) {
        let mut captures = Region::new();
        if !row
            .shape
            .search(&joined, 0, joined.len(), Some(&mut captures))
        {
            continue;
        }
        let capture = |n| captures.pos(n).map_or("", |(a, b)| &joined[a..b]);
        let arg = capture(1);
        let files = capture(2);
        if (matches!(row.content, Content::Diff)
            || row.lines.is_some()
            || matches!(row.length, Length::Silent))
            && !allowed(arg, row.flags)
        {
            continue;
        }
        let kind = match row.content {
            Content::File | Content::Numbered | Content::Revision => {
                let paths: Vec<_> = files.split('\0').collect();
                if paths.iter().any(|p| p.is_empty() || p.starts_with('-')) {
                    continue;
                }
                let path = if matches!(row.content, Content::Revision) {
                    files.split_once(':').unwrap().1
                } else {
                    paths[0]
                };
                let scope = syntax(path);
                if paths.len() > 1 && (scope.is_none() || paths.iter().any(|p| syntax(p) != scope))
                {
                    continue;
                }
                Kind::File {
                    path: path.into(),
                    numbered: matches!(row.content, Content::Numbered),
                }
            }
            Content::Diff => Kind::Diff,
            Content::Plain | Content::Inherit | Content::Verbatim => Kind::Plain,
        };
        let mut blank = false;
        let size = match row.length {
            Length::Silent => Size::Silent,
            Length::Unknown => Size::Unknown,
            Length::Exact(c) | Length::AtMost(c) => {
                let Some((n, b)) = count(c, arg, files) else {
                    continue;
                };
                blank = b;
                match (row.length, n) {
                    (Length::Exact(_), Some(n)) => Size::Exact(n),
                    (_, Some(n)) => Size::AtMost(n),
                    _ => Size::Unknown,
                }
            }
        };
        return Some((
            Segment {
                size,
                lines: row.lines,
                kind,
                blank,
                no_match_exit: row.no_match_exit,
            },
            matches!(row.content, Content::Verbatim),
        ));
    }
    None
}

fn syntax(path: &str) -> Option<syntect::parsing::Scope> {
    crate::ui::highlight::find_syntax(file_language(path)).map(|s| s.scope)
}

// Strip redirections before passing literal shell words to the shared pipeline
// parser. Quotes are preserved; expansions remain the parser's responsibility.
fn redirects(source: &str) -> Option<(String, Vec<usize>, bool, usize)> {
    let bytes = source.as_bytes();
    let mut out = String::new();
    let (mut i, mut quote, mut merged, mut stage) = (0, None, false, 0);
    let mut silent = Vec::new();
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'\\' && quote != Some(b'\'') {
            let end = i + 2;
            out.push_str(source.get(i..end)?);
            i = end;
            continue;
        }
        if matches!(b, b'\'' | b'"') && (quote.is_none() || quote == Some(b)) {
            quote = if quote.is_some() { None } else { Some(b) };
        }
        if quote.is_none() && b == b'|' {
            stage += 1;
        }
        if quote.is_none() && (b == b'>' || b == b'<') {
            if b == b'<' {
                return None;
            }
            let fd = if i > 0
                && bytes[i - 1].is_ascii_digit()
                && (i == 1 || bytes[i - 2].is_ascii_whitespace())
            {
                out.pop();
                bytes[i - 1] - b'0'
            } else {
                1
            };
            i += 1;
            if bytes.get(i) == Some(&b'>') {
                i += 1;
            }
            if bytes.get(i) == Some(&b'&') {
                i += 1;
                let target = *bytes.get(i)?;
                if fd == 2 && target == b'1' {
                    merged = true;
                } else {
                    return None;
                }
                i += 1;
            } else {
                while bytes.get(i).is_some_and(u8::is_ascii_whitespace) {
                    i += 1;
                }
                let start = i;
                let mut q = None;
                while let Some(&ch) = bytes.get(i) {
                    if q.is_none() && (ch.is_ascii_whitespace() || ch == b'|') {
                        break;
                    }
                    if matches!(ch, b'\'' | b'"') && (q.is_none() || q == Some(ch)) {
                        q = if q.is_some() { None } else { Some(ch) };
                    }
                    i += 1;
                }
                if start == i || q.is_some() {
                    return None;
                }
                if fd == 1 {
                    silent.push(stage);
                }
            }
            out.push(' ');
            continue;
        }
        let ch = source[i..].chars().next()?;
        out.push(ch);
        i += ch.len_utf8();
    }
    Some((out, silent, merged, stage))
}

fn pipeline(source: &str, programs: &mut Vec<String>) -> Segment {
    let Some((literal, silent, merged, last)) = redirects(source) else {
        return Segment::unknown();
    };
    if merged {
        return Segment::unknown();
    }
    if silent.contains(&last) {
        return Segment {
            size: Size::Silent,
            ..Segment::unknown()
        };
    }
    let Some(stages) = shell_summary::pipeline_words(&literal) else {
        return Segment::unknown();
    };
    for stage in &stages {
        if let Some(program) = stage
            .first()
            .and_then(|word| std::path::Path::new(word).file_name())
        {
            programs.push(program.to_string_lossy().into_owned());
        }
    }
    let mut stages = stages.iter();
    let Some(first) = stages.next() else {
        return Segment::unknown();
    };
    let mut segment = if silent.contains(&0) {
        Segment {
            size: Size::Silent,
            ..Segment::unknown()
        }
    } else {
        describe(first, false)
            .map(|(segment, _)| segment)
            .unwrap_or_else(Segment::unknown)
    };
    for (index, stage) in stages.enumerate() {
        if silent.contains(&(index + 1)) {
            segment = Segment {
                size: Size::Silent,
                ..Segment::unknown()
            };
            continue;
        }
        let Some((filter, verbatim)) = describe(stage, true) else {
            return Segment::unknown();
        };
        segment.size = match (&segment.size, filter.size) {
            (Size::Silent, _) => Size::Silent,
            (Size::AtMost(a) | Size::Exact(a), Size::AtMost(b)) => Size::AtMost((*a).min(b)),
            (_, Size::AtMost(b)) => Size::AtMost(b),
            (Size::Exact(n), Size::Unknown) if !verbatim => Size::AtMost(*n),
            (size, Size::Unknown) => size.clone(),
            _ => Size::Unknown,
        };
        segment.no_match_exit = None;
    }
    segment
}

fn section(label: &str, segment: Option<&Segment>, text: &str) -> Section {
    match segment.map(|s| &s.kind) {
        Some(Kind::Diff) => Section::Diff {
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
            let n = if forward {
                lines[start..end]
                    .iter()
                    .take_while(|l| regex.is_match(l))
                    .count()
            } else {
                lines[start..end]
                    .iter()
                    .rev()
                    .take_while(|l| regex.is_match(l))
                    .count()
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
        let diagnostic = stream.merged_stderr
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
    let (mut i, mut j, mut lo, mut hi) = (0, segments.len(), 0, lines.len());
    let front = markers.first().copied().unwrap_or(hi);
    let back = markers.last().map_or(0, |m| m + 1);
    let mut spans = Vec::new();
    while i < j {
        let Some(next) = boundary(&segments[i], &lines, lo, front.min(hi), true) else {
            break;
        };
        spans.push((lo, next, Some(i)));
        lo = next;
        i += 1;
    }
    while j > i {
        let Some(next) = boundary(&segments[j - 1], &lines, back.max(lo), hi, false) else {
            break;
        };
        spans.push((next, hi, Some(j - 1)));
        hi = next;
        j -= 1;
    }
    let chosen = block(&segments[i..j], hi - lo, truncated)
        .map(|s| segments.iter().position(|v| std::ptr::eq(s, v)).unwrap());
    let mut at = lo;
    for marker in markers.into_iter().filter(|m| *m >= lo && *m < hi) {
        spans.push((at, marker, chosen));
        spans.push((marker, marker + 1, None));
        at = marker + 1;
    }
    spans.push((at, hi, chosen));
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
