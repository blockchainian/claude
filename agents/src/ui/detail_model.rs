//! The open log event as the detail panel lays it out: a title row, then sections of
//! aligned groups of parts, drawn in a content column beside a time column.

use super::tool_detail::Part;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Line;

/// The detail panel's columns, sized like the log list's message and clock columns and
/// centered as a pair in the panel. Every row draws text only in `content`; `time` holds
/// only title-row times.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct DetailColumns {
    pub content: Rect,
    pub time: Rect,
}

impl DetailColumns {
    /// The column left of the content, where shell command brackets are drawn.
    pub fn gutter_x(&self) -> u16 {
        self.content.x.saturating_sub(1)
    }
}

/// Columns for a detail `panel` whose log list occupies `list`.
pub(super) fn detail_columns(panel: Rect, list: Rect) -> DetailColumns {
    let width = list.width.min(panel.width);
    let pair = Rect::new(
        panel.x + (panel.width - width) / 2,
        panel.y,
        width,
        panel.height,
    );
    let (content, time) = super::log::log_row_areas(pair);
    DetailColumns { content, time }
}

/// A title row: an optional left label and a centered title inside the content column,
/// and an optional time in the time column. Events and merged tool calls both use it.
pub(super) struct TitleRow {
    pub left: Option<String>,
    pub title: Line<'static>,
    /// Draw the title between dim rules instead of as plain text.
    pub rule: bool,
    pub time: Option<String>,
    /// The style of the left label and the time.
    pub meta_style: Style,
}

/// One message, or one tool call with its title row when the event merges several calls.
pub(super) struct Section {
    pub title: Option<TitleRow>,
    pub groups: Vec<Group>,
}

/// Parts sharing one left edge, centered together in the content column.
pub(super) struct Group {
    pub parts: Vec<Part>,
}

/// An open log event: its title row and its sections.
pub(super) struct Detail {
    pub title: TitleRow,
    pub sections: Vec<Section>,
}
