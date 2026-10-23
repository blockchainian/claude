use super::describe::{Segment, Size};
use super::shape::Kind;
use super::{shell_summary, syntax};
use syntect::parsing::Regex;

// An address returns a finite line number only for literal numeric addresses.
fn sed_address(source: &str) -> Option<(&str, Option<usize>)> {
    let source = source.trim_start();
    let bytes = source.as_bytes();
    let first = *bytes.first()?;
    let (mut end, mut number) = if first.is_ascii_digit() {
        let end = bytes.iter().take_while(|b| b.is_ascii_digit()).count();
        (end, source[..end].parse().ok())
    } else if first == b'$' {
        let end = match shell_summary::address_expansion(source) {
            // `$p` is sed's last-line address followed by its print command.
            Some(2)
                if b"pPdDqQnNhHwWbtT".contains(&bytes[1])
                    && (source.len() == 2
                        || source[2..].starts_with([';', '\n', '}', ' ', '\t'])) =>
            {
                1
            }
            Some(end) => end,
            None => 1,
        };
        (end, None)
    } else if first == b'/' || first == b'\\' {
        let start = usize::from(first == b'\\');
        let delimiter = source[start..].chars().next()?;
        let mut i = start + delimiter.len_utf8();
        let mut class = None;
        loop {
            let byte = *bytes.get(i)?;
            if byte == b'\\' {
                i += 1;
                i += source[i..].chars().next()?.len_utf8();
                continue;
            }
            if source[i..].starts_with(delimiter) && class.is_none() {
                break;
            }
            if let Some(start) = class {
                if byte == b'[' && matches!(bytes.get(i + 1), Some(b':' | b'.' | b'=')) {
                    let marker = bytes[i + 1];
                    i += 2;
                    while !(bytes.get(i) == Some(&marker) && bytes.get(i + 1) == Some(&b']')) {
                        bytes.get(i)?;
                        i += 1;
                    }
                    i += 2;
                    continue;
                }
                // An initial ] (also after ^) belongs to the character class.
                if byte == b']' && i != start && !(i == start + 1 && bytes[start] == b'^') {
                    class = None;
                }
            } else if byte == b'[' {
                class = Some(i + 1);
            }
            i += source[i..].chars().next()?.len_utf8();
        }
        i += delimiter.len_utf8();
        while matches!(bytes.get(i), Some(b'I' | b'M')) {
            i += 1;
        }
        (i, None)
    } else {
        return None;
    };
    if bytes.get(end) == Some(&b'~') {
        let start = end + 1;
        end = start
            + bytes[start..]
                .iter()
                .take_while(|b| b.is_ascii_digit())
                .count();
        if end == start {
            return None;
        }
        number = None;
    }
    Some((&source[end..], number))
}

/// Recognize sed scripts whose commands only select unchanged input lines.
pub(super) fn sed(words: &[String]) -> Option<Segment> {
    let mut scripts = Vec::new();
    let mut files = Vec::new();
    let mut quiet = false;
    let mut options = true;
    let mut args = words.iter().skip(1);
    while let Some(word) = args.next() {
        if options && word == "--" {
            options = false;
        } else if options && word.starts_with("--") {
            let (name, value) = word
                .split_once('=')
                .map_or((word.as_str(), None), |(k, v)| (k, Some(v)));
            match name {
                "--file" => return None,
                "--expression" => scripts.push(value.or_else(|| args.next().map(String::as_str))?),
                "--quiet" | "--silent" => quiet = true,
                _ => {}
            }
        } else if options && word.starts_with('-') && word != "-" {
            for (index, letter) in word[1..].char_indices() {
                match letter {
                    'f' => return None,
                    'e' => {
                        let value = &word[index + 2..];
                        scripts.push(if value.is_empty() {
                            args.next()?.as_str()
                        } else {
                            value
                        });
                        break;
                    }
                    'n' => quiet = true,
                    'i' => break, // Any attached suffix belongs to -i.
                    _ => {}
                }
            }
        } else {
            files.push(word.as_str());
        }
    }
    if scripts.is_empty() {
        if files.is_empty() {
            return None;
        }
        scripts.push(files.remove(0));
    }
    let mut cap = quiet.then_some(0usize);
    let beginning = files.len() == 1
        && scripts.len() == 1
        && Regex::new(r"\A1(?:,[0-9]+)?p\z".into()).is_match(scripts[0].trim());
    for script in scripts {
        let mut rest = script;
        loop {
            rest = rest.trim_start_matches(|c: char| c.is_whitespace() || c == ';');
            if rest.is_empty() {
                break;
            }
            let mut bound = None;
            if let Some((tail, first)) = sed_address(rest) {
                rest = tail.trim_start();
                bound = first.map(|_| 1usize);
                if let Some(tail) = rest.strip_prefix(',') {
                    let tail = tail.trim_start();
                    let (tail, last) = if tail.starts_with(['+', '~']) {
                        let (tail, _) = sed_address(&tail[1..])?;
                        (tail, None)
                    } else {
                        sed_address(tail)?
                    };
                    rest = tail.trim_start();
                    bound = first
                        .zip(last)
                        .and_then(|(a, b)| b.checked_sub(a)?.checked_add(1));
                }
            }
            if let Some(tail) = rest.strip_prefix('!') {
                rest = tail.trim_start();
                bound = None;
            }
            let command = rest.chars().next()?;
            rest = &rest[command.len_utf8()..];
            match command {
                'p' | 'P' | 'd' | 'D' | 'q' | 'Q' | 'n' | 'N' | 'b' | 't' | 'T' | ':' | '{'
                | '}' | 'w' | 'W' | 'h' | 'H' => {}
                _ => return None,
            }
            cap = if command == 'p' {
                cap.zip(bound).and_then(|(a, b)| a.checked_add(b))
            } else {
                None
            };
            if matches!(command, 'b' | 't' | 'T' | ':' | 'w' | 'W') {
                rest = rest.trim_start_matches([' ', '\t']);
                let end = rest.find([';', '\n']).unwrap_or(rest.len());
                rest = &rest[end..];
            } else if matches!(command, 'q' | 'Q') {
                rest = rest.trim_start_matches([' ', '\t']);
                rest = rest.trim_start_matches(|c: char| c.is_ascii_digit());
            }
        }
    }
    let kind = if let Some(&path) = files.first() {
        if files.iter().any(|p| p.contains(['$', '`'])) {
            return None;
        }
        let scope = syntax(path);
        if files.len() > 1 && (scope.is_none() || files.iter().any(|p| syntax(p) != scope)) {
            return None;
        }
        Kind::File {
            path: path.into(),
            numbered: false,
        }
    } else {
        Kind::Plain
    };
    Some(Segment {
        size: if files.len() > 1 {
            Size::Unknown
        } else {
            cap.map_or(Size::Unknown, Size::AtMost)
        },
        kind,
        beginning,
        ..Segment::unknown()
    })
}
