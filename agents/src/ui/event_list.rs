use super::event_detail::{EventDetailArea, EventDetailColumns, Section, event_detail_columns};
use super::formatting::{format_event_duration, format_relative_time};
use super::line_truncation::truncate_line_with_ellipsis_if_overflow;
use super::*;
use crate::event::{Event, Kind};
use chrono::{DateTime, Utc};

const TURN_BRACKET: [&str; 3] = ["┐", "│", "┘"];

pub(super) const CLOCK_FORMAT: &str = "%H:%M:%S";
pub(super) const CLOCK_WIDTH: usize = 8;

/// Rows above the scrolling sections: the top rule.
pub(super) const EVENT_TOP: u16 = 1;

pub(super) fn search_input_rect(area: Rect) -> Rect {
    super::render::row(area, area.height.saturating_sub(1), 1)
}

impl View {
    pub(super) fn refresh_transcript(&mut self) {
        let mode = if self.terminal_requested() {
            self.event_detail.return_mode
        } else {
            self.event_detail.mode
        };
        if mode != task_ui::EventDetailMode::Transcript {
            return;
        }
        let Some(event) = self.visible_event() else {
            self.event_detail.transcript = None;
            return;
        };
        if self
            .event_detail
            .transcript
            .as_ref()
            .is_some_and(|cached| cached.records == event.source_records)
        {
            return;
        }
        self.event_detail.transcript = Some(task_ui::Transcript {
            records: event.source_records.clone(),
            text: event.raw_text().map_err(|error| error.to_string()),
        });
        self.viewport.borrow_mut().event_detail_layout = None;
    }

    /// Resolve recorded agent settings with the same catalog as session borders and pickers.
    pub(super) fn display_event<'a>(&self, event: &'a Event) -> std::borrow::Cow<'a, Event> {
        if !event.calls.iter().any(|call| call.model_settings.is_some()) {
            return std::borrow::Cow::Borrowed(event);
        }
        let mut display = event.clone();
        for call in &mut display.calls {
            if let Some(settings) = &call.model_settings {
                if let Some(model) = settings.model.as_deref().filter(|model| !model.is_empty()) {
                    let model = self.model_text(settings.provider, model);
                    call.title.push_str(&format!(" · {model}"));
                    if call.name == "advisor" {
                        call.summary.push_str(&format!(" · {model}"));
                        display.body.clone_from(&call.summary);
                    }
                }
                if let Some(effort) = settings
                    .effort
                    .as_deref()
                    .filter(|effort| !effort.is_empty())
                {
                    call.title.push_str(&format!(" · {effort}"));
                }
            }
        }
        std::borrow::Cow::Owned(display)
    }

    pub(super) fn event_detail_text(&self, event: &Event) -> String {
        if self.event_detail.mode == task_ui::EventDetailMode::Transcript {
            return self
                .event_detail
                .transcript
                .as_ref()
                .expect("transcript loaded")
                .text
                .clone()
                .unwrap_or_else(|error| format!("Cannot read transcript records: {error}"));
        }
        open_text(&self.display_event(event))
    }

    fn event_detail_scroll_key(&self, event: &Event) -> u64 {
        let record = event
            .source_records
            .first()
            .map(|record| (&record.path, record.offset));
        event_detail_key(&(
            self.selected_id(),
            self.event_detail.mode,
            matches!(self.selected_event_row(), Some(EventRow::Pinned(_))),
            &event.source,
            record,
            (event.source.is_none() && record.is_none()).then_some((&event.kind, event.started_at)),
        ))
    }

    fn restore_event_detail_zoom(&self, event: &Event) {
        self.restore_event_detail();
        self.event_detail
            .scroll
            .borrow_mut()
            .switch_zooms(self.event_detail_scroll_key(event));
    }

    fn selected_event_detail_layout(
        &self,
        event: &Event,
        columns: EventDetailColumns,
        cwd: &Path,
        now: DateTime<Utc>,
    ) -> EventDetailLayout {
        self.restore_event_detail_zoom(event);
        if self.event_detail.mode == task_ui::EventDetailMode::Transcript {
            let transcript = self
                .event_detail
                .transcript
                .as_ref()
                .expect("transcript loaded");
            let sections = super::tool_detail::transcript_content(
                &transcript.text,
                self.selected_id()
                    .expect("detail belongs to a session")
                    .provider,
                &event.kind,
            )
            .sections;
            return event_detail_parts_layout(event, columns, cwd, now, sections);
        }
        let display = self.display_event(event);
        let sections =
            super::tool_detail::event_detail_content(&display, String::new(), Style::new().dim())
                .sections;
        let zooms = &self.event_detail.scroll.borrow().zooms;
        let mut images = self.inline_images.borrow_mut();
        event_detail_parts_layout_with_images(event, columns, cwd, now, sections, |index, image| {
            images.as_mut()?.size(
                image,
                columns.text.width,
                zooms.get(&index).copied().unwrap_or_default(),
            )
        })
    }

    pub(super) fn render_event(&self, area: Rect, buf: &mut Buffer, now: DateTime<Utc>) {
        self.viewport.borrow_mut().event_text = text_selection::TextRegion::default();
        if !self.selecting_list(text_selection::Panel::EventList) {
            self.viewport.borrow_mut().event_list_text = text_selection::TextRegion::default();
        }
        {
            let mut viewport = self.viewport.borrow_mut();
            viewport.event_panel = area;
            viewport.event_rows.clear();
            viewport.event_timeline_rows = 0;
            viewport.event_separator = None;
            viewport.event_jump = Rect::default();
            viewport.event_links.clear();
        }
        if area.is_empty() {
            return;
        }
        let Some(session) = self.selected_row() else {
            return;
        };
        if self.event_session.as_ref() != Some(&session.id) {
            return;
        }
        if self.event_list.search.active() && area.height > 1 {
            let input = search_input_rect(area);
            if self.event_list.search.editing {
                super::render::render_input_row(
                    &format!("Search {PROMPT_MARKER} "),
                    &self.event_list.search.query,
                    input,
                    buf,
                );
            } else {
                Line::from(self.event_list.search.query.text().to_owned())
                    .dim()
                    .right_aligned()
                    .render(input, buf);
            }
        }
        let max_scroll = self.event_max_scroll();
        self.event_list
            .scroll
            .set(self.event_list.scroll.get().min(max_scroll));
        self.reveal_event_selection();
        let scroll = self.event_scroll();
        let matching: Vec<_> = self.shown_event_rows().collect();
        let selected_index = self.event_list.cursor;
        let pinned_count = self.shown_pinned_count();
        let height = usize::from(
            area.height
                .saturating_sub(u16::from(self.event_list.search.active())),
        );
        let (pinned_height, timeline_height) = self.event_section_heights(height);
        let separator = usize::from(pinned_height > 0);
        let timeline_count = matching.len() - pinned_count;
        let start = scroll.min(timeline_count);
        let end = (start + timeline_height).min(timeline_count);
        let mut rows = Vec::new();
        let mut drawn_links = Vec::new();
        let pinned_start = self
            .event_list
            .pinned_scroll
            .get()
            .min(pinned_count.saturating_sub(pinned_height));
        self.event_list.pinned_scroll.set(pinned_start);
        let visible = (start..end)
            .chain(timeline_count + pinned_start..timeline_count + pinned_start + pinned_height);
        self.viewport.borrow_mut().event_separator = (separator > 0).then_some(timeline_height);
        for (index, position) in visible.enumerate() {
            let (event_index, event) = matching[position];
            let y = if position < timeline_count {
                index
            } else {
                timeline_height + separator + index - (end - start)
            };
            let pinned = matches!(event_index, super::task_ui::EventRow::Pinned(_));
            let last = match event_index {
                super::task_ui::EventRow::Pinned(_) => false,
                super::task_ui::EventRow::Timeline(full) => {
                    if self.event_list.sort == EventSort::Duration {
                        full + 1 == self.event_panel.len()
                    } else {
                        position + 1 == timeline_count
                    }
                }
            };
            let sort = if pinned {
                EventSort::Time
            } else {
                self.event_list.sort
            };
            let time = event_time(event, session, last, now, sort);
            let display = self.display_event(event);
            let links = render_event(
                &display,
                &time,
                last,
                super::render::row(area, y as u16, 1),
                buf,
                Some(&self.event_list.search),
                &session.cwd,
            );
            if selected_index == Some(position) {
                super::render::fill_list_row(
                    super::render::row(area, y as u16, 1),
                    buf,
                    super::style::cursor_style(),
                );
            }
            if !pinned
                && session.parent.is_none()
                && self.event_list.sort != EventSort::Duration
                && let Some(turn) = event.turn
            {
                let same_turn = |other: usize| {
                    matching.get(other).is_some_and(|(row, event)| {
                        matches!(row, EventRow::Timeline(_))
                            && event.turn.is_some_and(|other| other.id == turn.id)
                    })
                };
                let previous = position.checked_sub(1).is_some_and(same_turn);
                let next = same_turn(position + 1);
                if previous || next {
                    let bracket = if !previous && turn.started {
                        0
                    } else if !next && turn.complete {
                        2
                    } else {
                        1
                    };
                    let position = (area.right(), area.y + y as u16);
                    if buf.area.contains(position.into()) {
                        buf[position]
                            .set_symbol(TURN_BRACKET[bracket])
                            .set_style(super::tool_detail::bracket_style());
                    }
                }
            }
            rows.push(event_index);
            drawn_links.extend(links);
        }
        let mut text_area = super::render::row(
            area,
            0,
            if separator > 0 {
                (timeline_height + pinned_height + separator) as u16
            } else {
                rows.len() as u16
            },
        );
        text_area.width = text_area.width.saturating_sub(1);
        {
            let mut viewport = self.viewport.borrow_mut();
            viewport.event_timeline_rows = end - start;
            viewport.event_rows = rows;
            viewport.event_links = drawn_links;
        }
        self.capture_list_text(text_selection::Panel::EventList, text_area, scroll, buf);
        super::links::underline(buf, &self.viewport.borrow().event_links);
    }

    pub(super) fn visible_event(&self) -> Option<&Event> {
        self.event_list.background.as_ref().or_else(|| {
            self.event_list
                .cursor
                .and_then(|index| self.shown_event_rows().nth(index).map(|(_, event)| event))
        })
    }

    pub(super) fn render_event_detail(
        &self,
        areas: impl Into<EventDetailArea>,
        buf: &mut Buffer,
        now: DateTime<Utc>,
    ) {
        self.viewport.borrow_mut().event_detail_images.clear();
        let areas = areas.into();
        let area = areas.content;
        if self.event_session.as_ref() == self.selected_id() {
            self.restore_event_detail();
        }
        super::render::line("─".repeat(usize::from(area.width)).dim(), area, buf);
        if self.render_terminal(area, buf, now) {
            return;
        }
        let Some(session) = self.selected_row() else {
            return;
        };
        if self.event_session.as_ref() != Some(&session.id) {
            return;
        }
        let Some(event) = self.visible_event() else {
            return;
        };
        let time = event_time(
            event,
            session,
            self.is_newest_shown(),
            now,
            self.event_list.sort,
        );
        let list_links = self.viewport.borrow().event_links.clone();
        self.render_event_detail_content(event, &time, areas, buf, &session.cwd, now);
        self.viewport.borrow_mut().event_links.extend(list_links);
    }

    /// The following event's first title, once the current event's last row is visible.
    pub(super) fn next_event_title(&self) -> Option<Line<'static>> {
        let viewport = self.viewport.borrow();
        let layout = viewport.event_detail_layout.as_ref()?;
        if layout.visible.last().copied()? + 1 != layout.lines.len() {
            return None;
        }
        let width = usize::from(layout.columns.text.width);
        let session = self.selected_row()?;
        let (_, event) = self.shown_event_rows().nth(self.event_list.cursor? + 1)?;
        let title =
            super::tool_detail::event_detail_content(event, String::new(), Style::default())
                .sections
                .into_iter()
                .next()?
                .title;
        Some(title.title_in(width, &session.cwd).0)
    }

    /// Draws the duration label or the New button at the time column’s right edge.
    /// The button appears while an older event is selected and new events have arrived.
    pub(super) fn render_event_jump(&self, area: Rect, buf: &mut Buffer) {
        let mut viewport = self.viewport.borrow_mut();
        viewport.event_jump = Rect::default();
        viewport.event_label = Rect::default();
        if self.event_list.restoring {
            return;
        }
        let label = if self.event_list.sort == EventSort::Duration {
            self.event_list.unseen.set(0);
            " duration".to_owned()
        } else {
            if self.following_event() {
                self.event_list.unseen.set(0);
                return;
            }
            if self.event_list.unseen.get() == 0 {
                return;
            }
            format!(" ↓ {} New", self.event_list.unseen.get())
        };
        let width = (label.width() as u16).min(area.width.saturating_sub(2));
        let right = event_row_areas(self.center_layout(buf.area).event_panel)
            .1
            .right();
        let jump = Rect::new(right.saturating_sub(width), area.y, width, 1);
        Clear.render(jump, buf);
        Line::from(Span::styled(label, style::rule_label_style())).render(jump, buf);
        for x in jump.right()..area.right() {
            buf[(x, area.y)]
                .set_symbol(if x == jump.right() { "╶" } else { "─" })
                .set_style(Style::default().dim());
        }
        viewport.event_label = jump;
        if self.event_list.sort == EventSort::Time {
            viewport.event_jump = jump;
        }
    }

    /// Draws the selected event as scrolling sections, including their titles and times.
    fn render_event_detail_content(
        &self,
        event: &Event,
        time: &str,
        areas: impl Into<EventDetailArea>,
        buf: &mut Buffer,
        cwd: &std::path::Path,
        now: DateTime<Utc>,
    ) {
        self.restore_event_detail_zoom(event);
        let areas = areas.into();
        let area = areas.content;
        let list = self.center_layout(buf.area).event_panel;
        let columns = event_detail_columns(area, list);
        let mut body_area = super::render::row(
            columns.text,
            EVENT_TOP,
            area.height.saturating_sub(EVENT_TOP),
        );
        if self.event_detail.search.active() {
            body_area.height = body_area.height.saturating_sub(1);
            let input = search_input_rect(area);
            if self.event_detail.search.editing {
                super::render::render_input_row(
                    &format!("Search {PROMPT_MARKER} "),
                    &self.event_detail.search.query,
                    input,
                    buf,
                );
            } else {
                Line::from(self.event_detail.search.query.text().to_owned())
                    .dim()
                    .right_aligned()
                    .render(input, buf);
            }
        }
        let height = usize::from(body_area.height);
        let mut viewport = self.viewport.borrow_mut();
        let mut layout = match viewport.event_detail_layout.take() {
            Some(layout)
                if layout.columns == columns
                    && layout.height == height
                    && layout.theme_generation == super::highlight::theme_generation() =>
            {
                layout
            }
            _ => {
                let mut layout = self.selected_event_detail_layout(event, columns, cwd, now);
                layout.height = height;
                layout
            }
        };
        if self.event_detail.mode != task_ui::EventDetailMode::Transcript
            && (!shows_calls(event)
                || (event.calls.len() == 1
                    && (self.event_list.sort == EventSort::Duration
                        || event.calls[0].started_at.is_none())))
            && let Some((_, span)) = layout.times.first_mut()
        {
            span.content = time.to_owned().into();
            span.style = super::style::event_time_style(&event.kind, self.is_newest_shown());
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
        viewport.event_detail_columns = Some(columns);
        let rows = self.event_detail_scroll_layout(event, &layout, body_area, &mut viewport);
        let scroll = rows.scroll;
        let cuts = rows
            .cuts
            .into_iter()
            .filter(|&row| layout.cut_text(row))
            .collect::<Vec<_>>();
        let kept = rows.kept;
        if overflow && height > 0 {
            let thumb = (height * height)
                .div_ceil(height + viewport.event_detail_max)
                .max(1)
                .min(height);
            let top = scroll * (height - thumb) / viewport.event_detail_max.max(1);
            for row in 0..height {
                buf[(scrollbar_x, body_area.y + row as u16)].set_symbol(
                    if (top..top + thumb).contains(&row) {
                        "▕"
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
                    index: source,
                    continuation: layout.continuations.get(&source).copied(),
                    x: body_area.x,
                    y: body_area.y + row as u16,
                    cells,
                    ..Default::default()
                }
            })
            .collect();
        viewport.event_links = rows
            .iter()
            .filter(|row| !row.cells.is_empty())
            .flat_map(|row| {
                super::links::drawn_links(std::slice::from_ref(row), &layout.links[row.segment])
            })
            .collect();
        viewport.event_text = text_selection::TextRegion {
            area: body_area,
            sources: layout.sources.clone(),
            rows,
        };
        let mut visible = visible_rows
            .iter()
            .map(|&source| {
                if !layout.cut_text(source) {
                    return Line::default();
                }
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
        // The section on the top row keeps its title on the top edge once its title scrolls off.
        viewport.event_detail_footer_title = visible_rows.first().and_then(|&top| {
            layout
                .titles
                .iter()
                .rev()
                .find(|(row, _)| *row <= top)
                .filter(|(row, _)| *row < top)
                .map(|(_, title)| (title.clone(), columns))
        });
        let drawn_images = layout.drawn_images(&visible_rows, body_area);
        layout.visible = visible_rows.clone();
        viewport.event_detail_layout = Some(layout);
        drop(viewport);
        for line in &mut visible {
            highlight_matches(line, &self.event_detail.search);
        }
        // Text keeps its inset and clock column; changed rows fill the pane including padding.
        for (row, line) in visible.iter().enumerate() {
            if let Some(background) = line.style.bg {
                buf.set_style(
                    Rect::new(
                        areas.panel.x,
                        body_area.y + row as u16,
                        areas.panel.width,
                        1,
                    ),
                    Style::new().bg(background),
                );
            }
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
        self.highlight_text_selection(text_selection::Panel::Event, buf);
        super::links::underline(buf, &self.viewport.borrow().event_links);
        if let Some(images) = self.inline_images.borrow_mut().as_mut() {
            let detail = self.event_detail.scroll.borrow();
            for drawn in &drawn_images {
                // The image replaces its selectable placeholder text on screen.
                Clear.render(drawn.area, buf);
                let zoom = detail.zooms.get(&drawn.index).copied().unwrap_or_default();
                images.draw(&drawn.image, zoom, drawn.first_row, drawn.area, buf);
            }
            self.viewport.borrow_mut().event_detail_images = drawn_images;
        }
    }

    fn event_detail_scroll_layout(
        &self,
        event: &Event,
        layout: &EventDetailLayout,
        body_area: Rect,
        viewport: &mut Viewport,
    ) -> super::section_scroll::SectionLayout {
        self.restore_event_detail();
        let mut detail = self.event_detail.scroll.borrow_mut();
        detail.event = super::task_ui::EventId::from_event(event);
        let key = self.event_detail_scroll_key(event);
        let switched = detail.key != Some(key);
        let follow_output = std::mem::take(&mut detail.follow_output) && !switched;
        let entry = detail.entry.take().filter(|_| switched);
        if switched {
            let mut left = std::mem::take(&mut detail.state);
            left.left_keys = Some(std::mem::take(&mut left.keys));
            if let Some(previous) = detail.key.replace(key) {
                detail.saved.insert(previous, left);
            }
            let saved = detail.saved.remove(&key);
            // Entering by wheel starts at the end the wheel came from.
            detail.state = saved.filter(|_| entry.is_none()).unwrap_or_default();
        }
        let height = usize::from(body_area.height);
        let requested_scroll = detail.offset;
        let state = &mut detail.state;
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
        let allocations = if self.event_detail.mode != task_ui::EventDetailMode::Transcript
            && height >= static_rows + wants.iter().filter(|&&rows| rows > 0).count()
            && !self.event_detail.search.active()
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
        let mut rows = state.layout(
            &vec![1; layout.lines.len()],
            &layout.bodies,
            allocations.as_deref(),
            0..0,
            height,
        );
        if entry == Some(false) || follow_output {
            if allocations.is_some() {
                state.reveal = state.hidden.clone();
            } else {
                state.offset = rows.max;
            }
            rows = state.layout(
                &vec![1; layout.lines.len()],
                &layout.bodies,
                allocations.as_deref(),
                0..0,
                height,
            );
        }
        viewport.event_detail_max = rows.max;
        detail.offset = rows.scroll;
        rows
    }

    /// Refresh geometry before consuming input, including inputs batched ahead of a draw.
    pub(super) fn prepare_event_detail_scroll(&self) {
        let Some(event) = self.visible_event() else {
            return;
        };
        let Some(session) = self.selected_row() else {
            return;
        };
        self.restore_event_detail_zoom(event);
        let mut viewport = self.viewport.borrow_mut();
        let Some(columns) = viewport.event_detail_columns else {
            return;
        };
        let area = viewport.event_detail;
        let mut body_area = super::render::row(
            columns.text,
            EVENT_TOP,
            area.height.saturating_sub(EVENT_TOP),
        );
        if self.event_detail.search.active() {
            body_area.height = body_area.height.saturating_sub(1);
        }
        let layout = viewport.event_detail_layout.take().unwrap_or_else(|| {
            let mut layout =
                self.selected_event_detail_layout(event, columns, &session.cwd, Utc::now());
            layout.height = usize::from(body_area.height);
            layout
        });
        self.event_detail_scroll_layout(event, &layout, body_area, &mut viewport);
        viewport.event_detail_layout = Some(layout);
    }

    pub(super) fn jump_event_detail_match(&mut self) {
        let Some(event) = self.visible_event() else {
            return;
        };
        if self.event_detail.search.query.is_empty() {
            return;
        }
        let viewport = self.viewport.borrow();
        let cwd = self
            .selected_row()
            .map(|session| session.cwd.as_path())
            .unwrap_or(Path::new("."));
        let layout = self.selected_event_detail_layout(
            event,
            event_detail_columns(viewport.event_detail, viewport.event_panel),
            cwd,
            DateTime::UNIX_EPOCH,
        );
        let found = layout
            .lines
            .iter()
            .zip(&layout.rows)
            .position(|(line, (id, cells))| {
                if !self
                    .event_detail
                    .search
                    .indices(&line.to_string())
                    .is_empty()
                {
                    return true;
                }
                // The ellipsis maps to the hidden suffix, so these bounds cover the full row.
                let bytes = cells
                    .iter()
                    .map(|(_, bytes)| bytes.clone())
                    .reduce(|a, b| a.start.min(b.start)..a.end.max(b.end));
                bytes.is_some_and(|bytes| {
                    !self
                        .event_detail
                        .search
                        .indices(&layout.sources[*id].text[bytes])
                        .is_empty()
                })
            });
        drop(viewport);
        if let Some(line) = found {
            self.event_detail.scroll.borrow_mut().offset = line;
        }
    }

    /// Selects an index in the shown list; `None` selects its newest row.
    pub(super) fn select_event(&mut self, index: Option<usize>) {
        self.event_list.pinned_following = false;
        let background = self.event_list.background.take().is_some();
        let len = self.matching_event_count();
        let index = len.checked_sub(1).map(|last| {
            index
                .unwrap_or_else(|| self.newest_event_position().unwrap_or(last))
                .min(last)
        });
        if self.event_list.cursor != index || background {
            self.exit_image_zoom();
            self.cancel_event_detail_selection();
            self.event_list.cursor = index;
            self.event_detail.search.clear();
            let mut viewport = self.viewport.borrow_mut();
            viewport.event_detail_layout = None;
            let mut detail = self.event_detail.scroll.borrow_mut();
            detail.offset = 0;
            detail.entry = None;
        }
        if self.following_event() {
            self.event_list.unseen.set(0);
        }
        self.refresh_transcript();
    }

    #[cfg(test)]
    pub(super) fn select_fixture_event(&mut self, event: Option<Event>) {
        let index = event.map(|event| {
            let full = self
                .event_panel
                .iter()
                .position(|candidate| candidate == &event)
                .unwrap_or_else(|| {
                    self.event_panel.push(event);
                    self.prepare_event_search();
                    self.event_panel.len() - 1
                });
            self.shown_position(full).expect("fixture is shown")
        });
        self.select_event(index);
    }

    pub(super) fn following_event(&self) -> bool {
        !matches!(self.selected_event_row(), Some(EventRow::Pinned(_)))
            && (self.event_list.cursor.is_none()
                || self.event_list.cursor == self.newest_event_position())
            && self.event_list.background_return.is_none()
    }

    fn following_timeline(&self) -> bool {
        self.following_event()
            || (self.event_list.sort == EventSort::Time
                && self.event_list.pinned_following
                && matches!(self.selected_event_row(), Some(EventRow::Pinned(_))))
    }

    /// Keeps the cursor in view without moving a list being dragged for text selection.
    pub(super) fn reveal_event_selection(&self) {
        if self.mouse_press.is_some()
            || (self.selecting_list(text_selection::Panel::EventList)
                && self.text_selection_dragging())
        {
            return;
        }
        let Some(selected) = self.event_list.cursor else {
            return;
        };
        let timeline = self.shown_events().count();
        if let Some(offset) = selected.checked_sub(timeline) {
            if self.following_timeline() {
                self.event_list.scroll.set(self.event_max_scroll());
            }
            let height = self.event_pinned_height();
            if height > 0 {
                let scroll = self.event_list.pinned_scroll.get();
                self.event_list.pinned_scroll.set(if offset < scroll {
                    offset
                } else {
                    scroll.max((offset + 1).saturating_sub(height))
                });
            }
            return;
        }
        let offset = selected;
        let max = self.event_max_scroll();
        let height = self.event_timeline_height();
        if height == 0 {
            return;
        }
        self.event_list
            .scroll
            .set(self.event_list.scroll.get().min(max));
        if offset < self.event_list.scroll.get() {
            self.event_list.scroll.set(offset);
        } else if offset >= self.event_list.scroll.get() + height {
            self.event_list.scroll.set(offset + 1 - height);
        }
    }

    pub(super) fn navigate_event(&mut self, code: KeyCode) {
        match code {
            KeyCode::Up | KeyCode::Down => self.move_event_selection(code == KeyCode::Down),
            // Paging and Home/End stay in the timeline; from a pin they start at its newest event.
            KeyCode::PageUp | KeyCode::PageDown => {
                let forward = code == KeyCode::PageDown;
                if matches!(self.selected_event_row(), Some(EventRow::Pinned(_))) {
                    let Some(newest) = self.newest_event_position() else {
                        return;
                    };
                    self.navigate_to_event(newest);
                    if forward {
                        return;
                    }
                }
                for _ in 0..self.event_timeline_height().max(1) {
                    if forward
                        && self.event_list.pending_older == 0
                        && self.event_list.cursor.is_some()
                        && self.event_list.cursor >= self.newest_event_position()
                    {
                        break;
                    }
                    self.move_event_selection(forward);
                }
            }
            KeyCode::Home => self.show_first_event(),
            KeyCode::End => {
                self.event_list.pending_older = 0;
                if let Some(newest) = self.newest_event_position() {
                    self.navigate_to_event(newest);
                }
            }
            _ => unreachable!("list navigation action"),
        }
    }

    /// Moves one displayed event; App loads older rows at the oldest loaded boundary.
    pub(super) fn move_event_selection(&mut self, forward: bool) {
        self.pending_click = None;
        // Moving on cancels a pending jump to the transcript's first event.
        self.event_list.start_requested = false;
        self.event_list.background_return = None;
        self.event_list.background = None;
        if forward && self.event_list.pending_older > 0 {
            self.event_list.pending_older -= 1;
            return;
        }
        let Some(index) = self.event_list.cursor else {
            self.show_newest_event();
            return;
        };
        if forward && index + 1 >= self.matching_event_count() {
            return;
        }
        if !forward {
            let first_timeline = self
                .shown_events()
                .next()
                .map(|(full, _)| EventRow::Timeline(full));
            if self.selected_event_row() == first_timeline
                && !self.transcript_fully_loaded()
                && self.event_list.has_older
            {
                self.event_list.pending_older += 1;
                return;
            }
            if index == 0 {
                return;
            }
        }
        let next = if forward {
            index.saturating_add(1)
        } else {
            index.saturating_sub(1)
        };
        self.event_list.pending_older = 0;
        self.navigate_to_event(next);
    }

    fn navigate_to_event(&mut self, index: usize) {
        let keep_tail = self.following_timeline();
        self.select_event(Some(index));
        self.event_list.pinned_following = keep_tail
            && self.event_list.sort == EventSort::Time
            && matches!(self.selected_event_row(), Some(EventRow::Pinned(_)));
        self.reveal_event_selection();
    }

    /// Selects the first loaded event and asks App to load the whole transcript.
    pub(super) fn show_first_event(&mut self) {
        self.event_list.background_return = None;
        self.event_list.pending_older = 0;
        self.event_list.start_requested = true;
        self.navigate_to_event(0);
    }

    pub(crate) fn finish_event_start(&mut self) {
        if std::mem::take(&mut self.event_list.start_requested) {
            self.select_event(Some(0));
            self.reveal_event_selection();
        }
    }

    pub(crate) fn event_page_pending(&self) -> bool {
        self.event_list.pending_older > 0
    }

    /// Saves identities before another task takes over the loaded transcript window.
    pub(super) fn park_event_list(&mut self) {
        if self.event_list.restoring {
            return;
        }
        let selected = self
            .event_list
            .cursor
            .and_then(|index| self.shown_event_rows().nth(index))
            .and_then(|(row, event)| {
                task_ui::EventId::from_event(event)
                    .map(|id| (matches!(row, EventRow::Pinned(_)), id))
            });
        let first = self
            .event_scroll()
            .min(self.matching_event_count().saturating_sub(1));
        let anchor = self.shown_event_rows().nth(first).and_then(|(row, event)| {
            task_ui::EventId::from_event(event).map(|id| (matches!(row, EventRow::Pinned(_)), id))
        });
        self.event_list.position = if self.following_event() {
            task_ui::EventPosition::Live
        } else if let (Some((pinned, anchor)), Some(_)) = (anchor, &selected) {
            task_ui::EventPosition::At { anchor, pinned }
        } else {
            task_ui::EventPosition::Live
        };
        self.event_list.selected = selected;
        self.event_list.tail = self
            .event_panel
            .last()
            .and_then(task_ui::EventId::from_event);
        self.event_list.cursor = None;
        if let Some(saved) = &mut self.event_list.background_return {
            saved.event = None;
        }
        self.event_list.scroll.set(0);
    }

    pub(crate) fn initialize_event_list(
        &mut self,
        sorting: EventSort,
        filter: crate::event::EventFilter,
    ) {
        if !self.event_list.settings_initialized {
            self.event_list.sort = sorting;
            self.event_list.filter = filter;
            self.event_list.settings_initialized = true;
        }
    }

    pub(crate) fn restoring_event_position(&self) -> bool {
        self.event_list.restoring
            && matches!(self.event_list.position, task_ui::EventPosition::At { .. })
    }

    pub(crate) fn restore_event_list(&mut self) {
        self.event_list.restoring = false;
        if self.event_list.pinned_following
            && self.event_list.sort == EventSort::Time
            && let Some((true, id)) = &self.event_list.selected
            && let Some(selected) = self.row_identity_position(true, id)
        {
            self.event_list.cursor = Some(selected);
            if let Some(saved) = &mut self.event_list.background_return {
                saved.event = Some(selected);
            }
            self.event_list.scroll.set(self.event_max_scroll());
            self.event_list.unseen.set(0);
            self.refresh_transcript();
            return;
        }
        self.event_list.pinned_following = false;
        let at = match &self.event_list.position {
            task_ui::EventPosition::Live => None,
            task_ui::EventPosition::At { anchor, pinned } => {
                self.row_identity_position(*pinned, anchor).zip(
                    self.event_list
                        .selected
                        .as_ref()
                        .and_then(|(pinned, id)| self.row_identity_position(*pinned, id)),
                )
            }
        };
        if let Some((scroll, selected)) = at {
            self.event_list.cursor = Some(selected);
            if let Some(saved) = &mut self.event_list.background_return {
                saved.event = Some(selected);
            }
            self.event_list.scroll.set(scroll);
            if self.event_list.sort == EventSort::Time
                && let Some(tail) = &self.event_list.tail
                && let Some(index) = self
                    .event_panel
                    .iter()
                    .position(|event| event.has_source(&tail.0))
            {
                self.event_list
                    .unseen
                    .set(self.event_list.unseen.get() + self.event_panel.len() - index - 1);
            }
        } else {
            self.event_list.position = task_ui::EventPosition::Live;
            self.event_list.selected = None;
            self.event_list.cursor = self.newest_event_position();
            self.event_list.scroll.set(self.event_max_scroll());
            self.event_list.unseen.set(0);
            self.event_list.pending_older = 0;
            self.event_list.start_requested = false;
            self.event_list.background_return = None;
            self.event_list.background = None;
        }
        self.refresh_transcript();
    }

    /// Applies structural changes without comparing row contents.
    pub(crate) fn apply_event_update(&mut self, update: crate::event::Update) {
        self.event_detail.transcript = None;
        if self.event_list.restoring {
            self.last_update = Utc::now();
            self.event_panel = update.events;
            self.event_list.cursor = None;
            self.prepare_event_search();
            self.restore_event_list();
            return;
        }
        let following = self.following_event();
        let timeline_following = self.following_timeline();
        let selected_index = self
            .event_list
            .background_return
            .as_ref()
            .and_then(|saved| saved.event)
            .or(self.event_list.cursor);
        let selected_full = selected_index
            .and_then(|index| self.shown_event_rows().nth(index).map(|(full, _)| full));
        let pinned_selected = self
            .selected_event_row()
            .filter(|row| matches!(row, EventRow::Pinned(_)));
        let selected_full = selected_full.and_then(|row| match row {
            EventRow::Timeline(index) => Some(index),
            EventRow::Pinned(_) => None,
        });
        let reset_selected = selected_full.filter(|_| update.reset).and_then(|full| {
            let selected = &self.event_panel[full];
            update.events.iter().position(|event| {
                event.shares_source(selected)
                    || (selected.source.is_none()
                        && event.source.is_none()
                        && selected.calls.is_empty()
                        && event.calls.is_empty()
                        && selected.kind == event.kind
                        && selected.started_at == event.started_at)
            })
        });
        let selected_order = (self.event_list.sort == EventSort::Duration
            && self.selecting_list(text_selection::Panel::EventList))
        .then(|| {
            self.shown_events()
                .map(|(full, _)| full + update.prepended)
                .collect::<Vec<_>>()
        });
        let old_len = self.event_panel.len();
        let appended_start = update.events.len().saturating_sub(update.appended);
        self.last_update = Utc::now();
        self.event_panel = update.events;
        // Rematching below uses the explicitly shifted full-list position.
        self.event_list.cursor = None;
        self.prepare_event_search();
        if let Some(order) = selected_order
            && (update.reset || !self.shown_events().map(|(full, _)| full).eq(order))
        {
            // A continuous text range cannot preserve rows that become discontiguous.
            self.cancel_list_selection(text_selection::Panel::EventList);
        }
        let added = self
            .shown_events()
            .filter(|(full, _)| *full >= appended_start)
            .count();
        let prepended = self
            .shown_events()
            .filter(|(full, _)| *full < update.prepended)
            .count();
        let newest = self.newest_event_position();
        let reset_selected = reset_selected.and_then(|full| self.shown_position(full));
        self.event_list.cursor = if let Some(row) = pinned_selected {
            self.shown_row_position(row)
        } else if following {
            newest
        } else if update.reset {
            reset_selected.or(newest)
        } else {
            selected_full.and_then(|full| {
                let full = full.saturating_add(update.prepended);
                let full = if self.event_list.sort == EventSort::Time {
                    full.min(self.event_panel.len().saturating_sub(1))
                } else {
                    full
                };
                self.shown_position(full)
                    .or_else(|| {
                        (self.event_list.sort == EventSort::Time)
                            .then(|| self.shown_event_rows().position(|(row, _)| matches!(row, EventRow::Timeline(index) if index >= full)))
                            .flatten()
                    })
                    .or(newest)
            })
        };
        let shift = if self.event_list.sort == EventSort::Duration {
            self.event_list.cursor.unwrap_or(0) as isize - selected_index.unwrap_or(0) as isize
        } else {
            prepended as isize
        };
        if self.event_list.sort == EventSort::Duration {
            self.event_list.unseen.set(0);
        }
        if let Some(previous) = &mut self.event_list.background_return {
            previous.event = self.event_list.cursor;
        }
        self.cancel_event_detail_selection();
        self.viewport.borrow_mut().event_detail_layout = None;
        let max_scroll = self.event_max_scroll();
        if pinned_selected.is_some() && timeline_following && self.mouse_press.is_none() {
            self.event_list.scroll.set(max_scroll);
            self.event_list.unseen.set(0);
        } else if update.reset
            && pinned_selected.is_none()
            && (following || reset_selected.is_none())
        {
            self.event_list.scroll.set(max_scroll);
            self.event_list.unseen.set(0);
            self.event_detail.scroll.borrow_mut().offset = 0;
        } else if !following || self.mouse_press.is_some() {
            self.event_list
                .scroll
                .set(self.event_list.scroll.get().saturating_add_signed(shift));
            if self.event_list.sort == EventSort::Time && !update.reset {
                self.event_list
                    .unseen
                    .set(self.event_list.unseen.get() + added);
            }
            if self.event_list.sort == EventSort::Time {
                self.shift_list_selection(text_selection::Panel::EventList, prepended);
            }
        } else {
            self.event_list.scroll.set(max_scroll);
        }
        let pending = std::mem::take(&mut self.event_list.pending_older);
        if pending > 0
            && self.event_panel.len() > old_len
            && !update.reset
            && let Some(index) = self.event_list.cursor
        {
            self.select_event(Some(index.saturating_sub(pending)));
        }
        self.refresh_transcript();
    }

    #[cfg(test)]
    pub(crate) fn set_event(&mut self, events: Vec<Event>) {
        let appended = events.len().saturating_sub(self.event_panel.len());
        self.apply_event_update(crate::event::Update {
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
        let layout = viewport.event_detail_layout.as_ref()?;
        layout
            .running
            .iter()
            .any(|row| layout.visible.contains(row))
            .then(|| super::tool_detail::running_redraw_in(now))
    }

    /// How long until a visible row's time changes, or `None` if it never does.
    pub(crate) fn event_redraw_in(&self, now: DateTime<Utc>) -> Option<std::time::Duration> {
        let session = self.selected_row()?;
        if self.event_session.as_ref() != Some(&session.id) {
            return None;
        }
        if self.event_list.sort == EventSort::Duration {
            let height = self.event_timeline_height();
            return self
                .shown_events()
                .skip(self.event_scroll())
                .take(height)
                .filter_map(|(index, event)| {
                    mode_row_time(
                        event,
                        session,
                        index + 1 == self.event_panel.len(),
                        self.event_list.sort,
                    )
                })
                .filter_map(|time| row_time_changes_in(&time, now))
                .min();
        }
        let event = self.shown_events().last()?.1;
        if self.viewport.borrow().event_panel.height > 0
            && self.event_scroll() < self.event_max_scroll()
        {
            return None;
        }
        row_time_changes_in(&row_time(event, session, true), now)
    }

    fn is_newest_shown(&self) -> bool {
        self.event_list.background.is_none()
            && if self.event_list.sort == EventSort::Duration {
                self.event_list
                    .cursor
                    .and_then(|index| self.shown_event_rows().nth(index))
                    .is_some_and(|(row, _)| matches!(row, EventRow::Timeline(full) if full + 1 == self.event_panel.len()))
            } else {
                self.event_list.cursor.is_some()
                    && self.event_list.cursor == self.newest_event_position()
            }
    }

    pub(super) fn shown_position(&self, full: usize) -> Option<usize> {
        self.shown_row_position(EventRow::Timeline(full))
    }

    pub(super) fn selected_event_row(&self) -> Option<EventRow> {
        self.event_list
            .cursor
            .and_then(|index| self.shown_event_rows().nth(index).map(|(row, _)| row))
    }
    pub(super) fn shown_row_position(&self, row: EventRow) -> Option<usize> {
        self.shown_event_rows()
            .position(|(candidate, _)| candidate == row)
    }
    fn row_identity_position(&self, pinned: bool, id: &task_ui::EventId) -> Option<usize> {
        self.shown_event_rows().position(|(row, event)| {
            matches!(row, EventRow::Pinned(_)) == pinned && event.has_source(&id.0)
        })
    }
    pub(super) fn shown_pinned_count(&self) -> usize {
        self.event_list
            .pinned
            .iter()
            .enumerate()
            .filter(|(index, _)| self.pinned_event_matches(*index))
            .count()
    }

    /// Heights within the list body, excluding the search input row.
    pub(super) fn event_section_heights(&self, visible: usize) -> (usize, usize) {
        let pinned = self.shown_pinned_count().min(visible.saturating_sub(2));
        let timeline = visible.saturating_sub(pinned + usize::from(pinned > 0));
        (pinned, timeline)
    }

    pub(super) fn event_content_height(&self) -> usize {
        usize::from(
            self.viewport
                .borrow()
                .event_panel
                .height
                .saturating_sub(u16::from(self.event_list.search.active())),
        )
    }

    fn event_pinned_height(&self) -> usize {
        self.event_section_heights(self.event_content_height()).0
    }

    pub(super) fn event_timeline_height(&self) -> usize {
        self.event_section_heights(self.event_content_height()).1
    }

    pub(super) fn shown_event_rows(&self) -> impl Iterator<Item = (EventRow, &Event)> {
        self.shown_events()
            .map(|(index, event)| (EventRow::Timeline(index), event))
            .chain(
                self.event_list
                    .pinned
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| self.pinned_event_matches(*index))
                    .map(|(index, row)| (EventRow::Pinned(index), &row.event)),
            )
    }

    pub(super) fn newest_event_position(&self) -> Option<usize> {
        self.shown_events().count().checked_sub(1)
    }

    pub fn set_pinned_events(&mut self, key: &SessionKey, rows: Vec<PinnedRow>) {
        if self.selected_id() == Some(key) {
            let selected = self.selected_event_row();
            self.event_list.pinned = rows;
            self.rematch_event_at(selected);
            self.event_list.cursor = selected
                .and_then(|row| self.shown_row_position(row))
                .or_else(|| self.newest_event_position());
            self.viewport.get_mut().event_detail_layout = None;
        } else {
            self.task_uis
                .entry(draft_controls::TaskKey::Session(key.clone()))
                .or_default()
                .event_list
                .pinned = rows;
        }
    }
    pub fn clear_pinned_events(&mut self, key: &SessionKey) {
        self.set_pinned_events(key, Vec::new());
    }
    pub fn has_pinned_events(&self, key: &SessionKey) -> bool {
        if self.selected_id() == Some(key) {
            !self.event_list.pinned.is_empty()
        } else {
            self.task_uis
                .get(&draft_controls::TaskKey::Session(key.clone()))
                .is_some_and(|ui| !ui.event_list.pinned.is_empty())
        }
    }
    pub(super) fn toggle_event_pin(&mut self) -> Option<Action> {
        let session = self.selected_row()?;
        if session.status == SessionStatus::Inactive {
            return Some(Action::Unsupported("Pin"));
        }
        let task = session.id.clone();
        let event = self.visible_event()?.clone();
        if event.source.as_deref().is_none_or(str::is_empty)
            || event.source_records.is_empty()
            || matches!(event.kind, Kind::QueuedUser | Kind::QueuedSystem)
        {
            return Some(Action::Unsupported("Pin"));
        }
        let selected = self.selected_event_row();
        let selected_position = self.event_list.cursor;
        let overlaps = |row: &PinnedRow| {
            row.pin
                .records
                .iter()
                .any(|record| event.source_records.contains(record))
        };
        if self.event_list.pinned.iter().any(overlaps) {
            self.event_list.pinned.retain(|row| !overlaps(row));
        } else {
            let pin = crate::state::PinnedEvent {
                source: event.source.clone().expect("validated source"),
                records: event.source_records.clone(),
            };
            let mut snapshot = event.clone();
            snapshot.turn = None;
            snapshot.previous_end = None;
            self.event_list.pinned.push(PinnedRow {
                pin,
                event: snapshot,
            });
        }
        self.rematch_event_at(selected);
        self.cancel_list_selection(text_selection::Panel::EventList);
        self.viewport.get_mut().event_detail_layout = None;
        let target = selected.and_then(|row| match row {
            EventRow::Timeline(_) => self.shown_row_position(row),
            EventRow::Pinned(_) => {
                let pins = self.shown_pinned_count();
                if pins > 0 {
                    selected_position.map(|index| index.min(self.matching_event_count() - 1))
                } else {
                    event.source.as_ref().and_then(|source| {
                        self.row_identity_position(false, &task_ui::EventId(source.clone()))
                    })
                }
            }
        });
        self.select_event(target);
        self.reveal_event_selection();
        Some(Action::ToggleEventPin {
            task,
            pinned: self
                .event_list
                .pinned
                .iter()
                .map(|row| row.pin.clone())
                .collect(),
        })
    }

    /// The timeline events the active filters let through, with their full index.
    pub(super) fn shown_events(&self) -> impl Iterator<Item = (usize, &Event)> {
        let order: Box<dyn Iterator<Item = usize> + '_> =
            if self.event_list.sort == EventSort::Duration {
                Box::new(self.event_order.iter().copied())
            } else {
                Box::new(0..self.event_panel.len())
            };
        order
            .filter(|&index| self.event_matches(index))
            .filter(|&index| {
                self.event_list
                    .filter
                    .includes(&self.event_panel[index].kind)
            })
            .map(|index| (index, &self.event_panel[index]))
    }
}

/// How an event row's time column reads.
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

pub(super) fn sort_duration(
    event: &Event,
    session: &Session,
    last_update: DateTime<Utc>,
    last: bool,
) -> Option<chrono::Duration> {
    match mode_row_time(event, session, last, EventSort::Duration)? {
        RowTime::Settled(duration) => Some(duration),
        RowTime::Running(start) => Some(last_update - start),
        RowTime::Clock(_) | RowTime::Ago(_) => unreachable!("duration rows have elapsed times"),
    }
}

fn mode_row_time(event: &Event, session: &Session, last: bool, sort: EventSort) -> Option<RowTime> {
    if sort == EventSort::Time {
        return Some(row_time(event, session, last));
    }
    match event.kind {
        _ if (event.kind == Kind::Assistant && event.completed_time)
            || matches!(event.kind, Kind::Tool | Kind::Error | Kind::Denied) =>
        {
            match row_time(event, session, last) {
                time @ (RowTime::Settled(_) | RowTime::Running(_)) => Some(time),
                RowTime::Clock(_) | RowTime::Ago(_) => None,
            }
        }
        Kind::User | Kind::Assistant => event
            .previous_end
            .map(|previous| RowTime::Settled(event.started_at - previous)),
        _ => None,
    }
}

fn row_time(event: &Event, session: &Session, last: bool) -> RowTime {
    if event.completed_time
        && matches!(
            event.kind,
            Kind::Assistant | Kind::Tool | Kind::Error | Kind::Denied
        )
        && let Some(end) = event.ended_at
    {
        return match event.duration {
            Some(duration) => RowTime::Settled(duration),
            None => RowTime::Clock(end),
        };
    }
    if event.completed_time && event.kind == Kind::Assistant {
        return RowTime::Running(event.started_at);
    }
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
        RowTime::Clock(at) => super::formatting::clock_text(at),
        RowTime::Settled(duration) => format_event_duration(duration),
        RowTime::Running(start) => format_event_duration(now - start),
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

fn event_time(
    event: &Event,
    session: &Session,
    last: bool,
    now: DateTime<Utc>,
    sort: EventSort,
) -> String {
    mode_row_time(event, session, last, sort)
        .map(|time| format_row_time(&time, now))
        .unwrap_or_default()
}

/// Centers a one-row label of `width` on the panel midline, clear of the clock column's gap.
#[cfg(test)]
pub(super) fn event_content_centered(area: Rect, width: u16) -> Rect {
    let (content, _) = event_row_areas(area);
    let x = super::centering::centered_x(area, content, width);
    Rect::new(x, area.y, width.min(content.right().saturating_sub(x)), 1).intersection(area)
}

/// Reserves the same trailing clock column and separator for every row.
pub(super) fn event_row_areas(area: Rect) -> (Rect, Rect) {
    let clock_right = area.right().saturating_sub(u16::from(area.width > 0));
    let time_width = (CLOCK_WIDTH as u16).min(area.width.saturating_sub(1));
    let content_width = area.width.saturating_sub(time_width + 3);
    (
        Rect::new(area.x, area.y, content_width, area.height),
        Rect::new(clock_right - time_width, area.y, time_width, area.height),
    )
}

/// Draws one event row and returns its visible links.
fn render_event(
    event: &Event,
    time: &str,
    last: bool,
    area: Rect,
    buf: &mut Buffer,
    search: Option<&super::event_search::EventSearch>,
    cwd: &std::path::Path,
) -> Vec<super::links::DrawnLink> {
    if area.is_empty() {
        return Vec::new();
    }
    let (content_area, time_area) = event_row_areas(area);
    let time_style = super::style::event_time_style(&event.kind, last);
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
                ..Default::default()
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

/// The open event laid out for one panel size and highlight theme.
pub(super) struct EventDetailLayout {
    columns: EventDetailColumns,
    height: usize,
    theme_generation: u64,
    lines: Vec<Line<'static>>,
    /// Animated output rows, updated without repeating syntax highlighting.
    running: Vec<usize>,
    /// Source rows drawn by the last render, after folding and window clipping.
    visible: Vec<usize>,
    /// Each line's text part and the cells mapping it back to that part's bytes.
    rows: Vec<(usize, Cells)>,
    continuations: std::collections::BTreeMap<usize, bool>,
    sources: Vec<text_selection::Source>,
    /// The links in each text part.
    links: Vec<Vec<super::links::Link>>,
    /// Section title times, drawn separately in the time column.
    times: Vec<(usize, Span<'static>)>,
    /// Each section's title row and its centered title.
    titles: Vec<(usize, Line<'static>)>,
    bodies: Vec<std::ops::Range<usize>>,
    body_keys: Vec<u64>,
    images: Vec<DetailImage>,
}

struct DetailImage {
    index: usize,
    image: crate::images::ImageSource,
    rows: std::ops::Range<usize>,
    x: u16,
    width: u16,
}

impl EventDetailLayout {
    fn cut_text(&self, row: usize) -> bool {
        !self.images.iter().any(|image| image.rows.contains(&row))
    }

    fn drawn_images(&self, visible: &[usize], area: Rect) -> Vec<super::event_detail::DrawnImage> {
        self.images
            .iter()
            .filter_map(|image| {
                let top = visible.iter().position(|row| image.rows.contains(row))?;
                let height = visible[top..]
                    .iter()
                    .take_while(|row| image.rows.contains(row))
                    .count() as u16;
                let image_area =
                    Rect::new(image.x, area.y + top as u16, image.width, height).intersection(area);
                (!image_area.is_empty()).then(|| super::event_detail::DrawnImage {
                    index: image.index,
                    image: image.image.clone(),
                    area: image_area,
                    first_row: (visible[top] - image.rows.start) as u16,
                })
            })
            .collect()
    }
}

#[cfg(test)]
fn event_detail_layout(
    event: &Event,
    width: u16,
    cwd: &std::path::Path,
    now: DateTime<Utc>,
) -> EventDetailLayout {
    let view = View::new(Grouping::Project, false);
    let panel = Rect::new(0, 0, width, 1);
    let list = view.center_layout(Rect::new(0, 0, width, 45)).event_panel;
    event_detail_parts_layout(
        event,
        event_detail_columns(panel, list),
        cwd,
        now,
        super::tool_detail::event_detail_content(event, String::new(), Style::new().dim()).sections,
    )
}

/// Clip a rendered row while keeping its ellipsis selectable as the hidden source suffix.
pub(super) fn truncate_event_detail_line(
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

fn event_detail_parts_layout(
    event: &Event,
    columns: EventDetailColumns,
    cwd: &Path,
    now: DateTime<Utc>,
    sections: Vec<Section>,
) -> EventDetailLayout {
    event_detail_parts_layout_with_images(event, columns, cwd, now, sections, |_, _| None)
}

/// Layout uses resolved cell sizes, independently of image decoding and terminal uploads.
fn event_detail_parts_layout_with_images(
    event: &Event,
    columns: EventDetailColumns,
    cwd: &Path,
    now: DateTime<Utc>,
    sections: Vec<Section>,
    mut image_size: impl FnMut(usize, &crate::images::ImageSource) -> Option<(u16, u16)>,
) -> EventDetailLayout {
    let width = usize::from(columns.text.width);
    let body_style = Style::default();
    let mut image_index = 0;
    let mut layout = EventDetailLayout {
        columns,
        height: 0,
        theme_generation: super::highlight::theme_generation(),
        lines: Vec::new(),
        running: Vec::new(),
        visible: Vec::new(),
        rows: Vec::new(),
        continuations: Default::default(),
        sources: Vec::new(),
        links: Vec::new(),
        times: Vec::new(),
        titles: Vec::new(),
        bodies: Vec::new(),
        body_keys: Vec::new(),
        images: Vec::new(),
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
            layout
                .titles
                .push((layout.lines.len(), title.title_in(width, cwd).0));
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
                    let reading_width = if part.reading_column() {
                        columns.reading_width.min(columns.text.width)
                    } else {
                        columns.text.width
                    };
                    let (mut lines, mut breaks) =
                        part.layout_in(usize::from(reading_width), body_style, now, cwd);
                    let image = part.image_block().and_then(|image| {
                        let index = image_index;
                        image_index += 1;
                        image_size(index, image).map(|size| (index, size))
                    });
                    if let Some((_, (_, height))) = image {
                        lines.resize(usize::from(height), Line::default());
                        breaks.resize(lines.len(), None);
                    }
                    (part, lines, breaks, image)
                })
                .collect();
            let group_width = rendered
                .iter()
                .map(|(_, lines, _, image)| {
                    image.map_or_else(
                        || lines.iter().map(Line::width).max().unwrap_or(0),
                        |(_, (width, _))| usize::from(width),
                    )
                })
                .max()
                .unwrap_or(0)
                .min(width);
            let group_x =
                super::centering::centered_x(columns.panel, columns.text, group_width as u16);
            let padding = usize::from(group_x - columns.text.x);
            for (part, lines, breaks, image) in rendered {
                let id = layout.sources.len();
                let mut maps = part.maps_in(&lines, cwd);
                let first = layout.lines.len();
                if let Some((index, (width, _))) = image {
                    layout.images.push(DetailImage {
                        index,
                        image: part
                            .image_block()
                            .expect("image size belongs to an image")
                            .clone(),
                        rows: first..first + lines.len(),
                        x: group_x,
                        width: width.min(columns.text.right().saturating_sub(group_x)),
                    });
                }
                layout.continuations.extend(
                    breaks
                        .into_iter()
                        .enumerate()
                        .filter_map(|(row, join)| join.map(|space| (first + row, space))),
                );
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
                    let mut line = truncate_event_detail_line(line, cells, width);
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
        layout.body_keys.push(event_detail_key(
            &layout.sources[source_start..]
                .iter()
                .map(|source| &source.text)
                .collect::<Vec<_>>(),
        ));
        layout.bodies.push(body);
    }
    layout
}

fn event_detail_key(value: &impl std::hash::Hash) -> u64 {
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
            || event.calls.iter().any(|call| call.show_event_detail))
}

/// The copied event text, including the titles separating merged tool calls.
pub(super) fn open_text(event: &Event) -> String {
    let detail = super::tool_detail::event_detail_content(event, String::new(), Style::new().dim());
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

/// The footer confirmation after copying an open event.
pub(super) fn copied_label(_event: &Event) -> String {
    "Copied message".into()
}

pub(super) fn kind_label(kind: &Kind) -> String {
    super::formatting::title_words(&format!("{kind:?}"))
}

use super::text::is_prose;

pub(super) fn body_line(event: &Event, body_width: usize, cwd: &std::path::Path) -> Line<'static> {
    fitted_body_line(event, body_width, cwd).line
}

/// An event's body as one event row, with paths shortened to fit the row.
pub(super) struct FittedBody {
    pub(super) line: Line<'static>,
    /// The text the row was drawn from, before any shortening: the event's prose,
    /// or its tool text with home paths shown as `~`.
    source: String,
    /// Byte ranges of the tool paths inside `source`.
    paths: Vec<std::ops::Range<usize>>,
}

fn fitted_body_line(event: &Event, body_width: usize, cwd: &std::path::Path) -> FittedBody {
    let activity = event.activity();
    summary_line(
        &activity.label,
        &activity.kind,
        activity.exit_code,
        &activity.paths,
        body_width,
        cwd,
    )
}

/// One summary layout for event rows, latest activity and subagent boxes.
pub(super) fn summary_line(
    label: &str,
    kind: &Kind,
    exit_code: Option<i64>,
    paths: &[std::ops::Range<usize>],
    body_width: usize,
    cwd: &std::path::Path,
) -> FittedBody {
    let body_style = super::style::event_body_style(kind, exit_code);
    if is_prose(kind) {
        let body = label.to_owned();
        let line = super::text::summary(&body, cwd, body_width, kind, None);
        FittedBody {
            line,
            source: body,
            paths: Vec::new(),
        }
    } else {
        let (text, paths) = display_paths(label, paths, cwd);
        let shortened = super::text::summary(&text, cwd, body_width, kind, Some(&paths));
        FittedBody {
            line: super::style::exit_code_line(shortened.to_string(), exit_code, body_style),
            source: text,
            paths,
        }
    }
}

/// An event's body with whitespace runs collapsed, and the byte ranges of its
/// paths in that text.
fn display_paths(
    label: &str,
    paths: &[std::ops::Range<usize>],
    cwd: &std::path::Path,
) -> (String, Vec<std::ops::Range<usize>>) {
    let mut text = String::new();
    let mut ranges = Vec::new();
    let mut end = 0;
    for range in &summary_paths(label, paths, cwd) {
        text.push_str(&collapse_whitespace(&label[end..range.start]));
        let path = &label[range.clone()];
        ranges.push(text.len()..text.len() + path.len());
        text.push_str(path);
        end = range.end;
    }
    text.push_str(&collapse_whitespace(&label[end..]));
    let trimmed = text.len() - text.trim_start().len();
    for range in &mut ranges {
        *range = range.start - trimmed..range.end - trimmed;
    }
    (text.trim().to_owned(), ranges)
}

/// The paths in a tool event's body: the ones its provider marked, and any
/// other path its free text mentions.
fn tool_paths(event: &Event, cwd: &std::path::Path) -> Vec<std::ops::Range<usize>> {
    summary_paths(&event.body, &event.paths, cwd)
}

pub(super) fn summary_paths(
    label: &str,
    paths: &[std::ops::Range<usize>],
    cwd: &std::path::Path,
) -> Vec<std::ops::Range<usize>> {
    let mut paths = paths.to_vec();
    for range in super::paths::text_paths(label, cwd) {
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

fn highlight_matches(line: &mut Line<'static>, search: &super::event_search::EventSearch) {
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
pub(super) mod event_markdown {
    use super::*;
    use ratatui::style::Modifier;

    #[test]
    fn queued_pasted_image_links_in_the_list_and_event_detail() {
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
        let events = crate::providers::claude::event_reader::tail(&transcript, 20).unwrap();
        let event = &events[0];
        let image = &event.images[0];
        let crate::images::ImageSource::File {
            path: image_path, ..
        } = &image.source
        else {
            panic!("expected file source")
        };
        let layout = event_detail_layout(event, 200, dir.path(), DateTime::UNIX_EPOCH);
        assert!(
            layout
                .links
                .iter()
                .flatten()
                .any(|link| link.target == *image_path)
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
        assert!(links.iter().any(|(_, target)| target == image_path));
    }

    #[test]
    fn task_notifications_show_output_content_without_repeating_summary() {
        let dir = tempfile::tempdir().unwrap();
        let events = notification_events(dir.path(), Some("all 3 tests passed"));
        assert_eq!(events.len(), 1);
        for event in events {
            let summary = "等 tab 线程 ✓";
            assert_eq!(event.kind, Kind::Tool);
            assert_eq!(event.body, summary);
            assert_eq!(event.calls[0].summary, summary);
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
            let layout = event_detail_layout(&event, 120, dir.path(), DateTime::UNIX_EPOCH);
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
        crate::providers::claude::event_reader::tail(&transcript, 10).unwrap()
    }

    #[test]
    fn tool_without_calls_has_no_body_below_its_title() {
        let event = &Event {
            turn: None,
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
        let detail =
            super::tool_detail::event_detail_content(event, String::new(), Style::new().dim());
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
            event_detail_layout(event, 100, std::path::Path::new("/"), DateTime::UNIX_EPOCH)
                .lines
                .len(),
            2
        );
    }

    #[test]
    fn running_times_under_a_millisecond_are_blank() {
        let now = Utc::now();
        for nanos in [-1, 0, 499_999, 500_000] {
            let start = now - chrono::Duration::nanoseconds(nanos);
            assert_eq!(
                format_row_time(&RowTime::Running(start), now),
                if nanos < 500_000 { "" } else { "1ms" }
            );
        }
    }

    fn body_buffer(kind: Kind, body: &str, width: u16) -> Buffer {
        let area = Rect::new(0, 0, width + 10, 1);
        let mut buffer = Buffer::empty(area);
        let event = Event {
            turn: None,
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
    fn event_list_short_row_centers_on_panel_midline() {
        let body = "x".repeat(20);
        let buffer = body_buffer(Kind::Tool, &body, 90);
        let start = body_padding(&buffer);
        assert_eq!(start + body.width() / 2, usize::from(buffer.area.width / 2));
        assert_eq!(body_text(&buffer).trim(), body);
    }

    #[test]
    fn event_list_near_full_row_moves_left_to_time_gap_without_truncation() {
        let body = "x".repeat(86);
        let buffer = body_buffer(Kind::Tool, &body, 90);
        let (span, time) = event_row_areas(buffer.area);
        assert_eq!(
            body_padding(&buffer) + body.width(),
            usize::from(span.right())
        );
        assert_eq!(body_text(&buffer).trim(), body);
        assert_eq!(buffer[(span.right(), 0)].symbol(), " ");
        assert_eq!(buffer[(time.x - 1, 0)].symbol(), " ");
    }

    #[test]
    fn event_list_jump_centers_on_panel_midline_and_stays_before_time_gap() {
        let panel = Rect::new(10, 3, 100, 1);
        for label in [" ↓ 1 New ", " ↓ 12 New "] {
            let width = label.width() as u16;
            let jump = event_content_centered(panel, width);
            assert_eq!(jump.x + width / 2, panel.x + panel.width / 2);
            assert_eq!(jump.width, width);
        }
        assert_eq!(event_content_centered(panel, 86), Rect::new(13, 3, 86, 1));
        assert_eq!(event_content_centered(panel, 120), event_row_areas(panel).0);
        assert_eq!(event_content_centered(Rect::new(10, 3, 8, 1), 10).width, 0);
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
        let events =
            crate::providers::Provider::event_panel(&provider, &session, 100, false).unwrap();
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
        let time = event_time(
            event,
            &session,
            true,
            event.ended_at.unwrap(),
            EventSort::Time,
        );
        assert_eq!(time, "370ms");
        let area = Rect::new(0, 0, 40, 1);
        let mut buffer = Buffer::empty(area);
        render_event(event, &time, true, area, &mut buffer, None, &session.cwd);
        let column: String = (31..39).map(|x| buffer[(x, 0)].symbol()).collect();
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
        let events = crate::providers::claude::event_reader::tail(file.path(), 8).unwrap();
        let event = events.last().unwrap();
        assert_eq!(event.kind, Kind::Tool);
        assert!(event.ended_at.is_none());
        let previous = event.previous_end.unwrap();
        session.last_record_at = Some(previous + chrono::Duration::seconds(30));
        let now = previous + chrono::Duration::seconds(120);
        assert_eq!(
            event_time(event, &session, true, now, EventSort::Time),
            "2m 0s"
        );
        session.status = SessionStatus::Ready;
        assert_eq!(
            event_time(event, &session, true, now, EventSort::Time),
            "30s"
        );
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
        let events =
            crate::providers::Provider::event_panel(&provider, &session, 100, false).unwrap();
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
            let base = super::super::style::event_styles(&kind).1;
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
        for kind in [Kind::Tool, Kind::Session, Kind::Error, Kind::Denied] {
            let base = super::super::style::event_styles(&kind).1;
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
            assert_eq!(body_text(&buffer), "         Report: report.md 1. done");
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
                turn: None,
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
            let fitted = fitted_body_line(&event, 40, dir.path());
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
            assert_eq!(body_text(&buffer), "ok abcde…");
            for x in 3..9 {
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
        let events = crate::providers::claude::event_reader::tail(file.path(), 1).unwrap();
        let fitted = fitted_body_line(&events[0], width, std::path::Path::new("/project"));
        fitted
            .line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    #[test]
    fn merged_tool_row_preserves_the_left_path_first() {
        assert_eq!(
            merged_tool_row(70),
            "Edit /opt/work/agents/src/ui/tests.rs, Read /opt/work/…/tests.rs"
        );
    }

    #[test]
    fn merged_tool_row_shrinks_the_right_name_before_the_left_path() {
        assert_eq!(
            merged_tool_row(40),
            "Edit /opt/work/agents/…/tests.rs, Read …"
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
#[path = "event_reading_tests.rs"]
mod reading_tests;

#[cfg(test)]
mod panel_detail_tests {
    use super::super::event_detail::Group;
    use super::super::tool_detail::Part;
    use super::*;

    fn fixture_events(fixture: &str) -> Vec<Event> {
        let dir = tempfile::tempdir().unwrap();
        let transcript = dir.path().join("transcript.jsonl");
        std::fs::write(&transcript, fixture).unwrap();
        crate::providers::claude::event_reader::tail(&transcript, 200).unwrap()
    }

    fn fixture_event() -> Event {
        fixture_events(include_str!("../../tests/fixtures/transcript.jsonl"))
            .into_iter()
            .find(|event| event.kind == Kind::Assistant)
            .unwrap()
    }

    fn columns(width: u16) -> EventDetailColumns {
        event_detail_columns(Rect::new(10, 2, width, 20), Rect::new(0, 0, 50, 20))
    }

    fn layout_part(columns: EventDetailColumns, part: Part) -> EventDetailLayout {
        event_detail_parts_layout(
            &fixture_event(),
            columns,
            Path::new("/"),
            DateTime::UNIX_EPOCH,
            vec![Section {
                title: super::tool_detail::event_detail_content(
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

    fn image_section() -> (Event, Vec<Section>, crate::images::ImageSource) {
        let fixture = include_str!("../../tests/fixtures/claude-read-image.jsonl");
        let record: serde_json::Value =
            serde_json::from_str(fixture.lines().nth(1).unwrap()).unwrap();
        let pointer = "/message/content/0/content/0/source/data";
        assert!(record.pointer(pointer).unwrap().is_string());
        let image = crate::images::ImageSource::Record {
            record: crate::event::SourceRecord {
                path: Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("tests/fixtures/claude-read-image.jsonl"),
                offset: (fixture.lines().next().unwrap().len() + 1) as u64,
            },
            pointer: pointer.into(),
        };
        let event = fixture_event();
        let mut sections =
            super::tool_detail::event_detail_content(&event, String::new(), Style::new().dim())
                .sections;
        sections[0].groups = vec![Group {
            parts: vec![Part::image(crate::images::Image {
                label: "[image image/png]".into(),
                source: image.clone(),
            })],
        }];
        (event, sections, image)
    }

    #[test]
    fn inline_image_rows_budget_copy_and_clipping() {
        let (event, sections, image) = image_section();
        let layout = event_detail_parts_layout_with_images(
            &event,
            columns(100),
            Path::new("/"),
            DateTime::UNIX_EPOCH,
            sections,
            |_, _| Some((20, 8)),
        );
        let image_rows = layout.images[0].rows.clone();
        assert_eq!(image_rows.len(), 8);
        assert_eq!(
            layout.lines[image_rows.start].to_string().trim(),
            "[image image/png]"
        );
        for row in image_rows.start + 1..image_rows.end {
            assert_eq!(layout.lines[row].to_string().trim(), "");
            assert!(layout.rows[row].1.is_empty());
        }
        let source = &layout.sources[layout.rows[image_rows.start].0].text;
        assert_eq!(source.as_ref(), "[image image/png]");
        let mut scroll = SectionScroll::default();
        scroll.prepare(layout.body_keys.clone());
        let heights = vec![1; layout.lines.len()];
        let rows = scroll.layout(&heights, &layout.bodies, Some(&[3]), 0..0, 5);
        assert_eq!(scroll.hidden, [5]);
        let visible: Vec<_> = rows.kept.into_iter().skip(rows.scroll).take(5).collect();
        let area = Rect::new(10, 4, 80, 5);
        let drawn = layout.drawn_images(&visible, area);
        assert_eq!(drawn.len(), 1);
        assert_eq!(drawn[0].index, 0);
        assert_eq!(drawn[0].image, image);
        assert_eq!(drawn[0].area.height, 3);
        assert_eq!(drawn[0].area.y, 6);
        assert_eq!(drawn[0].area.width, 20);
        assert_eq!(drawn[0].first_row, 0);
        assert!(!layout.cut_text(rows.cuts[0]));
        scroll.reveal[0] = 2;
        let rows = scroll.layout(&heights, &layout.bodies, Some(&[3]), 0..0, 5);
        let visible: Vec<_> = rows.kept.into_iter().skip(rows.scroll).take(5).collect();
        let drawn = layout.drawn_images(&visible, area);
        assert_eq!(drawn[0].first_row, 0);
        assert_eq!(drawn[0].area.height, 5);
        let drawn = layout.drawn_images(
            &(image_rows.start + 3..image_rows.end).collect::<Vec<_>>(),
            area,
        );
        assert_eq!(drawn[0].first_row, 3);
        assert_eq!(drawn[0].area.height, 5);
    }

    #[test]
    fn inline_image_render_records_visible_rows_and_clears_next_frame() {
        let (event, sections, image) = image_section();
        let view = View::new(Grouping::Project, false);
        let panel = Rect::new(0, 0, 100, 6);
        let mut buffer = Buffer::empty(Rect::new(0, 0, 101, 6));
        let columns = event_detail_columns(panel, view.center_layout(buffer.area).event_panel);
        let mut layout = event_detail_parts_layout_with_images(
            &event,
            columns,
            Path::new("/"),
            DateTime::UNIX_EPOCH,
            sections,
            |_, _| Some((4, 8)),
        );
        layout.height = 5;
        view.viewport.borrow_mut().event_detail_layout = Some(layout);
        *view.inline_images.borrow_mut() =
            Some(super::super::inline_images::InlineImages::new(None));
        view.render_event_detail_content(
            &event,
            "12:34:56",
            panel,
            &mut buffer,
            Path::new("/"),
            DateTime::UNIX_EPOCH,
        );
        {
            let viewport = view.viewport.borrow();
            let drawn = &viewport.event_detail_images;
            assert_eq!(drawn.len(), 1);
            assert_eq!(drawn[0].index, 0);
            assert_eq!(drawn[0].image, image);
            assert_eq!(drawn[0].first_row, 0);
            assert_eq!(drawn[0].area.height, 3);
            assert_eq!(drawn[0].area.width, 4);
            let last = drawn[0].area.bottom() - 1;
            assert!(
                (columns.text.x..columns.text.right()).all(|x| buffer[(x, last)].symbol() != "…")
            );
            assert!(!buffer.content.iter().any(|cell| cell.symbol() == "["));
            assert_eq!(
                viewport.event_text.sources[1].text.as_ref(),
                "[image image/png]"
            );
        }
        view.render_event_detail(Rect::default(), &mut buffer, DateTime::UNIX_EPOCH);
        assert!(view.viewport.borrow().event_detail_images.is_empty());
        // Also discard hit regions when the whole frame omits the detail pane.
        view.viewport.borrow_mut().event_detail_images.push(
            super::super::event_detail::DrawnImage {
                index: 0,
                image,
                area: panel,
                first_row: 0,
            },
        );
        view.render_at(buffer.area, &mut buffer, DateTime::UNIX_EPOCH);
        assert!(view.viewport.borrow().event_detail_images.is_empty());
    }

    #[test]
    fn inline_image_zoom_resets_when_switching_events() {
        let (mut first, _, _) = image_section();
        first.source = Some("image-first".into());
        let mut second = first.clone();
        second.source = Some("image-second".into());
        let view = View::new(Grouping::Project, false);
        let panel = Rect::new(0, 0, 100, 12);
        let mut buffer = Buffer::empty(Rect::new(0, 0, 101, 12));
        let render = |event: &Event, buffer: &mut Buffer| {
            view.viewport.borrow_mut().event_detail_layout = None;
            view.render_event_detail_content(
                event,
                "12:34:56",
                panel,
                buffer,
                Path::new("/"),
                DateTime::UNIX_EPOCH,
            );
        };
        render(&first, &mut buffer);
        let zoom = super::super::inline_images::Zoom {
            level: 2,
            left: 0.25,
        };
        view.event_detail.scroll.borrow_mut().zooms.insert(0, zoom);
        render(&second, &mut buffer);
        assert!(view.event_detail.scroll.borrow().zooms.is_empty());
        let second_zoom = super::super::inline_images::Zoom {
            level: 3,
            left: 0.5,
        };
        view.event_detail
            .scroll
            .borrow_mut()
            .zooms
            .insert(0, second_zoom);
        render(&first, &mut buffer);
        assert!(view.event_detail.scroll.borrow().zooms.is_empty());
        render(&second, &mut buffer);
        assert!(view.event_detail.scroll.borrow().zooms.is_empty());
    }

    #[test]
    fn inline_image_order_includes_unavailable_images() {
        let (event, mut sections, image) = image_section();
        sections[0].groups[0]
            .parts
            .push(Part::image(crate::images::Image {
                label: "[image image/png]".into(),
                source: image,
            }));
        let layout = event_detail_parts_layout_with_images(
            &event,
            columns(100),
            Path::new("/"),
            DateTime::UNIX_EPOCH,
            sections,
            |index, _| (index == 1).then_some((20, 4)),
        );
        assert_eq!(layout.images.len(), 1);
        assert_eq!(layout.images[0].index, 1);
        assert_eq!(layout.bodies[0].len(), 5);
    }

    #[test]
    fn inline_image_unavailable_keeps_placeholder() {
        let (event, sections, _) = image_section();
        let layout = event_detail_parts_layout_with_images(
            &event,
            columns(100),
            Path::new("/"),
            DateTime::UNIX_EPOCH,
            sections,
            |_, _| None,
        );
        assert!(layout.images.is_empty());
        assert_eq!(layout.bodies[0].len(), 1);
        assert_eq!(
            layout.lines.last().unwrap().to_string().trim(),
            "[image image/png]"
        );
    }

    #[test]
    fn each_section_title_has_a_display_only_gap_before_its_body() {
        let mut events =
            fixture_events(include_str!("../../tests/fixtures/claude-tool-calls.jsonl"));
        events.push(fixture_event());
        for event in events {
            let sections =
                super::tool_detail::event_detail_content(&event, String::new(), Style::new().dim())
                    .sections;
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
            let layout = event_detail_layout(&event, 200, Path::new("/"), DateTime::UNIX_EPOCH);
            assert_eq!(layout.lines.len(), layout.rows.len());
            for id in titles {
                let row = layout
                    .rows
                    .iter()
                    .enumerate()
                    .position(|(row, (source, _))| {
                        *source == id && !layout.lines[row].to_string().trim().is_empty()
                    })
                    .unwrap();
                assert_eq!(layout.lines[row + 1].to_string(), "");
                assert!(layout.rows[row + 1].1.is_empty());
                assert_eq!(layout.rows[row + 2].0, id + 1);
            }
        }
    }

    #[test]
    fn json_detail_keeps_its_body_without_an_event_list() {
        let columns = event_detail_columns(Rect::new(0, 0, 80, 12), Rect::default());
        assert_eq!(columns.reading_width, 0);
        assert!(columns.text.width > 0);
        let raw = include_str!("../../tests/fixtures/codex-manager-await.jsonl");
        let part = Part::text(raw.into(), false);
        let expected = part.lines(
            usize::from(columns.text.width),
            Style::default(),
            DateTime::UNIX_EPOCH,
        );
        let layout = layout_part(columns, part);
        assert_eq!(layout.lines.len(), expected.len() + 2);
        assert_eq!(
            layout.lines[2..]
                .iter()
                .map(|line| line.to_string().trim_start().to_owned())
                .collect::<Vec<_>>(),
            expected
                .iter()
                .map(|line| line.to_string().trim_start().to_owned())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn section_titles_scroll_without_a_merged_event_header_or_rules() {
        let event = fixture_events(include_str!("../../tests/fixtures/claude-tool-calls.jsonl"))
            .into_iter()
            .find(|event| event.calls.len() > 1)
            .unwrap();
        let panel = Rect::new(0, 0, 200, 8);
        let mut buffer = Buffer::empty(Rect::new(0, 0, 201, 8));
        let view = View::new(Grouping::Project, false);
        view.render_event_detail_content(
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
            !row(&buffer, EVENT_TOP).contains("12:34:56"),
            "merged event time must not be drawn"
        );
        let viewport = view.viewport.borrow();
        let layout = viewport.event_detail_layout.as_ref().unwrap();
        assert!(
            layout.lines.len() > 8,
            "the detail must overflow the panel to scroll"
        );
        let mut expected = Buffer::empty(buffer.area);
        layout.lines[0].clone().render(
            super::super::render::row(layout.columns.text, EVENT_TOP, 1),
            &mut expected,
        );
        for x in layout.columns.text.x..layout.columns.text.right() {
            assert_eq!(
                buffer[(x, EVENT_TOP)].symbol(),
                expected[(x, EVENT_TOP)].symbol()
            );
        }
        assert!(
            layout
                .lines
                .iter()
                .all(|line| !line.to_string().contains('─'))
        );
        drop(viewport);
        view.event_detail.scroll.borrow_mut().offset = 1;
        let mut scrolled = Buffer::empty(buffer.area);
        view.render_event_detail_content(
            &event,
            "12:34:56",
            panel,
            &mut scrolled,
            Path::new("/"),
            DateTime::UNIX_EPOCH,
        );
        assert_ne!(row(&buffer, EVENT_TOP), row(&scrolled, EVENT_TOP));
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
            let layout = event_detail_parts_layout(
                &fixture_event(),
                columns,
                Path::new("/"),
                DateTime::UNIX_EPOCH,
                vec![Section {
                    title: super::tool_detail::event_detail_content(
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
            view.render_event_detail_content(
                &event,
                "12:34:56",
                panel,
                &mut buffer,
                Path::new("/"),
                DateTime::UNIX_EPOCH,
            );
            assert_eq!(
                (columns.time.x..columns.time.right())
                    .map(|x| buffer[(x, panel.y + EVENT_TOP)].symbol())
                    .collect::<String>(),
                "12:34:56"
            );
            for y in panel.y + EVENT_TOP + 1..panel.bottom() {
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
            view.render_event_detail_content(
                &event,
                "12:34:56",
                panel,
                &mut buffer,
                dir.path(),
                DateTime::UNIX_EPOCH,
            );
            let viewport = view.viewport.borrow();
            let layout = viewport.event_detail_layout.as_ref().unwrap();
            assert!(!layout.times.is_empty());
            assert_eq!(layout.columns.time.right(), panel.right() - 1);
            for (row, time) in &layout.times {
                let y = panel.y + EVENT_TOP + *row as u16;
                let start = layout.columns.time.right() - time.width() as u16;
                assert_eq!(
                    (start..layout.columns.time.right())
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
        event.calls[0].input = vec![crate::event::Section::Shell {
            prefix: 0,
            text: source.into(),
        }];
        for width in [200, 35] {
            let columns = columns(width);
            let layout = event_detail_parts_layout(
                &event,
                columns,
                Path::new("/"),
                DateTime::UNIX_EPOCH,
                super::super::tool_detail::event_detail_content(
                    &event,
                    String::new(),
                    Style::new().dim(),
                )
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
#[path = "event_detail_regression_tests.rs"]
mod event_detail_regression_tests;

#[cfg(test)]
#[path = "event_path_tests.rs"]
mod path_tests;

#[cfg(test)]
#[path = "turn_group_tests.rs"]
mod turn_group_tests;

#[cfg(test)]
#[path = "event_identity_tests.rs"]
mod event_identity_tests;
