use std::ops::Range;

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Command {
    pub range: Range<usize>,
    pub and_next: bool,
}

/// Returns source ranges only when shell grouping is unambiguous.
pub(super) fn split(source: &str) -> Option<Vec<Command>> {
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
                        loop {
                            let relative_end = source[index..].find('\n');
                            let line_end = relative_end.map_or(bytes.len(), |n| index + n);
                            let line = &source[index..line_end];
                            let line = if tabs {
                                line.trim_start_matches('\t')
                            } else {
                                line
                            };
                            if line == delimiter {
                                end = line_end;
                                index = line_end;
                                break;
                            }
                            relative_end?;
                            index = line_end + 1;
                        }
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
