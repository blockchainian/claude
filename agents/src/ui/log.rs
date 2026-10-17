use super::formatting::{format_log_duration, format_relative_time};
use super::line_truncation::truncate_line_with_ellipsis_if_overflow;
use super::*;
use crate::log::{Event, Kind};
use chrono::{DateTime, Local, Utc};

const CLOCK_FORMAT: &str = "%H:%M:%S";
pub(super) const CLOCK_WIDTH: usize = 8;

/// Rows above the detail header: the top rule.
pub(super) const LOG_TOP: u16 = 1;

pub(super) fn search_input_rect(area: Rect) -> Rect {
    super::render::row(area, area.height.saturating_sub(1), 1)
}

impl View {
    pub(super) fn detail_text(&self, event: &Event) -> String {
        if self.log_detail_raw {
            match event.raw_text() {
                Ok(text) => text,
                Err(error) => format!("Cannot read transcript: {error}"),
            }
        } else {
            open_text(event)
        }
    }

    fn selected_detail_layout(
        &self,
        event: &Event,
        width: u16,
        wide: bool,
        cwd: &Path,
        now: DateTime<Utc>,
    ) -> DetailLayout {
        if self.log_detail_raw {
            let source = self.detail_text(event);
            let drawn = source.replace('\t', "    ");
            let mut layout = detail_parts_layout(
                event,
                width,
                wide,
                cwd,
                now,
                vec![super::tool_detail::Part::raw_json(source)],
            );
            if super::highlight::JsonHighlight::enabled(&drawn) {
                let widest = layout.lines.iter().map(Line::width).max().unwrap_or(0);
                // Centering was already applied to plain rows by detail_parts_layout.
                let content_width = super::tool_detail::split_wide(
                    super::highlight::plain_lines(&drawn),
                    usize::from(width),
                )
                .iter()
                .map(Line::width)
                .max()
                .unwrap_or(0);
                layout.raw = Some(RawHighlight {
                    source: drawn,
                    highlighter: super::highlight::JsonHighlight::default(),
                    row: 0,
                    padding: widest.saturating_sub(content_width),
                    style: super::style::log_styles(&event.kind).1,
                });
            }
            layout
        } else {
            detail_parts_layout(event, width, wide, cwd, now, open_parts(event))
        }
    }

    pub(super) fn render_log(&self, area: Rect, buf: &mut Buffer, now: DateTime<Utc>) {
        self.viewport.borrow_mut().log_text = text_selection::TextRegion::default();
        if !self.selecting_list(text_selection::Panel::LogList) {
            self.viewport.borrow_mut().log_list_text = text_selection::TextRegion::default();
        }
        {
            let mut viewport = self.viewport.borrow_mut();
            viewport.log_rows.clear();
            viewport.log_jump = Rect::default();
            viewport.log_links.clear();
        }
        if area.is_empty() {
            return;
        }
        let Some(session) = self.selected_row() else {
            return;
        };
        if self.log_session.as_ref() != Some(&session.id) {
            return;
        }
        if self.log_search.active() && area.height > 1 {
            let input = search_input_rect(area);
            if self.log_search.editing {
                super::render::render_input_row(
                    &format!("Search {PROMPT_MARKER} "),
                    &self.log_search.query,
                    input,
                    buf,
                );
            } else {
                Line::from(self.log_search.query.text().to_owned())
                    .dim()
                    .right_aligned()
                    .render(input, buf);
            }
        }
        self.reveal_log_selection();
        let scroll = self.log_scroll();
        let matching: Vec<_> = self.shown_log_events().collect();
        let selected_index = self.log_selected;
        let end = matching.len().saturating_sub(scroll);
        let start = end.saturating_sub(usize::from(
            area.height
                .saturating_sub(u16::from(self.log_search.active())),
        ));
        let mut rows = Vec::new();
        let mut drawn_links = Vec::new();
        for (index, &(log_index, event)) in matching[start..end].iter().rev().enumerate() {
            let last = scroll == 0 && index == 0;
            let time = event_time(event, session, last, now);
            let links = render_event(
                event,
                &time,
                last,
                super::render::row(area, index as u16, 1),
                buf,
                Some(&self.log_search),
                &session.cwd,
            );
            if selected_index == Some(end - index - 1) {
                super::render::fill_list_row(
                    super::render::row(area, index as u16, 1),
                    buf,
                    super::style::cursor_style(),
                );
            }
            rows.push(log_index);
            drawn_links.extend(links);
        }
        let text_area = super::render::row(area, 0, rows.len() as u16);
        {
            let mut viewport = self.viewport.borrow_mut();
            viewport.log_rows = rows;
            viewport.log_links = drawn_links;
        }
        self.capture_list_text(text_selection::Panel::LogList, text_area, scroll, buf);
    }

    pub(super) fn visible_log_event(&self) -> Option<&Event> {
        self.log_background_event.as_ref().or_else(|| {
            self.log_selected
                .and_then(|index| self.shown_log_events().nth(index).map(|(_, event)| event))
        })
    }

    pub(super) fn render_log_detail(&self, area: Rect, buf: &mut Buffer, now: DateTime<Utc>) {
        super::render::line("─".repeat(usize::from(area.width)).dim(), area, buf);
        let Some(session) = self.selected_row() else {
            return;
        };
        if self.log_session.as_ref() != Some(&session.id) {
            return;
        }
        let Some(event) = self.visible_log_event() else {
            return;
        };
        let time = event_time(event, session, self.is_newest_shown(), now);
        let list_links = self.viewport.borrow().log_links.clone();
        self.render_event_detail(event, &time, area, buf, &session.cwd, now);
        self.viewport.borrow_mut().log_links.extend(list_links);
    }

    /// Draws the button that jumps back to the newest row, centered over the rows' content, while
    /// an older event is selected.
    pub(super) fn render_log_jump(&self, area: Rect, buf: &mut Buffer) {
        let mut viewport = self.viewport.borrow_mut();
        if self.following_log() {
            viewport.log_unseen = 0;
            return;
        }
        let label = match viewport.log_unseen {
            0 => " ↑ Latest ".to_owned(),
            unseen => format!(" ↑ {unseen} New "),
        };
        let jump = log_content_centered(area, label.width() as u16);
        Line::from(Span::styled(label, super::style::log_jump_style())).render(jump, buf);
        viewport.log_jump = jump;
    }

    /// Draws one log event whole: its list row's time and kind, a blank row, then the full body.
    fn render_event_detail(
        &self,
        event: &Event,
        time: &str,
        area: Rect,
        buf: &mut Buffer,
        cwd: &std::path::Path,
        now: DateTime<Utc>,
    ) {
        let (label_style, _) = super::style::log_styles(&event.kind);
        let time_style = super::style::log_time_style(&event.kind, self.is_newest_shown());
        let input_title = match event.calls.as_slice() {
            [call] => super::tool_detail::input_title(call),
            _ => None,
        };
        let mut header = Vec::new();
        if let Some(sender) = &event.sender {
            header.push(Span::styled(format!("({sender}) "), label_style));
        }
        if event.kind != Kind::Tool {
            header.push(Span::styled(kind_label(&event.kind), label_style));
        }
        if event.kind == Kind::Tool || shows_calls(event) {
            if event.kind != Kind::Tool {
                header.push(" ".into());
            }
            let title = if event.calls.len() == 1 {
                input_title
                    .clone()
                    .unwrap_or_else(|| event.calls[0].title.clone())
            } else {
                event.body.clone()
            };
            header.push(Span::styled(title, label_style));
        }
        let header_label = match event.calls.as_slice() {
            [call] if call.name == "task-notification" => {
                Some(Span::styled(format!("Task {}", call.id), time_style))
            }
            [call] => call
                .agent
                .as_ref()
                .and_then(|agent| agent.header.as_ref())
                .map(|header| Span::styled(header.clone(), time_style)),
            _ => None,
        };
        let header_row = super::render::row(area, LOG_TOP, 1);
        let (content_area, time_area) = log_row_areas(header_row);
        // The centered title keeps the metadata and time clear on both sides.
        let margin = (header_row.width - content_area.width).max(
            header_label
                .as_ref()
                .map_or(0, |id| id.width().min(usize::from(header_row.width)) as u16),
        );
        let mut header_area = Rect::new(
            header_row.x + margin,
            header_row.y,
            header_row.width.saturating_sub(margin * 2),
            1,
        );
        if input_title.is_some() {
            header_area = content_area;
        }
        if !header_area.is_empty() {
            let title = truncate_line_with_ellipsis_if_overflow(
                Line::from(header),
                header_area.width.into(),
            );
            let title = if input_title.is_some() {
                title
            } else {
                title.centered()
            };
            title.render(header_area, buf);
        }
        Line::from(Span::styled(time.to_owned(), time_style))
            .right_aligned()
            .render(time_area, buf);
        // Metadata occupies the slot opposite the time.
        if let Some(header_label) = header_label {
            let id_area = Rect::new(header_row.x, header_row.y, margin, 1);
            truncate_line_with_ellipsis_if_overflow(Line::from(header_label), id_area.width.into())
                .render(id_area, buf);
        }
        let mut body_area =
            super::render::row(area, LOG_TOP + 2, area.height.saturating_sub(LOG_TOP + 2));
        if self.log_detail_search.active() {
            body_area.height = body_area.height.saturating_sub(1);
            let input = search_input_rect(area);
            if self.log_detail_search.editing {
                super::render::render_input_row(
                    &format!("Search {PROMPT_MARKER} "),
                    &self.log_detail_search.query,
                    input,
                    buf,
                );
            } else {
                Line::from(self.log_detail_search.query.text().to_owned())
                    .dim()
                    .right_aligned()
                    .render(input, buf);
            }
        }
        let height = usize::from(body_area.height);
        let wide = buf.area.width >= super::render::WIDE_LAYOUT_COLUMNS;
        let mut viewport = self.viewport.borrow_mut();
        viewport.log_detail_wide = wide;
        let mut layout = match viewport.log_detail_layout.take() {
            Some(layout)
                if layout.width == area.width
                    && layout.wide == wide
                    && layout.height == height
                    && layout.theme_generation == super::highlight::theme_generation() =>
            {
                layout
            }
            _ => {
                let mut layout = self.selected_detail_layout(event, area.width, wide, cwd, now);
                layout.height = height;
                layout
            }
        };
        for &row in &layout.running {
            layout.lines[row]
                .spans
                .last_mut()
                .expect("running span exists")
                .content = super::tool_detail::running_frame(now).into();
        }
        // The scrollbar sits in the panel's right padding, outside the body.
        let scrollbar_x = area.right();
        let overflow = layout.lines.len() > height && scrollbar_x < buf.area.right();
        for (source, links) in layout.sources.iter().zip(&mut layout.links) {
            super::links::add_commits(&source.text, cwd, links);
        }
        viewport.log_detail_max = layout.lines.len().saturating_sub(height);
        viewport.log_detail_scroll = viewport.log_detail_scroll.min(viewport.log_detail_max);
        let scroll = viewport.log_detail_scroll;
        layout.highlight_raw(scroll + height);
        if overflow && height > 0 {
            let thumb = (height * height)
                .div_ceil(layout.lines.len())
                .max(1)
                .min(height);
            let top = scroll * (height - thumb) / viewport.log_detail_max.max(1);
            for row in 0..height {
                buf[(scrollbar_x, body_area.y + row as u16)].set_symbol(
                    if (top..top + thumb).contains(&row) {
                        "│"
                    } else {
                        " "
                    },
                );
            }
        }
        let rows: Vec<_> = layout
            .rows
            .iter()
            .skip(scroll)
            .take(height)
            .enumerate()
            .map(|(row, (segment, cells))| text_selection::DrawnRow {
                segment: *segment,
                x: body_area.x,
                y: body_area.y + row as u16,
                cells: cells.clone(),
            })
            .collect();
        viewport.log_links = rows
            .iter()
            .flat_map(|row| {
                super::links::drawn_links(std::slice::from_ref(row), &layout.links[row.segment])
            })
            .collect();
        viewport.log_text = text_selection::TextRegion {
            area: body_area,
            sources: layout.sources.clone(),
            rows,
        };
        let mut visible = layout.lines
            [scroll.min(layout.lines.len())..(scroll + height).min(layout.lines.len())]
            .to_vec();
        viewport.log_detail_layout = Some(layout);
        drop(viewport);
        for line in &mut visible {
            highlight_matches(line, &self.log_detail_search);
        }
        Paragraph::new(visible).render(body_area, buf);
        self.highlight_text_selection(text_selection::Panel::Log, buf);
    }

    pub(super) fn jump_detail_match(&mut self) {
        let Some(event) = self.visible_log_event() else {
            return;
        };
        if self.log_detail_search.query.is_empty() {
            return;
        }
        let viewport = self.viewport.borrow();
        let width = viewport.log_detail.width.max(1);
        let cwd = self
            .selected_row()
            .map(|session| session.cwd.as_path())
            .unwrap_or(Path::new("."));
        let layout = self.selected_detail_layout(
            event,
            width,
            viewport.log_detail_wide,
            cwd,
            DateTime::UNIX_EPOCH,
        );
        let found = layout
            .lines
            .iter()
            .position(|line| !self.log_detail_search.indices(&line.to_string()).is_empty());
        drop(viewport);
        if let Some(line) = found {
            self.viewport.borrow_mut().log_detail_scroll = line;
        }
    }

    /// Selects an index in the shown list; `None` selects its newest row.
    pub(super) fn select_log_event(&mut self, index: Option<usize>) {
        self.log_detail_focused = index.is_some();
        let background = self.log_background_event.take().is_some();
        let len = self.matching_log_count();
        let index = len
            .checked_sub(1)
            .map(|last| index.unwrap_or(last).min(last));
        if self.log_selected != index || background {
            self.cancel_detail_selection();
            self.log_selected = index;
            self.log_detail_search.clear();
            let mut viewport = self.viewport.borrow_mut();
            viewport.log_detail_layout = None;
            viewport.log_detail_other_layout = None;
            viewport.log_detail_scroll = 0;
        }
        if self.following_log() {
            self.viewport.borrow_mut().log_unseen = 0;
        }
    }

    #[cfg(test)]
    pub(super) fn select_fixture_event(&mut self, event: Option<Event>) {
        let index = event.map(|event| {
            let full = self
                .log
                .iter()
                .position(|candidate| candidate == &event)
                .unwrap_or_else(|| {
                    self.log.push(event);
                    self.prepare_log_search();
                    self.log.len() - 1
                });
            self.shown_log_events()
                .position(|(index, _)| index == full)
                .expect("fixture is shown")
        });
        self.select_log_event(index);
    }

    fn following_log(&self) -> bool {
        (self.log_selected.is_none()
            || self.log_selected == self.matching_log_count().checked_sub(1))
            && self.log_background_return.is_none()
    }

    /// Keeps the cursor in view without moving a list being dragged for text selection.
    fn reveal_log_selection(&self) {
        if self.mouse_press.is_some()
            || (self.selecting_list(text_selection::Panel::LogList)
                && self.text_selection_dragging())
        {
            return;
        }
        let Some(selected) = self.log_selected else {
            return;
        };
        let offset = self.matching_log_count().saturating_sub(selected + 1);
        let mut viewport = self.viewport.borrow_mut();
        let height = usize::from(
            viewport
                .log
                .height
                .saturating_sub(u16::from(self.log_search.active())),
        );
        if height == 0 {
            return;
        }
        if offset < viewport.log_scroll {
            viewport.log_scroll = offset;
        } else if offset >= viewport.log_scroll + height {
            viewport.log_scroll = offset + 1 - height;
        }
    }

    pub(super) fn navigate_log(&mut self, code: KeyCode) {
        match code {
            KeyCode::Up | KeyCode::Down => self.move_log_selection(code == KeyCode::Down),
            KeyCode::PageUp | KeyCode::PageDown => {
                let steps = self
                    .viewport
                    .borrow()
                    .log
                    .height
                    .saturating_sub(u16::from(self.log_search.active()))
                    .max(1);
                for _ in 0..steps {
                    self.move_log_selection(code == KeyCode::PageDown);
                }
            }
            KeyCode::Home | KeyCode::End => {
                self.log_pending_older = 0;
                self.select_log_event(if code == KeyCode::Home { Some(0) } else { None });
                self.reveal_log_selection();
            }
            _ => unreachable!("list navigation action"),
        }
    }

    /// Moves one displayed event; App loads older rows at the oldest loaded boundary.
    pub(super) fn move_log_selection(&mut self, older: bool) {
        self.pending_click = None;
        self.log_background_return = None;
        self.log_background_event = None;
        if !older && self.log_pending_older > 0 {
            self.log_pending_older -= 1;
            let mut viewport = self.viewport.borrow_mut();
            viewport.log_scroll = viewport.log_scroll.saturating_sub(1);
            return;
        }
        let Some(index) = self.log_selected else {
            self.show_newest_log();
            return;
        };
        if older && index == 0 {
            if self.log_search.active() {
                return;
            }
            self.log_pending_older += 1;
            self.viewport.borrow_mut().log_scroll += 1;
            return;
        }
        let next = if older {
            index.saturating_sub(1)
        } else {
            index.saturating_add(1)
        };
        self.log_pending_older = 0;
        self.select_log_event(Some(next));
        self.reveal_log_selection();
    }

    pub(crate) fn finish_log_start(&mut self) {
        if std::mem::take(&mut self.log_start_requested) {
            self.select_log_event(Some(0));
            self.reveal_log_selection();
        }
    }

    pub(crate) fn log_page_pending(&self) -> bool {
        self.log_pending_older > 0
    }

    /// Applies structural changes without comparing row contents.
    pub(crate) fn apply_log_update(&mut self, update: crate::log::Update) {
        let following = self.following_log();
        let selected_index = self
            .log_background_return
            .as_ref()
            .and_then(|saved| saved.event)
            .or(self.log_selected);
        let selected_full = selected_index
            .and_then(|index| self.shown_log_events().nth(index).map(|(full, _)| full));
        let old_len = self.log.len();
        let appended_start = update.events.len().saturating_sub(update.appended);
        self.log = update.events;
        // Rematching below uses the explicitly shifted full-list position.
        self.log_selected = None;
        self.prepare_log_search();
        let added = self
            .shown_log_events()
            .filter(|(full, _)| *full >= appended_start)
            .count();
        let len = self.matching_log_count();
        self.log_selected = if update.reset || following {
            len.checked_sub(1)
        } else {
            selected_full.and_then(|full| {
                let full = full
                    .saturating_add(update.prepended)
                    .min(self.log.len().saturating_sub(1));
                self.shown_log_events()
                    .position(|(index, _)| index >= full)
                    .or_else(|| len.checked_sub(1))
            })
        };
        if let Some(previous) = &mut self.log_background_return {
            // Background details retain a list index, independent of their synthetic event.
            previous.event = self.log_selected;
        }
        self.cancel_detail_selection();
        self.viewport.borrow_mut().log_detail_layout = None;
        self.viewport.borrow_mut().log_detail_other_layout = None;
        if update.reset {
            let mut viewport = self.viewport.borrow_mut();
            viewport.log_scroll = 0;
            viewport.log_unseen = 0;
            viewport.log_detail_scroll = 0;
        } else if !following || self.mouse_press.is_some() {
            let mut viewport = self.viewport.borrow_mut();
            viewport.log_scroll += added;
            viewport.log_unseen += added;
            drop(viewport);
            self.shift_list_selection(text_selection::Panel::LogList, added);
        } else {
            self.viewport.borrow_mut().log_scroll = 0;
        }
        let pending = std::mem::take(&mut self.log_pending_older);
        if pending > 0
            && self.log.len() > old_len
            && !update.reset
            && let Some(index) = self.log_selected
        {
            self.select_log_event(Some(index.saturating_sub(pending)));
        }
    }

    #[cfg(test)]
    pub(crate) fn set_log(&mut self, events: Vec<Event>) {
        let appended = events.len().saturating_sub(self.log.len());
        self.apply_log_update(crate::log::Update {
            events,
            appended,
            ..Default::default()
        });
    }

    /// Wake only when an animated tool output is in the visible detail viewport.
    pub(crate) fn tool_redraw_in(&self, now: DateTime<Utc>) -> Option<std::time::Duration> {
        let viewport = self.viewport.borrow();
        let layout = viewport.log_detail_layout.as_ref()?;
        let visible = viewport.log_detail_scroll
            ..viewport.log_detail_scroll + usize::from(viewport.log_text.area.height);
        layout
            .running
            .iter()
            .any(|row| visible.contains(row))
            .then(|| super::tool_detail::running_redraw_in(now))
    }

    /// How long until the top log row's time reads differently, or `None` if it never does.
    pub(crate) fn log_redraw_in(&self, now: DateTime<Utc>) -> Option<std::time::Duration> {
        let session = self.selected_row()?;
        if self.log_session.as_ref() != Some(&session.id) {
            return None;
        }
        let event = self.shown_log_events().last()?.1;
        if self.log_scroll() != 0 && !self.following_log() {
            return None;
        }
        row_time_changes_in(&row_time(event, session, true), now)
    }

    fn is_newest_shown(&self) -> bool {
        self.log_background_event.is_none()
            && self.log_selected.is_some()
            && self.log_selected == self.matching_log_count().checked_sub(1)
    }

    /// The log events the search lets through, with their index in the log.
    pub(super) fn shown_log_events(&self) -> impl Iterator<Item = (usize, &Event)> {
        self.log
            .iter()
            .enumerate()
            .filter(|(index, _)| self.log_matches(*index))
    }
}

/// How a log row's time column reads.
#[derive(Debug, PartialEq)]
enum RowTime {
    /// The wall-clock time the event started.
    Clock(DateTime<Utc>),
    /// A duration that no longer changes.
    Settled(chrono::Duration),
    /// A duration still growing from the given start.
    Running(DateTime<Utc>),
    /// The time elapsed since the given moment, as "12s ago".
    Ago(DateTime<Utc>),
}

fn row_time(event: &Event, session: &Session, last: bool) -> RowTime {
    if !matches!(event.kind, Kind::Tool | Kind::Error | Kind::Denied) {
        return if last {
            RowTime::Ago(event.started_at)
        } else if event.kind == Kind::Assistant
            && let Some(reference) = event.previous_end
        {
            RowTime::Settled(event.started_at - reference)
        } else {
            RowTime::Clock(event.started_at)
        };
    }
    if session.id.provider == crate::providers::ProviderId::Codex
        && event.kind == Kind::Tool
        && let Some(end) = event.ended_at
    {
        return match event.duration {
            Some(duration) => RowTime::Settled(duration),
            None => RowTime::Clock(end),
        };
    }
    let Some(previous) = event.previous_end else {
        return RowTime::Clock(event.started_at);
    };
    let background = event.body.ends_with(" *");
    let end = match event.ended_at {
        Some(end) => end,
        None if !last || background => event.started_at,
        None if session.parent.is_some() && session.status == SessionStatus::Working => {
            return RowTime::Running(previous);
        }
        None if session.process.is_none() || session.status == SessionStatus::Inactive => {
            session.last_record_at.unwrap_or(event.started_at)
        }
        None => return RowTime::Running(previous),
    };
    RowTime::Settled(end - previous)
}

fn format_row_time(time: &RowTime, now: DateTime<Utc>) -> String {
    match *time {
        RowTime::Clock(at) => at.with_timezone(&Local).format(CLOCK_FORMAT).to_string(),
        RowTime::Settled(duration) => format_log_duration(duration),
        RowTime::Running(start) if now - start < chrono::Duration::microseconds(500) => {
            "0ms".into()
        }
        RowTime::Running(start) => format_log_duration(now - start),
        RowTime::Ago(at) => format_relative_time(now, Some(at)),
    }
}

/// How long until `format_row_time` reads differently, or `None` if it never does.
fn row_time_changes_in(time: &RowTime, now: DateTime<Utc>) -> Option<std::time::Duration> {
    match *time {
        RowTime::Clock(_) | RowTime::Settled(_) => None,
        RowTime::Running(_) => Some(std::time::Duration::from_secs(1)),
        RowTime::Ago(at) => {
            let seconds = (now - at).num_seconds().max(0) as u64;
            let unit = match seconds {
                0..60 => 1,
                60..3600 => 60,
                3600..86400 => 3600,
                _ => 86400,
            };
            Some(std::time::Duration::from_secs(unit - seconds % unit))
        }
    }
}

fn event_time(event: &Event, session: &Session, last: bool, now: DateTime<Utc>) -> String {
    format_row_time(&row_time(event, session, last), now)
}

/// Centers a one-row label of `width` over the content beside the clock column, the line log
/// rows center their text on.
pub(super) fn log_content_centered(area: Rect, width: u16) -> Rect {
    let (content, _) = log_row_areas(area);
    let width = width.min(area.width);
    Rect::new(
        content.x + content.width.saturating_sub(width) / 2,
        area.y,
        width,
        1,
    )
    .intersection(area)
}

/// Reserves the same trailing clock column and separator for every row.
fn log_row_areas(area: Rect) -> (Rect, Rect) {
    let time_width = (CLOCK_WIDTH as u16).min(area.width);
    let content_width = area.width.saturating_sub(time_width + 2);
    (
        Rect::new(area.x, area.y, content_width, area.height),
        Rect::new(area.right() - time_width, area.y, time_width, area.height),
    )
}

/// Draws one log row and returns its visible links.
fn render_event(
    event: &Event,
    time: &str,
    last: bool,
    area: Rect,
    buf: &mut Buffer,
    search: Option<&super::log_search::LogSearch>,
    cwd: &std::path::Path,
) -> Vec<super::links::DrawnLink> {
    if area.is_empty() {
        return Vec::new();
    }
    let (content_area, time_area) = log_row_areas(area);
    let time_style = super::style::log_time_style(&event.kind, last);
    Line::from(Span::styled(time.to_owned(), time_style))
        .right_aligned()
        .render(time_area, buf);
    let content_width = usize::from(content_area.width);
    if content_width == 0 {
        return Vec::new();
    }
    let body_width = content_width;
    let fitted = fitted_body_line(event, body_width, cwd);
    let mut body = fitted.line;
    let padding = body_width.saturating_sub(body.width()) / 2;
    let body_x = content_area.x + padding as u16;
    let clipped = body.width() > body_width;
    let visible = if clipped {
        body_width.saturating_sub(1)
    } else {
        body_width
    } as u16;
    // Match the full row so a match crossing the right edge still highlights its prefix.
    let indices = search
        .filter(|search| !search.query.is_empty())
        .map(|search| search.indices(&body.to_string()));
    body = truncate_line_with_ellipsis_if_overflow(body, body_width);
    let links = {
        let links = if is_prose(&event.kind) || event.kind == Kind::QueuedUser {
            super::links::text_links(&fitted.source, cwd, &event.images)
        } else {
            super::links::path_links(&fitted.source, &fitted.paths, cwd)
        };
        let mut maps = super::text_selection_layout::map_lines(
            &fitted.source,
            std::slice::from_ref(&body),
            is_prose(&event.kind),
        );
        let mut cells = maps.pop().unwrap_or_default();
        cells.retain(|(columns, _)| columns.end <= visible);
        super::links::drawn_links(
            &[text_selection::DrawnRow {
                segment: 0,
                x: body_x,
                y: area.y,
                cells,
            }],
            &links,
        )
    };
    if let Some(indices) = indices {
        let ellipsis = if clipped { body.spans.pop() } else { None };
        highlight_indices(&mut body, &indices);
        if let Some(mut ellipsis) = ellipsis {
            ellipsis.style = body.spans.last().map(|span| span.style).unwrap_or_default();
            body.spans.push(ellipsis);
        }
    }
    body.render(
        Rect::new(body_x, area.y, (body_width - padding) as u16, 1),
        buf,
    );
    links
}

/// The screen cells of one drawn line, each with the source bytes it shows.
type Cells = Vec<(std::ops::Range<u16>, std::ops::Range<usize>)>;

/// The raw source prefix colored so far, with its saved grammar and theme state.
struct RawHighlight {
    source: String,
    highlighter: super::highlight::JsonHighlight,
    row: usize,
    padding: usize,
    style: Style,
}

/// The open log event laid out for one panel size and highlight theme.
pub(super) struct DetailLayout {
    raw: Option<RawHighlight>,
    width: u16,
    wide: bool,
    height: usize,
    theme_generation: u64,
    lines: Vec<Line<'static>>,
    /// Animated output rows, updated without repeating syntax highlighting.
    running: Vec<usize>,
    /// Each line's text part and the cells mapping it back to that part's bytes.
    rows: Vec<(usize, Cells)>,
    sources: Vec<text_selection::Source>,
    /// The links in each text part.
    links: Vec<Vec<super::links::Link>>,
}

impl DetailLayout {
    fn highlight_raw(&mut self, end: usize) {
        let Some(raw) = &mut self.raw else {
            return;
        };
        if raw.row >= end.min(self.lines.len()) {
            return;
        }
        raw.highlighter.advance(&raw.source, |line| {
            let rows = super::tool_detail::split_wide(vec![line], usize::from(self.width));
            for mut line in rows {
                for span in &mut line.spans {
                    span.style = raw.style.patch(span.style);
                }
                if raw.padding > 0 {
                    line.spans.insert(0, Span::raw(" ".repeat(raw.padding)));
                }
                self.lines[raw.row] = line;
                raw.row += 1;
            }
            raw.row < end
        });
    }
}

#[cfg(test)]
fn detail_layout(
    event: &Event,
    width: u16,
    cwd: &std::path::Path,
    now: DateTime<Utc>,
) -> DetailLayout {
    detail_parts_layout(
        event,
        width,
        width >= super::render::WIDE_LAYOUT_COLUMNS,
        cwd,
        now,
        open_parts(event),
    )
}

fn detail_parts_layout(
    event: &Event,
    width: u16,
    wide: bool,
    cwd: &Path,
    now: DateTime<Utc>,
    parts: Vec<super::tool_detail::Part>,
) -> DetailLayout {
    let theme_generation = super::highlight::theme_generation();
    let body_style = super::style::log_styles(&event.kind).1;
    let rendered: Vec<_> = parts
        .into_iter()
        .map(|part| {
            let body_width = part
                .column(usize::from(width), wide)
                .map_or(usize::from(width), |(width, _)| width);
            let lines = part.lines(body_width, body_style, now);
            (part, lines)
        })
        .collect();
    let mut group_widths = vec![0; rendered.len()];
    for (part, lines) in &rendered {
        if let Some(group) = part.group {
            let widest = lines.iter().map(Line::width).max().unwrap_or(0);
            group_widths[group] = group_widths[group].max(widest);
        }
    }
    let parts: Vec<_> = rendered
        .into_iter()
        .map(|(part, mut lines)| {
            let markdown = part.markdown();
            let mut maps = part.maps(&lines);
            let block_width = part.group.map_or_else(
                || lines.iter().map(Line::width).max().unwrap_or(0),
                |group| group_widths[group],
            );
            let padding = part.column(usize::from(width), wide).map_or_else(
                || usize::from(width).saturating_sub(block_width) / 2,
                |(_, left)| left,
            );
            if padding > 0 {
                // Every row keeps its position within the section, including its source cell map.
                for (line, cells) in lines.iter_mut().zip(&mut maps) {
                    line.spans.insert(0, Span::raw(" ".repeat(padding)));
                    for (range, _) in cells {
                        range.start += padding as u16;
                        range.end += padding as u16;
                    }
                }
            }
            let links = if markdown
                || (!shows_calls(event)
                    && (is_prose(&event.kind) || event.kind == Kind::QueuedUser))
            {
                let images = if shows_calls(event) {
                    &[][..]
                } else {
                    &event.images[..]
                };
                super::links::text_links(&part.source, cwd, images)
            } else {
                let paths = if !shows_calls(event) && part.source == event.body {
                    tool_paths(event, cwd)
                } else {
                    super::paths::text_paths(&part.source, cwd)
                };
                super::links::path_links(&part.source, &paths, cwd)
            };
            (part.running(), part.source, lines, maps, links)
        })
        .collect();
    let mut layout = DetailLayout {
        raw: None,
        width,
        wide,
        height: 0,
        theme_generation,
        lines: Vec::new(),
        running: Vec::new(),
        rows: Vec::new(),
        sources: Vec::new(),
        links: Vec::new(),
    };
    for (id, (running, source, lines, mut maps, links)) in parts.into_iter().enumerate() {
        if running {
            layout.running.push(layout.lines.len());
        }
        for cells in &mut maps {
            cells.retain(|(range, _)| range.end <= width);
        }
        layout
            .rows
            .extend(maps.into_iter().map(|cells| (id, cells)));
        layout.lines.extend(lines);
        layout.sources.push(text_selection::Source {
            id,
            text: source.into(),
            label: copied_label(event),
        });
        layout.links.push(links);
    }
    layout
}

/// Whether an event draws its tool calls when open, which makes its row open on click even
/// when its body fits the row.
pub(super) fn shows_calls(event: &Event) -> bool {
    !event.calls.is_empty()
        && (matches!(event.kind, Kind::Tool | Kind::Error | Kind::Denied)
            || event
                .calls
                .iter()
                .any(|call| call.name == "AskUserQuestion"))
}

/// The full text an open log event shows below its header.
pub(super) fn open_text(event: &Event) -> String {
    open_parts(event)
        .into_iter()
        .map(|part| part.source)
        .collect::<Vec<_>>()
        .join("\n")
}

fn open_parts(event: &Event) -> Vec<super::tool_detail::Part> {
    if shows_calls(event) {
        super::tool_detail::parts(event)
    } else if event.kind == Kind::Tool {
        Vec::new()
    } else if is_prose(&event.kind) {
        vec![super::tool_detail::Part::prose(event.body.clone(), true)]
    } else if matches!(event.kind, Kind::QueuedUser | Kind::QueuedSystem) {
        super::tool_detail::Part::prose_parts(event.body.clone(), false)
    } else {
        super::tool_detail::Part::text_parts(event.body.clone(), false)
    }
}

/// The footer confirmation after copying an open log event, such as "Copied user message".
pub(super) fn copied_label(event: &Event) -> String {
    format!("Copied {} message", kind_label(&event.kind).to_lowercase())
}

pub(super) fn kind_label(kind: &Kind) -> &'static str {
    match kind {
        Kind::User => "User",
        Kind::Compaction => "Compaction",
        Kind::Assistant => "Assistant",
        Kind::Agent => "Agent",
        Kind::Session => "Session",
        Kind::Tool => "Tool",
        Kind::Error => "Error",
        Kind::Denied => "Denied",
        Kind::QueuedUser | Kind::QueuedSystem => "Queue",
    }
}

/// Whether an event's body is prose the log renders as Markdown.
fn is_prose(kind: &Kind) -> bool {
    matches!(
        kind,
        Kind::User | Kind::Compaction | Kind::Assistant | Kind::Agent
    )
}

pub(super) fn body_line(event: &Event, body_width: usize, cwd: &std::path::Path) -> Line<'static> {
    fitted_body_line(event, body_width, cwd).line
}

/// An event's body as one log row, with paths shortened to fit the row.
struct FittedBody {
    line: Line<'static>,
    /// The text the row was drawn from, before any shortening: the event's prose with
    /// its whitespace collapsed, or its tool text with home paths shown as `~`.
    source: String,
    /// Byte ranges of the tool paths inside `source`.
    paths: Vec<std::ops::Range<usize>>,
}

fn fitted_body_line(event: &Event, body_width: usize, cwd: &std::path::Path) -> FittedBody {
    let listed;
    let event = if event.sender.is_some() || event.kind == Kind::Agent {
        // Peer rows keep their one-line Markdown interpretation; open bodies keep blocks.
        let body = if event.kind == Kind::Agent {
            collapse_whitespace(event.body.trim())
        } else {
            event.body.clone()
        };
        listed = Event {
            body: crate::log::participant_text(event.sender.as_deref(), &body),
            paths: event
                .paths
                .iter()
                .map(|range| {
                    let offset = event.sender.as_deref().map_or(0, |sender| {
                        crate::log::participant_text(Some(sender), "").len() + 1
                    });
                    range.start + offset..range.end + offset
                })
                .collect(),
            ..event.clone()
        };
        &listed
    } else {
        event
    };
    let body_style = super::style::log_styles(&event.kind).1;
    if is_prose(&event.kind) {
        // Markdown blocks end at line breaks, so they are kept until rendering.
        let body = event
            .body
            .lines()
            .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
            .collect::<Vec<_>>()
            .join("\n");
        let paths = super::paths::text_paths(&body, cwd);
        let shortened = super::line_truncation::abbreviate_paths_in_text(&body, &paths, body_width);
        let mut spans = Vec::new();
        for line in super::markdown::render(&shortened, usize::MAX) {
            if line.spans.iter().all(|span| span.content.trim().is_empty()) {
                continue;
            }
            if !spans.is_empty() {
                spans.push(Span::styled(" ", body_style));
            }
            spans.extend(line.spans.into_iter().map(|mut span| {
                span.style = body_style.patch(span.style);
                span
            }));
        }
        FittedBody {
            line: Line::from(spans),
            source: body,
            paths: Vec::new(),
        }
    } else {
        let (text, paths) = display_paths(event, cwd);
        let shortened = super::line_truncation::abbreviate_paths(&text, &paths, body_width);
        FittedBody {
            line: Line::from(Span::styled(shortened, body_style)),
            source: text,
            paths,
        }
    }
}

/// An event's body with whitespace runs collapsed and home paths shown as
/// `~`, and the byte ranges of its paths in that text.
fn display_paths(event: &Event, cwd: &std::path::Path) -> (String, Vec<std::ops::Range<usize>>) {
    let mut text = String::new();
    let mut ranges = Vec::new();
    let mut end = 0;
    for range in &tool_paths(event, cwd) {
        text.push_str(&collapse_whitespace(&event.body[end..range.start]));
        let path = super::formatting::format_directory_display(std::path::Path::new(
            &event.body[range.clone()],
        ));
        ranges.push(text.len()..text.len() + path.len());
        text.push_str(&path);
        end = range.end;
    }
    text.push_str(&collapse_whitespace(&event.body[end..]));
    let trimmed = text.len() - text.trim_start().len();
    for range in &mut ranges {
        *range = range.start - trimmed..range.end - trimmed;
    }
    (text.trim().to_owned(), ranges)
}

/// The paths in a tool event's body: the ones its provider marked, and any
/// other path its free text mentions.
fn tool_paths(event: &Event, cwd: &std::path::Path) -> Vec<std::ops::Range<usize>> {
    let mut paths = event.paths.clone();
    for range in super::paths::text_paths(&event.body, cwd) {
        if !paths
            .iter()
            .any(|known| known.start < range.end && range.start < known.end)
        {
            paths.push(range);
        }
    }
    paths
}

fn collapse_whitespace(text: &str) -> String {
    let mut collapsed = String::new();
    for character in text.chars() {
        if !character.is_whitespace() {
            collapsed.push(character);
        } else if !collapsed.ends_with(' ') {
            collapsed.push(' ');
        }
    }
    collapsed
}

fn highlight_matches(line: &mut Line<'static>, search: &super::log_search::LogSearch) {
    if !search.query.is_empty() {
        let indices = search.indices(&line.to_string());
        highlight_indices(line, &indices);
    }
}

fn highlight_indices(line: &mut Line<'static>, indices: &[u32]) {
    if !indices.is_empty() {
        let mut offset = 0u32;
        line.spans = std::mem::take(&mut line.spans)
            .into_iter()
            .flat_map(|span| {
                let mut spans = Vec::new();
                for grapheme in unicode_segmentation::UnicodeSegmentation::graphemes(
                    span.content.as_ref(),
                    true,
                ) {
                    let style = if indices.binary_search(&offset).is_ok() {
                        span.style.bold()
                    } else {
                        span.style
                    };
                    spans.push(Span::styled(grapheme.to_owned(), style));
                    offset += 1;
                }
                spans
            })
            .collect();
    }
}

#[cfg(test)]
pub(super) mod log_markdown {
    use super::*;
    use ratatui::style::Modifier;

    #[test]
    fn queued_pasted_image_links_in_the_list_and_detail() {
        let delivery: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/claude-queued-image.jsonl"
        ))
        .unwrap();
        let text = &delivery["attachment"]["prompt"][0]["text"];
        let enqueue = serde_json::json!({
            "type": "queue-operation", "operation": "enqueue", "content": text,
            "timestamp": delivery["timestamp"]
        });
        let remove = serde_json::json!({
            "type": "queue-operation", "operation": "remove", "content": text,
            "commandUuid": delivery["attachment"]["source_uuid"],
            "timestamp": delivery["timestamp"]
        });
        let dir = tempfile::tempdir().unwrap();
        let transcript = dir.path().join("transcript.jsonl");
        std::fs::write(&transcript, format!("{enqueue}\n{remove}\n{delivery}\n")).unwrap();
        let events = crate::providers::claude::log::tail(&transcript, 20).unwrap();
        let event = &events[0];
        let image = &event.images[0];
        let layout = detail_layout(event, 200, dir.path(), DateTime::UNIX_EPOCH);
        assert!(
            layout
                .links
                .iter()
                .flatten()
                .any(|link| link.target == image.path)
        );
        let area = Rect::new(0, 0, 200, 1);
        let links = render_event(
            event,
            "19:58:06",
            false,
            area,
            &mut Buffer::empty(area),
            None,
            dir.path(),
        );
        assert!(links.iter().any(|(_, target)| target == &image.path));
    }

    #[test]
    fn task_notifications_show_output_content_without_repeating_summary() {
        let dir = tempfile::tempdir().unwrap();
        let events = notification_events(dir.path(), Some("all 3 tests passed"));
        assert_eq!(events.len(), 1);
        for event in events {
            let summary = "Background command \"等 tab 线程\" completed (exit code 0)";
            assert_eq!(event.kind, Kind::Tool);
            assert_eq!(event.body, summary);
            assert_eq!(event.calls[0].title, summary);
            assert_eq!(event.calls[0].id, "b9wi31ww0");
            let body = open_text(&event);
            assert!(body.contains("all 3 tests passed"), "{body}");
            assert!(!body.contains("task-id"), "{body}");
            assert!(!body.contains("status"), "{body}");
            assert!(!body.contains("output-file"), "{body}");
            assert!(!body.contains(summary), "{body}");
            assert!(!body.contains("tool-use-id"), "{body}");
        }
    }

    #[test]
    fn task_notifications_keep_a_linked_path_once_the_output_is_gone() {
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("b9wi31ww0.output");
        for event in notification_events(dir.path(), None) {
            let body = open_text(&event);
            assert!(
                body.contains(&format!("output-file: {}", output.display())),
                "{body}"
            );
            std::fs::write(&output, "done").unwrap();
            let layout = detail_layout(&event, 120, dir.path(), DateTime::UNIX_EPOCH);
            assert!(
                layout
                    .links
                    .iter()
                    .flatten()
                    .any(|link| link.target == output)
            );
            std::fs::remove_file(&output).unwrap();
        }
    }

    /// The notification fixture's events, with its output file inside `dir` holding `output`.
    pub(in crate::ui) fn notification_events(
        dir: &std::path::Path,
        output: Option<&str>,
    ) -> Vec<Event> {
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/claude-task-notifications.jsonl");
        let file = dir.join("b9wi31ww0.output");
        if let Some(output) = output {
            std::fs::write(&file, output).unwrap();
        }
        let transcript = dir.join("transcript.jsonl");
        std::fs::write(
            &transcript,
            std::fs::read_to_string(fixture).unwrap().replace(
                "/tmp/agents-notification/tasks/b9wi31ww0.output",
                &file.display().to_string(),
            ),
        )
        .unwrap();
        crate::providers::claude::log::tail(&transcript, 10).unwrap()
    }

    #[test]
    fn tool_without_calls_has_no_body_below_its_title() {
        let event = &Event {
            source: None,
            source_records: Vec::new(),
            sender: None,
            kind: Kind::Tool,
            body: "finished".into(),
            paths: Vec::new(),
            started_at: Utc::now(),
            ended_at: None,
            duration: None,
            previous_end: None,
            images: Vec::new(),
            calls: Vec::new(),
        };
        assert!(event.calls.is_empty());
        assert!(open_parts(event).is_empty());
        assert!(open_text(event).is_empty());
        assert_eq!(
            detail_layout(event, 100, std::path::Path::new("/"), DateTime::UNIX_EPOCH).height,
            0
        );
    }

    #[test]
    fn running_times_keep_zero_milliseconds() {
        let now = Utc::now();
        for nanos in [-1, 0, 499_999, 500_000] {
            let start = now - chrono::Duration::nanoseconds(nanos);
            assert_eq!(
                format_row_time(&RowTime::Running(start), now),
                if nanos < 500_000 { "0ms" } else { "1ms" }
            );
        }
    }

    fn body_buffer(kind: Kind, body: &str, width: u16) -> Buffer {
        let area = Rect::new(0, 0, width + 10, 1);
        let mut buffer = Buffer::empty(area);
        let event = Event {
            source: None,
            source_records: Vec::new(),
            sender: None,
            kind,
            body: body.into(),
            paths: Vec::new(),
            started_at: Utc::now(),
            ended_at: None,
            duration: None,
            previous_end: None,
            images: Vec::new(),
            calls: Vec::new(),
        };
        render_event(
            &event,
            "12:00:00",
            false,
            area,
            &mut buffer,
            None,
            std::path::Path::new("/"),
        );
        buffer
    }

    fn body_padding(buffer: &Buffer) -> usize {
        (0..buffer.area.width)
            .position(|x| buffer[(x, 0)].symbol() != " ")
            .unwrap_or(0)
    }

    fn body_text(buffer: &Buffer) -> String {
        buffer.content[..buffer.content.len() - 10]
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>()
            .trim_end()
            .to_owned()
    }

    #[test]
    fn real_transcript_subsecond_tool_time_is_right_aligned_in_milliseconds() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("projects/project");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(
            project.join("4165651d-b9e3-458d-8612-f8114e312363.jsonl"),
            include_str!("../../tests/fixtures/transcript.jsonl"),
        )
        .unwrap();
        let mut store = crate::providers::claude::ClaudeProvider::new(dir.path().to_owned());
        let mut session = crate::providers::Provider::refresh(&mut store, 1)
            .sessions
            .remove(0);
        session.id.provider = crate::providers::ProviderId::Codex;
        session.transcript = Some(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/codex-inner/inline-array.jsonl"),
        );
        let provider = crate::providers::codex::CodexProvider::new(
            std::path::PathBuf::new(),
            std::path::PathBuf::new(),
        );
        let events = crate::providers::Provider::log(&provider, &session, 100, false).unwrap();
        let event = &events[1];
        assert_eq!(event.kind, Kind::Tool);
        assert_eq!(
            event.started_at.to_rfc3339(),
            "2026-10-14T00:08:29.800660750+00:00"
        );
        assert_eq!(
            event.ended_at.unwrap().to_rfc3339(),
            "2026-10-14T00:08:30.171+00:00"
        );
        assert_eq!(
            event.duration,
            Some(chrono::Duration::nanoseconds(370_339_250))
        );
        let time = event_time(event, &session, true, event.ended_at.unwrap());
        assert_eq!(time, "370ms");
        let area = Rect::new(0, 0, 40, 1);
        let mut buffer = Buffer::empty(area);
        render_event(event, &time, true, area, &mut buffer, None, &session.cwd);
        let column: String = (32..40).map(|x| buffer[(x, 0)].symbol()).collect();
        assert_eq!(column, "   370ms");
    }

    #[test]
    fn working_subagent_tool_time_advances_without_process_and_ready_time_freezes() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("projects/project");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(
            project.join("4165651d-b9e3-458d-8612-f8114e312363.jsonl"),
            include_str!("../../tests/fixtures/transcript.jsonl"),
        )
        .unwrap();
        let mut store = crate::providers::claude::ClaudeProvider::new(dir.path().to_owned());
        let mut session = crate::providers::Provider::refresh(&mut store, 1)
            .sessions
            .remove(0);
        session.parent = Some(crate::providers::SessionKey {
            provider: crate::providers::ProviderId::Claude,
            id: "parent".into(),
        });
        session.process = None;
        session.status = SessionStatus::Working;
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(
            file.path(),
            format!(
                "{}{}\n",
                include_str!("../../tests/fixtures/transcript.jsonl"),
                include_str!("../../tests/fixtures/automode_blocked.jsonl")
                    .lines()
                    .next()
                    .unwrap()
            ),
        )
        .unwrap();
        let events = crate::providers::claude::log::tail(file.path(), 8).unwrap();
        let event = events.last().unwrap();
        assert_eq!(event.kind, Kind::Tool);
        assert!(event.ended_at.is_none());
        let previous = event.previous_end.unwrap();
        session.last_record_at = Some(previous + chrono::Duration::seconds(30));
        let now = previous + chrono::Duration::seconds(120);
        assert_eq!(event_time(event, &session, true, now), "2m 0s");
        session.status = SessionStatus::Ready;
        assert_eq!(event_time(event, &session, true, now), "30s");
    }

    #[test]
    fn file_change_without_start_shows_clock_instead_of_duration() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/codex-inner/file-change-without-start.jsonl");
        let provider = crate::providers::codex::CodexProvider::new(
            std::path::PathBuf::new(),
            std::path::PathBuf::new(),
        );
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("projects/project");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(
            project.join("4165651d-b9e3-458d-8612-f8114e312363.jsonl"),
            include_str!("../../tests/fixtures/transcript.jsonl"),
        )
        .unwrap();
        let mut store = crate::providers::claude::ClaudeProvider::new(dir.path().to_owned());
        let mut session = crate::providers::Provider::refresh(&mut store, 1)
            .sessions
            .remove(0);
        session.id.provider = crate::providers::ProviderId::Codex;
        session.transcript = Some(path);
        let events = crate::providers::Provider::log(&provider, &session, 100, false).unwrap();
        let event = &events[0];
        assert_eq!(
            row_time(event, &session, true),
            RowTime::Clock(event.started_at)
        );
        assert_eq!(
            row_time(event, &session, false),
            RowTime::Clock(event.started_at)
        );
    }

    #[test]
    fn row_time_changes_when_its_text_would() {
        let now = Utc::now();
        let ago = |seconds| RowTime::Ago(now - chrono::Duration::seconds(seconds));
        let secs = |seconds| Some(std::time::Duration::from_secs(seconds));
        assert_eq!(row_time_changes_in(&ago(12), now), secs(1));
        assert_eq!(row_time_changes_in(&ago(125), now), secs(55));
        assert_eq!(row_time_changes_in(&ago(7300), now), secs(3500));
        assert_eq!(row_time_changes_in(&RowTime::Running(now), now), secs(1));
        assert_eq!(
            row_time_changes_in(&RowTime::Settled(chrono::Duration::seconds(5)), now),
            None
        );
        assert_eq!(row_time_changes_in(&RowTime::Clock(now), now), None);
    }

    #[test]
    fn prose_uses_markdown_and_preserves_base_and_inline_styles() {
        let _guard = crate::theme::TEST_LOCK.lock().unwrap();
        for kind in [Kind::User, Kind::Assistant, Kind::Agent] {
            let base = super::super::style::log_styles(&kind).1;
            let buffer = body_buffer(
                kind,
                "plain `code` **bold** [link](https://example.com)",
                80,
            );
            let x = body_padding(&buffer) as u16;
            assert_eq!(body_text(&buffer).trim(), "plain code bold link");
            assert_eq!(buffer[(x, 0)].fg, base.fg.unwrap_or(Color::Reset));
            assert_eq!(buffer[(x + 6, 0)].fg, crate::theme::inline_code_color());
            assert_eq!(buffer[(x + 11, 0)].fg, base.fg.unwrap_or(Color::Reset));
            assert!(buffer[(x + 11, 0)].modifier.contains(Modifier::BOLD));
            assert_eq!(buffer[(x + 16, 0)].fg, base.fg.unwrap_or(Color::Reset));
            assert!(!buffer[(x + 16, 0)].modifier.contains(Modifier::UNDERLINED));
        }
    }

    #[test]
    fn prose_blocks_keep_their_own_styles_on_one_row() {
        let buffer = body_buffer(Kind::User, "# Title\n\nplain  text\n- item", 80);
        let x = body_padding(&buffer) as u16;
        assert_eq!(body_text(&buffer).trim(), "Title plain text • item");
        assert!(buffer[(x, 0)].modifier.contains(Modifier::UNDERLINED));
        assert!(!buffer[(x + 6, 0)].modifier.contains(Modifier::UNDERLINED));
        assert!(!buffer[(x + 6, 0)].modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn non_prose_rows_keep_markdown_literal() {
        for kind in [
            Kind::Tool,
            Kind::Session,
            Kind::QueuedUser,
            Kind::QueuedSystem,
            Kind::Error,
            Kind::Denied,
        ] {
            let base = super::super::style::log_styles(&kind).1;
            let buffer = body_buffer(kind, "  `code`\n **bold** [link](url)  ", 80);
            let x = body_padding(&buffer) as u16;
            assert_eq!(body_text(&buffer).trim(), "`code` **bold** [link](url)");
            assert_eq!(buffer[(x, 0)].fg, base.fg.unwrap_or(Color::Reset));
            assert!(!buffer[(x, 0)].modifier.contains(Modifier::BOLD));
        }
    }

    #[test]
    fn empty_prose_and_zero_body_width_are_safe() {
        for kind in [Kind::User, Kind::Assistant, Kind::Agent] {
            assert_eq!(body_text(&body_buffer(kind.clone(), " \n ", 20)), "");
            assert_eq!(body_text(&body_buffer(kind, "`code`", 0)), "");
        }
    }

    #[test]
    fn prose_paths_shrink_before_the_line_is_cut() {
        for kind in [Kind::User, Kind::Assistant, Kind::Agent] {
            let buffer = body_buffer(
                kind,
                "Report: /private/tmp/claude-501/report.md 1. done",
                35,
            );
            assert_eq!(body_text(&buffer), "Report: /p…/t…/c…/report.md 1. done");
        }
    }

    #[test]
    fn prose_and_tool_rows_find_the_same_paths() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src/ui")).unwrap();
        std::fs::write(dir.path().join("src/ui/log.rs"), "").unwrap();
        let report = dir.path().join("report.md");
        std::fs::write(&report, "").unwrap();
        let body = format!("改了 src/ui/log.rs 和 {}，已发送", report.display());
        for kind in [Kind::User, Kind::Tool] {
            let event = Event {
                source: None,
                source_records: Vec::new(),
                sender: None,
                kind: kind.clone(),
                body: body.clone(),
                paths: Vec::new(),
                started_at: Utc::now(),
                ended_at: None,
                duration: None,
                previous_end: None,
                images: Vec::new(),
                calls: Vec::new(),
            };
            let fitted = fitted_body_line(&event, 30, dir.path());
            let links = if is_prose(&kind) {
                super::super::links::text_links(&fitted.source, dir.path(), &[])
            } else {
                super::super::links::path_links(&fitted.source, &fitted.paths, dir.path())
            };
            let targets: Vec<_> = links.into_iter().map(|link| link.target).collect();
            assert_eq!(
                targets,
                [dir.path().join("src/ui/log.rs"), report.clone()],
                "{kind:?}"
            );
            assert!(
                fitted.line.to_string().contains("report.md，已发送"),
                "{kind:?}"
            );
        }
    }

    #[test]
    fn cut_inline_code_keeps_colour_through_the_ellipsis() {
        let _guard = crate::theme::TEST_LOCK.lock().unwrap();
        for kind in [Kind::User, Kind::Assistant, Kind::Agent] {
            let buffer = body_buffer(kind, "ok `abcdefghijklmnop`", 10);
            assert_eq!(body_text(&buffer), "ok abcdef…");
            for x in 3..10 {
                assert_eq!(buffer[(x, 0)].fg, crate::theme::inline_code_color());
            }
        }
    }

    /// A row merging an Edit and a Read call, fitted to `width`.
    fn merged_tool_row(width: usize) -> String {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        for (id, name, path) in [
            ("a", "Edit", "/opt/work/agents/src/ui/tests.rs"),
            ("b", "Read", "/opt/work/agents/src/providers/codex/tests.rs"),
        ] {
            let record = serde_json::json!({
                "type":"assistant", "timestamp":"2026-10-06T16:39:01Z", "uuid":id,
                "message":{"id":"batch", "content":[
                    {"type":"tool_use", "id":id, "name":name, "input":{"file_path":path}}
                ]}
            });
            std::io::Write::write_all(&mut file, format!("{record}\n").as_bytes()).unwrap();
        }
        let events = crate::providers::claude::log::tail(file.path(), 1).unwrap();
        let fitted = fitted_body_line(&events[0], width, std::path::Path::new("/"));
        fitted
            .line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    #[test]
    fn merged_tool_row_abbreviates_every_path() {
        assert_eq!(
            merged_tool_row(70),
            "Edit /o…/w…/a…/s…/ui/tests.rs，Read /o…/w…/a…/s…/p…/codex/tests.rs"
        );
    }

    #[test]
    fn merged_tool_row_keeps_every_file_name_whole() {
        assert_eq!(
            merged_tool_row(40),
            "Edit /o…/w…/a…/s…/ui/tests.rs，Read /o…/w…/a…/s…/p…/c…/tests.rs"
        );
    }

    #[test]
    #[ignore = "manual per-frame markdown timing; run with --release --ignored --nocapture"]
    fn markdown_frame_timing() {
        use std::{hint::black_box, time::Instant};

        let bodies: Vec<_> = (0..60)
            .map(|row| {
                format!(
                    "Checked `src/session_{row}.rs` and **verified** the update. See [the report](https://example.com/reports/{row}) for details and the remaining checks."
                )
            })
            .collect();
        assert!(bodies.iter().all(|body| (80..=200).contains(&body.len())));
        let start = Instant::now();
        for _ in 0..1000 {
            for body in &bodies {
                black_box(super::super::markdown::render(black_box(body), 100));
            }
        }
        eprintln!(
            "60 rows, width 100, 1000 frames: {:.2} us/frame",
            start.elapsed().as_secs_f64() * 1_000_000.0 / 1000.0
        );
    }
}

#[cfg(test)]
#[path = "log_reading_tests.rs"]
mod reading_tests;
