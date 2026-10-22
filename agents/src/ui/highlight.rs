//! Syntax highlighting of the code and diff sections an open tool row shows.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use std::path::Path;
use std::sync::{Mutex, OnceLock};
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
    pub(super) static JSON_HIGHLIGHTED_LINES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

static SYNTAX_SET: OnceLock<SyntaxSet> = OnceLock::new();
/// The theme for the current terminal background; `None` until a background is applied.
#[cfg(not(test))]
static THEME: Mutex<HighlightTheme> = Mutex::new(HighlightTheme {
    theme: None,
    generation: 0,
});

// Render tests change themes independently, including between frames of the same view.
#[cfg(test)]
thread_local! {
    static THEME: Mutex<HighlightTheme> = const { Mutex::new(HighlightTheme {
        theme: None,
        generation: 0,
    }) };
}

struct HighlightTheme {
    theme: Option<Theme>,
    generation: u64,
}

fn syntax_set() -> &'static SyntaxSet {
    SYNTAX_SET.get_or_init(two_face::syntax::extra_newlines)
}

/// The bundled themes derive from GitHub's VS Code themes, with colors adjusted to match Nvim.
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
    with_current_theme(|current| {
        current.theme = Some(theme);
        current.generation += 1;
    });
    result
}

/// Identifies the highlight theme used by cached styled lines.
pub(super) fn theme_generation() -> u64 {
    with_current_theme(|current| current.generation)
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

fn with_current_theme<T>(f: impl FnOnce(&mut HighlightTheme) -> T) -> T {
    let apply = |theme: &Mutex<HighlightTheme>| {
        f(&mut theme
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()))
    };
    #[cfg(test)]
    return THEME.with(apply);
    #[cfg(not(test))]
    apply(&THEME)
}

/// Runs `f` with the current theme, the dark bundled theme before any background is applied.
fn with_theme<T>(f: impl FnOnce(&Theme) -> T) -> T {
    with_current_theme(|current| f(current.theme.get_or_insert_with(|| bundled_theme(false))))
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

/// A scope's foreground, resolved by syntect just like syntax-highlighted code.
pub(super) fn scope_style(scope: &str) -> Style {
    with_theme(|theme| {
        let scope = Scope::new(scope).expect("UI theme scope is valid");
        Style {
            fg: terminal_color(Highlighter::new(theme).style_for_stack(&[scope]).foreground),
            ..Style::default()
        }
    })
}

pub(super) fn selection_background() -> Option<Color> {
    with_theme(|theme| theme.settings.selection.and_then(terminal_color))
}

pub(super) fn accent_color() -> Option<Color> {
    with_theme(|theme| theme.settings.accent.and_then(terminal_color))
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
        0 => Some(match color.r {
            0 => Color::Black,
            1 => Color::Red,
            2 => Color::Green,
            3 => Color::Yellow,
            4 => Color::Blue,
            5 => Color::Magenta,
            6 => Color::Cyan,
            7 => Color::Gray,
            8 => Color::DarkGray,
            9 => Color::LightRed,
            10 => Color::LightGreen,
            11 => Color::LightYellow,
            12 => Color::LightBlue,
            13 => Color::LightMagenta,
            14 => Color::LightCyan,
            15 => Color::White,
            index => Color::Indexed(index),
        }),
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
pub(crate) fn find_syntax(lang: &str) -> Option<&'static SyntaxReference> {
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
    if let Some(lang) = lang.strip_prefix("numbered:") {
        if exceeds_limits(text) {
            return plain_lines(text);
        }
        let gutters: Vec<_> = text
            .lines()
            .map(|line| {
                line.split_once('\t')
                    .filter(|(prefix, _)| {
                        let number = prefix.trim_start_matches(char::is_whitespace);
                        !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit())
                    })
                    .map_or("", |(prefix, _)| &line[..prefix.len() + 1])
            })
            .collect();
        let code = text
            .split_inclusive('\n')
            .zip(&gutters)
            .map(|(line, gutter)| &line[gutter.len()..])
            .collect::<String>();
        let mut lines = code_lines(&code, lang);
        lines.resize_with(gutters.len(), Line::default);
        for (line, gutter) in lines.iter_mut().zip(gutters) {
            if !gutter.is_empty() {
                line.spans.insert(
                    0,
                    Span::styled(gutter.to_owned(), Style::new().add_modifier(Modifier::DIM)),
                );
            }
        }
        return lines;
    }
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

/// Unified diff hunks with separate syntax state for the before and after code.
/// Preformatted rows are clipped by the view, so only short changed rows need padding.
pub(super) fn diff_lines(text: &str, lang: &str, width: usize) -> Vec<Line<'static>> {
    let diff_style = |name| {
        let mut style = scope_style(name);
        style.bg = with_theme(|theme| {
            Highlighter::new(theme)
                .style_mod_for_stack(&[Scope::new(name).expect("diff scope is valid")])
                .background
                .and_then(terminal_color)
        });
        style
    };
    let deleted = diff_style("markup.deleted");
    let inserted = diff_style("markup.inserted");
    let foreground = with_theme(|theme| theme.settings.foreground.and_then(terminal_color));
    let syntax = !exceeds_limits(text) && find_syntax(lang).is_some();
    let rows: Vec<_> = text.lines().collect();
    rows.split_inclusive(|line| line.starts_with("@@"))
        .flat_map(|hunk| {
            let code = |skip| {
                let source: String = hunk
                    .iter()
                    .filter(|line| {
                        matches!(line.as_bytes().first(), Some(b'+' | b'-' | b' '))
                            && !line.starts_with(skip)
                    })
                    .map(|line| format!("{}\n", &line[1..]))
                    .collect();
                code_lines(&source, lang).into_iter()
            };
            let mut before = syntax.then(|| code('+'));
            let mut after = syntax.then(|| code('-'));
            hunk.iter()
                .map(|source| {
                    let marker = source.as_bytes().first().copied();
                    let style = match marker {
                        Some(b'-') => deleted,
                        Some(b'+') => inserted,
                        _ => Style::default(),
                    };
                    if !matches!(marker, Some(b'+' | b'-' | b' ')) {
                        return Line::from(Span::styled(
                            (*source).to_owned(),
                            if source.starts_with("@@") {
                                Style::new().dim()
                            } else {
                                Style::default()
                            },
                        ));
                    }
                    let old = (marker != Some(b'+'))
                        .then(|| before.as_mut().and_then(Iterator::next))
                        .flatten();
                    let new = (marker != Some(b'-'))
                        .then(|| after.as_mut().and_then(Iterator::next))
                        .flatten();
                    let content = if marker == Some(b'-') { old } else { new };
                    let mut spans = vec![Span::styled(source[..1].to_owned(), style)];
                    if let Some(content) = content {
                        spans.extend(content.spans.into_iter().map(|mut span| {
                            if matches!(marker, Some(b'+' | b'-'))
                                && (span.style.fg.is_none() || span.style.fg == foreground)
                            {
                                span.style.fg = style.fg;
                            }
                            span.style.bg = style.bg;
                            span
                        }));
                    } else {
                        spans.push(Span::styled(source[1..].to_owned(), style));
                    }
                    let mut line = Line::from(spans);
                    if style.bg.is_some() && line.width() < width {
                        line.spans
                            .push(Span::styled(" ".repeat(width - line.width()), style));
                    }
                    line
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ui_colors_follow_theme_changes() {
        let _guard = crate::theme::TEST_LOCK.lock().unwrap();
        struct Restore;
        impl Drop for Restore {
            fn drop(&mut self) {
                use_theme(false, None).unwrap();
            }
        }
        let _restore = Restore;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("custom.tmTheme");
        // Start with the real bundled theme, preserving its scope selectors.
        let source = std::str::from_utf8(include_bytes!("../../themes/dark.tmTheme")).unwrap();
        for (error, quote, selection, accent) in [
            ("#123456", "#234567", "#345678", "#456789"),
            ("#abcdef", "#bcdef0", "#cdef01", "#def012"),
        ] {
            let source = source
                .replace("#a0635d", error)
                .replace("#4a8b4f", quote)
                .replace("#3f8f7f", "#789abc")
                .replace("#220a0a", "#456789")
                .replace("#001408", "#56789a")
                .replace("#e9000000", selection)
                .replacen("#06000000", accent, 1)
                .replace("#06000000", "#56789a");
            std::fs::write(&path, source).unwrap();
            let generation = theme_generation();
            use_theme(false, Some(&path)).unwrap();
            assert!(theme_generation() > generation);
            let rgb = |hex: &str| {
                Some(Color::Rgb(
                    u8::from_str_radix(&hex[1..3], 16).unwrap(),
                    u8::from_str_radix(&hex[3..5], 16).unwrap(),
                    u8::from_str_radix(&hex[5..7], 16).unwrap(),
                ))
            };
            with_theme(|theme| {
                assert_eq!(
                    theme.settings.selection.and_then(terminal_color),
                    rgb(selection)
                );
                assert_eq!(theme.settings.accent.and_then(terminal_color), rgb(accent));
            });
            let diff = diff_lines("-removed\n+added", "", 0);
            let quote_lines = super::super::markdown::render("> quote", 80);
            let code = super::super::markdown::render("```\ncode\n```", 80);
            assert_eq!(
                [
                    super::super::style::error_style().fg,
                    super::super::style::selection_style().bg,
                    super::super::style::accent_color(),
                    quote_lines[0].spans[0].style.fg,
                    code[0].spans[0].style.fg,
                    diff[0].spans[0].style.fg,
                    diff[1].spans[0].style.fg,
                ],
                [
                    rgb(error),
                    rgb(selection),
                    rgb(accent),
                    rgb(quote),
                    Some(Color::Rgb(0x56, 0x78, 0x9a)),
                    rgb("#789abc"),
                    rgb("#789abc")
                ]
            );
            assert_eq!(diff[0].spans[0].style.bg, rgb("#456789"));
            assert_eq!(diff[1].spans[0].style.bg, rgb("#56789a"));
        }
        for (light, error, quote, selection) in [
            (true, Color::Rgb(130, 7, 30), Color::Rgb(17, 99, 41), 111),
            (false, Color::Rgb(160, 99, 93), Color::Rgb(74, 139, 79), 233),
        ] {
            use_theme(light, None).unwrap();
            assert_eq!(super::super::style::error_style().fg, Some(error));
            assert_eq!(
                super::super::style::selection_style().bg,
                Some(Color::Indexed(selection))
            );
            assert_eq!(super::super::style::accent_color(), Some(Color::Cyan));
            assert_eq!(scope_style("markup.quote").fg, Some(quote));
            assert_eq!(scope_style("markup.raw.block").fg, Some(Color::Cyan));
            let diff = diff_lines("-removed\n+added", "", 0);
            let markup = if light {
                Color::Rgb(0, 0, 0)
            } else {
                Color::Rgb(63, 143, 127)
            };
            assert_eq!(diff[0].spans[0].style.fg, Some(markup));
            assert_eq!(diff[1].spans[0].style.fg, Some(markup));
        }
        // An external theme may omit every UI setting and scope.
        let mut minimal = plist::Value::from_reader_xml(std::io::Cursor::new(source)).unwrap();
        let settings = minimal
            .as_dictionary_mut()
            .unwrap()
            .get_mut("settings")
            .unwrap()
            .as_array_mut()
            .unwrap();
        settings.truncate(1);
        let globals = settings[0]
            .as_dictionary_mut()
            .unwrap()
            .get_mut("settings")
            .unwrap()
            .as_dictionary_mut()
            .unwrap();
        globals.remove("selection");
        globals.remove("accent");
        minimal.to_file_xml(&path).unwrap();
        use_theme(false, Some(&path)).unwrap();
        assert_eq!(super::super::style::selection_style().bg, None);
        assert_eq!(super::super::style::accent_color(), None);
        for scope in [
            "markup.quote",
            "markup.raw.block",
            "message.error",
            "markup.deleted",
            "markup.inserted",
        ] {
            assert_eq!(scope_style(scope).fg, Some(Color::Indexed(23)));
        }
    }

    fn text(lines: &[Line<'_>]) -> Vec<String> {
        lines.iter().map(ToString::to_string).collect()
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
    fn numbered_code_keeps_gutters_dim_and_code_highlighted() {
        let source = "     1\tlet answer = \"ready\";\n     2\t// next\nplain text";
        let lines = code_lines(source, "numbered:rs");
        assert_eq!(text(&lines), source.lines().collect::<Vec<_>>());
        assert_eq!(lines[0].spans[0].content, "     1\t");
        assert!(lines[0].spans[0].style.add_modifier.contains(Modifier::DIM));
        assert_eq!(lines[0].spans[0].style.fg, None);
        assert!(lines[0].spans.len() > 2);
        assert_eq!(lines[1].spans[0].content, "     2\t");
        assert!(!lines[2].spans[0].style.add_modifier.contains(Modifier::DIM));
        assert_eq!(text(&code_lines("     1\t", "numbered:rs")), ["     1\t"]);
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
    fn shell_script_python_heredoc_after_script_arguments() {
        for source in [
            "python3 - agents <<'EOF'\nimport json\nEOF",
            "python3 - agents src tests <<'EOF'\nimport json\nEOF",
            "python3 - -c 'print(42)' <<'EOF'\nimport json\nEOF",
        ] {
            assert_script_styles(source, "import json\n", "python");
        }
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
        for (index, expected) in [
            Color::Black,
            Color::Red,
            Color::Green,
            Color::Yellow,
            Color::Blue,
            Color::Magenta,
            Color::Cyan,
            Color::Gray,
            Color::DarkGray,
            Color::LightRed,
            Color::LightGreen,
            Color::LightYellow,
            Color::LightBlue,
            Color::LightMagenta,
            Color::LightCyan,
            Color::White,
        ]
        .into_iter()
        .enumerate()
        {
            assert_eq!(terminal_color(color(index as u8, 0)), Some(expected));
        }
        assert_eq!(terminal_color(color(111, 0)), Some(Color::Indexed(111)));
        assert_eq!(terminal_color(color(4, 1)), None);
        assert_eq!(terminal_color(color(4, 0xff)), Some(Color::Rgb(4, 2, 3)));
    }

    #[test]
    fn diff_backgrounds_fill_width_in_both_themes() {
        for (light, removed, added) in [
            (false, Color::Rgb(34, 10, 10), Color::Rgb(0, 20, 8)),
            (true, Color::Rgb(208, 185, 185), Color::Rgb(125, 212, 183)),
        ] {
            use_theme(light, None).unwrap();
            let lines = diff_lines("@@ -1 +1 @@\n-旧\n+新\n same", "", 20);
            for (line, background) in lines[1..3].iter().zip([removed, added]) {
                assert_eq!(line.width(), 20);
                assert!(
                    line.spans
                        .iter()
                        .all(|span| span.style.bg == Some(background))
                );
            }
            for line in [&lines[0], &lines[3]] {
                assert!(line.spans.iter().all(|span| span.style.bg.is_none()));
            }
        }
    }

    #[test]
    fn diff_plain_text_uses_markup_foregrounds_and_preserves_syntax_colors() {
        let payload: serde_json::Value = serde_json::from_str(
            include_str!("../../tests/fixtures/claude-tool-calls.jsonl")
                .lines()
                .nth(5)
                .unwrap(),
        )
        .unwrap();
        let code = payload["toolUseResult"]["structuredPatch"][0]["lines"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(serde_json::Value::as_str)
            .find(|line| line.starts_with("+        let word = "))
            .unwrap()
            .strip_prefix('+')
            .unwrap();
        for light in [false, true] {
            for terminal_default in [false, true] {
                let mut theme = bundled_theme(light);
                if terminal_default {
                    theme.settings.foreground = Some(syntect::highlighting::Color {
                        r: 0,
                        g: 0,
                        b: 0,
                        a: 1,
                    });
                }
                let default = theme.settings.foreground.and_then(terminal_color);
                with_current_theme(|current| current.theme = Some(theme));
                let expected = byte_styles(&code_lines(code, "rs"));
                assert!(expected[0].iter().any(|style| style.fg == default));
                assert!(expected[0].iter().any(|style| style.fg != default));
                let lines = diff_lines(&format!("@@ -1 +1 @@\n-{code}\n+{code}\n {code}"), "rs", 0);
                assert!(lines[0].spans[0].style.add_modifier.contains(Modifier::DIM));
                let actual = byte_styles(&lines);
                for (row, scope) in [(1, "markup.deleted"), (2, "markup.inserted")] {
                    let markup = scope_style(scope).fg;
                    assert_ne!(default, markup);
                    assert_eq!(actual[row][0].fg, markup);
                    for (actual, expected) in actual[row][1..].iter().zip(&expected[0]) {
                        let foreground = if expected.fg.is_none() || expected.fg == default {
                            markup
                        } else {
                            expected.fg
                        };
                        assert_eq!(
                            actual.fg, foreground,
                            "light={light}, terminal_default={terminal_default}"
                        );
                        assert_eq!(actual.add_modifier, expected.add_modifier);
                    }
                }
                for (actual, expected) in actual[3][1..].iter().zip(&expected[0]) {
                    assert_eq!(actual, expected);
                }
            }
        }
    }

    #[test]
    fn diff_code_matches_rust_highlighting() {
        use_theme(false, None).unwrap();
        let code = "fn main() { let value = 42; }";
        let lines = diff_lines(&format!("-{code}\n+{code}"), "rs", 40);
        let expected = byte_styles(&code_lines(code, "rs"));
        let foreground = with_theme(|theme| theme.settings.foreground.and_then(terminal_color));
        for (line, scope) in lines.iter().zip(["markup.deleted", "markup.inserted"]) {
            assert_eq!(line.spans[0].content.len(), 1);
            assert_eq!(line.spans[0].style.fg, scope_style(scope).fg);
            assert!(
                line.spans
                    .iter()
                    .all(|span| span.style.bg == line.spans[0].style.bg)
            );
            assert!(line.spans[0].style.bg.is_some());
            assert!(
                line.spans
                    .iter()
                    .any(|span| span.style.fg != line.spans[0].style.fg)
            );
            let actual = byte_styles(std::slice::from_ref(line));
            for (actual, expected) in actual[0][1..].iter().zip(&expected[0]) {
                let expected = if expected.fg.is_none() || expected.fg == foreground {
                    scope_style(scope).fg
                } else {
                    expected.fg
                };
                assert_eq!(actual.fg, expected);
            }
        }
    }

    #[test]
    fn diff_unknown_and_oversized_code_keeps_markup_foregrounds() {
        use_theme(false, None).unwrap();
        for (lang, code) in [
            ("unknown-language", "fn main() {}".to_owned()),
            ("rs", "x".repeat(MAX_LINE_BYTES + 1)),
            ("rs", "x\n".repeat(MAX_LINES + 1)),
            (
                "rs",
                format!("{}\n", "x".repeat(256)).repeat(MAX_BYTES / 256 + 1),
            ),
        ] {
            let source = format!("-{code}\n+{code}");
            let lines = diff_lines(&source, lang, 20);
            for line in lines.iter().filter(|line| {
                line.spans
                    .first()
                    .is_some_and(|span| matches!(span.content.as_ref(), "-" | "+"))
            }) {
                let scope = if line.spans[0].content == "-" {
                    "markup.deleted"
                } else {
                    "markup.inserted"
                };
                assert!(
                    line.spans
                        .iter()
                        .all(|span| span.style.fg == scope_style(scope).fg)
                );
            }
        }
    }

    #[test]
    fn diff_uses_only_explicit_scope_backgrounds() {
        let mut theme = bundled_theme(false);
        for item in &mut theme.scopes {
            item.style.background = None;
        }
        with_current_theme(|current| current.theme = Some(theme));
        let lines = diff_lines("-fn main() {}\n+fn main() {}", "rs", 80);
        assert!(
            lines
                .iter()
                .flat_map(|line| &line.spans)
                .all(|span| span.style.bg.is_none())
        );
        assert_eq!(lines[0].width(), 13);
    }

    #[test]
    fn diff_keeps_before_and_after_multiline_syntax_separate() {
        use_theme(false, None).unwrap();
        let source = "@@ -1,3 +1,3 @@\n /* start\n-old comment\n+new comment\n */\n@@ -9 +9 @@\n-fn before() {}\n+fn after() {}";
        let lines = diff_lines(source, "rs", 0);
        let before = byte_styles(&code_lines("/* start\nold comment\n*/", "rs"));
        let after = byte_styles(&code_lines("/* start\nnew comment\n*/", "rs"));
        let styles = byte_styles(&lines);
        for (row, expected) in [
            (1, &after[0]),
            (2, &before[1]),
            (3, &after[1]),
            (4, &after[2]),
        ] {
            for (actual, expected) in styles[row][1..].iter().zip(expected) {
                assert_eq!(actual.fg, expected.fg);
            }
        }
        for (row, code) in [(6, "fn before() {}"), (7, "fn after() {}")] {
            let expected = byte_styles(&code_lines(code, "rs"));
            let foreground = with_theme(|theme| theme.settings.foreground.and_then(terminal_color));
            for (actual, expected) in styles[row][1..].iter().zip(&expected[0]) {
                let expected = if expected.fg.is_none() || expected.fg == foreground {
                    styles[row][0].fg
                } else {
                    expected.fg
                };
                assert_eq!(actual.fg, expected);
            }
        }
    }

    #[test]
    fn diff_keeps_wide_rows_unwrapped() {
        let source = "+中文代码很长";
        let lines = diff_lines(source, "rs", 4);
        assert_eq!(text(&lines), [source]);
        assert!(lines[0].width() > 4);
        assert_eq!(diff_lines("+", "rs", 0)[0].width(), 1);
    }

    #[test]
    fn diff_lines_colour_by_marker() {
        let _guard = crate::theme::TEST_LOCK.lock().unwrap();
        use_theme(false, None).unwrap();
        let lines = diff_lines("@@ -1 +1 @@\n-old\n+new\n same", "", 0);
        assert_eq!(text(&lines), ["@@ -1 +1 @@", "-old", "+new", " same"]);
        assert!(lines[0].spans[0].style.add_modifier.contains(Modifier::DIM));
        assert_eq!(lines[1].spans[0].style.fg, Some(Color::Rgb(63, 143, 127)));
        assert_eq!(lines[2].spans[0].style.fg, Some(Color::Rgb(63, 143, 127)));
        assert_eq!(lines[3].spans[0].style.fg, None);
    }
}
