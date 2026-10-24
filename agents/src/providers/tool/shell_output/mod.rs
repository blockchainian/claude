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
use shape::Kind;

#[derive(Clone, Copy, Default)]
pub(crate) struct Stream {
    pub merged_stderr: bool,
    pub trimmed: bool,
    pub exit_code: Option<i64>,
    pub is_error: bool,
}

fn syntax(path: &str) -> Option<syntect::parsing::Scope> {
    crate::ui::highlight::find_syntax(file_language(path)).map(|s| s.scope)
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

pub(super) fn sections(
    command: &str,
    label: &str,
    text: &str,
    stream: Stream,
) -> Option<Vec<Section>> {
    // shapes splits commands with their separators before describing pipelines.
    let commands = crate::ui::shell_commands::split_with_separators(command)?;
    let programs: Vec<_> = commands
        .iter()
        .filter_map(|part| {
            let stages = shell_summary::pipeline_words(&command[part.range.clone()])?;
            let program = stages.first()?.first()?;
            std::path::Path::new(program)
                .file_name()?
                .to_str()
                .map(str::to_owned)
        })
        .collect();
    let programs: Vec<_> = programs.iter().map(String::as_str).collect();
    let shapes = describe::shapes(command, &stream)?;
    let known: Vec<_> = shapes
        .iter()
        .flat_map(|shape| &shape.outcomes)
        .filter_map(|outcome| outcome.known.as_deref())
        .collect();
    let lines = lines::lines(text, &stream, &programs, &known);
    let solution = solve::solve(&shapes, &lines, text)?;
    let ownership = ownership::ownership(&shapes, &solution, &lines, text);
    let mut output = Vec::new();
    let mut cursor = 0;
    for (range, owner) in ownership.0 {
        let command = match owner {
            ownership::Owner::Command(index) => Some(index),
            ownership::Owner::Block(commands) => Some(commands.start),
            ownership::Owner::Plain => None,
        };
        let kind = command.map(|index| {
            let outcome = solution.outcomes[index]
                .iter()
                .next()
                .expect("owned shell output has a feasible outcome");
            &shapes[index].outcomes[outcome].kind
        });
        let mut index = range.start;
        while index < range.end {
            let lines::Line::Text(bytes) = &lines[index] else {
                index += 1;
                continue;
            };
            let start = bytes.start;
            let mut end = bytes.end;
            index += 1;
            while index < range.end {
                let lines::Line::Text(bytes) = &lines[index] else {
                    break;
                };
                if bytes.start != end {
                    break;
                }
                end = bytes.end;
                index += 1;
            }
            if cursor < start {
                output.push(text_section(label, &text[cursor..start]));
            }
            output.push(kind_section(label, kind, &text[start..end]));
            cursor = end;
        }
    }
    if cursor < text.len() {
        output.push(text_section(label, &text[cursor..]));
    }
    Some(output)
}

fn kind_section(label: &str, kind: Option<&Kind>, text: &str) -> Section {
    match kind {
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
            if matches!(language, "strings" | "ansi") {
                assert!(
                    actual(command, "line\n", Stream::default()).is_empty(),
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
                !actual(command, &"line\n".repeat(100), Stream::default()).is_empty(),
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
            let output = format!("{}\nfn a() {{}}\n", paths.join("\n"));
            assert_eq!(
                tests::actual(&format!("{command}; cat a.rs"), &output, Stream::default()),
                vec![tests::Span {
                    start: paths.len(),
                    end: paths.len() + 1,
                    language: "rs".into()
                }]
            );
        }
    }
}
