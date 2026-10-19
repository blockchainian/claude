use std::ops::Range;

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Command {
    pub range: Range<usize>,
    pub and_next: bool,
}

/// Returns source ranges only when shell grouping is unambiguous.
pub(crate) fn split(source: &str) -> Option<Vec<Command>> {
    let bytes = source.as_bytes();
    let mut commands = Vec::new();
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
            Some(b'`') => {
                if byte == b'\\' {
                    index += 2;
                } else {
                    if byte == b'`' {
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
                // ANSI-C quoting has its own escape grammar.
                return None;
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
                let (end, delimiter, tabs) = heredoc_delimiter(source, index + 2)?;
                heredocs.push((delimiter, tabs));
                index = end;
            }
            b'&' if groups.is_empty() && bytes.get(index + 1) == Some(&b'&') => {
                if !heredocs.is_empty() || bytes.get(index + 2) == Some(&b'&') {
                    return None;
                }
                push_command(source, start, index, true, &mut commands)?;
                index += 2;
                start = index;
            }
            b';' if groups.is_empty() => {
                if !heredocs.is_empty() || matches!(bytes.get(index + 1), Some(b';' | b'&')) {
                    return None;
                }
                push_command(source, start, index, false, &mut commands)?;
                index += 1;
                start = index;
            }
            b'\n' if groups.is_empty() => {
                let mut end = index;
                if !heredocs.is_empty() {
                    index += 1;
                    for (delimiter, tabs) in heredocs.drain(..) {
                        let (_, marker_end) = heredoc_body(source, index, &delimiter, tabs)?;
                        end = marker_end;
                        index = marker_end;
                    }
                }
                let command = source[start..end].trim();
                if !command.is_empty() {
                    // Newlines following pipeline operators continue that pipeline.
                    if command.ends_with('|') {
                        return None;
                    }
                    push_command(source, start, end, false, &mut commands)?;
                }
                index += usize::from(index < bytes.len());
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
        push_command(source, start, source.len(), false, &mut commands)?;
    } else if commands.last().is_some_and(|command| command.and_next) {
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
    and_next: bool,
    commands: &mut Vec<Command>,
) -> Option<()> {
    let text = source[start..end].trim();
    if text.is_empty() {
        return None;
    }
    let start = start + source[start..end].len() - source[start..end].trim_start().len();
    commands.push(Command {
        range: start..start + text.len(),
        and_next,
    });
    Some(())
}

fn heredoc_delimiter(source: &str, mut index: usize) -> Option<(usize, String, bool)> {
    let bytes = source.as_bytes();
    let tabs = bytes.get(index) == Some(&b'-');
    index += usize::from(tabs);
    while matches!(bytes.get(index), Some(b' ' | b'\t')) {
        index += 1;
    }
    let start = index;
    let mut delimiter = Vec::new();
    let mut quote = None;
    while let Some(&byte) = bytes.get(index) {
        match (quote, byte) {
            (Some(mark), byte) if mark == byte => quote = None,
            (None, b'\'' | b'"') => quote = Some(byte),
            (None, byte) if is_word_boundary(byte) => break,
            (_, b'\\') if quote != Some(b'\'') => {
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
    Some((index, String::from_utf8(delimiter).ok()?, tabs))
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

pub(super) enum SyntaxSource<'a> {
    Interpreter(&'a str),
    Extension(&'a str),
}

/// Embedded code, excluding the shell's quotes and delimiter.
pub(super) struct Script<'a> {
    pub syntax: SyntaxSource<'a>,
    pub range: Range<usize>,
}

/// Recognizes interpreter invocations and cat file heredocs without changing source bytes.
/// Compounds that `split` cannot safely group keep their shell highlighting.
pub(super) fn scripts(source: &str) -> Vec<Script<'_>> {
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
            if let Some(script) = cat_heredoc(source, index, command.range.end) {
                scripts.push(script);
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
                let Some((end, delimiter, tabs)) = heredoc_delimiter(source, index + 2) else {
                    break;
                };
                if let Some(newline) = source[end..].find('\n') {
                    let start = end + newline + 1;
                    if let Some((range, _)) = heredoc_body(source, start, &delimiter, tabs) {
                        scripts.push(Script {
                            syntax: SyntaxSource::Interpreter(interpreter),
                            range,
                        });
                    }
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

fn cat_heredoc(source: &str, mut index: usize, end: usize) -> Option<Script<'_>> {
    let bytes = source.as_bytes();
    let mut extension = None;
    let mut heredoc = None;
    while index < end {
        while matches!(bytes.get(index), Some(b' ' | b'\t')) {
            index += 1;
        }
        match bytes.get(index) {
            Some(b'>') if extension.is_none() => {
                index += 1;
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
            Some(b'<') if heredoc.is_none() && bytes.get(index + 1) == Some(&b'<') => {
                let (next, delimiter, tabs) = heredoc_delimiter(source, index + 2)?;
                index = next;
                heredoc = Some((delimiter, tabs));
            }
            Some(b'\n') => {
                let (delimiter, tabs) = heredoc?;
                let (range, _) = heredoc_body(source, index + 1, &delimiter, tabs)?;
                return Some(Script {
                    syntax: SyntaxSource::Extension(extension?),
                    range,
                });
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

    fn commands(source: &str) -> Vec<(&str, bool)> {
        split(source)
            .expect("valid shell command")
            .iter()
            .map(|command| (&source[command.range.clone()], command.and_next))
            .collect()
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
        ] {
            let source = format!("{inner} && next");
            assert_eq!(commands(&source), vec![(inner, true), ("next", false)]);
        }
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
            "echo $'a\\'&&b' && next",
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
