use super::formatting::{format_duration, format_relative_time};
use super::line_truncation::truncate_line_with_ellipsis_if_overflow;
use super::*;
use crate::sessions::log::{Event, Kind};
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
        if self.log_session.as_ref() != Some(&session.id) {
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
        let matching: Vec<_> = self
            .log
            .iter()
            .enumerate()
            .filter(|(index, event)| {
                self.log_filter.matches(&event.kind) && self.log_matches(*index)
            })
            .map(|(_, event)| event)
            .collect();
        let end = matching.len().saturating_sub(scroll);
        let start = end.saturating_sub(usize::from(area.height.saturating_sub(2)));
        let events = &matching[start..end];
        for (index, event) in events.iter().enumerate() {
            let last = scroll == 0 && index + 1 == events.len();
            let time = event_time(event, session, last, now);
            render_event(
                event,
                &time,
                CLOCK_WIDTH,
                last,
                super::render::row(area, index as u16 + 1, 1),
                buf,
                Some(&self.log_search),
            );
        }
    }
}

fn event_time(event: &Event, session: &Session, last: bool, now: DateTime<Utc>) -> String {
    let clock = || {
        event
            .started_at
            .with_timezone(&Local)
            .format(CLOCK_FORMAT)
            .to_string()
    };
    if !matches!(event.kind, Kind::Tool | Kind::Error | Kind::Denied) {
        return if last {
            format_relative_time(now, Some(event.started_at))
        } else if event.kind == Kind::Model
            && let Some(reference) = event.previous_end
        {
            format_duration((event.started_at - reference).num_seconds().max(0))
        } else {
            clock()
        };
    }
    let Some(previous) = event.previous_end else {
        return clock();
    };
    let background = event.body.ends_with(" *");
    let end = match event.ended_at {
        Some(end) => end,
        None if !last || background => event.started_at,
        None if session.parent.is_some() && session.status == SessionStatus::Working => now,
        None if session.process.is_none() || session.status == SessionStatus::Inactive => {
            session.last_record_at.unwrap_or(event.started_at)
        }
        None => now,
    };
    format_duration((end - previous).num_seconds().max(0))
}

fn render_event(
    event: &Event,
    time: &str,
    time_width: usize,
    last: bool,
    area: Rect,
    buf: &mut Buffer,
    search: Option<&super::log_search::LogSearch>,
) {
    if area.is_empty() {
        return;
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
        return;
    }
    let content_x = area.x + time_width as u16 + 1;
    let label = match event.kind {
        Kind::User => "User",
        Kind::Model => "Model",
        Kind::Agent => "Agent",
        Kind::Session => "Session",
        Kind::Tool => "Tool",
        Kind::Error => "Error",
        Kind::Denied => "Denied",
        Kind::QueuedUser | Kind::QueuedSystem => "Queue",
    };
    let label = format!("{label:<7} ");
    let label_width = label.width().min(content_width);
    Line::from(Span::styled(label, label_style))
        .render(Rect::new(content_x, area.y, label_width as u16, 1), buf);
    let body_width = content_width.saturating_sub(label_width);
    let mut body = body_line(event, body_width);
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
}

pub(super) fn body_line(event: &Event, body_width: usize) -> Line<'static> {
    let body_style = super::style::log_styles(&event.kind).1;
    let body = event.body.split_whitespace().collect::<Vec<_>>().join(" ");
    if matches!(event.kind, Kind::User | Kind::Model | Kind::Agent) {
        super::markdown::render(&body, usize::MAX)
            .into_iter()
            .next()
            .map(|mut line| {
                for span in &mut line.spans {
                    span.style = body_style.patch(span.style);
                }
                line
            })
            .unwrap_or_else(|| Line::from(Span::styled(body, body_style)))
    } else {
        let body = if let Some(path) = &event.path {
            let path_width = body_width.saturating_sub(body.width() + 1);
            format!(
                "{body} {}",
                super::line_truncation::abbreviate_path(path, path_width)
            )
        } else {
            body
        };
        Line::from(Span::styled(body, body_style))
    }
}

#[cfg(test)]
mod log_markdown {
    use super::*;
    use ratatui::style::Modifier;

    fn body_buffer(kind: Kind, body: &str, width: u16) -> Buffer {
        let area = Rect::new(0, 0, width + 17, 1);
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
        buffer.content[17..]
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
        let mut store = crate::sessions::Store::new(dir.path().to_owned());
        let mut session = store.refresh().sessions.remove(0);
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
        let events = crate::sessions::log::tail(file.path(), 8).unwrap();
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
    fn prose_uses_markdown_and_preserves_base_and_inline_styles() {
        for kind in [Kind::User, Kind::Model, Kind::Agent] {
            let base = super::super::style::log_styles(&kind).1;
            let buffer = body_buffer(
                kind,
                "plain `code` **bold** [link](https://example.com)",
                80,
            );
            assert_eq!(body_text(&buffer), "plain code bold link");
            assert_eq!(buffer[(17, 0)].fg, base.fg.unwrap_or(Color::Reset));
            assert_eq!(buffer[(23, 0)].fg, Color::Cyan);
            assert_eq!(buffer[(28, 0)].fg, base.fg.unwrap_or(Color::Reset));
            assert!(buffer[(28, 0)].modifier.contains(Modifier::BOLD));
            assert_eq!(buffer[(33, 0)].fg, Color::Cyan);
            assert!(buffer[(33, 0)].modifier.contains(Modifier::UNDERLINED));
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
            assert_eq!(buffer[(17, 0)].fg, base.fg.unwrap_or(Color::Reset));
            assert!(!buffer[(17, 0)].modifier.contains(Modifier::BOLD));
        }
    }

    #[test]
    fn empty_prose_and_zero_body_width_are_safe() {
        for kind in [Kind::User, Kind::Model, Kind::Agent] {
            assert_eq!(body_text(&body_buffer(kind.clone(), " \n ", 20)), "");
            assert_eq!(body_text(&body_buffer(kind, "`code`", 0)), "");
        }
    }

    #[test]
    fn cut_inline_code_keeps_colour_through_the_ellipsis() {
        for kind in [Kind::User, Kind::Model, Kind::Agent] {
            let buffer = body_buffer(kind, "ok `abcdefghijklmnop`", 10);
            assert_eq!(body_text(&buffer), "ok abcdef…");
            for x in 20..27 {
                assert_eq!(buffer[(x, 0)].fg, Color::Cyan);
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
