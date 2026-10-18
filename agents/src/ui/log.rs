use super::detail_model::{DetailColumns, Section, detail_columns};
use super::formatting::{format_log_duration, format_relative_time};
use super::line_truncation::truncate_line_with_ellipsis_if_overflow;
use super::*;
use crate::log::{Event, Kind};
use chrono::{DateTime, Local, Utc};

const CLOCK_FORMAT: &str = "%H:%M:%S";
pub(super) const CLOCK_WIDTH: usize = 8;

/// Rows above the scrolling sections: the top rule.
pub(super) const LOG_TOP: u16 = 1;

pub(super) fn search_input_rect(area: Rect) -> Rect {
    super::render::row(area, area.height.saturating_sub(1), 1)
}

impl View {
    pub(super) fn detail_text(&self, event: &Event) -> String {
        open_text(event)
    }

    fn selected_detail_layout(
        &self,
        event: &Event,
        columns: DetailColumns,
        cwd: &Path,
        now: DateTime<Utc>,
    ) -> DetailLayout {
        let sections =
            super::tool_detail::detail(event, String::new(), Style::new().dim()).sections;
        detail_parts_layout(event, columns, cwd, now, sections)
    }

    pub(super) fn render_log(&self, area: Rect, buf: &mut Buffer, now: DateTime<Utc>) {
        self.viewport.borrow_mut().log_text = text_selection::TextRegion::default();
        if !self.selecting_list(text_selection::Panel::LogList) {
            self.viewport.borrow_mut().log_list_text = text_selection::TextRegion::default();
        }
        {
            let mut viewport = self.viewport.borrow_mut();
            viewport.log = area;
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
        let max_scroll = self.log_max_scroll();
        {
            let mut viewport = self.viewport.borrow_mut();
            viewport.log_scroll = viewport.log_scroll.min(max_scroll);
        }
        self.reveal_log_selection();
        let scroll = self.log_scroll();
        let matching: Vec<_> = self.shown_log_events().collect();
        let selected_index = self.log_selected;
        let start = scroll.min(matching.len());
        let end = (start
            + usize::from(
                area.height
                    .saturating_sub(u16::from(self.log_search.active())),
            ))
        .min(matching.len());
        let mut rows = Vec::new();
        let mut drawn_links = Vec::new();
        for (index, &(log_index, event)) in matching[start..end].iter().enumerate() {
            let last = start + index + 1 == matching.len();
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
            if selected_index == Some(start + index) {
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
        if self.render_terminal(area, buf, now) {
            return;
        }
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

    /// Draws the button that jumps back to the newest row at the time column’s right edge while
    /// an older event is selected and new events have arrived.
    pub(super) fn render_log_jump(&self, area: Rect, buf: &mut Buffer) {
        let mut viewport = self.viewport.borrow_mut();
        viewport.log_jump = Rect::default();
        if self.following_log() {
            viewport.log_unseen = 0;
            return;
        }
        if viewport.log_unseen == 0 {
            return;
        }
        let label = format!(" ↓ {} New ", viewport.log_unseen);
        let width = (label.width() as u16).min(area.width.saturating_sub(2));
        let right = viewport.log.right().saturating_add(1).min(area.right());
        let jump = Rect::new(right.saturating_sub(width), area.y, width, 1);
        Clear.render(jump, buf);
        Line::from(Span::styled(label, Style::new().dim())).render(jump, buf);
        viewport.log_jump = jump;
    }

    /// Draws the selected event as scrolling sections, including their titles and times.
    fn render_event_detail(
        &self,
        event: &Event,
        time: &str,
        area: Rect,
        buf: &mut Buffer,
        cwd: &std::path::Path,
        now: DateTime<Utc>,
    ) {
        let list = self.center_layout(buf.area).log;
        let columns = detail_columns(area, list);
        let mut body_area =
            super::render::row(columns.text, LOG_TOP, area.height.saturating_sub(LOG_TOP));
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
        let mut viewport = self.viewport.borrow_mut();
        let mut layout = match viewport.log_detail_layout.take() {
            Some(layout)
                if layout.columns == columns
                    && layout.height == height
                    && layout.theme_generation == super::highlight::theme_generation() =>
            {
                layout
            }
            _ => {
                let mut layout = self.selected_detail_layout(event, columns, cwd, now);
                layout.height = height;
                layout
            }
        };
        if (!shows_calls(event) || (event.calls.len() == 1 && event.calls[0].started_at.is_none()))
            && let Some((_, span)) = layout.times.first_mut()
        {
            span.content = time.to_owned().into();
            span.style = super::style::log_time_style(&event.kind, self.is_newest_shown());
        }
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
        viewport.log_detail_columns = Some(columns);
        let rows = self.detail_scroll_layout(event, &layout, body_area, &mut viewport);
        let scroll = rows.scroll;
        let cuts = rows.cuts;
        let kept = rows.kept;
        if overflow && height > 0 {
            let thumb = (height * height)
                .div_ceil(height + viewport.log_detail_max)
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
        let visible_rows = kept
            .iter()
            .skip(scroll)
            .take(height)
            .copied()
            .collect::<Vec<_>>();
        let rows: Vec<_> = visible_rows
            .iter()
            .enumerate()
            .map(|(row, &source)| {
                let (segment, cells) = &layout.rows[source];
                let mut cells = cells.clone();
                if cuts.contains(&source) {
                    cells.retain(|(cells, _)| cells.end < body_area.width);
                }
                text_selection::DrawnRow {
                    segment: *segment,
                    x: body_area.x,
                    y: body_area.y + row as u16,
                    cells,
                }
            })
            .collect();
        viewport.log_links = rows
            .iter()
            .filter(|row| !row.cells.is_empty())
            .flat_map(|row| {
                super::links::drawn_links(std::slice::from_ref(row), &layout.links[row.segment])
            })
            .collect();
        viewport.log_text = text_selection::TextRegion {
            area: body_area,
            sources: layout.sources.clone(),
            rows,
        };
        let mut visible = visible_rows
            .iter()
            .map(|&source| {
                let mut line = layout.lines[source].clone();
                if cuts.contains(&source) {
                    line = super::line_truncation::truncate_line_to_width(
                        line,
                        usize::from(body_area.width.saturating_sub(1)),
                    );
                    line.spans.push("…".dim());
                }
                line
            })
            .collect::<Vec<_>>();
        let times = layout.times.clone();
        layout.visible = visible_rows.clone();
        viewport.log_detail_layout = Some(layout);
        drop(viewport);
        for line in &mut visible {
            highlight_matches(line, &self.log_detail_search);
        }
        Paragraph::new(visible).render(body_area, buf);
        for (row, time) in times {
            if let Some(row) = visible_rows.iter().position(|&source| source == row) {
                let time_area = Rect::new(
                    columns.time.x,
                    body_area.y + row as u16,
                    columns.time.width,
                    1,
                );
                Line::from(time).right_aligned().render(time_area, buf);
            }
        }
        self.highlight_text_selection(text_selection::Panel::Log, buf);
    }

    fn detail_scroll_layout(
        &self,
        event: &Event,
        layout: &DetailLayout,
        body_area: Rect,
        viewport: &mut Viewport,
    ) -> super::details_scroll::DetailsLayout {
        let record = event
            .source_records
            .first()
            .map(|record| (&record.path, record.offset));
        let key = detail_key(&(
            self.selected_id(),
            &event.source,
            record,
            (event.source.is_none() && record.is_none()).then_some((&event.kind, event.started_at)),
        ));
        let switched = viewport.log_detail_key != Some(key);
        if switched {
            let mut left = std::mem::take(&mut viewport.log_detail_state);
            left.left_keys = Some(std::mem::take(&mut left.keys));
            if let Some(previous) = viewport.log_detail_key.replace(key) {
                viewport.log_detail_saved.insert(previous, left);
            }
            viewport.log_detail_state = viewport.log_detail_saved.remove(&key).unwrap_or_default();
        }
        let height = usize::from(body_area.height);
        let requested_scroll = viewport.log_detail_scroll;
        let state = &mut viewport.log_detail_state;
        state.content_top = body_area.y;
        state.prepare(layout.body_keys.clone());
        let position = state.position();
        // Search and drag selection address the full source layout with a plain offset.
        let external_scroll = (!switched || requested_scroll > 0) && requested_scroll != position;
        let wants = layout
            .bodies
            .iter()
            .map(|body| body.len())
            .collect::<Vec<_>>();
        let static_rows = layout.lines.len() - wants.iter().sum::<usize>();
        let allocations = if height >= static_rows + wants.iter().filter(|&&rows| rows > 0).count()
            && !self.log_detail_search.active()
            && !external_scroll
            && state.offset == 0
        {
            Some(super::render::allocate(&wants, height - static_rows))
        } else {
            state.offset = if switched && !external_scroll {
                state.offset
            } else {
                requested_scroll
            };
            None
        };
        let rows = state.layout(
            &vec![1; layout.lines.len()],
            &layout.bodies,
            allocations.as_deref(),
            0..0,
            height,
        );
        viewport.log_detail_max = rows.max;
        viewport.log_detail_scroll = rows.scroll;
        rows
    }

    /// Refresh geometry before consuming input, including inputs batched ahead of a draw.
    pub(super) fn prepare_log_detail_scroll(&self) {
        let Some(event) = self.visible_log_event() else {
            return;
        };
        let Some(session) = self.selected_row() else {
            return;
        };
        let mut viewport = self.viewport.borrow_mut();
        let Some(columns) = viewport.log_detail_columns else {
            return;
        };
        let area = viewport.log_detail;
        let mut body_area =
            super::render::row(columns.text, LOG_TOP, area.height.saturating_sub(LOG_TOP));
        if self.log_detail_search.active() {
            body_area.height = body_area.height.saturating_sub(1);
        }
        let layout = viewport.log_detail_layout.take().unwrap_or_else(|| {
            let mut layout = self.selected_detail_layout(event, columns, &session.cwd, Utc::now());
            layout.height = usize::from(body_area.height);
            layout
        });
        self.detail_scroll_layout(event, &layout, body_area, &mut viewport);
        viewport.log_detail_layout = Some(layout);
    }

    pub(super) fn jump_detail_match(&mut self) {
        let Some(event) = self.visible_log_event() else {
            return;
        };
        if self.log_detail_search.query.is_empty() {
            return;
        }
        let viewport = self.viewport.borrow();
        let cwd = self
            .selected_row()
            .map(|session| session.cwd.as_path())
            .unwrap_or(Path::new("."));
        let layout = self.selected_detail_layout(
            event,
            detail_columns(viewport.log_detail, viewport.log),
            cwd,
            DateTime::UNIX_EPOCH,
        );
        let found = layout
            .lines
            .iter()
            .zip(&layout.rows)
            .position(|(line, (id, cells))| {
                if !self.log_detail_search.indices(&line.to_string()).is_empty() {
                    return true;
                }
                // The ellipsis maps to the hidden suffix, so these bounds cover the full row.
                let bytes = cells
                    .iter()
                    .map(|(_, bytes)| bytes.clone())
                    .reduce(|a, b| a.start.min(b.start)..a.end.max(b.end));
                bytes.is_some_and(|bytes| {
                    !self
                        .log_detail_search
                        .indices(&layout.sources[*id].text[bytes])
                        .is_empty()
                })
            });
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

    pub(super) fn following_log(&self) -> bool {
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
        let offset = selected;
        let max = self.log_max_scroll();
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
        viewport.log_scroll = viewport.log_scroll.min(max);
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
    pub(super) fn move_log_selection(&mut self, forward: bool) {
        self.pending_click = None;
        self.log_background_return = None;
        self.log_background_event = None;
        if forward && self.log_pending_older > 0 {
            self.log_pending_older -= 1;
            return;
        }
        let Some(index) = self.log_selected else {
            self.show_newest_log();
            return;
        };
        if !forward && index == 0 {
            if self.log_search.active() {
                return;
            }
            self.log_pending_older += 1;
            return;
        }
        let next = if forward {
            index.saturating_add(1)
        } else {
            index.saturating_sub(1)
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
        let prepended = self
            .shown_log_events()
            .filter(|(full, _)| *full < update.prepended)
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
        let max_scroll = self.log_max_scroll();
        if update.reset {
            let mut viewport = self.viewport.borrow_mut();
            viewport.log_scroll = max_scroll;
            viewport.log_unseen = 0;
            viewport.log_detail_scroll = 0;
        } else if !following || self.mouse_press.is_some() {
            let mut viewport = self.viewport.borrow_mut();
            viewport.log_scroll += prepended;
            viewport.log_unseen += added;
            drop(viewport);
            self.shift_list_selection(text_selection::Panel::LogList, prepended);
        } else {
            self.viewport.borrow_mut().log_scroll = max_scroll;
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
        if let Some(screen) = self.shown_terminal() {
            return screen
                .lock()
                .expect("terminal screen lock")
                .loading_remaining(std::time::Instant::now())
                .map(|remaining| remaining.min(super::tool_detail::running_redraw_in(now)));
        }
        let viewport = self.viewport.borrow();
        let layout = viewport.log_detail_layout.as_ref()?;
        layout
            .running
            .iter()
            .any(|row| layout.visible.contains(row))
            .then(|| super::tool_detail::running_redraw_in(now))
    }

    /// How long until the newest visible row's time changes, or `None` if it never does.
    pub(crate) fn log_redraw_in(&self, now: DateTime<Utc>) -> Option<std::time::Duration> {
        let session = self.selected_row()?;
        if self.log_session.as_ref() != Some(&session.id) {
            return None;
        }
        let event = self.shown_log_events().last()?.1;
        if self.viewport.borrow().log.height > 0 && self.log_scroll() < self.log_max_scroll() {
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
    if event.completed_time
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
    let background = event.background;
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

/// Centers a one-row label of `width` on the panel midline, clear of the clock column's gap.
#[cfg(test)]
pub(super) fn log_content_centered(area: Rect, width: u16) -> Rect {
    let (content, _) = log_row_areas(area);
    let x = super::centering::centered_x(area, content, width);
    Rect::new(x, area.y, width.min(content.right().saturating_sub(x)), 1).intersection(area)
}

/// Reserves the same trailing clock column and separator for every row.
pub(super) fn log_row_areas(area: Rect) -> (Rect, Rect) {
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
    let body_x = super::centering::centered_x(
        area,
        content_area,
        body.width().min(usize::from(u16::MAX)) as u16,
    );
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
        let mut maps = super::text_selection_layout::map_lines_in(
            &fitted.source,
            std::slice::from_ref(&body),
            is_prose(&event.kind),
            Some(cwd),
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
        Rect::new(body_x, area.y, content_area.right() - body_x, 1),
        buf,
    );
    links
}

/// The screen cells of one drawn line, each with the source bytes it shows.
type Cells = Vec<(std::ops::Range<u16>, std::ops::Range<usize>)>;

/// The open log event laid out for one panel size and highlight theme.
pub(super) struct DetailLayout {
    columns: DetailColumns,
    height: usize,
    theme_generation: u64,
    lines: Vec<Line<'static>>,
    /// Animated output rows, updated without repeating syntax highlighting.
    running: Vec<usize>,
    /// Source rows drawn by the last render, after folding and window clipping.
    visible: Vec<usize>,
    /// Each line's text part and the cells mapping it back to that part's bytes.
    rows: Vec<(usize, Cells)>,
    sources: Vec<text_selection::Source>,
    /// The links in each text part.
    links: Vec<Vec<super::links::Link>>,
    /// Section title times, drawn separately in the time column.
    times: Vec<(usize, Span<'static>)>,
    bodies: Vec<std::ops::Range<usize>>,
    body_keys: Vec<u64>,
}

#[cfg(test)]
fn detail_layout(
    event: &Event,
    width: u16,
    cwd: &std::path::Path,
    now: DateTime<Utc>,
) -> DetailLayout {
    let view = View::new(Grouping::Project, false);
    let panel = Rect::new(0, 0, width, 1);
    let list = view.center_layout(Rect::new(0, 0, width, 45)).log;
    detail_parts_layout(
        event,
        detail_columns(panel, list),
        cwd,
        now,
        super::tool_detail::detail(event, String::new(), Style::new().dim()).sections,
    )
}

/// Clip a rendered row while keeping its ellipsis selectable as the hidden source suffix.
pub(super) fn truncate_detail_line(
    line: Line<'static>,
    cells: &mut Cells,
    width: usize,
) -> Line<'static> {
    let overflow = line.width() > width;
    let mut line = truncate_line_with_ellipsis_if_overflow(line, width);
    let visible = if overflow && width > 0 {
        line.width().saturating_sub(1)
    } else {
        width
    } as u16;
    let hidden = cells.iter().filter(|(range, _)| range.end > visible);
    let start = hidden.clone().map(|(_, bytes)| bytes.start).min();
    let end = hidden.map(|(_, bytes)| bytes.end).max();
    cells.retain(|(range, _)| range.end <= visible);
    if overflow
        && width > 0
        && let (Some(start), Some(end)) = (start, end)
    {
        cells.push((width as u16 - 1..width as u16, start..end));
    }
    if overflow && width > 0 && line.width() < width {
        let ellipsis = line.spans.pop().expect("overflow has an ellipsis");
        // A wide grapheme can leave a cell unused; the ellipsis still sits at the text edge.
        line.spans
            .push(Span::raw(" ".repeat(width - line.width() - 1)));
        line.spans.push(ellipsis);
    }
    line
}

fn detail_parts_layout(
    event: &Event,
    columns: DetailColumns,
    cwd: &Path,
    now: DateTime<Utc>,
    sections: Vec<Section>,
) -> DetailLayout {
    let width = usize::from(columns.text.width);
    let body_style = Style::default();
    let mut layout = DetailLayout {
        columns,
        height: 0,
        theme_generation: super::highlight::theme_generation(),
        lines: Vec::new(),
        running: Vec::new(),
        visible: Vec::new(),
        rows: Vec::new(),
        sources: Vec::new(),
        links: Vec::new(),
        times: Vec::new(),
        bodies: Vec::new(),
        body_keys: Vec::new(),
    };
    for (section_index, section) in sections.into_iter().enumerate() {
        if section_index > 0 {
            layout.lines.push(Line::default());
            // Keep source indices stable while preserving the section break in cross-part copies.
            if let Some(source) = layout.sources.last_mut() {
                source.text = format!("{}\n", source.text).into();
            }
            layout.rows.push((layout.sources.len(), Vec::new()));
        }
        {
            let title = section.title;
            let id = layout.sources.len();
            let source = title.source();
            let (line, cells) = title.line_in(columns, cwd);
            if let Some(time) = title.time {
                layout
                    .times
                    .push((layout.lines.len(), Span::styled(time, title.meta_style)));
            }
            layout.lines.push(line);
            layout.rows.push((id, cells));
            layout
                .links
                .push(super::links::text_links(&source, cwd, &[]));
            layout.sources.push(text_selection::Source {
                id,
                text: source.into(),
                label: copied_label(event),
            });
        }
        layout.lines.push(Line::default());
        layout.rows.push((layout.sources.len() - 1, Vec::new()));
        let body_start = layout.lines.len();
        let source_start = layout.sources.len();
        for group in section.groups {
            let rendered: Vec<_> = group
                .parts
                .into_iter()
                .map(|part| {
                    let lines = part.lines_in(
                        usize::from(columns.reading_width.min(columns.text.width)),
                        body_style,
                        now,
                        cwd,
                    );
                    (part, lines)
                })
                .collect();
            let group_width = rendered
                .iter()
                .flat_map(|(_, lines)| lines.iter())
                .map(Line::width)
                .max()
                .unwrap_or(0)
                .min(width);
            let group_x =
                super::centering::centered_x(columns.panel, columns.text, group_width as u16);
            let padding = usize::from(group_x - columns.text.x);
            for (part, lines) in rendered {
                let id = layout.sources.len();
                let mut maps = part.maps_in(&lines, cwd);
                let first = layout.lines.len();
                if part.running() && width > 0 {
                    layout.running.push(first);
                }
                let mut links = if part.markdown()
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
                for link in &part.file_links {
                    if let Some(target) = super::vscode::file_line_link(&link.path, link.line) {
                        links.push(super::links::Link {
                            range: link.range.clone(),
                            target: target.into(),
                        });
                    }
                }
                if !shows_calls(event) {
                    part.remap_links(&event.body, &event.paths, cwd, &mut links);
                }
                for (line, cells) in lines.into_iter().zip(&mut maps) {
                    let mut line = truncate_detail_line(line, cells, width);
                    if padding > 0 {
                        line.spans.insert(0, Span::raw(" ".repeat(padding)));
                        for (range, _) in cells.iter_mut() {
                            range.start += padding as u16;
                            range.end += padding as u16;
                        }
                    }
                    layout.lines.push(line);
                }
                layout
                    .rows
                    .extend(maps.into_iter().map(|cells| (id, cells)));
                layout.sources.push(text_selection::Source {
                    id,
                    text: part.source.into(),
                    label: copied_label(event),
                });
                layout.links.push(links);
            }
        }
        let body = body_start..layout.lines.len();
        layout.body_keys.push(detail_key(
            &layout.sources[source_start..]
                .iter()
                .map(|source| &source.text)
                .collect::<Vec<_>>(),
        ));
        layout.bodies.push(body);
    }
    layout
}

fn detail_key(value: &impl std::hash::Hash) -> u64 {
    use std::hash::Hasher;
    let mut hasher = std::hash::DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

/// Whether an event draws its tool calls when open, which makes its row open on click even
/// when its body fits the row.
pub(super) fn shows_calls(event: &Event) -> bool {
    !event.calls.is_empty()
        && (matches!(event.kind, Kind::Tool | Kind::Error | Kind::Denied)
            || event.calls.iter().any(|call| call.show_details))
}

/// The copied event text, including the titles separating merged tool calls.
pub(super) fn open_text(event: &Event) -> String {
    let detail = super::tool_detail::detail(event, String::new(), Style::new().dim());
    let titled = shows_calls(event) && detail.sections.len() > 1;
    let mut sources = Vec::new();
    for (index, section) in detail.sections.into_iter().enumerate() {
        if index > 0 {
            sources.push(String::new());
        }
        if titled {
            sources.push(section.title.source());
        }
        for group in section.groups {
            sources.extend(group.parts.into_iter().map(|part| part.source));
        }
    }
    // Display-only arrows and running indicators have no source bytes.
    while sources.last().is_some_and(String::is_empty) {
        sources.pop();
    }
    sources
        .into_iter()
        .skip_while(String::is_empty)
        .collect::<Vec<_>>()
        .join("\n")
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
        let shortened =
            super::line_truncation::abbreviate_paths_in_text(&body, &paths, body_width, cwd);
        let mut spans = Vec::new();
        for line in super::markdown::render(&shortened, usize::MAX) {
            // Blank separators and drawn rules carry no text for a single row.
            let text = line.to_string();
            if text
                .chars()
                .all(|ch| ch.is_whitespace() || matches!(ch, '─' | '┼'))
            {
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
        let shortened = super::line_truncation::abbreviate_paths(&text, &paths, body_width, cwd);
        FittedBody {
            line: super::style::exit_code_line(
                shortened,
                match event.calls.as_slice() {
                    [call] => call.exit_code,
                    _ => None,
                },
                body_style,
            ),
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
    fn task_notifications_show_a_placeholder_once_the_output_is_gone() {
        let dir = tempfile::tempdir().unwrap();
        for event in notification_events(dir.path(), None) {
            let body = open_text(&event);
            assert!(body.contains("File no longer exists"), "{body}");
            assert!(!body.contains("output-file:"), "{body}");
            let layout = detail_layout(&event, 120, dir.path(), DateTime::UNIX_EPOCH);
            assert!(layout.links.iter().flatten().next().is_none());
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
            completed_time: false,
            background: false,
            source_records: Vec::new(),
            sender: None,
            detail_body: None,
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
        let detail = super::tool_detail::detail(event, String::new(), Style::new().dim());
        assert_eq!(detail.sections.len(), 1);
        assert_eq!(detail.sections[0].title.title, "finished");
        assert!(
            detail.sections[0]
                .groups
                .iter()
                .all(|group| group.parts.is_empty())
        );
        assert!(open_text(event).is_empty());
        assert_eq!(
            detail_layout(event, 100, std::path::Path::new("/"), DateTime::UNIX_EPOCH)
                .lines
                .len(),
            2
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
            completed_time: false,
            background: false,
            source_records: Vec::new(),
            sender: None,
            detail_body: None,
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
            std::path::Path::new("/project"),
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
    fn log_list_short_row_centers_on_panel_midline() {
        let body = "x".repeat(20);
        let buffer = body_buffer(Kind::Tool, &body, 90);
        let start = body_padding(&buffer);
        assert_eq!(start + body.width() / 2, usize::from(buffer.area.width / 2));
        assert_eq!(body_text(&buffer).trim(), body);
    }

    #[test]
    fn log_list_near_full_row_moves_left_to_time_gap_without_truncation() {
        let body = "x".repeat(86);
        let buffer = body_buffer(Kind::Tool, &body, 90);
        let (span, time) = log_row_areas(buffer.area);
        assert_eq!(
            body_padding(&buffer) + body.width(),
            usize::from(span.right())
        );
        assert_eq!(body_text(&buffer).trim(), body);
        assert_eq!(buffer[(span.right(), 0)].symbol(), " ");
        assert_eq!(buffer[(time.x - 1, 0)].symbol(), " ");
    }

    #[test]
    fn log_list_jump_centers_on_panel_midline_and_stays_before_time_gap() {
        let panel = Rect::new(10, 3, 100, 1);
        for label in [" ↓ 1 New ", " ↓ 12 New "] {
            let width = label.width() as u16;
            let jump = log_content_centered(panel, width);
            assert_eq!(jump.x + width / 2, panel.x + panel.width / 2);
            assert_eq!(jump.width, width);
        }
        assert_eq!(log_content_centered(panel, 86), Rect::new(14, 3, 86, 1));
        assert_eq!(log_content_centered(panel, 120), log_row_areas(panel).0);
        assert_eq!(log_content_centered(Rect::new(10, 3, 8, 1), 10).width, 0);
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
        assert!(buffer[(x, 0)].modifier.contains(Modifier::BOLD));
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
                completed_time: false,
                background: false,
                source_records: Vec::new(),
                sender: None,
                detail_body: None,
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

    #[test]
    fn prose_rows_flatten_tables_without_their_header_rule() {
        let buffer = body_buffer(Kind::Assistant, "| a | b |\n|---|---|\n| c | d |", 40);
        assert_eq!(body_text(&buffer).trim_start(), "a │ b c │ d");
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
        let fitted = fitted_body_line(&events[0], width, std::path::Path::new("/project"));
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
            "Edit /o…/w…/a…/s…/ui/tests.rs, Read /o…/w…/a…/s…/p…/codex/tests.rs"
        );
    }

    #[test]
    fn merged_tool_row_keeps_every_file_name_whole() {
        assert_eq!(
            merged_tool_row(40),
            "Edit /o…/w…/a…/s…/ui/tests.rs, Read /o…/w…/a…/s…/p…/c…/tests.rs"
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

#[cfg(test)]
mod panel_detail_tests {
    use super::super::detail_model::Group;
    use super::super::tool_detail::Part;
    use super::*;

    fn fixture_events(fixture: &str) -> Vec<Event> {
        let dir = tempfile::tempdir().unwrap();
        let transcript = dir.path().join("transcript.jsonl");
        std::fs::write(&transcript, fixture).unwrap();
        crate::providers::claude::log::tail(&transcript, 200).unwrap()
    }

    fn fixture_event() -> Event {
        fixture_events(include_str!("../../tests/fixtures/transcript.jsonl"))
            .into_iter()
            .find(|event| event.kind == Kind::Assistant)
            .unwrap()
    }

    fn columns(width: u16) -> DetailColumns {
        detail_columns(Rect::new(10, 2, width, 20), Rect::new(0, 0, 50, 20))
    }

    fn layout_part(columns: DetailColumns, part: Part) -> DetailLayout {
        detail_parts_layout(
            &fixture_event(),
            columns,
            Path::new("/"),
            DateTime::UNIX_EPOCH,
            vec![Section {
                title: super::tool_detail::detail(
                    &fixture_event(),
                    String::new(),
                    Style::new().dim(),
                )
                .sections
                .remove(0)
                .title,
                groups: vec![Group { parts: vec![part] }],
            }],
        )
    }

    #[test]
    fn each_section_title_has_a_display_only_gap_before_its_body() {
        let mut events =
            fixture_events(include_str!("../../tests/fixtures/claude-tool-calls.jsonl"));
        events.push(fixture_event());
        for event in events {
            let sections =
                super::tool_detail::detail(&event, String::new(), Style::new().dim()).sections;
            let mut id = 0;
            let titles: Vec<_> = sections
                .iter()
                .map(|section| {
                    let title = id;
                    id += 1 + section
                        .groups
                        .iter()
                        .map(|group| group.parts.len())
                        .sum::<usize>();
                    title
                })
                .collect();
            let layout = detail_layout(&event, 200, Path::new("/"), DateTime::UNIX_EPOCH);
            assert_eq!(layout.lines.len(), layout.rows.len());
            for id in titles {
                let row = layout
                    .rows
                    .iter()
                    .position(|(source, cells)| *source == id && !cells.is_empty())
                    .unwrap();
                assert_eq!(layout.lines[row + 1].to_string(), "");
                assert!(layout.rows[row + 1].1.is_empty());
                assert_eq!(layout.rows[row + 2].0, id + 1);
            }
        }
    }

    #[test]
    fn section_titles_scroll_without_a_merged_event_header_or_rules() {
        let event = fixture_events(include_str!("../../tests/fixtures/claude-tool-calls.jsonl"))
            .into_iter()
            .find(|event| event.calls.len() > 1)
            .unwrap();
        let panel = Rect::new(0, 0, 200, 12);
        let mut buffer = Buffer::empty(Rect::new(0, 0, 201, 12));
        let view = View::new(Grouping::Project, false);
        view.render_event_detail(
            &event,
            "12:34:56",
            panel,
            &mut buffer,
            Path::new("/"),
            DateTime::UNIX_EPOCH,
        );
        let row = |buffer: &Buffer, y| {
            (0..200)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        };
        assert!(
            !row(&buffer, LOG_TOP).contains("12:34:56"),
            "merged event time must not be drawn"
        );
        let viewport = view.viewport.borrow();
        let layout = viewport.log_detail_layout.as_ref().unwrap();
        let mut expected = Buffer::empty(buffer.area);
        layout.lines[0].clone().render(
            super::super::render::row(layout.columns.text, LOG_TOP, 1),
            &mut expected,
        );
        for x in layout.columns.text.x..layout.columns.text.right() {
            assert_eq!(
                buffer[(x, LOG_TOP)].symbol(),
                expected[(x, LOG_TOP)].symbol()
            );
        }
        assert!(
            layout
                .lines
                .iter()
                .all(|line| !line.to_string().contains('─'))
        );
        drop(viewport);
        view.viewport.borrow_mut().log_detail_scroll = 1;
        let mut scrolled = Buffer::empty(buffer.area);
        view.render_event_detail(
            &event,
            "12:34:56",
            panel,
            &mut scrolled,
            Path::new("/"),
            DateTime::UNIX_EPOCH,
        );
        assert_ne!(row(&buffer, LOG_TOP), row(&scrolled, LOG_TOP));
    }

    #[test]
    fn preformatted_rows_fill_text_and_ellipsis_copies_the_full_source() {
        let event = fixture_event();
        let source = event.body.lines().next().unwrap();
        for width in [200, 35] {
            let columns = columns(width);
            let layout = layout_part(columns, Part::text(source.into(), false));
            assert_eq!(layout.lines.len(), 3);
            assert_eq!(layout.lines[2].width(), usize::from(columns.text.width));
            assert!(layout.lines[2].to_string().ends_with('…'));
            let cells = &layout.rows[2].1;
            assert_eq!(cells.first().unwrap().0.start, 0);
            assert_eq!(cells.last().unwrap().0.end, columns.text.width);
            assert_eq!(
                cells
                    .iter()
                    .map(|(_, bytes)| &layout.sources[1].text[bytes.clone()])
                    .collect::<String>(),
                layout.sources[1].text.as_ref()
            );
        }
    }

    #[test]
    fn prose_wraps_at_the_smaller_reading_or_text_width() {
        let event = fixture_event();
        let source = event
            .body
            .lines()
            .find(|line| line.starts_with("界面"))
            .unwrap();
        for width in [200, 35] {
            let columns = columns(width);
            let layout = layout_part(columns, Part::prose(source.into(), false));
            assert!(layout.lines.len() > 1);
            let wrap_width = columns.reading_width.min(columns.text.width);
            for (_, cells) in layout.rows.iter().skip(2) {
                let start = cells.first().unwrap().0.start;
                let end = cells.last().unwrap().0.end;
                assert!(end - start <= wrap_width);
                assert!(columns.text.x + end <= columns.time.x - 2);
            }
            let longest = layout
                .rows
                .iter()
                .skip(2)
                .map(|(_, cells)| cells.last().unwrap().0.end - cells.first().unwrap().0.start)
                .max()
                .unwrap();
            assert!(longest >= wrap_width - 2);
        }
    }

    #[test]
    fn parts_share_the_group_origin() {
        for width in [200, 35] {
            let columns = columns(width);
            let source = "first\nsecond";
            let layout = detail_parts_layout(
                &fixture_event(),
                columns,
                Path::new("/"),
                DateTime::UNIX_EPOCH,
                vec![Section {
                    title: super::tool_detail::detail(
                        &fixture_event(),
                        String::new(),
                        Style::new().dim(),
                    )
                    .sections
                    .remove(0)
                    .title,
                    groups: vec![Group {
                        parts: vec![
                            Part::text(source.into(), false),
                            Part::text("third".into(), false),
                        ],
                    }],
                }],
            );
            let expected_x = columns.panel.x + columns.panel.width / 2 - 3;
            assert_eq!(
                layout
                    .rows
                    .iter()
                    .skip(2)
                    .map(|(_, cells)| columns.text.x + cells[0].0.start)
                    .collect::<Vec<_>>(),
                vec![expected_x; 3]
            );
        }
    }

    #[test]
    fn event_time_ends_at_panel_edge_and_body_leaves_the_time_column_empty() {
        let event = fixture_event();
        for width in [200, 35] {
            let panel = Rect::new(10, 2, width, 20);
            let columns = columns(width);
            let mut buffer = Buffer::empty(Rect::new(0, 0, width + 20, 24));
            let view = View::new(Grouping::Project, false);
            view.render_event_detail(
                &event,
                "12:34:56",
                panel,
                &mut buffer,
                Path::new("/"),
                DateTime::UNIX_EPOCH,
            );
            assert_eq!(
                (columns.time.x..columns.time.right())
                    .map(|x| buffer[(x, panel.y + LOG_TOP)].symbol())
                    .collect::<String>(),
                "12:34:56"
            );
            for y in panel.y + LOG_TOP + 1..panel.bottom() {
                assert!(
                    (columns.text.right()..panel.right()).all(|x| buffer[(x, y)].symbol() == " ")
                );
            }
        }
    }
    #[test]
    fn tool_times_use_the_panel_edge_and_the_title_metadata_style() {
        let dir = tempfile::tempdir().unwrap();
        let event = fixture_events(include_str!("../../tests/fixtures/claude-tool-calls.jsonl"))
            .into_iter()
            .find(|event| event.calls.len() > 1)
            .unwrap();
        for width in [200, 35] {
            let panel = Rect::new(10, 2, width, 100);
            let mut buffer = Buffer::empty(Rect::new(0, 0, width + 20, 104));
            let view = View::new(Grouping::Project, false);
            view.render_event_detail(
                &event,
                "12:34:56",
                panel,
                &mut buffer,
                dir.path(),
                DateTime::UNIX_EPOCH,
            );
            let viewport = view.viewport.borrow();
            let layout = viewport.log_detail_layout.as_ref().unwrap();
            assert!(!layout.times.is_empty());
            assert_eq!(layout.columns.time.right(), panel.right());
            for (row, time) in &layout.times {
                let y = panel.y + LOG_TOP + *row as u16;
                let start = panel.right() - time.width() as u16;
                assert_eq!(
                    (start..panel.right())
                        .map(|x| buffer[(x, y)].symbol())
                        .collect::<String>(),
                    time.content.as_ref()
                );
                assert!(
                    buffer[(start, y)]
                        .style()
                        .add_modifier
                        .contains(time.style.add_modifier)
                );
                assert!(
                    (layout.columns.text.right()..layout.columns.time.x)
                        .all(|x| buffer[(x, y)].symbol() == " ")
                );
            }
        }
    }

    #[test]
    fn shell_prefixes_count_toward_group_width_and_ellipsis_maps_hidden_command_bytes() {
        let mut event = fixture_events(include_str!(
            "../../tests/fixtures/claude-inline-script.jsonl"
        ))
        .into_iter()
        .find(|event| event.calls.iter().any(|call| call.name == "Bash"))
        .unwrap();
        let source = "echo 123456789012345678901234567890 && pwd; whoami";
        event.calls.truncate(1);
        event.calls[0].input = vec![crate::log::Section::Shell {
            prefix: 0,
            text: source.into(),
        }];
        for width in [200, 35] {
            let columns = columns(width);
            let layout = detail_parts_layout(
                &event,
                columns,
                Path::new("/"),
                DateTime::UNIX_EPOCH,
                super::super::tool_detail::detail(&event, String::new(), Style::new().dim())
                    .sections,
            );
            let id = layout
                .sources
                .iter()
                .find(|part| part.text.as_ref() == source)
                .unwrap()
                .id;
            let rows: Vec<_> = layout
                .rows
                .iter()
                .enumerate()
                .filter(|(_, (part, _))| *part == id)
                .collect();
            assert_eq!(rows.len(), 3);
            let group_width = ("echo 123456789012345678901234567890".len() + 2)
                .min(usize::from(columns.text.width));
            let padding = super::super::centering::centered_x(
                columns.panel,
                columns.text,
                group_width as u16,
            ) - columns.text.x;
            for ((row, (_, cells)), (prefix, command)) in rows.iter().zip([
                ("┌ ", "echo 123456789012345678901234567890"),
                ("└ ", "pwd"),
                ("  ", "whoami"),
            ]) {
                let line = &layout.lines[*row];
                assert!(
                    line.to_string()
                        .starts_with(&format!("{}{prefix}", " ".repeat(usize::from(padding))))
                );
                assert_eq!(cells.first().unwrap().0.start, padding + 2);
                assert!(line.width() <= usize::from(columns.text.width));
                assert_eq!(
                    cells
                        .iter()
                        .map(|(_, bytes)| &source[bytes.clone()])
                        .collect::<String>(),
                    command
                );
            }
            let first_line = &layout.lines[rows[0].0];
            assert_eq!(first_line.to_string().ends_with('…'), width == 35);
            if width == 35 {
                assert_eq!(rows[0].1.1.last().unwrap().0.end, columns.text.width);
            }
        }
    }
}

#[cfg(test)]
#[path = "log_detail_regression_tests.rs"]
mod detail_regression_tests;

#[cfg(test)]
#[path = "log_path_tests.rs"]
mod path_tests;
