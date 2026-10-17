use super::formatting::{
    format_duration, format_relative_time, format_session_duration, tail_lines,
};
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
            prompt.to_owned().cyan().bold(),
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
    let style = super::style::active_tab_style();
    buf.set_style(area, style);
    let (text, _) = input_row(prompt, edit, area);
    line(text, area, buf);
    // Wide graphemes reset their continuation cells while rendering. Apply the row fill
    // afterward as well, preserving the prompt's foreground color.
    buf.set_style(area, Style { fg: None, ..style });
}

const HORIZONTAL_PADDING: u16 = 1;

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

pub(super) struct CenterLayout {
    pub(super) header: Rect,
    pub(super) list: Rect,
    pub(super) gap: Rect,
    pub(super) details: Rect,
    pub(super) search: Rect,
    pub(super) usage_chart: Rect,
    pub(super) footer: Rect,
    pub(super) log: Rect,
    pub(super) prompt: Rect,
    pub(super) log_detail: Rect,
}

impl View {
    fn input(&self) -> (&str, &line_edit::LineEdit) {
        if self.rename_target.is_some() {
            ("Rename › ", &self.rename)
        } else {
            ("Search › ", &self.search)
        }
    }

    pub(super) fn center_layout(&self, area: Rect) -> CenterLayout {
        let footer_height = u16::from(area.height >= 2);
        let header_height = if area.height >= 6 { 2 } else { 0 };
        let footer = row(area, area.height - footer_height, footer_height);
        let header = row(area, 0, header_height);
        let wide = area.width >= WIDE_LAYOUT_COLUMNS;
        let prompt_height = self
            .input_box
            .height(
                area.width.saturating_sub(2 * HORIZONTAL_PADDING),
                &self.input_label(),
            )
            .min(area.height.saturating_sub(header_height + footer_height));
        let bottom_height = ((u32::from(area.height) * 382 + 500) / 1000) as u16;
        let log_height = bottom_height.saturating_sub(footer_height).max(10);
        let show_log = area.height.saturating_sub(footer_height + log_height) >= header_height + 6
            && (wide
                || self
                    .selected_row()
                    .is_some_and(|session| session.transcript.is_some()));
        // Exclude both rules around the prompt and, in narrow mode, the list/detail rule.
        // The detail rectangle adds its top rule back below; it is not content.
        let content_height = area
            .height
            .saturating_sub(header_height + footer_height + prompt_height + 2 + u16::from(!wide));
        let (top_height, list_height, detail_height) = if !show_log {
            (
                area.height
                    .saturating_sub(header_height + footer_height + prompt_height),
                0,
                0,
            )
        } else if wide {
            // Give an odd content row to the top; narrow remainders go top, then list.
            (content_height.div_ceil(2), 0, content_height / 2)
        } else {
            (
                content_height.div_ceil(3),
                (content_height + 1) / 3,
                content_height / 3,
            )
        };
        let body = row(area, header_height, top_height).inner(Margin::new(HORIZONTAL_PADDING, 0));
        let prompt = row(
            area,
            header_height + top_height + u16::from(show_log),
            prompt_height,
        );
        let log = if show_log && !wide {
            row(area, prompt.bottom() - area.y + 1, list_height)
                .inner(Margin::new(HORIZONTAL_PADDING, 0))
        } else {
            Rect::default()
        };
        let log_detail = if show_log {
            row(
                area,
                header_height + top_height + 1 + prompt_height + u16::from(!wide) + list_height,
                detail_height + 1,
            )
        } else {
            Rect::default()
        };
        let (body, log_list) = if wide && show_log {
            let column_width = (body.width - 6) / 4;
            let log_width = body.width - 6 - 2 * column_width;
            (
                Rect {
                    width: body.width - log_width - 3,
                    ..body
                },
                Rect::new(body.right() - log_width, body.y, log_width, body.height),
            )
        } else {
            (body, Rect::default())
        };
        let [list, gap, details] = if body.width >= 7 {
            let content = Rect {
                width: body.width - 3,
                ..body
            };
            let [list, mut details] =
                Layout::horizontal([Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)])
                    .areas(content);
            let gap = Rect::new(list.right(), body.y, 3, body.height);
            details.x += gap.width;
            [list, gap, details]
        } else {
            [body, Rect::default(), Rect::default()]
        };
        // Keep the chart's original canvas and scale when the search row takes list space.
        let mut usage_chart = list;
        let (list, search) = if self.rename_target.is_some() {
            let input = row(list, 0, 1);
            usage_chart = row(list, input.height, list.height.saturating_sub(input.height));
            (usage_chart, input)
        } else if self.searching {
            let entries = self
                .center_rows(&self.selectable_indices(), self.grouping)
                .len();
            let chart_height = self.usage_chart_height(list, entries);
            let above_chart = row(list, 0, list.height.saturating_sub(chart_height));
            let input = log::search_input_rect(above_chart);
            (
                row(
                    above_chart,
                    0,
                    above_chart.height.saturating_sub(input.height),
                ),
                input,
            )
        } else {
            (list, Rect::default())
        };
        CenterLayout {
            header,
            list,
            gap,
            details,
            search,
            usage_chart,
            footer,
            log: if wide && show_log { log_list } else { log },
            prompt,
            log_detail,
        }
    }
    pub(crate) fn log_event_count(&self, area: Rect) -> usize {
        usize::from(
            self.center_layout(area)
                .log
                .height
                .saturating_sub(u16::from(self.log_search.active())),
        )
    }

    pub fn cursor_pos(&self, area: Rect) -> Option<(u16, u16)> {
        let layout = self.center_layout(area);
        if let Some(editor) = &self.branch_editor
            && let Some((rect, _)) = self
                .viewport
                .borrow()
                .field_targets
                .iter()
                .find(|(_, field)| *field == Field::Branch)
        {
            let (_, caret) = input_row("Branch › ", &editor.editor, *rect);
            return Some((rect.x + caret, rect.y));
        }
        if self.input_box.focused && !self.help && self.confirm_dialog.is_none() {
            return self.input_box.cursor(
                layout.prompt.inner(Margin::new(HORIZONTAL_PADDING, 0)),
                &self.input_label(),
            );
        }
        if (self.log_search.editing || self.log_detail_search.editing)
            && !self.help
            && self.confirm_dialog.is_none()
            && !layout.log.is_empty()
        {
            let search_area = if self.log_detail_search.editing && !layout.log_detail.is_empty() {
                layout.log_detail.inner(Margin::new(HORIZONTAL_PADDING, 0))
            } else {
                layout.log
            };
            let input = log::search_input_rect(search_area);
            let search = if self.log_detail_search.editing {
                &self.log_detail_search
            } else {
                &self.log_search
            };
            let (_, caret) = input_row("Search › ", &search.query, input);
            return Some((input.x + caret, input.y));
        }
        if !(self.searching || self.rename_target.is_some())
            || self.confirm_dialog.is_some()
            || self.help
            || layout.search.is_empty()
        {
            return None;
        }
        let (prompt, edit) = self.input();
        let (_, caret) = input_row(prompt, edit, layout.search);
        Some((layout.search.x + caret, layout.search.y))
    }

    #[cfg(test)]
    pub fn render(&self, area: Rect, buf: &mut Buffer) {
        self.render_at(area, buf, chrono::Utc::now());
    }
    pub(crate) fn render_at(
        &self,
        area: Rect,
        buf: &mut Buffer,
        now: chrono::DateTime<chrono::Utc>,
    ) {
        Clear.render(area, buf);
        {
            // A panel this frame leaves out keeps no links from the last one.
            let mut viewport = self.viewport.borrow_mut();
            viewport.marker = None;
            viewport.field_targets.clear();
            viewport.details = Rect::default();
            viewport.log = Rect::default();
            viewport.log_borders = [Rect::default(); 2];
            viewport.log_detail = Rect::default();
            viewport.log_detail_jump = Rect::default();
            viewport.details_links.clear();
            viewport.log_links.clear();
        }
        let layout = self.center_layout(area);
        self.viewport.borrow_mut().prompt = layout.prompt;
        self.input_box.render(
            layout.prompt.inner(Margin::new(HORIZONTAL_PADDING, 0)),
            buf,
            &self.input_label(),
            self.input_disabled(),
        );
        let inset = |area: Rect| area.inner(Margin::new(HORIZONTAL_PADDING, 0));
        let labels = TASK_FILTERS
            .iter()
            .map(|(label, group)| {
                let count = self
                    .rows
                    .iter()
                    .enumerate()
                    .filter(|(index, s)| {
                        !self.hidden.contains(&s.id)
                            && self.group_index(*index).is_some_and(|parent| {
                                !self.hidden.contains(&self.rows[parent].id)
                                    && group.contains(&s.status)
                            })
                    })
                    .count();
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
        rule(row(layout.header, 1, 1), buf);

        line(self.center_footer_hints(), inset(layout.footer), buf);
        if let Some((label, _)) = &self.copied {
            Line::from(label.clone().dim())
                .right_aligned()
                .render(inset(layout.footer), buf);
        }
        if self.help {
            let body = inset(row(
                area,
                layout.header.height,
                layout.footer.y.saturating_sub(layout.header.bottom()),
            ));
            Clear.render(body, buf);
            let mut lines = self.center_help_lines(body.width);
            if lines.len() > usize::from(body.height) {
                lines.truncate(usize::from(body.height.saturating_sub(1)));
                lines.push("… resize to see all".dim().into());
            }
            Paragraph::new(lines).render(body, buf);
            return;
        }
        for y in layout.gap.y..layout.gap.bottom() {
            if layout.gap.width > 1 {
                buf[(layout.gap.x + 1, y)]
                    .set_symbol("│")
                    .set_style(Style::default().dim());
            }
        }
        if area.width >= WIDE_LAYOUT_COLUMNS && !layout.log.is_empty() {
            for y in layout.log.y..layout.log.bottom() {
                buf[(layout.log.x.saturating_sub(2), y)]
                    .set_symbol("│")
                    .set_style(Style::default().dim());
            }
        }
        self.render_center_rows(layout.list, buf, now);
        if self.searching || self.rename_target.is_some() {
            let (prompt, edit) = self.input();
            render_input_row(prompt, edit, layout.search, buf);
        }
        self.render_usage_chart(layout.usage_chart, buf, now);
        if let Some((notice, _)) = &self.notice
            && !layout.list.is_empty()
        {
            let bottom = row(layout.list, layout.list.height - 1, 1);
            Clear.render(bottom, buf);
            line(notice.clone().dim(), bottom, buf);
        }
        // Activity and task status share the rule immediately above the prompt.
        let middle = row(area, layout.prompt.y.saturating_sub(area.y + 1), 1);
        if !layout.log_detail.is_empty() {
            rule(middle, buf);
        }
        let mut details = layout.details;
        let details_rule = if details.is_empty() {
            None
        } else if !layout.log_detail.is_empty() {
            Some(Rect::new(details.x, middle.y, details.width, 1))
        } else {
            details.height = details.height.saturating_sub(1);
            Some(row(layout.details, details.height, 1))
        };
        if !details.is_empty() {
            self.render_details(details, buf, now);
        }
        {
            let mut viewport = self.viewport.borrow_mut();
            viewport.log = layout.log;
            viewport.log_detail = inset(layout.log_detail);
        }
        self.render_log(layout.log, buf, now);
        if !layout.log_detail.is_empty() {
            rule(row(layout.log_detail, 0, 1), buf);
            self.render_log_detail(inset(layout.log_detail), buf, now);
        }
        if !layout.log.is_empty() {
            self.viewport.borrow_mut().log_borders = [
                Rect::new(
                    layout.log.x,
                    layout.log.y.saturating_sub(1),
                    layout.log.width,
                    1,
                ),
                Rect::new(layout.log.x, layout.log.bottom(), layout.log.width, 1),
            ];
            let jump_y = if area.width >= WIDE_LAYOUT_COLUMNS {
                layout.log.bottom()
            } else {
                let border = row(area, layout.prompt.bottom() - area.y, 1);
                rule(border, buf);
                border.y
            };
            let log_rule = Rect::new(layout.log.x, jump_y, layout.log.width, 1);
            // The model and effort sit between the separator's joint, or the screen's left
            // edge in the narrow layout, and the screen's right edge.
            let left = if area.width >= WIDE_LAYOUT_COLUMNS {
                layout.log.x.saturating_sub(2)
            } else {
                area.x
            };
            self.render_log_model(
                Rect::new(left, jump_y, area.right().saturating_sub(left), 1),
                buf,
            );
            self.render_log_jump(log_rule, buf);
            let rule = Rect::new(layout.list.x, middle.y, layout.list.width, 1);
            self.render_activity_strip(rule, buf, now);
        }
        if let Some(details_rule) = details_rule {
            // The title and status rules run from the separator's joint on the left to the log's
            // joint or the screen's right edge.
            let left = layout.gap.x + 1;
            let right = if area.width >= WIDE_LAYOUT_COLUMNS && !layout.log.is_empty() {
                layout.log.x - 1
            } else {
                area.right()
            };
            if layout.header.height >= 2 {
                self.render_details_title(
                    Rect::new(left, layout.header.y + 1, right - left, 1),
                    buf,
                    now,
                );
            }
            self.render_details_status(Rect::new(left, details_rule.y, right - left, 1), buf);
        }
        let mut separators = vec![];
        if layout.gap.width > 1 {
            separators.push((layout.gap.x + 1, layout.gap.y, layout.gap.bottom()));
        }
        if area.width >= WIDE_LAYOUT_COLUMNS && !layout.log.is_empty() {
            separators.push((
                layout.log.x.saturating_sub(2),
                layout.log.y,
                layout.log.bottom(),
            ));
        }
        join_separators(&separators, area, buf);
        if self.log_selected.is_some() && self.viewport.borrow().log_detail_scroll > 0 {
            let label = " ↑ Top ";
            let jump =
                super::log::log_content_centered(inset(layout.log_detail), label.width() as u16);
            Line::from(Span::styled(label, super::style::log_jump_style())).render(jump, buf);
            self.viewport.borrow_mut().log_detail_jump = jump;
        }
        self.render_controls(details, buf);
        if let Some(dialog) = &self.confirm_dialog {
            self.render_confirm_dialog(dialog, layout.list, buf);
            self.viewport.borrow_mut().marker = None;
        }
    }
    fn render_confirm_dialog(&self, dialog: &ConfirmDialog, area: Rect, buf: &mut Buffer) {
        use ratatui::widgets::{Block, Borders};
        let width = area.width.min(80);
        let content_width = usize::from(width.saturating_sub(4).max(1));
        let (title, description, label) = match dialog.removal {
            Removal::Archive => (
                format!("Archive “{}”?", dialog.title),
                "This stops any running work in this task and its child agents, then archives them. Their history can be restored from the resume picker.",
                "Archive task and child agents",
            ),
            Removal::Delete => (
                format!("Permanently delete “{}”?", dialog.title),
                "This stops any running work in this task and its child agents, then permanently deletes their history. This cannot be undone.",
                "Permanently delete task and child agents",
            ),
        };
        let description = if matches!(dialog.removal, Removal::Delete)
            && self
                .rows
                .iter()
                .any(|session| session.id == dialog.id && session.background_job.is_some())
        {
            format!("{description} Worktrees and branches created by agents remain.")
        } else {
            description.to_owned()
        };
        let mut lines = Vec::new();
        for text in textwrap::wrap(&title, content_width) {
            lines.push(Line::from(text.into_owned().bold()));
        }
        for text in textwrap::wrap(&description, content_width) {
            lines.push(Line::from(text.into_owned().dim()));
        }
        lines.push(Line::default());
        for (confirm, label) in [(false, "Cancel"), (true, label)] {
            let selected = dialog.confirm == confirm;
            let text = format!(
                "{} {}. {label}",
                if selected { "›" } else { " " },
                if confirm { 2 } else { 1 }
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
        Clear.render(dialog, buf);
        super::links::cover(&mut self.viewport.borrow_mut().list_links, dialog);
        let block = Block::default().borders(Borders::ALL);
        let inner = block.inner(dialog).inner(Margin::new(1, 0));
        block.render(dialog, buf);
        Paragraph::new(lines).render(inner, buf);
    }
    /// Centers the selected task's title on the rule row `rule`, whose end cells are its
    /// corners, a space apart from the rule on each side, with the elapsed time at its right end.
    fn render_details_title(
        &self,
        rule: Rect,
        buf: &mut Buffer,
        now: chrono::DateTime<chrono::Utc>,
    ) {
        let Some(session) = self.selected_row() else {
            return;
        };
        self::rule(rule, buf);
        // The title stays centered on the whole rule, so the time's width is kept clear on both sides.
        let mut reserved = 0;
        if let Some(at) = session.started_at {
            let elapsed = format_session_duration((now - at).num_seconds());
            // Keep a title cell, its spaces and the rule cells and corners around it.
            if usize::from(rule.width) >= 2 * (elapsed.width() + 2) + 7 {
                let (_, area) = render_rule_ends(rule, 0, None, Some(elapsed.dim()), buf);
                reserved = area.map_or(0, |area| 2 * usize::from(area.width + 2));
            }
        }
        // The corner, one rule cell and a space on each side.
        let width = usize::from(rule.width).saturating_sub(6 + reserved);
        let title_style = self.title_style(&session.id).bold();
        let title = if width <= 1 {
            Line::from(Span::styled("…", title_style))
        } else {
            truncate_line_with_ellipsis_if_overflow(
                Line::from(Span::styled(
                    session.display_title().to_owned(),
                    title_style,
                )),
                width,
            )
        };
        let mut header = truncate_line_to_width(title, width);
        header.spans.insert(0, " ".into());
        header.spans.push(" ".into());
        let label_width = header.width() as u16;
        let x = rule.x + rule.width.saturating_sub(label_width) / 2;
        let label = Rect::new(x, rule.y, label_width.min(rule.width), 1).intersection(rule);
        Clear.render(label, buf);
        line(header, label, buf);
    }
    /// Centers the bold status on the rule between its two corner cells, a space apart on each
    /// side.
    fn render_details_status(&self, rule: Rect, buf: &mut Buffer) {
        let Some(session) = self.selected_row() else {
            return;
        };
        // The left corner is a joint, or the separator that keeps going down beside the prompt.
        self::rule(
            Rect::new(rule.x + 1, rule.y, rule.width.saturating_sub(1), 1),
            buf,
        );
        if buf[(rule.x, rule.y)].symbol() == "│" {
            buf[(rule.x, rule.y)].set_symbol("├");
        }
        let (status, _) = Self::status(session);
        let width = usize::from(rule.width).saturating_sub(4);
        let mut status = truncate_line_with_ellipsis_if_overflow(Line::from(status).bold(), width);
        status.spans.insert(0, " ".into());
        status.spans.push(" ".into());
        let status_width = (status.width() as u16).min(rule.width);
        let status_area = Rect::new(
            rule.x + rule.width.saturating_sub(status_width) / 2,
            rule.y,
            status_width,
            1,
        );
        Clear.render(status_area, buf);
        line(status, status_area, buf);
        // The pull request on the left and the project folder's name on the right flank the
        // status.
        let folder = session.cwd.file_name().map_or_else(
            || session.cwd.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        );
        let pr = session
            .pr
            .as_ref()
            .map(|pr| Span::styled(format!("#{}", pr.number), self.title_style(&session.id)));
        let (pr_area, folder_area) = render_rule_ends(
            rule,
            status_width,
            pr,
            Some(Span::styled(folder, style::time_style())),
            buf,
        );
        let mut viewport = self.viewport.borrow_mut();
        if let Some(area) = folder_area {
            viewport.details_links.push((area, session.cwd.clone()));
        }
        if let (Some(area), Some(pr)) = (pr_area, &session.pr) {
            let url = super::vscode::pull_request_link(&pr.url, &session.cwd)
                .unwrap_or_else(|| pr.url.clone());
            super::hyperlink::hyperlink(buf, area, &url);
            viewport.details_links.push((area, PathBuf::from(url)));
        }
    }

    /// Writes the selected session's model on the left of the log's border and its effort on
    /// the right; both open their pickers when clicked.
    fn render_log_model(&self, rule: Rect, buf: &mut Buffer) {
        let Some(session) = self.selected_row() else {
            return;
        };
        let effort = match session.effort.as_deref() {
            Some(effort) => Some(effort),
            None if session.is_live_background()
                && session.input_disabled_reason().is_none()
                && self
                    .input_choices
                    .get(&session.id.provider)
                    .is_some_and(|choices| !choices.efforts.is_empty()) =>
            {
                Some("Unknown")
            }
            None => None,
        };
        let (model_area, effort_area) = render_rule_ends(
            rule,
            0,
            Some(Span::styled(
                session.model_label().to_owned(),
                style::time_style(),
            )),
            effort.map(|effort| Span::styled(effort.to_owned(), style::time_style())),
            buf,
        );
        let mut viewport = self.viewport.borrow_mut();
        for (area, field) in [(model_area, Field::Model), (effort_area, Field::Effort)] {
            if let Some(area) = area {
                viewport.field_targets.push((area, field));
            }
        }
    }
    pub(super) fn render_details(
        &self,
        area: Rect,
        buf: &mut Buffer,
        now: chrono::DateTime<chrono::Utc>,
    ) {
        {
            let mut viewport = self.viewport.borrow_mut();
            viewport.details = Rect::default();
            viewport.details_copy.clear();
            viewport.details_background.clear();
            viewport.details_links.clear();
            viewport.details_text = text_selection::TextRegion::default();
        }
        if let Some(draft) = self.selected_draft() {
            self.viewport.borrow_mut().details = area;
            let lines = [
                "New task".to_owned(),
                draft.setup.cwd.display().to_string(),
                format!(
                    "Model: {}",
                    draft.setup.model.as_deref().unwrap_or("Default")
                ),
                format!(
                    "Effort: {}",
                    draft.setup.effort.as_deref().unwrap_or("Default")
                ),
                format!(
                    "Branch: {}",
                    if draft.setup.branch.is_empty() {
                        "—"
                    } else {
                        &draft.setup.branch
                    }
                ),
            ];
            for (index, text) in lines.into_iter().enumerate() {
                line(text, row(area, index as u16, 1), buf);
            }
            for (index, field) in [(2, Field::Model), (3, Field::Effort), (4, Field::Branch)] {
                if field == Field::Branch && draft.setup.branch.is_empty() {
                    continue;
                }
                let rect = row(area, index, 1);
                if !rect.is_empty() {
                    self.viewport.borrow_mut().field_targets.push((rect, field));
                }
            }
            if draft.working || draft.job.is_some() {
                line("Creating…", row(area, 5, 1), buf);
            }
            if let Some(error) = &draft.error {
                line(error.clone().red(), row(area, 6, 1), buf);
            }
            return;
        }
        let Some(session) = self.selected_row() else {
            return;
        };
        self.viewport.borrow_mut().details = area;
        let width = usize::from(area.width.max(1));
        // Sections top to bottom: activity, background, message, prompt,
        // needs attention, tasks. The title
        // sits on the rule above the panel.
        let content_top = area.y;
        let mut items: Vec<Item> = Vec::new();
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

        let activity = session.activity.as_ref();
        if let Some(activity) = activity {
            let wrapped = wrap_details(&activity.label, &session.cwd, width, false);
            let mut label = wrapped
                .first()
                .map_or(String::new(), |line| line.to_string());
            if wrapped.len() > 1 {
                label.push('…');
            }
            separate_section(&mut items);
            let start = items.len() + 1;
            items.extend([
                centered_header(
                    "Latest activity".dim().into(),
                    Span::styled(
                        format_relative_time(now, Some(activity.at)),
                        style::time_style(),
                    )
                    .into(),
                    width,
                )
                .into(),
                truncate_line_with_ellipsis_if_overflow(
                    Line::from(Span::styled(label, style::log_styles(&activity.kind).1)),
                    width,
                )
                .into(),
            ]);
            bodies.push(start..items.len());
            keys.push(body_key("Latest activity", &activity.label));
            copies.push(Some((activity.label.as_str(), "Copied latest activity")));
            body_links.push(super::links::text_links(&activity.label, &session.cwd, &[]));
        }
        // The agent appends an exit marker to the output file when the
        // command ends, which lands before the completion notification does.
        let boxes = session
            .background
            .iter()
            .map(|task| (task, tail_lines(&task.output, 1)))
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
            items.push(centered_header("Background".dim().into(), Line::default(), width).into());
            let start = items.len();
            items.extend(
                boxes
                    .into_iter()
                    .map(|(task, output)| Item::Background { task, output }),
            );
            items.extend(subagents.into_iter().map(Item::Subagent));
            // Tasks showing output come first; each group keeps its order.
            items[start..].sort_by_key(|item| item.height() == 1);
            background = start..items.len();
        }

        item_links.resize(items.len(), Vec::new());

        if let Some(message) = session.last_message.as_ref() {
            let mut header = Line::default();
            if let Some(at) = session.last_message_at {
                header =
                    Span::styled(format_relative_time(now, Some(at)), style::time_style()).into();
            }
            separate_section(&mut items);
            items.push(centered_header("Update".dim().into(), header, width).into());
            let start = items.len();
            let body_style = style::log_styles(&crate::log::Kind::Assistant).1;
            items.extend(
                wrap_details(message, &session.cwd, width, true)
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

        let prompt_header = if let Some(start) = session.prompt_at {
            let end = if session.status == SessionStatus::Working {
                now
            } else {
                session.last_record_at.unwrap_or(start)
            };
            centered_header(
                "Prompt".dim().into(),
                format_duration((end - start).num_seconds().max(0))
                    .dim()
                    .into(),
                width,
            )
        } else {
            centered_header("Prompt".dim().into(), Line::default(), width)
        };
        separate_section(&mut items);
        items.push(prompt_header.into());
        let prompt_text = match session.prompt.trim() {
            "" => "No prompt available.",
            prompt => prompt,
        };
        let prompt_start = items.len();
        let prompt_body = wrap_details(prompt_text, &session.cwd, width, true);
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
        for queued in &session.queued {
            let first = wrap_details(queued, &session.cwd, width, true)
                .into_iter()
                .next()
                .unwrap_or_default();
            let visible = if first.width() > width {
                width - 1
            } else {
                width
            } as u16;
            let mut cells =
                super::text_selection_layout::map_lines(queued, std::slice::from_ref(&first), true)
                    .pop()
                    .unwrap_or_default();
            cells.retain(|(columns, _)| columns.end <= visible);
            item_links.push(super::links::row_links(
                &cells,
                &super::links::text_links(queued, &session.cwd, &[]),
            ));
            items.push(truncate_line_with_ellipsis_if_overflow(first, width).into());
        }

        item_links.resize(items.len(), Vec::new());

        let needs = session
            .background_job
            .as_ref()
            .and_then(|job| job.needs.as_deref());
        if session.status == SessionStatus::NeedsInput || needs.is_some() {
            separate_section(&mut items);
            let start = items.len();
            items.push(
                centered_header(
                    Span::styled("Needs attention", style::attention_title_style()).into(),
                    Line::default(),
                    width,
                )
                .into(),
            );
            if let Some(needs) = needs {
                items.extend(
                    textwrap::wrap(needs, width.max(1))
                        .into_iter()
                        .map(|text| Line::from(text.into_owned()).into()),
                );
            } else if session.pending_questions.is_empty() {
                items.extend([
                    "Open task to review.".dim().into(),
                    "Waiting for your response.".into(),
                ]);
            } else {
                items.extend(session.pending_questions.iter().map(|question| {
                    truncate_line_with_ellipsis_if_overflow(
                        Line::from(Span::styled(
                            question.clone(),
                            style::log_styles(&crate::log::Kind::Assistant).1,
                        )),
                        width,
                    )
                    .into()
                }));
            }
            bodies.push(start..items.len());
            keys.push(body_key(
                "Needs attention",
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
            separate_section(&mut items);
            let in_progress = tasks
                .iter()
                .filter(|(_, status)| *status == SessionStatus::Working)
                .count();
            items.push(
                centered_header(
                    "Tasks".dim().into(),
                    format!("{in_progress}/{}", tasks.len()).dim().into(),
                    width,
                )
                .into(),
            );
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
            let markdown = !matches!(*label, "Copied latest activity" | "Copied tasks");
            let maps = super::text_selection_layout::map_lines(source, &lines, markdown);
            for (index, cells) in body.clone().zip(maps) {
                item_links[index] = super::links::row_links(&cells, &body_links[id]);
                text_rows[index] = Some(text_selection::DrawnRow {
                    segment: id,
                    x: area.x,
                    y: 0,
                    cells,
                });
            }
            sources.push(text_selection::Source {
                id,
                text: (*source).into(),
                label: (*label).to_owned(),
            });
        }
        self.viewport.borrow_mut().details_text = text_selection::TextRegion {
            area,
            sources,
            rows: Vec::new(),
        };
        let available = usize::from(area.height);
        let budget = budget_details(&items, &bodies, background, available);
        let (scroll, body_rows) = {
            let mut viewport = self.viewport.borrow_mut();
            let state = &mut viewport.details_scroll;
            state.content_top = content_top;
            // Back on a task, a body whose text changed while the user was away opens folded,
            // and the plain offset starts over.
            if let Some(left) = state.left_keys.take()
                && left != keys
            {
                state.reveal = keys
                    .iter()
                    .map(|key| {
                        left.iter()
                            .position(|old| old == key)
                            .and_then(|index| state.reveal.get(index).copied())
                            .unwrap_or(0)
                    })
                    .collect();
                state.offset = 0;
            }
            state.keys = keys;
            if state.reveal.len() != bodies.len() {
                state.reveal = vec![0; bodies.len()];
            }
            if let Some(budget) = &budget {
                state.fallback_max = None;
                state.offset = 0;
                state.hidden = budget.bodies.iter().map(|&(_, hidden)| hidden).collect();
                for (reveal, hidden) in state.reveal.iter_mut().zip(&state.hidden) {
                    *reveal = (*reveal).min(*hidden);
                }
                let mut keep = vec![true; items.len()];
                if let Some(cut) = &budget.background_cut {
                    items[cut.start] = "…".dim().into();
                    keep[cut.start + 1..cut.end].fill(false);
                }
                for (i, body) in bodies.iter().enumerate() {
                    let rows = budget.bodies[i].0 + state.reveal[i];
                    keep[body.start + rows..body.end].fill(false);
                    if rows > 0 && state.reveal[i] < state.hidden[i] {
                        let Item::Text(last) = &mut items[body.start + rows - 1] else {
                            unreachable!("details bodies contain text rows");
                        };
                        *last =
                            truncate_line_to_width(std::mem::take(last), width.saturating_sub(1));
                        last.spans.push("…".dim());
                        if let Some(mapped) = &mut text_rows[body.start + rows - 1] {
                            mapped
                                .cells
                                .retain(|(cells, _)| cells.end <= width.saturating_sub(1) as u16);
                        }
                        item_links[body.start + rows - 1]
                            .retain(|(cells, _)| cells.end <= width.saturating_sub(1) as u16);
                    }
                }
                state.bodies = bodies
                    .iter()
                    .enumerate()
                    .map(|(i, body)| {
                        let start = items[..body.start]
                            .iter()
                            .zip(&keep)
                            .filter(|(_, keep)| **keep)
                            .map(|(item, _)| item.height())
                            .sum::<usize>();
                        start..start + budget.bodies[i].0 + state.reveal[i]
                    })
                    .collect();
                let mut index = 0;
                items.retain(|_| {
                    let retain = keep[index];
                    index += 1;
                    retain
                });
                let mut index = 0;
                text_rows.retain(|_| {
                    let retain = keep[index];
                    index += 1;
                    retain
                });
                let mut index = 0;
                item_links.retain(|_| {
                    let retain = keep[index];
                    index += 1;
                    retain
                });
                (state.reveal.iter().sum::<usize>(), state.bodies.clone())
            } else {
                state.hidden.clear();
                state.bodies.clear();
                let total = items.iter().map(Item::height).sum::<usize>();
                let max = total.saturating_sub(available);
                state.fallback_max = Some(max);
                state.offset = state.offset.min(max);
                let row_of = |index: usize| items[..index].iter().map(Item::height).sum::<usize>();
                let rows = bodies
                    .iter()
                    .map(|body| row_of(body.start)..row_of(body.end))
                    .collect();
                (state.offset, rows)
            }
        };
        self.viewport.borrow_mut().details_copy = body_rows
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
        let total = items.iter().map(Item::height).sum::<usize>();
        let content_area = Rect::new(0, 0, area.width, total as u16);
        let mut content = Buffer::empty(content_area);
        let mut offset = 0;
        let mut drawn_links = Vec::new();
        for ((item, mapped), links) in items.into_iter().zip(text_rows).zip(item_links) {
            if offset >= scroll && offset < scroll + available {
                let y = area.y + (offset - scroll) as u16;
                if let Some(mut mapped) = mapped {
                    mapped.y = y;
                    self.viewport.borrow_mut().details_text.rows.push(mapped);
                }
                drawn_links.extend(links.into_iter().map(|(columns, target)| {
                    (
                        Rect::new(area.x + columns.start, y, columns.end - columns.start, 1),
                        target,
                    )
                }));
            }
            let height = item.height();
            let item_area = row(content_area, offset as u16, height as u16);
            match item {
                Item::Text(text) => line(text, item_area, &mut content),
                Item::Background { task, output } => {
                    let start = offset.max(scroll);
                    let end = (offset + height).min(scroll + available);
                    if start < end {
                        self.viewport.borrow_mut().details_background.push((
                            content_top + (start - scroll) as u16
                                ..content_top + (end - scroll) as u16,
                            task.clone(),
                        ));
                    }
                    render_background(task, output, now, item_area, &mut content);
                }
                Item::Subagent(child) => {
                    let (text, kind) = subagent_line(child);
                    let body = (!text.is_empty()).then(|| {
                        truncate_line_with_ellipsis_if_overflow(
                            Line::from(Span::styled(text.to_owned(), style::log_styles(&kind).1)),
                            usize::from(item_area.width.saturating_sub(2)),
                        )
                    });
                    let elapsed = child
                        .started_at
                        .map(|at| format_duration((now - at).num_seconds().max(0)));
                    render_background_box(
                        &child.title,
                        elapsed.as_deref(),
                        body.into_iter().collect(),
                        item_area,
                        &mut content,
                    );
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
                .details_text
                .rows
                .retain(|row| row.y != last.y);
            drawn_links.retain(|(rect, _)| rect.y != last.y);
            self.viewport
                .borrow_mut()
                .field_targets
                .retain(|(rect, _)| !rect.intersects(last));
        }
        self.viewport.borrow_mut().details_links = drawn_links;
        self.highlight_text_selection(text_selection::Panel::Details, buf);
    }
}

/// Puts a blank row between a section and the one above it.
fn separate_section(items: &mut Vec<Item<'_>>) {
    if !items.is_empty() {
        items.push(Line::default().into());
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

struct DetailsBudget {
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

fn budget_details(
    items: &[Item<'_>],
    bodies: &[std::ops::Range<usize>],
    background: std::ops::Range<usize>,
    available: usize,
) -> Option<DetailsBudget> {
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
    Some(DetailsBudget {
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

/// Keeps the label intact before fitting a trailing group into the remaining cells.
/// Returns the group's column offset for links drawn inside it.
fn left_right(label: Line<'static>, group: Line<'static>, width: usize) -> (Line<'static>, usize) {
    let label = truncate_line_with_ellipsis_if_overflow(label, width);
    if group.width() == 0 {
        return (label, width);
    }
    let group =
        truncate_line_with_ellipsis_if_overflow(group, width.saturating_sub(label.width() + 1));
    let start = width - group.width();
    let mut spans = label.spans;
    let used: usize = spans.iter().map(Span::width).sum();
    spans.push(" ".repeat(start - used).dim());
    spans.extend(group.spans);
    (Line::from(spans), start)
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

/// Draws a dim rule across `area`'s first row.
fn rule(area: Rect, buf: &mut Buffer) {
    line("─".repeat(usize::from(area.width)).dim(), area, buf);
}

/// Draws end labels with a half line toward their corner and a space toward the center,
/// keeping clear the `center` columns in the rule's middle. Returns where each text landed.
fn render_rule_ends(
    rule: Rect,
    center: u16,
    left: Option<Span<'static>>,
    right: Option<Span<'static>>,
    buf: &mut Buffer,
) -> (Option<Rect>, Option<Rect>) {
    // Each side keeps its corner cell, a space on both sides of its text and a rule cell before
    // the center.
    let side = usize::from(rule.width.saturating_sub(center) / 2).saturating_sub(4);
    let mut place = |text: Option<Span<'static>>, from_left: bool| {
        let style = text.as_ref()?.style;
        let text = truncate_line_with_ellipsis_if_overflow(Line::from(text?), side);
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
        let mut text = text;
        for span in &mut text.spans {
            span.style = style;
        }
        text.render(area, buf);
        Some(area)
    };
    (place(left, true), place(right, false))
}

/// One row-aligned piece of the details panel.
enum Item<'a> {
    Text(Line<'static>),
    Subagent(&'a Session),
    Background {
        task: &'a crate::model::BackgroundTask,
        output: Vec<String>,
    },
}

impl Item<'_> {
    fn height(&self) -> usize {
        match self {
            Self::Text(_) => 1,
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

/// The first line of a working subagent's current tool call, or of its last
/// message when it is between calls, with the log kind that styles it.
fn subagent_line(child: &Session) -> (&str, crate::log::Kind) {
    let activity = child
        .latest_activity()
        .filter(|_| child.status == SessionStatus::Working);
    let (text, kind) = if let Some(activity) = activity {
        (Some(activity.label.as_str()), activity.kind.clone())
    } else {
        (child.last_message.as_deref(), crate::log::Kind::Assistant)
    };
    (
        text.unwrap_or_default().lines().next().unwrap_or_default(),
        kind,
    )
}

/// A running background command: its description and elapsed time on the top
/// rule, the last output line below it when there is one.
fn render_background(
    task: &crate::model::BackgroundTask,
    output: Vec<String>,
    now: chrono::DateTime<chrono::Utc>,
    box_area: Rect,
    buf: &mut Buffer,
) {
    let elapsed = format_duration((now - task.started_at).num_seconds().max(0));
    render_background_box(
        &task.description,
        Some(&elapsed),
        output.into_iter().map(Line::from).collect(),
        box_area,
        buf,
    );
}

fn render_background_box(
    description: &str,
    elapsed: Option<&str>,
    output: Vec<Line<'static>>,
    box_area: Rect,
    buf: &mut Buffer,
) {
    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().dim());
    let inner = block.inner(box_area);
    block.render(box_area, buf);
    let label = Line::from(vec!["─ ".dim(), description.to_owned().not_dim()]);
    let time = elapsed
        .map(|text| Line::from(text.to_owned().dim()))
        .unwrap_or_default();
    let label = truncate_line_with_ellipsis_if_overflow(
        label,
        usize::from(box_area.width).saturating_sub(time.width() + 1),
    );
    let (title, _) = left_right(label, time, usize::from(box_area.width));
    if elapsed.is_some() {
        line(title, row(box_area, 0, 1), buf);
    } else {
        line(
            super::title_rule::title_rule(description, usize::from(box_area.width), None),
            row(box_area, 0, 1),
            buf,
        );
        if !box_area.is_empty() {
            buf[(box_area.right() - 1, box_area.y)].set_symbol("╴");
        }
    }
    // Output lines up with the title text under the rule.
    let inner = Rect {
        x: inner.x + 2,
        width: inner.width.saturating_sub(2),
        ..inner
    };
    Paragraph::new(output).render(inner, buf);
}

/// Rows a background task takes: its titled rule and the output lines below.
fn box_height(lines: usize) -> usize {
    1 + lines
}

fn wrap_details(
    text: &str,
    cwd: &std::path::Path,
    width: usize,
    markdown: bool,
) -> Vec<Line<'static>> {
    let paths = super::paths::text_paths(text, cwd);
    let text = super::line_truncation::abbreviate_paths_for_wrap(text, &paths, width);
    if markdown {
        super::markdown::render_with_options(
            &text,
            super::line_truncation::prose_wrap_options(width),
        )
    } else {
        textwrap::wrap(&text, super::line_truncation::prose_wrap_options(width))
            .into_iter()
            .map(|line| Line::from(line.into_owned()))
            .collect()
    }
}

#[cfg(test)]
mod wrapped_path_tests {
    use super::*;

    const MESSAGE: &str = "报告已写到 /private/tmp/claude-501/-Users-satoshi-Code-blockchainian-claude/0ed4978e-9ece-4b9d-9bc5-04bf44fedacb/scratchpad/background-research/report.md，并已发给 team-lead。";

    #[test]
    fn wrapped_details_keep_fitting_paths_and_markdown_styles() {
        for markdown in [false, true] {
            let lines = wrap_details(
                "some prose before /tmp/fits.md and after",
                std::path::Path::new("/"),
                20,
                markdown,
            );
            assert!(
                lines
                    .iter()
                    .any(|line| line.to_string().contains("/tmp/fits.md"))
            );
        }
        let lines = wrap_details(
            "**/private/workspace/reports/report.md**",
            std::path::Path::new("/"),
            20,
            true,
        );
        assert_eq!(lines[0].to_string(), "/p…/w…/r…/report.md");
        assert!(lines[0].spans.iter().all(|span| {
            span.style
                .add_modifier
                .contains(ratatui::style::Modifier::BOLD)
        }));
    }

    #[test]
    fn details_last_message_and_prompt_show_abbreviated_screenshot_path() {
        let now = chrono::Utc::now();
        let session = Session {
            id: crate::providers::SessionKey {
                provider: crate::providers::ProviderId::Claude,
                id: "path-probe".into(),
            },
            parent: None,
            cwd: "/tmp/project".into(),
            name: None,
            title: "Path probe".into(),
            prompt: MESSAGE.into(),
            detail_sources: Default::default(),
            prompt_images: Vec::new(),
            queued: Vec::new(),
            prompt_at: None,
            turn_messages: 0,
            last_record_at: None,
            model_label: None,
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
            pending_questions: Vec::new(),
            background_job: None,
            background: Vec::new(),
            tasks: Vec::new(),
            last_prompt: None,
            transcript: None,
        };
        let mut view = View::new(Grouping::Project, false);
        view.set_sessions(vec![session], false);
        let area = Rect::new(0, 0, 40, 30);
        let mut buf = Buffer::empty(area);
        view.render_details(area, &mut buf, now);
        let rows: Vec<String> = buf
            .content
            .chunks(40)
            .map(|row| row.iter().map(|cell| cell.symbol()).collect())
            .collect();
        assert_eq!(
            rows.iter()
                .filter(|row| row.contains("/p…/t…/c…/-…/0…/s…/b…/report.md"))
                .count(),
            2
        );
        assert!(rows.iter().any(|row| row.contains("Update")));
        assert!(!rows.iter().any(|row| row.contains("/private/")));
    }

    #[test]
    fn wrapped_details_abbreviate_screenshot_path_at_narrow_width() {
        let lines = wrap_details(MESSAGE, std::path::Path::new("/"), 40, true);
        let plain: Vec<_> = lines.iter().map(ToString::to_string).collect();
        assert_eq!(
            plain,
            [
                "报告已写到",
                "/p…/t…/c…/-…/0…/s…/b…/report.md，并已发",
                "给 team-lead。",
            ]
        );
        assert!(lines.iter().all(|line| line.width() <= 40));
    }
}
