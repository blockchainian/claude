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
            .map(|entry| Span::raw(super::key_name::display(&entry.key)).width())
            .max()
            .unwrap_or(0);
        let mut lines = vec![self.title.bold().into()];
        for entry in &self.entries {
            if entry.key.is_empty() {
                lines.push(Line::default());
                continue;
            }
            let key = Span::styled(
                super::key_name::display(&entry.key),
                Style {
                    fg: accent_color(),
                    ..Style::default()
                },
            );
            let padding = " ".repeat(key_width.saturating_sub(key.width()) + 2);
            lines.push(vec![key, padding.into(), entry.action.into()].into());
        }
        lines
    }
}

pub(crate) fn group_lines(groups: [Group; 3], width: u16) -> Vec<Line<'static>> {
    let groups = groups.map(|group| group.lines());
    let widths: Vec<usize> = groups
        .iter()
        .map(|group| group.iter().map(Line::width).max().unwrap_or(0))
        .collect();
    let width = usize::from(width.max(1));
    let mut result = Vec::new();
    if widths.iter().sum::<usize>() + COLUMN_GAP * (groups.len() - 1) <= width {
        result.extend(columns(&groups, &widths));
    } else if widths[0].max(widths[2]) + COLUMN_GAP + widths[1] <= width {
        result.extend(columns(
            &groups[..2],
            &[widths[0].max(widths[2]), widths[1]],
        ));
        result.push(Line::default());
        result.extend(columns(
            &groups[2..],
            &[widths[0].max(widths[2]), widths[1]],
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
    let height = groups.iter().map(Vec::len).max().unwrap_or(0);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_key_column_uses_display_names_and_widths() {
        let group = Group {
            title: "Keys",
            entries: vec![
                Shortcut {
                    key: "tab/⇧tab".into(),
                    action: "Filter",
                },
                Shortcut {
                    key: "esc".into(),
                    action: "Back",
                },
            ],
        };
        let lines = group.lines();
        assert_eq!(lines[1].spans[0].content, "Tab/⇧Tab");
        assert_eq!(lines[2].spans[0].content, "Esc");
        assert_eq!(lines[1].spans[0].width() + lines[1].spans[1].width(), 10);
        assert_eq!(lines[2].spans[0].width() + lines[2].spans[1].width(), 10);
    }
}
