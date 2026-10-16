use super::style::accent_color;
use super::*;
const COLUMN_GAP: usize = 4;

pub(crate) struct Shortcut {
    pub(crate) key: String,
    pub(crate) action: &'static str,
}

pub(crate) struct Group {
    pub(crate) title: &'static str,
    pub(crate) entries: Vec<Shortcut>,
}

impl Group {
    fn lines(&self) -> Vec<Line<'static>> {
        let key_width = self
            .entries
            .iter()
            .map(|entry| Span::raw(&entry.key).width())
            .max()
            .unwrap_or(/*default*/ 0);
        let mut lines = vec![self.title.bold().into()];
        for entry in &self.entries {
            let key = entry.key.clone().fg(accent_color());
            let padding = " ".repeat(key_width.saturating_sub(key.width()) + 2);
            lines.push(vec![key, padding.into(), entry.action.into()].into());
        }
        lines
    }
}

pub(crate) fn group_lines(groups: [Group; 4], width: u16) -> Vec<Line<'static>> {
    let groups = groups.map(|group| group.lines());
    let widths: Vec<usize> = groups
        .iter()
        .map(|group| group.iter().map(Line::width).max().unwrap_or(/*default*/ 0))
        .collect();
    let width = usize::from(width.max(/*other*/ 1));
    let mut result = Vec::new();
    if widths.iter().sum::<usize>() + COLUMN_GAP * (groups.len() - 1) <= width {
        result.extend(columns(&groups, &widths));
    } else if widths[0].max(widths[2]) + COLUMN_GAP + widths[1].max(widths[3]) <= width {
        result.extend(columns(
            &groups[..2],
            &[widths[0].max(widths[2]), widths[1].max(widths[3])],
        ));
        result.push(Line::default());
        result.extend(columns(
            &groups[2..],
            &[widths[0].max(widths[2]), widths[1].max(widths[3])],
        ));
    } else {
        for (index, group) in groups.into_iter().enumerate() {
            if index > 0 {
                result.push(Line::default());
            }
            result.extend(group);
        }
    }
    result
}

fn columns(groups: &[Vec<Line<'static>>], widths: &[usize]) -> Vec<Line<'static>> {
    let height = groups.iter().map(Vec::len).max().unwrap_or(/*default*/ 0);
    (0..height)
        .map(|row| {
            let mut line = Line::default();
            for (column, group) in groups.iter().enumerate() {
                let entry = group.get(row).cloned().unwrap_or_default();
                let padding = widths[column].saturating_sub(entry.width()) + COLUMN_GAP;
                line.extend(entry.spans);
                if column + 1 < groups.len() {
                    line.push_span(" ".repeat(padding));
                }
            }
            line
        })
        .collect()
}
