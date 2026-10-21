use super::formatting::format_directory_display;
use super::formatting::format_relative_time;
use super::navigation::CenterRow;
use super::render::row;
use super::render::{fill_list_row, line};
use super::style::cursor_style;
use super::*;

fn columns(area: Rect, indent: u16) -> (Rect, Rect, Rect) {
    const MIN_TITLE_WIDTH: u16 = 20;
    const STATUS_WIDTH: u16 = 11;
    const UPDATED_WIDTH: u16 = 9;
    const GAP: u16 = 2;

    let available = area.width.saturating_sub(2);
    let status_metadata = GAP + STATUS_WIDTH;
    let all_metadata = status_metadata + GAP + UPDATED_WIDTH;
    let (metadata, status_width, updated_width) = if available >= MIN_TITLE_WIDTH + all_metadata {
        (all_metadata, STATUS_WIDTH, UPDATED_WIDTH)
    } else if available >= MIN_TITLE_WIDTH + status_metadata {
        (status_metadata, STATUS_WIDTH, 0)
    } else {
        (0, 0, 0)
    };
    let title_space = area.width - metadata;
    let gutter = title_space.min(indent.saturating_add(2));
    let title = Rect::new(area.x + gutter, area.y, title_space - gutter, area.height);
    let updated = Rect::new(
        area.right() - updated_width,
        area.y,
        updated_width,
        area.height,
    );
    let status = Rect::new(
        area.right() - metadata + metadata.min(GAP),
        area.y,
        status_width,
        area.height,
    );
    (title, status, updated)
}

pub(super) fn heading_rect(text: &str, area: Rect) -> Rect {
    let width = text.width().min(usize::from(area.width)) as u16;
    Rect::new(area.x + (area.width - width) / 2, area.y, width, 1)
}

impl View {
    pub(super) fn group_heading(&self, index: usize, width: u16) -> String {
        let group = if self.is_pinned(index) {
            "Pinned".to_owned()
        } else {
            match self.grouping {
                Grouping::Project => self.project_groups[self.group_index(index).unwrap()]
                    .directory()
                    .map_or_else(
                        || format_directory_display(Path::new(""), None),
                        |path| {
                            super::line_truncation::abbreviate_path(
                                &path.to_string_lossy(),
                                usize::MAX,
                                None,
                            )
                        },
                    ),
                Grouping::Status => self.rows[self.group_index(index).unwrap()]
                    .status
                    .label()
                    .to_owned(),
                Grouping::Model => self
                    .session_model_text(&self.rows[self.group_index(index).unwrap()])
                    .unwrap_or_else(|| "Unknown".to_owned()),
            }
        };
        if self.grouping == Grouping::Project && !self.is_pinned(index) {
            super::line_truncation::abbreviate_path(&group, usize::from(width).min(64), None)
        } else {
            group
        }
    }

    pub(super) fn render_center_rows(
        &self,
        area: Rect,
        buf: &mut Buffer,
        reference: chrono::DateTime<chrono::Utc>,
    ) {
        let dragging =
            self.selecting_list(text_selection::Panel::Tasks) && self.text_selection_dragging();
        let mut state = self.viewport.borrow_mut();
        state.task_text = text_selection::TextRegion::default();
        state.list_links.clear();
        state.list = area;
        state.list_rows = Rect::default();
        state.fold_toggles.clear();
        state.first_entry = 0;
        let row_width = area.width;
        if area.is_empty() {
            return;
        }
        let indices = self.selectable_indices();
        if indices.is_empty() && (self.status_filter != 0 || self.drafts.is_empty()) {
            return;
        }
        let entries = self.center_rows(&indices, self.grouping);
        let mut has_next_sibling = vec![false; self.rows.len()];
        let mut parents = std::collections::HashSet::new();
        for entry in entries.iter().rev() {
            if let CenterRow::Task(index) | CenterRow::Parent(index) = entry
                && let Some(parent) = self.rows[*index].parent.as_ref()
            {
                has_next_sibling[*index] = !parents.insert(parent);
            }
        }
        let with_subtasks: std::collections::HashSet<_> = self
            .rows
            .iter()
            .filter(|row| !self.hidden.contains(&row.id))
            .filter_map(|row| row.parent.as_ref())
            .collect();
        // Hand out accents top to bottom before drawing.
        for entry in &entries {
            if let CenterRow::Task(index) | CenterRow::Parent(index) = entry {
                self.title_color(&self.rows[*index].id);
            }
        }
        let selected = entries
            .iter()
            .position(|row| match row {
                CenterRow::Task(index) => *index == self.selected,
                CenterRow::ShowMore => self.selected == usize::MAX,
                _ => false,
            })
            .unwrap_or_default();
        let padding = u16::from(area.height >= 3);
        let viewport = row(area, 0, area.height - padding);
        let height = usize::from(viewport.height);
        // Session reordering moves the cursor's row without moving the viewport.
        let reveal = state.task_selection.as_ref() != self.selected_id()
            || self.selected == usize::MAX
            || state.page_height != height;
        state.task_selection = self.selected_id().cloned();
        state.page_height = height;
        let mut start = state.scroll.min(entries.len().saturating_sub(height));
        state.task_scroll_max = entries.len().saturating_sub(height);
        if reveal && !dragging && selected <= start {
            start = if height > 1
                && selected > 0
                && matches!(entries[selected - 1], CenterRow::Group(_))
            {
                selected - 1
            } else {
                selected
            };
        }
        if reveal && !dragging && selected >= start + height {
            start = selected.saturating_add(1).saturating_sub(height);
        }
        state.scroll = start;
        state.first_entry = start;
        state.list_rows = Rect {
            width: row_width,
            ..viewport
        };
        for (offset, entry) in entries.iter().skip(start).take(height).enumerate() {
            let index = match entry {
                CenterRow::Group(index) | CenterRow::Task(index) | CenterRow::Parent(index) => {
                    *index
                }
                CenterRow::Gap => continue,
                CenterRow::ShowMore => {
                    let rect = row(viewport, offset as u16, 1);
                    let selected = self.selected == usize::MAX;
                    let style = if selected {
                        cursor_style()
                    } else {
                        Style::default()
                    };
                    fill_list_row(rect, buf, style);
                    let label = "Show more";
                    if selected {
                        state.marker = Some((rect.x, rect.y));
                    }
                    line(Line::from(format!("  {label}")).style(style), rect, buf);
                    continue;
                }
            };
            let task = &self.rows[index];
            let rect = row(viewport, offset as u16, 1);
            if matches!(entry, CenterRow::Group(_)) {
                let group = self.group_heading(index, rect.width);
                let rect = heading_rect(&group, rect);
                line(group.clone().dim(), rect, buf);
                if self.grouping == Grouping::Project
                    && !self.is_pinned(index)
                    && let Some(directory) =
                        self.project_groups[self.group_index(index).unwrap()].directory()
                {
                    let link =
                        Rect::new(rect.x, rect.y, group.width() as u16, 1).intersection(rect);
                    let links = [(link, directory.to_path_buf())];
                    state.list_links.extend(links);
                }
                continue;
            }
            let header = matches!(entry, CenterRow::Parent(_));
            let selected = !header && index == self.selected;
            let style = if header {
                Style::default().dim()
            } else {
                Style::default()
            };
            fill_list_row(rect, buf, style);
            let (status, mut dot) = Self::status(task);
            if self.collapsed[self.status_filter].contains(&task.id) {
                dot.content = "✚".into();
            }
            if header {
                dot.style = style;
            }
            if selected {
                state.marker = Some((rect.x, rect.y));
            }
            let depth = self.ancestors(index).count();
            let indent = u16::try_from(depth).unwrap_or(u16::MAX).saturating_mul(3);
            if indent > 0 {
                let ancestors: Vec<_> = self.ancestors(index).collect();
                let mut prefix = String::new();
                for ancestor in ancestors.iter().rev().skip(1) {
                    prefix.push_str(if has_next_sibling[*ancestor] {
                        "│  "
                    } else {
                        "   "
                    });
                }
                prefix.push_str(if has_next_sibling[index] {
                    "├─ "
                } else {
                    "└─ "
                });
                line(
                    Line::from(prefix).style(style),
                    Rect::new(rect.x, rect.y, rect.width.min(indent), 1),
                    buf,
                );
            }
            let dot_area = Rect::new(
                rect.x + rect.width.min(indent),
                rect.y,
                rect.width.saturating_sub(indent).min(1),
                1,
            );
            line(Line::from(dot), dot_area, buf);
            if !header && with_subtasks.contains(&task.id) {
                state.fold_toggles.push((dot_area, task.id.clone()));
            }
            let (title, status_area, updated) = columns(rect, indent);
            let title_style = if header {
                style
            } else {
                self.title_style(&task.id)
            };
            line(
                Line::from(task.display_title().to_owned()).style(title_style),
                title,
                buf,
            );
            let status_style = if !header && task.status == SessionStatus::NeedsInput {
                super::style::attention_title_style()
            } else {
                style
            };
            line(Line::from(status).style(status_style), status_area, buf);
            if !updated.is_empty() {
                let age = format_relative_time(reference, Some(task.updated_at));
                Line::from(age)
                    .style(super::style::time_style())
                    .right_aligned()
                    .render(updated, buf);
            }
            if selected {
                fill_list_row(rect, buf, cursor_style());
            }
        }
        let text_area = state.list_rows;
        drop(state);
        self.capture_list_text(text_selection::Panel::Tasks, text_area, start, buf);
        super::links::underline(buf, &self.viewport.borrow().list_links);
    }
}
