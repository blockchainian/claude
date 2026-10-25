//! Formats embedded in free text share detection, rendering and copy sources.

use super::highlight;
use ratatui::text::Line;

pub(super) mod json;
mod xml;

/// A registered format locates a candidate and accepts only a complete document.
pub(super) trait Format: Sync {
    fn candidate(&self, source: &str, start: usize) -> Option<usize>;
    fn parse(&self, source: &str) -> Option<Box<dyn Document>>;
}

/// Parsed content owns formatting and can preserve a format-specific palette.
pub(super) trait Document {
    fn text(&self) -> String;
    fn syntax(&self) -> &'static str;
    fn is_empty(&self) -> bool {
        false
    }
    fn lines(&self, width: usize) -> Vec<Line<'static>> {
        super::content::hard_wrap(self.highlighted(), width)
    }
    fn highlighted(&self) -> Vec<Line<'static>> {
        highlight::code_lines(&self.text(), self.syntax())
    }
}

static FORMATS: &[&dyn Format] = &[&json::Json, &xml::Xml];

pub(super) fn parse(source: &str) -> Option<Box<dyn Document>> {
    FORMATS.iter().find_map(|format| format.parse(source))
}

/// Failed candidates advance once and remain literal, including their nested fragments.
pub(super) fn detect(source: &str, start: usize) -> (usize, Option<Box<dyn Document>>) {
    #[cfg(test)]
    json::record_json_scan(1);
    for format in FORMATS {
        if let Some(end) = format.candidate(source, start) {
            let document = format
                .parse(&source[start..end])
                .filter(|document| !document.is_empty());
            return (end, document);
        }
    }
    (start + 1, None)
}

/// Markdown keeps inline code and prose intact; only standalone blocks are lifted.
pub(super) fn line_block(source: &str, start: usize, end: usize) -> bool {
    source[..start]
        .rsplit('\n')
        .next()
        .is_some_and(|line| line.trim().is_empty())
        && source[end..]
            .split('\n')
            .next()
            .is_some_and(|line| line.trim().is_empty())
}

/// Declared Markdown code retains its own language and literal copy source.
pub(super) fn code_blocks(source: &str) -> Vec<std::ops::Range<usize>> {
    use pulldown_cmark::{Event, Parser, Tag};
    Parser::new(source)
        .into_offset_iter()
        .filter_map(|(event, range)| {
            matches!(event, Event::Start(Tag::CodeBlock(_))).then_some(range)
        })
        .collect()
}
