use super::formatting::{format_duration, format_relative_time, tail_lines};
use super::line_truncation::{truncate_line_to_width, truncate_line_with_ellipsis_if_overflow};
use super::*;
use ratatui::widgets::{Block, Borders};

pub(super) fn row(area: Rect, offset: u16, height: u16) -> Rect {
    let offset = offset.min(area.height);
    Rect::new(
        area.x,
        area.y + offset,
        area.width,
        height.min(area.height - offset),
    )
}
pub(super) fn line(text: impl Into<Line<'static>>, area: Rect, buf: &mut Buffer) {
    if !area.is_empty() {
        truncate_line_with_ellipsis_if_overflow(text.into(), usize::from(area.width))
            .render(row(area, 0, 1), buf);
    }
}
fn input_row(prompt: &str, edit: &line_edit::LineEdit, area: Rect) -> (Line<'static>, u16) {
    let prompt_width = prompt.width();
    let (text, caret) = edit.window(usize::from(area.width).saturating_sub(prompt_width));
    (
        Line::from(vec![
            Span::styled(
                prompt.to_owned(),
                Style {
                    fg: style::accent_color(),
                    ..Style::default().bold()
                },
            ),
            text.to_owned().into(),
        ]),
        (prompt_width + usize::from(caret)).min(usize::from(area.width.saturating_sub(1))) as u16,
    )
}

pub(super) fn render_input_row(
    prompt: &str,
    edit: &line_edit::LineEdit,
    area: Rect,
    buf: &mut Buffer,
) {
    let (text, _) = input_row(prompt, edit, area);
    line(text, area, buf);
}

pub(super) const HORIZONTAL_PADDING: u16 = 1;
const PANEL_GAP: u16 = 2 * HORIZONTAL_PADDING + 1;

/// Fills a list row through both padding cells, stopping before column separators.
pub(super) fn fill_list_row(area: Rect, buf: &mut Buffer, style: Style) {
    if area.is_empty() {
        return;
    }
    let left = area.x.saturating_sub(HORIZONTAL_PADDING);
    let right = area.right().saturating_add(HORIZONTAL_PADDING);
    let padded = Rect::new(left, area.y, right - left, area.height).intersection(buf.area);
    buf.set_style(padded, style);
}

pub(super) const WIDE_LAYOUT_COLUMNS: u16 = 150;

#[derive(Clone, Copy)]
pub(super) struct CenterLayout {
    pub(super) header: Rect,
    /// The panels above the prompt, where global dialogs center.
    pub(super) top: Rect,
    pub(super) list: Rect,
    pub(super) gap: Rect,
    pub(super) task_details: Rect,
    pub(super) search: Rect,
    pub(super) usage_chart: Rect,
    pub(super) footer: Rect,
    pub(super) event_panel: Rect,
    pub(super) prompt: Rect,
    pub(super) event_detail: Rect,
    pub(super) prompt_top_rule: Rect,
    pub(super) prompt_bottom_rule: Rect,
    pub(super) event_detail_top_rule: Rect,
    pub(super) footer_rule: Rect,
}

impl View {
    fn input(&self) -> (String, &line_edit::LineEdit) {
        (format!("Search {PROMPT_MARKER} "), &self.search)
    }

    pub(super) fn center_layout(&self, area: Rect) -> CenterLayout {
        self.panel_layout(area, true)
    }

    pub(super) fn panel_layout(&self, area: Rect, zoom: bool) -> CenterLayout {
        let footer_height = u16::from(area.height >= 2);
        let header_height = if area.height >= 6 { 2 } else { 0 };
        let footer = row(area, area.height - footer_height, footer_height);
        let header = row(area, 0, header_height);
        let wide = area.width >= WIDE_LAYOUT_COLUMNS;
        // The prompt or question grows with its content up to a share of the rows between
        // the header and the footer: half in narrow mode, where the event list stacks below
        // it, and two thirds in wide mode, where the event list sits beside the panels.
        let content_rows = area
            .height
            .saturating_sub(header_height + footer_height + 2)
            .max(2);
        let prompt_cap = if wide {
            content_rows * 2 / 3
        } else {
            content_rows / 2
        };
        let prompt_height = self
            .active_question()
            .map_or_else(
                || {
                    self.input_box.height(
                        area.width.saturating_sub(2 * HORIZONTAL_PADDING),
                        &self.input_label(),
                    )
                },
                |question| question.height(area.width.saturating_sub(2 * HORIZONTAL_PADDING)),
            )
            .min(prompt_cap);
        let bottom_height = ((u32::from(area.height) * 382 + 500) / 1000) as u16;
        let event_height = bottom_height.saturating_sub(footer_height).max(10);
        let show_event =
            area.height.saturating_sub(footer_height + event_height) >= header_height + 6;
        // Exclude both rules around the prompt and, in narrow mode, the list/detail rule.
        // The output keeps its top rule inside this share and leaves one row for its bottom rule.
        let content_height = area
            .height
            .saturating_sub(header_height + footer_height + prompt_height + 2 + u16::from(!wide));
        let (mut top_height, mut list_height, mut event_detail_height) = if !show_event {
            (
                area.height
                    .saturating_sub(header_height + footer_height + prompt_height + 2),
                0,
                0,
            )
        } else if wide {
            // Round the upper share down and give the remaining content rows below.
            let top = (u32::from(content_height) * 618 / 1000) as u16;
            (top, 0, content_height - top)
        } else {
            (
                content_height.div_ceil(3),
                (content_height + 1) / 3,
                content_height / 3,
            )
        };
        if zoom && let Some(panel) = self.height_zoom {
            let height = top_height + list_height + event_detail_height;
            // Adjacent frame rules coincide when the panel between them disappears.
            // The detail's first row already counts its top rule in `height`.
            match panel {
                Panel::EventDetail if show_event => {
                    (top_height, list_height, event_detail_height) =
                        (0, 0, height + u16::from(!wide));
                }
                Panel::EventList if show_event && !wide => {
                    (top_height, list_height, event_detail_height) = (0, height, 0);
                }
                Panel::TaskList | Panel::TaskDetails | Panel::EventList => {
                    (top_height, list_height, event_detail_height) =
                        (height + u16::from(show_event && !wide), 0, 0);
                }
                _ => {}
            }
        }
        // With no top panels, the header already supplies the prompt's upper rule.
        // Give the reclaimed row to the remaining panel below the prompt.
        let prompt_top_gap = u16::from(top_height > 0);
        if show_event && top_height == 0 {
            if event_detail_height > 0 {
                event_detail_height += 1;
            } else if list_height > 0 {
                list_height += 1;
            }
        }
        let body = row(area, header_height, top_height).inner(Margin::new(HORIZONTAL_PADDING, 0));
        let top = body;
        let prompt = row(
            area,
            header_height + top_height + prompt_top_gap,
            prompt_height,
        );
        let mut event_panel = if show_event && !wide {
            row(area, prompt.bottom() - area.y + 1, list_height)
                .inner(Margin::new(HORIZONTAL_PADDING, 0))
        } else {
            Rect::default()
        };
        let event_detail = if show_event {
            row(
                area,
                prompt.bottom() - area.y + u16::from(!wide && list_height > 0) + list_height,
                event_detail_height,
            )
        } else {
            Rect::default()
        };
        let frame_rule = |y| row(area, y, 1);
        let prompt_top_rule = frame_rule(prompt.y.saturating_sub(area.y + 1));
        let prompt_bottom_rule = frame_rule(prompt.bottom() - area.y);
        let event_detail_top_rule = if show_event {
            frame_rule(event_detail.y.saturating_sub(area.y))
        } else {
            prompt_bottom_rule
        };
        let footer_rule = frame_rule(footer.y.saturating_sub(area.y + 1));
        let (body, mut event_list) = if wide && show_event {
            let column_width = (body.width - 2 * PANEL_GAP) / 4;
            let event_width = body.width - 2 * PANEL_GAP - 2 * column_width;
            (
                Rect {
                    width: body.width - event_width - PANEL_GAP,
                    ..body
                },
                Rect::new(body.right() - event_width, body.y, event_width, body.height),
            )
        } else {
            (body, Rect::default())
        };
        let [mut list, mut gap, mut task_details] = if body.width >= 2 * PANEL_GAP + 1 {
            let content = Rect {
                width: body.width - PANEL_GAP,
                ..body
            };
            let [list, mut task_details] =
                Layout::horizontal([Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)])
                    .areas(content);
            let gap = Rect::new(list.right(), body.y, PANEL_GAP, body.height);
            task_details.x += gap.width;
            [list, gap, task_details]
        } else {
            [body, Rect::default(), Rect::default()]
        };
        if zoom && let Some(panel) = self.width_zoom {
            match panel {
                Panel::TaskList | Panel::TaskDetails => {
                    list = if panel == Panel::TaskList {
                        top
                    } else {
                        Rect::default()
                    };
                    task_details = if panel == Panel::TaskDetails {
                        top
                    } else {
                        Rect::default()
                    };
                    gap = Rect::default();
                    event_list = Rect::default();
                }
                Panel::EventList if wide && show_event => {
                    event_list = top;
                    list = Rect::default();
                    gap = Rect::default();
                    task_details = Rect::default();
                }
                _ => {}
            }
        }
        // Empty rows retain no usable hit targets.
        if event_panel.is_empty() {
            event_panel = Rect::default();
        }
        // Keep the chart's original canvas and scale when the input row takes list space.
        let usage_chart = list;
        let (list, search) = if self.searching {
            let input = event_list::search_input_rect(list);
            (
                row(list, 0, list.height.saturating_sub(input.height)),
                input,
            )
        } else {
            (list, Rect::default())
        };
        CenterLayout {
            header,
            top,
            list,
            gap,
            task_details,
            search,
            usage_chart,
            footer,
            event_panel: if wide && show_event {
                event_list
            } else {
                event_panel
            },
            prompt,
            event_detail,
            prompt_top_rule,
            prompt_bottom_rule,
            event_detail_top_rule,
            footer_rule,
        }
    }
    pub(crate) fn event_count(&self, area: Rect) -> usize {
        usize::from(
            self.center_layout(area)
                .event_panel
                .height
                .saturating_sub(u16::from(self.event_list.search.active())),
        )
    }

    pub fn cursor_pos(&self, area: Rect) -> Option<(u16, u16)> {
        if !self.help
            && self.confirm_dialog.is_none()
            && let Some(cursor) = self.terminal_cursor()
        {
            return Some(cursor);
        }
        if !self.help
            && self.confirm_dialog.is_none()
            && self.active_panel == Panel::TaskDetails
            && let Some(editor) = self.title_editor()
        {
            let input = self.viewport.borrow().title_input;
            if input.is_empty() {
                return None;
            }
            let (_, caret) = editor.window(usize::from(input.width));
            return Some((input.x + caret, input.y));
        }
        let layout = self.center_layout(area);
        if !self.help
            && self.confirm_dialog.is_none()
            && self.active_panel == Panel::TaskDetails
            && let Some(selector) = &self.selector
            && let Some(editor) = selector.branch_editor()
        {
            let input = self.viewport.borrow().task_details_input;
            if !input.is_empty() {
                let (_, caret) = input_row(&format!("New branch {PROMPT_MARKER} "), editor, input);
                return Some((input.x + caret, input.y));
            }
        }
        if !self.help
            && self.confirm_dialog.is_none()
            && self.active_panel == Panel::Prompt
            && let Some(question) = self.active_question()
        {
            return question.cursor(layout.prompt.inner(Margin::new(HORIZONTAL_PADDING, 0)));
        }
        if self.prompt_focused()
            && self.input_disabled().is_none()
            && !self.help
            && self.active_question().is_none()
        {
            return self.input_box.cursor(
                layout.prompt.inner(Margin::new(HORIZONTAL_PADDING, 0)),
                &self.input_label(),
            );
        }
        if self.task_search_editing()
            && !self.help
            && self.confirm_dialog.is_none()
            && !layout.event_panel.is_empty()
        {
            let search_area =
                if self.active_panel == Panel::EventDetail && !layout.event_detail.is_empty() {
                    layout
                        .event_detail
                        .inner(Margin::new(HORIZONTAL_PADDING, 0))
                } else {
                    layout.event_panel
                };
            let input = event_list::search_input_rect(search_area);
            let search = if self.active_panel == Panel::EventDetail {
                &self.event_detail.search
            } else {
                &self.event_list.search
            };
            let (_, caret) = input_row(&format!("Search {PROMPT_MARKER} "), &search.query, input);
            return Some((input.x + caret, input.y));
        }
        if !self.searching || self.confirm_dialog.is_some() || self.help || layout.search.is_empty()
        {
            return None;
        }
        let (prompt, edit) = self.input();
        let (_, caret) = input_row(&prompt, edit, layout.search);
        Some((layout.search.x + caret, layout.search.y))
    }

    #[cfg(test)]
    pub fn render(&self, area: Rect, buf: &mut Buffer) {
        self.render_at(area, buf, chrono::Utc::now());
    }
    /// Draws the footer: the hints, the scrolled-off section `title` beside the task hints, and
    /// the latest feedback, or else the failure of the selected draft, question or prompt.
    fn render_footer(
        &self,
        area: Rect,
        wide: bool,
        title: Option<(Line<'static>, super::event_detail::EventDetailColumns)>,
        buf: &mut Buffer,
    ) {
        super::footer::Footer {
            hints: self.center_footer_hints(),
            title: title.filter(|_| self.footer_shows_task_hints()),
            feedback: self
                .feedback
                .as_ref()
                .map(|(feedback, _)| feedback.line())
                .or_else(|| self.selection_failure().map(|failure| failure.line())),
        }
        .render(area, wide, buf);
    }
    pub(super) fn clear_overlay(&self, area: Rect, modal: bool, buf: &mut Buffer) {
        use ratatui::buffer::CellWidth;

        self.viewport
            .borrow_mut()
            .overlays
            .push(Overlay { area, modal });
        let area = area.intersection(buf.area);
        if area.is_empty() {
            return;
        }
        // Clear the outside half of any wide character split by an overlay edge.
        for y in area.y..area.bottom() {
            if area.x > buf.area.x && buf[(area.x - 1, y)].cell_width() > 1 {
                buf[(area.x - 1, y)].reset();
            }
            if area.right() < buf.area.right() && buf[(area.right() - 1, y)].cell_width() > 1 {
                buf[(area.right(), y)].reset();
            }
        }
        Clear.render(area, buf);
    }

    pub(crate) fn render_at(
        &self,
        area: Rect,
        buf: &mut Buffer,
        now: chrono::DateTime<chrono::Utc>,
    ) {
        Clear.render(area, buf);
        let navigation_layout = self.panel_layout(area, false);
        {
            // A panel this frame leaves out keeps no links from the last one.
            let mut viewport = self.viewport.borrow_mut();
            viewport.area = area;
            viewport.navigation_layout = Some(navigation_layout);
            viewport.overlays.clear();
            viewport.picker_rows.clear();
            viewport.confirm_options.clear();
            viewport.list_links.clear();
            viewport.marker = None;
            viewport.field_targets.clear();
            viewport.task_details = Rect::default();
            viewport.task_details_text = text_selection::TextRegion::default();
            viewport.task_details_title = Rect::default();
            viewport.title_input = Rect::default();
            viewport.task_details_input = Rect::default();
            viewport.event_panel = Rect::default();
            viewport.event_borders = [Rect::default(); 2];
            viewport.output_borders = [Rect::default(); 2];
            viewport.event_detail = Rect::default();
            viewport.terminal = Rect::default();
            viewport.task_details_links.clear();
            viewport.event_links.clear();
        }
        let layout = self.center_layout(area);
        self.viewport.borrow_mut().prompt = layout.prompt;
        if let Some(question) = self.active_question() {
            question.render(layout.prompt.inner(Margin::new(HORIZONTAL_PADDING, 0)), buf);
        } else {
            self.input_box.render(
                layout.prompt.inner(Margin::new(HORIZONTAL_PADDING, 0)),
                buf,
                &self.input_label(),
                self.selected_id()
                    .map_or_else(Style::default, |id| self.title_style(id)),
                self.input_disabled(),
                self.prompt_focused(),
            );
        }
        let inset = |area: Rect| area.inner(Margin::new(HORIZONTAL_PADDING, 0));
        let labels = TASK_FILTERS
            .iter()
            .enumerate()
            .map(|(tab, (label, _))| {
                let count = self.visible_indices_for_filter(tab).len();
                format!("{label} {count}")
            })
            .collect::<Vec<_>>();
        let tabs = inset(row(layout.header, 0, 1));
        self.viewport.borrow_mut().tab_bar = tabs;
        self.viewport.borrow_mut().tabs = super::selection_tabs::render_filled_tab_bar(
            &labels.iter().map(String::as_str).collect::<Vec<_>>(),
            self.status_filter,
            tabs,
            buf,
        );
        let weekly_target = super::weekly_usage::render(
            row(layout.header, 0, 1),
            &self.viewport.borrow().tabs,
            [
                self.weekly_usage.get(&ProviderId::Claude),
                self.weekly_usage.get(&ProviderId::Codex),
            ],
            self.usage_view,
            self.use_colors,
            buf,
        );
        if let Some(target) = weekly_target {
            self.viewport
                .borrow_mut()
                .field_targets
                .push((target, Field::WeeklyUsage));
        }
        rule(row(layout.header, 1, 1), buf);

        if self.help {
            let body = inset(row(
                area,
                layout.header.height,
                layout.footer.y.saturating_sub(layout.header.bottom()),
            ));
            self.clear_overlay(body, true, buf);
            let mut lines = self.center_help_lines(body.width);
            if lines.len() > usize::from(body.height) {
                lines.truncate(usize::from(body.height.saturating_sub(1)));
                lines.push("… resize to see all".dim().into());
            }
            Paragraph::new(lines).render(body, buf);
            self.render_footer(
                inset(layout.footer),
                area.width >= WIDE_LAYOUT_COLUMNS,
                None,
                buf,
            );
            return;
        }
        for y in layout.gap.y..layout.gap.bottom() {
            if layout.gap.width > 1 {
                buf[(layout.gap.x + HORIZONTAL_PADDING, y)]
                    .set_symbol("│")
                    .set_style(Style::default().dim());
            }
        }
        let event_separator = (area.width >= WIDE_LAYOUT_COLUMNS
            && !layout.event_panel.is_empty()
            && layout.event_panel.x > layout.top.x)
            .then(|| layout.event_panel.x.saturating_sub(HORIZONTAL_PADDING + 1));
        if let Some(x) = event_separator {
            for y in layout.event_panel.y..layout.event_panel.bottom() {
                buf[(x, y)]
                    .set_symbol("│")
                    .set_style(Style::default().dim());
            }
        }
        self.render_center_rows(layout.list, buf, now);
        self.render_usage_chart(layout.usage_chart, buf, now);
        if self.searching {
            let (prompt, edit) = self.input();
            Clear.render(layout.search, buf);
            render_input_row(&prompt, edit, layout.search, buf);
        }
        // Activity and task status share the rule immediately above the prompt.
        for border in [
            layout.prompt_top_rule,
            layout.prompt_bottom_rule,
            layout.event_detail_top_rule,
            layout.footer_rule,
        ] {
            rule(border, buf);
        }
        let task_details = layout.task_details;
        if !task_details.is_empty() {
            self.render_task_details(task_details, buf, now);
        }
        {
            let mut viewport = self.viewport.borrow_mut();
            viewport.event_panel = layout.event_panel;
            viewport.search = layout.search;
            viewport.event_detail = inset(layout.event_detail);
        }
        self.render_event(layout.event_panel, buf, now);
        if !layout.event_detail.is_empty() {
            self.viewport.borrow_mut().output_borders =
                [layout.event_detail_top_rule, layout.footer_rule];
            self.render_event_detail(
                super::event_detail::EventDetailArea {
                    panel: layout.event_detail,
                    content: inset(layout.event_detail),
                },
                buf,
                now,
            );
        }
        let footer_title = self.viewport.borrow_mut().event_detail_footer_title.take();
        self.render_footer(
            inset(layout.footer),
            area.width >= WIDE_LAYOUT_COLUMNS,
            footer_title,
            buf,
        );
        self.render_frame_labels(navigation_layout, layout, area, buf, now);
        let mut separators = vec![];
        if !layout.gap.is_empty() && layout.gap.width > 1 {
            separators.push((
                layout.gap.x + HORIZONTAL_PADDING,
                layout.gap.y,
                layout.gap.bottom(),
            ));
        }
        if let Some(x) = event_separator {
            separators.push((x, layout.event_panel.y, layout.event_panel.bottom()));
        }
        join_separators(&separators, area, buf);
        self.render_controls(task_details, buf);
        if let Some(dialog) = &self.confirm_dialog {
            self.render_confirm_dialog(dialog, layout.top, buf);
            self.viewport.borrow_mut().marker = None;
        }
    }
    /// Labels keep their unzoomed columns while their frame rules move or coincide.
    fn render_frame_labels(
        &self,
        baseline: CenterLayout,
        layout: CenterLayout,
        area: Rect,
        buf: &mut Buffer,
        now: DateTime<Utc>,
    ) {
        let wide = area.width >= WIDE_LAYOUT_COLUMNS;
        let at = |rect: Rect, y| Rect::new(rect.x, y, rect.width, 1).intersection(area);
        let left = baseline.gap.x + HORIZONTAL_PADDING;
        let right = if wide && !baseline.event_panel.is_empty() {
            baseline.event_panel.x - HORIZONTAL_PADDING
        } else {
            area.right()
        };
        let title = Rect::new(
            left,
            layout.prompt_top_rule.y,
            right.saturating_sub(left),
            1,
        )
        .intersection(area);
        let status = at(title, layout.header.y + 1).intersection(layout.header);
        let status = if status.is_empty() {
            Rect::default()
        } else {
            status
        };
        // Save the status labels separately so an input-side label takes precedence when
        // rules coincide, without clearing unrelated status labels on the same rule.
        let mut status_base = Buffer::empty(status);
        rule(status, &mut status_base);
        let mut status_labels = status_base.clone();
        if !status.is_empty() {
            self.render_status_rule(status, &mut status_labels, now);
        }
        let before = (status.x..status.right())
            .map(|x| buf[(x, status.y)].clone())
            .collect::<Vec<_>>();

        if !baseline.event_panel.is_empty() {
            let borders = [
                at(
                    baseline.event_panel,
                    if wide {
                        layout.header.bottom().saturating_sub(1)
                    } else {
                        layout.prompt_bottom_rule.y
                    },
                ),
                at(
                    baseline.event_panel,
                    if wide {
                        layout.prompt_top_rule.y
                    } else {
                        layout.event_detail_top_rule.y
                    },
                ),
            ];
            if !layout.event_panel.is_empty() {
                self.viewport.borrow_mut().event_borders = [
                    Rect {
                        x: layout.event_panel.x,
                        width: layout.event_panel.width,
                        ..borders[0]
                    },
                    Rect {
                        x: layout.event_panel.x,
                        width: layout.event_panel.width,
                        ..borders[1]
                    },
                ];
            }
            let model_y = if wide {
                layout.prompt_top_rule.y
            } else {
                layout.prompt_bottom_rule.y
            };
            let event_left = if wide {
                baseline
                    .event_panel
                    .x
                    .saturating_sub(HORIZONTAL_PADDING + 1)
            } else {
                area.x
            };
            let jump_y = if self.event_list.sort == EventSort::Duration {
                borders[1].y
            } else {
                model_y
            };
            let jump_rule = Rect::new(
                event_left,
                jump_y,
                area.right().saturating_sub(event_left),
                1,
            );
            let mut jump_buffer = Buffer::empty(jump_rule);
            self.render_event_jump(jump_rule, &mut jump_buffer);
            let jump = self.viewport.borrow().event_label;
            let model_left = if !wide && baseline.gap.width > 0 {
                left
            } else {
                event_left
            };
            let baseline_model_y = if wide {
                baseline.prompt_top_rule.y
            } else {
                baseline.prompt_bottom_rule.y
            };
            let baseline_jump_y = if self.event_list.sort == EventSort::Duration {
                if wide {
                    baseline.prompt_top_rule.y
                } else {
                    baseline.event_detail_top_rule.y
                }
            } else {
                baseline_model_y
            };
            let reservation = if baseline_jump_y == baseline_model_y {
                jump
            } else {
                Rect::default()
            };
            self.render_event_model(
                Rect::new(
                    model_left,
                    model_y,
                    area.right().saturating_sub(model_left),
                    1,
                ),
                reservation,
                buf,
            );
            let mut occupied = self
                .viewport
                .borrow()
                .field_targets
                .iter()
                .filter(|(_, field)| matches!(field, Field::Model | Field::Effort))
                .map(|(rect, _)| *rect)
                .reduce(|left, right| left.union(right))
                .map(|rect| Rect::new(rect.x - 1, rect.y, rect.width + 2, 1))
                .into_iter()
                .collect::<Vec<_>>();
            let placed_jump = place_frame_label(jump, event_left, &occupied);
            if let Some(placed) = placed_jump {
                for offset in 0..jump.width {
                    buf[(placed.x + offset, placed.y)] =
                        jump_buffer[(jump.x + offset, jump.y)].clone();
                }
                occupied.push(placed);
            }
            {
                let mut viewport = self.viewport.borrow_mut();
                viewport.event_label = placed_jump.unwrap_or_default();
                viewport.event_jump = if self.event_list.sort == EventSort::Time {
                    viewport.event_label
                } else {
                    Rect::default()
                };
            }
            let baseline_filter_y = if wide {
                baseline.prompt_top_rule.y
            } else {
                baseline.event_detail_top_rule.y
            };
            let merged = borders[1].y == model_y && baseline_filter_y != baseline_model_y;
            self.render_event_filter(borders[1], merged, &occupied, buf);
            self.render_activity_strip(at(baseline.list, layout.prompt_top_rule.y), buf, now);
        }
        if !title.is_empty() && baseline.task_details.width > 0 {
            self.render_title_rule(title, buf);
        }
        // A whole status label yields on overlap, including its surrounding spaces.
        let mut x = status.x;
        while x < status.right() {
            if status_labels[(x, status.y)] == status_base[(x, status.y)] {
                x += 1;
                continue;
            }
            let start = x;
            while x < status.right() && status_labels[(x, status.y)] != status_base[(x, status.y)] {
                x += 1;
            }
            if (start..x)
                .all(|column| buf[(column, status.y)] == before[usize::from(column - status.x)])
            {
                for column in start..x {
                    buf[(column, status.y)] = status_labels[(column, status.y)].clone();
                }
            }
        }
    }

    pub(super) fn render_confirm_dialog(
        &self,
        dialog: &ConfirmDialog,
        area: Rect,
        buf: &mut Buffer,
    ) {
        use ratatui::widgets::{Block, Borders};
        let width = area.width.min(80);
        let content_width = usize::from(width.saturating_sub(4).max(1));
        let (title, description, confirm_label) = match &dialog.action {
            ConfirmAction::Delete(id) => {
                let title = format!("Permanently delete “{}”?", dialog.title);
                let description = "This stops any running work in this task and its child agents, then permanently deletes their history. This cannot be undone.";
                let description = if self
                    .rows
                    .iter()
                    .any(|session| session.id == *id && session.background_job.is_some())
                {
                    format!("{description} Worktrees and branches created by agents remain.")
                } else {
                    description.to_owned()
                };
                (
                    title,
                    description,
                    "Permanently delete task and child agents",
                )
            }
            ConfirmAction::Reset { provider, request } => {
                let (title, description) =
                    reset_credit::reset_text(*provider, &request.offer, Utc::now());
                (title, description, "Redeem reset credit")
            }
        };
        let mut lines = Vec::new();
        for text in textwrap::wrap(&title, content_width) {
            lines.push(Line::from(text.into_owned().bold()));
        }
        for text in textwrap::wrap(&description, content_width) {
            lines.push(Line::from(text.into_owned().dim()));
        }
        lines.push(Line::default());
        let options_start = lines.len() as u16;
        for (index, label) in ["Cancel", confirm_label].into_iter().enumerate() {
            let confirm = index == 1;
            let selected = dialog.confirm == confirm;
            let text = format!(
                "{} {}. {label}",
                if selected { PROMPT_MARKER } else { " " },
                index + 1
            );
            lines.push(Line::from(Span::styled(
                text,
                if selected {
                    super::style::selection_style()
                } else {
                    Style::default()
                },
            )));
        }
        let height = area.height.min(lines.len() as u16 + 2);
        let dialog = Rect::new(
            area.x + (area.width - width) / 2,
            area.y + (area.height - height) / 2,
            width,
            height,
        );
        self.clear_overlay(dialog, true, buf);
        let viewport = &mut *self.viewport.borrow_mut();
        let block = Block::default().borders(Borders::ALL);
        let inner = block.inner(dialog).inner(Margin::new(1, 0));
        viewport.confirm_options = [false, true]
            .into_iter()
            .enumerate()
            .filter_map(|(index, confirm)| {
                let y = inner.y + options_start + index as u16;
                (y < inner.bottom()).then_some((Rect::new(inner.x, y, inner.width, 1), confirm))
            })
            .collect();
        block.render(dialog, buf);
        Paragraph::new(lines).render(inner, buf);
    }
    /// Centers the bold status on the rule row `rule`, whose end cells are its corners, a space
    /// apart from the rule on each side, with the elapsed time at its right end.
    fn render_status_rule(&self, rule: Rect, buf: &mut Buffer, now: chrono::DateTime<chrono::Utc>) {
        let session = self
            .selected_row()
            .filter(|_| self.selected_draft().is_none());
        let status = match (session, self.selected_draft()) {
            (Some(session), _) => Self::status(session).0,
            (None, Some(_)) => SessionStatus::New.label(),
            (None, None) => return,
        };
        self::rule(rule, buf);
        // The status stays centered on the whole rule, keeping both end labels clear.
        let fits = |text: &String| usize::from(rule.width) >= 2 * (text.width() + 2) + 7;
        let elapsed = session
            .and_then(|session| {
                let start = session.started_at?;
                let end = if session.status == SessionStatus::Inactive {
                    session.last_record_at.unwrap_or(session.updated_at)
                } else {
                    now
                };
                Some(format_duration((end - start).num_seconds()))
            })
            .filter(fits);
        let cache_remaining = session
            .and_then(|session| session.cache_expires_at)
            .map(|expires| expires - now);
        // A Claude subagent's five-minute cache counts down only in its last minute.
        let countdown_window = if session.is_some_and(|session| {
            session.id.provider == ProviderId::Claude && session.parent.is_some()
        }) {
            chrono::Duration::seconds(59)
        } else {
            chrono::Duration::minutes(15)
        };
        let elapsed = elapsed.map(|elapsed| {
            let mut spans = Vec::new();
            if let Some(seconds) = cache_remaining
                .filter(|remaining| {
                    *remaining >= chrono::Duration::seconds(1) && *remaining <= countdown_window
                })
                .map(|remaining| remaining.num_seconds())
            {
                let countdown = format_duration(seconds);
                if fits(&format!("{countdown} · {elapsed}")) {
                    spans.push(Span::styled(countdown, style::error_style()));
                    spans.push(" · ".dim());
                }
            }
            spans.push(elapsed.dim());
            Line::from(spans)
        });
        // A Claude task past a quarter of its context shows the percentage in the error color.
        let context = session.and_then(|session| {
            let percent = self.selected_context_percent()?;
            let text = format!("{percent}%");
            fits(&text).then(|| {
                if session.id.provider == ProviderId::Claude && percent > 25 {
                    Span::styled(text, style::error_style())
                } else {
                    text.dim()
                }
            })
        });
        let (left, right) = render_rule_ends(rule, 0, context.map(Line::from), elapsed, buf);
        let reserved = [left, right]
            .into_iter()
            .flatten()
            .map(|area| 2 * usize::from(area.width + 2))
            .max()
            .unwrap_or(0);
        // The corner, one rule cell and a space on each side.
        let width = usize::from(rule.width).saturating_sub(6 + reserved);
        let status = if width <= 1 {
            Line::from("…".bold())
        } else {
            // An expired prompt cache follows the status as ⏸︎.
            let status = if cache_remaining
                .is_some_and(|remaining| remaining < chrono::Duration::seconds(1))
            {
                format!("{status} ⏸\u{fe0e}")
            } else {
                status.to_string()
            };
            truncate_line_with_ellipsis_if_overflow(Line::from(status).bold(), width)
        };
        let (header, label) = centered_rule_label(truncate_line_to_width(status, width), rule);
        Clear.render(label, buf);
        line(header, label, buf);
    }
    /// Centers the selected task's name, or a new task's label, on the rule under the details
    /// panel, a space apart on each side; a double click edits a task's name. The project
    /// folder on the left and the pull request or branch on the right flank it.
    fn render_title_rule(&self, rule: Rect, buf: &mut Buffer) {
        let session = self
            .selected_row()
            .filter(|_| self.selected_draft().is_none());
        let draft = self.selected_draft();
        // A linked worktree is named by its repository, though its link opens the worktree.
        let (title, key, cwd, heading) = match (session, draft) {
            (Some(session), _) => (
                session.display_title().to_owned(),
                session.id.clone(),
                &session.cwd,
                self.project_groups[self.selected].heading.clone(),
            ),
            (None, Some(draft)) => (
                draft.name.clone().unwrap_or_else(|| "New task".to_owned()),
                draft.key(),
                &draft.setup.cwd,
                draft.heading(),
            ),
            (None, None) => return,
        };
        // The name, shown or edited, keeps the color it has in the task list.
        let name_style = self.title_style(&key);
        let title_style = name_style.bold();
        // The left corner is a joint, or the separator that keeps going down beside the prompt.
        self::rule(
            Rect::new(rule.x + 1, rule.y, rule.width.saturating_sub(1), 1),
            buf,
        );
        if buf[(rule.x, rule.y)].symbol() == "│" {
            buf[(rule.x, rule.y)].set_symbol("├");
        }
        let folder_width = usize::from(rule.width).saturating_sub(title.width().min(20) + 10) / 2;
        let folder_width = folder_width.min(usize::from(rule.width / 3));
        let folder = super::formatting::format_directory_display(&heading, Some(folder_width));
        let right = match (session, draft) {
            (Some(session), _) => session
                .pr
                .as_ref()
                .map(|pr| Span::styled(format!("#{}", pr.number), self.title_style(&session.id)))
                .or_else(|| {
                    session
                        .git_branch
                        .as_ref()
                        .map(|branch| Span::styled(branch.clone(), style::time_style()))
                }),
            (None, Some(draft)) => (!draft.branch_name().is_empty())
                .then(|| draft.branch_name().to_owned())
                .map(|branch| Span::styled(branch, style::time_style())),
            _ => None,
        };
        // Keep short branch labels intact; longer labels share the folder's width budget.
        let right = right.map(|span| {
            truncate_line_with_ellipsis_if_overflow(Line::from(span), folder_width.max(20))
        });
        // The title stays centered on the whole rule, so the wider side label's width is kept
        // clear on both sides, plus the corner, one rule cell and a space on each side.
        let side = right
            .as_ref()
            .map_or(0, |line| line.width())
            .max(folder.width());
        let width = usize::from(rule.width).saturating_sub(6 + 2 * (side + 2));
        let editor = self.title_editor();
        let title = if let Some(editor) = editor {
            let placeholder = self.name_target.is_some() && editor.is_empty();
            let display_width = if placeholder {
                "New task".width()
            } else {
                editor.text().width() + 1
            };
            let input_width = display_width.min(width);
            let (text, _) = editor.window(input_width);
            let (text, style) = if placeholder {
                ("New task", Style::default().dim())
            } else {
                (text, name_style)
            };
            let mut title = truncate_line_to_width(
                Line::from(Span::styled(text.to_owned(), style)),
                input_width,
            );
            title
                .spans
                .push(" ".repeat(input_width.saturating_sub(title.width())).into());
            title
        } else if width <= 1 {
            truncate_line_to_width(Line::from(Span::styled("…", title_style)), width.max(1))
        } else {
            truncate_line_with_ellipsis_if_overflow(
                Line::from(Span::styled(title, title_style)),
                width,
            )
        };
        let mut label = title;
        label.spans.insert(0, " ".into());
        label.spans.push(" ".into());
        let label_width = (label.width() as u16).min(rule.width);
        let label_area = Rect::new(
            rule.x + rule.width.saturating_sub(label_width) / 2,
            rule.y,
            label_width,
            1,
        );
        Clear.render(label_area, buf);
        line(label, label_area, buf);
        let input = label_area.inner(Margin::new(1, 0));
        if editor.is_some() {
            self.viewport.borrow_mut().title_input = input;
        }
        if session.is_some() || draft.is_some_and(|draft| draft.name.is_some()) {
            self.viewport.borrow_mut().task_details_title = input;
        }
        let (folder_area, right_area) = render_rule_ends(
            rule,
            label_width,
            Some(Line::from(Span::styled(folder, style::time_style()))),
            right,
            buf,
        );
        let mut viewport = self.viewport.borrow_mut();
        let Some(session) = session else {
            // A new task's folder is still a choice, so it opens the repository browser, not the folder.
            for (area, field) in [(folder_area, Field::Directory), (right_area, Field::Branch)] {
                if let Some(area) = area {
                    viewport.field_targets.push((area, field));
                }
            }
            return;
        };
        let mut links = Vec::new();
        if let Some(area) = folder_area
            && let Some(target) = super::links::existing(cwd.clone())
        {
            links.push((area, target));
        }
        if let (Some(area), Some(pr)) = (right_area, &session.pr) {
            let url = super::vscode::pull_request_link(&pr.url, &session.cwd)
                .unwrap_or_else(|| pr.url.clone());
            super::hyperlink::hyperlink(buf, area, &url);
            links.push((area, PathBuf::from(url)));
        }
        super::links::underline(buf, &links);
        viewport.task_details_links.extend(links);
    }

    fn render_event_filter(&self, rule: Rect, merged: bool, occupied: &[Rect], buf: &mut Buffer) {
        let title = match self.event_list.filter {
            crate::event::EventFilter::All => return,
            crate::event::EventFilter::Message => "Assistant",
            crate::event::EventFilter::Tool => "Tool",
        };
        let width = title.width() as u16;
        if rule.is_empty() || rule.width < width + 4 {
            return;
        }
        let (header, label) = centered_rule_label(Line::from(title), rule);
        let label = if merged {
            let Some(label) = place_frame_label(label, rule.x, occupied) else {
                return;
            };
            label
        } else {
            let viewport = self.viewport.borrow();
            if label.intersects(viewport.event_label)
                || viewport
                    .field_targets
                    .iter()
                    .any(|(target, _)| label.intersects(*target))
            {
                return;
            }
            label
        };
        Clear.render(label, buf);
        line(header, label, buf);
    }

    /// Groups the model and effort on the left; each value opens its own picker.
    fn render_event_model(&self, rule: Rect, reservation: Rect, buf: &mut Buffer) {
        let draft = self.selected_draft();
        let (model, effort) = match (self.selected_row().filter(|_| draft.is_none()), draft) {
            // A session shows only the settings it has reported; nothing stands in for the rest.
            (Some(session), _) => (
                session
                    .model
                    .as_deref()
                    .filter(|model| !model.is_empty())
                    .map(|model| self.model_text(session.id.provider, model)),
                session.effort.clone(),
            ),
            // A new task shows its defaults too, so both stay clickable.
            (None, Some(draft)) => (
                self.draft_model(draft)
                    .map(|model| self.model_text(draft.setup.provider, model)),
                self.draft_effort(draft).map(str::to_owned),
            ),
            (None, None) => return,
        };
        let separator = super::title_rule::SEPARATOR;
        let right = if reservation.intersects(rule) {
            reservation.x.saturating_sub(1)
        } else {
            rule.right().saturating_sub(2)
        };
        let mut x = rule.x + 2;
        let values: Vec<_> = [(model, Field::Model), (effort, Field::Effort)]
            .into_iter()
            .filter_map(|(value, field)| {
                value
                    .filter(|value| !value.is_empty())
                    .map(|value| (value, field))
            })
            .collect();
        for (index, (value, field)) in values.iter().enumerate() {
            let gap = if index == 0 {
                0
            } else {
                separator.width() as u16
            };
            let mut available = right.saturating_sub(x).saturating_sub(gap);
            if index == 0 && values.len() == 2 {
                let reserve = (values[1].0.width() as u16).min(available / 2);
                available = available.saturating_sub(reserve + separator.width() as u16);
            }
            if available == 0 {
                break;
            }
            if gap > 0 {
                Line::from(separator)
                    .style(style::time_style())
                    .render(Rect::new(x, rule.y, gap, 1), buf);
                x += gap;
            } else {
                buf[(x - 1, rule.y)]
                    .set_symbol("╴")
                    .set_style(style::time_style());
            }
            let text = truncate_line_with_ellipsis_if_overflow(
                Line::from(value.clone()),
                usize::from(available),
            );
            let width = text.width() as u16;
            let area = Rect::new(x, rule.y, width, 1);
            text.style(style::time_style()).render(area, buf);
            self.viewport
                .borrow_mut()
                .field_targets
                .push((area, *field));
            x += width;
            buf[(x, rule.y)]
                .set_symbol(" ")
                .set_style(style::time_style());
        }
    }
    pub(super) fn render_task_details(
        &self,
        area: Rect,
        buf: &mut Buffer,
        now: chrono::DateTime<chrono::Utc>,
    ) {
        {
            let mut viewport = self.viewport.borrow_mut();
            viewport.task_details = Rect::default();
            viewport.task_details_title = Rect::default();
            viewport.title_input = Rect::default();
            viewport.task_details_input = Rect::default();
            viewport.task_details_copy.clear();
            viewport.task_details_background.clear();
            viewport.task_details_links.clear();
            viewport.task_details_text = text_selection::TextRegion::default();
        }
        if self.selected_draft().is_some() {
            self.viewport.borrow_mut().task_details = area;
            return;
        }
        let Some(session) = self.selected_row() else {
            return;
        };
        self.viewport.borrow_mut().task_details = area;
        let width = usize::from(area.width.max(1));
        // Sections top to bottom: background, activity, update, prompt and queued
        // messages, needs input, tasks. The title sits on the rule below the panel.
        let content_top = area.y;
        let mut items: Vec<Item> = Vec::new();
        let mut continuations = std::collections::BTreeMap::new();
        let mut bodies = Vec::new();
        // The full text a double click copies from each body, parallel to `bodies`.
        let mut copies: Vec<Option<(&str, &'static str)>> = Vec::new();
        // A fingerprint of each body's text, parallel to `bodies`.
        let mut keys = Vec::new();
        // The links inside each body's text, parallel to `bodies`.
        let mut body_links: Vec<Vec<super::links::Link>> = Vec::new();
        // The columns each item draws a link on, parallel to `items`; bodies fill theirs
        // once their rows are mapped to their text.
        let mut item_links: Vec<Vec<(std::ops::Range<u16>, PathBuf)>> = Vec::new();

        // The agent appends an exit marker to the output file when the
        // command ends, which lands before the completion notification does.
        let boxes = self
            .background_for(session)
            .map(|task| {
                (
                    task,
                    task.output
                        .as_deref()
                        .map(|path| tail_lines(path, 1))
                        .unwrap_or_default(),
                )
            })
            .filter(|(_, lines)| {
                !lines
                    .last()
                    .is_some_and(|last| last.starts_with("[exited with code"))
            })
            .collect::<Vec<_>>();
        let mut background = items.len()..items.len();
        let subagents = self.subagents_for(session).collect::<Vec<_>>();
        if !boxes.is_empty() || !subagents.is_empty() {
            separate_section(&mut items);
            items.push(Item::Decoration(centered_header(
                "Background".dim().into(),
                Line::default(),
                width,
            )));
            let start = items.len();
            items.extend(
                boxes
                    .into_iter()
                    .map(|(task, output)| Item::Background { task, output }),
            );
            items.extend(subagents.into_iter().map(Item::Subagent));
            // Mix tasks and descendants: two-line entries first, then oldest starts,
            // unknown starts last, with identity/path breaking ties regardless of depth or kind.
            items[start..].sort_by_key(|item| {
                let (started_at, identity, provider) = match item {
                    Item::Background { task, .. } => (
                        Some(task.started_at),
                        task.output
                            .as_deref()
                            .map(std::path::Path::as_os_str)
                            .unwrap_or_else(|| std::ffi::OsStr::new(&task.description)),
                        None,
                    ),
                    Item::Subagent(child) => (
                        child.started_at,
                        std::ffi::OsStr::new(&child.id.id),
                        Some(child.id.provider),
                    ),
                    Item::Text(_) | Item::Decoration(_) => {
                        unreachable!("Background contains only tasks and descendants")
                    }
                };
                (
                    item.height() == 1,
                    started_at.is_none(),
                    started_at,
                    identity,
                    provider,
                )
            });
            background = start..items.len();
        }

        item_links.resize(items.len(), Vec::new());

        let activity = session.activity.as_ref();
        if let Some(activity) = activity {
            let label = super::event_list::summary_line(
                &activity.label,
                &activity.kind,
                activity.exit_code,
                &activity.paths,
                width,
                &session.cwd,
            )
            .line;
            separate_section(&mut items);
            let start = items.len() + 1;
            items.extend([
                Item::Decoration(centered_header(
                    "Last activity".dim().into(),
                    Span::styled(
                        format_relative_time(now, Some(activity.at)),
                        style::time_style(),
                    )
                    .into(),
                    width,
                )),
                truncate_line_with_ellipsis_if_overflow(label, width).into(),
            ]);
            bodies.push(start..items.len());
            keys.push(body_key("Last activity", &activity.label));
            copies.push(Some((activity.label.as_str(), "Copied last activity")));
            body_links.push(if super::text::is_prose(&activity.kind) {
                super::links::text_links(&activity.label, &session.cwd, &[])
            } else {
                super::links::path_links(
                    &activity.label,
                    &super::event_list::summary_paths(
                        &activity.label,
                        &activity.paths,
                        &session.cwd,
                    ),
                    &session.cwd,
                )
            });
        }

        item_links.resize(items.len(), Vec::new());

        if let Some(message) = session.last_message.as_ref() {
            let mut header = Line::default();
            if let Some(at) = session.last_message_at {
                header =
                    Span::styled(format_relative_time(now, Some(at)), style::time_style()).into();
            }
            separate_section(&mut items);
            items.push(Item::Decoration(centered_header(
                "Update".dim().into(),
                header,
                width,
            )));
            let start = items.len();
            let body_style = style::event_styles(&crate::event::Kind::Assistant).1;
            let (lines, breaks) = super::text::layout(message, &session.cwd, width, true);
            continuations.extend(
                breaks
                    .into_iter()
                    .enumerate()
                    .filter_map(|(row, join)| join.map(|space| (start + row, space))),
            );
            items.extend(
                lines
                    .into_iter()
                    .map(|mut line| {
                        for span in &mut line.spans {
                            span.style = body_style.patch(span.style);
                        }
                        line
                    })
                    .map(Item::from),
            );
            bodies.push(start..items.len());
            keys.push(body_key("Update", message));
            copies.push(Some((message.as_str(), "Copied update")));
            body_links.push(super::links::text_links(message, &session.cwd, &[]));
        }

        if !session.prompt.trim().is_empty()
            || session
                .parent
                .as_ref()
                .is_none_or(|parent| parent.provider != ProviderId::Codex)
        {
            let prompt_header = if let Some(elapsed) = turn_elapsed(session, now) {
                centered_header("Prompt".dim().into(), elapsed.dim().into(), width)
            } else {
                centered_header("Prompt".dim().into(), Line::default(), width)
            };
            separate_section(&mut items);
            items.push(Item::Decoration(prompt_header));
            let prompt_text = match session.prompt.trim() {
                "" => "No prompt available.",
                prompt => prompt,
            };
            let prompt_start = items.len();
            let (prompt_body, breaks) = super::text::layout(prompt_text, &session.cwd, width, true);
            continuations.extend(
                breaks
                    .into_iter()
                    .enumerate()
                    .filter_map(|(row, join)| join.map(|space| (prompt_start + row, space))),
            );
            items.extend(prompt_body.into_iter().map(Item::from));
            bodies.push(prompt_start..items.len());
            keys.push(body_key("Prompt", &session.prompt));
            copies.push(
                Some((session.prompt.as_str(), "Copied prompt"))
                    .filter(|(prompt, _)| !prompt.trim().is_empty()),
            );
            body_links.push(super::links::text_links(
                &session.prompt,
                &session.cwd,
                &session.prompt_images,
            ));
            item_links.resize(items.len(), Vec::new());
        }
        for queued in &session.queued {
            separate_section(&mut items);
            let start = items.len();
            let (lines, breaks) = super::text::layout(queued, &session.cwd, width, true);
            continuations.extend(
                breaks
                    .into_iter()
                    .enumerate()
                    .filter_map(|(row, join)| join.map(|space| (start + row, space))),
            );
            items.extend(lines.into_iter().map(Item::from));
            bodies.push(start..items.len());
            keys.push(body_key("Prompt", queued));
            copies.push(Some((queued.as_str(), "Copied prompt")));
            body_links.push(super::links::text_links(queued, &session.cwd, &[]));
        }

        item_links.resize(items.len(), Vec::new());

        let needs = session
            .background_job
            .as_ref()
            .and_then(|job| job.needs.as_deref());
        if session.status == SessionStatus::NeedsInput || needs.is_some() {
            separate_section(&mut items);
            let start = items.len();
            items.push(Item::Decoration(centered_header(
                Span::styled("Needs input", style::attention_title_style()).into(),
                Line::default(),
                width,
            )));
            if let Some(needs) = needs {
                items.extend(
                    super::text::lines(needs, &session.cwd, width, false)
                        .into_iter()
                        .map(Item::from),
                );
            } else if session.pending_questions.is_empty() {
                items.extend([
                    "Open task to review.".dim().into(),
                    "Waiting for your response.".into(),
                ]);
            } else {
                items.extend(session.pending_questions.iter().map(|question| {
                    truncate_line_with_ellipsis_if_overflow(
                        super::text::summary(
                            question,
                            &session.cwd,
                            width,
                            &crate::event::Kind::Assistant,
                            None,
                        ),
                        width,
                    )
                    .into()
                }));
            }
            bodies.push(start..items.len());
            keys.push(body_key(
                "Needs input",
                &(needs, &session.pending_questions),
            ));
            copies.push(None);
            body_links.push(Vec::new());
        }
        item_links.resize(items.len(), Vec::new());

        let tasks = [
            (crate::model::TaskStatus::InProgress, SessionStatus::Working),
            (crate::model::TaskStatus::Pending, SessionStatus::Ready),
        ]
        .into_iter()
        .flat_map(|(status, indicator)| {
            session
                .tasks
                .iter()
                .filter(move |task| task.status == status)
                .map(move |task| (task, indicator))
        })
        .collect::<Vec<_>>();
        let tasks_text = tasks
            .iter()
            .map(|(task, status)| {
                format!(
                    "{} {}",
                    Self::status_indicator(*status).content,
                    task.subject
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        if !tasks.is_empty() {
            // The tasks section sits on the panel's bottom row when everything fits.
            let above = items.iter().map(Item::height).sum::<usize>();
            let section = usize::from(!items.is_empty()) + 1 + tasks.len();
            let gap = usize::from(area.height).saturating_sub(above + section);
            items.extend((0..gap).map(|_| Item::Decoration(Line::default())));
            item_links.resize(items.len(), Vec::new());
            separate_section(&mut items);
            let in_progress = tasks
                .iter()
                .filter(|(_, status)| *status == SessionStatus::Working)
                .count();
            items.push(Item::Decoration(centered_header(
                "Tasks".dim().into(),
                format!("{in_progress}/{}", tasks.len()).dim().into(),
                width,
            )));
            let start = items.len();
            for (task, status) in tasks {
                items.push(
                    truncate_line_with_ellipsis_if_overflow(
                        Line::from(vec![
                            Self::status_indicator(status),
                            Span::raw(format!(" {}", task.subject)),
                        ]),
                        width,
                    )
                    .into(),
                );
            }
            bodies.push(start..items.len());
            keys.push(body_key("Tasks", &tasks_text));
            copies.push(Some((tasks_text.as_str(), "Copied tasks")));
            body_links.push(Vec::new());
            item_links.resize(items.len(), Vec::new());
        }

        let mut item_ids = (0..items.len()).collect::<Vec<_>>();
        let mut text_rows = vec![None; items.len()];
        let mut sources = Vec::new();
        for (id, (body, copy)) in bodies.iter().zip(&copies).enumerate() {
            let Some((source, label)) = copy else {
                continue;
            };
            let lines = items[body.clone()]
                .iter()
                .map(|item| match item {
                    Item::Text(line) => line.clone(),
                    _ => unreachable!("copyable bodies contain text rows"),
                })
                .collect::<Vec<_>>();
            let markdown = match *label {
                "Copied last activity" => {
                    activity.is_some_and(|activity| super::text::is_prose(&activity.kind))
                }
                "Copied tasks" => false,
                _ => true,
            };
            let maps = super::text_selection_layout::map_lines(source, &lines, markdown);
            for (index, cells) in body.clone().zip(maps) {
                item_links[index] = super::links::row_links(&cells, &body_links[id]);
                text_rows[index] = Some(text_selection::DrawnRow {
                    segment: body.start,
                    index,
                    continuation: continuations.get(&index).copied(),
                    x: area.x,
                    y: 0,
                    cells,
                    ..Default::default()
                });
            }
            sources.push(text_selection::Source {
                id: body.start,
                text: (*source).into(),
                label: (*label).to_owned(),
            });
        }
        self.viewport.borrow_mut().task_details_text = text_selection::TextRegion {
            area,
            sources,
            rows: Vec::new(),
        };
        let available = usize::from(area.height);
        let budget = budget_task_details(&items, &bodies, background, available);
        let (scroll, body_rows, total, indents) = {
            let mut viewport = self.viewport.borrow_mut();
            let state = &mut viewport.task_details_scroll;
            state.content_top = content_top;
            state.prepare(keys);
            let excluded = budget
                .as_ref()
                .and_then(|budget| budget.background_cut.as_ref())
                .map_or(0..0, |cut| {
                    items[cut.start] = "…".dim().into();
                    cut.start + 1..cut.end
                });
            let heights = items.iter().map(Item::height).collect::<Vec<_>>();
            let allocations = budget.as_ref().map(|budget| {
                budget
                    .bodies
                    .iter()
                    .map(|&(rows, _)| rows)
                    .collect::<Vec<_>>()
            });
            let layout = state.layout(
                &heights,
                &bodies,
                allocations.as_deref(),
                excluded,
                available,
            );
            for &cut in &layout.cuts {
                let (Item::Text(last) | Item::Decoration(last)) = &mut items[cut] else {
                    unreachable!("details bodies contain text rows");
                };
                *last = truncate_line_to_width(std::mem::take(last), width.saturating_sub(1));
                last.spans.push("…".dim());
                if let Some(mapped) = &mut text_rows[cut] {
                    mapped
                        .cells
                        .retain(|(cells, _)| cells.end <= width.saturating_sub(1) as u16);
                }
                item_links[cut].retain(|(cells, _)| cells.end <= width.saturating_sub(1) as u16);
            }
            // Each body's rows share one left edge, centered as a block in the panel.
            let mut indents = vec![0; items.len()];
            for body in &bodies {
                let block = items[body.clone()]
                    .iter()
                    .map(|item| match item {
                        Item::Text(text) => text.width(),
                        _ => 0,
                    })
                    .max()
                    .unwrap_or(0);
                let indent = (width.saturating_sub(block) / 2) as u16;
                for index in body.clone() {
                    if matches!(items[index], Item::Text(_)) {
                        indents[index] = indent;
                    }
                }
            }
            let mut keep = vec![false; items.len()];
            for &index in &layout.kept {
                keep[index] = true;
            }
            let mut rows = keep.iter();
            item_ids.retain(|_| *rows.next().unwrap());
            let mut rows = keep.iter();
            items.retain(|_| *rows.next().unwrap());
            let mut rows = keep.iter();
            text_rows.retain(|_| *rows.next().unwrap());
            let mut rows = keep.iter();
            item_links.retain(|_| *rows.next().unwrap());
            let mut rows = keep.iter();
            indents.retain(|_| *rows.next().unwrap());
            (layout.scroll, state.bodies.clone(), layout.total, indents)
        };
        self.viewport.borrow_mut().task_details_copy = body_rows
            .into_iter()
            .zip(copies)
            .filter_map(|(rows, text)| {
                let start = rows.start.max(scroll);
                let end = rows.end.min(scroll + available);
                let screen_row = |row: usize| content_top + (row - scroll) as u16;
                let (text, label) = text.filter(|_| start < end)?;
                Some((screen_row(start)..screen_row(end), text.to_owned(), label))
            })
            .collect();
        let content_area = Rect::new(0, 0, area.width, total as u16);
        let mut content = Buffer::empty(content_area);
        let mut offset = 0;
        let mut drawn_links = Vec::new();
        for ((((item, mapped), links), id), indent) in items
            .into_iter()
            .zip(text_rows)
            .zip(item_links)
            .zip(item_ids)
            .zip(indents)
        {
            let fallback = mapped.is_none() && !matches!(item, Item::Decoration(_));
            if offset >= scroll && offset < scroll + available {
                let y = area.y + (offset - scroll) as u16;
                if let Some(mut mapped) = mapped {
                    mapped.x += indent;
                    mapped.y = y;
                    self.viewport
                        .borrow_mut()
                        .task_details_text
                        .rows
                        .push(mapped);
                }
                drawn_links.extend(links.into_iter().map(|(columns, target)| {
                    (
                        Rect::new(
                            area.x + indent + columns.start,
                            y,
                            columns.end - columns.start,
                            1,
                        ),
                        target,
                    )
                }));
            }
            let height = item.height();
            let item_area = row(content_area, offset as u16, height as u16);
            let title_columns = match item {
                Item::Text(text) | Item::Decoration(text) => {
                    let mut text_area = item_area;
                    text_area.x += indent;
                    text_area.width = text_area.width.saturating_sub(indent);
                    line(text, text_area, &mut content);
                    None
                }
                Item::Background { task, output } => {
                    let start = offset.max(scroll);
                    let end = (offset + height).min(scroll + available);
                    if start < end && task.output.is_some() {
                        self.viewport.borrow_mut().task_details_background.push((
                            content_top + (start - scroll) as u16
                                ..content_top + (end - scroll) as u16,
                            task.clone(),
                        ));
                    }
                    Some(render_background(
                        task,
                        output,
                        &session.cwd,
                        now,
                        item_area,
                        &mut content,
                    ))
                }
                Item::Subagent(child) => {
                    let (text, kind, exit_code, paths) = subagent_line(child);
                    let body = (!text.is_empty()).then(|| {
                        truncate_line_with_ellipsis_if_overflow(
                            super::event_list::summary_line(
                                text,
                                &kind,
                                exit_code,
                                paths,
                                usize::from(item_area.width),
                                &child.cwd,
                            )
                            .line,
                            usize::from(item_area.width),
                        )
                    });
                    // A known turn uses the same clock as its Prompt. Without a
                    // turn timestamp, show the child's own age.
                    let elapsed = turn_elapsed(child, now).or_else(|| {
                        child
                            .started_at
                            .map(|at| format_duration((now - at).num_seconds().max(0)))
                    });
                    Some(render_background_box(
                        child.display_title(),
                        elapsed.as_deref(),
                        body.into_iter().collect(),
                        item_area,
                        &mut content,
                    ))
                }
            };
            if fallback {
                let mut source = String::new();
                let mut rows = Vec::new();
                for dy in 0..height {
                    let y = offset + dy;
                    let mut cells = Vec::new();
                    let mut x = 0;
                    let mut pending_space = Vec::new();
                    while x < area.width {
                        let symbol = content[(x, y as u16)].symbol();
                        let width = symbol.width().max(1) as u16;
                        if dy == 0
                            && title_columns.as_ref().is_some_and(|ranges| {
                                !ranges.iter().any(|range| range.contains(&x))
                            })
                        {
                            x += width;
                            continue;
                        }
                        if symbol.trim().is_empty() {
                            pending_space.push((x..x + width, symbol.to_owned()));
                        } else {
                            if !cells.is_empty() {
                                for (columns, symbol) in &pending_space {
                                    let start = source.len();
                                    source.push_str(symbol);
                                    cells.push((columns.clone(), start..source.len()));
                                }
                            }
                            pending_space.clear();
                            let start = source.len();
                            source.push_str(symbol);
                            cells.push((x..x + width, start..source.len()));
                        }
                        x += width;
                    }
                    if !cells.is_empty() {
                        rows.push((y, cells));
                        source.push('\n');
                    }
                }
                if !source.is_empty() {
                    source.pop();
                    let mut viewport = self.viewport.borrow_mut();
                    viewport
                        .task_details_text
                        .sources
                        .push(text_selection::Source {
                            id,
                            text: source.into(),
                            label: "Copied selection".into(),
                        });
                    for (y, cells) in rows {
                        if y >= scroll && y < scroll + available {
                            viewport
                                .task_details_text
                                .rows
                                .push(text_selection::DrawnRow {
                                    segment: id,
                                    index: y,
                                    x: area.x,
                                    y: area.y + (y - scroll) as u16,
                                    cells,
                                    ..Default::default()
                                });
                        }
                    }
                }
            }
            offset += height;
        }
        for y in 0..available.min(total - scroll) {
            for x in 0..area.width {
                buf[(area.x + x, area.y + y as u16)] = content[(x, (scroll + y) as u16)].clone();
            }
        }
        if budget.is_none() && total - scroll > available && available > 0 {
            let last = row(area, (available - 1) as u16, 1);
            Clear.render(last, buf);
            line("…".dim(), last, buf);
            self.viewport
                .borrow_mut()
                .task_details_text
                .rows
                .retain(|row| row.y != last.y);
            drawn_links.retain(|(rect, _)| rect.y != last.y);
            self.viewport
                .borrow_mut()
                .field_targets
                .retain(|(rect, _)| !rect.intersects(last));
        }
        self.viewport
            .borrow_mut()
            .task_details_text
            .sources
            .sort_by_key(|source| source.id);
        self.viewport.borrow_mut().task_details_links = drawn_links;
        self.highlight_text_selection(text_selection::Panel::TaskDetails, buf);
        super::links::underline(buf, &self.viewport.borrow().task_details_links);
    }
}

fn turn_elapsed(session: &Session, now: chrono::DateTime<chrono::Utc>) -> Option<String> {
    let start = session.prompt_at?;
    let end = if session.status == SessionStatus::Working {
        now
    } else {
        session.last_record_at.unwrap_or(start)
    };
    Some(format_duration((end - start).num_seconds().max(0)))
}

/// Puts a blank row between a section and the one above it.
fn separate_section(items: &mut Vec<Item<'_>>) {
    if !items.is_empty() {
        items.push(Item::Decoration(Line::default()));
    }
}

/// Share rows equally, returning unused shares from sections that fit.
pub(super) fn allocate(wants: &[usize], budget: usize) -> Vec<usize> {
    let mut rows = vec![0; wants.len()];
    let mut remaining = budget.min(wants.iter().sum());
    while remaining > 0 {
        let active = wants
            .iter()
            .zip(&rows)
            .filter(|(want, have)| want > have)
            .count();
        let share = remaining / active;
        let mut extra = remaining % active;
        for (have, want) in rows.iter_mut().zip(wants) {
            if *have == *want {
                continue;
            }
            let grant = (share + usize::from(extra > 0)).min(want - *have);
            extra = extra.saturating_sub(1);
            *have += grant;
            remaining -= grant;
        }
    }
    rows
}

struct TaskDetailsBudget {
    bodies: Vec<(usize, usize)>,
    background_cut: Option<std::ops::Range<usize>>,
}

/// A fingerprint of one details body: its section and the text it shows.
fn body_key(section: &str, text: &impl std::hash::Hash) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::hash::DefaultHasher::new();
    (section, text).hash(&mut hasher);
    hasher.finish()
}

fn budget_task_details(
    items: &[Item<'_>],
    bodies: &[std::ops::Range<usize>],
    background: std::ops::Range<usize>,
    available: usize,
) -> Option<TaskDetailsBudget> {
    let wants = bodies.iter().map(|body| body.len()).collect::<Vec<_>>();
    let floors = wants.iter().filter(|&&want| want > 0).count();
    let background_rows = items[background.clone()]
        .iter()
        .map(Item::height)
        .sum::<usize>();
    let static_rows = items.iter().map(Item::height).sum::<usize>()
        - wants.iter().sum::<usize>()
        - background_rows;
    if available < static_rows + floors + usize::from(background_rows > 0) {
        return None;
    }
    let mut kept_background = background_rows;
    let box_budget = available - static_rows - floors;
    let background_cut = if background_rows > box_budget {
        kept_background = 0;
        let mut end = background.start;
        for index in background.clone() {
            let height = items[index].height();
            if kept_background + height + 1 > box_budget {
                break;
            }
            kept_background += height;
            end = index + 1;
        }
        kept_background += 1;
        Some(end..background.end)
    } else {
        None
    };
    let bodies = allocate(&wants, available - static_rows - kept_background)
        .into_iter()
        .zip(wants)
        .map(|(alloc, want)| (alloc, want - alloc))
        .collect();
    Some(TaskDetailsBudget {
        bodies,
        background_cut,
    })
}

/// Centers a section label, fitting its right value into the space after it.
fn centered_header(label: Line<'static>, value: Line<'static>, width: usize) -> Line<'static> {
    let label = truncate_line_with_ellipsis_if_overflow(label, width);
    let start = (width - label.width()) / 2;
    let available = width.saturating_sub(start + label.width() + 1);
    let value = truncate_line_with_ellipsis_if_overflow(value, available);
    let mut header = Line::from(" ".repeat(start).dim());
    header.spans.extend(label.spans);
    if value.width() > 0 {
        header
            .spans
            .push(" ".repeat(width - header.width() - value.width()).dim());
        header.spans.extend(value.spans);
    }
    header
}

/// Joins each vertical separator, given as its column and rows, to the rules above and below it.
fn join_separators(separators: &[(u16, u16, u16)], area: Rect, buf: &mut Buffer) {
    for &(x, top, bottom) in separators {
        for (y, joint) in [(top.checked_sub(1), "┬"), (Some(bottom), "┴")] {
            let Some(y) = y.filter(|&y| y >= area.y && y < area.bottom()) else {
                continue;
            };
            let cell = &mut buf[(x, y)];
            match (cell.symbol(), joint) {
                ("─", _) => {
                    cell.set_symbol(joint);
                }
                ("┬", "┴") | ("┴", "┬") => {
                    cell.set_symbol("┼");
                }
                _ => {}
            }
        }
    }
}

fn centered_rule_label(title: Line<'_>, rule: Rect) -> (Line<'_>, Rect) {
    let mut header = title.bold();
    header.spans.insert(0, " ".into());
    header.spans.push(" ".into());
    let width = header.width() as u16;
    let x = rule.x + rule.width.saturating_sub(width) / 2;
    let label = Rect::new(x, rule.y, width.min(rule.width), 1).intersection(rule);
    (header, label)
}

/// Keeps a label in its columns unless an occupied label forces it left, leaving a rule cell.
fn place_frame_label(mut label: Rect, left: u16, occupied: &[Rect]) -> Option<Rect> {
    if label.is_empty() {
        return None;
    }
    loop {
        if label.x < left {
            return None;
        }
        let Some(obstacle) = occupied
            .iter()
            .filter(|rect| label.intersects(**rect))
            .min_by_key(|rect| rect.x)
        else {
            return Some(label);
        };
        label.x = obstacle.x.checked_sub(label.width + 1)?;
    }
}

/// Draws a dim rule across `area`'s first row.
fn rule(area: Rect, buf: &mut Buffer) {
    line("─".repeat(usize::from(area.width)).dim(), area, buf);
}

/// Draws end labels with a half line toward their corner and a space toward the center,
/// keeping clear the `center` columns in the rule's middle. Returns where each text landed.
fn render_rule_ends(
    rule: Rect,
    center: u16,
    left: Option<Line<'static>>,
    right: Option<Line<'static>>,
    buf: &mut Buffer,
) -> (Option<Rect>, Option<Rect>) {
    // Each side keeps its corner cell, a space on both sides of its text and a rule cell before
    // the center.
    let side = usize::from(rule.width.saturating_sub(center) / 2).saturating_sub(4);
    let mut place = |text: Option<Line<'static>>, from_left: bool| {
        let text = truncate_line_with_ellipsis_if_overflow(text?, side);
        let width = text.width() as u16;
        if width == 0 {
            return None;
        }
        let x = if from_left {
            rule.x + 2
        } else {
            rule.right() - 2 - width
        };
        let area = Rect::new(x, rule.y, width, 1);
        Clear.render(Rect::new(x - 1, rule.y, width + 2, 1), buf);
        let (padding, symbol) = if from_left {
            (x - 1, "╴")
        } else {
            (x + width, "╶")
        };
        buf[(padding, rule.y)]
            .set_symbol(symbol)
            .set_style(Style::default().dim());
        text.render(area, buf);
        Some(area)
    };
    (place(left, true), place(right, false))
}

/// One row-aligned piece of the task details panel.
enum Item<'a> {
    Text(Line<'static>),
    Decoration(Line<'static>),
    Subagent(&'a Session),
    Background {
        task: &'a crate::model::BackgroundTask,
        output: Vec<String>,
    },
}

impl Item<'_> {
    fn height(&self) -> usize {
        match self {
            Self::Text(_) | Self::Decoration(_) => 1,
            Self::Background { output, .. } => box_height(output.len()),
            Self::Subagent(child) => box_height(usize::from(!subagent_line(child).0.is_empty())),
        }
    }
}

impl<T: Into<Line<'static>>> From<T> for Item<'_> {
    fn from(text: T) -> Self {
        Self::Text(text.into())
    }
}

/// A working subagent's current tool call, or its last message between calls,
/// with the event list kind that styles it.
fn subagent_line(
    child: &Session,
) -> (
    &str,
    crate::event::Kind,
    Option<i64>,
    &[std::ops::Range<usize>],
) {
    let activity = child
        .latest_activity()
        .filter(|_| child.status == SessionStatus::Working);
    if let Some(activity) = activity {
        (
            &activity.label,
            activity.kind.clone(),
            activity.exit_code,
            &activity.paths,
        )
    } else {
        (
            child.last_message.as_deref().unwrap_or_default(),
            crate::event::Kind::Assistant,
            None,
            &[],
        )
    }
}

/// A running background command: its description and elapsed time on the top
/// rule, the last output line below it when there is one.
fn render_background(
    task: &crate::model::BackgroundTask,
    output: Vec<String>,
    cwd: &std::path::Path,
    now: chrono::DateTime<chrono::Utc>,
    box_area: Rect,
    buf: &mut Buffer,
) -> Vec<std::ops::Range<u16>> {
    let elapsed = format_duration((now - task.started_at).num_seconds().max(0));
    render_background_box(
        &task.description,
        Some(&elapsed),
        output
            .into_iter()
            .flat_map(|text| {
                super::tool_detail::Part::text_parts(text, false)
                    .into_iter()
                    .flat_map(|part| {
                        part.lines_in(usize::from(box_area.width), Style::default(), now, cwd)
                    })
            })
            .collect(),
        box_area,
        buf,
    )
}

fn render_background_box(
    description: &str,
    elapsed: Option<&str>,
    output: Vec<Line<'static>>,
    box_area: Rect,
    buf: &mut Buffer,
) -> Vec<std::ops::Range<u16>> {
    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().dim());
    let inner = block.inner(box_area);
    block.render(box_area, buf);
    let title =
        super::title_rule::centered_title_rule(description, usize::from(box_area.width), elapsed);
    // The first and last spans draw rules; the middle spans contain the title.
    let mut columns = Vec::new();
    if title.spans.len() >= 4 {
        let start = title.spans[0].width() as u16;
        let end = (title.width() - title.spans.last().unwrap().width()).saturating_sub(1) as u16;
        columns.push(start..end);
        if let Some(elapsed) = elapsed {
            let last = title.spans.last().unwrap();
            if last.content.ends_with(elapsed) {
                columns.push(box_area.width - elapsed.width() as u16..box_area.width);
            }
        }
    }
    line(title, row(box_area, 0, 1), buf);
    Paragraph::new(output).render(inner, buf);
    columns
}

/// Rows a background task takes: its titled rule and the output lines below.
fn box_height(lines: usize) -> usize {
    1 + lines
}

#[cfg(test)]
mod wrapped_path_tests {
    use super::*;

    const MESSAGE: &str = "报告已写到 /private/tmp/claude-501/-Users-satoshi-Code-blockchainian-claude/0ed4978e-9ece-4b9d-9bc5-04bf44fedacb/scratchpad/background-research/report.md，并已发给 team-lead。";

    #[test]
    fn task_details_and_event_share_structured_message_layout() {
        let source = include_str!("../../tests/fixtures/structured_text/real-theme.plist");
        let cwd = std::path::Path::new("/project");
        let expected: Vec<_> = super::super::tool_detail::Part::prose_parts(source.into(), true)
            .into_iter()
            .flat_map(|part| part.lines_in(80, Style::default(), chrono::DateTime::UNIX_EPOCH, cwd))
            .collect();
        assert_eq!(super::text::lines(source, cwd, 80, true), expected);
    }

    #[test]
    fn wrapped_task_details_keep_fitting_paths_and_markdown_styles() {
        for markdown in [false, true] {
            let lines = super::text::lines(
                "some prose before /tmp/fits.md and after",
                std::path::Path::new("/project"),
                20,
                markdown,
            );
            assert!(
                lines
                    .iter()
                    .any(|line| line.to_string().contains("/tmp/fits.md"))
            );
        }
        let lines = super::text::lines(
            "**/private/workspace/reports/report.md**",
            std::path::Path::new("/project"),
            20,
            true,
        );
        assert_eq!(lines[0].to_string(), "/private/…/report.md");
        assert!(lines[0].spans.iter().all(|span| {
            span.style
                .add_modifier
                .contains(ratatui::style::Modifier::BOLD)
        }));
    }

    #[test]
    fn task_details_last_message_and_prompt_show_abbreviated_screenshot_path() {
        let now = chrono::Utc::now();
        let session = Session {
            id: crate::providers::SessionKey {
                provider: crate::providers::ProviderId::Claude,
                id: "path-probe".into(),
            },
            parent: None,
            cwd: crate::test_project(),
            name: None,
            title: "Path probe".into(),
            prompt: MESSAGE.into(),
            task_details_sources: Default::default(),
            prompt_images: Vec::new(),
            queued: Vec::new(),
            prompt_at: None,
            turn_messages: 0,
            last_record_at: None,
            cache_expires_at: None,
            model: None,
            effort: None,
            git_branch: None,
            pr: None,
            status: SessionStatus::Ready,
            process: None,
            updated_at: now,
            started_at: None,
            last_message: Some(MESSAGE.into()),
            last_message_at: None,
            activity: None,
            ask: None,
            pending_questions: Vec::new(),
            background_job: None,
            background: Vec::new(),
            tasks: Vec::new(),
            last_prompt: None,
            transcript: None,
            archived: false,
        };
        let mut view = View::new(Grouping::Project, false);
        view.set_sessions(vec![session], false);
        let area = Rect::new(0, 0, 40, 30);
        let mut buf = Buffer::empty(area);
        view.render_task_details(area, &mut buf, now);
        let rows: Vec<String> = buf
            .content
            .chunks(40)
            .map(|row| row.iter().map(|cell| cell.symbol()).collect())
            .collect();
        assert_eq!(
            rows.iter()
                .filter(|row| row.contains("…/report.md"))
                .count(),
            2
        );
        assert!(rows.iter().any(|row| row.contains("Update")));
    }

    #[test]
    fn wrapped_task_details_abbreviate_screenshot_path_at_narrow_width() {
        let lines = super::text::lines(MESSAGE, std::path::Path::new("/project"), 40, true);
        let plain: Vec<_> = lines.iter().map(ToString::to_string).collect();
        assert_eq!(
            plain,
            [
                "报告已写到",
                "/private/tmp/claude-501/…/report.md，并",
                "已发给 team-lead。",
            ]
        );
        assert!(lines.iter().all(|line| line.width() <= 40));
    }
}

#[cfg(test)]
mod background_title_tests {
    use super::*;
    use unicode_width::UnicodeWidthStr;

    #[test]
    fn duration_label_occupies_jump_position_without_click_target_or_overlap() {
        let mut view = View::default();
        view.add_draft(DraftSetup {
            provider: ProviderId::Claude,
            cwd: std::env::temp_dir(),
            model: Some("a-model-name-that-needs-truncation".into()),
            effort: Some("high".into()),
            branch: "main".into(),
        });
        for width in [44, 80, 100, 149, 150, 200] {
            let area = Rect::new(0, 0, width, 40);
            let mut buf = Buffer::empty(area);
            view.event_list.sort = EventSort::Duration;
            view.event_list.restoring = false;
            view.render(area, &mut buf);
            let viewport = view.viewport.borrow();
            let top = viewport.event_borders[0];
            let y = viewport.event_borders[1].y;
            let label = Rect::new(viewport.event_panel.right() + 1 - 10, y, 10, 1);
            let text: String = (label.x..label.right())
                .map(|x| buf[(x, y)].symbol())
                .collect();
            assert_eq!(text, " duration ", "width {width}");
            let row: String = (0..width).map(|x| buf[(x, y)].symbol()).collect();
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
                "tests/ui_snapshots/event_duration_label_{width}.txt"
            ));
            let snapshot = format!("{row}\n");
            if std::env::var_os("UPDATE_SNAPSHOTS").is_some() {
                std::fs::write(&path, &snapshot).unwrap();
            }
            assert_eq!(snapshot, std::fs::read_to_string(path).unwrap());
            assert!(viewport.event_jump.is_empty());
            for x in label.x..label.right() {
                assert!(buf[(x, y)].modifier.contains(ratatui::style::Modifier::DIM));
            }
            for (target, _) in &viewport.field_targets {
                assert!(!target.intersects(label), "width {width}: {target:?}");
            }
            let text: String = (0..width).map(|x| buf[(x, top.y)].symbol()).collect();
            assert!(!text.contains("duration"), "width {width}: {text}");
        }
    }

    #[test]
    fn background_titles_are_centered_with_optional_time_and_cjk() {
        for (title, left, right) in [("Check files", 13, 14), ("中文答题卡住", 13, 13)] {
            for time in [None, Some("35s")] {
                let area = Rect::new(0, 0, 40, 2);
                let mut buf = Buffer::empty(area);
                render_background_box(title, time, vec![Line::from("output")], area, &mut buf);
                let text: String = buf.content[..40]
                    .iter()
                    .enumerate()
                    .filter(|(x, _)| *x == 0 || buf.content[x - 1].symbol().width() < 2)
                    .map(|(_, c)| c.symbol())
                    .collect();
                let suffix = time.map(|t| format!(" {t}")).unwrap_or_default();
                assert_eq!(
                    text,
                    format!(
                        "{} {title} {}{suffix}",
                        "─".repeat(left),
                        "─".repeat(right - suffix.width())
                    ),
                    "{title} {time:?}"
                );
                assert_eq!(buf[(0, 1)].symbol(), "o");
            }
        }
    }
}

#[cfg(test)]
mod branch_input_tests {
    use super::*;
    use crate::ui::draft_controls::{Selector, SelectorKind};

    #[test]
    fn branch_input_matches_search_row_with_panel_padding_and_cursor() {
        for width in [100, 160, 200] {
            for text in ["", "feature/你好", "feature/".repeat(30).as_str()] {
                let area = Rect::new(4, 2, width, 40);
                let mut view = View::default();
                view.set_active_panel(Panel::TaskDetails);
                let mut editor = line_edit::LineEdit::default();
                editor.insert(text);
                view.selector = Some(Selector::new(
                    SelectorKind::Branch {
                        draft: crate::model::DraftKey {
                            millis: 0,
                            counter: 1,
                        },
                        branches: vec!["main".into()],
                        editor,
                        editing: true,
                    },
                    1,
                ));
                let mut buffer = Buffer::empty(area);
                view.render(area, &mut buffer);
                let rect = view.viewport.borrow().task_details_input;
                assert_eq!(rect.x, view.center_layout(area).task_details.x);
                let editor = view.selector.as_ref().unwrap().branch_editor().unwrap();
                let mut expected = Buffer::empty(rect);
                render_input_row("New branch ❯ ", editor, rect, &mut expected);
                assert_eq!(
                    view.cursor_pos(area),
                    Some((rect.x + input_row("New branch ❯ ", editor, rect).1, rect.y))
                );
                for x in rect.x..rect.right() {
                    assert_eq!(buffer[(x, rect.y)], expected[(x, rect.y)]);
                    assert_eq!(buffer[(x, rect.y)].bg, Color::Reset);
                }
            }
        }
    }
}

#[cfg(test)]
mod title_input_tests {
    use super::*;

    #[test]
    fn new_task_name_keeps_its_task_list_color_while_shown_and_edited() {
        let _guard = crate::theme::TEST_LOCK.lock().unwrap();
        let directory = tempfile::tempdir().unwrap();
        let mut view = View::new(Grouping::Project, true);
        view.set_active_panel(Panel::TaskDetails);
        view.add_draft(DraftSetup {
            provider: ProviderId::Claude,
            cwd: directory.path().to_owned(),
            model: None,
            effort: None,
            branch: String::new(),
        });
        let key = view.selected_draft().unwrap().key();
        let color = Some(view.title_color(&key));
        let rule = Rect::new(0, 0, 60, 1);
        let name_cell = |view: &View, text: &str| {
            let mut buf = Buffer::empty(rule);
            view.render_title_rule(rule, &mut buf);
            let x = (0..rule.width)
                .find(|&x| buf[(x, 0)].symbol() == &text[..1])
                .unwrap();
            buf[(x, 0)].clone()
        };
        view.rename.set_text("Draft name");
        assert_eq!(name_cell(&view, "D").fg, color.unwrap());
        assert!(view.accept_draft_name());
        assert_eq!(name_cell(&view, "D").fg, color.unwrap());
    }

    #[test]
    fn title_editor_keeps_wide_caret_inside_the_rule_and_clears_side_labels() {
        let directory = tempfile::tempdir().unwrap();
        let cwd = directory.path().join("项目");
        std::fs::create_dir(&cwd).unwrap();
        let mut view = View::default();
        view.set_active_panel(Panel::TaskDetails);
        view.add_draft(DraftSetup {
            provider: ProviderId::Claude,
            cwd,
            model: None,
            effort: None,
            branch: "main".into(),
        });
        for width in [12, 20, 40, 80] {
            for text in ["", "你好", "改 Claude Code 的颜色", &"你好".repeat(80)] {
                view.rename.set_text(text);
                for code in [KeyCode::Home, KeyCode::Right, KeyCode::End, KeyCode::Left] {
                    view.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
                    let rule = Rect::new(7, 3, width, 1);
                    let mut buf = Buffer::empty(Rect::new(5, 2, width + 4, 3));
                    view.viewport.borrow_mut().field_targets.clear();
                    view.render_title_rule(rule, &mut buf);
                    let input = view.viewport.borrow().title_input;
                    assert!(input.x >= rule.x && input.right() <= rule.right());
                    if !input.is_empty() {
                        let (visible, caret) = view.rename.window(usize::from(input.width));
                        assert!(caret < input.width);
                        if !text.is_empty() {
                            let mut expected = Buffer::empty(input);
                            let style = view.title_style(&view.selected_draft().unwrap().key());
                            line(
                                Span::styled(visible.to_owned(), style),
                                input,
                                &mut expected,
                            );
                            for x in input.x..input.right() {
                                assert_eq!(buf[(x, input.y)], expected[(x, input.y)]);
                            }
                        }
                    }
                    for (label, _) in &view.viewport.borrow().field_targets {
                        assert!(label.right() < input.x || label.x > input.right());
                    }
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "panel_frame_tests.rs"]
mod panel_frame_tests;
