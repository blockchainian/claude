use super::{Stream, sections};
use crate::event::Section;
use serde::{Deserialize, Serialize};
use std::io::Write;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub(super) struct Span {
    pub(super) start: usize,
    pub(super) end: usize,
    pub(super) language: String,
}

fn spans(output: &[Section], source: &str) -> Vec<Span> {
    let mut rebuilt = String::new();
    let mut line = 0;
    let mut spans: Vec<Span> = Vec::new();
    for section in output {
        let (text, language) = match section {
            Section::Code { text, lang, .. } => (text, Some(lang.as_str())),
            Section::Markdown { text, .. } => (text, Some("md")),
            Section::Diff { text, .. } => (text, Some("diff")),
            Section::Text { text, .. } => (text, None),
            _ => panic!("unexpected shell output section: {section:?}"),
        };
        rebuilt.push_str(text);
        let end = line + text.split_inclusive('\n').count();
        if let Some(language) = language.filter(|_| end > line) {
            if let Some(previous) = spans
                .last_mut()
                .filter(|s| s.end == line && s.language == language)
            {
                previous.end = end;
            } else {
                spans.push(Span {
                    start: line,
                    end,
                    language: language.into(),
                });
            }
        }
        line = end;
    }
    assert_eq!(
        rebuilt, source,
        "sections must preserve the complete output"
    );
    spans
}

pub(super) fn actual(command: &str, stdout: &str, stream: Stream) -> Vec<Span> {
    sections(command, "stdout", stdout, stream)
        .map(|output| spans(&output, stdout))
        .unwrap_or_default()
}

type Expected<'a> = &'a [(usize, usize, &'a str)];
type Case<'a> = (&'a str, &'a str, Expected<'a>);
type StreamCase<'a> = (&'a str, &'a str, Stream, Expected<'a>);

#[test]
fn shell_output_command_table() {
    // A case is only a command, its real output shape, and the expected colored ranges.
    let cases: &[Case<'_>] = &[
        (
            "cd src && sed -n 35,70p checks.test.mjs",
            "export const value = 1;\n",
            &[(0, 1, "mjs")],
        ),
        ("sed -n 1,2p a.rs", "fn a() {}\n", &[(0, 1, "rs")]),
        ("sed -n 1,2p a.rs", "a\nb\nc\n", &[]),
        ("sed -n 1p a.rs; head -n 1 b.rs", "a\nb", &[(0, 2, "rs")]),
        ("cat a.rs; cat b.rs", "a\nb", &[(0, 2, "rs")]),
        ("cat a.yml b.yaml", "a: 1\nb: 2", &[(0, 2, "yml")]),
        ("cat a.rs b.py", "a\nb", &[]),
        ("head -n 2 a.rs; head -n 2 b.py", "a\nb\nc", &[]),
        ("head -n 1 a.rs; cat b.py", "a\n", &[]),
        ("head -1 a.rs", "a", &[(0, 1, "rs")]),
        ("tail -n1 a.rs", "a", &[(0, 1, "rs")]),
        ("tail -n +2 a.rs", "a\nb", &[(0, 2, "rs")]),
        ("cat a.rs | head -1", "a", &[(0, 1, "rs")]),
        ("cat a.rs | tail -n 1", "a", &[(0, 1, "rs")]),
        ("cat a.rs | sed -n 2p", "a", &[(0, 1, "rs")]),
        (
            "cat a.rs | head -n 0 > out; cat b.py",
            "print(1)",
            &[(0, 1, "py")],
        ),
        (
            "cat a.rs > out | cat b.py; cat c.rs",
            "print(1)\nfn main() {}",
            &[],
        ),
        (
            "cat a.rs > out | head -n 1; cat b.py",
            "print(1)",
            &[(0, 1, "py")],
        ),
        ("echo '---'; cat a.rs", "---\nfn main() {}", &[(1, 2, "rs")]),
        ("printf ''; cat a.rs", "fn main() {}", &[(0, 1, "rs")]),
        ("cat a.rs | cat", "a\nb", &[(0, 2, "rs")]),
        ("cat a.rs | grep foo", "a", &[]),
        ("cat a.rs | head -1; cat b.py", "a\nb", &[]),
        (
            "nl -ba a.rs",
            "     1\tfn main() {}",
            &[(0, 1, "numbered:rs")],
        ),
        (
            "cat -n a.rs",
            "     1\tfn main() {}",
            &[(0, 1, "numbered:rs")],
        ),
        (
            "nl -ba a.rs | sed -n 1,2p",
            "     1\tfn main() {}",
            &[(0, 1, "numbered:rs")],
        ),
        ("cat -n a.rs; cat a.rs", "1\tfn a() {}\nfn a() {}", &[]),
        (
            "git diff -- a.rs",
            "diff --git a/a.rs b/a.rs\n@@ -1 +1 @@\n-a\n+b",
            &[(0, 4, "diff")],
        ),
        ("git show HEAD:a.rs", "fn main() {}", &[(0, 1, "rs")]),
        ("git show HEAD", "@@ -1 +1 @@\n-a\n+b", &[(0, 3, "diff")]),
        ("git log -p -1", "@@ -1 +1 @@\n-a\n+b", &[(0, 3, "diff")]),
        (
            "diff -u a.rs b.rs",
            "@@ -1 +1 @@\n-a\n+b",
            &[(0, 3, "diff")],
        ),
        (
            "git diff | head -2",
            "diff --git a/a.rs b/a.rs\nindex 1234567..abcdef0 100644\n",
            &[(0, 2, "diff")],
        ),
        (
            "git diff; git show HEAD",
            "diff --git a/a.rs b/a.rs\n-a\n+b",
            &[(0, 3, "diff")],
        ),
        (
            "git add a.rs; git diff --cached -- a.rs",
            "diff --git a/a.rs b/a.rs\n@@ -1 +1 @@\n-a\n+b",
            &[(0, 4, "diff")],
        ),
        ("git add -v a.rs; git diff", "add 'a.rs'\n-a\n+b", &[]),
        ("git diff --stat", "a.rs | 1 +", &[]),
        (
            "git diff -- a.rs",
            "diff --git a/a.rs b/a.rs\n@@ -1 +1 @@\n-a\n+b",
            &[(0, 4, "diff")],
        ),
        ("git show --stat HEAD", "a.rs | 1 +", &[]),
        ("sed -n 1p a.rs; git diff", "a\n-b\n+c", &[]),
        (
            "head -n 1 a.rs; rg -n foo src",
            "a\nsrc/a.rs:2:foo\n",
            &[(0, 1, "rs")],
        ),
        (
            "grep -n foo a.rs; head -n 1 b.py",
            "2:foo\nprint(1)",
            &[(1, 2, "py")],
        ),
        ("head -n 1 a.rs; rg -n foo src", "a\n", &[(0, 1, "rs")]),
        (
            "head -n 1 a.rs; rg -n --count foo src",
            "a\nsrc/a.rs:2\n",
            &[],
        ),
        (
            "git status --short; head -n 1 a.rs",
            " M a.rs\na",
            &[(1, 2, "rs")],
        ),
        (
            "git log --oneline -1; head -n 1 a.rs",
            "abcdef12 title\na",
            &[(1, 2, "rs")],
        ),
        (
            "git log --oneline --graph; head -n 1 a.rs",
            "* abcdef12 title\na",
            &[],
        ),
        ("echo header; cat a.rs", "header\na\nb", &[(1, 3, "rs")]),
        (
            "cat a.rs; printf '%s\\n' footer",
            "a\nb\nfooter\n",
            &[(0, 2, "rs")],
        ),
        (
            "cat a.rs; wc -l a.rs b.rs",
            "a\n 1 a.rs\n 1 b.rs\n 2 total\n",
            &[(0, 1, "rs")],
        ),
        (
            "head -n 1 a.rs; echo separator; head -n 1 b.py",
            "a\nseparator\nb",
            &[(0, 1, "rs"), (2, 3, "py")],
        ),
        (
            "cd agents && sed -n 60,160p src/providers/codex/mod.rs; echo ----; sed -n 290,420p src/providers/claude/mod.rs",
            "fn a() {}\n----\nfn b() {}\n",
            &[(0, 1, "rs"), (2, 3, "rs")],
        ),
        (
            "sed -n 1,100p a.py; printf '%s\\n' ---- next; sed -n 1,100p b.rs",
            "print(1)\n----\nnext\nfn b() {}\n",
            &[(0, 1, "py"), (3, 4, "rs")],
        ),
        (
            "cat a.rs; echo ----; cat b.rs",
            "fn a() {}\n----\n----\nfn b() {}\n",
            &[],
        ),
        (
            "cat a.rs; echo; cat b.rs",
            "fn a() {}\n\nfn b() {}\n",
            &[(0, 1, "rs"), (2, 3, "rs")],
        ),
        (
            "cat a.rs; echo ----; cat b.rs",
            "fn a() {}\nfn b() {}\n",
            &[],
        ),
        (
            "cat a.py; printf '%s\\n' ---- next; cat b.rs",
            "print(1)\n----\nother\nnext\nfn b() {}\n",
            &[],
        ),
        (
            "cat a.py; echo first; cat b.rs; echo second; cat c.py",
            "print(1)\nsecond\nfn b() {}\nfirst\nprint(2)\n",
            &[],
        ),
        (
            "cat a.py; echo first; cat b.rs; echo second; cat c.py",
            "print(1)\nfirst\nfn b() {}\nsecond\nprint(2)\n",
            &[(0, 1, "py"), (2, 3, "rs"), (4, 5, "py")],
        ),
        (
            "cat a.py; echo first; cat b.rs; echo second; cat c.py",
            "print(1)\nfirst\nfn b() {}\nsecond\nprint(2)\nfirst\n",
            &[(0, 1, "py"), (2, 3, "rs"), (4, 6, "py")],
        ),
        (
            "cat a.py; printf '%s\\n' first second; cat b.rs; echo second; cat c.py",
            "print(1)\nfirst\nsecond\nfn b() {}\nprint(2)\n",
            &[],
        ),
        ("head -n 1 a.rs; ls", "a\nb", &[]),
        ("head -n 1 a.rs; rg foo src", "a\nsrc/a.rs:foo", &[]),
        ("head -n 1 a.rs > out; cat b.rs", "a\nb", &[(0, 2, "rs")]),
        ("head -n 1 a.rs; ls > out", "a", &[(0, 1, "rs")]),
        ("cat a.rs 2>&1", "a", &[]),
        ("cat $FILE > out; cat b.rs", "fn main() {}", &[(0, 1, "rs")]),
        ("head -n 1 $FILE; cat b.rs", "a\nb", &[]),
        ("head -n 1 *.rs; cat b.rs", "a\nb", &[]),
        ("head -n 1 ~/a.rs; cat b.rs", "a\nb", &[]),
        ("head -n 1 a.rs; echo \"$X\"; cat b.rs", "a\nb", &[]),
        ("cat a.rs || echo fallback", "a", &[]),
        ("cat a.rs & cat b.rs", "a\nb", &[]),
        ("cat a.rs & cat b.rs; cat c.rs", "a\nb\nc", &[]),
        ("cat a.rs || cat b.rs", "a\nb", &[(0, 2, "rs")]),
        ("cat $(pwd)/a.rs", "a", &[]),
        ("for f in a.rs; do cat $f; done", "a", &[]),
        ("cat <<EOF\na\nEOF", "a", &[]),
        ("cat 'foo bar.py'", "print(1)", &[(0, 1, "py")]),
        ("cat 'foo;bar.rs'", "fn main() {}", &[(0, 1, "rs")]),
        ("cd -; cat a.rs", "dir\na", &[]),
        ("pushd src; cat a.rs", "dir\na", &[]),
        ("popd; cat a.rs", "dir\na", &[]),
        ("mkdir -p dir; sleep 1; cat a.rs", "a", &[(0, 1, "rs")]),
        ("head -n nope a.rs", "a", &[]),
        ("cat < a.rs", "a", &[]),
        ("cat `pwd`/a.rs", "a", &[]),
        ("cat a{,b}.rs", "a", &[]),
        ("cat \"$FILE.rs\"", "a", &[]),
        ("cat '' a.rs", "a", &[]),
        ("cat ''", "a", &[]),
        ("cat 'a.rs", "a", &[]),
        ("sed -n '1,2,3p' a.rs", "a", &[]),
        ("wc -l src/*.rs; cat a.rs", "1 a.rs\na", &[]),
        ("sed -n '1p;2p' a.rs", "a", &[(0, 1, "rs")]),
        (
            "sed -n 1p a.unknown-language; cat b.py",
            "plain\nprint(1)",
            &[],
        ),
    ];
    for &(command, stdout, expected) in cases {
        let expected: Vec<_> = expected
            .iter()
            .map(|&(start, end, language)| Span {
                start,
                end,
                language: language.into(),
            })
            .collect();
        assert_eq!(
            actual(command, stdout, Stream::default()),
            expected,
            "{command}"
        );
    }
}

#[test]
fn shell_output_search_options() {
    let cases = [
        ("grep -n foo -A8 a.rs", "2:foo\n3-    #[default]\n", true),
        ("grep -rn foo src", "src/a.rs:2:foo\n", true),
        ("grep -rnE foo src", "src/a.rs:2:foo\n", true),
        (
            "grep -rn -A1 foo src",
            "src/a.rs:2:foo\nsrc/a.rs-3-context\n--\nsrc/a.rs:8:foo\nsrc/a.rs-9-context\n",
            true,
        ),
        ("grep -n -A 8 foo a.rs", "2:foo\n3-context\n", true),
        (
            "grep -rn -B3 -A3 foo src",
            "src/a.rs-1-before\nsrc/a.rs:2:foo\nsrc/a.rs-3-after\n",
            true,
        ),
        (
            "grep -rn --include='*.rs' foo src",
            "src/a.rs:2:foo\n",
            true,
        ),
        (
            "grep -rn --include '*.rs' foo src",
            "src/a.rs:2:foo\n",
            true,
        ),
        (
            "rg -n -A8 foo src",
            "src/a.rs:2:foo\nsrc/a.rs-3-after\n",
            true,
        ),
        ("rg -n -e '-l' src", "src/a.rs:2:-l\n", true),
        ("grep -rn --include '-l' foo src", "src/a.rs:2:foo\n", true),
        ("grep -rln foo src", "src/a.rs\n", false),
        ("grep -n -l foo src", "src/a.rs\n", false),
        ("grep -nc foo src", "src/a.rs:2\n", false),
        ("rg -n --json foo src", "{\"type\":\"match\"}\n", false),
        ("grep -rn foo src", "src/a.rs-after\n", false),
    ];
    for (search, output, recognized) in cases {
        for forward in [false, true] {
            let (command, stdout, start) = if forward {
                (
                    format!("{search}; cat b.rs"),
                    format!("{output}fn b() {{}}\n"),
                    output.lines().count(),
                )
            } else {
                (
                    format!("cat b.rs; {search}"),
                    format!("fn b() {{}}\n{output}"),
                    0,
                )
            };
            let expected = if search == "grep -rn foo src" && !recognized {
                vec![Span {
                    start: 0,
                    end: 2,
                    language: "rs".into(),
                }]
            } else if recognized {
                vec![Span {
                    start,
                    end: start + 1,
                    language: "rs".into(),
                }]
            } else {
                vec![]
            };
            assert_eq!(
                actual(&command, &stdout, Stream::default()),
                expected,
                "{command}"
            );
        }
    }
    assert_eq!(
        actual(
            "cat b.rs; grep -rn -A1 foo src",
            "fn b() {}\nsrc/a.rs:1:foo\nsrc/a.rs-2-after\n",
            Stream::default()
        ),
        vec![Span {
            start: 0,
            end: 1,
            language: "rs".into()
        }]
    );
    let command = "cd agents && sed -n 294,350p src/ui/input.rs; grep -n \"pub enum Panel\\|enum Panel\" -A8 src/ui/mod.rs";
    let output = format!(
        "{}233:enum Panel {{\n234-    #[default]\n{}",
        "    let value = 1;\n".repeat(57),
        "235-    Panel,\n".repeat(7)
    );
    assert_eq!(
        actual(command, &output, Stream::default()),
        vec![Span {
            start: 0,
            end: 57,
            language: "rs".into()
        }]
    );
}

#[test]
fn shell_output_line_formats_survive_filters() {
    let formats = [
        ("rg -n foo a.mjs", "72:const foo = 1;\n"),
        ("grep -n foo a.mjs", "72:const foo = 1;\n"),
        ("git status --short", " M a.mjs\n"),
        ("git log --oneline -1", "abcdef12 title\n"),
    ];
    let filters = [
        "head -25",
        "tail -n 25",
        "sed -n 1,25p",
        "cat",
        "head -25 | tail -n 25 | sed -n 1,25p | cat",
        "tail -n +1",
        "sed -n '1,$p'",
    ];
    for (producer, line) in formats {
        for filter in filters {
            for forward in [false, true] {
                let (command, stdout, start) = if forward {
                    (
                        format!("{producer} | {filter}; sed -n 140,190p a.mjs"),
                        format!("{line}export const value = 1;\n"),
                        1,
                    )
                } else {
                    (
                        format!("sed -n 140,190p a.mjs; {producer} | {filter}"),
                        format!("export const value = 1;\n{line}"),
                        0,
                    )
                };
                let expected = vec![Span {
                    start,
                    end: start + 1,
                    language: "mjs".into(),
                }];
                assert_eq!(
                    actual(&command, &stdout, Stream::default()),
                    expected,
                    "{command}"
                );
            }
        }
    }
}

#[test]
fn shell_output_provider_rules() {
    let claude = Stream {
        trimmed: true,
        ..Stream::default()
    };
    let codex = Stream {
        merged_stderr: true,
        ..Stream::default()
    };
    let cases: &[StreamCase<'_>] = &[
        ("echo; cat a.rs; echo", "a\nb", claude, &[(0, 2, "rs")]),
        (
            "cat a.rs; rg -n foo missing.rs a.rs | head -25",
            "fn a() {}\nrg: missing.rs: No such file or directory\n72:foo\n",
            codex,
            &[(0, 1, "rs")],
        ),
        (
            "rg -n foo missing.rs a.rs | head -25; cat b.py",
            "72:foo\nrg: missing.rs: No such file or directory\nprint(1)\n",
            codex,
            &[(2, 3, "py")],
        ),
        (
            "cat missing.py | head -25; cat b.py",
            "cat: missing.py: No such file or directory\nprint(1)\n",
            codex,
            &[(1, 2, "py")],
        ),
        (
            "cat a.rs; cat missing.rs | head -25",
            "fn a() {}\ncat: missing.rs: No such file or directory\n",
            codex,
            &[(0, 1, "rs")],
        ),
        (
            "cat a.rs; echo footer",
            "cat: missing.rs: No such file or directory\nfn a() {}\nfooter\n",
            codex,
            &[(1, 2, "rs")],
        ),
        (
            "cat a.yml",
            "cat: literal file content\n",
            claude,
            &[(0, 1, "yml")],
        ),
        ("cat a.rs", "rg: unrelated prefix\n", codex, &[(0, 1, "rs")]),
        (
            "rg -n marker a.rs; cat b.rs",
            "1:contains(\"…133 tokens truncated…\")\n2:next hit\nfn b() {}\n",
            codex,
            &[(2, 3, "rs")],
        ),
        (
            "cat b.rs; rg -n marker a.rs",
            "fn b() {}\n1:contains(\"[... omitted end of long line]\")\n2:next hit\n",
            codex,
            &[(0, 1, "rs")],
        ),
        (
            "cat a.rs",
            "let text = \"…133 tokens truncated…\";\n",
            codex,
            &[(0, 1, "rs")],
        ),
        (
            "printf '\\n'; cat a.rs; printf '\\n'",
            "a",
            claude,
            &[(0, 1, "rs")],
        ),
        ("echo; cat a.rs; echo", "\na\n\n", codex, &[(1, 2, "rs")]),
        (
            "cat a.rs; echo footer",
            "a",
            Stream {
                is_error: true,
                ..claude
            },
            &[],
        ),
        (
            "cat a.rs",
            "a",
            Stream {
                is_error: true,
                ..claude
            },
            &[(0, 1, "rs")],
        ),
        (
            "cat a.rs",
            "a\ncat: error",
            Stream {
                exit_code: Some(1),
                ..codex
            },
            &[(0, 1, "rs")],
        ),
        (
            "cat a.rs; rg -n absent b.rs",
            "a",
            Stream {
                exit_code: Some(1),
                ..codex
            },
            &[(0, 1, "rs")],
        ),
        (
            "cat a.rs; grep -n absent b.rs",
            "a",
            Stream {
                exit_code: Some(1),
                ..codex
            },
            &[(0, 1, "rs")],
        ),
        (
            "cat a.rs; rg -n absent b.rs",
            "a\nrg: error",
            Stream {
                exit_code: Some(2),
                ..codex
            },
            &[(0, 1, "rs")],
        ),
        (
            "head -n 100 a.rs",
            "Total output lines: 100\n\na\n…238 tokens truncated…\nb\nc",
            codex,
            &[(2, 3, "rs"), (4, 6, "rs")],
        ),
        (
            "cat a.rs",
            "a\n[... omitted end of long line]\nb",
            codex,
            &[(0, 3, "rs")],
        ),
        (
            "head -n 1 a.rs; cat b.py",
            "a\n…238 tokens truncated…\nb",
            codex,
            &[],
        ),
        (
            "head -n 1 a.rs; cat b.py",
            "Total output lines: 5\n\na\nb\n…238 tokens truncated…\nc",
            codex,
            &[],
        ),
        (
            "cat a.rs",
            "Warning: truncated output (original token count: 10000)\nTotal output lines: 50\n\na\n…238 tokens truncated…\nb",
            codex,
            &[(3, 4, "rs"), (5, 6, "rs")],
        ),
        (
            "echo header; cat a.rs; echo footer",
            "Total output lines: 100\n\nheader\na\n…238 tokens truncated…\nb\nfooter",
            codex,
            &[(3, 4, "rs"), (5, 6, "rs")],
        ),
    ];
    for &(command, stdout, stream, expected) in cases {
        let expected: Vec<_> = expected
            .iter()
            .map(|&(start, end, language)| Span {
                start,
                end,
                language: language.into(),
            })
            .collect();
        assert_eq!(actual(command, stdout, stream), expected, "{command}");
    }
}

#[derive(Deserialize)]
struct Call {
    provider: String,
    command: String,
    stdout: String,
    #[serde(default)]
    error: bool,
    exit: Option<i64>,
    #[serde(default)]
    expected: Vec<Span>,
    #[serde(default)]
    minimum_highlighted: usize,
}

impl Call {
    fn stream(&self) -> Stream {
        Stream {
            merged_stderr: self.provider == "codex",
            trimmed: self.provider == "claude",
            exit_code: self.exit,
            is_error: self.error,
        }
    }
}

fn language_scope(language: &str) -> String {
    let language = language.strip_prefix("numbered:").unwrap_or(language);
    crate::ui::highlight::find_syntax(language)
        .expect("fixture language must be supported")
        .scope
        .to_string()
}

#[test]
fn shell_output_diff_and_patch_share_scope() {
    assert_eq!(language_scope("patch"), language_scope("diff"));
}

#[derive(Deserialize)]
struct ScopeInput {
    paths: Vec<String>,
    languages: Vec<String>,
}

#[test]
fn shell_output_repository_corpus_has_no_wrong_colors() {
    let mut colored = 0;
    let mut wrong = Vec::new();
    for (index, line) in include_str!("../../../../tests/fixtures/shell-output-corpus.jsonl")
        .lines()
        .enumerate()
    {
        let call: Call = serde_json::from_str(line).expect("valid corpus record");
        let observed = actual(&call.command, &call.stdout, call.stream());
        let highlighted: usize = observed.iter().map(|s| s.end - s.start).sum();
        assert!(
            highlighted >= call.minimum_highlighted,
            "corpus record {} lost highlighting: {} < {}: {}",
            index + 1,
            highlighted,
            call.minimum_highlighted,
            call.command
        );
        for span in observed {
            for line in span.start..span.end {
                colored += 1;
                let correct = call.expected.iter().any(|expected| {
                    expected.start <= line
                        && line < expected.end
                        && language_scope(&expected.language) == language_scope(&span.language)
                });
                if !correct {
                    wrong.push((index + 1, line + 1, span.language.clone()));
                }
            }
        }
    }
    assert!(colored > 0, "the corpus must exercise actual highlighting");
    assert!(wrong.is_empty(), "wrong colored corpus lines: {wrong:?}");
}

#[test]
#[ignore = "local transcript audit; requires SHELL_OUTPUT_INPUT and SHELL_OUTPUT_REPORT"]
fn shell_output_audit() {
    let input = std::env::var("SHELL_OUTPUT_INPUT").expect("SHELL_OUTPUT_INPUT");
    let report = std::env::var("SHELL_OUTPUT_REPORT").expect("SHELL_OUTPUT_REPORT");
    let scope_input = std::env::var("SHELL_OUTPUT_SCOPE_INPUT").expect("SHELL_OUTPUT_SCOPE_INPUT");
    let scope_report =
        std::env::var("SHELL_OUTPUT_SCOPE_REPORT").expect("SHELL_OUTPUT_SCOPE_REPORT");
    let scope_input: ScopeInput =
        serde_json::from_str(&std::fs::read_to_string(scope_input).expect("read scope input"))
            .expect("valid scope input");
    let resolve = |language: &str| {
        crate::ui::highlight::find_syntax(language.strip_prefix("numbered:").unwrap_or(language))
            .map(|syntax| syntax.scope.to_string())
    };
    let mut languages: std::collections::BTreeMap<String, Option<String>> = scope_input
        .languages
        .into_iter()
        .map(|language| {
            let scope = resolve(&language);
            (language, scope)
        })
        .collect();
    let paths: std::collections::BTreeMap<String, Option<String>> = scope_input
        .paths
        .into_iter()
        .map(|path| {
            let language = super::file_language(&path);
            let scope = resolve(language);
            languages.insert(language.to_owned(), scope.clone());
            (path, scope)
        })
        .collect();
    let source = std::fs::read_to_string(input).expect("read audit input");
    let calls: Vec<Call> = source
        .lines()
        .map(|line| serde_json::from_str(line).expect("valid audit record"))
        .collect();
    let start = std::time::Instant::now();
    let outputs: Vec<_> = calls
        .iter()
        .map(|call| sections(&call.command, "stdout", &call.stdout, call.stream()))
        .collect();
    let elapsed = start.elapsed().as_secs_f64();
    let mut file =
        std::io::BufWriter::new(std::fs::File::create(report).expect("create audit report"));
    for (call, output) in calls.iter().zip(outputs) {
        let actual = output
            .map(|output| spans(&output, &call.stdout))
            .unwrap_or_default();
        for span in &actual {
            languages.insert(span.language.clone(), resolve(&span.language));
        }
        serde_json::to_writer(
            &mut file,
            &serde_json::json!({"provider":call.provider,"command":call.command,"actual":actual}),
        )
        .expect("write audit record");
        writeln!(file).expect("write audit newline");
    }
    file.flush().expect("flush audit report");
    std::fs::write(
        scope_report,
        serde_json::to_vec(&serde_json::json!({
            "paths": paths, "languages": languages,
        }))
        .expect("serialize scopes"),
    )
    .expect("write scope report");
    println!(
        "{}",
        serde_json::json!({"calls":calls.len(),"elapsed_seconds":elapsed})
    );
}

fn boundary_cases() -> serde_json::Value {
    serde_json::from_str(include_str!(
        "../../../../tests/fixtures/shell-output-boundaries.json"
    ))
    .unwrap()
}

#[test]
fn shell_output_real_boundary_ranges() {
    let cases = boundary_cases();
    let expected: &[(&str, Expected<'_>)] = &[
        ("2", &[(0, 180, "rs"), (217, 238, "rs")]),
        ("3", &[(0, 235, "rs"), (235, 326, "py")]),
        ("4", &[(0, 151, "rs"), (156, 184, "py")]),
        ("8", &[(0, 61, "rs"), (62, 114, "log")]),
    ];
    for (id, expected) in expected {
        let case = &cases[id];
        assert_eq!(
            actual(
                case["command"].as_str().unwrap(),
                case["output"].as_str().unwrap(),
                Stream {
                    merged_stderr: true,
                    exit_code: case["exit"].as_i64(),
                    ..Stream::default()
                }
            ),
            expected
                .iter()
                .map(|&(start, end, language)| Span {
                    start,
                    end,
                    language: language.into()
                })
                .collect::<Vec<_>>(),
            "real case {id}",
        );
    }
}

#[test]
fn shell_output_real_ambiguous_opening() {
    let cases = boundary_cases();
    let case = &cases["3"];
    let command = case["command"]
        .as_str()
        .unwrap()
        .replace("730,780p", "730,1000p");
    assert!(
        actual(
            &command,
            case["output"].as_str().unwrap(),
            Stream {
                merged_stderr: true,
                exit_code: case["exit"].as_i64(),
                ..Stream::default()
            }
        )
        .is_empty()
    );
}

#[test]
fn shell_output_real_mismatched_cat_path() {
    let cases = boundary_cases();
    let case = &cases["8"];
    let command = case["command"]
        .as_str()
        .unwrap()
        .replace("agents/.cargo/config.toml", "agents/.cargo/other.toml");
    assert!(
        actual(
            &command,
            case["output"].as_str().unwrap(),
            Stream {
                merged_stderr: true,
                exit_code: case["exit"].as_i64(),
                ..Stream::default()
            }
        )
        .is_empty()
    );
}

#[test]
fn shell_output_inconsistent_description_invalidates_call() {
    let cases = boundary_cases();
    let case = &cases["2"];
    let command = case["command"].as_str().unwrap().replace("1,180p", "1,1p");
    assert!(
        actual(
            &command,
            case["output"].as_str().unwrap(),
            Stream {
                merged_stderr: true,
                exit_code: case["exit"].as_i64(),
                ..Stream::default()
            }
        )
        .is_empty()
    );
}

#[test]
fn shell_output_boundary_matches_line_contents() {
    for ending in ["\n", "\r\n"] {
        let text = format!("src/a.rs{ending}fn main() {{}}{ending}");
        assert_eq!(
            actual("rg --files src; head -n 1 a.rs", &text, Stream::default()),
            vec![Span {
                start: 1,
                end: 2,
                language: "rs".into()
            }]
        );
    }
    assert!(
        actual(
            "rg --files src; head -n 1 a.rs",
            "src/a.rs \nfn main() {}\n",
            Stream::default()
        )
        .is_empty()
    );
}
