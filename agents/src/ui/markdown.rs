use super::{
    content::{Block, Kind},
    highlight,
};
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::{
    style::Style,
    text::{Line, Span},
};

/// Recognize supported Markdown structure without treating ordinary line breaks as prose.
pub(super) fn has_markers(text: &str) -> bool {
    Parser::new_ext(text, Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES).any(|event| {
        matches!(
            event,
            Event::Start(
                Tag::Heading { .. }
                    | Tag::CodeBlock(pulldown_cmark::CodeBlockKind::Fenced(_))
                    | Tag::List(_)
                    | Tag::Strong
                    | Tag::Link { .. }
                    | Tag::Table(_)
            ) | Event::Code(_)
        )
    })
}

pub(crate) fn render(text: &str, width: usize) -> Vec<Line<'static>> {
    render_with_options(text, textwrap::Options::new(width))
}

pub(super) fn render_with_options(
    text: &str,
    options: textwrap::Options<'_>,
) -> Vec<Line<'static>> {
    blocks(text)
        .into_iter()
        .flat_map(|block| block.layout(options.clone()))
        .collect()
}

pub(super) fn blocks(text: &str) -> Vec<Block> {
    let mut writer = Writer::default();
    for event in Parser::new_ext(text, Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES) {
        match event {
            Event::Start(tag) => {
                let style = writer.style();
                match tag {
                    Tag::Table(_) => {
                        writer.separate();
                        writer.kind = Kind::Preformatted;
                        writer.table = Some(Vec::new());
                    }
                    Tag::TableHead => {
                        writer.styles.push(style.bold());
                        writer.table.get_or_insert_default().push(Vec::new());
                    }
                    Tag::TableRow => writer.table.get_or_insert_default().push(Vec::new()),
                    Tag::TableCell => {
                        if let Some(row) = writer.table.as_mut().and_then(|rows| rows.last_mut()) {
                            row.push(Line::default());
                        }
                    }
                    Tag::Paragraph if writer.indent == 0 => writer.separate(),
                    Tag::Heading { level, .. } => {
                        writer.separate();
                        writer.styles.push(match level {
                            HeadingLevel::H1 | HeadingLevel::H2 => style.bold(),
                            HeadingLevel::H3 => style.bold().italic(),
                            _ => style.italic(),
                        });
                    }
                    Tag::Emphasis => writer.styles.push(style.italic()),
                    Tag::Strong => writer.styles.push(style.bold()),
                    Tag::Strikethrough => writer.styles.push(style.crossed_out()),
                    // A link reads as its text; the link layer underlines it like a path.
                    Tag::Link { .. } => writer.styles.push(style),
                    Tag::BlockQuote => {
                        writer.separate();
                        writer.styles.push(style.green());
                    }
                    Tag::CodeBlock(kind) => {
                        writer.separate();
                        writer.code = true;
                        writer.code_lang = match kind {
                            CodeBlockKind::Fenced(info) => {
                                info.split_whitespace().next().map(str::to_owned)
                            }
                            CodeBlockKind::Indented => None,
                        };
                        writer.kind = Kind::Preformatted;
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
                                    Style::new().fg(crate::theme::text_accent_color()),
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
                TagEnd::TableHead => {
                    writer.styles.pop();
                }
                TagEnd::Table => {
                    let rows = writer.table.take().unwrap_or_default();
                    writer.table_lines(rows);
                    writer.kind = Kind::Prose;
                }
                TagEnd::Paragraph => writer.flush(),
                TagEnd::CodeBlock => {
                    writer.code_lines();
                    writer.styles.pop();
                    writer.code = false;
                    writer.kind = Kind::Prose;
                }
                TagEnd::Heading(_) | TagEnd::BlockQuote => {
                    writer.flush();
                    writer.styles.pop();
                    writer.code = false;
                    writer.kind = Kind::Prose;
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
            Event::Text(text) if writer.code => writer.code_text.push_str(&text),
            Event::Text(text) | Event::Html(text) | Event::InlineHtml(text) => {
                writer.push(&text, writer.style())
            }
            Event::Code(text) => {
                writer.push(&text, writer.style().fg(crate::theme::inline_code_color()))
            }
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
    writer.lines
}

#[derive(Default)]
struct Writer {
    lines: Vec<Block>,
    current: Line<'static>,
    styles: Vec<Style>,
    lists: Vec<Option<u64>>,
    indent: usize,
    code: bool,
    code_text: String,
    code_lang: Option<String>,
    kind: Kind,
    /// Cells of the table being parsed, row by row; the first row is the header.
    table: Option<Vec<Vec<Line<'static>>>>,
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
        if let Some(cell) = self
            .table
            .as_mut()
            .and_then(|rows| rows.last_mut())
            .and_then(|row| row.last_mut())
        {
            cell.spans.push(Span::styled(text, style));
            return;
        }
        if self.current.spans.is_empty() && self.indent > 0 {
            self.current.spans.push(Span::raw(" ".repeat(self.indent)));
        }
        self.current.spans.push(Span::styled(text, style));
    }

    /// Align each column to its widest cell, divide columns with rules and underline the header.
    fn table_lines(&mut self, rows: Vec<Vec<Line<'static>>>) {
        let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
        let widths: Vec<usize> = (0..columns)
            .map(|column| {
                rows.iter()
                    .filter_map(|row| row.get(column))
                    .map(Line::width)
                    .max()
                    .unwrap_or(0)
            })
            .collect();
        let style = self.style();
        for (index, row) in rows.into_iter().enumerate() {
            let mut cells = row.into_iter();
            for (column, width) in widths.iter().enumerate() {
                if column > 0 {
                    self.current.spans.push(Span::styled(" │ ", style));
                }
                let cell = cells.next().unwrap_or_default();
                let padding = width - cell.width();
                self.current.spans.extend(cell.spans);
                if column + 1 < columns && padding > 0 {
                    self.current.spans.push(Span::raw(" ".repeat(padding)));
                }
            }
            self.flush_code();
            if index == 0 {
                let rule = widths
                    .iter()
                    .enumerate()
                    .map(|(column, width)| {
                        let gaps = usize::from(column > 0) + usize::from(column + 1 < columns);
                        "─".repeat(width + gaps)
                    })
                    .collect::<Vec<_>>()
                    .join("┼");
                self.current.spans.push(Span::styled(rule, style));
                self.flush_code();
            }
        }
    }

    fn code_lines(&mut self) {
        let text = std::mem::take(&mut self.code_text)
            .chars()
            .map(|ch| {
                if ch.is_control() && ch != '\n' {
                    ' '
                } else {
                    ch
                }
            })
            .collect::<String>();
        let lang = self.code_lang.take();
        let mut highlighted = lang
            .filter(|lang| highlight::find_syntax(lang).is_some())
            .map(|lang| highlight::code_lines(&text, &lang).into_iter());
        for part in text.split_inclusive('\n') {
            if let Some(lines) = &mut highlighted {
                let line = lines.next().expect("highlighted code line");
                if line.spans.is_empty() {
                    self.push("", self.style());
                }
                for span in line.spans {
                    self.push(&span.content, span.style);
                }
            } else {
                self.push(part.trim_end_matches('\n'), self.style());
            }
            if part.ends_with('\n') {
                self.flush_code();
            }
        }
        self.flush();
    }

    fn flush(&mut self) {
        if !self.current.spans.is_empty() {
            self.flush_code();
        }
    }

    fn flush_code(&mut self) {
        self.lines.push(Block {
            lines: vec![std::mem::take(&mut self.current)],
            indent: self.indent,
            kind: self.kind,
        });
    }

    fn separate(&mut self) {
        self.flush();
        if self
            .lines
            .last()
            .is_some_and(|block| block.lines.iter().any(|line| !line.spans.is_empty()))
        {
            self.lines.push(Block {
                lines: vec![Line::default()],
                indent: 0,
                kind: Kind::Prose,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::{Color, Modifier};

    fn plain(lines: &[Line<'_>]) -> Vec<String> {
        lines.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn tables_render_cells_and_preserve_selection_sources() {
        let source = "| Name | Value |\n| --- | --- |\n| alpha | **two** |";
        let lines = render(source, 40);
        assert_eq!(
            plain(&lines),
            ["Name  │ Value", "──────┼──────", "alpha │ two"]
        );
        assert!(
            lines[0].spans[0]
                .style
                .add_modifier
                .contains(Modifier::BOLD)
        );
        let maps = super::super::text_selection_layout::map_lines(source, &lines, true);
        let name = source.find("Name").unwrap();
        let two = source.find("two").unwrap();
        assert_eq!(maps[0][0].1, name..name + 1);
        assert!(maps[1].is_empty());
        assert_eq!(maps[2].last().unwrap().1, two + 2..two + 3);
    }

    #[test]
    fn inline_styles() {
        let _guard = crate::theme::TEST_LOCK.lock().unwrap();
        crate::theme::use_default_accents(true);
        let lines = render(
            "`code` **bold** *italic* ~~gone~~ [label](https://example.com)",
            80,
        );
        for (text, color, modifier) in [
            ("code", Some(Color::Indexed(18)), Modifier::empty()),
            ("bold", None, Modifier::BOLD),
            ("italic", None, Modifier::ITALIC),
            ("gone", None, Modifier::CROSSED_OUT),
            ("label", None, Modifier::empty()),
        ] {
            let span = lines[0]
                .spans
                .iter()
                .find(|span| span.content == text)
                .unwrap();
            assert_eq!(span.style.fg, color);
            assert_eq!(span.style.add_modifier, modifier);
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
        assert_eq!(
            ordered[0].spans[0].style.fg,
            Some(crate::theme::text_accent_color())
        );
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
    fn fenced_code_uses_language_styles() {
        let _guard = crate::theme::TEST_LOCK.lock().unwrap();
        let code = "/* comment\n   continued */\nfn main() {\n    let text = \"中文\";\n}\n";
        for info in ["rust", "rs", "rust extra words"] {
            let lines = render(&format!("```{info}\n{code}```"), 80);
            assert_eq!(lines, super::super::highlight::code_lines(code, "rs"));
        }
    }

    #[test]
    fn code_without_known_language_stays_cyan() {
        for source in [
            "```unknown-language\n  code\nnext\n```",
            "```\n  code\nnext\n```",
            "      code\n    next",
        ] {
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

    #[test]
    fn fenced_code_preserves_layout_and_selection_sources() {
        for prefix in ["", "- item\n\n  ", "- outer\n  - item\n\n    "] {
            let code = "/* 中文\n   continued */\n\n\tlet text = \"&amp;\";\n";
            let fenced = format!("```rust\n{code}```");
            let source = if prefix.is_empty() {
                fenced
            } else {
                let indent = prefix.lines().last().unwrap();
                format!("{prefix}{}", fenced.replace('\n', &format!("\n{indent}")))
            };
            let plain_source = source.replacen("```rust", "```", 1);
            let lines = render(&source, 80);
            let plain_lines = render(&plain_source, 80);
            assert_eq!(plain(&lines), plain(&plain_lines));
            let maps = super::super::text_selection_layout::map_lines(&source, &lines, true);
            let plain_maps =
                super::super::text_selection_layout::map_lines(&plain_source, &plain_lines, true);
            let selected =
                |source: &str, maps: Vec<Vec<super::super::text_selection_layout::CellMap>>| {
                    maps.into_iter()
                        .map(|row| {
                            row.into_iter()
                                .map(|(cells, bytes)| (cells, source[bytes].to_owned()))
                                .collect::<Vec<_>>()
                        })
                        .collect::<Vec<_>>()
                };
            assert_eq!(selected(&source, maps), selected(&plain_source, plain_maps));
            let blocks = blocks(&source);
            let plain_blocks = super::blocks(&plain_source);
            assert_eq!(blocks.len(), plain_blocks.len());
            for (block, plain_block) in blocks.iter().zip(&plain_blocks) {
                assert_eq!(
                    std::mem::discriminant(&block.kind),
                    std::mem::discriminant(&plain_block.kind)
                );
                assert_eq!(block.indent, plain_block.indent);
            }
        }
    }

    #[test]
    fn headings_quotes_and_code() {
        for (source, modifier) in [
            ("# title", Modifier::BOLD),
            ("## title", Modifier::BOLD),
            ("### title", Modifier::BOLD | Modifier::ITALIC),
            ("#### title", Modifier::ITALIC),
            ("##### title", Modifier::ITALIC),
            ("###### title", Modifier::ITALIC),
        ] {
            let lines = render(source, 40);
            assert_eq!(lines[0].to_string(), "title");
            assert_eq!(lines[0].spans[0].style.add_modifier, modifier);
        }
        assert_eq!(
            render("> quote", 40)[0].spans[0].style.fg,
            Some(Color::Green)
        );
        for source in ["```rust\n  code\nnext\n```", "      code\n    next"] {
            assert_eq!(plain(&render(source, 40)), ["  code", "next"]);
        }
    }
}
