use super::formatting::{format_duration, format_relative_time, visible_suffix};
use super::hints::hint_line;
use super::line_truncation::truncate_line_with_ellipsis_if_overflow;
use super::*;

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
struct CenterLayout {
    header: Rect,
    list: Rect,
    gap: Rect,
    details: Rect,
    search: Rect,
    footer: Rect,
}
impl View {
    fn editor_text(&self) -> &str {
        if self.rename_target.is_some() {
            &self.rename
        } else {
            &self.search
        }
    }

    fn center_layout(&self, area: Rect) -> CenterLayout {
        let footer_height = u16::from(area.height >= 2);
        let header_height = if area.height >= 6 { 3 } else { 0 };
        let footer = row(area, area.height - footer_height, footer_height);
        let header = row(area, 0, header_height);
        let body = row(
            area,
            header_height,
            footer.y.saturating_sub(header.bottom()),
        )
        .inner(Margin::new(2, 0));
        let [list, gap, details] = if body.width >= 90 {
            Layout::horizontal([
                Constraint::Min(46),
                Constraint::Length(3),
                Constraint::Length(38),
            ])
            .areas(body)
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
        }
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
        Clear.render(area, buf);
        self.viewport.borrow_mut().marker = None;
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
                    .filter(|s| !self.hidden.contains(&s.id) && group.is_none_or(|g| g == s.status))
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
        super::selection_tabs::render_filled_tab_bar(
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
        let now = chrono::Utc::now();
        self.render_center_rows(layout.list, buf, now);
        if !layout.details.is_empty() {
            self.render_details(layout.details, buf, now);
        }
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
        let Some(session) = self.selected_row() else {
            return;
        };
        let (status, dot) = Self::status(session);
        let mut lines = vec![
            "Task details".bold().into(),
            Line::default(),
            Line::from(Span::styled(
                session.display_title().to_owned(),
                self.title_style(&session.id).bold(),
            )),
            Line::from(vec![dot, " ".into(), status.into()]),
            Line::default(),
            Line::from(vec![
                "Model: ".dim(),
                session.model_name().to_owned().into(),
            ]),
            Line::from(vec![
                "Reasoning: ".dim(),
                session
                    .effort
                    .as_deref()
                    .unwrap_or("Unknown")
                    .to_owned()
                    .into(),
            ]),
            Line::default(),
            "Project".dim().into(),
            session.cwd.display().to_string().into(),
        ];
        let mut pr_link = None;
        if let Some(branch) = &session.git_branch {
            lines.push(Line::default());
            if let Some(pr) = &session.pr {
                let label = format!("#{}", pr.number);
                let prefix_width = 8 + branch.width() + 3;
                let line_width = prefix_width + label.len();
                if lines.len() < usize::from(area.height) && line_width <= usize::from(area.width) {
                    pr_link = Some((
                        Rect::new(
                            area.x + prefix_width as u16,
                            area.y + lines.len() as u16,
                            label.len() as u16,
                            1,
                        ),
                        &pr.url,
                    ));
                }
                lines.push(Line::from(vec![
                    "Branch  ".dim(),
                    branch.clone().into(),
                    " · ".dim(),
                    label.into(),
                ]));
            } else {
                lines.extend(["Branch".dim().into(), branch.clone().into()]);
            }
        }
        let width = usize::from(area.width.max(1));
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
        let mut prompt = vec![Line::default(), Line::from(header)];
        let prompt_text = match session.prompt.trim() {
            "" => "No prompt available.",
            prompt => prompt,
        };
        let mut prompt_body = textwrap::wrap(prompt_text, width)
            .into_iter()
            .map(|s| Line::from(s.into_owned()))
            .collect::<Vec<_>>();
        if prompt_body.len() > 2 {
            prompt_body.truncate(2);
            prompt_body[1] = "…".dim().into();
        }
        prompt.extend(prompt_body);
        // Activity sits above the prompt and takes the height the prompt leaves over.
        let mut details = Vec::new();
        if session.status == SessionStatus::NeedsInput {
            details.extend([
                Line::default(),
                "Needs attention".red().into(),
                "Open task to review.".dim().into(),
                "Waiting for your response.".into(),
            ]);
        }
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
            details.extend([
                Line::default(),
                Line::from(vec![
                    "Latest activity".dim(),
                    " · ".dim(),
                    format_relative_time(now, Some(activity.at)).dim(),
                ]),
                truncate_line_with_ellipsis_if_overflow(Line::from(label), width),
            ]);
        }
        if let Some(message) = session.last_message.as_ref().filter(|_| activity.is_none()) {
            let mut header = vec!["Last message".dim()];
            if let Some(at) = session.last_message_at {
                header.extend([" · ".dim(), format_relative_time(now, Some(at)).dim()]);
            }
            details.extend([Line::default(), Line::from(header)]);
            details.extend(
                textwrap::wrap(message, width)
                    .into_iter()
                    .map(|s| Line::from(s.into_owned())),
            );
        }
        let available = usize::from(area.height).saturating_sub(lines.len() + prompt.len());
        if details.len() > available {
            details.truncate(available);
            if let Some(last) = details.last_mut() {
                *last = "…".dim().into();
            }
        }
        lines.extend(details);
        lines.extend(prompt);
        lines.truncate(usize::from(area.height));
        for (offset, text) in lines.into_iter().enumerate() {
            line(text, row(area, offset as u16, 1), buf);
        }
        if let Some((area, url)) = pr_link {
            super::hyperlink::hyperlink(buf, area, url);
        }
    }
}
