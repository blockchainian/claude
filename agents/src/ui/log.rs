use super::formatting::{format_duration, format_relative_time};
use super::line_truncation::truncate_line_with_ellipsis_if_overflow;
use super::*;
use crate::log::{Event, Kind};
use chrono::{DateTime, Local, Utc};

const CLOCK_FORMAT: &str = "%H:%M:%S";
const CLOCK_WIDTH: usize = 8;

pub(super) fn search_input_rect(area: Rect) -> Rect {
    super::render::row(area, area.height.saturating_sub(1), 1)
}

impl View {
    pub(super) fn render_log(&self, area: Rect, buf: &mut Buffer, now: DateTime<Utc>) {
        if area.is_empty() {
            return;
        }
        let Some(session) = self.selected_row() else {
            return;
        };
        super::render::line("─".repeat(usize::from(area.width)).dim(), area, buf);
        self.viewport.borrow_mut().log_rows.clear();
        if self.log_session.as_ref() != Some(&session.id) {
            return;
        }
        if let Some(event) = &self.log_open {
            self.render_open_event(event, area, buf);
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
        let matching: Vec<_> = self.shown_log_events().collect();
        let end = matching.len().saturating_sub(scroll);
        let start = end.saturating_sub(usize::from(area.height.saturating_sub(2)));
        let mut rows = Vec::new();
        for (index, &(log_index, event)) in matching[start..end].iter().rev().enumerate() {
            let last = scroll == 0 && index == 0;
            let time = event_time(event, session, last, now);
            let cut = render_event(
                event,
                &time,
                CLOCK_WIDTH,
                last,
                super::render::row(area, index as u16 + 1, 1),
                buf,
                Some(&self.log_search),
            );
            rows.push((log_index, cut));
        }
        self.viewport.borrow_mut().log_rows = rows;
    }

    /// Draws one log event whole: its time and kind, a blank row, then the full body.
    fn render_open_event(&self, event: &Event, area: Rect, buf: &mut Buffer) {
        let (label_style, body_style) = super::style::log_styles(&event.kind);
        let header = Line::from(vec![
            Span::styled(
                event
                    .started_at
                    .with_timezone(&Local)
                    .format(CLOCK_FORMAT)
                    .to_string(),
                Style::default(),
            )
            .dim(),
            " · ".dim(),
            Span::styled(kind_label(&event.kind), label_style),
        ]);
        super::render::line(header, super::render::row(area, 1, 1), buf);
        let body = open_text(event);
        let options = super::line_truncation::prose_wrap_options(usize::from(area.width));
        let mut lines = if is_prose(&event.kind) {
            super::markdown::render_with_options(&body, options)
        } else {
            textwrap::wrap(&body, options)
                .into_iter()
                .map(|line| Line::from(line.into_owned()))
                .collect()
        };
        for line in &mut lines {
            for span in &mut line.spans {
                span.style = body_style.patch(span.style);
            }
        }
        let body_area = super::render::row(area, 3, area.height.saturating_sub(3));
        let mut viewport = self.viewport.borrow_mut();
        viewport.log_open_scroll = viewport
            .log_open_scroll
            .min(lines.len().saturating_sub(usize::from(body_area.height)));
        Paragraph::new(lines.split_off(viewport.log_open_scroll)).render(body_area, buf);
    }

    /// How long until the top log row's time reads differently, or `None` if it never does.
    pub(crate) fn log_redraw_in(&self, now: DateTime<Utc>) -> Option<std::time::Duration> {
        let session = self.selected_row()?;
        if self.log_scroll() != 0
            || self.log_open.is_some()
            || self.log_session.as_ref() != Some(&session.id)
        {
            return None;
        }
        let (_, event) = self.shown_log_events().last()?;
        row_time_changes_in(&row_time(event, session, true), now)
    }

    /// The log events the filter and search let through, with their index in the log.
    fn shown_log_events(&self) -> impl Iterator<Item = (usize, &Event)> {
        self.log.iter().enumerate().filter(|(index, event)| {
            self.log_filter.matches(&event.kind) && self.log_matches(*index)
        })
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
        return RowTime::Settled((end - event.started_at).num_seconds().max(0));
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

fn render_event(
    event: &Event,
    time: &str,
    time_width: usize,
    last: bool,
    area: Rect,
    buf: &mut Buffer,
    search: Option<&super::log_search::LogSearch>,
) -> bool {
    if area.is_empty() {
        return false;
    }
    let width = usize::from(area.width);
    let time_width = time_width.min(width);
    let time_area = Rect::new(area.x, area.y, time_width as u16, 1);
    let (label_style, _) = super::style::log_styles(&event.kind);
    let time_style = super::style::log_time_style(&event.kind, last);
    Line::from(Span::styled(time.to_owned(), time_style))
        .right_aligned()
        .render(time_area, buf);
    let content_width = width.saturating_sub(time_width + 1);
    if content_width == 0 {
        return true;
    }
    let content_x = area.x + time_width as u16 + 1;
    let label = format!("{:<9} ", kind_label(&event.kind));
    let label_width = label.width().min(content_width);
    Line::from(Span::styled(label, label_style))
        .render(Rect::new(content_x, area.y, label_width as u16, 1), buf);
    let body_width = content_width.saturating_sub(label_width);
    let (mut body, shortened) = fitted_body_line(event, body_width);
    let cut = shortened || body.width() > body_width;
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
    truncate_line_with_ellipsis_if_overflow(body, body_width).render(
        Rect::new(content_x + label_width as u16, area.y, body_width as u16, 1),
        buf,
    );
    cut
}

/// The full text an open log event shows below its header.
pub(super) fn open_text(event: &Event) -> String {
    match &event.path {
        Some(path) if !is_prose(&event.kind) => format!("{} {path}", event.body),
        _ => event.body.clone(),
    }
}

/// The footer confirmation after copying an open log event, such as "Copied user message".
pub(super) fn copied_label(event: &Event) -> String {
    format!("Copied {} message", kind_label(&event.kind).to_lowercase())
}

fn kind_label(kind: &Kind) -> &'static str {
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

pub(super) fn body_line(event: &Event, body_width: usize) -> Line<'static> {
    fitted_body_line(event, body_width).0
}

/// An event's body as one log row, with paths shortened to fit `body_width`,
/// and whether any path was shortened.
fn fitted_body_line(event: &Event, body_width: usize) -> (Line<'static>, bool) {
    let body_style = super::style::log_styles(&event.kind).1;
    let body = event.body.split_whitespace().collect::<Vec<_>>().join(" ");
    if is_prose(&event.kind) {
        let shortened = super::line_truncation::abbreviate_paths_in_text(&body, body_width);
        let line = super::markdown::render(&shortened, usize::MAX)
            .into_iter()
            .next()
            .map(|mut line| {
                for span in &mut line.spans {
                    span.style = body_style.patch(span.style);
                }
                line
            })
            .unwrap_or_else(|| Line::from(Span::styled(shortened.clone(), body_style)));
        (line, shortened != body)
    } else if let Some(path) = &event.path {
        let path_width = body_width.saturating_sub(body.width() + 1);
        let shortened = super::line_truncation::abbreviate_path(path, path_width);
        let cut = shortened != *path;
        (
            Line::from(Span::styled(format!("{body} {shortened}"), body_style)),
            cut,
        )
    } else {
        (Line::from(Span::styled(body, body_style)), false)
    }
}

#[cfg(test)]
mod log_markdown {
    use super::*;
    use ratatui::style::Modifier;

    fn body_buffer(kind: Kind, body: &str, width: u16) -> Buffer {
        let area = Rect::new(0, 0, width + 19, 1);
        let mut buffer = Buffer::empty(area);
        let event = Event {
            kind,
            body: body.into(),
            path: None,
            started_at: Utc::now(),
            ended_at: None,
            previous_end: None,
        };
        render_event(&event, "12:00:00", 8, false, area, &mut buffer, None);
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
            assert_eq!(buffer[(35, 0)].fg, crate::theme::link_color());
            assert!(buffer[(35, 0)].modifier.contains(Modifier::UNDERLINED));
        }
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
