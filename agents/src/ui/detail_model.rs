//! The open log event as the detail panel lays it out: sections with title rows and
//! aligned groups of parts, drawn left of a time column at the panel's right edge.

use super::tool_detail::Part;
use ratatui::layout::Rect;
use ratatui::style::Style;

/// The detail panel's columns, computed from the panel alone exactly like a log list row's:
/// a time column at the panel's right edge after a two-cell gap, and the text area left of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct DetailColumns {
    /// The whole panel; centered text sits on its midline.
    pub panel: Rect,
    /// Every row's text area, from the left edge to the time column's gap. Title rows place
    /// their left and center segments here; body rows use it as the content column.
    pub text: Rect,
    pub time: Rect,
    /// The wrap width of free text: the log list's message column width.
    pub reading_width: u16,
}

/// Columns for a detail `panel` whose log list occupies `list`.
pub(super) fn detail_columns(panel: Rect, list: Rect) -> DetailColumns {
    let (text, time) = super::log::log_row_areas(panel);
    DetailColumns {
        panel,
        text,
        time,
        reading_width: super::log::log_row_areas(list).0.width,
    }
}

/// A title row: an optional left label, a title centered on the panel's midline, and an
/// optional time in the time column. Every section uses it.
pub(super) struct TitleRow {
    pub left: Option<String>,
    pub exit_code: Option<i64>,
    pub title: String,
    /// The style of the centered title.
    pub title_style: Style,
    pub time: Option<String>,
    /// The style of the left label and the time.
    pub meta_style: Style,
}

/// One message or tool call, with its title and aligned body groups.
pub(super) struct Section {
    pub title: TitleRow,
    pub groups: Vec<Group>,
}

/// Parts sharing one left edge, centered together on the panel's midline.
pub(super) struct Group {
    pub parts: Vec<Part>,
}

/// An open log event as a sequence of sections.
pub(super) struct Detail {
    pub sections: Vec<Section>,
}
