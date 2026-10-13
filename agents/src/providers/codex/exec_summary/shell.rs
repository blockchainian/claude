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

#[derive(Default)]
pub(super) struct Commands(Vec<(String, Vec<String>)>);
impl Commands {
    pub(super) fn add(&mut self, name: &str, sub: Option<&str>) {
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
    pub(super) fn finish(self) -> Option<String> {
        if self.0.is_empty() {
            return None;
        }
        Some(
            self.0
                .into_iter()
                .map(|(name, subs)| match subs.len() {
                    0 => name,
                    1 => format!("{name} {}", subs[0]),
                    _ => format!("{name} {{{}}}", subs.join(", ")),
                })
                .collect::<Vec<_>>()
                .join(", "),
        )
    }
}

/// Return command names in first-appearance order; an empty result means unsupported shell syntax.
pub(super) fn summarize_shell_commands(cmd: &str) -> String {
    let mut commands = Commands::default();
    shell(cmd, &mut commands, 0)
        .and_then(|()| commands.finish())
        .unwrap_or_default()
}

pub(super) fn collect(cmd: &str, commands: &mut Commands) -> Option<()> {
    shell(cmd, commands, 0)
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
        if name.contains('$') {
            return None;
        }
        if name == "command"
            && words
                .get(i + 1)
                .is_some_and(|w| matches!(w.as_str(), "-v" | "-V"))
        {
            out.add(name, None);
            return Some(());
        }
        match name {
            "env" | "sudo" | "time" | "command" | "builtin" | "nohup" | "timeout" | "gtimeout"
            | "xargs" => {
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
                if matches!(name, "timeout" | "gtimeout") {
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
        if sub.is_some_and(|s| s.contains('$')) {
            return None;
        }
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
