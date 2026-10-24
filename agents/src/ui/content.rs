//! Content semantics shared by detail producers and their layout.

use ratatui::text::{Line, Span};

#[derive(Clone, Default)]
pub(super) enum Kind {
    #[default]
    Prose,
    Preformatted,
    Table(Table),
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
            Kind::Table(table) => {
                let layout = table.layout(self.indent, options);
                return (layout.lines, layout.breaks);
            }
            Kind::Preformatted => {
                breaks.resize(self.lines.len(), None);
                lines = self.lines;
            }
        }
        (lines, breaks)
    }
}

/// Cells remain in source order until the reading width is known.
#[derive(Clone)]
pub(super) struct Table {
    pub rows: Vec<Vec<Line<'static>>>,
    pub style: ratatui::style::Style,
}

pub(super) struct TableLayout {
    pub lines: Vec<Line<'static>>,
    pub breaks: Vec<Option<bool>>,
    /// Display columns mapped to bytes in the concatenated, unformatted cells.
    pub sources: Vec<Vec<super::text_selection_layout::CellMap>>,
}

impl Table {
    pub fn layout(&self, indent: usize, options: textwrap::Options<'_>) -> TableLayout {
        use unicode_segmentation::UnicodeSegmentation;
        use unicode_width::UnicodeWidthStr;
        let mut layout = TableLayout {
            lines: Vec::new(),
            breaks: Vec::new(),
            sources: Vec::new(),
        };
        let available = options.width.saturating_sub(indent);
        if available == 0 {
            return layout;
        }
        let columns = self.rows.iter().map(Vec::len).max().unwrap_or(0);
        if columns == 0 {
            return layout;
        }
        let natural: Vec<_> = (0..columns)
            .map(|column| {
                self.rows
                    .iter()
                    .filter_map(|row| row.get(column))
                    .map(Line::width)
                    .max()
                    .unwrap_or(0)
            })
            .collect();
        let widths = table_widths(&natural, available);
        let rule = widths
            .iter()
            .enumerate()
            .map(|(column, width)| {
                let gaps = usize::from(column > 0) + usize::from(column + 1 < columns);
                "─".repeat(width + gaps)
            })
            .collect::<Vec<_>>()
            .join("┼");
        let mut base = 0;
        let rows: Vec<Vec<_>> = self
            .rows
            .iter()
            .map(|row| {
                (0..columns)
                    .map(|column| {
                        let cell = row.get(column).cloned().unwrap_or_default();
                        let plain = cell.to_string();
                        let start = base;
                        base += plain.len();
                        let (lines, breaks) = wrap(
                            cell,
                            0,
                            textwrap::Options::new(widths[column].max(1)).break_words(true),
                        );
                        (plain, start, lines, breaks)
                    })
                    .collect()
            })
            .collect();
        let wrapped = rows
            .iter()
            .flatten()
            .any(|(_, _, lines, _)| lines.len() > 1);
        for (row_index, row) in rows.iter().enumerate() {
            if row_index == 1 || (wrapped && row_index > 1) {
                layout.lines.push(Line::from(vec![
                    Span::raw(" ".repeat(indent)),
                    Span::styled(rule.clone(), self.style),
                ]));
                layout.breaks.push(Some(false));
                layout.sources.push(Vec::new());
            }
            let height = row
                .iter()
                .map(|(_, _, lines, _)| lines.len())
                .max()
                .unwrap_or(1);
            let mut offsets = vec![0; columns];
            for display_row in 0..height {
                let mut spans = Vec::new();
                if indent > 0 {
                    spans.push(Span::raw(" ".repeat(indent)));
                }
                let mut sources = Vec::new();
                let mut x = indent;
                let mut space = false;
                for (column, (plain, base, lines, breaks)) in row.iter().enumerate() {
                    if column > 0 {
                        spans.push(Span::styled(" │ ", self.style));
                        x += 3;
                    }
                    let line = lines.get(display_row).cloned().unwrap_or_default();
                    let text = line.to_string();
                    let start = offsets[column]
                        + plain[offsets[column]..]
                            .find(&text)
                            .expect("cell wrap belongs to source");
                    offsets[column] = start + text.len();
                    for (byte, grapheme) in text.grapheme_indices(true) {
                        let end = x + grapheme.width();
                        sources.push((
                            x as u16..end as u16,
                            base + start + byte..base + start + byte + grapheme.len(),
                        ));
                        x = end;
                    }
                    space |= breaks.get(display_row) == Some(&Some(true));
                    let padding = widths[column].saturating_sub(line.width());
                    spans.extend(line.spans);
                    if column + 1 < columns && padding > 0 {
                        spans.push(Span::raw(" ".repeat(padding)));
                        x += padding;
                    }
                }
                layout.lines.push(Line::from(spans));
                layout.breaks.push((display_row > 0).then_some(space));
                layout.sources.push(sources);
            }
        }
        // A header-only table still has its underline.
        if rows.len() == 1 {
            layout.lines.push(Line::from(vec![
                Span::raw(" ".repeat(indent)),
                Span::styled(rule, self.style),
            ]));
            layout.breaks.push(Some(false));
            layout.sources.push(Vec::new());
        }
        layout
    }
}

fn table_widths(natural: &[usize], width: usize) -> Vec<usize> {
    let available = width.saturating_sub(3 * natural.len().saturating_sub(1));
    if natural.iter().sum::<usize>() <= available {
        return natural.to_vec();
    }
    let mut widths = vec![0; natural.len()];
    let minimum: Vec<_> = natural.iter().map(|&width| width.min(6)).collect();
    // Below the minimum, share the remaining columns and split words as needed.
    if minimum.iter().sum::<usize>() > available {
        let mut remaining = available;
        while remaining > 0 {
            for (width, &limit) in widths.iter_mut().zip(&minimum) {
                if remaining > 0 && *width < limit {
                    *width += 1;
                    remaining -= 1;
                }
            }
        }
        return widths;
    }
    let share = width / natural.len();
    let mut active: Vec<_> = (0..natural.len()).collect();
    let mut short: Vec<_> = active
        .iter()
        .copied()
        .filter(|&i| natural[i] <= share)
        .collect();
    short.sort_by_key(|&i| natural[i]);
    for i in short {
        let reserved: usize = active
            .iter()
            .filter(|&&other| other != i)
            .map(|&other| minimum[other])
            .sum();
        if widths.iter().sum::<usize>() + natural[i] + reserved <= available {
            widths[i] = natural[i];
            active.retain(|&other| other != i);
        }
    }
    loop {
        let remaining = available - widths.iter().sum::<usize>();
        let total: usize = active.iter().map(|&i| natural[i]).sum();
        let mut fixed = Vec::new();
        for &i in &active {
            let proportional = remaining * natural[i] / total;
            if natural[i] <= proportional {
                widths[i] = natural[i];
                fixed.push(i);
            } else if proportional < minimum[i] {
                widths[i] = minimum[i];
                fixed.push(i);
            }
        }
        if fixed.is_empty() {
            for &i in &active {
                widths[i] = remaining * natural[i] / total;
            }
            let mut leftover = available - widths.iter().sum::<usize>();
            for &i in &active {
                if leftover > 0 && widths[i] < natural[i] {
                    widths[i] += 1;
                    leftover -= 1;
                }
            }
            return widths;
        }
        active.retain(|i| !fixed.contains(i));
        if active.is_empty() {
            return widths;
        }
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
