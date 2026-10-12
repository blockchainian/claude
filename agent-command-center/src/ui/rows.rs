use super::formatting::format_directory_display;
use super::formatting::format_relative_time;
use super::navigation::CenterRow;
use super::render::line;
use super::render::row;
use super::style::selection_style;
use super::*;

fn columns(area: Rect) -> (Rect, Rect, Rect) {
    let metadata = if area.width >= 56 { 24 } else { 0 };
    let gutter = area.width.min(/*other*/ 4);
    let title = Rect::new(
        area.x + gutter,
        area.y,
        area.width - gutter - metadata,
        area.height,
    );
    let updated = Rect::new(
        area.right() - metadata.min(/*other*/ 9),
        area.y,
        metadata.min(/*other*/ 9),
        area.height,
    );
    let status = Rect::new(
        title.right() + metadata.min(/*other*/ 2),
        area.y,
        if metadata == 24 { 11 } else { 0 },
        area.height,
    );
    (title, status, updated)
}

impl View {
    pub(super) fn render_center_rows(&self, area: Rect, buf: &mut Buffer) {
        let row_width = area.width;
        let area = Rect {
            width: row_width.saturating_sub(/*rhs*/ 1),
            ..area
        };
        if area.is_empty() {
            return;
        }
        let indices = self.selectable_indices();
        let mut state = self.viewport.borrow_mut();
        if indices.is_empty() {
            line(
                if self.loading && self.rows.is_empty() {
                    "Loading tasks…"
                } else if self.rows.is_empty() {
                    "No tasks yet"
                } else {
                    "No matching tasks"
                }
                .dim(),
                area,
                buf,
            );
            return;
        }
        let entries = self.center_rows(&indices, self.grouping);
        let selected = entries
            .iter()
            .position(|row| match row {
                CenterRow::Task(index) => *index == self.selected,
                CenterRow::ShowMore => self.selected == usize::MAX,
                _ => false,
            })
            .unwrap_or_default();
        let padding = u16::from(area.height >= 3);
        let viewport = row(area, padding, area.height - padding * 2);
        if padding > 0 {
            let (title, status, updated) = columns(row(area, /*offset*/ 0, /*height*/ 1));
            line("Tasks".dim(), title, buf);
            line("Status".dim(), status, buf);
            Line::from("Updated".dim())
                .right_aligned()
                .render(updated, buf);
        }
        let reference = chrono::Utc::now();
        let height = usize::from(viewport.height);
        state.page_height = height;
        let mut start = state.scroll.min(entries.len().saturating_sub(height));
        if selected < start {
            start = if height > 1
                && selected > 0
                && matches!(entries[selected - 1], CenterRow::Group(_))
            {
                selected - 1
            } else {
                selected
            };
        }
        if selected >= start + height {
            start = selected.saturating_add(/*rhs*/ 1).saturating_sub(height);
        }
        state.scroll = start;
        for (offset, entry) in entries.iter().skip(start).take(height).enumerate() {
            let index = match entry {
                CenterRow::Group(index) | CenterRow::Task(index) => *index,
                CenterRow::Gap => continue,
                CenterRow::ShowMore => {
                    let rect = row(viewport, offset as u16, /*height*/ 1);
                    let selected = self.selected == usize::MAX;
                    let style = if selected {
                        selection_style()
                    } else {
                        Style::default()
                    };
                    buf.set_style(
                        Rect {
                            width: row_width,
                            ..rect
                        },
                        style,
                    );
                    let label = if self.loading {
                        "Loading more…"
                    } else {
                        "Show more"
                    };
                    let marker = if selected { "›" } else { " " };
                    line(
                        Line::from(format!("{marker}   {label}")).style(style),
                        rect,
                        buf,
                    );
                    continue;
                }
            };
            let task = &self.rows[index];
            let rect = row(viewport, offset as u16, /*height*/ 1);
            if matches!(entry, CenterRow::Group(_)) {
                let group = if self.is_pinned(index) {
                    "Pinned".to_owned()
                } else {
                    match self.grouping {
                        Grouping::Project => format_directory_display(&task.cwd),
                        Grouping::Status => task.status.label().to_owned(),
                        Grouping::Model => task.model_name().to_owned(),
                    }
                };
                let count = indices
                    .iter()
                    .filter(|&&candidate| {
                        candidate != usize::MAX && self.same_group(self.grouping, candidate, index)
                    })
                    .count();
                let total = (0..self.rows.len())
                    .filter(|&candidate| self.same_group(self.grouping, candidate, index))
                    .count();
                let count = if count == total {
                    count.to_string()
                } else {
                    format!("{count} of {total}")
                };
                let group = if self.grouping == Grouping::Project && !self.is_pinned(index) {
                    super::line_truncation::center_truncate_path(
                        &group,
                        usize::from(rect.width)
                            .saturating_sub(count.width() + 2)
                            .min(/*other*/ 64),
                    )
                } else {
                    group
                };
                line(format!("{group}  {count}").dim(), rect, buf);
                continue;
            }
            let style = if index == self.selected {
                selection_style()
            } else {
                Style::default()
            };
            buf.set_style(
                Rect {
                    width: row_width,
                    ..rect
                },
                style,
            );
            let (status, mut dot) = Self::status(task);
            if index == self.selected {
                dot.style = style;
            }
            line(
                Line::from(if index == self.selected { "›" } else { " " }).style(style),
                rect,
                buf,
            );
            line(
                Line::from(dot),
                Rect::new(
                    rect.x + rect.width.min(/*other*/ 2),
                    rect.y,
                    rect.width.saturating_sub(/*rhs*/ 2).min(/*other*/ 1),
                    /*height*/ 1,
                ),
                buf,
            );
            let (title, status_area, updated) = columns(rect);
            let title_style = if index == self.selected {
                style
            } else {
                self.title_style(&task.id)
            };
            line(
                Line::from(task.display_title().to_owned()).style(title_style),
                title,
                buf,
            );
            line(Line::from(status).style(style), status_area, buf);
            if !updated.is_empty() {
                let age = format_relative_time(reference, Some(task.updated_at));
                Line::from(age)
                    .style(if index == self.selected {
                        style
                    } else {
                        style.dim()
                    })
                    .right_aligned()
                    .render(updated, buf);
            }
        }
        if padding > 0 && start > 0 {
            line("↑".dim(), row(area, /*offset*/ 0, /*height*/ 1), buf);
        }
        if padding > 0 && start + height < entries.len() {
            line(
                "↓".dim(),
                row(
                    area,
                    area.height.saturating_sub(/*rhs*/ 1),
                    /*height*/ 1,
                ),
                buf,
            );
        }
    }
}
