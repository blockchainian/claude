//! Content semantics shared by detail producers and their layout.

use ratatui::text::{Line, Span};

#[derive(Clone, Copy, Default)]
pub(super) enum Kind {
    #[default]
    Prose,
    Preformatted,
}

pub(super) struct Block {
    pub lines: Vec<Line<'static>>,
    pub indent: usize,
    pub kind: Kind,
}

impl Block {
    pub fn layout(self, options: textwrap::Options<'_>) -> Vec<Line<'static>> {
        self.layout_with_breaks(options).0
    }

    /// Each row starts a display line unless wrapping continued the preceding row.
    pub fn layout_with_breaks(
        self,
        options: textwrap::Options<'_>,
    ) -> (Vec<Line<'static>>, Vec<Option<bool>>) {
        let mut lines = Vec::new();
        let mut breaks = Vec::new();
        match self.kind {
            Kind::Prose => {
                for line in self.lines {
                    let (wrapped, joins) = wrap(line, self.indent, options.clone());
                    lines.extend(wrapped);
                    breaks.extend(joins);
                }
            }
            Kind::Preformatted => {
                breaks.resize(self.lines.len(), None);
                lines = self.lines;
            }
        }
        (lines, breaks)
    }
}

fn wrap(
    line: Line<'static>,
    indent: usize,
    options: textwrap::Options<'_>,
) -> (Vec<Line<'static>>, Vec<Option<bool>>) {
    let width = options.width;
    if width == 0 {
        return (Vec::new(), Vec::new());
    }
    if line.width() <= width {
        return (vec![line], vec![None]);
    }
    let plain = line.to_string();
    let padding = " ".repeat(indent.min(width.saturating_sub(1)));
    let options = options
        .wrap_algorithm(textwrap::WrapAlgorithm::FirstFit)
        .subsequent_indent(&padding);
    let mut offset = 0;
    let mut breaks = Vec::new();
    let lines = textwrap::wrap(&plain, options)
        .into_iter()
        .enumerate()
        .map(|(i, text)| {
            let text = if i > 0 { &text[padding.len()..] } else { &text };
            let start = offset
                + plain[offset..]
                    .find(text)
                    .expect("wrapped text belongs to source");
            let end = start + text.len();
            breaks.push((i > 0).then_some(start > offset));
            offset = end;
            let mut spans = Vec::new();
            if i > 0 && !padding.is_empty() {
                spans.push(Span::raw(padding.clone()));
            }
            let mut position = 0;
            for span in &line.spans {
                let next = position + span.content.len();
                let from = start.max(position);
                let to = end.min(next);
                if from < to {
                    spans.push(Span::styled(
                        span.content[from - position..to - position].to_owned(),
                        span.style,
                    ));
                }
                position = next;
            }
            Line::from(spans)
        })
        .collect();
    (lines, breaks)
}
