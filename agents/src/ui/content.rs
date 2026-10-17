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
        match self.kind {
            Kind::Prose => self
                .lines
                .into_iter()
                .flat_map(|line| wrap(line, self.indent, options.clone()))
                .collect(),
            Kind::Preformatted => self.lines,
        }
    }
}

fn wrap(line: Line<'static>, indent: usize, options: textwrap::Options<'_>) -> Vec<Line<'static>> {
    let width = options.width;
    if width == 0 {
        return Vec::new();
    }
    if line.width() <= width {
        return vec![line];
    }
    let plain = line.to_string();
    let padding = " ".repeat(indent.min(width.saturating_sub(1)));
    let options = options
        .wrap_algorithm(textwrap::WrapAlgorithm::FirstFit)
        .subsequent_indent(&padding);
    let mut offset = 0;
    textwrap::wrap(&plain, options)
        .into_iter()
        .enumerate()
        .map(|(i, text)| {
            let text = if i > 0 { &text[padding.len()..] } else { &text };
            let start = offset
                + plain[offset..]
                    .find(text)
                    .expect("wrapped text belongs to source");
            let end = start + text.len();
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
        .collect()
}
