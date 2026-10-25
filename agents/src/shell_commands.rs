#![allow(dead_code)]

use std::ops::Range;

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Command {
    pub range: Range<usize>,
    pub and_next: bool,
}

/// The shell operator immediately before a command; newlines count as semicolons.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Separator {
    Start,
    Semicolon,
    And,
    Or,
    Pipe,
}

/// A command's source byte range and preceding shell operator.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SeparatedCommand {
    pub range: Range<usize>,
    pub separator: Separator,
}

/// Return command ranges with separators; ambiguous syntax or background & returns None.
pub(crate) fn split_with_separators(source: &str) -> Option<Vec<SeparatedCommand>> {
    parse(source, true)
}

/// Returns source ranges only when shell grouping is unambiguous.
pub(crate) fn split(source: &str) -> Option<Vec<Command>> {
    let commands = parse(source, false)?;
    Some(
        commands
            .iter()
            .enumerate()
            .map(|(index, command)| Command {
                range: command.range.clone(),
                and_next: commands
                    .get(index + 1)
                    .is_some_and(|next| next.separator == Separator::And),
            })
            .collect(),
    )
}

fn parse(source: &str, separated: bool) -> Option<Vec<SeparatedCommand>> {
    let bytes = source.as_bytes();
    let mut commands = Vec::new();
    let mut separator = Separator::Start;
    let mut groups = Vec::new();
    let mut heredocs = Vec::new();
    let mut start = 0;
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        match groups.last().copied() {
            Some(b'\'') => {
                if byte == b'\'' {
                    groups.pop();
                }
                index += 1;
                continue;
            }
            Some(b'`') | Some(b'$') => {
                if byte == b'\\' {
                    index += 2;
                } else {
                    if byte == b'`' && groups.last() == Some(&b'`')
                        || byte == b'\'' && groups.last() == Some(&b'$')
                    {
                        groups.pop();
                    }
                    index += 1;
                }
                continue;
            }
            Some(b'"') => {
                match byte {
                    b'\\' => index += 2,
                    b'"' => {
                        groups.pop();
                        index += 1;
                    }
                    b'`' => {
                        groups.push(b'`');
                        index += 1;
                    }
                    b'$' if bytes.get(index + 1) == Some(&b'(') => {
                        groups.push(b')');
                        index += 2;
                    }
                    b'$' if bytes.get(index + 1) == Some(&b'{') => {
                        groups.push(b'}');
                        index += 2;
                    }
                    _ => index += 1,
                }
                continue;
            }
            _ => {}
        }
        match byte {
            b'$' if bytes.get(index + 1) == Some(&b'\'') => {
                // ANSI-C quoting ends at an unescaped single quote.
                groups.push(b'$');
                index += 2;
            }
            b'\\' => {
                if index + 1 == bytes.len() {
                    return None;
                }
                index += 2;
            }
            b'\'' | b'"' | b'`' => {
                groups.push(byte);
                index += 1;
            }
            b'(' | b'{' => {
                groups.push(if byte == b'(' { b')' } else { b'}' });
                index += 1;
            }
            b'[' if bytes.get(index + 1) == Some(&b'[') => {
                groups.push(b']');
                index += 2;
            }
            b']' if groups.last() == Some(&b']') && bytes.get(index + 1) == Some(&b']') => {
                groups.pop();
                index += 2;
            }
            b']' if bytes.get(index + 1) == Some(&b']') => return None,
            b')' | b'}' => {
                if groups.pop() != Some(byte) {
                    return None;
                }
                index += 1;
            }
            b'#' if index == 0 || bytes[index - 1].is_ascii_whitespace() => {
                if source[start..index].trim().is_empty() {
                    return None;
                }
                while index < bytes.len() && bytes[index] != b'\n' {
                    index += 1;
                }
            }
            b'<' if bytes.get(index + 1) == Some(&b'<') => {
                // A heredoc inside a group needs the complete shell grammar.
                if !groups.is_empty() || bytes.get(index + 2) == Some(&b'<') {
                    return None;
                }
                let (end, heredoc) = heredoc_delimiter(source, index + 2)?;
                heredocs.push(heredoc);
                index = end;
            }
            b'&' if groups.is_empty() && bytes.get(index + 1) == Some(&b'&') => {
                if !heredocs.is_empty() || bytes.get(index + 2) == Some(&b'&') {
                    return None;
                }
                push_command(source, start, index, separator, &mut commands)?;
                separator = Separator::And;
                index += 2;
                start = index;
            }
            b'|' if separated && groups.is_empty() => {
                if !heredocs.is_empty() {
                    return None;
                }
                let or = bytes.get(index + 1) == Some(&b'|');
                let width = if or { 2 } else { 1 };
                if matches!(bytes.get(index + width), Some(b'|' | b'&')) {
                    return None;
                }
                push_command(source, start, index, separator, &mut commands)?;
                separator = if or { Separator::Or } else { Separator::Pipe };
                index += width;
                start = index;
            }
            b'<' | b'>' if separated => {
                // Consume redirection operators before interpreting & or | as separators.
                index += 1;
                if bytes.get(index) == Some(&b'&')
                    || (byte == b'>' && bytes.get(index) == Some(&b'|'))
                {
                    index += 1;
                }
            }
            b'&' if separated => {
                if !groups.is_empty() && bytes.get(index + 1) == Some(&b'&') {
                    index += 2;
                    continue;
                }
                // Combined stdout/stderr redirects are not jobs.
                if bytes.get(index + 1) != Some(&b'>') {
                    return None;
                }
                index += 1;
            }
            b';' if groups.is_empty() => {
                if !heredocs.is_empty() || matches!(bytes.get(index + 1), Some(b';' | b'&')) {
                    return None;
                }
                push_command(source, start, index, separator, &mut commands)?;
                separator = Separator::Semicolon;
                index += 1;
                start = index;
            }
            b'\n' if groups.is_empty() => {
                let mut end = index;
                if !heredocs.is_empty() {
                    index += 1;
                    for heredoc in heredocs.drain(..) {
                        let (_, marker_end) =
                            heredoc_body(source, index, &heredoc.delimiter, heredoc.strip_tabs)?;
                        end = marker_end;
                        index = marker_end + usize::from(marker_end < bytes.len());
                    }
                }
                let command = source[start..end].trim();
                if !command.is_empty() {
                    // Newlines following pipeline operators continue that pipeline.
                    if command.ends_with('|') {
                        return None;
                    }
                    push_command(source, start, end, separator, &mut commands)?;
                    separator = Separator::Semicolon;
                }
                index = end + usize::from(end < bytes.len());
                start = index;
            }
            _ => {
                if groups.is_empty()
                    && byte.is_ascii_alphabetic()
                    && (index == 0 || is_word_boundary(bytes[index - 1]))
                {
                    let end = bytes[index..]
                        .iter()
                        .position(|byte| is_word_boundary(*byte))
                        .map_or(bytes.len(), |length| index + length);
                    // Reserved-word compounds have separators at this same nesting depth.
                    if (source[start..index].trim().is_empty()
                        || source[start..index].trim_end().ends_with(['|', '&'])
                        || source[start..index]
                            .split_ascii_whitespace()
                            .next()
                            .is_some_and(|word| word.contains('=')))
                        && matches!(
                            &source[index..end],
                            "if" | "then"
                                | "else"
                                | "elif"
                                | "fi"
                                | "for"
                                | "while"
                                | "until"
                                | "do"
                                | "done"
                                | "case"
                                | "esac"
                                | "select"
                                | "function"
                                | "coproc"
                        )
                    {
                        return None;
                    }
                }
                index += 1;
            }
        }
    }
    if !groups.is_empty() || !heredocs.is_empty() || index > bytes.len() {
        return None;
    }
    if source[start..].trim_end().ends_with('|') {
        return None;
    }
    if !source[start..].trim().is_empty() {
        push_command(source, start, source.len(), separator, &mut commands)?;
    } else if matches!(separator, Separator::And | Separator::Or | Separator::Pipe) {
        return None;
    }
    Some(commands)
}

fn is_word_boundary(byte: u8) -> bool {
    byte.is_ascii_whitespace() || b";&|(){}<>".contains(&byte)
}

fn push_command(
    source: &str,
    start: usize,
    end: usize,
    separator: Separator,
    commands: &mut Vec<SeparatedCommand>,
) -> Option<()> {
    let text = source[start..end].trim();
    if text.is_empty() {
        return None;
    }
    let start = start + source[start..end].len() - source[start..end].trim_start().len();
    commands.push(SeparatedCommand {
        range: start..start + text.len(),
        separator,
    });
    Some(())
}

struct Heredoc {
    delimiter: String,
    strip_tabs: bool,
    quoted: bool,
}

fn heredoc_delimiter(source: &str, mut index: usize) -> Option<(usize, Heredoc)> {
    let bytes = source.as_bytes();
    let tabs = bytes.get(index) == Some(&b'-');
    index += usize::from(tabs);
    while matches!(bytes.get(index), Some(b' ' | b'\t')) {
        index += 1;
    }
    let start = index;
    let mut delimiter = Vec::new();
    let mut quote = None;
    let mut quoted = false;
    while let Some(&byte) = bytes.get(index) {
        match (quote, byte) {
            (Some(mark), byte) if mark == byte => quote = None,
            (None, b'\'' | b'"') => {
                quote = Some(byte);
                quoted = true;
            }
            (None, byte) if is_word_boundary(byte) => break,
            (_, b'\\') if quote != Some(b'\'') => {
                quoted = true;
                index += 1;
                let &escaped = bytes.get(index)?;
                if escaped == b'\n' {
                    return None;
                }
                delimiter.push(escaped);
            }
            (_, byte) => delimiter.push(byte),
        }
        index += 1;
    }
    if quote.is_some() || start == index {
        return None;
    }
    Some((
        index,
        Heredoc {
            delimiter: String::from_utf8(delimiter).ok()?,
            strip_tabs: tabs,
            quoted,
        },
    ))
}

/// The body and delimiter's end use the same boundaries for layout and highlighting.
fn heredoc_body(
    source: &str,
    start: usize,
    delimiter: &str,
    tabs: bool,
) -> Option<(Range<usize>, usize)> {
    let mut offset = start;
    loop {
        let newline = source[offset..].find('\n');
        let end = newline.map_or(source.len(), |n| offset + n);
        let marker = &source[offset..end];
        let marker = if tabs {
            marker.trim_start_matches('\t')
        } else {
            marker
        };
        if marker == delimiter {
            return Some((start..offset, end));
        }
        newline?;
        offset = end + 1;
    }
}

#[derive(Clone, Copy)]
pub(crate) enum SyntaxSource<'a> {
    Interpreter(&'a str),
    Extension(&'a str),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScriptKind {
    Heredoc { literal: bool, strip_tabs: bool },
    Argument,
}

/// Embedded code, excluding the shell's quotes and delimiter.
pub(crate) struct Script<'a> {
    pub kind: ScriptKind,
    pub syntax: SyntaxSource<'a>,
    pub range: Range<usize>,
}

/// Recognizes interpreter invocations and cat file heredocs without changing source bytes.
/// Compounds that `split` cannot safely group keep their shell highlighting.
pub(crate) fn scripts(source: &str) -> Vec<Script<'_>> {
    let Some(commands) = split(source) else {
        return Vec::new();
    };
    let mut scripts = Vec::new();
    for command in commands {
        let mut index = command.range.start;
        let Some(interpreter) = argument(source, &mut index) else {
            continue;
        };
        let interpreter = &source[interpreter];
        if interpreter == "cat" {
            if let Some(found) = cat_heredocs(source, index, command.range.end) {
                scripts.extend(found);
            }
            continue;
        }
        let mut stdin = false;
        while index < command.range.end {
            while matches!(source.as_bytes().get(index), Some(b' ' | b'\t')) {
                index += 1;
            }
            if source[index..].starts_with("<<") {
                if source[index..].starts_with("<<<") {
                    break;
                }
                if let Some(found) = interpreter_heredocs(source, index, interpreter) {
                    scripts.extend(found);
                }
                break;
            }
            let Some(option) = argument(source, &mut index) else {
                break;
            };
            let option = &source[option];
            if stdin || option == "-" {
                stdin = true;
                continue;
            }
            if matches!(option, "-c" | "-e" | "--eval") {
                if let Some(range) = argument(source, &mut index) {
                    scripts.push(Script {
                        kind: ScriptKind::Argument,
                        syntax: SyntaxSource::Interpreter(interpreter),
                        range,
                    });
                }
                break;
            }
            if !option.starts_with('-') || option == "--" {
                break;
            }
        }
    }
    scripts
}

fn interpreter_heredocs<'a>(
    source: &'a str,
    mut index: usize,
    interpreter: &'a str,
) -> Option<Vec<Script<'a>>> {
    let mut heredocs = Vec::new();
    loop {
        let (end, heredoc) = heredoc_delimiter(source, index + 2)?;
        heredocs.push(heredoc);
        index = end;
        while matches!(source.as_bytes().get(index), Some(b' ' | b'\t')) {
            index += 1;
        }
        if !source[index..].starts_with("<<") || source[index..].starts_with("<<<") {
            let newline = source[index..].find('\n')?;
            return heredoc_scripts(
                source,
                index + newline + 1,
                heredocs,
                SyntaxSource::Interpreter(interpreter),
            );
        }
    }
}

fn heredoc_scripts<'a>(
    source: &'a str,
    mut start: usize,
    heredocs: Vec<Heredoc>,
    syntax: SyntaxSource<'a>,
) -> Option<Vec<Script<'a>>> {
    let mut scripts = Vec::new();
    for heredoc in heredocs {
        let (range, end) = heredoc_body(source, start, &heredoc.delimiter, heredoc.strip_tabs)?;
        let literal = heredoc.quoted || !source[range.clone()].contains(['$', '`', '\\']);
        scripts.push(Script {
            kind: ScriptKind::Heredoc {
                literal,
                strip_tabs: heredoc.strip_tabs,
            },
            syntax,
            range,
        });
        start = end + usize::from(end < source.len());
    }
    Some(scripts)
}

fn cat_heredocs(source: &str, mut index: usize, end: usize) -> Option<Vec<Script<'_>>> {
    let bytes = source.as_bytes();
    let mut extension = None;
    let mut heredocs = Vec::new();
    while index < end {
        while matches!(bytes.get(index), Some(b' ' | b'\t')) {
            index += 1;
        }
        match bytes.get(index) {
            Some(b'>') if extension.is_none() => {
                index += if bytes.get(index + 1) == Some(&b'>') {
                    2
                } else {
                    1
                };
                let path = argument(source, &mut index)?;
                let literal = &source[path.clone()];
                let quote = path.start.checked_sub(1).map(|i| bytes[i]);
                if quote != Some(b'\'')
                    && (literal.contains(['$', '`'])
                        || (quote != Some(b'"') && literal.contains(['*', '?', '[', '~'])))
                {
                    return None;
                }
                extension = Some(std::path::Path::new(literal).extension()?.to_str()?);
            }
            Some(b'<') if bytes.get(index + 1) == Some(&b'<') => {
                let (next, heredoc) = heredoc_delimiter(source, index + 2)?;
                index = next;
                heredocs.push(heredoc);
            }
            Some(b'\n') => {
                return heredoc_scripts(
                    source,
                    index + 1,
                    heredocs,
                    SyntaxSource::Extension(extension?),
                );
            }
            _ => return None,
        }
    }
    None
}

/// A literal shell word's content range; surrounding quotes stay shell syntax.
/// Concatenated words and unquoted expansions require evaluation and are left as shell.
fn argument(source: &str, index: &mut usize) -> Option<Range<usize>> {
    let bytes = source.as_bytes();
    while matches!(bytes.get(*index), Some(b' ' | b'\t')) {
        *index += 1;
    }
    let &first = bytes.get(*index)?;
    if is_word_boundary(first) || first == b'#' {
        return None;
    }
    let quote = matches!(first, b'\'' | b'"').then_some(first);
    *index += usize::from(quote.is_some());
    let start = *index;
    while let Some(&byte) = bytes.get(*index) {
        if Some(byte) == quote || (quote.is_none() && is_word_boundary(byte)) {
            break;
        }
        if byte == b'\\' && quote != Some(b'\'') {
            *index += 1;
            bytes.get(*index)?;
        } else if quote.is_none() && matches!(byte, b'\'' | b'"' | b'$' | b'`') {
            return None;
        }
        *index += 1;
    }
    let end = *index;
    if quote.is_some() {
        if bytes.get(*index).copied() != quote {
            return None;
        }
        *index += 1;
        if bytes
            .get(*index)
            .is_some_and(|byte| !is_word_boundary(*byte))
        {
            return None;
        }
    }
    Some(start..end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_heredoc_literals_and_tabs() {
        for (source, literal, strip_tabs) in [
            ("node <<'EOF'\n$value `name` \\\nEOF", true, false),
            ("node <<\"EOF\"\n$value\nEOF", true, false),
            ("node <<E'OF'\n$value\nEOF", true, false),
            ("node <<\\EOF\n$value\nEOF", true, false),
            ("node <<EOF\nconsole.log(1)\nEOF", true, false),
            ("node <<EOF\n$value\nEOF", false, false),
            ("node <<EOF\n`name`\nEOF", false, false),
            ("node <<EOF\nconsole.log(\\n)\nEOF", false, false),
            ("node <<-EOF\n\tconsole.log(1)\n\tEOF", true, true),
            ("cat > file.js <<EOF\n$value\nEOF", false, false),
            ("cat > file.js <<'EOF'\n$value\nEOF", true, false),
        ] {
            let parsed = scripts(source);
            assert_eq!(parsed.len(), 1, "{source}");
            assert_eq!(
                parsed[0].kind,
                ScriptKind::Heredoc {
                    literal,
                    strip_tabs
                },
                "{source}"
            );
        }
    }

    #[test]
    fn classifies_eval_arguments() {
        for source in [
            "node -e 'console.log(1)'",
            "sh -c 'echo hello'",
            "node --eval 'console.log(1)'",
        ] {
            let parsed = scripts(source);
            assert_eq!(parsed.len(), 1, "{source}");
            assert_eq!(parsed[0].kind, ScriptKind::Argument);
            assert!(!source[parsed[0].range.clone()].contains('\''));
        }
    }

    #[test]
    fn classifies_two_heredocs_in_source_order() {
        for header in ["node", "cat > file.js"] {
            let source =
                format!("{header} <<'FIRST' <<-SECOND\n$value\nFIRST\n\tconsole.log(2)\n\tSECOND");
            let parsed = scripts(&source);
            assert_eq!(parsed.len(), 2, "{source}");
            assert_eq!(&source[parsed[0].range.clone()], "$value\n");
            assert_eq!(&source[parsed[1].range.clone()], "\tconsole.log(2)\n");
            assert!(parsed[0].range.end < parsed[1].range.start);
            assert_eq!(
                parsed[0].kind,
                ScriptKind::Heredoc {
                    literal: true,
                    strip_tabs: false
                }
            );
            assert_eq!(
                parsed[1].kind,
                ScriptKind::Heredoc {
                    literal: true,
                    strip_tabs: true
                }
            );
        }
    }

    fn commands(source: &str) -> Vec<(&str, bool)> {
        split(source)
            .expect("valid shell command")
            .iter()
            .map(|command| (&source[command.range.clone()], command.and_next))
            .collect()
    }

    fn separated(source: &str) -> Vec<(&str, Separator)> {
        split_with_separators(source)
            .expect("valid foreground shell commands")
            .into_iter()
            .map(|command| (&source[command.range], command.separator))
            .collect()
    }

    #[test]
    fn separates_shell_operators() {
        use Separator::*;
        let source = "git status --short; git diff --stat\nsed -n '1,40p' src/main.rs && cargo test || cat errors.log | head -n 20";
        assert_eq!(
            separated(source),
            vec![
                ("git status --short", Start),
                ("git diff --stat", Semicolon),
                ("sed -n '1,40p' src/main.rs", Semicolon),
                ("cargo test", And),
                ("cat errors.log", Or),
                ("head -n 20", Pipe),
            ]
        );
        for operator in ["&&", "||", "|"] {
            let source = format!("git status {operator}\n git diff");
            assert_eq!(separated(&source).len(), 2);
        }
    }

    #[test]
    fn separated_commands_protect_shell_contexts_and_heredocs() {
        for inner in [
            "echo 'a && b || c | d; e & f'",
            "echo \"a && b || c | d; e & f\"",
            "echo $(a && b || c | d)",
            "echo `a && b || c | d`",
            "echo $'a && b || c | d'",
            "echo a\\&b",
            "cat <<'EOF'\na && b || c | d; e & f\nEOF",
            "cat <<-EOF\n\ta && b || c | d; e & f\n\tEOF",
        ] {
            let source = format!("{inner}\necho done");
            assert_eq!(
                separated(&source),
                vec![
                    (inner, Separator::Start),
                    ("echo done", Separator::Semicolon)
                ]
            );
        }
    }

    #[test]
    fn separated_commands_reject_background_but_keep_redirects() {
        for source in [
            "cargo test &",
            "cargo test & git status",
            "cargo test &>out &",
            r"echo \>&",
            "echo $(cargo test &)",
        ] {
            assert_eq!(split_with_separators(source), None, "{source}");
        }
        for command in [
            "cargo test 2>&1",
            "cargo test &>out",
            "cargo test &>>out",
            "cat <&0",
            "echo 2>&-",
            "echo >out 2>&1",
            "cargo test >|out",
        ] {
            let source = format!("{command} && git status");
            assert_eq!(
                separated(&source),
                vec![(command, Separator::Start), ("git status", Separator::And)]
            );
        }
        for source in ["a ||", "a |", "a &&", "a ||| b", "a |& b", "a ;; b"] {
            assert_eq!(split_with_separators(source), None, "{source}");
        }
    }

    #[test]
    fn splits_real_bash_command() {
        let source = r#"cd ~/Code/blockchainian/dot && F="darwin/Library/Application Support/Code/User/settings.json" && git diff --stat -- "$F"; sed -i '' 's/^  "workbench.panel.defaultLocation": "left",$/  "workbench.panel.defaultLocation": "left",\n  "workbench.panel.showLabels": false,/' "$F" && sed -n 6,9p "$F""#;
        let result = commands(source);
        assert_eq!(result.len(), 5);
        assert_eq!(result[0], ("cd ~/Code/blockchainian/dot", true));
        assert_eq!(
            result[1],
            (
                r#"F="darwin/Library/Application Support/Code/User/settings.json""#,
                true
            )
        );
        assert_eq!(result[2], (r#"git diff --stat -- "$F""#, false));
        assert!(result[3].0.starts_with("sed -i '' 's/"));
        assert!(result[3].1);
        assert_eq!(result[4], (r#"sed -n 6,9p "$F""#, false));
    }

    #[test]
    fn protects_shell_contexts() {
        for inner in [
            "echo 'a && b; c'",
            "echo \"a && b; c\"",
            "echo $(a && b; c)",
            "echo \"$(a && echo \"b; c\")\"",
            "echo `a && b; c`",
            "(a && b; c)",
            "{ a && b; c; }",
            "[[ a && b; c ]]",
            "echo ${value:-a;b}",
            "echo a\\;b",
            "echo $'a && b; c'",
            "echo $'a\\'&&b'",
        ] {
            let source = format!("{inner} && next");
            assert_eq!(commands(&source), vec![(inner, true), ("next", false)]);
        }
    }

    #[test]
    fn splits_ansi_c_quoted_command() {
        let source = r#"rm -f clip/*; t(){ tmux send-keys -t p -l "$1"; }; t $'\e[<0;162;59M'; sleep 0.1 && grep -c $'\e\[7m'"#;
        assert_eq!(
            commands(source),
            vec![
                ("rm -f clip/*", false),
                (r#"t(){ tmux send-keys -t p -l "$1"; }"#, false),
                (r"t $'\e[<0;162;59M'", false),
                ("sleep 0.1", true),
                (r"grep -c $'\e\[7m'", false),
            ]
        );
    }

    #[test]
    fn keeps_other_operators_inline() {
        let source = "a || b | c & d && e; f\ng";
        assert_eq!(
            commands(source),
            vec![
                ("a || b | c & d", true),
                ("e", false),
                ("f", false),
                ("g", false)
            ]
        );
    }

    #[test]
    fn keeps_and_chain_across_newline() {
        assert_eq!(commands("a &&\n b"), vec![("a", true), ("b", false)]);
    }

    #[test]
    fn protects_heredoc_bodies() {
        let source = "cat <<'EOF'\na && b; c\nEOF\necho done && next";
        assert_eq!(
            commands(source),
            vec![
                ("cat <<'EOF'\na && b; c\nEOF", false),
                ("echo done", true),
                ("next", false)
            ]
        );
        let source = "cat <<-EOF\n\ta && b; c\n\tEOF\nnext";
        assert_eq!(commands(source).len(), 2);
    }

    #[test]
    fn malformed_or_compound_shell_falls_back() {
        for source in [
            "echo 'a && b",
            "echo $(a && b",
            "a &&",
            "a ;; b",
            "a &&& b",
            "a ||",
            "a |",
            "a && # comment\n b",
            "X=1 if true; then a; fi",
            "cat <<EOF\na && b",
            "if true; then a; fi",
            "for x in a; do b; done",
            "case x in x) a;; esac",
            "a; )",
        ] {
            assert_eq!(split(source), None, "{source}");
        }
    }
}
