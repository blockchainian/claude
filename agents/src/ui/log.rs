use super::formatting::{format_duration, format_relative_time};
use super::line_truncation::truncate_line_with_ellipsis_if_overflow;
use super::*;
use crate::sessions::log::{Event, Kind};
use chrono::{DateTime, Local, Utc};

impl View {
    pub(super) fn render_log(&self, area: Rect, buf: &mut Buffer, now: DateTime<Utc>) {
        if area.is_empty() {
            return;
        }
        let Some(session) = self.selected_row() else {
            return;
        };
        super::render::line("─".repeat(usize::from(area.width)).dim(), area, buf);
        if self.log_session.as_deref() != Some(session.id.as_str()) {
            return;
        }
        let events = &self.log[self.log.len().saturating_sub(8)..];
        for (index, event) in events.iter().enumerate() {
            let last = index + 1 == events.len();
            let time = event_time(event, session, last, now);
            render_event(
                event,
                &time,
                last,
                super::render::row(area, index as u16 + 1, 1),
                buf,
            );
        }
    }
}

fn event_time(event: &Event, session: &Session, last: bool, now: DateTime<Utc>) -> String {
    let clock = || {
        event
            .started_at
            .with_timezone(&Local)
            .format("%H:%M:%S")
            .to_string()
    };
    if !matches!(event.kind, Kind::Tool | Kind::Error) {
        return if last {
            format_relative_time(now, Some(event.started_at))
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
        None if session.process.is_none() || session.status == SessionStatus::Inactive => {
            session.last_record_at.unwrap_or(event.started_at)
        }
        None => now,
    };
    format_duration((end - previous).num_seconds().max(0))
}

fn render_event(event: &Event, time: &str, last: bool, area: Rect, buf: &mut Buffer) {
    if area.is_empty() {
        return;
    }
    let width = usize::from(area.width);
    let time_width = time.width().min(width);
    let time_area = Rect::new(
        area.right() - time_width as u16,
        area.y,
        time_width as u16,
        1,
    );
    let (label_style, body_style) = super::style::log_styles(&event.kind);
    let time_style = if last {
        body_style.bold()
    } else {
        Style::default().dim()
    };
    Line::from(Span::styled(time.to_owned(), time_style))
        .right_aligned()
        .render(time_area, buf);
    let content_width = width.saturating_sub(time_width + 3);
    if content_width == 0 {
        return;
    }
    let label = match event.kind {
        Kind::User => "User",
        Kind::Model => "Model",
        Kind::Agent => "Agent",
        Kind::Session => "Session",
        Kind::Tool => "Tool",
        Kind::Error => "Error",
        Kind::QueuedUser | Kind::QueuedSystem => "Queue",
    };
    let label = format!("{label:>7} ");
    let label_width = label.width().min(content_width);
    Line::from(Span::styled(label, label_style))
        .render(Rect::new(area.x, area.y, label_width as u16, 1), buf);
    let body_width = content_width.saturating_sub(label_width);
    let body = event.body.split_whitespace().collect::<Vec<_>>().join(" ");
    truncate_line_with_ellipsis_if_overflow(Line::from(Span::styled(body, body_style)), body_width)
        .render(
            Rect::new(area.x + label_width as u16, area.y, body_width as u16, 1),
            buf,
        );
}
