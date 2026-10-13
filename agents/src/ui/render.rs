use super::formatting::{format_duration, format_relative_time, tail_lines, visible_suffix};
use super::hints::hint_line;
use super::line_truncation::truncate_line_with_ellipsis_if_overflow;
use super::*;
use ratatui::widgets::{Block, BorderType, Borders};

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
pub(super) struct CenterLayout {
    pub(super) header: Rect,
    pub(super) list: Rect,
    pub(super) gap: Rect,
    pub(super) details: Rect,
    pub(super) search: Rect,
    pub(super) footer: Rect,
    pub(super) log: Rect,
}
impl View {
    fn editor_text(&self) -> &str {
        if self.rename_target.is_some() {
            &self.rename
        } else {
            &self.search
        }
    }

    pub(super) fn center_layout(&self, area: Rect) -> CenterLayout {
        let footer_height = u16::from(area.height >= 2);
        let header_height = if area.height >= 6 { 3 } else { 0 };
        let footer = row(area, area.height - footer_height, footer_height);
        let header = row(area, 0, header_height);
        let bottom_height = ((u32::from(area.height) * 382 + 500) / 1000) as u16;
        let log_height = bottom_height.saturating_sub(footer_height).max(10);
        let log_height = if area.height.saturating_sub(footer_height + log_height)
            >= header_height + 6
            && self
                .selected_row()
                .is_some_and(|session| session.transcript.is_some())
        {
            log_height
        } else {
            0
        };
        let log = row(area, area.height - footer_height - log_height, log_height);
        let body = row(area, header_height, log.y.saturating_sub(header.bottom()))
            .inner(Margin::new(2, 0));
        let [list, gap, details] = if body.width >= 90 {
            let content = Rect {
                width: body.width - 3,
                ..body
            };
            let [list, mut details] =
                Layout::horizontal([Constraint::Ratio(618, 1000), Constraint::Ratio(382, 1000)])
                    .areas(content);
            let gap = Rect::new(list.right(), body.y, 3, body.height);
            details.x += gap.width;
            [list, gap, details]
        } else {
            [body, Rect::default(), Rect::default()]
        };
        let search = row(
            list,
            0,
            u16::from(self.searching || self.rename_target.is_some()),
        );
        let list = row(
            list,
            search.height,
            list.height.saturating_sub(search.height),
        );
        CenterLayout {
            header,
            list,
            gap,
            details,
            search,
            footer,
            log,
        }
    }
    pub(crate) fn log_event_count(&self, area: Rect) -> usize {
        usize::from(self.center_layout(area).log.height.saturating_sub(2))
    }

    pub fn cursor_pos(&self, area: Rect) -> Option<(u16, u16)> {
        let layout = self.center_layout(area);
        if !(self.searching || self.rename_target.is_some())
            || self.delete_dialog.is_some()
            || self.help
            || layout.search.is_empty()
        {
            return None;
        }
        let available = usize::from(layout.search.width).saturating_sub(10);
        Some((
            layout.search.x
                + (9 + visible_suffix(self.editor_text(), available).width())
                    .min(usize::from(layout.search.width.saturating_sub(1)))
                    as u16,
            layout.search.y,
        ))
    }
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
        self.viewport.borrow_mut().marker = None;
        self.viewport.borrow_mut().details = Rect::default();
        self.viewport.borrow_mut().log = Rect::default();
        let layout = self.center_layout(area);
        let inset = |area: Rect| area.inner(Margin::new(2, 0));
        line(
            vec![
                "Agent command center".bold(),
                format!("  Group: {}  g", self.grouping().label()).dim(),
            ],
            inset(layout.header),
            buf,
        );
        let labels = TASK_FILTERS
            .iter()
            .map(|(label, group)| {
                let count = self
                    .rows
                    .iter()
                    .filter(|s| !self.hidden.contains(&s.id) && group.contains(&s.status))
                    .count();
                format!("{label} {count}")
            })
            .collect::<Vec<_>>();
        let mut tabs = inset(row(layout.header, 1, 1));
        let filter_hint = if self.searching || self.rename_target.is_some() {
            Line::default()
        } else {
            hint_line(&[("tab/⇧tab", "filter")])
        };
        if tabs.width >= 80 && filter_hint.width() > 0 {
            let width = filter_hint.width() as u16;
            filter_hint.render(
                Rect::new(tabs.right() - width, tabs.y, width, tabs.height),
                buf,
            );
            tabs.width = tabs.width.saturating_sub(width + 2);
        }
        self.viewport.borrow_mut().tab_bar = tabs;
        self.viewport.borrow_mut().tabs = super::selection_tabs::render_filled_tab_bar(
            &labels.iter().map(String::as_str).collect::<Vec<_>>(),
            self.status_filter,
            tabs,
            buf,
        );
        line(
            "─".repeat(usize::from(area.width.saturating_sub(4))).dim(),
            inset(row(layout.header, 2, 1)),
            buf,
        );
        if self.searching || self.rename_target.is_some() {
            buf.set_style(layout.search, super::style::active_tab_style());
            let available = usize::from(layout.search.width).saturating_sub(10);
            line(
                vec![
                    if self.rename_target.is_some() {
                        "Rename › "
                    } else {
                        "Search › "
                    }
                    .cyan()
                    .bold(),
                    visible_suffix(self.editor_text(), available)
                        .to_owned()
                        .into(),
                ],
                layout.search,
                buf,
            );
        }
        if let Some(notice) = &self.notice {
            line(notice.clone().dim(), inset(layout.footer), buf);
        } else {
            line(self.center_footer_hints(), inset(layout.footer), buf);
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
        self.render_center_rows(layout.list, buf, now);
        if !layout.details.is_empty() {
            self.render_details(layout.details, buf, now);
        }
        self.viewport.borrow_mut().log = layout.log;
        self.render_log(inset(layout.log), buf, now);
        if let Some(dialog) = &self.delete_dialog {
            self.render_delete_dialog(dialog, layout.list, buf);
            self.viewport.borrow_mut().marker = None;
        }
    }
    fn render_delete_dialog(&self, dialog: &DeleteDialog, area: Rect, buf: &mut Buffer) {
        use ratatui::widgets::{Block, Borders};
        let width = area.width.min(80);
        let content_width = usize::from(width.saturating_sub(4).max(1));
        let mut lines = Vec::new();
        for text in textwrap::wrap(
            &format!("Permanently delete “{}”?", dialog.title),
            content_width,
        ) {
            lines.push(Line::from(text.into_owned().bold()));
        }
        let description = "This stops any running work in this task and its child agents, then permanently deletes their history. This cannot be undone.";
        for text in textwrap::wrap(description, content_width) {
            lines.push(Line::from(text.into_owned().dim()));
        }
        lines.push(Line::default());
        for (confirm, label) in [
            (false, "Cancel"),
            (true, "Permanently delete task and child agents"),
        ] {
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
        let block = Block::default().borders(Borders::ALL);
        let inner = block.inner(dialog).inner(Margin::new(1, 0));
        block.render(dialog, buf);
        Paragraph::new(lines).render(inner, buf);
    }
    pub(super) fn render_details(
        &self,
        area: Rect,
        buf: &mut Buffer,
        now: chrono::DateTime<chrono::Utc>,
    ) {
        self.viewport.borrow_mut().details = Rect::default();
        self.viewport.borrow_mut().details_total_rows = 0;
        let Some(session) = self.selected_row() else {
            return;
        };
        self.viewport.borrow_mut().details = area;
        let width = usize::from(area.width.max(1));
        let (status, dot) = Self::status(session);
        // Sections top to bottom: title, prompt, the newer of latest activity
        // and last message, background, needs attention, project, branch, model.
        // Whatever does not fit is cut at the bottom and marked with an ellipsis.
        let mut header = Line::from(vec![dot, " ".into(), status.into(), " · ".dim()]);
        let title_width = width.saturating_sub(header.width());
        let title_style = self.title_style(&session.id).bold();
        let title = if title_width <= 1 {
            Line::from(Span::styled("…", title_style))
        } else {
            truncate_line_with_ellipsis_if_overflow(
                Line::from(Span::styled(
                    session.display_title().to_owned(),
                    title_style,
                )),
                title_width,
            )
        };
        header.spans.extend(title.spans);
        let mut items: Vec<Item> = vec![header.into()];

        let mut header = vec!["Prompt".dim()];
        if let Some(start) = session.prompt_at {
            let end = if session.status == SessionStatus::Working {
                now
            } else {
                session.last_record_at.unwrap_or(start)
            };
            let messages = session.turn_messages;
            let label = if messages == 1 { "message" } else { "messages" };
            header.extend([
                " · ".dim(),
                format!("{messages} {label}").dim(),
                " · ".dim(),
                format_duration((end - start).num_seconds().max(0)).dim(),
            ]);
        }
        items.extend([Line::default().into(), Line::from(header).into()]);
        let prompt_text = match session.prompt.trim() {
            "" => "No prompt available.",
            prompt => prompt,
        };
        let prompt_body = super::markdown::render(prompt_text, width);
        items.extend(prompt_body.into_iter().map(Item::from));
        items.extend(session.queued.iter().map(|queued| {
            let first = super::markdown::render(queued, width)
                .into_iter()
                .next()
                .unwrap_or_default();
            truncate_line_with_ellipsis_if_overflow(first, width).into()
        }));

        // Latest activity and Last message share one slot and follow the
        // clock: whichever of the latest tool call and the last reply is newer.
        let activity = session
            .activity
            .as_ref()
            .filter(|activity| session.last_message_at.is_none_or(|at| activity.at > at));
        if let Some(activity) = activity {
            let wrapped = textwrap::wrap(&activity.label, width);
            let mut label = wrapped
                .first()
                .map_or(String::new(), |line| line.to_string());
            if wrapped.len() > 1 {
                label.push('…');
            }
            items.extend([
                Line::default().into(),
                Line::from(vec![
                    "Latest activity".dim(),
                    " · ".dim(),
                    format_relative_time(now, Some(activity.at)).dim(),
                ])
                .into(),
                truncate_line_with_ellipsis_if_overflow(
                    Line::from(Span::styled(
                        label,
                        style::log_styles(&crate::sessions::log::Kind::Tool).1,
                    )),
                    width,
                )
                .into(),
            ]);
        }
        if let Some(message) = session.last_message.as_ref().filter(|_| activity.is_none()) {
            let mut header = vec!["Last message".dim()];
            if let Some(at) = session.last_message_at {
                header.extend([" · ".dim(), format_relative_time(now, Some(at)).dim()]);
            }
            items.extend([Line::default().into(), Line::from(header).into()]);
            let body_style = style::log_styles(&crate::sessions::log::Kind::Model).1;
            items.extend(
                super::markdown::render(message, width)
                    .into_iter()
                    .map(|mut line| {
                        for span in &mut line.spans {
                            span.style = body_style.patch(span.style);
                        }
                        line
                    })
                    .map(Item::from),
            );
        }

        // Claude Code appends an exit marker to the output file when the
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
        if !boxes.is_empty() {
            items.extend([Line::default().into(), "Background".dim().into()]);
            items.extend(
                boxes
                    .into_iter()
                    .map(|(task, output)| Item::Background { task, output }),
            );
        }

        if session.status == SessionStatus::NeedsInput {
            items.extend([
                Line::default().into(),
                "Needs attention".red().into(),
                "Open task to review.".dim().into(),
                "Waiting for your response.".into(),
            ]);
        }

        let mut project = vec!["Project".dim()];
        if let Some(branch) = &session.git_branch {
            project.extend([
                " · ".dim(),
                Span::styled(branch.clone(), self.title_style(&session.id)),
            ]);
        }
        let link = session.pr.as_ref().and_then(|pr| {
            let label = format!("#{}", pr.number);
            project.push(" · ".dim());
            let prefix_width = Line::from(project.clone()).width();
            let link = line_width_fits(prefix_width + label.len(), width)
                .then(|| (prefix_width as u16, label.len() as u16, pr.url.clone()));
            project.push(Span::styled(label, self.title_style(&session.id)));
            link
        });
        items.extend([
            Line::default().into(),
            Item::Link {
                line: Line::from(project),
                link,
            },
            session.cwd.display().to_string().into(),
        ]);

        let mut model = vec!["Model: ".dim(), session.model_name().to_owned().into()];
        if let Some(effort) = &session.effort {
            model.extend([" · ".dim(), effort.clone().into()]);
        }
        items.extend([Line::default().into(), Line::from(model).into()]);

        let total = items.iter().map(Item::height).sum::<usize>();
        let available = usize::from(area.height);
        let scroll = {
            let mut viewport = self.viewport.borrow_mut();
            viewport.details_total_rows = total;
            viewport.details_scroll = viewport.details_scroll.min(total.saturating_sub(available));
            viewport.details_scroll
        };
        let content_area = Rect::new(0, 0, area.width, total as u16);
        let mut content = Buffer::empty(content_area);
        let mut offset = 0;
        for item in items {
            let height = item.height();
            let item_area = row(content_area, offset as u16, height as u16);
            match item {
                Item::Text(text) => line(text, item_area, &mut content),
                Item::Link { line: text, link } => {
                    line(text, item_area, &mut content);
                    if let Some((x, width, url)) = link {
                        super::hyperlink::hyperlink(
                            &mut content,
                            Rect::new(item_area.x + x, item_area.y, width, 1),
                            &url,
                        );
                    }
                }
                Item::Background { task, output } => {
                    render_background(task, output, now, item_area, &mut content);
                }
            }
            offset += height;
        }
        for y in 0..available.min(total - scroll) {
            for x in 0..area.width {
                buf[(area.x + x, area.y + y as u16)] = content[(x, (scroll + y) as u16)].clone();
            }
        }
        if total - scroll > available && available > 0 {
            let last = row(area, (available - 1) as u16, 1);
            Clear.render(last, buf);
            line("…".dim(), last, buf);
        }
    }
}

fn line_width_fits(line_width: usize, width: usize) -> bool {
    line_width <= width
}

/// One row-aligned piece of the details panel.
enum Item<'a> {
    Text(Line<'static>),
    Link {
        line: Line<'static>,
        /// Column offset, width and target of the pull request link.
        link: Option<(u16, u16, String)>,
    },
    Background {
        task: &'a crate::model::BackgroundTask,
        output: Vec<String>,
    },
}

impl Item<'_> {
    fn height(&self) -> usize {
        match self {
            Self::Text(_) | Self::Link { .. } => 1,
            Self::Background { output, .. } => box_height(output.len()),
        }
    }
}

impl<T: Into<Line<'static>>> From<T> for Item<'_> {
    fn from(text: T) -> Self {
        Self::Text(text.into())
    }
}

/// A running background command: its description and elapsed time on the top
/// rule, the last output line inside when there is one.
fn render_background(
    task: &crate::model::BackgroundTask,
    output: Vec<String>,
    now: chrono::DateTime<chrono::Utc>,
    box_area: Rect,
    buf: &mut Buffer,
) {
    // One space keeps the title off the corner glyph on both sides.
    let elapsed = format_duration((now - task.started_at).num_seconds().max(0));
    let description_style = if task.waits_for_codex {
        style::log_styles(&crate::sessions::log::Kind::Model).1
    } else {
        Style::default()
    };
    let mut title = truncate_line_with_ellipsis_if_overflow(
        Line::from(Span::styled(task.description.clone(), description_style)),
        usize::from(box_area.width.saturating_sub(4))
            .saturating_sub(elapsed.chars().count() + " · ".chars().count()),
    );
    title.spans.extend([" · ".dim(), elapsed.dim(), " ".into()]);
    // Without output the task is just its titled top rule, which has no
    // corner glyph, so the rule itself leads the title; half-width
    // glyphs at both ends keep it visually inside the box edges.
    let borders = if output.is_empty() {
        title.spans.insert(0, "╶ ".dim());
        Borders::TOP
    } else {
        title.spans.insert(0, " ".into());
        Borders::ALL
    };
    let block = Block::default()
        .borders(borders)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().dim())
        .title_style(Style::default().not_dim())
        .title(title);
    let inner = block.inner(box_area);
    block.render(box_area, buf);
    if output.is_empty() && box_area.width > 0 {
        buf[(box_area.right() - 1, box_area.y)].set_symbol("╴");
    }
    // One column of padding on both sides keeps output off the borders.
    let inner = Rect {
        x: inner.x + 1,
        width: inner.width.saturating_sub(2),
        ..inner
    };
    Paragraph::new(output.into_iter().map(Line::from).collect::<Vec<_>>()).render(inner, buf);
}

/// Rows a background box takes: a bare top rule without output, a full box otherwise.
fn box_height(lines: usize) -> usize {
    if lines == 0 { 1 } else { 2 + lines }
}
