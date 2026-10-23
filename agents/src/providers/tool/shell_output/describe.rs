#![allow(dead_code)]

use super::sed::sed;
use super::shape::{Kind, OutputShape};
use super::{Stream, shell_summary, syntax};
use std::sync::LazyLock;
use syntect::parsing::{Regex, Region};

#[derive(Clone, Copy)]
enum Count {
    Fixed(usize),
    Number,
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
    values: &'static [&'static str],
    required: &'static [&'static str],
    length: Length,
    lines: Option<&'static LazyLock<Regex>>,
    content: Content,
    no_match_exit: Option<i64>,
    filter: bool,
}
impl Row {
    fn options(
        mut self,
        values: &'static [&'static str],
        required: &'static [&'static str],
    ) -> Self {
        self.values = values;
        self.required = required;
        self
    }

    fn lines(mut self, regex: &'static LazyLock<Regex>) -> Self {
        self.lines = Some(regex);
        self
    }
}
static SEARCH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:(?:[^\s:]+:)?\d+[:-]|[^\s:]+-\d+-|--$)".into()));
// Require a path separator or a filename extension to avoid treating bare code
// identifiers and numeric output as paths. Unusual names remain unanchored.
static PATHS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:[^\s:/]+/)+[^\s:]+$|^[\w@+-]+\.[\w.@+-]+$".into()));
static STATUS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(?:[ MADRCU?!]{2} \S|## )".into()));
static NUMSTAT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(?:\d+|-)\t(?:\d+|-)\t".into()));
static LOG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[0-9a-f]{7,40} ".into()));

// Shapes match shell words separated by NUL. Capture 1 supplies the count or
// arguments; capture 2 supplies file names. Flags reject output-changing options;
// value-taking options consume their argument before checking further flags.
const DIFF_FLAGS: &[&str] = &[
    "--stat",
    "--numstat",
    "--shortstat",
    "--name-only",
    "--name-status",
    "--summary",
    "--dirstat",
    "--check",
    "--quiet",
    "-s",
    "--no-patch",
    "--oneline",
    "--format*",
    "--pretty*",
];

const DIFF_VALUES: &[&str] = &[
    "-U",
    "--unified",
    "-S",
    "-G",
    "--diff-filter",
    "--src-prefix",
    "--dst-prefix",
    "--line-prefix",
    "-n",
    "--max-count",
    "--author",
    "--grep",
];

static ROWS: LazyLock<Vec<Row>> = LazyLock::new(|| {
    use Content::*;
    use Count::*;
    use Length::*;
    let row = |shape: &str, flags, length, content, no_match_exit, filter| Row {
        shape: Regex::new(format!("(?s)\\A(?:{shape})\\z")),
        flags,
        values: &[],
        required: &[],
        length,
        lines: None,
        content,
        no_match_exit,
        filter,
    };
    vec![
        row(
            r"git\x00log\x00((?:[^\x00]+\x00)*--(?:format|pretty)=(?:t?format:)?%h [^\x00]*(?:\x00.*)?)",
            &["--graph", "--patch", "-p", "--stat*", "-z", "--numstat", "--name-only", "--name-status", "--shortstat"],
            Unknown,
            Plain,
            None,
            false,
        )
        .lines(&LOG),
        row(
            r"git\x00diff\x00((?:[^\x00]+\x00)*--numstat(?:\x00.*)?)",
            &["-z", "--stat*", "--patch", "-p", "--name-only", "--name-status", "--shortstat", "--summary"],
            Unknown,
            Plain,
            None,
            false,
        )
        .lines(&NUMSTAT),
        row(r"cd\x00([^\x00-][^\x00]*)", &[], Silent, Plain, None, false),
        row(
            r"mkdir\x00(.+)",
            &["-v", "--verbose", "--help", "--version"],
            Silent,
            Plain,
            None,
            false,
        )
        .options(&["-m", "--mode"], &[]),
        row(r"sleep\x00([0-9.]+)", &[], Silent, Plain, None, false),
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
            DIFF_FLAGS,
            Unknown,
            Diff,
            None,
            false,
        )
        .options(DIFF_VALUES, &[]),
        row(
            r"git\x00log\x00((?:-p|--patch)(?:\x00.*)?)",
            DIFF_FLAGS,
            Unknown,
            Diff,
            None,
            false,
        )
        .options(DIFF_VALUES, &[]),
        row(
            r"diff\x00-u\x00(.+)",
            DIFF_FLAGS,
            Unknown,
            Diff,
            None,
            false,
        )
        .options(DIFF_VALUES, &[]),
        row(
            r"rg\x00(.+)",
            &["-0", "--null", "--json", "--heading", "--pretty", "-p", "-n", "--line-number", "-c", "--count", "--count-matches", "-q", "--quiet"],
            Unknown, Plain, Some(1), false,
        )
        .lines(&PATHS)
        .options(&["-g", "--glob", "--iglob", "-t", "--type", "-e", "--regexp", "-f", "--file"],
                 &["--files", "-l", "--files-with-matches"]),
        // A plain rg filter selects complete input lines without changing shape.
        row(
            r"rg\x00((?:-[ivFsSwx]+\x00)*[^\x00-][^\x00]*)",
            &["-n", "--line-number", "-H", "--with-filename", "-l", "-c", "--count", "--json", "--heading", "--pretty", "-p", "-o", "--only-matching", "-r", "--replace", "-0", "--null", "--files"],
            Unknown, Inherit, Some(1), true,
        ),
        row(
            r"(?:rg|grep)\x00(.+)",
            &[
                "-l",
                "-L",
                "-c",
                "-q",
                "-p",
                "--files",
                "--files-with-matches",
                "--files-without-match",
                "--count",
                "--count-matches",
                "--json",
                "--heading",
                "--pretty",
                "--quiet",
            ],
            Unknown,
            Plain,
            Some(1),
            false,
        )
        .lines(&SEARCH)
        .options(
            &[
                "-A",
                "-B",
                "-C",
                "-e",
                "-f",
                "-g",
                "-t",
                "-T",
                "-m",
                "-M",
                "--after-context",
                "--before-context",
                "--context",
                "--include",
                "--exclude",
                "--exclude-dir",
                "--include-dir",
                "--glob",
                "--iglob",
                "--type",
                "--type-not",
                "--regexp",
                "--file",
                "--max-count",
                "--max-columns",
                "--color",
                "--colors",
            ],
            &["-n", "--line-number"],
        ),
        row(
            r"git\x00status\x00(.+)",
            &["-z", "--null", "--porcelain*", "--long"],
            Unknown,
            Plain,
            None,
            false,
        )
        .lines(&STATUS)
        .options(
            &["--untracked-files", "--ignore-submodules"],
            &["-s", "--short"],
        ),
        row(
            r"git\x00log\x00((?:[^\x00]+\x00)*--oneline(?:\x00.*)?)",
            &[
                "--format*",
                "--pretty*",
                "--graph",
                "--patch",
                "-p",
                "--stat*",
                "-z",
            ],
            Unknown,
            Plain,
            None,
            false,
        )
        .lines(&LOG)
        .options(
            &[
                "-n",
                "--max-count",
                "--author",
                "--grep",
                "--since",
                "--until",
            ],
            &[],
        ),
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
        row(r"cat", &[], Unknown, Verbatim, None, true),
    ]
});

#[derive(Clone)]
pub(super) enum Size {
    Silent,
    Exact(usize),
    AtMost(usize),
    Unknown,
}
#[derive(Clone)]
pub(super) struct Segment {
    pub(super) size: Size,
    pub(super) lines: Option<&'static LazyLock<Regex>>,
    pub(super) kind: Kind,
    pub(super) blank: bool,
    pub(super) known: Option<String>,
    pub(super) no_match_exit: Option<i64>,
    pub(super) beginning: bool,
    pub(super) cat: bool,
}
impl Segment {
    pub(super) fn unknown() -> Self {
        Self {
            size: Size::Unknown,
            lines: None,
            kind: Kind::Plain,
            blank: false,
            known: None,
            no_match_exit: None,
            beginning: false,
            cat: false,
        }
    }
}

fn allowed(args: &str, row: &Row) -> bool {
    let blocked = |option: &str| {
        row.flags.iter().any(|flag| {
            flag.strip_suffix('*')
                .map_or(option == *flag, |prefix| option.starts_with(prefix))
        })
    };
    let mut required = row.required.is_empty();
    let mut words = args.split('\0');
    while let Some(word) = words.next() {
        if word == "--" {
            break;
        }
        if !word.starts_with('-') || word == "-" {
            continue;
        }
        if word.starts_with("--") {
            let (option, value) = word
                .split_once('=')
                .map_or((word, None), |(k, v)| (k, Some(v)));
            if blocked(option) {
                return false;
            }
            required |= row.required.contains(&option);
            if value.is_none() && row.values.contains(&option) && words.next().is_none() {
                return false;
            }
        } else {
            for (index, letter) in word[1..].char_indices() {
                let option = format!("-{letter}");
                if blocked(&option) {
                    return false;
                }
                required |= row.required.contains(&option.as_str());
                if row.values.contains(&option.as_str()) {
                    if index + 1 + letter.len_utf8() == word.len() && words.next().is_none() {
                        return false;
                    }
                    break;
                }
            }
        }
    }
    required
}

fn count(rule: Count, arg: &str, files: &str) -> Option<(Option<usize>, Option<String>)> {
    let number = |s: &str| s.parse::<usize>().ok();
    Some(match rule {
        Count::Fixed(n) => (Some(n), None),
        Count::Number => (Some(number(arg)?), None),
        Count::Files => {
            let names: Vec<_> = files.split('\0').collect();
            if names.iter().any(|s| s.is_empty() || s.starts_with('-')) {
                return None;
            }
            (Some(names.len() + usize::from(names.len() > 1)), None)
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
            (Some(output.lines().count()), Some(output))
        }
        Count::Printf => {
            let mut words = arg.split('\0');
            let output = super::super::printf_separator(words.next()?, &words.collect::<Vec<_>>())?;
            if !output.is_empty() && !output.ends_with('\n') {
                return None;
            }
            (Some(output.lines().count()), Some(output))
        }
    })
}

fn describe(words: &[String], filter: bool) -> Option<(Segment, bool)> {
    if words.first().is_some_and(|w| w == "sed") {
        return sed(words).map(|s| (s, false));
    }
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
            || matches!(row.content, Content::Inherit)
            || matches!(row.length, Length::Silent))
            && !allowed(arg, row)
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
        let mut known = None;
        let size = match row.length {
            Length::Silent => Size::Silent,
            Length::Unknown => Size::Unknown,
            Length::Exact(c) | Length::AtMost(c) => {
                let Some((n, output)) = count(c, arg, files) else {
                    continue;
                };
                known = output;
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
                blank: known.as_ref().is_some_and(|s| s.chars().all(|c| c == '\n')),
                known,
                no_match_exit: row.no_match_exit,
                beginning: matches!(words[0].as_str(), "cat" | "head")
                    && files.split('\0').count() == 1,
                cat: words[0] == "cat" && files.split('\0').count() == 1,
            },
            matches!(row.content, Content::Verbatim),
        ));
    }
    None
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
        if quote != Some(b'\'')
            && b == b'$'
            && let Some(len) = shell_summary::address_expansion(&source[i..])
        {
            out.push_str(&source[i..i + len]);
            i += len;
            continue;
        }
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

pub(super) fn pipeline(source: &str, programs: &mut Vec<String>) -> Segment {
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
        if !matches!(filter.kind, Kind::Plain) {
            segment = filter;
            continue;
        }
        segment.cat = false;
        if !verbatim {
            segment.beginning &=
                stage.first().is_some_and(|word| word == "head") || filter.beginning;
        }
        if stage.first().is_some_and(|word| word == "sed") {
            if !matches!(segment.size, Size::Silent) {
                segment.size = filter.size;
            }
            segment.known = None;
            segment.no_match_exit = None;
            continue;
        }
        segment.size = match (&segment.size, filter.size) {
            (Size::Silent, _) => Size::Silent,
            (Size::AtMost(a) | Size::Exact(a), Size::AtMost(b)) => Size::AtMost((*a).min(b)),
            (_, Size::AtMost(b)) => Size::AtMost(b),
            (Size::Exact(n), Size::Unknown) if !verbatim => Size::AtMost(*n),
            (size, Size::Unknown) => size.clone(),
            _ => Size::Unknown,
        };
        // Only a verbatim filter preserves a producer's complete known output.
        if !verbatim {
            segment.known = None;
        }
        segment.no_match_exit = None;
    }
    segment
}

/// Describe command outputs for the stream; unsupported shell syntax returns None.
pub fn shapes(_command: &str, _stream: &Stream) -> Option<Vec<OutputShape>> {
    todo!()
}
