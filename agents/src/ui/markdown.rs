use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::{
    style::Style,
    text::{Line, Span},
};

pub(crate) fn render(text: &str, width: usize) -> Vec<Line<'static>> {
    if width == 0 {
        return Vec::new();
    }
    let mut writer = Writer::default();
    for event in Parser::new_ext(text, Options::ENABLE_STRIKETHROUGH) {
        match event {
            Event::Start(tag) => {
                let style = writer.style();
                match tag {
                    Tag::Paragraph if writer.indent == 0 => writer.separate(),
                    Tag::Heading { level, .. } => {
                        writer.separate();
                        writer.styles.push(match level {
                            HeadingLevel::H1 => style.bold().underlined(),
                            HeadingLevel::H2 => style.bold(),
                            HeadingLevel::H3 => style.bold().italic(),
                            _ => style.italic(),
                        });
                    }
                    Tag::Emphasis => writer.styles.push(style.italic()),
                    Tag::Strong => writer.styles.push(style.bold()),
                    Tag::Strikethrough => writer.styles.push(style.crossed_out()),
                    Tag::Link { .. } => writer.styles.push(style.cyan().underlined()),
                    Tag::BlockQuote => {
                        writer.separate();
                        writer.styles.push(style.green());
                    }
                    Tag::CodeBlock(_) => {
                        writer.separate();
                        writer.code = true;
                        writer.styles.push(style.cyan());
                    }
                    Tag::List(start) => {
                        writer.flush();
                        writer.lists.push(start);
                    }
                    Tag::Item => {
                        writer.flush();
                        let padding = (writer.lists.len() - 1) * 4;
                        let marker = match writer.lists.last_mut() {
                            Some(Some(index)) => {
                                let marker = Span::styled(
                                    format!("{:>padding$}{index}. ", ""),
                                    Style::new().light_blue(),
                                );
                                *index += 1;
                                marker
                            }
                            _ => Span::raw(format!("{}• ", " ".repeat(padding))),
                        };
                        writer.indent = marker.width();
                        writer.current.spans.push(marker);
                    }
                    _ => {}
                }
            }
            Event::End(tag) => match tag {
                TagEnd::Paragraph => writer.flush(),
                TagEnd::Heading(_) | TagEnd::BlockQuote | TagEnd::CodeBlock => {
                    writer.flush();
                    writer.styles.pop();
                    writer.code = false;
                }
                TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough | TagEnd::Link => {
                    writer.styles.pop();
                }
                TagEnd::Item => writer.flush(),
                TagEnd::List(_) => {
                    writer.flush();
                    writer.lists.pop();
                    writer.indent = writer.lists.last().map_or(0, |index| {
                        (writer.lists.len() - 1) * 4
                            + index.map_or(2, |n| n.saturating_sub(1).to_string().len() + 2)
                    });
                }
                _ => {}
            },
            Event::Text(text) if writer.code => {
                for part in text.split_inclusive('\n') {
                    writer.push(part.trim_end_matches('\n'), writer.style());
                    if part.ends_with('\n') {
                        writer.flush_code();
                    }
                }
            }
            Event::Text(text) | Event::Html(text) | Event::InlineHtml(text) => {
                writer.push(&text, writer.style())
            }
            Event::Code(text) => writer.push(&text, writer.style().cyan()),
            Event::SoftBreak => writer.push(" ", writer.style()),
            Event::HardBreak => writer.flush(),
            Event::Rule => {
                writer.separate();
                writer.push("───", writer.style());
                writer.flush();
            }
            _ => {}
        }
    }
    writer.flush();
    writer
        .lines
        .into_iter()
        .flat_map(|(line, indent)| wrap(line, indent, width))
        .collect()
}

#[derive(Default)]
struct Writer {
    lines: Vec<(Line<'static>, usize)>,
    current: Line<'static>,
    styles: Vec<Style>,
    lists: Vec<Option<u64>>,
    indent: usize,
    code: bool,
}

impl Writer {
    fn style(&self) -> Style {
        self.styles.last().copied().unwrap_or_default()
    }

    fn push(&mut self, text: &str, style: Style) {
        let text = text
            .chars()
            .map(|ch| if ch.is_control() { ' ' } else { ch })
            .collect::<String>();
        if self.current.spans.is_empty() && self.indent > 0 {
            self.current.spans.push(Span::raw(" ".repeat(self.indent)));
        }
        self.current.spans.push(Span::styled(text, style));
    }

    fn flush(&mut self) {
        if !self.current.spans.is_empty() {
            self.flush_code();
        }
    }

    fn flush_code(&mut self) {
        self.lines
            .push((std::mem::take(&mut self.current), self.indent));
    }

    fn separate(&mut self) {
        self.flush();
        if self
            .lines
            .last()
            .is_some_and(|(line, _)| !line.spans.is_empty())
        {
            self.lines.push((Line::default(), 0));
        }
    }
}

fn wrap(line: Line<'static>, indent: usize, width: usize) -> Vec<Line<'static>> {
    if line.width() <= width {
        return vec![line];
    }
    let plain = line.to_string();
    let padding = " ".repeat(indent.min(width.saturating_sub(1)));
    let options = textwrap::Options::new(width)
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

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::{Color, Modifier};

    fn plain(lines: &[Line<'_>]) -> Vec<String> {
        lines.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn inline_styles() {
        let lines = render(
            "`code` **bold** *italic* ~~gone~~ [label](https://example.com)",
            80,
        );
        for (text, color, modifier) in [
            ("code", Some(Color::Cyan), Modifier::empty()),
            ("bold", None, Modifier::BOLD),
            ("italic", None, Modifier::ITALIC),
            ("gone", None, Modifier::CROSSED_OUT),
            ("label", Some(Color::Cyan), Modifier::UNDERLINED),
        ] {
            let span = lines[0]
                .spans
                .iter()
                .find(|span| span.content == text)
                .unwrap();
            assert_eq!(span.style.fg, color);
            assert!(span.style.add_modifier.contains(modifier));
        }
    }

    #[test]
    fn lists_and_continuations() {
        assert_eq!(
            plain(&render("- one two three\n  - nested", 10)),
            ["• one two", "  three", "    • nest", "      ed"]
        );
        let ordered = render("3. item", 20);
        assert_eq!(ordered[0].to_string(), "3. item");
        assert_eq!(ordered[0].spans[0].style.fg, Some(Color::LightBlue));
    }

    #[test]
    fn wrapping_preserves_styles_and_width() {
        let lines = render("**hello world** 中文 👩‍💻abcdef", 7);
        assert!(lines.len() > 1);
        assert!(lines.iter().all(|line| line.width() <= 7));
        assert!(
            lines[0].spans[0]
                .style
                .add_modifier
                .contains(Modifier::BOLD)
        );
        assert_eq!(render("text", 0), Vec::<Line<'static>>::new());
    }

    #[test]
    fn plain_text_and_paragraphs() {
        assert_eq!(plain(&render("plain text", 40)), ["plain text"]);
        assert_eq!(
            plain(&render("first\n\nsecond", 40)),
            ["first", "", "second"]
        );
        assert_eq!(plain(&render("one\u{1b}two\tthree", 40)), ["one two three"]);
    }

    #[test]
    fn headings_quotes_and_code() {
        for (source, modifier) in [
            ("# title", Modifier::BOLD | Modifier::UNDERLINED),
            ("## title", Modifier::BOLD),
            ("### title", Modifier::BOLD | Modifier::ITALIC),
            ("#### title", Modifier::ITALIC),
            ("##### title", Modifier::ITALIC),
            ("###### title", Modifier::ITALIC),
        ] {
            let lines = render(source, 40);
            assert_eq!(lines[0].to_string(), "title");
            assert!(lines[0].spans[0].style.add_modifier.contains(modifier));
        }
        assert_eq!(
            render("> quote", 40)[0].spans[0].style.fg,
            Some(Color::Green)
        );
        for source in ["```rust\n  code\nnext\n```", "      code\n    next"] {
            let lines = render(source, 40);
            assert_eq!(plain(&lines), ["  code", "next"]);
            assert!(
                lines
                    .iter()
                    .flat_map(|line| &line.spans)
                    .all(|span| span.style.fg == Some(Color::Cyan))
            );
        }
    }
}
