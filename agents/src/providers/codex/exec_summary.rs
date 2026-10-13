/// Commands observed in rollouts whose first positional argument selects an action.
const SUBCOMMANDS: &[&str] = &[
    "git",
    "cargo",
    "gh",
    "npm",
    "yarn",
    "docker",
    "codex",
    "claude",
    "brew",
    "tmux",
    "rustup",
    "uv",
    "bun",
    "xcrun",
    "adb",
    "wrangler",
    "vercel",
    "gcloud",
    "launchctl",
    "defaults",
    "security",
];

#[derive(Debug, Clone)]
struct Token {
    text: String,
    quoted: bool,
    dynamic: bool,
}

/// Summarize tool calls without executing JavaScript or shell code.
pub(super) fn summarize(input: &str) -> String {
    parse(input).unwrap_or_else(|| {
        let preview: String = input
            .chars()
            .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
            .take(512)
            .collect();
        if input.trim().is_empty() {
            "exec".into()
        } else {
            format!("exec {preview}")
        }
    })
}

#[derive(Default)]
struct Commands(Vec<(String, Vec<String>)>);
impl Commands {
    fn add(&mut self, name: &str, sub: Option<&str>) {
        let index = self
            .0
            .iter()
            .position(|(n, _)| n == name)
            .unwrap_or_else(|| {
                self.0.push((name.into(), vec![]));
                self.0.len() - 1
            });
        if let Some(sub) = sub
            && !self.0[index].1.iter().any(|s| s == sub)
        {
            self.0[index].1.push(sub.into());
        }
    }
    fn finish(self) -> Option<String> {
        if self.0.is_empty() {
            return None;
        }
        Some(
            self.0
                .into_iter()
                .map(|(name, subs)| match subs.len() {
                    0 => name,
                    1 => format!("{name} {}", subs[0]),
                    _ => format!("{name} {{{}}}", subs.join(",")),
                })
                .collect::<Vec<_>>()
                .join(", "),
        )
    }
}

fn parse(input: &str) -> Option<String> {
    let tokens = javascript(input)?;
    let mut commands = Commands::default();
    for (i, token) in tokens.iter().enumerate() {
        if token.quoted || token.text != "tools" {
            continue;
        }
        let tail = &tokens[i + 1..];
        if tail.len() < 3 || tail[0].text != "." || tail[2].text != "(" {
            continue;
        }
        let name = &tail[1].text;
        if name != "exec_command" {
            commands.add(name, None);
            continue;
        }
        let bindings = bindings(&tokens[..i]);
        let mut arg = i + 4;
        let object = value(&tokens, &mut arg, &bindings, 0);
        if let Some(objects) = object {
            for object in objects {
                shell(object.get("cmd")?.as_str()?, &mut commands, 0)?;
            }
        } else {
            // Other argument fields may be dynamic; only cmd must be static.
            let mut nesting = 1;
            let mut found = false;
            for (j, t) in tail.iter().enumerate().skip(3) {
                if !t.quoted {
                    match t.text.as_str() {
                        "(" => nesting += 1,
                        ")" => nesting -= 1,
                        _ => {}
                    }
                }
                if nesting == 0 {
                    break;
                }
                if t.text != "cmd" {
                    continue;
                }
                let mut end = i + 1 + j;
                if tail.get(j + 1).is_some_and(|t| t.text == ":") {
                    end += 2;
                }
                for cmd in value(&tokens, &mut end, &bindings, 0)? {
                    shell(cmd.as_str()?, &mut commands, 0)?;
                }
                if !tokens
                    .get(end)
                    .is_some_and(|t| matches!(t.text.as_str(), "," | "}"))
                {
                    return None;
                }
                found = true;
            }
            if !found {
                return None;
            }
        }
    }
    commands.finish()
}

type Bindings = std::collections::HashMap<String, Vec<serde_json::Value>>;

// Resolve literal data and the bindings of array callbacks, never evaluate JS.
fn value(
    tokens: &[Token],
    index: &mut usize,
    bindings: &Bindings,
    depth: usize,
) -> Option<Vec<serde_json::Value>> {
    use serde_json::Value;
    if depth > 64 {
        return None;
    }
    let token = tokens.get(*index)?;
    *index += 1;
    let mut values = if token.quoted {
        if token.dynamic {
            return None;
        }
        vec![Value::String(token.text.clone())]
    } else if token.text == "[" {
        let mut array = Vec::new();
        while tokens.get(*index)?.text != "]" {
            let values = value(tokens, index, bindings, depth + 1)?;
            if values.len() != 1 {
                return None;
            }
            array.push(values.into_iter().next()?);
            if tokens.get(*index)?.text == "," {
                *index += 1;
            } else {
                break;
            }
        }
        if tokens.get(*index)?.text != "]" {
            return None;
        }
        *index += 1;
        vec![Value::Array(array)]
    } else if token.text == "{" {
        let mut object = serde_json::Map::new();
        while tokens.get(*index)?.text != "}" {
            let key = tokens.get(*index)?.text.clone();
            *index += 1;
            if tokens.get(*index)?.text == ":" {
                *index += 1;
            } else {
                *index -= 1;
            }
            let values = value(tokens, index, bindings, depth + 1)?;
            if values.len() != 1 {
                return None;
            }
            object.insert(key, values.into_iter().next()?);
            if tokens.get(*index)?.text == "," {
                *index += 1;
            } else {
                break;
            }
        }
        if tokens.get(*index)?.text != "}" {
            return None;
        }
        *index += 1;
        vec![Value::Object(object)]
    } else if let Ok(number) = token.text.parse::<i64>() {
        vec![Value::from(number)]
    } else if matches!(token.text.as_str(), "true" | "false" | "null") {
        vec![serde_json::from_str(&token.text).ok()?]
    } else {
        bindings.get(&token.text)?.clone()
    };
    while tokens.get(*index).is_some_and(|t| t.text == ".") {
        let key = &tokens.get(*index + 1)?.text;
        values = values
            .into_iter()
            .map(|v| v.get(key).cloned())
            .collect::<Option<_>>()?;
        *index += 2;
    }
    Some(values)
}

fn bindings(tokens: &[Token]) -> Bindings {
    let mut bindings = Bindings::new();
    for i in 0..tokens.len() {
        // Literal declarations preceding this call.
        if tokens[i].text == "=" && i > 1 && tokens[i - 2].text == "const" && !tokens[i - 1].quoted
        {
            let mut end = i + 1;
            if let Some(values) = value(tokens, &mut end, &bindings, 0)
                && tokens
                    .get(end)
                    .is_none_or(|t| matches!(t.text.as_str(), ";" | "," | ")"))
            {
                bindings.insert(tokens[i - 1].text.clone(), values);
            }
        }
        if tokens[i].text == "."
            && tokens
                .get(i + 1)
                .is_some_and(|t| matches!(t.text.as_str(), "map" | "forEach"))
            && i > 0
        {
            let Some(arrays) = bindings.get(&tokens[i - 1].text).cloned() else {
                continue;
            };
            let Some(rows) = arrays.first().and_then(serde_json::Value::as_array) else {
                continue;
            };
            let mut j = i + 3;
            if tokens.get(j).is_some_and(|t| t.text == "async") {
                j += 1;
            }
            if tokens.get(j).is_some_and(|t| t.text == "(") {
                j += 1;
            }
            if tokens.get(j).is_some_and(|t| t.text == "[") {
                j += 1;
                let mut column = 0;
                while let Some(t) = tokens.get(j) {
                    if t.text == "]" {
                        break;
                    }
                    if t.text == "," {
                        column += 1;
                    } else if let Some(values) = rows
                        .iter()
                        .map(|row| row.get(column).cloned())
                        .collect::<Option<Vec<_>>>()
                    {
                        bindings.insert(t.text.clone(), values);
                    }
                    j += 1;
                }
            } else if let Some(t) = tokens.get(j) {
                bindings.insert(t.text.clone(), rows.clone());
            }
        }
        if tokens[i].text == "of"
            && i > 0
            && let Some(name) = tokens.get(i + 1)
            && let Some(rows) = bindings
                .get(&name.text)
                .and_then(|values| values.first())
                .and_then(serde_json::Value::as_array)
                .cloned()
        {
            bindings.insert(tokens[i - 1].text.clone(), rows);
        }
    }
    bindings
}

// Strings and comments are opaque, so embedded source text cannot become a tool call.
fn javascript(input: &str) -> Option<Vec<Token>> {
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();
    let mut delimiters = Vec::new();
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c == '/' && chars.get(i + 1) == Some(&'/') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if c == '/' && chars.get(i + 1) == Some(&'*') {
            i += 2;
            while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                i += 1;
            }
            if i + 1 == chars.len() {
                return None;
            }
            i += 2;
            continue;
        }
        if c == '/'
            && out.last().is_none_or(|t: &Token| {
                !t.quoted
                    && matches!(
                        t.text.as_str(),
                        "=" | "(" | "[" | "{" | ":" | "," | "!" | ";" | "return" | ">" | "&" | "|"
                    )
            })
        {
            i += 1;
            let mut class = false;
            while i < chars.len() {
                match chars[i] {
                    '\\' => {
                        i += 2;
                        continue;
                    }
                    '[' => class = true,
                    ']' => class = false,
                    '/' if !class => break,
                    '\n' => return None,
                    _ => {}
                }
                i += 1;
            }
            if i >= chars.len() {
                return None;
            }
            i += 1;
            while chars.get(i).is_some_and(|c| c.is_ascii_alphabetic()) {
                i += 1;
            }
            out.push(Token {
                text: String::new(),
                quoted: true,
                dynamic: true,
            });
            continue;
        }
        if matches!(c, '\'' | '"' | '`') {
            let quote = c;
            i += 1;
            let mut text = String::new();
            while i < chars.len() && chars[i] != quote {
                if chars[i] == '\\' {
                    i += 1;
                    let escaped = *chars.get(i)?;
                    match escaped {
                        'n' => text.push('\n'),
                        'r' => text.push('\r'),
                        't' => text.push('\t'),
                        '\n' => {}
                        'u' | 'x' => {
                            let count = if escaped == 'u' { 4 } else { 2 };
                            let digits: String = chars.get(i + 1..i + 1 + count)?.iter().collect();
                            text.push(char::from_u32(u32::from_str_radix(&digits, 16).ok()?)?);
                            i += count;
                        }
                        c => text.push(c),
                    }
                } else {
                    text.push(chars[i]);
                }
                i += 1;
            }
            if i == chars.len() {
                return None;
            }
            i += 1;
            out.push(Token {
                dynamic: quote == '`' && text.contains("${"),
                text,
                quoted: true,
            });
        } else if c.is_alphanumeric() || matches!(c, '_' | '$') {
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || matches!(chars[i], '_' | '$')) {
                i += 1;
            }
            out.push(Token {
                text: chars[start..i].iter().collect(),
                quoted: false,
                dynamic: false,
            });
        } else {
            match c {
                '(' | '[' | '{' => delimiters.push(c),
                ')' | ']' | '}' => {
                    let expected = match c {
                        ')' => '(',
                        ']' => '[',
                        _ => '{',
                    };
                    if delimiters.pop() != Some(expected) {
                        return None;
                    }
                }
                _ => {}
            }
            out.push(Token {
                text: c.to_string(),
                quoted: false,
                dynamic: false,
            });
            i += 1;
        }
    }
    delimiters.is_empty().then_some(out)
}

fn assignment(word: &str) -> bool {
    word.split_once('=').is_some_and(|(name, _)| {
        !name.is_empty()
            && name
                .chars()
                .enumerate()
                .all(|(i, c)| c == '_' || c.is_ascii_alphabetic() || (i > 0 && c.is_ascii_digit()))
    })
}

fn option_value(command: &str, option: &str) -> bool {
    match command {
        "git" => matches!(
            option,
            "-C" | "-c"
                | "--git-dir"
                | "--work-tree"
                | "--namespace"
                | "--config-env"
                | "--exec-path"
        ),
        "cargo" => matches!(
            option,
            "--color" | "--config" | "-Z" | "--manifest-path" | "--target-dir"
        ),
        "gh" => matches!(option, "--repo" | "-R" | "--hostname"),
        "npm" | "pnpm" | "yarn" => matches!(
            option,
            "--prefix"
                | "--workspace"
                | "-w"
                | "--cwd"
                | "--dir"
                | "-C"
                | "--filter"
                | "-F"
                | "--registry"
                | "--cache"
                | "--userconfig"
        ),
        "docker" => matches!(
            option,
            "--context" | "-c" | "--host" | "-H" | "--config" | "--log-level" | "-l"
        ),
        "kubectl" => matches!(
            option,
            "--context"
                | "--namespace"
                | "-n"
                | "--kubeconfig"
                | "--cluster"
                | "--user"
                | "--server"
                | "-s"
                | "--token"
        ),
        "tmux" => matches!(option, "-L" | "-S" | "-f" | "-T"),
        "codex" => matches!(
            option,
            "-C" | "--cd"
                | "-c"
                | "--config"
                | "-m"
                | "--model"
                | "-p"
                | "--profile"
                | "-s"
                | "--sandbox"
                | "-a"
                | "--ask-for-approval"
                | "--add-dir"
        ),
        "adb" => matches!(option, "-s" | "-t" | "-H" | "-P" | "-L"),
        "xcrun" => matches!(option, "--sdk" | "-sdk" | "--toolchain" | "-toolchain"),
        "uv" => matches!(
            option,
            "--directory" | "--project" | "--config-file" | "--cache-dir" | "--python"
        ),
        "wrangler" => matches!(option, "-c" | "--config" | "-e" | "--env" | "--cwd"),
        "vercel" => matches!(
            option,
            "-t" | "--token"
                | "-S"
                | "--scope"
                | "--cwd"
                | "-A"
                | "--local-config"
                | "--global-config"
        ),
        "gcloud" => matches!(
            option,
            "--project"
                | "--configuration"
                | "--account"
                | "--impersonate-service-account"
                | "--format"
                | "--flags-file"
        ),
        "claude" => matches!(
            option,
            "--model"
                | "--permission-mode"
                | "--settings"
                | "--allowedTools"
                | "--disallowedTools"
                | "--add-dir"
        ),
        _ => false,
    }
}

fn simple(words: &[String], out: &mut Commands, depth: usize) -> Option<()> {
    if words
        .first()
        .is_some_and(|w| matches!(w.as_str(), "for" | "select" | "case" | "function"))
    {
        return Some(());
    }
    let mut i = 0;
    while let Some(word) = words.get(i) {
        if assignment(word)
            || matches!(
                word.as_str(),
                "!" | "then" | "do" | "else" | "if" | "elif" | "while" | "until" | "{" | "}"
            )
        {
            i += 1;
            continue;
        }
        if matches!(word.as_str(), "fi" | "done" | "esac" | "in") {
            return Some(());
        }
        let name = word.rsplit('/').next()?;
        if name == "command"
            && words
                .get(i + 1)
                .is_some_and(|w| matches!(w.as_str(), "-v" | "-V"))
        {
            out.add(name, None);
            return Some(());
        }
        match name {
            "env" | "sudo" | "time" | "command" | "builtin" | "nohup" | "timeout" | "xargs" => {
                i += 1;
                while let Some(arg) = words.get(i) {
                    if arg == "--" {
                        i += 1;
                        break;
                    }
                    if !arg.starts_with('-') {
                        break;
                    }
                    let takes_value = match name {
                        "env" => matches!(
                            arg.as_str(),
                            "-u" | "--unset" | "-C" | "--chdir" | "-S" | "--split-string"
                        ),
                        "sudo" => matches!(
                            arg.as_str(),
                            "-u" | "-g" | "-h" | "-p" | "-C" | "-T" | "-R" | "-D"
                        ),
                        "timeout" => {
                            matches!(arg.as_str(), "-s" | "--signal" | "-k" | "--kill-after")
                        }
                        "xargs" => matches!(
                            arg.as_str(),
                            "-I" | "-n"
                                | "-P"
                                | "-L"
                                | "-d"
                                | "-E"
                                | "-s"
                                | "--replace"
                                | "--max-args"
                                | "--max-procs"
                        ),
                        "time" => matches!(arg.as_str(), "-o" | "-f" | "--output" | "--format"),
                        _ => false,
                    };
                    i += 1 + usize::from(takes_value && !arg.contains('='));
                }
                if name == "timeout" {
                    i += 1;
                }
                continue;
            }
            "bash" | "sh" | "zsh" => {
                if let Some(pos) = words[i + 1..]
                    .iter()
                    .position(|w| w.starts_with('-') && !w.starts_with("--") && w.contains('c'))
                {
                    return shell(words.get(i + 2 + pos)?, out, depth + 1);
                }
            }
            _ => {}
        }
        let sub = if SUBCOMMANDS.contains(&name) {
            let mut j = i + 1;
            while let Some(arg) = words.get(j) {
                if arg == "--" {
                    j += 1;
                    break;
                }
                if !arg.starts_with('-') && !arg.starts_with('+') {
                    break;
                }
                j += 1 + usize::from(option_value(name, arg) && !arg.contains('='));
            }
            words.get(j).map(String::as_str)
        } else {
            None
        };
        out.add(name, sub);
        return Some(());
    }
    Some(())
}

// Scan shell words, preserving quoted scripts and separating only shell operators.
fn shell(input: &str, out: &mut Commands, depth: usize) -> Option<()> {
    if depth > 64 {
        return None;
    }
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    let mut redirect_target = false;
    let mut nested = Vec::new();
    let mut heredocs: Vec<(String, bool)> = Vec::new();
    while i < chars.len() {
        let c = chars[i];
        if quote.is_none()
            && c == '<'
            && chars.get(i + 1) == Some(&'<')
            && chars.get(i + 2) != Some(&'<')
            && (i == 0 || chars[i - 1] != '<')
        {
            i += 2;
            let strip_tabs = chars.get(i) == Some(&'-');
            if strip_tabs {
                i += 1;
            }
            while chars.get(i).is_some_and(|c| matches!(c, ' ' | '\t')) {
                i += 1;
            }
            let mut delimiter = String::new();
            let mut delimiter_quote = None;
            while let Some(&c) = chars.get(i) {
                if delimiter_quote.is_none() && (c.is_whitespace() || matches!(c, ';' | '&' | '|'))
                {
                    break;
                }
                if matches!(c, '\'' | '"')
                    && (delimiter_quote.is_none() || delimiter_quote == Some(c))
                {
                    delimiter_quote = if delimiter_quote.is_some() {
                        None
                    } else {
                        Some(c)
                    };
                } else if c == '\\' && delimiter_quote != Some('\'') {
                    i += 1;
                    delimiter.push(*chars.get(i)?);
                } else {
                    delimiter.push(c);
                }
                i += 1;
            }
            if delimiter.is_empty() || delimiter_quote.is_some() {
                return None;
            }
            heredocs.push((delimiter, strip_tabs));
            continue;
        }
        if quote.is_none()
            && (matches!(c, '<' | '>') || (c == '&' && chars.get(i + 1) == Some(&'>')))
        {
            if !word.is_empty() && !word.chars().all(|c| c.is_ascii_digit()) && !redirect_target {
                words.push(std::mem::take(&mut word));
            } else {
                word.clear();
            }
            if c == '&' {
                i += 1;
            }
            let operator = chars[i];
            while chars.get(i) == Some(&operator) {
                i += 1;
            }
            if chars.get(i) == Some(&'&') {
                i += 1;
            }
            redirect_target = true;
            continue;
        }
        if c == '\\' && quote != Some('\'') {
            i += 1;
            let c = *chars.get(i)?;
            if c != '\n' {
                word.push(c);
            }
        } else if matches!(c, '\'' | '"') && (quote.is_none() || quote == Some(c)) {
            quote = if quote.is_some() { None } else { Some(c) };
        } else if quote != Some('\'') && (c == '`' || (c == '$' && chars.get(i + 1) == Some(&'(')))
        {
            let backtick = c == '`';
            i += if backtick { 1 } else { 2 };
            let start = i;
            let mut balance = 1;
            let mut inner_quote = None;
            while i < chars.len() {
                let current = chars[i];
                if current == '\\' {
                    i += 2;
                    continue;
                }
                if backtick && current == '`' {
                    balance = 0;
                    break;
                }
                if matches!(current, '\'' | '"')
                    && (inner_quote.is_none() || inner_quote == Some(current))
                {
                    inner_quote = if inner_quote.is_some() {
                        None
                    } else {
                        Some(current)
                    };
                }
                if !backtick && inner_quote.is_none() {
                    if current == '(' {
                        balance += 1;
                    }
                    if current == ')' {
                        balance -= 1;
                    }
                    if balance == 0 {
                        break;
                    }
                }
                i += 1;
            }
            if balance != 0 {
                return None;
            }
            nested.push(chars[start..i].iter().collect::<String>());
            word.push_str("$()");
        } else if quote.is_none() && (c.is_whitespace() || matches!(c, ';' | '&' | '|' | '(' | ')'))
        {
            if !word.is_empty() {
                if redirect_target {
                    word.clear();
                    redirect_target = false;
                } else {
                    words.push(std::mem::take(&mut word));
                }
            }
            if matches!(c, ';' | '&' | '|' | '\n' | '(' | ')') {
                simple(&words, out, depth)?;
                for script in nested.drain(..) {
                    shell(&script, out, depth + 1)?;
                }
                words.clear();
                if c == '\n' {
                    for (delimiter, strip_tabs) in heredocs.drain(..) {
                        i += 1;
                        loop {
                            let start = i;
                            while i < chars.len() && chars[i] != '\n' {
                                i += 1;
                            }
                            let line: String = chars[start..i].iter().collect();
                            if (if strip_tabs {
                                line.trim_start_matches('\t')
                            } else {
                                &line
                            }) == delimiter
                            {
                                break;
                            }
                            if i == chars.len() {
                                return None;
                            }
                            i += 1;
                        }
                    }
                }
            }
        } else if quote.is_none() && c == '#' && word.is_empty() {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        } else {
            word.push(c);
        }
        i += 1;
    }
    if quote.is_some() || !heredocs.is_empty() {
        return None;
    }
    if !word.is_empty() && !redirect_target {
        words.push(word);
    }
    simple(&words, out, depth)?;
    for script in nested {
        shell(&script, out, depth + 1)?;
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn commands_and_nested_shells() {
        assert_eq!(
            summarize(
                r#"text(await tools.exec_command({cmd:"cat file; git -C dir status && git branch | head; echo $(pwd); bash -lc 'env X=y cargo test'"}));"#
            ),
            "cat, git {status,branch}, head, echo, pwd, cargo test"
        );
    }
    #[test]
    fn other_tools_and_fallback() {
        assert_eq!(
            summarize(
                r#"text(await tools.write_stdin({session_id:1})); image(await tools.view_image({path:"x"}));"#
            ),
            "write_stdin, view_image"
        );
        assert_eq!(summarize("broken ("), "exec broken (");
    }
    #[test]
    fn heredoc_body_is_data() {
        assert_eq!(
            summarize(
                r#"text(await tools.exec_command({cmd:"python3 - <<'PY'\nprint('not a shell command')\nPY\ngit status"}));"#
            ),
            "python3, git status"
        );
    }
    #[test]
    fn wrappers_options_and_arbitrary_subcommands() {
        assert_eq!(
            summarize(
                r#"text(await tools.exec_command({cmd:"sudo -u root timeout 5 env A=b git --git-dir repo arbitrary-action; printf x | xargs -I {} cargo +stable custom-action; time zsh -c 'cd dir; rg x | head'; (pwd); echo `uname`"}));"#
            ),
            "git arbitrary-action, printf, cargo custom-action, cd, rg, head, pwd, echo, uname"
        );
    }
    #[test]
    fn scrubbed_rollout_examples() {
        let cases: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/codex-exec-summary.json"
        ))
        .unwrap();
        for case in cases.as_array().unwrap() {
            let input = case["input"].as_str().unwrap();
            assert_eq!(
                summarize(input),
                case["expected"].as_str().unwrap(),
                "{input}"
            );
        }
    }
    #[test]
    fn static_command_bindings() {
        for input in [
            r#"const cmds = ["git status", "git arbitrary", "pwd"]; const rs = await Promise.all(cmds.map(cmd => tools.exec_command({cmd, max_output_tokens:1})));"#,
            r#"const jobs = [["a", "git status"], ["b", "git arbitrary"], ["c", "pwd"]]; await Promise.all(jobs.map(([name, cmd]) => tools.exec_command({cmd})));"#,
            r#"const jobs = [{cmd:"git status"}, {cmd:"git arbitrary"}, {cmd:"pwd"}]; await Promise.all(jobs.map(j => tools.exec_command(j)));"#,
        ] {
            assert_eq!(summarize(input), "git {status,arbitrary}, pwd");
        }
    }
    #[test]
    fn javascript_regex_and_dynamic_templates() {
        assert_eq!(
            summarize(
                r#"const pattern = /[)]/g; text(await tools.exec_command({cmd:"git status"}));"#
            ),
            "git status"
        );
        let input = r#"text(await tools.exec_command({cmd:`git ${action}`}));"#;
        assert_eq!(summarize(input), format!("exec {input}"));
    }
    #[test]
    fn redirects_and_loop_headers_are_not_commands() {
        assert_eq!(
            summarize(
                r#"text(await tools.exec_command({cmd: ">out git -C dir status 2>&1; printf x &>/dev/null; for f in *.rs; do rg x \"$f\"; done"}));"#
            ),
            "git status, printf, rg"
        );
    }
    #[test]
    #[ignore = "rechecks an existing local report without rereading rollouts"]
    fn review_report() {
        let path = std::env::var("EXEC_SUMMARY_REPORT").unwrap();
        let mut records: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let mut fallbacks = 0;
        for record in records.as_array_mut().unwrap() {
            let input = record["input"].as_str().unwrap();
            let result = parse(input);
            fallbacks += usize::from(result.is_none());
            record["output"] = serde_json::json!(summarize(input));
            record["fallback"] = serde_json::json!(result.is_none());
        }
        std::fs::write(path, serde_json::to_vec(&records).unwrap()).unwrap();
        eprintln!("fallbacks: {fallbacks}");
    }
    #[test]
    #[ignore = "reads local rollouts; writes its report only to EXEC_SUMMARY_REPORT"]
    fn real_rollouts() {
        use std::{
            fs,
            io::{BufRead, BufReader},
            path::{Path, PathBuf},
        };
        fn files(path: &Path, out: &mut Vec<PathBuf>) {
            for entry in fs::read_dir(path).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    files(&path, out);
                } else if path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("rollout-")
                    && path.extension().is_some_and(|e| e == "jsonl")
                {
                    out.push(path);
                }
            }
        }
        let home = std::env::var_os("CODEX_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap()).join(".codex"));
        let mut paths = vec![];
        files(&home.join("sessions"), &mut paths);
        paths.sort();
        let mut records = vec![];
        let mut fallbacks = 0;
        for path in &paths {
            for line in BufReader::new(fs::File::open(path).unwrap()).lines() {
                let Ok(record) = serde_json::from_str::<serde_json::Value>(&line.unwrap()) else {
                    continue;
                };
                let payload = &record["payload"];
                if record["type"] != "response_item"
                    || payload["type"] != "custom_tool_call"
                    || payload["name"] != "exec"
                {
                    continue;
                }
                let input = payload["input"].as_str().unwrap();
                let output =
                    std::panic::catch_unwind(|| summarize(input)).expect("summary panicked");
                let fallback = parse(input).is_none();
                fallbacks += usize::from(fallback);
                records
                    .push(serde_json::json!({"input":input,"output":output,"fallback":fallback}));
            }
        }
        let report = std::env::var_os("EXEC_SUMMARY_REPORT")
            .expect("set EXEC_SUMMARY_REPORT outside ~/.codex");
        assert!(!Path::new(&report).starts_with(&home));
        fs::write(report, serde_json::to_vec(&records).unwrap()).unwrap();
        eprintln!(
            "rollouts={} inputs={} fallbacks={fallbacks}",
            paths.len(),
            records.len()
        );
        assert!(!records.is_empty());
        assert_eq!(
            fallbacks, 0,
            "some valid exec scripts cannot be summarized statically; see the report"
        );
    }
}
