use super::formatting::format_directory_display;
use super::formatting::format_relative_time;
use super::navigation::CenterRow;
use super::render::row;
use super::render::{fill_list_row, line};
use super::style::cursor_style;
use super::*;

fn columns(area: Rect) -> (Rect, Rect, Rect) {
    const MIN_TITLE_WIDTH: u16 = 20;
    const STATUS_WIDTH: u16 = 11;
    const UPDATED_WIDTH: u16 = 9;
    const GAP: u16 = 2;

    let gutter = area.width.min(2);
    let available = area.width - gutter;
    let status_metadata = GAP + STATUS_WIDTH;
    let all_metadata = status_metadata + GAP + UPDATED_WIDTH;
    let (metadata, status_width, updated_width) = if available >= MIN_TITLE_WIDTH + all_metadata {
        (all_metadata, STATUS_WIDTH, UPDATED_WIDTH)
    } else if available >= MIN_TITLE_WIDTH + status_metadata {
        (status_metadata, STATUS_WIDTH, 0)
    } else {
        (0, 0, 0)
    };
    let title = Rect::new(area.x + gutter, area.y, available - metadata, area.height);
    let updated = Rect::new(
        area.right() - updated_width,
        area.y,
        updated_width,
        area.height,
    );
    let status = Rect::new(
        title.right() + metadata.min(GAP),
        area.y,
        status_width,
        area.height,
    );
    (title, status, updated)
}

impl View {
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
        // Hand out accents top to bottom before drawing.
        for entry in &entries {
            if let CenterRow::Task(index) | CenterRow::Parent(index) = entry {
                self.title_color(&self.rows[*index].id);
            }
        }
        let selected = entries
            .iter()
            .position(|row| match row {
                CenterRow::Task(index) => self.selected_draft.is_none() && *index == self.selected,
                CenterRow::Draft(id) => self.selected_draft == Some(*id),
                CenterRow::ShowMore => self.selected == usize::MAX,
                _ => false,
            })
            .unwrap_or_default();
        let padding = u16::from(area.height >= 3);
        let viewport = row(area, 0, area.height - padding);
        let height = usize::from(viewport.height);
        state.page_height = height;
        let mut start = state.scroll.min(entries.len().saturating_sub(height));
        state.task_scroll_max = entries.len().saturating_sub(height);
        if !dragging && selected <= start {
            start = if height > 1
                && selected > 0
                && matches!(entries[selected - 1], CenterRow::Group(_))
            {
                selected - 1
            } else {
                selected
            };
        }
        if !dragging && selected >= start + height {
            start = selected.saturating_add(/*rhs*/ 1).saturating_sub(height);
        }
        state.scroll = start;
        state.first_entry = start;
        state.list_rows = Rect {
            width: row_width,
            ..viewport
        };
        for (offset, entry) in entries.iter().skip(start).take(height).enumerate() {
            if let CenterRow::Draft(id) | CenterRow::DraftGroup(id) = entry {
                let draft = self
                    .drafts
                    .iter()
                    .find(|draft| draft.id == *id)
                    .expect("visible draft");
                let rect = row(viewport, offset as u16, 1);
                if matches!(entry, CenterRow::DraftGroup(_)) {
                    line(format!("{}", draft.heading().display()).dim(), rect, buf);
                } else {
                    let selected = self.selected_draft == Some(*id);
                    if selected {
                        fill_list_row(rect, buf, cursor_style());
                        state.marker = Some((rect.x, rect.y));
                    }
                    let text = "  New task";
                    line(
                        Line::from(text).style(if selected {
                            cursor_style()
                        } else {
                            Style::default()
                        }),
                        rect,
                        buf,
                    );
                }
                continue;
            }
            let index = match entry {
                CenterRow::Group(index) | CenterRow::Task(index) | CenterRow::Parent(index) => {
                    *index
                }
                CenterRow::Gap | CenterRow::Draft(_) | CenterRow::DraftGroup(_) => continue,
                CenterRow::ShowMore => {
                    let rect = row(viewport, offset as u16, /*height*/ 1);
                    let selected = self.selected == usize::MAX;
                    let style = if selected {
                        cursor_style()
                    } else {
                        Style::default()
                    };
                    fill_list_row(rect, buf, style);
                    let label = if self.loading {
                        "Loading more…"
                    } else {
                        "Show more"
                    };
                    if selected {
                        state.marker = Some((rect.x, rect.y));
                    }
                    line(
                        Line::from(format!("  {label}")).style(style),
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
                        Grouping::Project => self.project_groups[self.group_index(index).unwrap()]
                            .directory()
                            .map_or_else(|| "No directory".to_owned(), format_directory_display),
                        Grouping::Status => task.status.label().to_owned(),
                        Grouping::Model => self.rows[self.group_index(index).unwrap()]
                            .model_name()
                            .to_owned(),
                    }
                };
                let group = if self.grouping == Grouping::Project && !self.is_pinned(index) {
                    super::line_truncation::abbreviate_path(
                        &group,
                        usize::from(rect.width).min(/*other*/ 64),
                    )
                } else {
                    group
                };
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
            let selected = !header && self.selected_draft.is_none() && index == self.selected;
            let style = if header {
                Style::default().dim()
            } else {
                Style::default()
            };
            fill_list_row(rect, buf, style);
            let (status, mut dot) = Self::status(task);
            if header {
                dot.style = style;
            }
            if selected {
                state.marker = Some((rect.x, rect.y));
            }
            let indent = if task.parent.is_some() { 2 } else { 0 };
            if indent > 0 {
                line(
                    Line::from("└ ").style(style),
                    Rect::new(
                        rect.x,
                        rect.y,
                        rect.width.min(2),
                        1,
                    ),
                    buf,
                );
            }
            line(
                Line::from(dot),
                Rect::new(
                    rect.x + rect.width.min(indent),
                    rect.y,
                    rect.width.saturating_sub(indent).min(/*other*/ 1),
                    /*height*/ 1,
                ),
                buf,
            );
            let (mut title, status_area, updated) = columns(rect);
            let indent = title.width.min(indent);
            title.x += indent;
            title.width -= indent;
            let title_style = if header {
                style
            } else {
                self.title_style(&task.id)
            };
            line(
                Line::from(if task.background_job.is_some() {
                    format!("bg {}", task.display_title())
                } else {
                    task.display_title().to_owned()
                })
                .style(title_style),
                title,
                buf,
            );
            line(Line::from(status).style(style), status_area, buf);
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
        if padding > 0 && start > 0 {
            Line::from("↑".dim())
                .right_aligned()
                .render(row(area, area.height - 1, 1), buf);
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
        let text_area = state.list_rows;
        drop(state);
        self.capture_list_text(text_selection::Panel::Tasks, text_area, start, buf);
    }
}
