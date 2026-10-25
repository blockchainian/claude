use super::{
    content::{Block, Kind, Table},
    highlight,
};
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::{
    style::Style,
    text::{Line, Span},
};
use std::{
    collections::HashMap,
    io::Write,
    process::{Command, Stdio},
    sync::{LazyLock, Mutex},
};

fn mermaid(text: &str) -> Option<String> {
    static CACHE: LazyLock<Mutex<HashMap<String, Option<String>>>> =
        LazyLock::new(|| Mutex::new(HashMap::new()));
    CACHE
        .lock()
        .expect("mermaid cache lock")
        .entry(text.to_owned())
        .or_insert_with(|| {
            let mut child = Command::new("mermaid-ascii")
                .args(["-f", "-"])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .ok()?;
            let written = child
                .stdin
                .take()
                .expect("mermaid stdin")
                .write_all(text.as_bytes());
            let output = child.wait_with_output().ok()?;
            written.ok()?;
            if !output.status.success() {
                return None;
            }
            String::from_utf8(output.stdout).ok()
        })
        .clone()
}

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
                        writer
                            .styles
                            .push(style.patch(highlight::scope_style("markup.quote")));
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
                        writer
                            .styles
                            .push(style.patch(highlight::scope_style("markup.raw.block")));
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

    fn table_lines(&mut self, rows: Vec<Vec<Line<'static>>>) {
        self.lines.push(Block {
            lines: Vec::new(),
            indent: self.indent,
            kind: Kind::Table(Table::new(rows, self.style())),
        });
    }

    fn code_lines(&mut self) {
        let text = std::mem::take(&mut self.code_text);
        let lang = self.code_lang.take();
        let diagram = lang.as_deref() == Some("mermaid");
        if diagram && let Some(output) = mermaid(&text) {
            for part in output.lines() {
                self.push(part, Style::default());
                self.flush_code();
            }
            return;
        }
        let text = text
            .chars()
            .map(|ch| {
                if ch.is_control() && ch != '\n' {
                    ' '
                } else {
                    ch
                }
            })
            .collect::<String>();
        let mut highlighted = lang
            .filter(|lang| highlight::find_syntax(lang).is_some())
            .map(|lang| highlight::code_block(&text, &lang).into_iter());
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
        if diagram {
            self.push(
                "Install mermaid-ascii to render this diagram.",
                super::style::time_style(),
            );
            self.flush_code();
        }
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
            kind: self.kind.clone(),
        });
    }

    fn separate(&mut self) {
        self.flush();
        if self.lines.last().is_some_and(|block| {
            matches!(block.kind, Kind::Table(_))
                || block.lines.iter().any(|line| !line.spans.is_empty())
        }) {
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

    fn mermaid_process(name: &str, installed: bool) -> bool {
        if std::env::var_os("AGENTS_MERMAID_TEST").is_some() {
            return true;
        }
        let directory = tempfile::tempdir().unwrap();
        if installed {
            let binary = std::path::PathBuf::from(std::env::var_os("HOME").unwrap())
                .join("go/bin/mermaid-ascii");
            assert!(
                binary.is_file(),
                "install mermaid-ascii in ~/go/bin for this test"
            );
            std::os::unix::fs::symlink(binary, directory.path().join("mermaid-ascii")).unwrap();
        }
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                &format!("ui::markdown::tests::{name}"),
                "--nocapture",
            ])
            .env("AGENTS_MERMAID_TEST", "1")
            .env("PATH", directory.path())
            .status()
            .unwrap();
        assert!(status.success());
        false
    }

    #[test]
    fn bold_next_to_cjk_punctuation() {
        for (source, bold) in [
            ("**适用范围太窄。**这条规则开头是…", "适用范围太窄。"),
            ("**放错了位置。**“发给…", "放错了位置。"),
            (
                "**改 CLAUDE.md 的适用范围。**把",
                "改 CLAUDE.md 的适用范围。",
            ),
            ("中文**「粗体」**后文", "「粗体」"),
            ("中文**(bold)**后文", "(bold)"),
            ("a**「bold」**b", "「bold」"),
            ("한**(bold)**글", "(bold)"),
            ("ｶ**(bold)**ﾅ", "(bold)"),
            ("字\u{e0100}**(bold)**后", "(bold)"),
            ("字\u{fe00}**(bold)**后", "(bold)"),
            ("a**「\u{fe00}bold」\u{fe00}**b", "「\u{fe00}bold」\u{fe00}"),
        ] {
            let lines = render(source, 100);
            assert_eq!(plain(&lines).join(""), source.replace("**", ""), "{source}");
            let styled: String = lines
                .iter()
                .flat_map(|line| &line.spans)
                .filter(|span| span.style.add_modifier.contains(Modifier::BOLD))
                .map(|span| span.content.as_ref())
                .collect();
            assert_eq!(styled, bold, "{source}");
            assert!(has_markers(source), "{source}");
        }
    }

    #[test]
    fn cjk_flanking_preserves_other_delimiter_rules() {
        let lines = render("a**bold!\u{fe00}**b", 100);
        assert_eq!(plain(&lines), ["abold!\u{fe00}b"]);
        assert!(lines[0].spans.iter().any(|span| {
            span.content == "bold!\u{fe00}" && span.style.add_modifier.contains(Modifier::BOLD)
        }));
        for source in [
            "a**(bold)**b",
            "a\u{fe00}**(bold)**b",
            "a**😀bold😀**b",
            "字** bold**后",
            "字**bold **后",
            "字__bold__后",
            "a__「bold」__b",
            "`**适用范围太窄。**这`",
        ] {
            let lines = render(source, 100);
            assert!(
                lines
                    .iter()
                    .flat_map(|line| &line.spans)
                    .all(|span| { !span.style.add_modifier.contains(Modifier::BOLD) }),
                "{source}"
            );
        }
    }

    #[test]
    fn mermaid_renders_box_drawing_and_caches_across_widths() {
        if !mermaid_process("mermaid_renders_box_drawing_and_caches_across_widths", true) {
            return;
        }
        let source = "```mermaid\ngraph LR\n A[Start] --> B[Finish]\n```";
        let lines = render(source, 80);
        assert!(plain(&lines).join("\n").contains('┌'));
        assert!(plain(&lines).join("\n").contains("Start"));
        assert!(
            lines
                .iter()
                .flat_map(|line| &line.spans)
                .all(|span| span.style == Style::default())
        );
        std::fs::remove_file(
            std::path::PathBuf::from(std::env::var_os("PATH").unwrap()).join("mermaid-ascii"),
        )
        .unwrap();
        assert_eq!(render(source, 10), lines);
    }

    #[test]
    fn mermaid_missing_program_preserves_source_and_caches_failure() {
        if !mermaid_process(
            "mermaid_missing_program_preserves_source_and_caches_failure",
            false,
        ) {
            return;
        }
        let source = "```mermaid\ngraph TD\n A[Missing] --> B[Program]\n```";
        let lines = render(source, 100);
        assert_eq!(
            plain(&lines),
            [
                "graph TD",
                " A[Missing] --> B[Program]",
                "Install mermaid-ascii to render this diagram."
            ]
        );
        assert_eq!(lines[0].spans[0].style.fg, Some(Color::Cyan));
        assert_eq!(lines[2].spans[0].style, super::super::style::time_style());
        let binary = std::path::PathBuf::from(std::env::var_os("HOME").unwrap())
            .join("go/bin/mermaid-ascii");
        std::os::unix::fs::symlink(
            binary,
            std::path::PathBuf::from(std::env::var_os("PATH").unwrap()).join("mermaid-ascii"),
        )
        .unwrap();
        assert_eq!(render(source, 80), lines);
    }

    #[test]
    fn mermaid_failed_program_preserves_source_and_hint() {
        if !mermaid_process("mermaid_failed_program_preserves_source_and_hint", true) {
            return;
        }
        assert_eq!(
            plain(&render("```mermaid\ninvalid diagram\n```", 100)),
            [
                "invalid diagram",
                "Install mermaid-ascii to render this diagram."
            ]
        );
    }

    #[test]
    fn mermaid_wide_diagram_is_clipped_without_wrapping() {
        if !mermaid_process("mermaid_wide_diagram_is_clipped_without_wrapping", true) {
            return;
        }
        let source = "```mermaid\ngraph LR\n A[Wide start] --> B[Wide finish]\n```";
        let lines = render(source, 10);
        assert!(lines.iter().any(|line| line.width() > 10));
        assert!(plain(&lines).join("\n").contains('─'));
        let area = ratatui::layout::Rect::new(0, 0, 10, lines.len() as u16);
        let mut buffer = ratatui::buffer::Buffer::empty(area);
        ratatui::widgets::Widget::render(
            ratatui::widgets::Paragraph::new(lines.clone()),
            area,
            &mut buffer,
        );
        for (row, line) in lines.iter().enumerate() {
            let visible: String = (0..10)
                .map(|column| buffer[(column, row as u16)].symbol())
                .collect();
            assert_eq!(
                visible.trim_end(),
                line.to_string()
                    .chars()
                    .take(10)
                    .collect::<String>()
                    .trim_end()
            );
        }
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

    const WIDE_TABLE: &str = "| Item | Status | Note |\n| --- | --- | --- |\n| F1 | done | The code is done and its first two tests went from failing to passing. |\n| F2 | wait | Committed as 9c4b6ab4; 118 targeted tests pass. |";

    #[test]
    fn wide_tables_wrap_cells_to_the_reading_width() {
        let lines = render(WIDE_TABLE, 50);
        assert_eq!(
            plain(&lines),
            [
                "Item │ Status │ Note",
                "─────┼────────┼───────────────────────────────────",
                "F1   │ done   │ The code is done and its first two",
                "     │        │ tests went from failing to",
                "     │        │ passing.",
                "─────┼────────┼───────────────────────────────────",
                "F2   │ wait   │ Committed as 9c4b6ab4; 118",
                "     │        │ targeted tests pass.",
            ]
        );
        for line in &lines {
            assert!(line.width() <= 50);
            if line.to_string().contains('│') {
                let text = line.to_string();
                let columns: Vec<_> = text
                    .char_indices()
                    .filter(|(_, ch)| *ch == '│')
                    .map(|(i, _)| unicode_width::UnicodeWidthStr::width(&text[..i]))
                    .collect();
                assert_eq!(columns, [5, 14]);
            }
        }
    }

    #[test]
    fn wrapped_table_selection_copies_cells_without_generated_rules() {
        use super::super::{
            Grouping, View,
            text_selection::{DrawnRow, Source, TextRegion},
        };
        use ratatui::layout::Rect;
        let (lines, breaks): (Vec<_>, Vec<_>) = blocks(WIDE_TABLE)
            .into_iter()
            .map(|block| block.layout_with_breaks(textwrap::Options::new(50)))
            .fold(
                (Vec::new(), Vec::new()),
                |(mut lines, mut breaks), (next, joins)| {
                    lines.extend(next);
                    breaks.extend(joins);
                    (lines, breaks)
                },
            );
        assert!(breaks.iter().any(Option::is_some));
        let maps = super::super::text_selection_layout::map_lines(WIDE_TABLE, &lines, true);
        let mut view = View::new(Grouping::Project, false);
        view.viewport.borrow_mut().event_text = TextRegion {
            area: Rect::new(0, 0, 50, lines.len() as u16),
            sources: vec![Source {
                id: 0,
                text: WIDE_TABLE.into(),
                label: "table".into(),
            }],
            rows: lines
                .iter()
                .zip(maps)
                .zip(breaks)
                .enumerate()
                .map(|(index, ((line, cells), continuation))| DrawnRow {
                    segment: 0,
                    index,
                    x: 0,
                    y: index as u16,
                    cells,
                    continuation,
                    text: line.to_string(),
                })
                .collect(),
        };
        // Select the Note cell from its first character through its last continuation.
        view.begin_text_selection(16, 2);
        view.extend_text_selection(24, 4);
        let Some(super::super::Action::Copy { text, .. }) = view.finish_text_selection(24, 4)
        else {
            panic!("copy action")
        };
        assert_eq!(
            text,
            "The code is done and its first two tests went from failing to passing."
        );
        assert!(maps_for_rules_are_empty(WIDE_TABLE, &lines));
    }

    fn maps_for_rules_are_empty(source: &str, lines: &[Line<'_>]) -> bool {
        super::super::text_selection_layout::map_lines(source, lines, true)
            .iter()
            .zip(lines)
            .filter(|(_, line)| line.to_string().contains('┼'))
            .all(|(map, _)| map.is_empty())
    }

    /// The recorded appium session call whose capabilities cell is a JSON string.
    fn capabilities_table() -> (serde_json::Value, String) {
        let capabilities = serde_json::json!({
            "appium:udid": "00000000-0000000000000000",
            "appium:usePreinstalledWDA": true,
            "appium:prebuiltWDAPath": "/Users/user/.cache/appium-mcp/wda-real/16.12.4/signed/23ed55a4-b472-45e1-b469-ee696e1e150c/Payload-resigned.ipa",
            "appium:wdaLaunchTimeout": 30000,
            "appium:shouldTerminateApp": false,
            "appium:newCommandTimeout": 600
        });
        let row = serde_json::json!({"platform": "ios", "capabilities": capabilities.to_string()});
        let source = crate::providers::tool::markdown_table(&row, "input").unwrap();
        (capabilities, source)
    }

    #[test]
    fn structured_table_cells_draw_like_structured_sections() {
        let (capabilities, source) = capabilities_table();
        let lines = render(&source, 60);
        let text = plain(&lines);
        assert_eq!(text[0], "platform │ capabilities");
        assert_eq!(text[2], "ios      │ {");
        assert_eq!(
            text[3],
            "         │   \"appium:udid\": \"00000000-0000000000000000\","
        );
        assert_eq!(text.last().unwrap(), "         │ }");
        let document = super::super::structured_text::parse(&capabilities.to_string()).unwrap();
        let expected: Vec<String> = document.lines(49).iter().map(ToString::to_string).collect();
        let cells: Vec<String> = text[2..]
            .iter()
            .map(|line| line.chars().skip(11).collect())
            .collect();
        assert_eq!(cells, expected);
        assert!(expected.len() > 8, "the long path wraps inside the column");
        for line in &lines {
            assert!(line.width() <= 60);
        }
        assert!(
            lines[3]
                .spans
                .iter()
                .any(|span| span.style.fg.is_some() && span.content.contains("appium:udid"))
        );
    }

    #[test]
    fn structured_table_cell_selection_copies_the_displayed_text() {
        use super::super::{
            Grouping, View,
            text_selection::{DrawnRow, Source, TextRegion},
        };
        use ratatui::layout::Rect;
        let (capabilities, source) = capabilities_table();
        let (lines, breaks): (Vec<_>, Vec<_>) = blocks(&source)
            .into_iter()
            .map(|block| block.layout_with_breaks(textwrap::Options::new(60)))
            .fold(
                (Vec::new(), Vec::new()),
                |(mut lines, mut breaks), (next, joins)| {
                    lines.extend(next);
                    breaks.extend(joins);
                    (lines, breaks)
                },
            );
        let maps = super::super::text_selection_layout::map_lines(&source, &lines, true);
        let mut view = View::new(Grouping::Project, false);
        view.viewport.borrow_mut().event_text = TextRegion {
            area: Rect::new(0, 0, 60, lines.len() as u16),
            sources: vec![Source {
                id: 0,
                text: source.clone().into(),
                label: "table".into(),
            }],
            rows: lines
                .iter()
                .zip(maps)
                .zip(breaks)
                .enumerate()
                .map(|(index, ((line, cells), continuation))| DrawnRow {
                    segment: 0,
                    index,
                    x: 0,
                    y: index as u16,
                    cells,
                    continuation,
                    text: line.to_string(),
                })
                .collect(),
        };
        let last = lines.len() as u16 - 1;
        view.begin_text_selection(11, 2);
        view.extend_text_selection(12, last);
        let Some(super::super::Action::Copy { text, .. }) = view.finish_text_selection(12, last)
        else {
            panic!("copy action")
        };
        let document = super::super::structured_text::parse(&capabilities.to_string()).unwrap();
        assert_eq!(text, document.text());
        assert!(maps_for_rules_are_empty(&source, &lines));
    }

    #[test]
    fn tables_wrap_cjk_styles_headers_and_indentation() {
        let source = "| **标题中文** | Note |\n| --- | --- |\n| 中文中文中文 | abcdefghijklmnopqrstuvwxyz |\n| 中文 | short |";
        let lines = render(source, 17);
        assert!(lines.iter().all(|line| line.width() <= 17));
        assert_eq!(
            lines
                .iter()
                .filter(|line| line.to_string().contains('┼'))
                .count(),
            2
        );
        for line in &lines {
            for span in &line.spans {
                if span.content.contains('标') || span.content.contains('题') {
                    assert!(span.style.add_modifier.contains(Modifier::BOLD));
                }
            }
        }
        let maps = super::super::text_selection_layout::map_lines(source, &lines, true);
        let mapped: Vec<_> = maps
            .iter()
            .flatten()
            .map(|(_, bytes)| bytes.clone())
            .collect();
        for (event, range) in Parser::new_ext(source, Options::ENABLE_TABLES).into_offset_iter() {
            if let Event::Text(text) = event {
                for (offset, ch) in text.char_indices() {
                    assert!(
                        mapped.contains(
                            &(range.start + offset..range.start + offset + ch.len_utf8())
                        )
                    );
                }
            }
        }
        let mut table = blocks(source).pop().unwrap();
        table.indent = 4;
        let indented = table.layout(textwrap::Options::new(21));
        assert_eq!(
            plain(&indented),
            plain(&lines)
                .iter()
                .map(|line| format!("    {line}"))
                .collect::<Vec<_>>()
        );
        assert_eq!(render(source, 0), Vec::<Line<'static>>::new());
    }

    #[test]
    fn table_width_allocation_preserves_short_columns_and_redistributes() {
        let source = format!(
            "| A | B | C |\n|---|---|---|\n| {} | {} | {} |",
            "a".repeat(6),
            "b".repeat(35),
            "c".repeat(100)
        );
        let lines = render(&source, 108);
        assert_eq!(
            lines[1].to_string(),
            format!("{}┼{}┼{}", "─".repeat(7), "─".repeat(37), "─".repeat(62))
        );
        assert!(lines.iter().all(|line| line.width() <= 108));
        let equal = render("| A | B |\n|---|---|\n| abcdefgh | ijklmnop |", 16);
        assert!(equal.iter().all(|line| line.width() <= 16));
        let narrow = render("| A | B |\n|---|---|\n| abcdefghijkl | mnopqrstuvwx |", 10);
        assert!(narrow.iter().all(|line| line.width() <= 10));
        assert_eq!(narrow[2].to_string(), "abcd │ mno");
        assert_eq!(narrow[3].to_string(), "efgh │ pqr");
    }

    #[test]
    fn wrapped_table_maps_each_column_with_prose_options() {
        let source = "before\n\n| Path | Note |\n|---|---|\n| /one/two/three/four/five/six/seven/eight/nine | **a &amp; b** |\n| /another/very/long/path/to/a/file.rs | tail |\n\nafter";
        let lines = render_with_options(
            source,
            super::super::line_truncation::prose_wrap_options(30),
        );
        let maps = super::super::text_selection_layout::map_lines(source, &lines, true);
        let ranges: Vec<_> = maps
            .iter()
            .flatten()
            .map(|(_, bytes)| bytes.clone())
            .collect();
        for label in [
            "/one/two/three/four/five/six/seven/eight/nine",
            "/another/very/long/path/to/a/file.rs",
            "tail",
            "after",
        ] {
            let start = source.find(label).unwrap();
            for offset in 0..label.len() {
                assert!(
                    ranges.contains(&(start + offset..start + offset + 1)),
                    "{label}: {offset}"
                );
            }
        }
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
            assert_eq!(lines, super::super::highlight::code_block(code, "rs"));
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
        let _guard = crate::theme::TEST_LOCK.lock().unwrap();
        highlight::use_theme(false, None).unwrap();
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
            Some(Color::Rgb(74, 139, 79))
        );
        for source in ["```rust\n  code\nnext\n```", "      code\n    next"] {
            assert_eq!(plain(&render(source, 40)), ["  code", "next"]);
        }
    }
}
