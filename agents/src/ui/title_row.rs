//! Event and tool titles drawn inside the detail panel's content column.

use super::detail_model::TitleRow;
use super::line_truncation::truncate_line_with_ellipsis_if_overflow;
use super::text_selection_layout::{CellMap, map_lines};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

impl TitleRow {
    /// Draw only the content column; the layout draws the time separately.
    pub fn line(&self, width: usize) -> (Line<'static>, Vec<CellMap>) {
        let source = self.source();
        if self.rule {
            let mut line = super::title_rule::centered_title_rule(&source, width, None);
            let maps = if line.spans.len() < 4 {
                Vec::new()
            } else {
                let padding = line.spans[0].width();
                let title = Line::from(line.spans[1..line.spans.len() - 2].to_vec());
                if self.left.is_some() {
                    let label = self.left_label(padding.saturating_sub(1));
                    line.spans[0] = Span::styled(
                        format!("{label}{}", " ".repeat(padding - label.width())),
                        self.meta_style,
                    );
                }
                title_maps(&source, title, padding)
            };
            return (line, maps);
        }

        let title = truncate_line_with_ellipsis_if_overflow(self.title.clone(), width);
        let title_width = title.width();
        let padding = (width - title_width) / 2;
        let maps = title_maps(&source, title.clone(), padding);
        let label = self.left_label(padding.saturating_sub(1));
        let mut spans = vec![Span::styled(
            format!("{label}{}", " ".repeat(padding - label.width())),
            self.meta_style,
        )];
        spans.extend(title.spans.into_iter().map(|mut span| {
            span.style = title.style.patch(span.style);
            span
        }));
        spans.push(Span::raw(" ".repeat(width - padding - title_width)));
        (Line::from(spans), maps)
    }

    /// The title's copy source, excluding metadata and generated padding or rules.
    pub fn source(&self) -> String {
        self.title.to_string()
    }

    fn left_label(&self, width: usize) -> Line<'static> {
        truncate_line_with_ellipsis_if_overflow(
            Line::from(
                self.left
                    .as_deref()
                    .unwrap_or("")
                    .replace(['\n', '\r', '\t'], " "),
            ),
            width,
        )
    }
}

/// An ellipsis selects all remaining title bytes, including hidden whitespace.
fn title_maps(source: &str, mut title: Line<'static>, padding: usize) -> Vec<CellMap> {
    let truncated = title.width() > 0 && source.replace('\t', "    ").width() > title.width();
    if truncated {
        title.spans.pop();
    }
    let mut maps = map_lines(source, std::slice::from_ref(&title), false).remove(0);
    if truncated {
        let start = maps.last().map_or(0, |(_, bytes)| bytes.end);
        let column = title.width() as u16;
        maps.push((column..column + 1, start..source.len()));
    }
    for (cells, _) in &mut maps {
        cells.start += padding as u16;
        cells.end += padding as u16;
    }
    maps
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::{Color, Style};

    fn row(title: &str, rule: bool) -> TitleRow {
        TitleRow {
            left: None,
            title: Line::from(title.to_owned()),
            rule,
            time: Some("12s".into()),
            meta_style: Style::new().dim(),
        }
    }

    #[test]
    fn rules_fill_the_content_column_and_copy_only_the_title() {
        for title in [
            "Bash",
            "看探针结果",
            "a",
            "a界e\u{301}",
            "  Read 界面  Read 界面  Read 界面",
        ] {
            let row = row(title, true);
            for width in 0..=100 {
                let (line, maps) = row.line(width);
                assert_eq!(line.width(), width);
                assert_eq!(
                    line,
                    super::super::title_rule::centered_title_rule(title, width, None)
                );
                assert!(
                    line.spans
                        .iter()
                        .all(|span| span.style == Style::new().dim())
                );
                if width < 5 {
                    continue;
                }
                let drawn = Line::from(line.spans[1..line.spans.len() - 2].to_vec());
                let left = line.spans[0].width();
                assert_eq!(left, (width - drawn.width() - 2) / 2 + 1);
                let copied: String = maps
                    .iter()
                    .map(|(_, bytes)| &title[bytes.clone()])
                    .collect();
                assert_eq!(copied, title);
                assert!(maps.iter().all(|(cells, _)| cells.start as usize >= left
                    && cells.end as usize <= left + drawn.width()));
                if title.width() > width - 4 {
                    assert!(drawn.to_string().ends_with('…'));
                } else {
                    assert_eq!(drawn.to_string(), title);
                }
            }
        }
    }

    #[test]
    fn rule_keeps_left_metadata_outside_the_centered_title() {
        let mut row = row("Read", true);
        row.left = Some("Task b9wi31ww0".into());
        let (line, maps) = row.line(60);
        assert!(line.to_string().starts_with("Task b9wi31ww0"));
        assert_eq!(maps.first().unwrap().0.start, 28);
        assert_eq!(maps.last().unwrap().0.end, 32);
        assert_eq!(line.width(), 60);
        assert!(!line.to_string().contains("12s"));
    }

    #[test]
    fn plain_title_has_priority_and_does_not_reserve_time_space() {
        let mut row = row("Read 界面", false);
        row.left = Some("long description".into());
        for width in 0..=40 {
            let (line, maps) = row.line(width);
            let title = truncate_line_with_ellipsis_if_overflow(row.title.clone(), width);
            let left = (width - title.width()) / 2;
            assert_eq!(line.width(), width);
            assert!(line.to_string().ends_with(&format!(
                "{title}{}",
                " ".repeat(width - left - title.width())
            )));
            if width > 0 {
                assert_eq!(maps[0].0.start as usize, left);
                let source = row.source();
                let copied: String = maps
                    .iter()
                    .map(|(_, bytes)| &source[bytes.clone()])
                    .collect();
                assert_eq!(copied, source);
            } else {
                assert!(maps.is_empty());
            }
        }
        assert_eq!(row.line(9).0.to_string(), "Read 界面");
    }

    #[test]
    fn left_label_is_single_line_truncated_and_uses_metadata_style() {
        let mut row = row("Bash", false);
        row.left = Some("a\nb\tc\rd".into());
        row.meta_style = Style::new().fg(Color::Cyan).dim();
        row.title.style = Style::new().bold();
        assert_eq!(row.line(24).0.to_string(), "a b c d   Bash          ");
        let (line, _) = row.line(16);
        assert_eq!(line.to_string(), "a b … Bash      ");
        assert_eq!(line.spans[0].style, row.meta_style);
        assert_eq!(line.spans[1].style, row.title.style);
    }

    #[test]
    fn ellipsis_maps_to_hidden_title_bytes_in_content_coordinates() {
        for rule in [false, true] {
            let row = row("界e\u{301} trailing title", rule);
            let (line, maps) = row.line(if rule { 8 } else { 4 });
            let (cells, bytes) = maps.last().unwrap();
            assert_eq!(&row.source()[bytes.clone()], " trailing title");
            assert_eq!(cells.end - cells.start, 1);
            assert_eq!(
                line.to_string()
                    .find('…')
                    .map(|byte| line.to_string()[..byte].width()),
                Some(cells.start as usize)
            );
        }
    }
}
