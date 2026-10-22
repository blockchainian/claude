//! The open event as the detail panel lays it out: sections with title rows and
//! aligned groups of parts, drawn left of a time column at the panel's right edge.

use super::tool_detail::Part;
use ratatui::layout::Rect;
use ratatui::style::Style;

/// The detail panel's columns, computed from the panel alone exactly like an event list row's:
/// a time column at the panel's right edge after a two-cell gap, and the text area left of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct EventDetailColumns {
    /// The whole panel; centered text sits on its midline.
    pub panel: Rect,
    /// Every row's text area, from the left edge to the time column's gap. Title rows place
    /// their left and center segments here; body rows use it as the content column.
    pub text: Rect,
    pub time: Rect,
    /// The wrap width of free text: the event list's message column width.
    pub reading_width: u16,
}

/// Columns for a detail `panel` whose event list occupies `list`.
pub(super) fn event_detail_columns(panel: Rect, list: Rect) -> EventDetailColumns {
    let (text, time) = super::event_list::event_row_areas(panel);
    EventDetailColumns {
        panel,
        text,
        time,
        reading_width: super::event_list::event_row_areas(list).0.width,
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

/// An open event as a sequence of sections.
pub(super) struct EventDetailContent {
    pub sections: Vec<Section>,
}

/// Scroll state belongs to the task; panel geometry stays in Viewport.
#[derive(Default)]
pub(super) struct EventDetailScroll {
    pub restoring: bool,
    /// Lines of the selected event scrolled off the top.
    pub offset: usize,
    pub state: super::SectionScroll,
    pub key: Option<u64>,
    pub event: Option<super::task_ui::EventId>,
    /// The wheel direction that selected the entry, until its layout starts it at that end.
    pub entry: Option<bool>,
    /// The wheel direction the selected entry's title holds reports against, and how many remain.
    pub title_hold: Option<(bool, usize)>,
    /// An output refresh reached while the detail was at its bottom.
    pub follow_output: bool,
    pub saved: std::collections::HashMap<u64, super::SectionScroll>,
}

impl super::View {
    pub(crate) fn event_output_path(&self) -> Option<std::path::PathBuf> {
        if self.terminal_requested()
            || self.event_detail.mode == super::task_ui::EventDetailMode::Transcript
            || self.event_session.as_ref() != self.selected_id()
        {
            return None;
        }
        self.visible_event()?.calls.iter().find_map(|call| {
            call.output
                .as_ref()?
                .iter()
                .find_map(|section| match section {
                    crate::event::Section::OutputFile(path) => Some(path.clone()),
                    _ => None,
                })
        })
    }

    pub(crate) fn refresh_event_output(&mut self) {
        let mut detail = self.event_detail.scroll.borrow_mut();
        let state = &detail.state;
        detail.follow_output = state
            .fallback_max
            .map_or_else(|| state.reveal == state.hidden, |max| state.offset >= max);
        self.viewport.borrow_mut().event_detail_layout = None;
    }

    pub(super) fn restore_event_detail(&self) {
        let mut scroll = self.event_detail.scroll.borrow_mut();
        if !scroll.restoring {
            return;
        }
        let selected = self
            .visible_event()
            .and_then(super::task_ui::EventId::from_event);
        if selected.is_none() || selected != scroll.event {
            *scroll = EventDetailScroll::default();
        } else {
            scroll.restoring = false;
            if scroll.state.left_keys.is_none() {
                scroll.state.left_keys = Some(std::mem::take(&mut scroll.state.keys));
            }
        }
    }
}
