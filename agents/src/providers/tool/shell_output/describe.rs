#![allow(dead_code)]

use super::sed::description as sed_description;
use super::shape::{Kind, LineRange, Outcome, OutputShape, Pattern, Strength};
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
    range: LineRange,
    count: Option<Count>,
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
static COUNTS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*\d+(\s+\d+)*(\s+\S.*)?$".into()));
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
    let silent = (SILENT, None);
    let unknown = (UNKNOWN, None);
    let exact = |count| {
        (
            LineRange {
                min: 1,
                max: Some(1),
            },
            Some(count),
        )
    };
    let at_most = |count| {
        (
            LineRange {
                min: 0,
                max: Some(1),
            },
            Some(count),
        )
    };
    let row = |shape: &str, flags, (range, count), content, no_match_exit, filter| Row {
        shape: Regex::new(format!("(?s)\\A(?:{shape})\\z")),
        flags,
        values: &[],
        required: &[],
        range,
        count,
        lines: None,
        content,
        no_match_exit,
        filter,
    };
    vec![
        row(
            r"git\x00log\x00((?:[^\x00]+\x00)*--(?:format|pretty)=(?:t?format:)?%h [^\x00]*(?:\x00.*)?)",
            &["--graph", "--patch", "-p", "--stat*", "-z", "--numstat", "--name-only", "--name-status", "--shortstat"],
            unknown,
            Plain,
            None,
            false,
        )
        .lines(&LOG),
        row(
            r"git\x00diff\x00((?:[^\x00]+\x00)*--numstat(?:\x00.*)?)",
            &["-z", "--stat*", "--patch", "-p", "--name-only", "--name-status", "--shortstat", "--summary"],
            unknown,
            Plain,
            None,
            false,
        )
        .lines(&NUMSTAT),
        row(r"cd\x00([^\x00-][^\x00]*)", &[], silent, Plain, None, false),
        row(
            r"mkdir\x00(.+)",
            &["-v", "--verbose", "--help", "--version"],
            silent,
            Plain,
            None,
            false,
        )
        .options(&["-m", "--mode"], &[]),
        row(r"sleep\x00([0-9.]+)", &[], silent, Plain, None, false),
        row(
            r"(?:head|tail)\x00-n\x00([0-9]+)\x00([^\x00]+)",
            &[],
            at_most(Number),
            File,
            None,
            false,
        ),
        row(
            r"(?:head|tail)\x00-n?([0-9]+)\x00([^\x00]+)",
            &[],
            at_most(Number),
            File,
            None,
            false,
        ),
        row(
            r"tail\x00-n\x00(\+[0-9]+)\x00([^\x00]+)",
            &[],
            unknown,
            File,
            None,
            false,
        ),
        row(
            r"(?:head|tail)\x00()([^\x00]+)",
            &[],
            at_most(Fixed(10)),
            File,
            None,
            false,
        ),
        row(
            r"cat\x00-n\x00()([^\x00]+)",
            &[],
            unknown,
            Numbered,
            None,
            false,
        ),
        row(
            r"nl\x00-ba\x00()([^\x00]+)",
            &[],
            unknown,
            Numbered,
            None,
            false,
        ),
        row(r"cat\x00()(.+)", &[], unknown, File, None, false),
        row(
            r"git\x00show\x00()([^\x00:]+:[^\x00]+)",
            &[],
            unknown,
            Revision,
            None,
            false,
        ),
        row(
            r"git\x00(?:diff|show)(?:\x00([^:]*))?",
            DIFF_FLAGS,
            unknown,
            Diff,
            None,
            false,
        )
        .options(DIFF_VALUES, &[]),
        row(
            r"git\x00log\x00((?:-p|--patch)(?:\x00.*)?)",
            DIFF_FLAGS,
            unknown,
            Diff,
            None,
            false,
        )
        .options(DIFF_VALUES, &[]),
        row(
            r"diff\x00-u\x00(.+)",
            DIFF_FLAGS,
            unknown,
            Diff,
            None,
            false,
        )
        .options(DIFF_VALUES, &[]),
        row(
            r"rg\x00(.+)",
            &["-0", "--null", "--json", "--heading", "--pretty", "-p", "-n", "--line-number", "-c", "--count", "--count-matches", "-q", "--quiet"],
            unknown, Plain, Some(1), false,
        )
        .lines(&PATHS)
        .options(&["-g", "--glob", "--iglob", "-t", "--type", "-e", "--regexp", "-f", "--file"],
                 &["--files", "-l", "--files-with-matches"]),
        // A plain rg filter selects complete input lines without changing shape.
        row(
            r"rg\x00((?:-[ivFsSwx]+\x00)*[^\x00-][^\x00]*)",
            &["-n", "--line-number", "-H", "--with-filename", "-l", "-c", "--count", "--json", "--heading", "--pretty", "-p", "-o", "--only-matching", "-r", "--replace", "-0", "--null", "--files"],
            unknown, Inherit, Some(1), true,
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
            unknown,
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
            unknown,
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
            unknown,
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
            exact(Files),
            Plain,
            None,
            false,
        ).lines(&COUNTS),
        row(
            r"echo(?:\x00((?!-[neE]+(?:\x00|\z)).*))?",
            &[],
            exact(Echo),
            Plain,
            None,
            false,
        ),
        row(r"printf\x00(.*)", &[], exact(Printf), Plain, None, false),
        row(
            r"(?:head|tail)\x00-n\x00([0-9]+)",
            &[],
            at_most(Number),
            Inherit,
            None,
            true,
        ),
        row(
            r"(?:head|tail)\x00-n?([0-9]+)",
            &[],
            at_most(Number),
            Inherit,
            None,
            true,
        ),
        row(
            r"(?:head|tail)",
            &[],
            at_most(Fixed(10)),
            Inherit,
            None,
            true,
        ),
        row(
            r"tail\x00-n\x00(\+[0-9]+)",
            &[],
            unknown,
            Inherit,
            None,
            true,
        ),
        row(r"cat", &[], unknown, Verbatim, None, true),
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

const SILENT: LineRange = LineRange {
    min: 0,
    max: Some(0),
};
const UNKNOWN: LineRange = LineRange { min: 0, max: None };

/// Shared command facts for the constraint path and the Segment adapter.
pub(super) struct Description {
    pub(super) range: LineRange,
    pub(super) every: Option<&'static LazyLock<Regex>>,
    pub(super) kind: Kind,
    pub(super) blank: bool,
    pub(super) known: Option<String>,
    pub(super) no_match_exit: Option<i64>,
    pub(super) beginning: bool,
    pub(super) cat: bool,
}

impl Description {
    pub(super) fn unknown() -> Self {
        Self {
            range: UNKNOWN,
            every: None,
            kind: Kind::Plain,
            blank: false,
            known: None,
            no_match_exit: None,
            beginning: false,
            cat: false,
        }
    }

    fn outcome(self) -> Outcome {
        let first = if self.beginning && matches!(self.kind, Kind::Diff) {
            Some(Pattern {
                regex: Regex::new("^diff --git ".into()),
                strength: Strength::Required,
            })
        } else if self.beginning {
            opening(&self.kind)
        } else {
            None
        };
        Outcome {
            lines: self.range,
            kind: self.kind,
            first,
            last: None,
            every: self.every.map(|regex| Pattern {
                regex: (**regex).clone(),
                strength: Strength::Required,
            }),
            known: self.known,
            blank_only: self.blank,
        }
    }

    fn failure(&self) -> Failure {
        if self.cat {
            let Kind::File { path, .. } = &self.kind else {
                unreachable!("cat file descriptions have a file kind");
            };
            Failure::Cat(path.clone())
        } else if self.no_match_exit.is_some() {
            Failure::NoMatch
        } else {
            Failure::Unknown
        }
    }
}

impl From<Description> for Segment {
    fn from(description: Description) -> Self {
        let size = match description.range {
            LineRange { max: Some(0), .. } => Size::Silent,
            LineRange {
                min,
                max: Some(max),
            } if min == max => Size::Exact(max),
            LineRange { max: Some(max), .. } => Size::AtMost(max),
            LineRange { max: None, .. } => Size::Unknown,
        };
        Self {
            size,
            lines: description.every,
            kind: description.kind,
            blank: description.blank,
            known: description.known,
            no_match_exit: description.no_match_exit,
            beginning: description.beginning,
            cat: description.cat,
        }
    }
}

fn opening(kind: &Kind) -> Option<Pattern> {
    let Kind::File { path, numbered } = kind else {
        return None;
    };
    let extension = std::path::Path::new(path).extension()?.to_str()?;
    let pattern = match extension {
        "py" => r"(?:import |from \S+ import |#!)",
        "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" => r"import ",
        "go" => r"package ",
        "rs" => r"(?:use |//!|#!\[)",
        _ => return None,
    };
    let prefix = if *numbered { r"^\s*1\s+" } else { "^" };
    Some(Pattern {
        regex: Regex::new(format!("{prefix}{pattern}")),
        strength: Strength::Hint,
    })
}

/// A command failure, before stream and exit information are applied.
enum Failure {
    Unknown,
    Cat(String),
    NoMatch,
}

impl Failure {
    fn outcome(self, _stream: &Stream) -> Outcome {
        let mut output = Description::unknown();
        match self {
            Self::Unknown => {}
            Self::NoMatch => output.range = SILENT,
            Self::Cat(path) if _stream.merged_stderr => {
                output.range = LineRange {
                    min: 1,
                    max: Some(1),
                };
                output.known = Some(format!("cat: {path}: No such file or directory\n"));
            }
            Self::Cat(_) => output.range = SILENT,
        }
        output.outcome()
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

fn description(words: &[String], filter: bool) -> Option<(Description, bool)> {
    if words.first().is_some_and(|w| w == "sed") {
        return sed_description(words).map(|s| (s, false));
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
            || row.range == SILENT)
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
        let range = if let Some(rule) = row.count {
            let Some((n, output)) = count(rule, arg, files) else {
                continue;
            };
            known = output;
            n.map_or(UNKNOWN, |n| LineRange {
                min: if row.range.min == 0 { 0 } else { n },
                max: Some(n),
            })
        } else {
            row.range
        };
        return Some((
            Description {
                range,
                every: row.lines,
                kind,
                blank: known.as_ref().is_some_and(|s| s.chars().all(|c| c == '\n')),
                known,
                no_match_exit: row.no_match_exit,
                beginning: (words.first().is_some_and(|word| word == "git")
                    && words.get(1).is_some_and(|word| word == "diff"))
                    || matches!(words[0].as_str(), "cat" | "head")
                        && files.split('\0').count() == 1,
                cat: words[0] == "cat" && !files.is_empty() && files.split('\0').count() == 1,
            },
            matches!(row.content, Content::Verbatim),
        ));
    }
    None
}

fn describe(words: &[String], filter: bool) -> Option<(Segment, bool)> {
    description(words, filter).map(|(output, verbatim)| (output.into(), verbatim))
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

struct Pipeline {
    output: Description,
    failure: Option<Failure>,
    upstream_failure: bool,
}

impl Pipeline {
    fn unknown() -> Self {
        Self {
            output: Description::unknown(),
            failure: None,
            upstream_failure: false,
        }
    }
}

fn pipeline_description(source: &str, programs: &mut Vec<String>) -> Pipeline {
    let Some((literal, silent, merged, last)) = redirects(source) else {
        return Pipeline::unknown();
    };
    if merged {
        return Pipeline::unknown();
    }
    if silent.contains(&last) {
        return Pipeline {
            output: Description {
                range: SILENT,
                ..Description::unknown()
            },
            failure: Some(Failure::Unknown),
            upstream_failure: last > 0,
        };
    }
    let Some(stages) = shell_summary::pipeline_words(&literal) else {
        return Pipeline::unknown();
    };
    for stage in &stages {
        if let Some(program) = stage
            .first()
            .and_then(|word| std::path::Path::new(word).file_name())
        {
            programs.push(program.to_string_lossy().into_owned());
        }
    }
    let upstream_failure = stages.len() > 1;
    let mut stages = stages.iter();
    let Some(first) = stages.next() else {
        return Pipeline::unknown();
    };
    let described = description(first, false);
    let mut failure = described
        .as_ref()
        .filter(|_| {
            !matches!(
                first.first().map(String::as_str),
                Some("echo" | "printf" | "pwd" | "true")
            )
        })
        .map(|(output, _)| output.failure());
    let mut output = if silent.contains(&0) {
        Description {
            range: SILENT,
            ..Description::unknown()
        }
    } else {
        described
            .map(|(output, _)| output)
            .unwrap_or_else(Description::unknown)
    };
    for (index, stage) in stages.enumerate() {
        if silent.contains(&(index + 1)) {
            output = Description {
                range: SILENT,
                ..Description::unknown()
            };
            failure = Some(Failure::Unknown);
            continue;
        }
        let Some((filter, verbatim)) = description(stage, true) else {
            return Pipeline::unknown();
        };
        failure = Some(filter.failure());
        if !matches!(filter.kind, Kind::Plain) {
            output = filter;
            continue;
        }
        output.cat = false;
        if !verbatim {
            output.beginning &= stage.first().is_some_and(|word| word == "head")
                || !matches!(output.kind, Kind::Diff) && filter.beginning;
        }
        if stage.first().is_some_and(|word| word == "sed") {
            if output.range != SILENT {
                output.range = filter.range;
            }
            output.known = None;
            output.no_match_exit = None;
            continue;
        }
        output.range = match (output.range, filter.range.max) {
            (range, _) if range == SILENT => SILENT,
            (LineRange { max: Some(a), .. }, Some(b)) => LineRange {
                min: 0,
                max: Some(a.min(b)),
            },
            (_, Some(b)) => LineRange {
                min: 0,
                max: Some(b),
            },
            (range, None) if !verbatim => LineRange { min: 0, ..range },
            (range, None) => range,
        };
        // Only a verbatim filter preserves a producer's complete known output.
        if !verbatim {
            output.known = None;
        }
        output.no_match_exit = None;
    }
    Pipeline {
        output,
        failure,
        upstream_failure,
    }
}

pub(super) fn pipeline(source: &str, programs: &mut Vec<String>) -> Segment {
    pipeline_description(source, programs).output.into()
}

fn pipeline_shape(source: &str, stream: &Stream, success: bool) -> OutputShape {
    let mut pipeline = pipeline_description(source, &mut Vec::new());
    let failure_outcome = |failure: Failure| {
        if matches!(failure, Failure::Cat(_)) {
            return failure.outcome(stream);
        }
        let mut output = Description::unknown();
        output.range = SILENT;
        output.outcome()
    };
    let no_match = matches!(pipeline.failure, Some(Failure::NoMatch));
    let mut outcomes = Vec::new();
    if !no_match || pipeline.output.range.max != Some(0) {
        if no_match {
            pipeline.output.range.min = 1;
        }
        outcomes.push(pipeline.output.outcome());
    }
    if pipeline.upstream_failure {
        outcomes.push(failure_outcome(Failure::Unknown));
    }
    if !success && let Some(failure) = pipeline.failure {
        outcomes.push(failure_outcome(failure));
    }
    OutputShape {
        outcomes,
        conditional: false,
    }
}

/// Describe command outputs for the stream; unsupported shell syntax returns None.
pub fn shapes(command: &str, stream: &Stream) -> Option<Vec<OutputShape>> {
    use crate::ui::shell_commands::{Separator, split_with_separators};

    let commands = split_with_separators(command)?;
    let mut groups: Vec<(std::ops::Range<usize>, Separator)> = Vec::new();
    for part in commands {
        if part.separator == Separator::Pipe {
            let (range, _) = groups.last_mut()?;
            range.end = part.range.end;
        } else {
            groups.push((part.range, part.separator));
        }
    }
    // Only the final && chain is known to have succeeded when the shell exits 0.
    let final_group = groups
        .iter()
        .rposition(|(_, separator)| *separator != Separator::And)
        .unwrap_or(0);
    let mut shapes: Vec<_> = groups
        .into_iter()
        .enumerate()
        .map(|(index, (range, separator))| {
            let mut shape = pipeline_shape(
                &command[range],
                stream,
                stream.exit_code == Some(0) && index >= final_group,
            );
            shape.conditional = separator == Separator::And;
            if shape.conditional && index < final_group {
                for outcome in &mut shape.outcomes {
                    outcome.lines.min = 0;
                }
            }
            shape
        })
        .collect();
    if stream.trimmed {
        let possible = |shape: &OutputShape| {
            shape
                .outcomes
                .iter()
                .any(|outcome| outcome.lines.max != Some(0))
        };
        let first = shapes.iter().position(possible);
        let last = shapes.iter().rposition(possible);
        for index in first.into_iter().chain(last) {
            for outcome in &mut shapes[index].outcomes {
                outcome.lines.min = 0;
            }
        }
    }
    Some(shapes)
}

#[cfg(test)]
mod shape_tests {
    use super::super::shape::{LineRange, Strength};
    use super::*;

    fn one(command: &str) -> OutputShape {
        pipeline_shape(command, &Stream::default(), false)
    }

    #[test]
    fn command_chains_group_pipes_and_filter_only_final_success_chain() {
        let stream = Stream {
            merged_stderr: true,
            exit_code: Some(0),
            ..Stream::default()
        };
        let result = shapes(
            "cat before.py; cat a.rs | head -n 2 && rg -n needle src",
            &stream,
        )
        .unwrap();
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].outcomes.len(), 2);
        assert!(!result[0].conditional);
        assert_eq!(
            result[1].outcomes.len(),
            2,
            "upstream pipeline failure survives"
        );
        assert!(!result[1].conditional);
        assert!(result[2].conditional);
        assert_eq!(result[2].outcomes.len(), 1);
        let result = shapes("cat a.py && cat b.rs", &stream).unwrap();
        assert!(result.iter().all(|shape| shape.outcomes.len() == 1));
        let result = shapes("cat a.py || cat b.rs", &stream).unwrap();
        assert_eq!(result[0].outcomes.len(), 2);
        assert_eq!(result[1].outcomes.len(), 1);
        assert!(!result[1].conditional);
        let failed = Stream {
            exit_code: Some(1),
            ..stream
        };
        let result = shapes("cat a.py && rg -n needle src", &failed).unwrap();
        assert!(result.iter().all(|shape| shape.outcomes.len() == 2));
        assert!(shapes("cat a.py & echo end", &stream).is_none());
    }

    #[test]
    fn conditional_commands_before_final_group_can_be_skipped() {
        let stream = Stream {
            merged_stderr: true,
            exit_code: Some(0),
            ..Stream::default()
        };
        let result = shapes(
            "echo first && echo second && cat missing.py; echo last",
            &stream,
        )
        .unwrap();
        assert_eq!(result[0].outcomes[0].lines.min, 1);
        assert_eq!(
            result[1].outcomes[0].lines,
            LineRange {
                min: 0,
                max: Some(1)
            }
        );
        assert!(result[1].conditional);
        assert!(result[2].conditional);
        assert!(result[2].outcomes.iter().all(|o| o.lines.min == 0));
        assert_eq!(result[2].outcomes[1].lines.max, Some(1));
        assert_eq!(result[3].outcomes[0].lines.min, 1);
    }

    #[test]
    fn trimming_relaxes_only_first_and_last_possible_outputs() {
        let stream = Stream {
            trimmed: true,
            exit_code: Some(0),
            ..Stream::default()
        };
        let result = shapes(
            "cd src && echo first && echo middle && echo last && sleep 1",
            &stream,
        )
        .unwrap();
        assert_eq!(result[0].outcomes[0].lines.max, Some(0));
        assert_eq!(result[1].outcomes[0].lines.min, 0);
        assert_eq!(result[2].outcomes[0].lines.min, 1);
        assert_eq!(result[3].outcomes[0].lines.min, 0);
        assert_eq!(result[4].outcomes[0].lines.max, Some(0));
        assert_eq!(result[1].outcomes[0].known.as_deref(), Some("first\n"));
        let result = shapes("printf '\\n'; echo middle; printf '\\n'", &stream).unwrap();
        assert!(result[0].outcomes[0].blank_only);
        assert!(result[2].outcomes[0].blank_only);
    }

    #[test]
    fn file_failure_cannot_consume_source_output() {
        let stream = Stream {
            merged_stderr: true,
            ..Stream::default()
        };
        for command in ["sed -n 1,2p a.rs", "head -n 2 a.rs", "cat a.rs | head -n 2"] {
            let shapes = shapes(command, &stream).unwrap();
            let text = "use std::io;\nfn main() {}\n";
            let lines = super::super::lines::lines(text, &stream, &[], &[]);
            let solution = super::super::solve::solve(&shapes, &lines, text).unwrap();
            assert!(
                solution.outcomes[0]
                    .iter()
                    .all(|index| matches!(shapes[0].outcomes[index].kind, Kind::File { .. })),
                "{command}"
            );
        }
    }

    #[test]
    fn literal_output_commands_have_no_failure_outcome() {
        for command in ["echo hello", "printf 'hello\\n'", "pwd", "true"] {
            assert_eq!(
                shapes(command, &Stream::default()).unwrap()[0]
                    .outcomes
                    .len(),
                1,
                "{command}"
            );
        }
    }

    #[test]
    fn command_classes_have_success_ranges_and_kinds() {
        for (command, min, max, file, diff) in [
            ("cd src", 0, Some(0), false, false),
            ("mkdir -p build", 0, Some(0), false, false),
            ("sleep 0.1", 0, Some(0), false, false),
            ("head -n 3 a.py", 0, Some(3), true, false),
            ("tail -n 2 a.rs", 0, Some(2), true, false),
            ("cat a.rs", 0, None, true, false),
            ("cat -n a.rs", 0, None, true, false),
            ("nl -ba a.rs", 0, None, true, false),
            ("git show HEAD:a.rs", 0, None, true, false),
            ("git diff", 0, None, false, true),
            ("git log -p", 0, None, false, true),
            ("diff -u a.rs b.rs", 0, None, false, true),
            ("wc -l a.rs b.rs", 3, Some(3), false, false),
            ("echo hello", 1, Some(1), false, false),
            ("printf 'hello\\n'", 1, Some(1), false, false),
            ("sed -n '1,3p' a.py", 0, Some(3), true, false),
            ("custom-command", 0, None, false, false),
        ] {
            let shape = one(command);
            let output = &shape.outcomes[0];
            assert_eq!(output.lines, LineRange { min, max }, "{command}");
            assert_eq!(matches!(output.kind, Kind::File { .. }), file, "{command}");
            assert_eq!(matches!(output.kind, Kind::Diff), diff, "{command}");
        }
    }

    #[test]
    fn line_formats_are_required_patterns() {
        for (command, line) in [
            ("rg -n needle src", "src/main.rs:12:needle"),
            ("rg --files src", "src/main.rs"),
            ("git status --short", " M src/main.rs"),
            ("git log --oneline", "12abcde change"),
            ("git diff --numstat", "1\t2\tsrc/main.rs"),
        ] {
            let shape = one(command);
            let pattern = shape.outcomes[0].every.as_ref().expect(command);
            assert_eq!(pattern.strength, Strength::Required);
            assert!(pattern.regex.is_match(line), "{command}: {line}");
        }
    }

    #[test]
    fn cat_failure_has_no_stdout_lines() {
        for merged_stderr in [false, true] {
            let shape = pipeline_shape(
                "cat missing.py",
                &Stream {
                    merged_stderr,
                    ..Stream::default()
                },
                false,
            );
            assert_eq!(shape.outcomes.len(), 2);
            assert_eq!(
                shape.outcomes[1].lines.max,
                Some(usize::from(merged_stderr))
            );
            assert_eq!(shape.outcomes[1].known.is_some(), merged_stderr);
        }
    }

    #[test]
    fn rg_no_match_has_zero_lines_and_success_is_unchanged() {
        let shape = one("rg -n needle src");
        assert_eq!(shape.outcomes.len(), 2);
        assert_eq!(shape.outcomes[0].lines.min, 1);
        assert_eq!(
            shape.outcomes[1].lines,
            LineRange {
                min: 0,
                max: Some(0)
            }
        );
        assert!(shape.outcomes[0].every.is_some());
        let success = pipeline_shape("rg -n needle src", &Stream::default(), true);
        assert_eq!(success.outcomes.len(), 1);
        assert_eq!(success.outcomes[0].lines, shape.outcomes[0].lines);
        assert!(success.outcomes[0].every.is_some());
    }

    #[test]
    fn redirections_keep_their_shape_after_command_splitting() {
        let stream = Stream {
            exit_code: Some(0),
            ..Stream::default()
        };
        let shapes = shapes(
            "cat a.py 2>&1; echo done > result.txt; cat b.rs | cat",
            &stream,
        )
        .unwrap();
        assert_eq!(shapes.len(), 3);
        assert_eq!(shapes[0].outcomes.len(), 1);
        assert_eq!(shapes[0].outcomes[0].lines, UNKNOWN);
        assert!(matches!(shapes[0].outcomes[0].kind, Kind::Plain));
        assert_eq!(shapes[1].outcomes[0].lines, SILENT);
        assert_eq!(shapes[2].outcomes.len(), 2);
        assert!(matches!(shapes[2].outcomes[0].kind, Kind::File { .. }));
    }

    #[test]
    fn rg_cannot_succeed_when_its_input_is_known_empty() {
        let shape = one("printf '' | rg needle");
        assert!(
            shape
                .outcomes
                .iter()
                .all(|output| matches!(output.kind, Kind::Plain))
        );
        // The conservative upstream-failure result and the no-match result remain.
        assert_eq!(shape.outcomes.len(), 2);
        assert_eq!(shape.outcomes[1].lines, SILENT);
        let success = pipeline_shape("printf '' | rg needle", &Stream::default(), true);
        assert_eq!(success.outcomes.len(), 1);
        assert_eq!(success.outcomes[0].lines, SILENT);
    }

    #[test]
    fn openings_only_hint_at_file_start_in_the_right_language() {
        for (command, yes, no) in [
            ("cat a.py", "import os", "package main"),
            ("head a.rs", "use std::io;", "import os"),
            ("sed -n '1,3p' a.go", "package main", "use std::io;"),
        ] {
            let shape = one(command);
            let hint = shape.outcomes[0].first.as_ref().expect(command);
            assert_eq!(hint.strength, Strength::Hint);
            assert!(hint.regex.is_match(yes));
            assert!(!hint.regex.is_match(no));
        }
        for command in [
            "tail a.py",
            "sed -n '2,3p' a.py",
            "cat a.txt",
            "cat py",
            "cat a.py | tail -n 2",
            "cat a.py | rg import",
        ] {
            assert!(one(command).outcomes[0].first.is_none(), "{command}");
        }
    }

    #[test]
    fn pipeline_preserves_verbatim_text_and_merged_redirect_is_unknown() {
        let shape = one("echo hello | cat");
        assert_eq!(shape.outcomes[0].known.as_deref(), Some("hello\n"));
        assert_eq!(
            shape.outcomes[0].lines,
            LineRange {
                min: 1,
                max: Some(1)
            }
        );
        let shape = one("cat a.py | head -n 2");
        assert_eq!(shape.outcomes[0].lines.max, Some(2));
        assert!(shape.outcomes[0].first.is_some());
        let shape = one("cat a.py 2>&1");
        assert_eq!(shape.outcomes.len(), 1);
        assert_eq!(shape.outcomes[0].lines, LineRange { min: 0, max: None });
        assert!(matches!(shape.outcomes[0].kind, Kind::Plain));
        let shape = pipeline_shape("cat a.py | cat", &Stream::default(), true);
        assert_eq!(
            shape.outcomes.len(),
            2,
            "upstream failure survives final-stage success"
        );
        assert!(matches!(shape.outcomes[1].kind, Kind::Plain));
    }
}
