//! Syntax highlighting of the code and diff sections an open tool row shows.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use std::sync::OnceLock;
use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle, Highlighter, Theme};
use syntect::parsing::{Scope, SyntaxReference, SyntaxSet};
use syntect::util::LinesWithEndings;
use two_face::theme::EmbeddedThemeName;

/// Texts larger than this, or with more lines or a longer line than the limits below, are
/// drawn plain: highlighting them would stall the frame.
const MAX_BYTES: usize = 512 * 1024;
const MAX_LINES: usize = 10_000;
const MAX_LINE_BYTES: usize = 4 * 1024;

static SYNTAX_SET: OnceLock<SyntaxSet> = OnceLock::new();
static THEME: OnceLock<Theme> = OnceLock::new();

fn syntax_set() -> &'static SyntaxSet {
    SYNTAX_SET.get_or_init(two_face::syntax::extra_newlines)
}

fn theme() -> &'static Theme {
    THEME.get_or_init(|| {
        two_face::theme::extra()
            .get(EmbeddedThemeName::Base16OceanDark)
            .clone()
    })
}

/// The bundled JSON grammar scopes object keys as quoted strings inside mapping keys.
pub(super) fn json_key_foreground() -> Color {
    static FOREGROUND: OnceLock<Color> = OnceLock::new();
    *FOREGROUND.get_or_init(|| {
        let scopes = [
            "source.json",
            "meta.mapping.key.json",
            "string.quoted.double.json",
        ]
        .map(|scope| Scope::new(scope).expect("JSON key scope is valid"));
        let foreground = Highlighter::new(theme())
            .style_for_stack(&scopes)
            .foreground;
        Color::Rgb(foreground.r, foreground.g, foreground.b)
    })
}

/// The syntax for a language token such as "bash" or a file extension such as "rs".
fn find_syntax(lang: &str) -> Option<&'static SyntaxReference> {
    let set = syntax_set();
    let lang = lang.trim();
    if lang.is_empty() {
        return None;
    }
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
    highlighted(text, lang).unwrap_or_else(|| plain_lines(text))
}

fn highlighted(text: &str, lang: &str) -> Option<Vec<Line<'static>>> {
    if text.is_empty() || exceeds_limits(text) {
        return None;
    }
    let syntax = find_syntax(lang)?;
    let mut highlighter = HighlightLines::new(syntax, theme());
    let mut lines = Vec::new();
    for line in LinesWithEndings::from(text) {
        let ranges = highlighter.highlight_line(line, syntax_set()).ok()?;
        let spans: Vec<_> = ranges
            .into_iter()
            .filter_map(|(style, part)| {
                let part = part.trim_end_matches(['\n', '\r']);
                (!part.is_empty()).then(|| Span::styled(part.to_owned(), convert(style)))
            })
            .collect();
        lines.push(Line::from(spans));
    }
    Some(lines)
}

fn convert(style: syntect::highlighting::Style) -> Style {
    let fg = style.foreground;
    let mut converted = Style::default().fg(Color::Rgb(fg.r, fg.g, fg.b));
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

    #[test]
    fn known_language_highlights_and_keeps_the_text() {
        let lines = code_lines("let x = 1;\n", "rs");
        assert_eq!(text(&lines), ["let x = 1;"]);
        assert!(lines[0].spans.len() > 1);
        assert!(lines[0].spans.iter().all(|span| span.style.fg.is_some()));
        assert_eq!(text(&code_lines("echo hi", "bash")), ["echo hi"]);
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
    fn diff_lines_colour_by_marker() {
        let lines = diff_lines("@@ -1 +1 @@\n-old\n+new\n same");
        assert_eq!(text(&lines), ["@@ -1 +1 @@", "-old", "+new", " same"]);
        assert!(lines[0].spans[0].style.add_modifier.contains(Modifier::DIM));
        assert_eq!(lines[1].spans[0].style.fg, Some(Color::Red));
        assert_eq!(lines[2].spans[0].style.fg, Some(Color::Green));
        assert_eq!(lines[3].spans[0].style.fg, None);
    }
}
