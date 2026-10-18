//! Event and tool titles drawn inside the detail panel's content column.

use super::detail_model::{DetailColumns, TitleRow};
use super::line_truncation::truncate_line_with_ellipsis_if_overflow;
use super::text_selection_layout::{CellMap, map_lines_in};
use ratatui::text::{Line, Span};

impl TitleRow {
    /// Draw the text area; cell columns are relative to its left edge.
    /// The caller draws `time` separately in `columns.time` with `meta_style`.
    #[cfg(test)]
    pub fn line(&self, columns: DetailColumns) -> (Line<'static>, Vec<CellMap>) {
        self.line_in(columns, std::path::Path::new("/nonexistent"))
    }

    pub fn line_in(
        &self,
        columns: DetailColumns,
        cwd: &std::path::Path,
    ) -> (Line<'static>, Vec<CellMap>) {
        let width = usize::from(columns.text.width);
        let source = self.source();
        let paths = super::paths::text_paths(&source, cwd);
        let display = super::line_truncation::abbreviate_paths(&source, &paths, width, cwd);
        let drawn = Line::from(Span::styled(
            display.replace('\t', "    "),
            self.title_style,
        ));
        let mut maps =
            map_lines_in(&source, std::slice::from_ref(&drawn), false, Some(cwd)).remove(0);
        let mut title = super::log::truncate_detail_line(drawn, &mut maps, width);
        for span in &mut title.spans {
            span.style = self.title_style;
        }
        let padding = usize::from(
            super::centering::centered_x(columns.panel, columns.text, title.width() as u16)
                - columns.text.x,
        );
        for (cells, _) in &mut maps {
            cells.start += padding as u16;
            cells.end += padding as u16;
        }
        let label = self.left_label(padding);

        let label_width = label.width();
        let mut spans = label.spans;
        if padding > label_width {
            spans.push(Span::styled(
                " ".repeat(padding - label_width),
                self.meta_style,
            ));
        }
        let title_width = title.width();
        spans.extend(title.spans);
        spans.push(Span::styled(
            " ".repeat(width - padding - title_width),
            self.meta_style,
        ));
        (Line::from(spans), maps)
    }

    /// The title's copy source, excluding metadata and generated padding.
    pub fn source(&self) -> String {
        self.title.clone()
    }

    fn left_label(&self, width: usize) -> Line<'static> {
        let text = self
            .left
            .as_deref()
            .unwrap_or("")
            .replace(['\n', '\r', '\t'], " ");
        truncate_line_with_ellipsis_if_overflow(
            super::style::exit_code_line(text, self.exit_code, self.meta_style),
            width,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::super::detail_model::{DetailColumns, detail_columns};
    use super::*;
    use ratatui::{
        layout::Rect,
        style::{Color, Style},
    };

    fn row(title: &str) -> TitleRow {
        TitleRow {
            left: None,
            exit_code: None,
            title: title.into(),
            title_style: Style::new().fg(Color::Blue).bold(),
            time: Some("12s".into()),
            meta_style: Style::new().dim(),
        }
    }

    fn columns(width: u16) -> DetailColumns {
        detail_columns(Rect::new(10, 2, width, 8), Rect::new(0, 0, 40, 8))
    }

    #[test]
    fn short_titles_use_the_panel_midline_in_wide_and_narrow_panels() {
        for (width, expected_x) in [(100, 58), (40, 28)] {
            let columns = columns(width);
            let row = row("Bash");
            let (line, maps) = row.line(columns);
            assert_eq!(columns.text.x + maps.first().unwrap().0.start, expected_x);
            assert_eq!(line.width(), usize::from(columns.text.width));
            assert!(!line.to_string().contains('─'));
            assert!(!line.to_string().contains("12s"));
            assert_eq!(columns.time.right(), columns.panel.right());
            assert_eq!(columns.time.x - columns.text.right(), 2);
            assert!(
                line.spans
                    .iter()
                    .filter(|span| span.content.contains("Bash"))
                    .all(|span| span.style == row.title_style)
            );
        }
    }

    #[test]
    fn long_titles_move_against_the_gap_and_overflow_starts_without_padding() {
        for width in [100, 40] {
            let columns = columns(width);
            let row = row(&"T".repeat(usize::from(columns.text.width - 4)));
            let (_, maps) = row.line(columns);
            assert_eq!(
                columns.text.x + maps.last().unwrap().0.end,
                columns.text.right()
            );
            let row = row_title_with_metadata(&"T".repeat(usize::from(width)));
            let (line, maps) = row.line(columns);
            assert!(line.to_string().starts_with('T'));
            assert!(line.to_string().ends_with('…'));
            assert_eq!(maps.first().unwrap().0.start, 0);
            assert_eq!(maps.last().unwrap().0.end, columns.text.width);
            assert_eq!(
                maps.iter()
                    .map(|(_, bytes)| &row.title[bytes.clone()])
                    .collect::<String>(),
                row.title
            );
        }
    }

    #[test]
    fn oversized_wide_titles_start_at_text_left_and_end_at_the_gap() {
        for width in [100, 40, 39] {
            let columns = columns(width);
            let row = row(&"界".repeat(100));
            let (line, maps) = row.line(columns);
            assert!(line.to_string().starts_with('界'));
            assert!(line.to_string().ends_with('…'));
            assert_eq!(maps.first().unwrap().0.start, 0);
            assert_eq!(maps.last().unwrap().0.end, columns.text.width);
            assert_eq!(line.width(), usize::from(columns.text.width));
            assert!(!line.to_string().contains('─'));
            assert_eq!(
                maps.iter()
                    .map(|(_, bytes)| &row.title[bytes.clone()])
                    .collect::<String>(),
                row.title
            );
        }
    }

    fn row_title_with_metadata(title: &str) -> TitleRow {
        let mut row = row(title);
        row.left = Some("Task very-long-metadata".into());
        row
    }

    #[test]
    fn metadata_fills_only_the_space_left_of_the_title() {
        let columns = columns(40);
        let row = row_title_with_metadata("Bash");
        let (line, maps) = row.line(columns);
        let left_width = usize::from(maps.first().unwrap().0.start);
        assert_eq!(left_width, 18);
        let mut buffer = ratatui::buffer::Buffer::empty(columns.text);
        ratatui::widgets::Widget::render(line.clone(), columns.text, &mut buffer);
        assert_eq!(
            buffer[(columns.text.x + left_width as u16 - 1, columns.text.y)].symbol(),
            "…"
        );
        for x in columns.text.x..columns.text.x + left_width as u16 {
            assert!(
                buffer[(x, columns.text.y)]
                    .modifier
                    .contains(ratatui::style::Modifier::DIM)
            );
        }
        assert_eq!(
            buffer[(columns.text.x + left_width as u16, columns.text.y)].fg,
            row.title_style.fg.unwrap()
        );
        assert_eq!(
            maps.iter()
                .map(|(_, bytes)| &row.title[bytes.clone()])
                .collect::<String>(),
            "Bash"
        );
    }

    #[test]
    fn plain_titles_fill_text_and_unicode_ellipsis_copies_the_hidden_suffix() {
        for title in [
            "Bash",
            "看探针结果",
            "a",
            "a界e\u{301}",
            "界e\u{301} trailing title",
            "  Read 界面  Read 界面",
        ] {
            for width in 0..=100 {
                let columns = columns(width);
                let row = row(title);
                let (line, maps) = row.line(columns);
                assert_eq!(line.width(), usize::from(columns.text.width));
                assert!(!line.to_string().contains('─'));
                if columns.text.width == 0 {
                    assert!(maps.is_empty());
                } else {
                    assert_eq!(
                        maps.iter()
                            .map(|(_, bytes)| &title[bytes.clone()])
                            .collect::<String>(),
                        title
                    );
                    assert!(
                        maps.iter()
                            .all(|(cells, _)| cells.end <= columns.text.width)
                    );
                }
            }
        }
    }
}
