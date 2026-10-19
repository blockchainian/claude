//! Syntax highlighting of the code and diff sections an open tool row shows.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use std::path::Path;
use std::sync::{Mutex, MutexGuard, OnceLock};
use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle, Highlighter, Theme, ThemeSet};
use syntect::parsing::{Scope, SyntaxReference, SyntaxSet};
use syntect::util::LinesWithEndings;

/// Texts larger than this, or with more lines or a longer line than the limits below, are
/// drawn plain: highlighting them would stall the frame.
const MAX_BYTES: usize = 512 * 1024;
const MAX_LINES: usize = 10_000;
const MAX_LINE_BYTES: usize = 4 * 1024;

#[cfg(test)]
thread_local! {
    static HIGHLIGHT_CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    pub(super) static JSON_HIGHLIGHTED_LINES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

// Highlighted lines and embedded shell styles do not depend on panel width.
// Keep complete results within both entry and allocated-content budgets per render thread.
const CODE_CACHE_ENTRIES: usize = 32;
const CODE_CACHE_BYTES: usize = 4 * 1024 * 1024;

thread_local! {
    static CODE_CACHE: std::cell::RefCell<CodeCache> = const { std::cell::RefCell::new(CodeCache {
        entries: Vec::new(),
        bytes: 0,
    }) };
}

struct CodeCache {
    entries: Vec<CodeEntry>,
    bytes: usize,
}

struct CodeEntry {
    text: String,
    language: String,
    generation: u64,
    lines: Vec<Line<'static>>,
    bytes: usize,
}

impl CodeCache {
    fn get(&mut self, text: &str, language: &str, generation: u64) -> Option<Vec<Line<'static>>> {
        let position = self.entries.iter().position(|entry| {
            entry.generation == generation && entry.text == text && entry.language == language
        })?;
        let entry = self.entries.remove(position);
        let lines = entry.lines.clone();
        self.entries.push(entry);
        Some(lines)
    }

    fn insert(&mut self, text: &str, language: &str, generation: u64, lines: &[Line<'static>]) {
        if text.len() > CODE_CACHE_BYTES || language.len() > CODE_CACHE_BYTES {
            return;
        }
        let lines = lines.to_vec();
        let text = text.to_owned();
        let language = language.to_owned();
        let bytes = std::mem::size_of::<CodeEntry>()
            + text.capacity()
            + language.capacity()
            + lines.capacity() * std::mem::size_of::<Line<'static>>()
            + lines
                .iter()
                .map(|line| {
                    line.spans.capacity() * std::mem::size_of::<Span<'static>>()
                        + line
                            .spans
                            .iter()
                            .map(|span| match &span.content {
                                std::borrow::Cow::Owned(text) => text.capacity(),
                                std::borrow::Cow::Borrowed(_) => 0,
                            })
                            .sum::<usize>()
                })
                .sum::<usize>();
        if bytes > CODE_CACHE_BYTES {
            return;
        }
        while self.entries.len() >= CODE_CACHE_ENTRIES || self.bytes + bytes > CODE_CACHE_BYTES {
            self.bytes -= self.entries.remove(0).bytes;
        }
        self.bytes += bytes;
        self.entries.push(CodeEntry {
            text,
            language,
            generation,
            lines,
            bytes,
        });
    }
}

static SYNTAX_SET: OnceLock<SyntaxSet> = OnceLock::new();
/// The theme for the current terminal background; `None` until a background is applied.
static THEME: Mutex<HighlightTheme> = Mutex::new(HighlightTheme {
    theme: None,
    generation: 0,
});

struct HighlightTheme {
    theme: Option<Theme>,
    generation: u64,
}

fn syntax_set() -> &'static SyntaxSet {
    SYNTAX_SET.get_or_init(two_face::syntax::extra_newlines)
}

/// The `light` and `dark` themes in `themes/`, built from GitHub's themes by `themes/build.py`.
fn bundled_theme(light_background: bool) -> Theme {
    let source: &[u8] = if light_background {
        include_bytes!("../../themes/light.tmTheme")
    } else {
        include_bytes!("../../themes/dark.tmTheme")
    };
    ThemeSet::load_from_reader(&mut std::io::Cursor::new(source))
        .expect("bundled theme is a valid .tmTheme")
}

/// Highlights with the `.tmTheme` at `path` when given, else with the bundled theme for the
/// background; returns the reason when the file cannot be used, keeping the bundled theme.
pub fn use_theme(light_background: bool, path: Option<&Path>) -> Result<(), String> {
    let (theme, result) = theme_for(light_background, path);
    let mut current = locked_theme();
    current.theme = Some(theme);
    current.generation += 1;
    result
}

/// Identifies the highlight theme used by cached styled lines.
pub(super) fn theme_generation() -> u64 {
    locked_theme().generation
}

fn theme_for(light_background: bool, path: Option<&Path>) -> (Theme, Result<(), String>) {
    let loaded = path.map(|path| {
        ThemeSet::get_theme(path).map_err(|error| format!("{}: {error}", path.display()))
    });
    match loaded {
        Some(Ok(theme)) => (theme, Ok(())),
        Some(Err(error)) => (bundled_theme(light_background), Err(error)),
        None => (bundled_theme(light_background), Ok(())),
    }
}

fn locked_theme() -> MutexGuard<'static, HighlightTheme> {
    THEME
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Runs `f` with the current theme, the dark bundled theme before any background is applied.
fn with_theme<T>(f: impl FnOnce(&Theme) -> T) -> T {
    let mut theme = locked_theme();
    f(theme.theme.get_or_insert_with(|| bundled_theme(false)))
}

/// Token kinds emitted by the JSON pretty-printer, in the order returned by `json_styles`.
#[derive(Clone, Copy)]
pub(super) enum JsonToken {
    Key,
    String,
    Number,
    Literal,
    Punctuation,
    Escape,
    StringQuote,
}

/// JSON token styles from the same theme and grammar scopes as syntax-highlighted code.
pub(super) fn json_styles() -> [Style; 7] {
    with_theme(json_styles_for)
}

fn json_styles_for(theme: &Theme) -> [Style; 7] {
    let highlighter = Highlighter::new(theme);
    let scopes: [&[&str]; 7] = [
        &[
            "source.json",
            "meta.mapping.key.json",
            "string.quoted.double.json",
        ],
        &[
            "source.json",
            "meta.mapping.value.json",
            "string.quoted.double.json",
        ],
        &[
            "source.json",
            "meta.mapping.value.json",
            "constant.numeric.integer.decimal.json",
        ],
        &[
            "source.json",
            "meta.mapping.value.json",
            "constant.language.json",
        ],
        &[
            "source.json",
            "meta.mapping.json",
            "punctuation.section.mapping.begin.json",
        ],
        &[
            "source.json",
            "meta.mapping.value.json",
            "string.quoted.double.json",
            "constant.character.escape.json",
        ],
        &[
            "source.json",
            "meta.mapping.value.json",
            "string.quoted.double.json",
            "punctuation.definition.string.begin.json",
        ],
    ];
    scopes.map(|names| {
        let stack: Vec<_> = names
            .iter()
            .map(|name| Scope::new(name).expect("JSON grammar scope is valid"))
            .collect();
        convert(highlighter.style_for_stack(&stack))
    })
}

/// The bundled JSON grammar scopes object keys as quoted strings inside mapping keys.
#[cfg(test)]
pub(super) fn json_key_foreground() -> Color {
    json_styles()[JsonToken::Key as usize]
        .fg
        .unwrap_or(Color::Reset)
}

/// A theme color as bat encodes it: alpha 0 holds an ANSI palette index in `r`, alpha 1 means the
/// terminal's default foreground (`None`), and any other alpha is RGB.
fn terminal_color(color: syntect::highlighting::Color) -> Option<Color> {
    match color.a {
        0 => Some(Color::Indexed(color.r)),
        1 => None,
        _ => Some(Color::Rgb(color.r, color.g, color.b)),
    }
}

/// Extensions the bundled syntaxes don't list, with one of the same language that they do.
const EXTENSION_ALIASES: [(&str, &str); 4] = [
    ("mjs", "js"),
    ("cjs", "js"),
    ("jsx", "tsx"),
    ("jsonc", "json"),
];

/// The syntax for a language token such as "bash" or a file extension such as "rs".
fn find_syntax(lang: &str) -> Option<&'static SyntaxReference> {
    let set = syntax_set();
    let lang = lang.trim();
    if lang.is_empty() {
        return None;
    }
    let lang = EXTENSION_ALIASES
        .iter()
        .find(|(alias, _)| alias.eq_ignore_ascii_case(lang))
        .map_or(lang, |(_, extension)| extension);
    set.find_syntax_by_token(lang)
        .or_else(|| set.find_syntax_by_extension(lang))
        .or_else(|| {
            let lower = lang.to_ascii_lowercase();
            set.syntaxes()
                .iter()
                .find(|syntax| syntax.name.to_ascii_lowercase() == lower)
        })
}

fn exceeds_limits(text: &str) -> bool {
    text.len() > MAX_BYTES
        || text.lines().count() > MAX_LINES
        || text.lines().any(|line| line.len() > MAX_LINE_BYTES)
}

/// `text` as one line per source line, highlighted as `lang` when the language is known
/// and the text is within the size limits, plain otherwise.
pub(super) fn code_lines(text: &str, lang: &str) -> Vec<Line<'static>> {
    let generation = theme_generation();
    if let Some(lines) = CODE_CACHE.with(|cache| cache.borrow_mut().get(text, lang, generation)) {
        return lines;
    }
    let lines = code_lines_uncached(text, lang);
    if generation == theme_generation() {
        CODE_CACHE.with(|cache| cache.borrow_mut().insert(text, lang, generation, &lines));
    }
    lines
}

fn code_lines_uncached(text: &str, lang: &str) -> Vec<Line<'static>> {
    let Some(mut lines) = highlighted(text, lang) else {
        return plain_lines(text);
    };
    if find_syntax(lang).map(|syntax| syntax.scope)
        == find_syntax("bash").map(|syntax| syntax.scope)
    {
        for script in super::shell_commands::scripts(text) {
            let syntax = match script.syntax {
                super::shell_commands::SyntaxSource::Interpreter(interpreter) => {
                    let shebang = format!("#!/usr/bin/env {interpreter}");
                    syntax_set().find_syntax_by_first_line(&shebang)
                }
                super::shell_commands::SyntaxSource::Extension(extension) => find_syntax(extension),
            };
            if let Some(syntax) = syntax
                && let Some(embedded) = highlighted(&text[script.range.clone()], &syntax.name)
            {
                overlay(&mut lines, text, script.range, embedded);
            }
        }
    }
    lines
}

/// Replaces styles only inside a script range; the shell parser still sees the entire command.
fn overlay(
    lines: &mut [Line<'static>],
    text: &str,
    range: std::ops::Range<usize>,
    embedded: Vec<Line<'static>>,
) {
    let first_row = text[..range.start]
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count();
    let first_column = range.start - text[..range.start].rfind('\n').map_or(0, |n| n + 1);
    for (row, script) in embedded.into_iter().enumerate() {
        let column = if row == 0 { first_column } else { 0 };
        let length: usize = script.spans.iter().map(|span| span.content.len()).sum();
        let line = &mut lines[first_row + row];
        let mut spans = Vec::new();
        let mut suffix = Vec::new();
        let mut offset = 0;
        for span in &line.spans {
            let end = offset + span.content.len();
            if offset < column {
                spans.push(Span::styled(
                    span.content[..(column - offset).min(span.content.len())].to_owned(),
                    span.style,
                ));
            }
            if end > column + length {
                suffix.push(Span::styled(
                    span.content[(column + length).saturating_sub(offset)..].to_owned(),
                    span.style,
                ));
            }
            offset = end;
        }
        spans.extend(script.spans);
        spans.extend(suffix);
        line.spans = spans;
    }
}

fn highlighted(text: &str, lang: &str) -> Option<Vec<Line<'static>>> {
    if text.is_empty() || exceeds_limits(text) {
        return None;
    }
    let syntax = find_syntax(lang)?;
    with_theme(|theme| {
        #[cfg(test)]
        HIGHLIGHT_CALLS.with(|count| count.set(count.get() + 1));
        let mut highlighter = HighlightLines::new(syntax, theme);
        let mut lines = Vec::new();
        for line in LinesWithEndings::from(text) {
            #[cfg(test)]
            if lang == "json" {
                JSON_HIGHLIGHTED_LINES.with(|count| count.set(count.get() + 1));
            }
            let ranges = highlighter.highlight_line(line, syntax_set()).ok()?;
            lines.push(styled_line(ranges));
        }
        Some(lines)
    })
}

fn styled_line(ranges: Vec<(syntect::highlighting::Style, &str)>) -> Line<'static> {
    let spans: Vec<_> = ranges
        .into_iter()
        .filter_map(|(style, part)| {
            let part = part.trim_end_matches(['\n', '\r']);
            (!part.is_empty()).then(|| Span::styled(part.to_owned(), convert(style)))
        })
        .collect();
    Line::from(spans)
}

fn convert(style: syntect::highlighting::Style) -> Style {
    let mut converted = Style::default();
    if let Some(fg) = terminal_color(style.foreground) {
        converted = converted.fg(fg);
    }
    if style.font_style.contains(FontStyle::BOLD) {
        converted = converted.add_modifier(Modifier::BOLD);
    }
    if style.font_style.contains(FontStyle::ITALIC) {
        converted = converted.add_modifier(Modifier::ITALIC);
    }
    if style.font_style.contains(FontStyle::UNDERLINE) {
        converted = converted.add_modifier(Modifier::UNDERLINED);
    }
    converted
}

/// `text` as one unstyled line per source line.
pub(super) fn plain_lines(text: &str) -> Vec<Line<'static>> {
    text.lines()
        .map(|line| Line::from(line.to_owned()))
        .collect()
}

/// Unified diff hunks: removed lines red, added lines green, hunk headers dim.
pub(super) fn diff_lines(text: &str) -> Vec<Line<'static>> {
    text.lines()
        .map(|line| {
            let style = if line.starts_with("@@") {
                Style::default().add_modifier(Modifier::DIM)
            } else if line.starts_with('-') {
                Style::default().fg(Color::Red)
            } else if line.starts_with('+') {
                Style::default().fg(Color::Green)
            } else {
                Style::default()
            };
            Line::from(Span::styled(line.to_owned(), style))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(lines: &[Line<'_>]) -> Vec<String> {
        lines.iter().map(ToString::to_string).collect()
    }

    fn recorded_shell_command() -> String {
        include_str!("../../tests/fixtures/claude-tool-details.jsonl")
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .flat_map(|record| {
                record["message"]["content"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
            })
            .find_map(|content| {
                (content["name"] == "Bash")
                    .then(|| content["input"]["command"].as_str().unwrap().to_owned())
            })
            .unwrap()
    }

    #[test]
    fn recorded_code_reuses_highlighting_and_invalidates_changed_keys() {
        let _guard = crate::theme::TEST_LOCK.lock().unwrap();
        let source = recorded_shell_command();
        use_theme(false, None).unwrap();
        HIGHLIGHT_CALLS.with(|count| count.set(0));
        let original = code_lines(&source, "bash");
        let calls = HIGHLIGHT_CALLS.with(std::cell::Cell::get);
        assert!(calls > 0);
        assert_eq!(code_lines(&source, "bash"), original);
        assert_eq!(HIGHLIGHT_CALLS.with(std::cell::Cell::get), calls);
        code_lines(&(source.clone() + "\n"), "bash");
        assert!(HIGHLIGHT_CALLS.with(std::cell::Cell::get) > calls);
        let calls = HIGHLIGHT_CALLS.with(std::cell::Cell::get);
        code_lines(&source, "python");
        assert!(HIGHLIGHT_CALLS.with(std::cell::Cell::get) > calls);
        let calls = HIGHLIGHT_CALLS.with(std::cell::Cell::get);
        use_theme(true, None).unwrap();
        let light = code_lines(&source, "bash");
        assert!(HIGHLIGHT_CALLS.with(std::cell::Cell::get) > calls);
        assert_ne!(light, original);
        use_theme(false, None).unwrap();
    }

    #[test]
    fn recorded_shell_cache_preserves_embedded_highlighting() {
        let _guard = crate::theme::TEST_LOCK.lock().unwrap();
        let record: serde_json::Value = serde_json::from_str(
            include_str!("../../tests/fixtures/claude-inline-script.jsonl").trim(),
        )
        .unwrap();
        let source = record["message"]["content"]
            .as_array()
            .unwrap()
            .iter()
            .find(|content| content["name"] == "Bash")
            .unwrap()["input"]["command"]
            .as_str()
            .unwrap()
            .to_owned();
        assert!(!super::super::shell_commands::scripts(&source).is_empty());
        use_theme(false, None).unwrap();
        let expected = code_lines_uncached(&source, "bash");
        HIGHLIGHT_CALLS.with(|count| count.set(0));
        let drawn = code_lines(&source, "bash");
        let calls = HIGHLIGHT_CALLS.with(std::cell::Cell::get);
        assert!(calls > 1, "shell and embedded script both highlight");
        assert_eq!(drawn, expected);
        assert_eq!(code_lines(&source, "bash"), expected);
        assert_eq!(HIGHLIGHT_CALLS.with(std::cell::Cell::get), calls);
    }

    #[test]
    fn recorded_code_cache_bounds_entries_and_allocated_text() {
        let _guard = crate::theme::TEST_LOCK.lock().unwrap();
        CODE_CACHE.with(|cache| {
            cache.borrow_mut().entries.clear();
            cache.borrow_mut().bytes = 0;
        });
        let source = recorded_shell_command();
        for suffix in 0..=CODE_CACHE_ENTRIES {
            code_lines(&format!("{source}\n# {suffix}"), "bash");
        }
        CODE_CACHE.with(|cache| {
            let cache = cache.borrow();
            assert_eq!(cache.entries.len(), CODE_CACHE_ENTRIES);
            assert!(cache.bytes <= CODE_CACHE_BYTES);
            assert!(
                !cache
                    .entries
                    .iter()
                    .any(|entry| entry.text == format!("{source}\n# 0"))
            );
        });
        let huge = source.repeat(CODE_CACHE_BYTES / source.len() + 1);
        code_lines(&huge, "bash");
        CODE_CACHE.with(|cache| {
            let cache = cache.borrow();
            assert!(cache.bytes <= CODE_CACHE_BYTES);
            assert!(!cache.entries.iter().any(|entry| entry.text == huge));
        });
        let large = source.repeat((CODE_CACHE_BYTES / 8) / source.len());
        for suffix in 0..32 {
            code_lines(&format!("{large}\n# {suffix}"), "unknown-language");
        }
        CODE_CACHE.with(|cache| {
            let cache = cache.borrow();
            assert!(cache.bytes <= CODE_CACHE_BYTES);
            assert!(
                cache.entries.len() < CODE_CACHE_ENTRIES,
                "byte budget evicts before entry budget"
            );
        });
    }

    #[test]
    fn json_token_styles_match_both_bundled_themes() {
        let source = r#"{"key": "value\t", "number": 42, "truth": true, "nothing": null, "array": [-12.5e2, false]}"#;
        for light in [false, true] {
            let theme = bundled_theme(light);
            let styles = json_styles_for(&theme);
            let mut highlighted = HighlightLines::new(find_syntax("json").unwrap(), &theme);
            let ranges = highlighted.highlight_line(source, syntax_set()).unwrap();
            for (needle, token) in [
                ("key", JsonToken::Key),
                ("\"key\"", JsonToken::Key),
                ("\"value", JsonToken::StringQuote),
                ("\", \"number", JsonToken::StringQuote),
                ("value", JsonToken::String),
                ("42", JsonToken::Number),
                ("-12.5e2", JsonToken::Number),
                ("true", JsonToken::Literal),
                ("false", JsonToken::Literal),
                ("null", JsonToken::Literal),
                ("{", JsonToken::Punctuation),
                ("[", JsonToken::Punctuation),
                ("]", JsonToken::Punctuation),
                (":", JsonToken::Punctuation),
                (",", JsonToken::Punctuation),
                ("}", JsonToken::Punctuation),
                ("\\t", JsonToken::Escape),
            ] {
                let offset = source.find(needle).unwrap();
                let mut cursor = 0;
                let (style, _) = ranges
                    .iter()
                    .find(|(_, part)| {
                        let contains = cursor <= offset && offset < cursor + part.len();
                        cursor += part.len();
                        contains
                    })
                    .unwrap();
                assert_eq!(styles[token as usize], convert(*style), "{light}: {needle}");
            }
        }
    }

    #[test]
    fn known_language_highlights_and_keeps_the_text() {
        let lines = code_lines("let x = 1;\n", "rs");
        assert_eq!(text(&lines), ["let x = 1;"]);
        assert!(lines[0].spans.len() > 1);
        assert!(lines[0].spans.iter().all(|span| span.style.fg.is_some()));
        assert_eq!(text(&code_lines("echo hi", "bash")), ["echo hi"]);
    }

    fn byte_styles(lines: &[Line<'_>]) -> Vec<Vec<Style>> {
        lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .flat_map(|span| std::iter::repeat_n(span.style, span.content.len()))
                    .collect()
            })
            .collect()
    }

    fn assert_script_styles(source: &str, script: &str, lang: &str) {
        let _guard = crate::theme::TEST_LOCK.lock().unwrap();
        let lines = code_lines(source, "bash");
        assert_eq!(text(&lines), source.lines().collect::<Vec<_>>());
        let actual = byte_styles(&lines);
        let shell = byte_styles(&highlighted(source, "bash").unwrap());
        let embedded = byte_styles(&highlighted(script, lang).unwrap());
        let start = source.find(script).unwrap();
        let end = start + script.len();
        let mut offset = 0;
        let mut embedded_offset = 0;
        let embedded: Vec<_> = embedded.into_iter().flatten().collect();
        for (row, line) in source.split_inclusive('\n').enumerate() {
            for column in 0..line.trim_end_matches(['\r', '\n']).len() {
                let expected = if (start..end).contains(&(offset + column)) {
                    let style = embedded[embedded_offset];
                    embedded_offset += 1;
                    style
                } else {
                    shell[row][column]
                };
                assert_eq!(
                    actual[row][column], expected,
                    "{source}: row {row}, byte {column}"
                );
            }
            offset += line.len();
        }
    }

    #[test]
    fn shell_script_python_argument() {
        let (source, script, lang) = (
            "python3 -c \"\nimport json;d=json.load(open('/tmp/x.json'))\nfor t in d['threads']:\n  if t['id'].startswith('01a147c6'): print(t)\n\"",
            "\nimport json;d=json.load(open('/tmp/x.json'))\nfor t in d['threads']:\n  if t['id'].startswith('01a147c6'): print(t)\n",
            "python",
        );
        assert_script_styles(source, script, lang);
    }

    #[test]
    fn shell_script_python_heredoc() {
        let (source, script, lang) = (
            "cd /some/dir && python3 - <<'EOF'\nimport json\nrows=[json.loads(l) for l in open('a.jsonl')]\nEOF",
            "import json\nrows=[json.loads(l) for l in open('a.jsonl')]\n",
            "python",
        );
        assert_script_styles(source, script, lang);
    }

    #[test]
    fn shell_script_node_argument() {
        let (source, script, lang) = (
            "node -e 'const x = 42; console.log(x)' && echo done",
            "const x = 42; console.log(x)",
            "js",
        );
        assert_script_styles(source, script, lang);
    }

    #[test]
    fn shell_script_node_heredoc() {
        let (source, script, lang) = (
            "node <<EOF\nconst x = 42; console.log(x)\nEOF\necho done",
            "const x = 42; console.log(x)\n",
            "js",
        );
        assert_script_styles(source, script, lang);
    }

    #[test]
    fn shell_scripts_follow_bundled_shebang_syntaxes() {
        for (source, script, lang) in [
            (
                "python -c 'import json; print(42)'",
                "import json; print(42)",
                "python",
            ),
            ("/usr/bin/python3 -c 'print(42)'", "print(42)", "python"),
            ("ruby -e 'puts 42'", "puts 42", "ruby"),
            (
                "python3 <<-EOF\n\timport json\n\tprint(42)\n\tEOF",
                "\timport json\n\tprint(42)\n",
                "python",
            ),
            (
                "python3 -c 'print(\"中文\")' && echo done",
                "print(\"中文\")",
                "python",
            ),
        ] {
            assert_script_styles(source, script, lang);
        }
    }

    #[test]
    fn shell_script_cat_heredoc_by_extension() {
        for (source, script, lang) in [
            (
                "cat > foo.py <<'EOF'\nimport json\nEOF",
                "import json\n",
                "py",
            ),
            ("cat <<EOF > a.rs\nlet x = 42;\nEOF", "let x = 42;\n", "rs"),
            (
                "cat > 'some file.py' <<EOF\nimport json\nEOF",
                "import json\n",
                "py",
            ),
        ] {
            assert_script_styles(source, script, lang);
        }
    }

    #[test]
    fn shell_scripts_leave_non_interpreter_arguments_alone() {
        let _guard = crate::theme::TEST_LOCK.lock().unwrap();
        for source in [
            "echo python3 -c 'import json'",
            "python3 script.py -c 'import json'",
            "python3 script.py <<EOF\nimport json\nEOF",
            "cat <<EOF\nimport json\nEOF",
            "cat >> foo.py <<EOF\nimport json\nEOF",
            "cat > foo.unknownext <<EOF\nimport json\nEOF",
            "tee foo.py <<EOF\nimport json\nEOF",
            "cat > $file.py <<EOF\nimport json\nEOF",
            "cat > \"$file.py\" <<EOF\nimport json\nEOF",
            "cat > foo.py > bar.rs <<EOF\nimport json\nEOF",
            "cat > foo.py <<EOF <<END\nimport json\nEOF\nlet x = 42;\nEND",
            "echo \"python3 -c 'import json'\"",
            "python3 -c 'import json'\"suffix\"",
            "python3 -c \"unterminated",
        ] {
            assert_eq!(
                code_lines(source, "bash"),
                highlighted(source, "bash").unwrap(),
                "{source}"
            );
        }
    }

    #[test]
    fn extension_aliases_find_their_language() {
        for lang in ["mjs", "cjs", "jsx", "jsonc"] {
            let lines = code_lines("const x = { \"a\": 1 };", lang);
            assert!(lines[0].spans.len() > 1, "{lang} is not highlighted");
        }
    }

    #[test]
    fn unknown_language_and_oversized_text_are_plain() {
        let lines = code_lines("let x = 1;", "no-such-language");
        assert_eq!(lines[0].spans.len(), 1);
        assert_eq!(lines[0].spans[0].style, Style::default());
        let long = format!("let x = \"{}\";", "a".repeat(MAX_LINE_BYTES));
        assert_eq!(code_lines(&long, "rs")[0].spans.len(), 1);
        let many = "x\n".repeat(MAX_LINES + 1);
        assert_eq!(code_lines(&many, "rs").len(), MAX_LINES + 1);
        assert_eq!(code_lines(&many, "rs")[0].spans.len(), 1);
    }

    #[test]
    fn theme_follows_the_background_unless_a_file_is_given() {
        let background = |theme: &Theme| theme.settings.background.map(|c| (c.r, c.g, c.b));
        let (light, result) = theme_for(true, None);
        assert_eq!((light.name.as_deref(), result), (Some("light"), Ok(())));
        let (dark, result) = theme_for(false, None);
        assert_eq!((dark.name.as_deref(), result), (Some("dark"), Ok(())));

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("custom.tmTheme");
        std::fs::write(
            &path,
            r##"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict><key>settings</key><array>
<dict><key>settings</key><dict><key>background</key><string>#123456</string></dict></dict>
</array></dict></plist>"##,
        )
        .unwrap();
        for light_background in [true, false] {
            let (theme, result) = theme_for(light_background, Some(&path));
            assert_eq!(
                (background(&theme), result),
                (Some((0x12, 0x34, 0x56)), Ok(()))
            );
        }
        let missing = dir.path().join("missing.tmTheme");
        let (theme, result) = theme_for(true, Some(&missing));
        assert_eq!(theme.name.as_deref(), Some("light"));
        assert!(result.is_err());
    }

    #[test]
    fn theme_colors_decode_terminal_palette_alphas() {
        use syntect::highlighting::Color as ThemeColor;
        let color = |r, a| ThemeColor { r, g: 2, b: 3, a };
        assert_eq!(terminal_color(color(4, 0)), Some(Color::Indexed(4)));
        assert_eq!(terminal_color(color(4, 1)), None);
        assert_eq!(terminal_color(color(4, 0xff)), Some(Color::Rgb(4, 2, 3)));
    }

    #[test]
    fn diff_lines_colour_by_marker() {
        let lines = diff_lines("@@ -1 +1 @@\n-old\n+new\n same");
        assert_eq!(text(&lines), ["@@ -1 +1 @@", "-old", "+new", " same"]);
        assert!(lines[0].spans[0].style.add_modifier.contains(Modifier::DIM));
        assert_eq!(lines[1].spans[0].style.fg, Some(Color::Red));
        assert_eq!(lines[2].spans[0].style.fg, Some(Color::Green));
        assert_eq!(lines[3].spans[0].style.fg, None);
    }
}
