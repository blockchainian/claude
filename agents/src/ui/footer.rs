//! The footer row: key hints on the left, the scrolled-off section title in the middle and
//! feedback, such as a copy confirmation or an error, on the right. The medium and small layouts
//! have no middle, so the title takes the right whenever no feedback shows. Feedback too wide
//! for the right grows leftward over the middle and then the hints, and is clipped to the row.

use super::event_detail::EventDetailColumns;
use super::line_truncation::truncate_line_with_ellipsis_if_overflow;
use super::*;

/// A message for the footer's right end.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Feedback {
    Info(String),
    Error(String),
}

impl Feedback {
    pub(super) fn line(&self) -> Line<'static> {
        match self {
            Self::Info(text) => Line::from(text.clone().dim()),
            Self::Error(text) => Line::styled(text.clone(), style::error_style()),
        }
    }
}

impl View {
    /// The failure kept by the selected draft, question or prompt until it is retried, or the
    /// selected draft's creation while it runs.
    pub(super) fn selection_feedback(&self) -> Option<Feedback> {
        let text = match self.selected_draft() {
            Some(draft) if draft.working || draft.job.is_some() => {
                return Some(Feedback::Info("Creating task…".into()));
            }
            Some(draft) => draft.error.clone(),
            None => self
                .active_question()
                .and_then(|question| question.error())
                .or_else(|| self.input_box.failure())
                .map(str::to_owned),
        }?;
        Some(Feedback::Error(text))
    }
}

pub(super) struct Footer {
    pub hints: Line<'static>,
    pub title: Option<(Line<'static>, EventDetailColumns)>,
    pub feedback: Option<Line<'static>>,
}

impl Footer {
    pub(super) fn render(
        self,
        area: Rect,
        large: bool,
        buf: &mut Buffer,
    ) -> Vec<hints::HintTarget> {
        let mut keys = hints::action_targets(
            &self.hints,
            area,
            &[
                ("?", hints::HintAction::Help),
                ("Esc", hints::HintAction::CloseHelp),
            ],
        );
        let start = area
            .x
            .saturating_add(self.hints.width() as u16)
            .saturating_add(2);
        render::line(self.hints, area, buf);
        // Each part after the hints keeps two cells clear of the one before it.
        let mut end = area.right();
        if let Some(feedback) = self.feedback {
            let feedback =
                truncate_line_with_ellipsis_if_overflow(feedback, usize::from(area.width));
            let width = feedback.width() as u16;
            let x = area.right() - width;
            // Feedback over the hints clears the cells it covers and the gap before it.
            if x < start {
                let gap = x.saturating_sub(2).max(area.x);
                let visible = Rect::new(area.x, area.y, gap.saturating_sub(area.x), area.height);
                for target in &mut keys {
                    target.area = target.area.intersection(visible);
                }
                keys.retain(|target| !target.area.is_empty());
                for column in gap..x {
                    buf[(column, area.y)].reset();
                }
            }
            feedback.render(Rect::new(x, area.y, width, 1), buf);
            if !large {
                return keys;
            }
            end = x.saturating_sub(2);
        }
        if let Some((title, columns)) = self.title {
            let span = Rect::new(start, area.y, end.saturating_sub(start), 1);
            render_title(title, columns, span, large, buf);
        }
        keys
    }
}

/// Draws the title within `span`: on the event detail's midline when `centered` and that
/// fits, otherwise ending at the text column so the time column stays empty.
fn render_title(
    title: Line<'static>,
    columns: EventDetailColumns,
    span: Rect,
    centered: bool,
    buf: &mut Buffer,
) {
    let end = columns.text.right().min(span.right());
    if end <= span.x {
        return;
    }
    let title = truncate_line_with_ellipsis_if_overflow(
        Line::from(title.to_string()).dim(),
        usize::from(end - span.x),
    );
    let width = title.width() as u16;
    let midline = super::centering::centered_x(columns.panel, columns.text, width);
    let x = if centered && midline >= span.x && midline + width <= span.right() {
        midline
    } else {
        end - width
    };
    title.render(Rect::new(x, span.y, width, 1), buf);
}
