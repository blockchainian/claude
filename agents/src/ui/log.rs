use super::formatting::{format_duration, format_relative_time};
use super::line_truncation::truncate_line_with_ellipsis_if_overflow;
use super::*;
use crate::log::{Event, Kind};
use chrono::{DateTime, Local, Utc};

const CLOCK_FORMAT: &str = "%H:%M:%S";
const CLOCK_WIDTH: usize = 8;
const KIND_WIDTH: usize = 9;

/// Rows above the first event: the top rule and a blank row.
pub(super) const LOG_TOP: u16 = 2;

pub(super) fn search_input_rect(area: Rect) -> Rect {
    super::render::row(area, area.height.saturating_sub(1), 1)
}

impl View {
    pub(super) fn render_log(&self, area: Rect, buf: &mut Buffer, now: DateTime<Utc>) {
        self.viewport.borrow_mut().log_text = text_selection::TextRegion::default();
        if !self.selecting_list(text_selection::Panel::LogList) {
            self.viewport.borrow_mut().log_list_text = text_selection::TextRegion::default();
        }
        if area.is_empty() {
            return;
        }
        let Some(session) = self.selected_row() else {
            return;
        };
        super::render::line("─".repeat(usize::from(area.width)).dim(), area, buf);
        {
            let mut viewport = self.viewport.borrow_mut();
            viewport.log_rows.clear();
            viewport.log_jump = Rect::default();
            viewport.log_links.clear();
        }
        if self.log_session.as_ref() != Some(&session.id) {
            return;
        }
        if let Some(event) = &self.log_open {
            let time = event_time(event, session, self.is_newest_shown(event), now);
            self.render_open_event(event, &time, area, buf, &session.cwd);
            return;
        }
        if self.log_search.active() && area.height > 1 {
            let input = search_input_rect(area);
            if self.log_search.editing {
                super::render::render_input_row("Search › ", &self.log_search.query, input, buf);
            } else {
                Line::from(self.log_search.query.text().to_owned())
                    .dim()
                    .right_aligned()
                    .render(input, buf);
            }
        }
        let scroll = self.log_scroll();
        self.render_log_jump(area, scroll, buf);
        let matching: Vec<_> = self.shown_log_events().collect();
        let end = matching.len().saturating_sub(scroll);
        let start = end.saturating_sub(usize::from(area.height.saturating_sub(LOG_TOP + 1)));
        let mut rows = Vec::new();
        let mut drawn_links = Vec::new();
        for (index, &(log_index, event)) in matching[start..end].iter().rev().enumerate() {
            let last = scroll == 0 && index == 0;
            let time = event_time(event, session, last, now);
            let (cut, links) = render_event(
                event,
                &time,
                last,
                super::render::row(area, index as u16 + LOG_TOP, 1),
                buf,
                Some(&self.log_search),
                &session.cwd,
            );
            rows.push((log_index, cut));
            drawn_links.extend(links);
        }
        let text_area = super::render::row(area, LOG_TOP, rows.len() as u16);
        {
            let mut viewport = self.viewport.borrow_mut();
            viewport.log_rows = rows;
            viewport.log_links = drawn_links;
        }
        self.capture_list_text(text_selection::Panel::LogList, text_area, scroll, buf);
    }

    /// Draws the button that jumps back to the newest row on the rule, its arrow above the
    /// rows' bodies, while the log is scrolled off its newest row.
    fn render_log_jump(&self, area: Rect, scroll: usize, buf: &mut Buffer) {
        let mut viewport = self.viewport.borrow_mut();
        if scroll == 0 {
            viewport.log_unseen = 0;
            return;
        }
        let label = match viewport.log_unseen {
            0 => " ↑ Latest ".to_owned(),
            unseen => format!(" ↑ {unseen} New "),
        };
        let x = area.x + (CLOCK_WIDTH + KIND_WIDTH + 1) as u16;
        let jump = Rect::new(x, area.y, label.width() as u16, 1).intersection(area);
        Line::from(Span::styled(label, super::style::log_jump_style())).render(jump, buf);
        viewport.log_jump = jump;
    }

    /// Draws one log event whole: its list row's time and kind, a blank row, then the full body.
    fn render_open_event(
        &self,
        event: &Event,
        time: &str,
        area: Rect,
        buf: &mut Buffer,
        cwd: &std::path::Path,
    ) {
        let (label_style, _) = super::style::log_styles(&event.kind);
        let time_style = super::style::log_time_style(&event.kind, self.is_newest_shown(event));
        let mut header = vec![Span::styled(time.to_owned(), time_style), " ".into()];
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
                event.calls[0].title.clone()
            } else {
                event.body.clone()
            };
            header.push(Span::styled(title, label_style));
        }
        super::render::line(
            Line::from(header),
            super::render::row(area, LOG_TOP, 1),
            buf,
        );
        let mut body_area =
            super::render::row(area, LOG_TOP + 2, area.height.saturating_sub(LOG_TOP + 2));
        let height = usize::from(body_area.height);
        let mut viewport = self.viewport.borrow_mut();
        let mut layout = match viewport.log_open_layout.take() {
            Some(layout) if layout.width == area.width && layout.height == height => layout,
            _ => {
                let mut layout = open_layout(event, area.width, cwd);
                if layout.lines.len() > height && area.width > 1 {
                    layout = open_layout(event, area.width - 1, cwd);
                    layout.width = area.width;
                }
                layout.height = height;
                layout
            }
        };
        let overflow = layout.lines.len() > height && area.width > 1;
        if overflow {
            body_area.width -= 1;
        }
        for (source, links) in layout.sources.iter().zip(&mut layout.links) {
            super::links::add_commits(&source.text, cwd, links);
        }
        viewport.log_open_max = layout.lines.len().saturating_sub(height);
        viewport.log_open_scroll = viewport.log_open_scroll.min(viewport.log_open_max);
        let scroll = viewport.log_open_scroll;
        if overflow && height > 0 {
            let thumb = (height * height)
                .div_ceil(layout.lines.len())
                .max(1)
                .min(height);
            let top = scroll * (height - thumb) / viewport.log_open_max.max(1);
            for row in 0..height {
                buf[(body_area.right(), body_area.y + row as u16)].set_symbol(
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
        let visible = layout.lines
            [scroll.min(layout.lines.len())..(scroll + height).min(layout.lines.len())]
            .to_vec();
        viewport.log_open_layout = Some(layout);
        drop(viewport);
        Paragraph::new(visible).render(body_area, buf);
        self.highlight_text_selection(text_selection::Panel::Log, buf);
    }

    /// Opens `event` in the log panel, or closes the open event, dropping its cached layout.
    pub(super) fn set_log_open(&mut self, event: Option<Event>) {
        self.cancel_open_selection();
        self.log_open = event;
        self.viewport.borrow_mut().log_open_layout = None;
    }

    /// Replaces the task log. Off the newest row, or while a mouse button is held, the rows on
    /// screen stay put and the newer events count toward the jump button.
    pub(crate) fn set_log(&mut self, events: Vec<Event>) {
        let newest = self.left_log_newest.take().or_else(|| {
            self.shown_log_events()
                .last()
                .map(|(_, event)| event.started_at)
        });
        self.log = events;
        self.prepare_log_search();
        if let Some(open) = &self.log_open
            && let Some(current) = self.log.iter().find(|event| {
                if shows_calls(open) {
                    event
                        .calls
                        .iter()
                        .any(|call| open.calls.iter().any(|old| old.id == call.id))
                } else {
                    event.started_at == open.started_at && event.kind == open.kind
                }
            })
            && current != open
        {
            self.set_log_open(Some(current.clone()));
        }
        let following = self.log_scroll() == 0 && self.mouse_press.is_none();
        let Some(newest) = newest.filter(|_| !following) else {
            return;
        };
        let added = self
            .shown_log_events()
            .filter(|(_, event)| event.started_at > newest)
            .count();
        let mut viewport = self.viewport.borrow_mut();
        viewport.log_scroll += added;
        viewport.log_unseen += added;
        self.shift_list_selection(text_selection::Panel::LogList, added);
    }

    /// How long until the top log row's time reads differently, or `None` if it never does.
    pub(crate) fn log_redraw_in(&self, now: DateTime<Utc>) -> Option<std::time::Duration> {
        let session = self.selected_row()?;
        if self.log_session.as_ref() != Some(&session.id) {
            return None;
        }
        let event = match &self.log_open {
            Some(event) if self.is_newest_shown(event) => event,
            Some(_) => return None,
            None if self.log_scroll() != 0 => return None,
            None => self.shown_log_events().last()?.1,
        };
        row_time_changes_in(&row_time(event, session, true), now)
    }

    /// Whether `event` is the newest log event the search lets through.
    fn is_newest_shown(&self, event: &Event) -> bool {
        self.shown_log_events()
            .last()
            .is_some_and(|(_, newest)| newest.started_at == event.started_at)
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
    Settled(i64),
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
            RowTime::Settled((event.started_at - reference).num_seconds().max(0))
        } else {
            RowTime::Clock(event.started_at)
        };
    }
    if session.id.provider == crate::providers::ProviderId::Codex
        && event.kind == Kind::Tool
        && let Some(end) = event.ended_at
    {
        return match event.duration {
            Some(duration) => RowTime::Settled(duration.num_seconds().max(0)),
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
    RowTime::Settled((end - previous).num_seconds().max(0))
}

fn format_row_time(time: &RowTime, now: DateTime<Utc>) -> String {
    match *time {
        RowTime::Clock(at) => at.with_timezone(&Local).format(CLOCK_FORMAT).to_string(),
        RowTime::Settled(seconds) => format_duration(seconds),
        RowTime::Running(start) => format_duration((now - start).num_seconds().max(0)),
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

/// Draws one log row and returns whether its body was cut, with the links drawn on it.
fn render_event(
    event: &Event,
    time: &str,
    last: bool,
    area: Rect,
    buf: &mut Buffer,
    search: Option<&super::log_search::LogSearch>,
    cwd: &std::path::Path,
) -> (bool, Vec<super::links::DrawnLink>) {
    if area.is_empty() {
        return (false, Vec::new());
    }
    let width = usize::from(area.width);
    let time_width = CLOCK_WIDTH.min(width);
    let time_area = Rect::new(area.x, area.y, time_width as u16, 1);
    let (label_style, _) = super::style::log_styles(&event.kind);
    let time_style = super::style::log_time_style(&event.kind, last);
    Line::from(Span::styled(time.to_owned(), time_style))
        .right_aligned()
        .render(time_area, buf);
    let content_width = width.saturating_sub(time_width + 1);
    if content_width == 0 {
        return (true, Vec::new());
    }
    let content_x = area.x + time_width as u16 + 1;
    let label = format!("{:<KIND_WIDTH$} ", kind_label(&event.kind));
    let label_width = label.width().min(content_width);
    Line::from(Span::styled(label, label_style))
        .render(Rect::new(content_x, area.y, label_width as u16, 1), buf);
    let body_width = content_width.saturating_sub(label_width);
    let fitted = fitted_body_line(event, body_width, cwd);
    let mut body = fitted.line;
    let cut = fitted.shortened || body.width() > body_width;
    let body_x = content_x + label_width as u16;
    let links = {
        let links = if is_prose(&event.kind) {
            super::links::text_links(&fitted.source, cwd, &event.images)
        } else {
            super::links::path_links(&fitted.source, &fitted.paths, cwd)
        };
        let visible = if body.width() > body_width {
            body_width.saturating_sub(1)
        } else {
            body_width
        } as u16;
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
    if let Some(search) = search.filter(|search| !search.query.is_empty()) {
        let indices = search.indices(&body.to_string());
        let mut offset = 0u32;
        body.spans = body
            .spans
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
    truncate_line_with_ellipsis_if_overflow(body, body_width)
        .render(Rect::new(body_x, area.y, body_width as u16, 1), buf);
    (cut, links)
}

/// The screen cells of one drawn line, each with the source bytes it shows.
type Cells = Vec<(std::ops::Range<u16>, std::ops::Range<usize>)>;

/// The open log event laid out at one panel width, reused every frame until the event or
/// the width changes.
pub(super) struct OpenLayout {
    width: u16,
    height: usize,
    lines: Vec<Line<'static>>,
    /// Each line's text part and the cells mapping it back to that part's bytes.
    rows: Vec<(usize, Cells)>,
    sources: Vec<text_selection::Source>,
    /// The links in each text part.
    links: Vec<Vec<super::links::Link>>,
}

fn open_layout(event: &Event, width: u16, cwd: &std::path::Path) -> OpenLayout {
    let body_style = super::style::log_styles(&event.kind).1;
    let parts: Vec<_> = open_parts(event)
        .into_iter()
        .map(|part| {
            let markdown = part.markdown();
            let lines = part.lines(usize::from(width), body_style);
            let maps = part.maps(&lines);
            let links = if markdown || (!shows_calls(event) && is_prose(&event.kind)) {
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
            (part.source, lines, maps, links)
        })
        .collect();
    let mut layout = OpenLayout {
        width,
        height: 0,
        lines: Vec::new(),
        rows: Vec::new(),
        sources: Vec::new(),
        links: Vec::new(),
    };
    for (id, (source, lines, mut maps, links)) in parts.into_iter().enumerate() {
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
    matches!(event.kind, Kind::Tool | Kind::Error | Kind::Denied) && !event.calls.is_empty()
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
    } else if is_prose(&event.kind) {
        vec![super::tool_detail::Part::text(event.body.clone(), true)]
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
    matches!(kind, Kind::User | Kind::Assistant | Kind::Agent)
}

pub(super) fn body_line(event: &Event, body_width: usize, cwd: &std::path::Path) -> Line<'static> {
    fitted_body_line(event, body_width, cwd).line
}

/// An event's body as one log row, with paths shortened to fit the row.
struct FittedBody {
    line: Line<'static>,
    /// Whether any path was shortened.
    shortened: bool,
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
            shortened: shortened != body,
            source: body,
            paths: Vec::new(),
        }
    } else {
        let (text, paths) = display_paths(event, cwd);
        let shortened = super::line_truncation::abbreviate_paths(&text, &paths, body_width);
        FittedBody {
            shortened: shortened != collapse_whitespace(event.body.trim()),
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

#[cfg(test)]
mod log_markdown {
    use super::*;
    use ratatui::style::Modifier;

    fn body_buffer(kind: Kind, body: &str, width: u16) -> Buffer {
        let area = Rect::new(0, 0, width + 19, 1);
        let mut buffer = Buffer::empty(area);
        let event = Event {
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

    fn body_text(buffer: &Buffer) -> String {
        buffer.content[19..]
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>()
            .trim_end()
            .to_owned()
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
        assert_eq!(row_time_changes_in(&RowTime::Settled(5), now), None);
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
            assert_eq!(body_text(&buffer), "plain code bold link");
            assert_eq!(buffer[(19, 0)].fg, base.fg.unwrap_or(Color::Reset));
            assert_eq!(buffer[(25, 0)].fg, crate::theme::inline_code_color());
            assert_eq!(buffer[(30, 0)].fg, base.fg.unwrap_or(Color::Reset));
            assert!(buffer[(30, 0)].modifier.contains(Modifier::BOLD));
            assert_eq!(buffer[(35, 0)].fg, base.fg.unwrap_or(Color::Reset));
            assert!(!buffer[(35, 0)].modifier.contains(Modifier::UNDERLINED));
        }
    }

    #[test]
    fn prose_blocks_keep_their_own_styles_on_one_row() {
        let buffer = body_buffer(Kind::User, "# Title\n\nplain  text\n- item", 80);
        assert_eq!(body_text(&buffer), "Title plain text • item");
        assert!(buffer[(19, 0)].modifier.contains(Modifier::UNDERLINED));
        assert!(!buffer[(25, 0)].modifier.contains(Modifier::UNDERLINED));
        assert!(!buffer[(25, 0)].modifier.contains(Modifier::BOLD));
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
            assert_eq!(body_text(&buffer), "`code` **bold** [link](url)");
            assert_eq!(buffer[(19, 0)].fg, base.fg.unwrap_or(Color::Reset));
            assert!(!buffer[(19, 0)].modifier.contains(Modifier::BOLD));
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
            for x in 22..29 {
                assert_eq!(buffer[(x, 0)].fg, crate::theme::inline_code_color());
            }
        }
    }

    /// A row merging an Edit and a Read call, fitted to `width`.
    fn merged_tool_row(width: usize) -> (String, bool) {
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
        let (line, cut) = (fitted.line, fitted.shortened);
        let text = line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        (text, cut)
    }

    #[test]
    fn merged_tool_row_abbreviates_every_path() {
        assert_eq!(
            merged_tool_row(70),
            (
                "Edit /o…/w…/a…/s…/ui/tests.rs，Read /o…/w…/a…/s…/p…/codex/tests.rs".into(),
                true
            )
        );
    }

    #[test]
    fn merged_tool_row_keeps_every_file_name_whole() {
        assert_eq!(
            merged_tool_row(40).0,
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
